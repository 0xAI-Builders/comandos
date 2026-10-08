//! Proceso upstream stdio compartido: lanzado con el mismo `command(spec, true)` que usa
//! `serve`, con stdin/stdout por tuberías y stderr a `/dev/null`, como el proxy directo y el
//! Python: los servidores escriben ahí tokens y URLs de autorización que no deben acabar en
//! el journal del daemon.
use super::{MAX_LINE, blank, scan::IdScan};
use crate::Result;
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
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

/// Una única combinación para la clave de compartición y el entorno realmente lanzado.
/// Ninguna variable desconocida (credencial, perfil, ejecutable…) se descarta como ruido.
pub(super) fn effective_environment(
    overrides: &[(String, String)],
    base: Base<'_>,
) -> Vec<(String, String)> {
    let mut env: BTreeMap<String, String> = match base {
        Base::Environ(environ) => environ.iter().cloned().collect(),
        Base::Path(path) => {
            let mut env: BTreeMap<String, String> = std::env::vars_os()
                .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
                .collect();
            if let Some(path) = path {
                env.insert("PATH".into(), path.into());
            }
            env
        }
    };
    env.extend(overrides.iter().cloned());
    env.into_iter().collect()
}

/// El entorno ya está completamente resuelto por el registro y forma parte de su clave.
/// El adaptador HTTP puede añadir únicamente su snapshot privado después de esa clave.
pub(super) fn spawn(spec: &Value, cwd: &Path, env: &[(String, String)]) -> Result<Upstream> {
    let mut spec = spec.clone();
    if let Some(o) = spec.as_object_mut() {
        o.remove("cwd");
    }
    let effective = env.iter().find(|(k, _)| k == "PATH").map(|(_, v)| &**v);
    let command = spec["command"].as_str().map(crate::expand_user);
    if let Some(effective) = effective
        && let Some(name) = command.filter(|c| !c.is_empty() && !c.contains('/'))
        && let Some(found) = find_in_path(&name, effective)
    {
        spec["command"] = Value::String(found.to_string_lossy().into_owned());
    }
    let mut std_command = crate::command_env(&spec, true, Some(env))?;
    std_command.env_clear();
    std_command.envs(env.iter().map(|(k, v)| (k, v)));
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
