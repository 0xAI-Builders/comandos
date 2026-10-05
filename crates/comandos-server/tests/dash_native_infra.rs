//! Infraestructura de la Fase 2b: flag `--no-native`, ruta de la base,
//! puerta de esquema y clase `Native` del enrutador.
use comandos_server::dash::{
    DEFAULT_LEGACY_PORT, build,
    native::{
        Fault, Native, NativeOptions,
        state::{Refusal, StateBackend},
        tmux::Program,
    },
    parse_args,
    router::{classify, classify_with},
    trace_line,
};
use http::Method;
use std::{fs, path::PathBuf, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn root(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("cmd-native-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    base
}

#[test]
fn native_is_on_by_default_and_off_with_flag() {
    let home = PathBuf::from("/tmp/no-existe-home");
    let cfg = parse_args(&[], &home, None).unwrap();
    assert!(cfg.native);
    assert!(!cfg.trace_forward);
    assert_eq!(
        cfg.state_db,
        home.join(".local/state/comandos/app-state.sqlite3")
    );
    let cfg = parse_args(&["--no-native".into()], &home, None).unwrap();
    assert!(!cfg.native);
    assert_eq!(cfg.port, 4777);
    assert_eq!(cfg.legacy_port, DEFAULT_LEGACY_PORT);
}

#[test]
fn classify_with_native_off_is_phase_2a() {
    let exists = |p: &str| p == "/index.html";
    for (method, target) in [
        (Method::GET, "/"),
        (Method::GET, "/prefs"),
        (Method::GET, "/notices?after=3"),
        (Method::POST, "/presence"),
        (Method::HEAD, "/index.html"),
        (Method::GET, "/no-existe"),
    ] {
        assert_eq!(
            classify_with(&method, target, &exists, false),
            classify(&method, target, &exists),
            "{method} {target}"
        );
    }
}

#[test]
fn trace_line_omits_query() {
    assert_eq!(
        trace_line(&Method::GET, "/notices?deviceId=secreto"),
        "comandos dash: reenvío GET /notices"
    );
}

fn bump_schema(db: &std::path::Path, version: i64) {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute(
        "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?, 'futuro', 0)",
        [version],
    )
    .unwrap();
}

#[test]
fn state_backend_migrates_then_refuses_newer_schema() {
    let base = root("gate");
    let db = base.join("state/app-state.sqlite3");
    let backend = StateBackend::open(&db, 1_791_115_200.0).unwrap();
    backend.admit().unwrap();
    let known = *comandos_server::dash::native::state::known_versions()
        .iter()
        .max()
        .unwrap();
    bump_schema(&db, known + 1);
    assert_eq!(
        backend.admit(),
        Err(Refusal::Newer {
            found: known + 1,
            known
        })
    );
    drop(backend);
    assert!(matches!(
        StateBackend::open(&db, 1_791_115_200.0),
        Err(Refusal::Newer { .. })
    ));
    let _ = fs::remove_dir_all(&base);
}

fn options(base: &std::path::Path) -> NativeOptions {
    let mut opts = NativeOptions::for_home(base, base.join("state/app-state.sqlite3"));
    opts.clock = Arc::new(|| 1_791_115_200_000);
    // Nunca el tmux ni el ssh reales (`for_home` los busca en el PATH): una
    // ruta que los necesitara falla en vez de tocar las sesiones del usuario.
    opts.tmux.program = Program::named("/no-existe/tmux");
    opts.ssh = Program::named("/no-existe/ssh");
    opts
}

#[tokio::test]
async fn schema_newer_while_live_forwards_once() {
    let base = root("live");
    let native = Native::new(options(&base));
    assert!(native.ready().await);
    let known = *comandos_server::dash::native::state::known_versions()
        .iter()
        .max()
        .unwrap();
    bump_schema(&base.join("state/app-state.sqlite3"), known + 1);
    // El primer trabajo descubre el esquema nuevo y apaga todo el conjunto.
    assert!(native.with_state(|_| ()).await.is_err());
    assert!(!native.enabled());
    assert!(native.with_state(|_| ()).await.is_err());
    assert!(!native.ready().await);
    assert_eq!(native.refusals(), 1, "una sola línea en stderr");
    native.shutdown().await;
    let _ = fs::remove_dir_all(&base);
}

/// Un fallo de la puerta que no dice nada del esquema (aquí, la tabla de
/// versiones ausente un momento; en vivo, un `SQLITE_BUSY`) declina esa
/// petición sin apagar el conjunto: la siguiente se responde en nativo.
#[tokio::test]
async fn transient_gate_error_declines_once_and_native_stays() {
    let base = root("transient-gate");
    let native = Native::new(options(&base));
    assert!(native.ready().await);
    let db = base.join("state/app-state.sqlite3");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("ALTER TABLE schema_migrations RENAME TO schema_migrations_aside")
        .unwrap();
    assert!(matches!(
        native.with_state(|_| ()).await,
        Err(Fault::Decline)
    ));
    assert!(native.enabled(), "un fallo pasajero no apaga el conjunto");
    assert_eq!(native.refusals(), 0, "sin línea de apagado");
    conn.execute_batch("ALTER TABLE schema_migrations_aside RENAME TO schema_migrations")
        .unwrap();
    assert!(matches!(native.with_state(|_| 7).await, Ok(7)));
    assert!(native.ready().await);
    native.shutdown().await;
    let _ = fs::remove_dir_all(&base);
}

#[tokio::test]
async fn unopenable_database_disables_native() {
    let base = root("unopen");
    // Un directorio donde va el archivo: no se puede abrir como base.
    fs::create_dir_all(base.join("state/app-state.sqlite3")).unwrap();
    let native = Native::new(options(&base));
    assert!(!native.ready().await);
    assert!(!native.enabled());
    assert_eq!(native.refusals(), 1);
    let _ = fs::remove_dir_all(&base);
}

/// Un pánico en un trabajo de la base retira el worker: esa petición es la
/// única que recibe el fallo; las siguientes se declinan (y el frente las
/// reenvía), con una sola línea en stderr y sin volver a abrir la base.
#[tokio::test]
async fn panicked_job_fails_once_then_native_declines() {
    let base = root("panic");
    let native = Native::new(options(&base));
    assert!(native.ready().await);
    let boom = native
        .with_state(|_| -> u8 { panic!("estado inconsistente") })
        .await;
    assert!(matches!(
        boom,
        Err(Fault::Error(comandos_server::HandlerError::Failure))
    ));
    assert!(!native.enabled());
    assert_eq!(native.refusals(), 1, "una sola línea en stderr");
    for _ in 0..2 {
        assert!(matches!(
            native.with_state(|_| ()).await,
            Err(Fault::Decline)
        ));
    }
    assert!(!native.ready().await);
    assert_eq!(native.refusals(), 1);
    native.shutdown().await;
    let _ = fs::remove_dir_all(&base);
}

/// Heredado falso: responde `{"legacy": true}` a una petición y cierra.
async fn fake_legacy() -> (u16, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut seen = Vec::new();
        let mut chunk = [0u8; 1024];
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut chunk).await.unwrap();
            assert!(n > 0, "petición incompleta");
            seen.extend_from_slice(&chunk[..n]);
        }
        let body = r#"{"legacy": true}"#;
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(body.as_bytes()).await.unwrap();
        String::from_utf8_lossy(&seen).into_owned()
    });
    (port, task)
}

#[tokio::test]
async fn front_forwards_after_worker_panic() {
    let base = root("panic-front");
    fs::create_dir_all(base.join("dash")).unwrap();
    let (legacy_port, legacy) = fake_legacy().await;
    let mut cfg = parse_args(&[], &base, Some(&legacy_port.to_string())).unwrap();
    cfg.dash_dir = base.join("dash");
    cfg.token = b"token-de-prueba".to_vec();
    let (config, native) = build(cfg, Some(options(&base)));
    let native = native.unwrap();
    assert!(native.ready().await);
    let boom = native.with_state(|_| -> u8 { panic!("fallo") }).await;
    assert!(matches!(boom, Err(Fault::Error(_))));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (stop, shutdown) = tokio::sync::watch::channel(false);
    let front = tokio::spawn(comandos_server::serve(listener, config, shutdown));
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream
        .write_all(
            format!("GET /prefs HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut wire = String::new();
    stream.read_to_string(&mut wire).await.unwrap();
    assert!(wire.starts_with("HTTP/1.1 200"), "{wire}");
    assert!(wire.ends_with(r#"{"legacy": true}"#), "{wire}");
    assert!(legacy.await.unwrap().starts_with("GET /prefs HTTP/1.1"));
    assert_eq!(native.refusals(), 1, "una sola línea en stderr");

    stop.send(true).unwrap();
    front.await.unwrap().unwrap();
    native.shutdown().await;
    let _ = fs::remove_dir_all(&base);
}
