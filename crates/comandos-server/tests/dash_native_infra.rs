//! Infraestructura de la Fase 2b: flag `--no-native`, ruta de la base,
//! puerta de esquema y clase `Native` del enrutador.
use comandos_server::dash::{
    DEFAULT_LEGACY_PORT,
    native::{
        Native, NativeOptions,
        state::{Refusal, StateBackend},
    },
    parse_args,
    router::{classify, classify_with},
    trace_line,
};
use http::Method;
use std::{fs, path::PathBuf, sync::Arc};

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
