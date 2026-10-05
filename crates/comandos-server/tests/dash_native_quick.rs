//! POST /terminal/quick en la barra, nativo: reclamo y cierre en trabajos
//! cortos del worker; espera y lanzamiento fuera de él.
//!
//! Seguridad tmux: todo tmux de estas pruebas es el servidor privado del HOME
//! temporal (`Tmux::private`, `-S` explícito; `TestHome::tmux_command` para
//! leerlo). El `systemd-run` es siempre el falso de `support::fake_scope`, que
//! anota su argv y ejecuta el tmux de la prueba; nunca el del sistema.
mod support;
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    fs,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use support::{
    FakeLegacy, TestHome, dead_port, fake_scope, fake_scope_calls, front, get, request_body,
    tmux_available,
};
use tokio::time::timeout;

/// Ninguna petición de estas pruebas puede colgarse (B13).
const LIMIT: Duration = Duration::from_secs(30);

fn opts(home: &TestHome) -> comandos_server::dash::native::NativeOptions {
    let mut o = home.options();
    o.quick_base = home.root.join("Terminal");
    o.scope = Some(fake_scope(home));
    o
}

async fn quick(port: u16, body: &str) -> support::Wire {
    timeout(
        LIMIT,
        request_body(port, "POST", "/terminal/quick", "", body),
    )
    .await
    .expect("la terminal rápida no debe colgarse")
}

fn json_of(wire: &support::Wire) -> Value {
    serde_json::from_slice(&wire.body).unwrap()
}

fn rows(home: &TestHome) -> Vec<(String, String, Option<String>)> {
    let Ok(conn) = rusqlite::Connection::open(home.state_db()) else {
        return Vec::new();
    };
    let Ok(mut stmt) =
        conn.prepare("SELECT request_id, state, error FROM quick_terminal_requests ORDER BY 1")
    else {
        return Vec::new();
    };
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// `argv` del `systemd-run` falso con el tmux de la prueba (ruta, `-f
/// /dev/null -S <socket>`) reducido a `tmux`, como lo escribe `scope_cmd`.
fn as_python(home: &TestHome, call: &[String], tmux_path: &str) -> Vec<String> {
    let socket = comandos_server::dash::native::tmux::private_socket(&home.tmux_dir());
    let prefix = [
        tmux_path.to_owned(),
        "-f".to_owned(),
        "/dev/null".to_owned(),
        "-S".to_owned(),
        socket.display().to_string(),
    ];
    let at = call
        .windows(prefix.len())
        .position(|w| w == prefix)
        .expect("el tmux de la prueba con -S va tras las banderas del scope");
    let mut out: Vec<String> = call[..at].to_vec();
    out.push("tmux".into());
    out.extend(call[at + prefix.len()..].iter().cloned());
    out
}

#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_sidebar_opens_once_and_replays() {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new("quick-open");
    let front = front(&home, dead_port(), opts(&home)).await;
    let body = r#"{"requestId":"req-1","place":"sidebar"}"#;
    let first = quick(front.port, body).await;
    assert_eq!(first.status, 200, "{}", first.text());
    let v = json_of(&first);
    assert_eq!(v["created"], Value::Bool(true));
    let tab = v["tabId"].as_str().unwrap();
    assert!(tab.starts_with("term-q"), "{v}");
    let cwd = v["cwd"].as_str().unwrap();
    assert!(fs::metadata(cwd).unwrap().is_dir());
    assert!(cwd.starts_with(home.root.join("Terminal").to_str().unwrap()));
    assert_eq!(v["label"], json!(cwd.rsplit('/').next().unwrap()));
    // Una sola shell, por el scope de USUARIO, con el argv de `scope_cmd`.
    let calls = fake_scope_calls(&home);
    assert_eq!(calls.len(), 1, "{calls:?}");
    let python = as_python(&home, &calls[0], "tmux");
    assert_eq!(
        python,
        [
            "--user",
            "--scope",
            "--collect",
            "--quiet",
            "tmux",
            "new-session",
            "-d",
            "-s",
            tab,
            "-c",
            cwd,
            "-P",
            "-F",
            "#{pane_id}",
        ]
    );
    // La llave lógica quedó en el pane (`set-option -p @comandos-pane-key`).
    let key = home
        .tmux_command()
        .args([
            "display-message",
            "-p",
            "-t",
            &format!("={tab}:"),
            "#{@comandos-pane-key}",
        ])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&key.stdout).trim(),
        v["paneKey"].as_str().unwrap()
    );
    let again = quick(front.port, body).await;
    let w = json_of(&again);
    assert_eq!(
        (w["created"].clone(), w["cwd"].clone()),
        (Value::Bool(false), v["cwd"].clone())
    );
    assert_eq!(fake_scope_calls(&home).len(), 1, "la repetición no lanza");
    // Otra petición con la sesión viva (reintento tras `failed`): sin lanzar.
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    conn.execute(
        "UPDATE quick_terminal_requests SET state='failed' WHERE request_id='req-1'",
        [],
    )
    .unwrap();
    let retried = json_of(&quick(front.port, body).await);
    assert_eq!(retried["created"], Value::Bool(true));
    assert_eq!(retried["cwd"], v["cwd"]);
    assert_eq!(fake_scope_calls(&home).len(), 1, "has-session la encontró");
    front.stop().await;
}

/// Review Focus 5: doble clic con el mismo `requestId`. Una sola shell; la
/// segunda espera el reclamo sin retener el worker (GET /workspace responde).
#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_concurrent_same_request_one_shell_worker_free() {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new("quick-conc");
    // tmux lento en new-session; conserva el `-f /dev/null -S <socket>` que
    // le antepone `Tmux::private`.
    let wrapper = home.root.join("bin/tmux-lento");
    fs::write(
        &wrapper,
        "#!/bin/sh\ncase \"$*\" in *new-session*) sleep 1;; esac\nexec tmux \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(
        &wrapper,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let mut o = opts(&home);
    o.tmux.program.path = wrapper.clone();
    let front = front(&home, dead_port(), o).await;
    let port = front.port;
    let body = r#"{"requestId":"req-2","place":"sidebar"}"#;
    let a = tokio::spawn(async move { quick(port, body).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let b = tokio::spawn(async move { quick(port, body).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let started = std::time::Instant::now();
    assert_eq!(get(port, "/workspace").await.status, 200);
    assert!(
        started.elapsed() < Duration::from_millis(400),
        "el worker quedó retenido: {:?}",
        started.elapsed()
    );
    let (a, b) = (a.await.unwrap(), b.await.unwrap());
    assert_eq!(
        (a.status, b.status),
        (200, 200),
        "{} {}",
        a.text(),
        b.text()
    );
    let (va, vb) = (json_of(&a), json_of(&b));
    assert_eq!(va["tabId"], vb["tabId"]);
    assert_eq!(va["cwd"], vb["cwd"]);
    let created = [&va, &vb]
        .iter()
        .filter(|v| v["created"] == Value::Bool(true))
        .count();
    assert_eq!(created, 1);
    assert_eq!(fake_scope_calls(&home).len(), 1, "una sola shell");
    assert_eq!(
        rows(&home),
        [("req-2".to_owned(), "ready".to_owned(), None)]
    );
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_outside_sidebar_unscoped_and_invalid() {
    let home = TestHome::new("quick-dec");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, opts(&home)).await;
    for body in [
        r#"{"requestId":"r"}"#,
        r#"{"requestId":"r","place":"tab"}"#,
        r#"{"requestId":"r","place":["sidebar"]}"#,
    ] {
        assert_eq!(quick(front.port, body).await.text(), r#"{"legacy": true}"#);
    }
    let bad = quick(front.port, r#"{"requestId":"-x","place":"sidebar"}"#).await;
    assert_eq!(
        (bad.status, bad.text().as_str()),
        (
            400,
            r#"{"error": "requestId inv\u00e1lido", "code": "request", "retryable": false}"#
        )
    );
    let missing = quick(front.port, r#"{"place":"sidebar"}"#).await;
    assert_eq!(missing.status, 400);
    assert!(
        rows(&home).is_empty(),
        "un requestId inválido no toca la base"
    );
    assert!(!home.root.join("Terminal").exists());
    front.stop().await;
    // Sin `systemd-run` (scope vacío) se reenvía: nunca una sesión sin scope.
    let front = front_unscoped(&home, legacy.port).await;
    let declined = quick(front.port, r#"{"requestId":"r","place":"sidebar"}"#).await;
    assert_eq!(declined.text(), r#"{"legacy": true}"#);
    assert!(rows(&home).is_empty());
    front.stop().await;
    assert_eq!(legacy.requests().len(), 4);
}

async fn front_unscoped(home: &TestHome, legacy: u16) -> support::Front {
    let mut o = opts(home);
    o.scope = None;
    front(home, legacy, o).await
}

/// Fallo de un paso tras el reclamo: 502 con el texto del Python, fila
/// `failed` con el mensaje, y el reintento reusa la misma carpeta.
#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_launch_failure_matches_python_text() {
    let home = TestHome::new("quick-fail");
    let mut o = opts(&home);
    o.tmux.program.path = "/no-existe/tmux".into();
    let front = front(&home, dead_port(), o).await;
    let body = r#"{"requestId":"r9","place":"sidebar"}"#;
    let got = quick(front.port, body).await;
    assert_eq!(got.status, 502, "{}", got.text());
    let v = json_of(&got);
    let message = "[Errno 2] No such file or directory: 'tmux'";
    assert_eq!(
        v["error"],
        Value::String(format!("No se pudo abrir la terminal: {message}"))
    );
    assert_eq!(
        (v["code"].clone(), v["retryable"].clone()),
        (Value::String("launch".into()), Value::Bool(true))
    );
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["error", "code", "retryable", "cwd"]);
    assert_eq!(
        rows(&home),
        [(
            "r9".to_owned(),
            "failed".to_owned(),
            Some(message.to_owned())
        )]
    );
    let again = json_of(&quick(front.port, body).await);
    assert_eq!(again["cwd"], v["cwd"], "el reintento reusa la carpeta");
    assert!(fake_scope_calls(&home).is_empty());
    front.stop().await;
}

/// `systemd-run` que no arranca: `[Errno 2] …: 'systemd-run'` como el Python.
#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_scope_missing_matches_python_text() {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new("quick-noscope");
    let mut o = opts(&home);
    o.scope = Some(comandos_server::dash::native::quick::scope_program(
        "/no-existe/systemd-run",
    ));
    let front = front(&home, dead_port(), o).await;
    let got = quick(front.port, r#"{"requestId":"r1","place":"sidebar"}"#).await;
    assert_eq!(got.status, 502, "{}", got.text());
    assert_eq!(
        json_of(&got)["error"],
        json!("No se pudo abrir la terminal: [Errno 2] No such file or directory: 'systemd-run'")
    );
    front.stop().await;
}

/// Carpeta imposible: 500 `folder` con el texto del Python, sin fila.
#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_folder_failure() {
    let home = TestHome::new("quick-folder");
    fs::write(home.root.join("archivo"), "x").unwrap();
    let mut o = opts(&home);
    o.quick_base = home.root.join("archivo/Terminal");
    let front = front(&home, dead_port(), o).await;
    let got = quick(front.port, r#"{"requestId":"r1","place":"sidebar"}"#).await;
    assert_eq!(got.status, 500, "{}", got.text());
    let expected = json!({
        "error": format!(
            "No se pudo crear la carpeta en {}: Not a directory",
            home.root.join("archivo/Terminal").display()
        ),
        "code": "folder",
        "retryable": true,
    });
    assert_eq!(
        got.text(),
        comandos_core::json::response_dumps(&expected).unwrap()
    );
    assert!(rows(&home).is_empty());
    front.stop().await;
}

/// Otra apertura del mismo `requestId` en curso (concesión vigente): tras el
/// plazo de 15 s, 409 `busy`. El reloj avanza un segundo por lectura.
#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_busy_after_wait() {
    let home = TestHome::new("quick-busy");
    let ticks = Arc::new(AtomicU64::new(0));
    let mut o = opts(&home);
    let clock = ticks.clone();
    o.clock_seconds = Arc::new(move || 1_000.0 + clock.fetch_add(1, Ordering::SeqCst) as f64);
    let front = front(&home, dead_port(), o).await;
    // El primer GET abre (y migra) la base.
    assert_eq!(get(front.port, "/workspace").await.status, 200);
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    conn.execute(
        "INSERT INTO quick_terminal_requests VALUES ('r1', 'launching', '/x/T', 'term-qx', 'pane-qx', NULL, 1e12, 0, 0)",
        [],
    )
    .unwrap();
    let got = quick(front.port, r#"{"requestId":"r1","place":"sidebar"}"#).await;
    assert_eq!(
        (got.status, got.text().as_str()),
        (
            409,
            r#"{"error": "La terminal se est\u00e1 abriendo; reintenta en unos segundos", "code": "busy", "retryable": true}"#
        )
    );
    assert!(ticks.load(Ordering::SeqCst) >= 15);
    front.stop().await;
}

const LAUNCH_ORACLE: &str = r#"
import importlib.machinery, importlib.util, json, os, sys, types
repo, sess, cwd, key, pane = sys.argv[1:6]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
calls = []
def run(argv, **kw):
    calls.append(list(argv))
    return types.SimpleNamespace(returncode=0, stdout=pane + "\n", stderr="")
dash.subprocess.run = run
dash.tmux = lambda *a, **k: (calls.append(["tmux", *a]), types.SimpleNamespace(returncode=0, stdout="", stderr=""))[1]
dash.quick_terminal_launch(sess, cwd, key)
print(json.dumps(calls))
"#;

/// R1: el argv del lanzamiento es el de `scope_cmd` + `quick_terminal_launch`
/// del Python (gestor de usuario), y el `set-option` va al pane que tmux dio.
#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_launch_argv_matches_python() {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new("quick-argv");
    // Envoltorio que anota cada llamada a tmux (tras el prefijo privado).
    let log = home.root.join("tmux-calls");
    let wrapper = home.root.join("bin/tmux-log");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexec tmux \"$@\"\n",
            log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(
        &wrapper,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let mut o = opts(&home);
    o.tmux.program.path = wrapper.clone();
    let front = front(&home, dead_port(), o).await;
    let got = quick(front.port, r#"{"requestId":"argv-1","place":"sidebar"}"#).await;
    assert_eq!(got.status, 200, "{}", got.text());
    front.stop().await;
    let v = json_of(&got);
    let (sess, cwd, key) = (
        v["tabId"].as_str().unwrap(),
        v["cwd"].as_str().unwrap(),
        v["paneKey"].as_str().unwrap(),
    );
    let calls = fake_scope_calls(&home);
    assert_eq!(calls.len(), 1);
    let launched = as_python(&home, &calls[0], wrapper.to_str().unwrap());
    let socket = comandos_server::dash::native::tmux::private_socket(&home.tmux_dir());
    let seen = fs::read_to_string(&log).unwrap();
    let set_option = seen
        .lines()
        .find(|l| l.contains("set-option"))
        .expect("set-option tras new-session");
    let pane = set_option
        .split_whitespace()
        .skip_while(|w| *w != "-t")
        .nth(1)
        .unwrap()
        .to_owned();
    assert!(pane.starts_with('%'), "{set_option}");
    let rust_set = set_option
        .strip_prefix(&format!("-f /dev/null -S {} ", socket.display()))
        .expect("set-option con -S privado");
    let Some(out) = support::oracle::run_python(
        LAUNCH_ORACLE,
        &[
            OsStr::new(sess),
            OsStr::new(cwd),
            OsStr::new(key),
            OsStr::new(&pane),
        ],
        &home.root.join("py-home"),
    ) else {
        return;
    };
    let python: Vec<Vec<String>> = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(python.len(), 2, "{python:?}");
    let mut expected_launch = python[0].clone();
    assert_eq!(expected_launch.remove(0), "systemd-run");
    assert_eq!(launched, expected_launch);
    assert_eq!(format!("tmux {rust_set}"), python[1].join(" "));
}

/// tmux que tarda 1 s en `new-session` (conserva el `-f /dev/null -S
/// <socket>` privado que antepone `Tmux::private`).
fn slow_tmux(home: &TestHome) -> std::path::PathBuf {
    let wrapper = home.root.join("bin/tmux-lento");
    fs::write(
        &wrapper,
        "#!/bin/sh\ncase \"$*\" in *new-session*) sleep 1;; esac\nexec tmux \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(
        &wrapper,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    wrapper
}

fn pane_key(home: &TestHome, tab: &str) -> String {
    let out = home
        .tmux_command()
        .args([
            "display-message",
            "-p",
            "-t",
            &format!("={tab}:"),
            "#{@comandos-pane-key}",
        ])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Revisión de la Tarea 6: si el worker rechaza el cierre después de lanzar,
/// es la excepción de `_finish` (500), nunca un reenvío al Python (que daría
/// 409 tras 15 s con la fila en `launching`). Una sola shell.
#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_finish_refused_is_500_not_forwarded() {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new("quick-finish-refused");
    let legacy = FakeLegacy::start().await;
    let mut o = opts(&home);
    o.tmux.program.path = slow_tmux(&home);
    let front = front(&home, legacy.port, o).await;
    // El primer GET abre (y migra) la base.
    assert_eq!(get(front.port, "/workspace").await.status, 200);
    let port = front.port;
    let pending =
        tokio::spawn(
            async move { quick(port, r#"{"requestId":"req-fin","place":"sidebar"}"#).await },
        );
    // Reclamo hecho y tmux dentro de su segundo de `new-session`: la puerta
    // del worker falla en el cierre (tabla de versiones apartada un momento).
    tokio::time::sleep(Duration::from_millis(400)).await;
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    conn.execute_batch("ALTER TABLE schema_migrations RENAME TO schema_migrations_aside")
        .unwrap();
    let got = pending.await.unwrap();
    conn.execute_batch("ALTER TABLE schema_migrations_aside RENAME TO schema_migrations")
        .unwrap();
    assert_eq!(
        (got.status, got.text().as_str()),
        (500, r#"{"error": "Error interno del tablero"}"#)
    );
    assert!(legacy.requests().is_empty(), "{:?}", legacy.requests());
    assert_eq!(fake_scope_calls(&home).len(), 1, "una sola shell");
    front.stop().await;
}

/// Revisión de la Tarea 6: un cliente que se va a mitad del lanzamiento no
/// cancela el trabajo. La shell se abre una vez, la llave queda en el pane, la
/// fila pasa a `ready` y el reintento la encuentra lista.
#[tokio::test(flavor = "current_thread")]
async fn quick_terminal_abandoned_request_still_finishes() {
    use comandos_server::{
        Request,
        dash::native::{Native, NativeRoute, Outcome},
    };
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new("quick-abandon");
    let mut o = opts(&home);
    o.tmux.program.path = slow_tmux(&home);
    let native = Arc::new(Native::new(o));
    let body = r#"{"requestId":"req-suelta","place":"sidebar"}"#;
    let request = Request {
        method: http::Method::POST,
        target: "/terminal/quick".into(),
        peer: "127.0.0.1:12345".parse().unwrap(),
        headers: vec![],
        data: Some(serde_json::from_str(body).unwrap()),
        body: bytes::Bytes::copy_from_slice(body.as_bytes()),
        internal_producer: false,
    };
    let cut = timeout(
        Duration::from_millis(400),
        native.dispatch(NativeRoute::QuickTerminal, &request),
    )
    .await;
    assert!(cut.is_err(), "la petición se soltó a mitad del lanzamiento");
    let mut ready = false;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if rows(&home)
            .iter()
            .any(|(id, state, _)| id == "req-suelta" && state == "ready")
        {
            ready = true;
            break;
        }
    }
    assert!(ready, "la fila quedó en {:?}", rows(&home));
    assert_eq!(fake_scope_calls(&home).len(), 1, "una sola shell");
    let replay = match native
        .dispatch(NativeRoute::QuickTerminal, &request)
        .await
        .unwrap()
    {
        Outcome::Reply(reply) => reply,
        Outcome::Decline => panic!("el reintento declinó"),
    };
    assert_eq!(replay.status, http::StatusCode::OK);
    let v: Value = match &replay.body {
        comandos_server::ReplyBody::Bytes(bytes) => serde_json::from_slice(bytes).unwrap(),
        _ => panic!("cuerpo inesperado"),
    };
    assert_eq!(v["created"], Value::Bool(false));
    let tab = v["tabId"].as_str().unwrap();
    assert_eq!(pane_key(&home, tab), v["paneKey"].as_str().unwrap());
    assert_eq!(fake_scope_calls(&home).len(), 1);
    native.shutdown().await;
}
