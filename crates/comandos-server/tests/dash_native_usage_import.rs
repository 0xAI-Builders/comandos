//! Dueño único de los efectos de GET /usage/state (D1, Tarea 8 de la 2e): la
//! ruta es nativa, importa tras la gracia y cada 60 s, salta la vuelta si otro
//! frente tiene el candado, no hace nada sin efectos de uso o con el carril de
//! uso apagado, no sube la generación del memo si la importación falla, no
//! escribe bordes si la recolección de la 2d declinó, y registra las
//! configuraciones observadas como `ensure_observed_configs` del Python.
mod support;
use comandos_server::{
    Request,
    dash::native::{
        Native, NativeOptions, NativeRoute, Outcome,
        files::FileLock,
        tmux::{Program, Tmux},
        usage::{UsageRoute, state},
    },
};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};
use support::{FakeLegacy, NOW_MS, TestHome, front, get, oracle::run_python, seed_usage};

/// Una línea de transcript de Claude con uso, fechada 1 h antes del reloj fijo.
fn claude_line(home: &TestHome, id: &str) {
    let dir = home.root.join(".claude/projects/-r-a");
    std::fs::create_dir_all(&dir).unwrap();
    let at = chrono::DateTime::from_timestamp(NOW_MS / 1000 - 3600, 0)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ");
    std::fs::write(
        dir.join(format!("{id}.jsonl")),
        format!(
            "{{\"type\":\"assistant\",\"timestamp\":\"{at}\",\"cwd\":\"/r/a\",\"sessionId\":\"s\",\
             \"message\":{{\"id\":\"{id}\",\"model\":\"claude-fable-5\",\"usage\":{{\"input_tokens\":5,\"output_tokens\":7}}}}}}\n"
        ),
    )
    .unwrap();
}

fn turns(home: &TestHome) -> i64 {
    let conn = rusqlite::Connection::open(home.usage_db()).unwrap();
    conn.query_row("select count(*) from usage_turns", [], |r| r.get(0))
        .unwrap()
}

async fn wait_turns(home: &TestHome, want: i64) -> bool {
    for _ in 0..200 {
        if turns(home) == want {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    false
}

/// Espera a que no haya importación en vuelo.
async fn settle(native: &Native) {
    for _ in 0..300 {
        if !native.import_owner().running() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("la importación no terminó");
}

fn request(target: &str) -> Request {
    Request {
        method: http::Method::GET,
        target: target.into(),
        peer: "127.0.0.1:12345".parse().unwrap(),
        headers: vec![],
        data: None,
        body: bytes::Bytes::new(),
        internal_producer: false,
    }
}

/// GET /usage/state directo al conjunto nativo: estado HTTP, o `None` si declinó.
async fn usage_state(native: &Arc<Native>) -> Option<u16> {
    match native
        .dispatch(
            NativeRoute::Usage(UsageRoute::State),
            &request("/usage/state"),
        )
        .await
    {
        Ok(Outcome::Reply(reply)) => Some(reply.status.as_u16()),
        Ok(Outcome::Decline) => None,
        Err(_) => Some(500),
    }
}

/// `tmux` que siempre sale con 1: ninguna prueba alcanza un servidor tmux.
fn no_tmux(opts: &mut NativeOptions) {
    opts.tmux = Tmux {
        program: Program::named("/bin/false"),
        timeout: Duration::from_secs(2),
    };
}

#[tokio::test]
async fn usage_state_is_native_and_imports_after_grace() {
    let home = TestHome::new("import-grace");
    seed_usage(&home, "");
    claude_line(&home, "m1");
    let mut opts = home.options();
    opts.usage_import_grace_ms = 0;
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, opts).await;
    let wire = get(front.port, "/usage/state").await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert!(
        wire.text().starts_with("{\"generated_at\": "),
        "cuerpo del frente: {}",
        wire.text()
    );
    assert!(legacy.requests().is_empty(), "el Python no ve /usage/state");
    assert!(
        wait_turns(&home, 1).await,
        "la importación del frente escribió el turno"
    );
    // `/usage/stateX` sigue siendo del Python.
    let _ = get(front.port, "/usage/stateX").await;
    assert_eq!(legacy.requests().len(), 1);
    front.stop().await;
}

#[tokio::test]
async fn import_lock_contended_skips_cycle() {
    let home = TestHome::new("import-lock");
    seed_usage(&home, "");
    claude_line(&home, "m2");
    let held = FileLock::try_acquire(&home.hooks().join("comandos-usage-import.lock"))
        .unwrap()
        .unwrap();
    let mut opts = home.options();
    opts.usage_import_grace_ms = 0;
    let front = front(&home, support::dead_port(), opts).await;
    assert_eq!(get(front.port, "/usage/state").await.status, 200);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        turns(&home),
        0,
        "otro frente tiene el candado: esta vuelta no importa"
    );
    drop(held);
    front.stop().await;
}

/// La ruta apagada (`USAGE_STATE_NATIVE = false`): GET /usage/state va al
/// Python y el frente no hace ninguno de sus efectos (ni importa ni registra
/// panes; el refresco de límites al arrancar lo cubre `dash_native_limits`).
#[tokio::test]
async fn usage_state_off_forwards_without_effects() {
    let home = TestHome::new("import-off");
    seed_usage(&home, "");
    claude_line(&home, "m0");
    let mut opts = home.options();
    assert!(
        NativeOptions::for_home(&home.root, home.state_db()).usage_state_native,
        "encendido en producción"
    );
    opts.usage_state_native = false;
    opts.usage_import_grace_ms = 0;
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, opts).await;
    assert_eq!(
        get(front.port, "/usage/state").await.text(),
        r#"{"legacy": true}"#
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(turns(&home), 0, "sin importación del frente");
    let conn = rusqlite::Connection::open(home.usage_db()).unwrap();
    let panes: i64 = conn
        .query_row("select count(*) from usage_panes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(panes, 0, "sin record_pane");
    front.stop().await;
}

#[tokio::test]
async fn usage_lane_down_hands_effects_back() {
    let home = TestHome::new("import-handover");
    seed_usage(&home, "pragma user_version = 12;");
    claude_line(&home, "m3");
    let mut opts = home.options();
    opts.usage_import_grace_ms = 0;
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, opts).await;
    assert_eq!(
        get(front.port, "/usage/state").await.text(),
        r#"{"legacy": true}"#
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        turns(&home),
        0,
        "con el carril apagado el frente no importa"
    );
    assert!(
        !home.hooks().join("pane-models.txt").exists(),
        "ni escribe bordes"
    );
    front.stop().await;
}

#[tokio::test]
async fn no_usage_effects_writes_nothing() {
    let home = TestHome::new("import-shadow");
    seed_usage(&home, "");
    claude_line(&home, "m4");
    let mut opts = home.options();
    opts.usage_effects = false;
    opts.usage_import_grace_ms = 0;
    let front = front(&home, support::dead_port(), opts).await;
    assert_eq!(get(front.port, "/usage/state").await.status, 200);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(turns(&home), 0);
    assert!(!home.hooks().join("pane-models.txt").exists());
    front.stop().await;
}

/// Gracia de 75 s desde el arranque y luego una vuelta cada 60 s como mucho.
#[tokio::test]
async fn grace_and_interval_gate_imports() {
    let home = TestHome::new("import-interval");
    seed_usage(&home, "");
    claude_line(&home, "m5");
    let clock = Arc::new(AtomicI64::new(NOW_MS));
    let mut opts = home.options();
    no_tmux(&mut opts);
    let c = clock.clone();
    opts.clock = Arc::new(move || c.load(Ordering::Acquire));
    assert_eq!(opts.usage_import_grace_ms, 75_000);
    let native = Arc::new(Native::new(opts));
    let generation = || native.usage_engine.generation();
    clock.store(NOW_MS + 74_000, Ordering::Release);
    assert_eq!(usage_state(&native).await, Some(200));
    settle(&native).await;
    assert_eq!((turns(&home), generation()), (0, 0), "dentro de la gracia");
    clock.store(NOW_MS + 76_000, Ordering::Release);
    assert_eq!(usage_state(&native).await, Some(200));
    settle(&native).await;
    assert_eq!((turns(&home), generation()), (1, 1), "pasada la gracia");
    claude_line(&home, "m6");
    clock.store(NOW_MS + 135_000, Ordering::Release);
    assert_eq!(usage_state(&native).await, Some(200));
    settle(&native).await;
    assert_eq!(
        (turns(&home), generation()),
        (1, 1),
        "a 59 s de la anterior"
    );
    clock.store(NOW_MS + 136_000, Ordering::Release);
    assert_eq!(usage_state(&native).await, Some(200));
    settle(&native).await;
    assert_eq!((turns(&home), generation()), (2, 2), "a 60 s");
    assert!(
        native.import_owner().seen_len() <= 2,
        "seen acotado al corte"
    );
    native.shutdown().await;
}

#[tokio::test]
async fn import_failure_keeps_generation() {
    let home = TestHome::new("import-failure");
    seed_usage(&home, "");
    claude_line(&home, "m7");
    let mut opts = home.options();
    no_tmux(&mut opts);
    opts.usage_import_grace_ms = 0;
    let mut env = std::collections::BTreeMap::new();
    env.insert("COMANDOS_USAGE_LOCAL_DAYS".to_owned(), "x".to_owned());
    opts.usage_env = Arc::new(env);
    let native = Arc::new(Native::new(opts));
    assert_eq!(usage_state(&native).await, Some(200));
    settle(&native).await;
    assert_eq!(
        native.usage_engine.generation(),
        0,
        "int('x') mataba el hilo"
    );
    assert_eq!(turns(&home), 0);
    native.shutdown().await;
}

/// `tmux` falso que anota cada argv en `fakebin/argv`; `list-panes` con
/// `@ccmodel` imprime `fakebin/options`; lo demás no imprime nada.
fn argv_tmux(home: &TestHome) -> (Tmux, PathBuf) {
    let dir = home.root.join("fakebin");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tmux");
    std::fs::write(
        &path,
        "#!/bin/sh\n\
         d=$(dirname \"$0\")\n\
         for a in \"$@\"; do printf '%s\\n' \"$a\" >> \"$d/argv\"; done\n\
         printf -- '--\\n' >> \"$d/argv\"\n\
         case \"$1\" in\n\
         list-panes) case \"$4\" in *ccmodel*) cat \"$d/options\" ;; esac ;;\n\
         esac\n\
         exit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("options"), "%5\tviejo\n").unwrap();
    let tmux = Tmux {
        program: Program::named(&path),
        timeout: Duration::from_secs(5),
    };
    assert!(tmux.program.path.starts_with(&home.root));
    (tmux, dir)
}

/// Revisión de T7b: si la recolección de la 2d declina, `live_panes` va vacía
/// sin serlo y esa vuelta NO toca los bordes (con la lista vacía los quitaría
/// de todos los panes). Las tarjetas de `/state` ya están en caché antes de
/// romper la recolección: lo único que falta es la recolección de la vuelta.
/// Control: sin el declinar, el borde viejo de `%5` (que no es un panel vivo)
/// se quita con `set-option -u`.
#[tokio::test]
async fn gather_decline_applies_no_borders() {
    for declined in [false, true] {
        let home = TestHome::new(if declined {
            "import-decline"
        } else {
            "import-borders"
        });
        seed_usage(&home, "");
        let (tmux, fake) = argv_tmux(&home);
        let mut opts = home.options();
        opts.tmux = tmux;
        let native = Arc::new(Native::new(opts));
        // Las tarjetas de `/state` en caché (el reloj fijo las mantiene frescas).
        assert!(native.states_cached().await.is_ok(), "tarjetas de /state");
        if declined {
            // `app-tabs.json` que no es UTF-8: la recolección de la 2d declina.
            std::fs::write(home.hooks().join("app-tabs.json"), b"{\"\xff\": 1}").unwrap();
        }
        assert!(native.states_cached().await.is_ok(), "siguen en caché");
        // La señal que la ruta mira.
        let Ok(reply) = state::compute(&native).await else {
            panic!("compute declinó");
        };
        assert_eq!(reply.live_declined, declined);
        assert_eq!(usage_state(&native).await, Some(200));
        for _ in 0..100 {
            if !native.pane_models().running() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let argv = std::fs::read_to_string(fake.join("argv")).unwrap_or_default();
        if declined {
            assert!(!argv.contains("set-option"), "{argv}");
            assert!(!argv.contains("@ccmodel"), "ni siquiera se leen: {argv}");
            assert!(!home.hooks().join("pane-models.txt").exists());
        } else {
            assert!(
                argv.contains("set-option\n-p\n-u\n-t\n%5\n@ccmodel\n"),
                "{argv}"
            );
            assert!(home.hooks().join("pane-models.txt").exists());
        }
        native.shutdown().await;
    }
}

/// `ensure_observed_configs` (cc-dash:315) contra el Python: tarjetas de
/// `/state` con y sin configuración previa, cambios de esfuerzo y de ruta, y
/// las que no cuentan; dos vueltas (la segunda no escribe nada).
#[tokio::test]
async fn observed_configs_match_python() {
    let home = TestHome::new("import-observed");
    let cards = json!([
        {"alive": true, "model": "claude-fable-5", "session": "s1", "pane": "%1", "agent": "claude",
         "effort": "high", "account": "relotto"},
        {"alive": true, "model": "gpt-5.5", "session": "s2", "pane": "%2", "harness": "codex", "motor": "codex",
         "routeId": "codex:codex", "harnessAccount": "work", "motorAccount": "work"},
        {"alive": true, "model": "grok-5", "session": "s3", "pane": "%3", "agent": "grok"},
        {"alive": false, "model": "m", "session": "s4", "pane": "%4", "agent": "claude"},
        {"alive": true, "model": "", "session": "s5", "pane": "%5", "agent": "claude"},
        {"alive": true, "model": "m", "session": "", "pane": "%6", "agent": "claude"},
        {"alive": true, "model": "m", "session": "s7", "pane": "%7"},
        {"alive": true, "model": "claude-fable-5", "session": "s8", "pane": "%8", "agent": "claude", "effort": "low"}
    ]);
    let seed = format!(
        "insert into usage_session_configs values('prev3','s3','%3',{at},'grok','grok','grok-5','','main','main',\
         'grok:grok','runtime','exact');\
         insert into usage_session_configs values('prev8','s8','%8',{at},'claude','claude','claude-fable-5','high',\
         'main','main','claude:claude','runtime','exact');",
        at = NOW_MS / 1000 - 100
    );
    let ours = home.root.join("ours.sqlite");
    let theirs = home.hooks().join("comandos-usage.sqlite");
    for db in [&ours, &theirs] {
        let conn = comandos_store::usage::open_usage_db_at(db).unwrap();
        comandos_store::usage::ensure_schema(&conn).unwrap();
        conn.execute_batch(&seed).unwrap();
    }
    let script = r#"
import importlib.machinery, importlib.util, json, os, sys
import urllib.request
def _no_net(*a, **k):
    raise OSError("sin red en la prueba")
urllib.request.urlopen = _no_net
repo = sys.argv[1]
sys.path.insert(0, os.path.join(repo, "bin"))
sys.path.insert(0, os.path.join(repo, "lib"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
cards = json.loads(sys.argv[2])
dash.USAGE_DB = sys.argv[3]
dash.time.time = lambda: int(sys.argv[4])
dash.read_states_cached = lambda: cards
print(json.dumps([dash.ensure_observed_configs(), dash.ensure_observed_configs()]))
"#;
    let now = NOW_MS / 1000;
    let Some(out) = run_python(
        script,
        &[
            OsStr::new(&cards.to_string()),
            theirs.as_os_str(),
            OsStr::new(&now.to_string()),
        ],
        &home.root,
    ) else {
        return;
    };
    let conn = comandos_store::usage::open_usage_db_at(&ours).unwrap();
    let cards = cards.as_array().unwrap();
    let first = comandos_store::usage_import::ensure_observed_configs(&conn, cards, now).unwrap();
    let second = comandos_store::usage_import::ensure_observed_configs(&conn, cards, now).unwrap();
    let theirs_counts: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(json!([first, second]), theirs_counts);
    let dump = |db: &std::path::Path| {
        let conn = rusqlite::Connection::open(db).unwrap();
        let mut stmt = conn
            .prepare("select * from usage_session_configs order by 1")
            .unwrap();
        let n = stmt.column_count();
        stmt.query_map([], |r| {
            Ok((0..n)
                .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                .collect::<Vec<_>>()
                .join("|"))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };
    let rows = dump(&ours);
    assert_eq!(rows.len(), 5, "{rows:#?}");
    assert_eq!(rows, dump(&theirs));
    // Una tarjeta que no es objeto lanza (`s.get`): se omite también la reconciliación.
    assert!(
        comandos_store::usage_import::ensure_observed_configs(&conn, &[json!(1)], now).is_err()
    );
}
