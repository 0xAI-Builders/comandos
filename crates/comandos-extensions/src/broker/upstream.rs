//! Proceso upstream stdio compartido: lanzado con el mismo `command(spec, true)` que usa
//! `serve`, con stdin/stdout por tuberías y stderr a `/dev/null`, como el proxy directo y el
//! Python: los servidores escriben ahí tokens y URLs de autorización que no deben acabar en
//! el journal del daemon.
use super::{MAX_LINE, blank};
use crate::Result;
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use serde_json::Value;
use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Child,
    sync::mpsc,
};

/// Líneas encoladas hacia el stdin del upstream; al llenarse, el actor deja de leer clientes.
const STDIN_QUEUE: usize = 256;
/// Líneas del stdout del upstream pendientes de que el actor las procese.
const LINE_QUEUE: usize = 256;
/// Espera entre SIGTERM y SIGKILL al cerrar un upstream.
const GRACE: Duration = Duration::from_secs(5);

pub(super) struct Upstream {
    /// Escritor del stdin (una tarea); cada elemento es una línea sin `\n`.
    pub stdin: mpsc::Sender<Vec<u8>>,
    /// Líneas del stdout; `None` cuando el upstream cierra su salida.
    pub lines: mpsc::Receiver<Vec<u8>>,
    pub child: Child,
    pub pid: u32,
}

/// Primer archivo regular ejecutable de `name` en los directorios de `path`, como `execvp`.
fn find_in_path(name: &str, path: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|c| {
            std::fs::metadata(c).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// Entorno base del upstream, según lo que mandó la sesión en su `attach`.
pub(super) enum Base<'a> {
    /// Entorno completo de la sesión: sustituye al del daemon.
    Environ(&'a [(String, String)]),
    /// Protocolo anterior: solo el `PATH` de la sesión (si lo hay), sobre el entorno del daemon.
    Path(Option<&'a str>),
}

/// Lanza `spec` como lo habría lanzado el proxy directo de la sesión: en `cwd`, con el entorno
/// de la sesión (`Base::Environ`, sin nada del daemon) y su `env` expandido encima (el spec
/// gana). Un `command` sin `/` se resuelve en el `PATH` efectivo: el del spec o el de la sesión.
/// Con `Base::Path` el entorno es el del daemon con el `PATH` de la sesión.
pub(super) fn spawn(
    spec: &Value,
    cwd: &Path,
    env: &[(String, String)],
    base: Base<'_>,
) -> Result<Upstream> {
    let mut spec = spec.clone();
    if let Some(o) = spec.as_object_mut() {
        o.remove("cwd");
    }
    let mut env = env.to_vec();
    let session_path = match base {
        Base::Environ(environ) => environ.iter().find(|(k, _)| k == "PATH").map(|(_, v)| &**v),
        Base::Path(path) => {
            if let Some(path) = path
                && !env.iter().any(|(k, _)| k == "PATH")
            {
                env.push(("PATH".into(), path.into()));
            }
            path
        }
    };
    // PATH efectivo del upstream (el del spec gana): ahí se busca el ejecutable.
    let effective = env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| &**v)
        .or(session_path);
    let command = spec["command"].as_str().map(crate::expand_user);
    if let Some(effective) = effective
        && let Some(name) = command.filter(|c| !c.is_empty() && !c.contains('/'))
        && let Some(found) = find_in_path(&name, effective)
    {
        spec["command"] = Value::String(found.to_string_lossy().into_owned());
    }
    let mut std_command = crate::command_env(&spec, true, Some(&env))?;
    if let Base::Environ(environ) = base {
        // `env_clear` borra también el `env` del spec: se vuelve a poner encima del environ.
        std_command.env_clear();
        std_command.envs(environ.iter().map(|(k, v)| (k, v)));
        std_command.envs(env.iter().map(|(k, v)| (k, v)));
    }
    std_command
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut command = tokio::process::Command::from(std_command);
    // Grupo propio para que el cierre alcance a los nietos (npx → node). `kill_on_drop`
    // solo cubre al líder: es la red si el actor desaparece sin pasar por `terminate`.
    command.kill_on_drop(true).process_group(0);
    let mut child = command
        .spawn()
        .map_err(|_| "No se pudo lanzar el servidor")?;
    let pid = child.id().ok_or("El servidor terminó al arrancar")?;
    let mut stdin = child.stdin.take().ok_or("Servidor sin stdin")?;
    let stdout = child.stdout.take().ok_or("Servidor sin stdout")?;
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(STDIN_QUEUE);
    tokio::spawn(async move {
        while let Some(mut line) = rx.recv().await {
            line.push(b'\n');
            if stdin.write_all(&line).await.is_err() || stdin.flush().await.is_err() {
                break;
            }
        }
    });
    let (line_tx, lines) = mpsc::channel(LINE_QUEUE);
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        loop {
            let line = match read_upstream(&mut reader, &mut buf).await {
                Ok(Read::Line) if blank(&buf) => continue,
                Ok(Read::Line) => buf.clone(),
                Ok(Read::Oversized(size, reply)) => {
                    let to = if reply.is_some() {
                        "a su petición"
                    } else {
                        "sin destinatario"
                    };
                    eprintln!(
                        "broker: upstream {pid}: línea de {size} bytes descartada (más de {MAX_LINE}); error {to}: respuesta demasiado grande"
                    );
                    match reply {
                        Some(reply) => reply,
                        None => continue,
                    }
                }
                Ok(Read::Eof) => break,
                Err(e) => {
                    eprintln!("broker: upstream {pid}: salida ilegible ({e})");
                    break;
                }
            };
            if line_tx.send(line).await.is_err() {
                break;
            }
        }
    });
    Ok(Upstream {
        stdin: tx,
        lines,
        child,
        pid,
    })
}

/// Resultado de leer una línea del stdout del upstream.
#[derive(Debug, PartialEq)]
enum Read {
    Eof,
    /// Línea completa (sin `\n`) en el búfer.
    Line,
    /// Línea de más de [`MAX_LINE`] bytes (tamaño total), ya descartada; con la respuesta de
    /// error que sustituye a una respuesta JSON-RPC con `id`.
    Oversized(usize, Option<Vec<u8>>),
}

/// Como `super::read_line`, pero una línea mayor que [`MAX_LINE`] no es un error: deja de
/// guardarse, se consume hasta su `\n` mientras [`IdScan`] busca su `id` de primer nivel, y
/// se devuelve [`Read::Oversized`]. Así el upstream compartido sigue vivo.
async fn read_upstream<R: AsyncBufRead + Unpin>(r: &mut R, buf: &mut Vec<u8>) -> io::Result<Read> {
    buf.clear();
    let mut big: Option<(IdScan, usize)> = None;
    loop {
        let chunk = r.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(match big {
                Some((scan, size)) => Read::Oversized(size, scan.error_reply()),
                None if buf.is_empty() => Read::Eof,
                None => Read::Line,
            });
        }
        let (n, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        match &mut big {
            Some((scan, size)) => {
                scan.feed(&chunk[..n]);
                *size += n;
            }
            None => buf.extend_from_slice(&chunk[..n]),
        }
        r.consume(n);
        if let Some((scan, size)) = big.take_if(|_| done) {
            return Ok(Read::Oversized(size, scan.error_reply()));
        }
        if done {
            buf.pop();
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return Ok(Read::Line);
        }
        if big.is_none() && buf.len() > MAX_LINE {
            let mut scan = IdScan::default();
            scan.feed(buf);
            big = Some((scan, buf.len()));
            *buf = Vec::new(); // suelta los 256 MiB ya leídos
        }
    }
}

/// Lee un objeto JSON en streaming y recuerda su `id` de primer nivel (y si tiene `method`),
/// sin guardar el resto. Un `"id"` anidado (dentro de `result`) no cuenta.
#[derive(Default)]
struct IdScan {
    depth: u32,
    in_str: bool,
    escaped: bool,
    /// En el objeto de primer nivel, el siguiente valor de cadena es una clave.
    key_next: bool,
    key: Vec<u8>,
    capturing: bool,
    value: Vec<u8>,
    id: Option<Vec<u8>>,
    method: bool,
}

/// Bytes máximos de una clave o de un `id` de primer nivel que se recuerdan.
const SCAN_FIELD: usize = 256;

impl IdScan {
    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.byte(b);
        }
    }

    fn byte(&mut self, b: u8) {
        let top = self.depth == 1;
        if self.in_str {
            if self.capturing {
                self.push_value(b);
            } else if top && self.key_next && self.key.len() <= SCAN_FIELD {
                self.key.push(b);
            }
            if self.escaped {
                self.escaped = false;
            } else if b == b'\\' {
                self.escaped = true;
            } else if b == b'"' {
                self.in_str = false;
                if top && self.key_next && !self.capturing {
                    self.key.pop(); // comilla de cierre
                }
            }
            return;
        }
        match b {
            b'"' => {
                self.in_str = true;
                if self.capturing {
                    self.push_value(b);
                } else if top && self.key_next {
                    self.key.clear();
                }
            }
            b'{' | b'[' => {
                if self.capturing {
                    self.push_value(b);
                }
                self.depth += 1;
                if self.depth == 1 {
                    self.key_next = true;
                }
            }
            b'}' | b']' => {
                if top {
                    self.finish();
                } else if self.capturing {
                    self.push_value(b);
                }
                self.depth = self.depth.saturating_sub(1);
            }
            b':' if top => {
                self.key_next = false;
                match self.key.as_slice() {
                    b"id" if self.id.is_none() => {
                        self.capturing = true;
                        self.value.clear();
                    }
                    b"method" => self.method = true,
                    _ => {}
                }
            }
            b',' if top => {
                self.finish();
                self.key_next = true;
            }
            _ if self.capturing => self.push_value(b),
            _ => {}
        }
    }

    fn push_value(&mut self, b: u8) {
        if self.value.len() <= SCAN_FIELD {
            self.value.push(b);
        }
    }

    fn finish(&mut self) {
        if std::mem::take(&mut self.capturing) {
            self.id = Some(std::mem::take(&mut self.value));
        }
    }

    /// Error -32603 para el `id` de una respuesta (no de un request ni una notificación del
    /// upstream, que no tienen a quién avisar). El `Mux` lo traduce y lo entrega a su dueño.
    fn error_reply(&self) -> Option<Vec<u8>> {
        if self.method {
            return None;
        }
        let id: Value = serde_json::from_slice(self.id.as_deref()?).ok()?;
        if !(id.is_u64() || id.is_string()) {
            return None;
        }
        let reply = serde_json::json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":"respuesta demasiado grande"}});
        serde_json::to_vec(&reply).ok()
    }
}

/// SIGTERM al grupo, 5 s de gracia para el líder, SIGKILL; recoge al líder y al final manda
/// SIGKILL otra vez al grupo para los nietos que ignoren SIGTERM (ESRCH si ya no queda
/// nadie). El líder sin recoger reserva su pid como id de grupo, así que la primera señal
/// no puede alcanzar a un proceso ajeno.
pub(super) async fn terminate(up: &mut Upstream) -> String {
    let group = Pid::from_raw(up.pid as i32);
    let _ = killpg(group, Signal::SIGTERM);
    let status = match tokio::time::timeout(GRACE, up.child.wait()).await {
        Ok(status) => status,
        Err(_) => {
            let _ = killpg(group, Signal::SIGKILL);
            up.child.wait().await
        }
    };
    let _ = killpg(group, Signal::SIGKILL);
    status.map_or_else(|_| "desconocido".into(), |s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::IdScan;

    fn reply(line: &str) -> Option<String> {
        let mut scan = IdScan::default();
        // En dos trozos partidos a mitad de la clave: el estado sobrevive entre llamadas.
        let (a, b) = line.split_at(line.len() / 2);
        scan.feed(a.as_bytes());
        scan.feed(b.as_bytes());
        scan.error_reply().map(|r| String::from_utf8(r).unwrap())
    }

    #[test]
    fn finds_the_top_level_id_wherever_it_is() {
        let first = reply(r#"{"jsonrpc":"2.0","id":5,"result":{"id":9,"text":"x"}}"#).unwrap();
        assert!(first.contains(r#""id":5"#), "{first}");
        let last =
            reply(r#"{"result":{"content":[{"id":9,"t":"a\"id\":3"}]},"jsonrpc":"2.0","id":12}"#)
                .unwrap();
        assert!(last.contains(r#""id":12"#), "{last}");
        let string = reply(r#"{ "id" : "a\"b" , "result" : {} }"#).unwrap();
        assert!(string.contains(r#""id":"a\"b""#), "{string}");
        assert!(last.contains(r#""code":-32603"#) && last.contains("respuesta demasiado grande"));
    }

    #[test]
    fn requests_notifications_and_bad_ids_get_no_reply() {
        assert_eq!(
            reply(r#"{"jsonrpc":"2.0","id":3,"method":"sampling/createMessage","params":{}}"#),
            None
        );
        assert_eq!(
            reply(r#"{"jsonrpc":"2.0","method":"notifications/message","params":{"id":1}}"#),
            None
        );
        assert_eq!(reply(r#"{"jsonrpc":"2.0","result":{"id":1}}"#), None);
        assert_eq!(reply(r#"{"jsonrpc":"2.0","id":{"x":1},"result":{}}"#), None);
        assert_eq!(reply(r#"{"jsonrpc":"2.0","id":null,"result":{}}"#), None);
    }
}
