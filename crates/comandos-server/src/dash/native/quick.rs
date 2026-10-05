//! POST `/terminal/quick` (`quick_terminal_request` 5513 del `bin/cc-dash`
//! confirmado, `lib/quick_terminal.py`) solo con `place == "sidebar"` (D7):
//! la terminal de la barra no se registra como pestaña, así que no toca
//! `app-tabs-meta.json` ni el candado de proceso del Python.
//!
//! El worker de la base solo ve trabajos cortos: el reclamo (`_claim`, con la
//! reserva de la carpeta dentro de la misma transacción) y el cierre
//! (`_finish`). La espera entre reclamos (50 ms) y el lanzamiento (hasta 15 s)
//! corren fuera, así un doble clic no retiene la base. Tras el reclamo ya no se
//! declina (excepción 3b de los rulings): la fila `launching` y la carpeta ya
//! existen, como en el Python.
//!
//! Seguridad: la sesión nueva nace SIEMPRE dentro de un scope del gestor de
//! usuario (`systemd-run --user --scope --collect --quiet tmux …`, como
//! `scope_cmd`), nunca del sistema. Sin `systemd-run` (opción `scope` vacía)
//! se declina: un servidor tmux que naciera en el cgroup del frente moriría con
//! él al reiniciar el servicio (riesgo R2 del preflight).
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    light::data,
    py::{self, repr_ascii},
    reply,
    tmux::{Program, RunError, TmuxError, run_program},
};
use crate::{HandlerError, Request};
use comandos_runtime::{
    providers::which_path,
    quick_terminal::{
        Claim, Error as QuickError, Options, POLL_SECONDS, Terminal, WAIT_SECONDS, claim, finish,
        valid_request_id,
    },
};
use http::StatusCode;
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub const ROUTES: &[Entry] = &[Entry {
    verb: Verb::Post,
    key: Key::Raw("/terminal/quick"),
    route: NativeRoute::QuickTerminal,
}];

/// Plazo de `subprocess.run(..., timeout=15)` en `quick_terminal_launch`.
const LAUNCH_SECONDS: u64 = 15;

/// Lo que `scope_cmd` antepone tras `systemd-run`: gestor de USUARIO.
pub const SCOPE_FLAGS: [&str; 4] = ["--user", "--scope", "--collect", "--quiet"];

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// `systemd-run` (ruta ya resuelta) con las banderas de `scope_cmd`.
pub fn scope_program(path: impl Into<PathBuf>) -> Program {
    let mut program = Program::named(path);
    program.prefix = SCOPE_FLAGS.iter().map(Into::into).collect();
    program
}

/// `shutil.which("systemd-run")` sobre el `PATH` del frente (A5): solo `PATH`,
/// sin los bins de usuario de `provider_registry.which`.
pub fn find_scope(search_path: Option<&OsStr>) -> Option<Program> {
    which_path("systemd-run", search_path).map(scope_program)
}

pub async fn answer(native: &Arc<Native>, request: &Request) -> Answer {
    let data = data(request)?;
    if !matches!(data.get("place"), Some(Value::String(place)) if place == "sidebar") {
        return Err(Fault::Decline);
    }
    let Some(scope) = native.options().scope.clone() else {
        return Err(Fault::Decline);
    };
    let raw = data.get("requestId").unwrap_or(&Value::Null);
    if !valid_request_id(raw) {
        return reply(
            StatusCode::BAD_REQUEST,
            &json!({"error": "requestId inválido", "code": "request", "retryable": false}),
        );
    }
    let id = raw.as_str().ok_or_else(failure)?.to_owned();
    // Reclamo, lanzamiento y cierre en su propia tarea: si el cliente se va a
    // mitad, también con el reclamo en el worker, soltar la petición no deja la
    // fila en `launching` sin lanzador (el reintento esperaría 15 s y daría 409
    // hasta que venciera la concesión de 30 s) ni mata el `tmux` del scope. El
    // hilo del Python también termina aunque el cliente se vaya.
    let job = tokio::spawn({
        let native = native.clone();
        async move { claim_and_launch(&native, id, scope).await }
    });
    job.await.map_err(|_| failure())?
}

/// El bucle de reclamos de `quick_terminal_request` y, con un reclamo
/// propio, el lanzamiento y el cierre.
async fn claim_and_launch(native: &Native, id: String, scope: Program) -> Answer {
    let seconds = native.options().clock_seconds.clone();
    let deadline = seconds() + WAIT_SECONDS;
    let terminal = loop {
        match claim_once(native, &id).await? {
            Ok(Claim::Ready(terminal)) => return reply(StatusCode::OK, &terminal.result(false)),
            Ok(Claim::Own(terminal)) => break terminal,
            Ok(Claim::Wait) => {
                if seconds() >= deadline {
                    return reply(
                        StatusCode::CONFLICT,
                        &json!({
                            "error": "La terminal se está abriendo; reintenta en unos segundos",
                            "code": "busy",
                            "retryable": true,
                        }),
                    );
                }
                tokio::time::sleep(Duration::from_secs_f64(POLL_SECONDS)).await;
            }
            Err(QuickError::Quick(q)) if q.code == "folder" => {
                return reply(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &json!({"error": q.message, "code": "folder", "retryable": q.retryable}),
                );
            }
            // Un error de SQLite no lo captura `quick_terminal_request`: 500.
            Err(_) => return Err(failure()),
        }
    };
    launch_and_finish(native, id, terminal, scope).await
}

/// `launch` + `_finish` + la respuesta, tras un reclamo propio. Aquí ya no se
/// declina (excepción 3b): un worker que rechaza el cierre es la excepción sin
/// capturar de `_finish` (500), nunca un reenvío.
async fn launch_and_finish(
    native: &Native,
    id: String,
    terminal: Terminal,
    scope: Program,
) -> Answer {
    let outcome = launch(native, &terminal, &scope).await;
    let stored = outcome
        .as_ref()
        .err()
        .map(|message| message.chars().take(500).collect::<String>());
    let state = if outcome.is_ok() { "ready" } else { "failed" };
    let seconds = native.options().clock_seconds.clone();
    let cwd = terminal.cwd.clone();
    native
        .with_state(move |b| finish(&b.conn, &id, state, stored.as_deref(), &cwd, &|| seconds()))
        .await
        .map_err(|_| failure())?
        .map_err(|_| failure())?;
    match outcome {
        Ok(()) => reply(StatusCode::OK, &terminal.result(true)),
        Err(message) => reply(
            StatusCode::BAD_GATEWAY,
            &json!({
                "error": format!("No se pudo abrir la terminal: {message}"),
                "code": "launch",
                "retryable": true,
                "cwd": terminal.cwd,
            }),
        ),
    }
}

/// Un `_claim` como trabajo corto del worker. `now` es el `datetime.now(ZONE)`
/// del nombre de la carpeta; `clock`, el `time.time()` de la concesión.
async fn claim_once(
    native: &Native,
    id: &str,
) -> Result<comandos_runtime::quick_terminal::Result<Claim>, Fault> {
    let base = native.options().quick_base.clone();
    let now = chrono::DateTime::from_timestamp_millis((native.options().clock)())
        .ok_or_else(failure)?
        .fixed_offset();
    let seconds = native.options().clock_seconds.clone();
    let id = id.to_owned();
    native
        .with_state(move |b| claim(&b.conn, &id, &Options::new(&base, now), &|| seconds()))
        .await
}

/// `os.makedirs` + `exists` + `launch` del `try` de `open_quick_terminal`; el
/// error es `str(exc) or exc.__class__.__name__`.
async fn launch(native: &Native, t: &Terminal, scope: &Program) -> Result<(), String> {
    let cwd = PathBuf::from(&t.cwd);
    let made = tokio::task::spawn_blocking({
        let cwd = cwd.clone();
        move || std::fs::create_dir_all(&cwd)
    })
    .await;
    match made {
        Ok(Ok(())) => {}
        // `os.makedirs` nombra el componente que falló; aquí, la carpeta entera.
        Ok(Err(error)) => return Err(os_error_text(&error, &t.cwd)),
        Err(_) => return Err("RuntimeError".into()),
    }
    let tmux = &native.options().tmux;
    // `quick_terminal_exists`: `tmux("has-session", "-t", "=" + sess)`, 5 s.
    let target = format!("={}", t.session);
    let exists = tmux
        .run(&["has-session", "-t", &target])
        .await
        .map_err(|e| tmux_text(&e))?;
    if exists.ok {
        return Ok(());
    }
    // `scope_cmd(["tmux", "new-session", …])`: `systemd-run --user --scope
    // --collect --quiet <tmux> <prefijo de tmux> new-session …`. El entorno de
    // tmux (el socket privado en las pruebas) pasa al programa del scope.
    let mut program = scope.clone();
    program
        .prefix
        .push(tmux.program.path.clone().into_os_string());
    program.prefix.extend(tmux.program.prefix.iter().cloned());
    program.env.extend(tmux.program.env.iter().cloned());
    program
        .env_remove
        .extend(tmux.program.env_remove.iter().cloned());
    let args = [
        "new-session",
        "-d",
        "-s",
        &t.session,
        "-c",
        &t.cwd,
        "-P",
        "-F",
        "#{pane_id}",
    ];
    let out = match run_program(&program, &args, Duration::from_secs(LAUNCH_SECONDS)).await {
        Ok(out) => out,
        Err(RunError::Timeout) => {
            // El argv del Python: los nombres sin resolver y sin prefijo de prueba.
            let mut argv: Vec<&str> = vec!["systemd-run"];
            argv.extend(SCOPE_FLAGS);
            argv.push("tmux");
            argv.extend(args);
            return Err(timeout_text(&argv, LAUNCH_SECONDS));
        }
        Err(RunError::Spawn(error)) => return Err(os_error_text(&error, "systemd-run")),
        // `text=True` con bytes no UTF-8: el texto del códec no se reproduce.
        Err(RunError::Decode) => return Err("UnicodeDecodeError".into()),
    };
    if !out.ok {
        // `RuntimeError((r.stderr or "tmux falló").strip())`.
        let raw = if out.stderr.is_empty() {
            "tmux falló"
        } else {
            out.stderr.as_str()
        };
        let text = py::strip(raw);
        return Err(if text.is_empty() {
            "RuntimeError".into()
        } else {
            text.to_owned()
        });
    }
    let pane = py::strip(&out.stdout);
    if pane.starts_with('%') {
        // El resultado no cuenta (el Python lo ignora); su excepción sí.
        tmux.run(&[
            "set-option",
            "-p",
            "-t",
            pane,
            "@comandos-pane-key",
            &t.pane,
        ])
        .await
        .map_err(|e| tmux_text(&e))?;
    }
    // Registro: nada; la terminal vive en la barra (`place == "sidebar"`).
    Ok(())
}

/// `str(exc)` de una excepción de `tmux()` capturada por el `try`.
fn tmux_text(error: &TmuxError) -> String {
    if let Some(text) = error.python_message() {
        return text;
    }
    match error {
        // `[Errno N] <strerror>: 'tmux'`, el `OSError` de `subprocess.run`.
        TmuxError::Spawn(_, Some(code)) => {
            os_error_text(&io::Error::from_raw_os_error(*code), "tmux")
        }
        TmuxError::Decode => "UnicodeDecodeError".into(),
        _ => "OSError".into(),
    }
}

/// `repr(str)` de Python: `repr_ascii` o, con texto no ASCII, las mismas
/// comillas y escapes conservando los caracteres (diferencia aceptada para
/// no imprimibles Unicode, que el Python escaparía).
fn py_repr(text: &str) -> String {
    if let Some(repr) = repr_ascii(text) {
        return repr;
    }
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// `str(OSError)` con nombre de archivo: `[Errno N] <strerror>: '<nombre>'`.
fn os_error_text(error: &io::Error, filename: &str) -> String {
    let message = error.to_string();
    match error.raw_os_error() {
        Some(code) => {
            let strerror = message
                .strip_suffix(&format!(" (os error {code})"))
                .unwrap_or(&message);
            format!("[Errno {code}] {strerror}: {}", py_repr(filename))
        }
        None => message,
    }
}

/// `str(subprocess.TimeoutExpired)`: `Command '[…]' timed out after N seconds`.
fn timeout_text(argv: &[&str], seconds: u64) -> String {
    let parts: Vec<String> = argv.iter().map(|a| py_repr(a)).collect();
    format!(
        "Command '[{}]' timed out after {seconds} seconds",
        parts.join(", ")
    )
}

/// `quick_terminal_lib.default_base()` con el entorno del frente al arrancar.
pub fn default_base(home: &Path) -> PathBuf {
    let custom = std::env::var_os("COMANDOS_QUICK_TERMINAL_BASE").map(PathBuf::from);
    comandos_runtime::quick_terminal::default_base(home, custom.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_match_python() {
        let argv = ["systemd-run", "--user", "tmux", "#{pane_id}", "/tmp/a b"];
        assert_eq!(
            timeout_text(&argv, 15),
            "Command '['systemd-run', '--user', 'tmux', '#{pane_id}', '/tmp/a b']' timed out after 15 seconds"
        );
        let missing = io::Error::from_raw_os_error(2);
        assert_eq!(
            os_error_text(&missing, "systemd-run"),
            "[Errno 2] No such file or directory: 'systemd-run'"
        );
        // Cualquier `OSError` al arrancar tmux conserva su `errno`, como el Python.
        assert_eq!(
            tmux_text(&TmuxError::Spawn(io::ErrorKind::PermissionDenied, Some(13))),
            "[Errno 13] Permission denied: 'tmux'"
        );
        assert_eq!(
            tmux_text(&TmuxError::Spawn(io::ErrorKind::Other, Some(8))),
            "[Errno 8] Exec format error: 'tmux'"
        );
        assert_eq!(py_repr("año"), "'año'");
        assert_eq!(py_repr("it's"), "\"it's\"");
    }

    #[test]
    fn scope_is_the_user_manager() {
        let scope = scope_program("/usr/bin/systemd-run");
        let flags: Vec<&OsStr> = scope.prefix.iter().map(|p| p.as_os_str()).collect();
        assert_eq!(flags, ["--user", "--scope", "--collect", "--quiet"]);
    }
}
