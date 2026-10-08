//! Corte `services` (plan 2f-3, Tarea 3): configuración SSH de `bin/cc-dash`
//! (`parse_ssh_config` 7350, `_ssh_block` 7389, `ssh_add` 7458,
//! `ssh_update` 7473, `ssh_key_setup` 7516, `ssh_remove` 7550).
//!
//! - GET `/ssh` (prefijo): los hosts de `~/.ssh/config`.
//! - POST `/ssh-add`, `/ssh-del`, `/ssh-update`: reescriben `~/.ssh/config`
//!   bajo `flock(~/.ssh/config.lock)` (el mismo candado que el Python) con
//!   `write_file_atomic` (temporal en el mismo directorio, fsync, permisos del
//!   archivo previo o 0600 y rename). Una edición es quitar + agregar en UNA
//!   reescritura: si el alta no valida, el archivo queda como estaba.
//! - POST `/ssh-key-setup`: sesión tmux `ssh-key-<host>` con `ssh-copy-id`,
//!   dentro de un scope del gestor de usuario (`systemd-run --user --scope
//!   --collect --quiet tmux new-session …`), como el Python.
//!
//! Ligereza: cada ruta hace a lo sumo tres saltos al pool de bloqueo (crear
//! `~/.ssh`, esperar el candado en la cola de `FileLock::acquire_timeout` y
//! leer-escribir con el candado tomado); nada se cachea.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    files::{FileLock, LOCK_WAIT, write_text_atomic},
    light::{data, error, read_reply},
    procs::which_in,
    py::{str_scalar, take_chars},
    reply,
    tmux::{RunError, run_program},
};
use crate::{HandlerError, Request};
use comandos_core::text::{is_space, shlex_quote, splitlines, strip};
use comandos_runtime::ssh_config;
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::{
    io,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshRoute {
    List,
    Add,
    Del,
    Update,
    KeySetup,
}

pub const ROUTES: &[Entry] = &[
    Entry {
        verb: Verb::Get,
        key: Key::Prefix("/ssh"),
        route: NativeRoute::Ssh(SshRoute::List),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/ssh-add"),
        route: NativeRoute::Ssh(SshRoute::Add),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/ssh-del"),
        route: NativeRoute::Ssh(SshRoute::Del),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/ssh-update"),
        route: NativeRoute::Ssh(SshRoute::Update),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/ssh-key-setup"),
        route: NativeRoute::Ssh(SshRoute::KeySetup),
    },
];

/// Plazo de `subprocess.run(args, …, timeout=15)` en `ssh_key_setup`.
const SETUP_SECONDS: u64 = 15;

const NO_CONFIG: &str = "No hay ~/.ssh/config";
const UNKNOWN_HOST: &str = "Host desconocido; agregalo primero";

pub async fn answer(native: &Arc<Native>, route: SshRoute, request: &Request) -> Answer {
    match route {
        SshRoute::List => list(native).await,
        SshRoute::Add => outcome(add(native, data(request)?).await?),
        SshRoute::Del => outcome(remove(native, data(request)?).await?),
        SshRoute::Update => outcome(update(native, data(request)?).await?),
        SshRoute::KeySetup => key_setup(native, data(request)?).await,
    }
}

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// `if err: 400 {"error": err}` y si no `200 {"ok": True}`.
fn outcome(message: Option<String>) -> Answer {
    match message {
        Some(message) => error(StatusCode::BAD_REQUEST, &message),
        None => reply(StatusCode::OK, &json!({"ok": true})),
    }
}

/// Un trabajo de disco en el pool de bloqueo; si revienta, la excepción sin
/// capturar del Python (500).
async fn blocking<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Result<T, Fault> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|_| failure())
}

/// GET `/ssh`: `parse_ssh_config()` (sin archivo → `[]`; texto que no es UTF-8
/// u otro `OSError` → excepción, 500).
async fn list(native: &Native) -> Answer {
    let home = native.options().home.clone();
    let hosts = blocking(move || ssh_config::hosts(&home))
        .await?
        .map_err(|_| failure())?;
    read_reply(&Value::Array(
        hosts.into_iter().map(Value::Object).collect(),
    ))
}

// ---------------------------------------------------------------------------
// Validación (`_ssh_block`)
// ---------------------------------------------------------------------------

/// El bloque validado: el alias y sus líneas (la primera empieza con `\n`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Block {
    host: String,
    lines: Vec<String>,
}

/// `d.get(key, "").strip()`: un valor que no es texto revienta con
/// `AttributeError` (500).
fn text_field<'a>(d: &'a Map<String, Value>, key: &str) -> Result<&'a str, Fault> {
    match d.get(key) {
        None => Ok(""),
        Some(Value::String(s)) => Ok(strip(s)),
        Some(_) => Err(failure()),
    }
}

/// `str(d.get("port", "")).strip()`. Lo único que el Python mira de un puerto
/// que no es texto ni entero es que no son dígitos y no lleva controles (el
/// `repr` de listas y objetos los escapa; un flotante siempre lleva `.`, `e`,
/// `inf` o `nan`): basta un texto con esas dos propiedades.
fn port_field(d: &Map<String, Value>) -> String {
    match d.get("port") {
        None => String::new(),
        Some(value) => strip(&str_scalar(value).unwrap_or_else(|| "?".to_owned())).to_owned(),
    }
}

/// `^[<clase ASCII>]{1,max}\Z` de las expresiones de `_ssh_block`.
fn ascii_class(s: &str, max: usize, extra: &[u8]) -> bool {
    (1..=max).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
}

/// `_ssh_block(d)`: el bloque, o el mensaje de error en el mismo orden de
/// validación que el Python. `Fault::Decline` solo con un puerto no ASCII
/// (`str.isdigit` acepta dígitos Unicode que `int()` a veces no): aún no hubo
/// efectos.
fn ssh_block(d: &Map<String, Value>) -> Result<Result<Block, &'static str>, Fault> {
    let host = text_field(d, "host")?;
    let hostname = text_field(d, "hostname")?;
    let user = text_field(d, "user")?;
    let port = port_field(d);
    let identity = text_field(d, "identity")?;
    if !ssh_config::is_host(host) {
        return Ok(Err("Alias invalido (letras, numeros, . _ -)"));
    }
    // Ni un salto de línea ni un control: ninguna directiva inyectada
    // (ProxyCommand = RCE).
    if [hostname, user, port.as_str(), identity]
        .iter()
        .any(|v| v.chars().any(|c| (c as u32) < 32))
    {
        return Ok(Err("Caracteres no permitidos"));
    }
    if !ascii_class(hostname, 255, b"._:-[]") {
        return Ok(Err("Hostname/IP invalido"));
    }
    if !user.is_empty() && !ascii_class(user, 64, b"._@-") {
        return Ok(Err("Usuario invalido"));
    }
    if !port.is_empty() {
        if !port.is_ascii() {
            return Err(Fault::Decline);
        }
        let valid = port.bytes().all(|b| b.is_ascii_digit())
            && port.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n));
        if !valid {
            return Ok(Err("Puerto invalido"));
        }
    }
    if !identity.is_empty() && !ascii_class(identity, 255, b"._/~-") {
        return Ok(Err("Ruta de llave invalida"));
    }
    let mut lines = vec![format!("\nHost {host}"), format!("    HostName {hostname}")];
    if !user.is_empty() {
        lines.push(format!("    User {user}"));
    }
    if !port.is_empty() {
        lines.push(format!("    Port {port}"));
    }
    if !identity.is_empty() {
        lines.push(format!("    IdentityFile {identity}"));
    }
    Ok(Ok(Block {
        host: host.to_owned(),
        lines,
    }))
}

// ---------------------------------------------------------------------------
// Texto de `~/.ssh/config`
// ---------------------------------------------------------------------------

/// `_ssh_with_block(text, block)`.
fn with_block(text: Option<&str>, block: &Block) -> String {
    format!("{}{}\n", text.unwrap_or(""), block.lines.join("\n"))
}

/// `l.strip().lower().startswith("host ")`: ningún carácter no ASCII baja a
/// `h`, `o`, `s` o `t`, así que basta comparar los cinco primeros bytes.
fn is_host_line(stripped: &str) -> bool {
    stripped
        .get(..5)
        .is_some_and(|head| head.eq_ignore_ascii_case("host "))
}

/// `_ssh_without_host(text, host)`: quita el bloque `Host <host>` (solo de un
/// nombre). `host` es el valor tal como llegó: uno que no es texto nunca está
/// en la lista de nombres (`in` compara con `==`), como en el Python.
fn without_host(text: &str, host: &Value) -> Result<String, &'static str> {
    let mut out: Vec<&str> = Vec::new();
    let (mut skip, mut found) = (false, false);
    for line in splitlines(text) {
        let s = strip(line);
        if is_host_line(s) {
            let names: Vec<&str> = s.split(is_space).filter(|n| !n.is_empty()).collect();
            let names = names.get(1..).unwrap_or_default();
            if names.iter().any(|name| host.as_str() == Some(name)) {
                found = true;
                if names.len() > 1 {
                    return Err("Ese host comparte linea con otros; editalo a mano");
                }
                skip = true;
                continue;
            }
            skip = false;
        }
        if !skip {
            out.push(line);
        }
    }
    if !found {
        return Err("No existe");
    }
    let joined = out.join("\n");
    Ok(format!("{}\n", joined.trim_end_matches(is_space)))
}

/// `any(h["host"] == host for h in parse_ssh_config(text))`.
fn has_host(text: &str, host: &str) -> bool {
    ssh_config::parse(text)
        .iter()
        .any(|h| h.get("host").and_then(Value::as_str) == Some(host))
}

fn exists_message(host: &str) -> String {
    format!("'{host}' ya existe en ~/.ssh/config")
}

/// `_ssh_config_path()`.
fn config_path(home: &Path) -> PathBuf {
    home.join(".ssh").join("config")
}

/// `with file_lock(path)`: el `flock` de `<path>.lock` (crea `~/.ssh` y el
/// candado si faltan, como `file_lock`). El Python espera sin plazo; aquí, un
/// dueño colgado más allá de `LOCK_WAIT` declina: lo único que pudo pasar
/// antes es crear el directorio y el candado, que el Python repite igual.
async fn lock(home: &Path) -> Result<FileLock, Fault> {
    match FileLock::acquire_timeout(&config_path(home), LOCK_WAIT).await {
        Ok(lock) => Ok(lock),
        Err(e) if e.kind() == io::ErrorKind::TimedOut => Err(Fault::Decline),
        Err(_) => Err(failure()),
    }
}

/// `_ssh_config_read` + una reescritura con el candado tomado: `edit` decide
/// a partir del texto (sin archivo: `None`) y devuelve el texto nuevo o el
/// mensaje de error. Errores de E/S → 500 (el Python no los captura).
async fn rewrite<F>(home: PathBuf, lock: FileLock, edit: F) -> Result<Option<String>, Fault>
where
    F: FnOnce(Option<&str>) -> Result<String, String> + Send + 'static,
{
    blocking(move || -> io::Result<Option<String>> {
        let _lock = lock;
        let text = ssh_config::read(&home)?;
        match edit(text.as_deref()) {
            Ok(new) => {
                write_text_atomic(&config_path(&home), &new)?;
                Ok(None)
            }
            Err(message) => Ok(Some(message)),
        }
    })
    .await?
    .map_err(|_| failure())
}

/// `ssh_add(d)`.
async fn add(native: &Native, d: &Map<String, Value>) -> Result<Option<String>, Fault> {
    let block = match ssh_block(d)? {
        Ok(block) => block,
        Err(message) => return Ok(Some(message.to_owned())),
    };
    let home = native.options().home.clone();
    // `os.makedirs(~/.ssh, mode=0o700, exist_ok=True)`: un archivo con ese
    // nombre es `FileExistsError` (500).
    let dir = home.join(".ssh");
    blocking(move || {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
    })
    .await?
    .map_err(|_| failure())?;
    let lock = lock(&home).await?;
    rewrite(home, lock, move |text| {
        if has_host(text.unwrap_or(""), &block.host) {
            return Err(exists_message(&block.host));
        }
        // Nuevo: 0600 (`write_file_atomic` conserva el modo de uno existente).
        Ok(with_block(text, &block))
    })
    .await
}

/// `ssh_update(str(data.get("orig", "")), d)`: quitar + agregar en una sola
/// reescritura.
async fn update(native: &Native, d: &Map<String, Value>) -> Result<Option<String>, Fault> {
    // `str()` de un flotante, lista u objeto (su `repr`) no se reproduce:
    // declinar, aún sin efectos.
    let orig = match d.get("orig") {
        None => String::new(),
        Some(value) => str_scalar(value).ok_or(Fault::Decline)?,
    };
    let block = match ssh_block(d)? {
        Ok(block) => block,
        Err(message) => return Ok(Some(message.to_owned())),
    };
    let home = native.options().home.clone();
    let lock = lock(&home).await?;
    rewrite(home, lock, move |text| {
        let text = text.ok_or_else(|| NO_CONFIG.to_owned())?;
        let rest = without_host(text, &Value::String(orig)).map_err(str::to_owned)?;
        if has_host(&rest, &block.host) {
            return Err(exists_message(&block.host));
        }
        Ok(with_block(Some(&rest), &block))
    })
    .await
}

/// `ssh_remove(data.get("host", ""))`, sin `str()`.
async fn remove(native: &Native, d: &Map<String, Value>) -> Result<Option<String>, Fault> {
    let host = d
        .get("host")
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()));
    let home = native.options().home.clone();
    let lock = lock(&home).await?;
    rewrite(home, lock, move |text| {
        let text = text.ok_or_else(|| NO_CONFIG.to_owned())?;
        without_host(text, &host).map_err(str::to_owned)
    })
    .await
}

// ---------------------------------------------------------------------------
// `ssh_key_setup`
// ---------------------------------------------------------------------------

/// Lo que se decide antes de tocar tmux.
enum Prep {
    Message(&'static str),
    Key(String),
    Decline,
}

/// `os.path.expanduser(path)` con el HOME del frente; `None` con `~usuario`
/// (necesitaría `pwd`) → declinar.
fn expand_user(path: &str, home: &str) -> Option<String> {
    let Some(rest) = path.strip_prefix('~') else {
        return Some(path.to_owned());
    };
    let tail_at = rest.find('/').unwrap_or(rest.len());
    if tail_at != 0 {
        return None;
    }
    let joined = format!("{}{rest}", home.trim_end_matches('/'));
    Some(if joined.is_empty() {
        "/".to_owned()
    } else {
        joined
    })
}

/// `ssh_public_key_for_host(host)` (ya sabido que el host existe, aunque se
/// relee como el Python). Una ruta relativa se resolvería contra el directorio
/// de trabajo del Python, que no es el del frente: declinar.
fn public_key(home: &Path, host: &str) -> io::Result<Prep> {
    let Some(home_text) = home.to_str() else {
        return Ok(Prep::Decline);
    };
    let Some(entry) = ssh_config::host_entry(home, host)? else {
        return Ok(Prep::Message(UNKNOWN_HOST));
    };
    let mut candidates = Vec::new();
    let identity = strip(entry.get("identity").and_then(Value::as_str).unwrap_or(""));
    if !identity.is_empty() {
        let Some(path) = expand_user(identity, home_text) else {
            return Ok(Prep::Decline);
        };
        candidates.push(if path.ends_with(".pub") {
            path
        } else {
            format!("{path}.pub")
        });
    }
    let base = home_text.trim_end_matches('/');
    candidates.push(format!("{base}/.ssh/id_ed25519.pub"));
    candidates.push(format!("{base}/.ssh/id_rsa.pub"));
    for key in candidates {
        if !Path::new(&key).is_absolute() {
            return Ok(Prep::Decline);
        }
        // `os.path.isfile`: sigue enlaces; cualquier error es «no».
        if std::fs::metadata(&key).is_ok_and(|m| m.is_file()) {
            return Ok(Prep::Key(key));
        }
    }
    Ok(Prep::Message(
        "No encontre llave publica (~/.ssh/id_ed25519.pub o IdentityFile.pub)",
    ))
}

/// La orden literal que corre la sesión de `ssh_key_setup`.
fn setup_command(host: &str, key: &str) -> String {
    let host_q = shlex_quote(host);
    format!(
        "printf '%s\\n\\n' {}; \
         ssh-copy-id -o ConnectTimeout=8 -i {} {host_q}; \
         rc=$?; echo; \
         if [ \"$rc\" -eq 0 ]; then   echo {};   exec ssh {host_q}; \
         else   echo {};   exec $SHELL; fi",
        shlex_quote(&format!("Instalando llave publica para {host}")),
        shlex_quote(key),
        shlex_quote("Llave instalada. Conectando..."),
        shlex_quote("No se pudo instalar la llave. Revisa password/red y reintenta."),
    )
}

/// POST `/ssh-key-setup`: `ssh_key_setup(data.get("host", ""))`.
async fn key_setup(native: &Native, d: &Map<String, Value>) -> Answer {
    // `SSH_HOST_RE.match(<no texto>)` es `TypeError` (500).
    let host = match d.get("host") {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(_) => return Err(failure()),
    };
    let opts = native.options();
    let home = opts.home.clone();
    let search_path = opts.search_path.clone();
    let prep = blocking({
        let host = host.clone();
        move || -> io::Result<Prep> {
            if !ssh_config::is_host(&host) || ssh_config::host_entry(&home, &host)?.is_none() {
                return Ok(Prep::Message(UNKNOWN_HOST));
            }
            if which_in(search_path.as_deref(), "ssh-copy-id").is_none() {
                return Ok(Prep::Message("ssh-copy-id no esta instalado"));
            }
            public_key(&home, &host)
        }
    })
    .await?
    .map_err(|_| failure())?;
    let key = match prep {
        Prep::Message(message) => return error(StatusCode::BAD_REQUEST, message),
        Prep::Decline => return Err(Fault::Decline),
        Prep::Key(key) => key,
    };
    let session = take_chars(&format!("ssh-key-{host}"), 60);
    let target = format!("={session}");
    let exists = opts
        .tmux
        .run(&["has-session", "-t", &target])
        .await
        .map_err(|e| Fault::Error(e.uncaught()))?;
    if exists.ok {
        return reply(StatusCode::OK, &json!({"ok": true, "session": session}));
    }
    // Linux: sin `systemd-run` el frente declina
    // (aún sin efectos), como `/terminal/quick`: un servidor tmux nacido en el
    // cgroup del frente moriría con él al reiniciar el servicio. Darwin va directo.
    let Some(program) = super::quick::platform_tmux(
        comandos_runtime::platform::host(),
        opts.scope.as_ref(),
        &opts.tmux.program,
    ) else {
        return Err(Fault::Decline);
    };
    let cmd = setup_command(&host, &key);
    let args = ["new-session", "-d", "-s", &session, "-n", "setup", &cmd];
    let out = match run_program(&program, &args, Duration::from_secs(SETUP_SECONDS)).await {
        Ok(out) => out,
        Err(RunError::Timeout) => return Err(Fault::Error(HandlerError::Timeout)),
        Err(RunError::Spawn(_) | RunError::Decode) => return Err(failure()),
    };
    if !out.ok {
        let text = strip(&out.stderr);
        let message = if text.is_empty() {
            "No se pudo crear la sesion"
        } else {
            text
        };
        return error(StatusCode::BAD_REQUEST, message);
    }
    reply(StatusCode::OK, &json!({"ok": true, "session": session}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    fn message(value: Value) -> Option<&'static str> {
        match ssh_block(&obj(value)) {
            Ok(Ok(_)) => None,
            Ok(Err(m)) => Some(m),
            Err(_) => Some("<fault>"),
        }
    }

    #[test]
    fn block_validation_order_matches_python() {
        assert_eq!(
            message(json!({"host": "a b", "hostname": "x\n"})),
            Some("Alias invalido (letras, numeros, . _ -)")
        );
        assert_eq!(
            message(json!({"host": "a", "hostname": "h\nProxyCommand x"})),
            Some("Caracteres no permitidos")
        );
        assert_eq!(
            message(json!({"host": "a", "hostname": "h", "port": "2\u{7}"})),
            Some("Caracteres no permitidos")
        );
        assert_eq!(message(json!({"host": "a"})), Some("Hostname/IP invalido"));
        assert_eq!(
            message(json!({"host": "a", "hostname": "h", "user": "a b"})),
            Some("Usuario invalido")
        );
        for port in [
            json!("0"),
            json!("65536"),
            json!("-1"),
            json!(1.0),
            json!(true),
        ] {
            assert_eq!(
                message(json!({"host": "a", "hostname": "h", "port": port})),
                Some("Puerto invalido")
            );
        }
        assert_eq!(
            message(json!({"host": "a", "hostname": "h", "identity": "a b"})),
            Some("Ruta de llave invalida")
        );
        assert!(matches!(
            ssh_block(&obj(json!({"host": 5}))),
            Err(Fault::Error(HandlerError::Failure))
        ));
        assert!(matches!(
            ssh_block(&obj(json!({"host": "a", "hostname": "h", "port": "２２"}))),
            Err(Fault::Decline)
        ));
        let block = ssh_block(&obj(json!({
            "host": " srv ", "hostname": "10.0.0.1", "user": "root", "port": 22,
            "identity": "~/.ssh/k"
        })));
        assert!(matches!(&block, Ok(Ok(b)) if b.lines == [
            "\nHost srv", "    HostName 10.0.0.1", "    User root", "    Port 22",
            "    IdentityFile ~/.ssh/k"
        ]));
    }

    #[test]
    fn without_host_matches_python() {
        let text = "# c\nHost a\n  HostName x\nhost b c\n  User u\nHOST d\n  Port 2\n\n";
        assert_eq!(
            without_host(text, &json!("a")),
            Ok("# c\nhost b c\n  User u\nHOST d\n  Port 2\n".to_owned())
        );
        assert_eq!(
            without_host(text, &json!("d")),
            Ok("# c\nHost a\n  HostName x\nhost b c\n  User u\n".to_owned())
        );
        assert_eq!(
            without_host(text, &json!("b")),
            Err("Ese host comparte linea con otros; editalo a mano")
        );
        assert_eq!(without_host(text, &json!("z")), Err("No existe"));
        assert_eq!(without_host(text, &json!(5)), Err("No existe"));
    }

    #[test]
    fn expand_user_matches_posixpath() {
        assert_eq!(expand_user("~/.ssh/k", "/h/"), Some("/h/.ssh/k".into()));
        assert_eq!(expand_user("~", "/h"), Some("/h".into()));
        assert_eq!(expand_user("~", "/"), Some("/".into()));
        assert_eq!(expand_user("/k", "/h"), Some("/k".into()));
        assert_eq!(expand_user("~root/k", "/h"), None);
    }

    #[test]
    fn setup_command_is_literal() {
        assert_eq!(
            setup_command("srv", "/h/.ssh/id_ed25519.pub"),
            "printf '%s\\n\\n' 'Instalando llave publica para srv'; \
             ssh-copy-id -o ConnectTimeout=8 -i /h/.ssh/id_ed25519.pub srv; rc=$?; echo; \
             if [ \"$rc\" -eq 0 ]; then   echo 'Llave instalada. Conectando...';   \
             exec ssh srv; else   echo 'No se pudo instalar la llave. Revisa password/red y \
             reintenta.';   exec $SHELL; fi"
        );
    }
}
