//! Dominio A: avisos, campana, presencia, sonido y long-poll por el frente.
mod support;

use comandos_server::dash::native::tmux::{Program, Tmux};
use serde_json::{Value, json};
use std::{
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
use support::{
    FakeLegacy, TestHome, dead_port, front, get, http_golden::FrozenHttp, request_body,
    tmux_available,
};

// Expected output only: never replay a listener, waiter, PID or log.
fn freeze_watch(target: &str, source: Option<support::Wire>) -> (u16, String) {
    let bytes = comandos_oracle::oracle_at(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        "notices-watch",
        &json!({"source":support::frozen::SOURCE_COMMIT,
            "python":"3.10.12", "clock_ms":support::NOW_MS, "fixture_timer":"_do_GET monotonic; document timestamps fixed", "target":target}),
        || {
            let wire = source.ok_or("original long-poll required in record/check")?;
            serde_json::to_vec(&(wire.status, wire.text())).map_err(|e| e.to_string())
        },
    );
    serde_json::from_slice(&bytes).unwrap()
}

fn seed_shared_native_startup(home: &TestHome) {
    let conn = comandos_runtime::open_state(&home.state_db(), 5000).unwrap();
    // Native background startup ensures policy_v1 in the shared HOME.
    // Preseed that deterministic startup input before oracle capture.
    comandos_store::focus::ensure_policy(
        &conn,
        &comandos_core::focus::policy_v1(),
        support::NOW_MS,
    )
    .unwrap();
}

fn seed_event(home: &TestHome, id: &str, kind: &str, extra: Value) {
    let conn = comandos_runtime::open_state(&home.state_db(), 5000).unwrap();
    // Schema creation belongs to this private fixture, not the notice event.
    conn.execute(
        "UPDATE schema_migrations SET applied_at=?",
        [support::NOW_MS as f64 / 1000.0],
    )
    .unwrap();
    let mut event = json!({"eventId": id, "source": "test", "kind": kind,
            "evidence": "confirmed", "correlation": "local", "sessionKey": "s1",
            "occurredAtMs": 1_791_115_100_000_u64, "receivedAtMs": 1_791_115_100_000_u64});
    for (key, value) in extra.as_object().unwrap() {
        event[key] = value.clone();
    }
    comandos_store::append_event(&conn, &event, 0, "generated", &format!("r-{id}")).unwrap();
}

fn seed_permission(home: &TestHome, id: &str) {
    seed_event(home, id, "permission_requested", json!({}));
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[tokio::test]
async fn notices_list_count_read_presence_sound_prefs() {
    let home = TestHome::new("notices");
    seed_permission(&home, "e1");
    let front = front(&home, dead_port(), home.options()).await;
    let list = get(front.port, "/notices?deviceId=d1").await;
    assert_eq!(list.status, 200);
    let page = parse(&list.text());
    let keys: Vec<&str> = page
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "notices",
            "nextAfter",
            "pending",
            "prefs",
            "focusActive",
            "badge"
        ]
    );
    assert_eq!(page["notices"][0]["eventId"], "e1");
    assert_eq!(page["badge"], 1);
    let bad = get(front.port, "/notices?after=x").await;
    assert_eq!(
        (bad.status, bad.text().as_str()),
        (
            400,
            r#"{"error": "invalid literal for int() with base 10: 'x'"}"#
        )
    );
    let bad = get(front.port, "/notices?after=1&limit=1.5").await;
    assert_eq!(
        bad.text(),
        r#"{"error": "invalid literal for int() with base 10: '1.5'"}"#
    );
    assert_eq!(
        get(front.port, "/notifs/count").await.text(),
        r#"{"count": 1}"#
    );
    assert_eq!(get(front.port, "/notices/prefs").await.status, 200);
    // `json.dumps` del Python escapa lo no ASCII (`ensure_ascii`).
    let presence = request_body(front.port, "POST", "/presence", "", r#"{"deviceId": ""}"#).await;
    assert_eq!(
        (presence.status, presence.text().as_str()),
        (400, r#"{"error": "deviceId inv\u00e1lido"}"#)
    );
    let presence = request_body(
        front.port,
        "POST",
        "/presence",
        "",
        r#"{"deviceId": "d1", "visible": true, "canPlayAudio": true, "kind": "desktop"}"#,
    )
    .await;
    assert_eq!(presence.text(), r#"{"ok": true}"#);
    let sound = request_body(
        front.port,
        "POST",
        "/notices/sound",
        "",
        r#"{"eventId": "e1", "deviceId": "d1"}"#,
    )
    .await;
    assert_eq!(sound.status, 200);
    assert!(parse(&sound.text()).get("play").is_some());
    let bad = request_body(
        front.port,
        "POST",
        "/notices/read",
        "",
        r#"{"eventIds": "e1"}"#,
    )
    .await;
    assert_eq!(
        (bad.status, bad.text().as_str()),
        (400, r#"{"error": "eventIds inv\u00e1lido"}"#)
    );
    let read = request_body(
        front.port,
        "POST",
        "/notices/read",
        "",
        r#"{"eventIds": ["e1", 3, ""]}"#,
    )
    .await;
    assert_eq!(read.text(), r#"{"ok": true, "read": ["e1"]}"#);
    assert_eq!(
        get(front.port, "/notifs/count").await.text(),
        r#"{"count": 0}"#
    );
    seed_permission(&home, "e2");
    let all = request_body(front.port, "POST", "/notices/read", "", r#"{"all": true}"#).await;
    assert_eq!(all.text(), r#"{"ok": true, "read": ["e2"]}"#);
    let prefs = request_body(front.port, "POST", "/notices/prefs", "", "{}").await;
    assert_eq!(prefs.status, 200);
    assert_eq!(prefs.text(), get(front.port, "/notices/prefs").await.text());
    front.stop().await;
}

#[tokio::test]
async fn watch_returns_when_revision_changes() {
    let home = TestHome::new("watch");
    seed_permission(&home, "e1");
    let front = front(&home, dead_port(), home.options()).await;
    let now = parse(&get(front.port, "/notices/watch?wait=0").await.text());
    assert_eq!(now["unread"], json!(["e1"]));
    let rev = now["rev"].as_str().unwrap().to_owned();
    let port = front.port;
    let started = Instant::now();
    let waiting =
        tokio::spawn(async move { get(port, &format!("/notices/watch?rev={rev}&wait=10")).await });
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        !waiting.is_finished(),
        "sin cambios la petición sigue abierta"
    );
    request_body(
        front.port,
        "POST",
        "/notices/read",
        "",
        r#"{"eventIds": ["e1"]}"#,
    )
    .await;
    let woke = parse(&waiting.await.unwrap().text());
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(woke["unread"], json!([]));
    assert_eq!(woke["badge"], 0);
    let keys: Vec<&str> = woke
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["rev", "badge", "unread", "pending", "latest"]);
    // `wait` inválido es 25 s; negativo es 0.
    let quick = Instant::now();
    get(front.port, "/notices/watch?rev=otra&wait=-5").await;
    assert!(quick.elapsed() < Duration::from_secs(1));
    front.stop().await;
}

#[tokio::test]
async fn watch_storm_keeps_worker_responsive() {
    let home = TestHome::new("watch-storm");
    seed_permission(&home, "e1");
    let front = front(&home, dead_port(), home.options()).await;
    let rev = parse(&get(front.port, "/notices/watch?wait=0").await.text())["rev"]
        .as_str()
        .unwrap()
        .to_owned();
    let port = front.port;
    let watchers: Vec<_> = (0..50)
        .map(|_| {
            let rev = rev.clone();
            tokio::spawn(
                async move { get(port, &format!("/notices/watch?rev={rev}&wait=5")).await },
            )
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(500)).await;
    for _ in 0..5 {
        let t = Instant::now();
        assert_eq!(get(port, "/notifs/count").await.status, 200);
        assert!(
            t.elapsed() < Duration::from_secs(1),
            "la campana no espera a los long-polls"
        );
    }
    // Como en el Python, la presencia no cambia la revisión: nadie despierta.
    let presence = request_body(
        port,
        "POST",
        "/presence",
        "",
        r#"{"deviceId": "d1", "visible": true}"#,
    )
    .await;
    assert_eq!(presence.text(), r#"{"ok": true}"#);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        watchers.iter().all(|w| !w.is_finished()),
        "la presencia no es un cambio de avisos"
    );
    request_body(port, "POST", "/notices/read", "", r#"{"eventIds": ["e1"]}"#).await;
    let woke = Instant::now();
    for watcher in watchers {
        assert_eq!(watcher.await.unwrap().status, 200);
    }
    assert!(
        woke.elapsed() < Duration::from_secs(2),
        "todos despiertan con el cambio"
    );
    front.stop().await;
}

#[tokio::test]
async fn notices_survive_hung_tmux() {
    let home = TestHome::new("notices-hung");
    seed_permission(&home, "e1");
    let mut opts = home.options();
    opts.tmux = Tmux {
        program: Program {
            path: "tail".into(),
            prefix: vec!["-f".into(), "/dev/null".into(), "--".into()],
            env: vec![],
            env_remove: vec![],
            env_clear: false,
        },
        timeout: Duration::from_millis(300),
    };
    let front = front(&home, dead_port(), opts).await;
    // is_live es None: no se filtra por terminal viva.
    assert_eq!(
        get(front.port, "/notifs/count").await.text(),
        r#"{"count": 1}"#
    );
    assert_eq!(get(front.port, "/notices").await.status, 200);
    let watch = parse(&get(front.port, "/notices/watch?wait=0").await.text());
    assert_eq!(watch["badge"], 1);
    front.stop().await;
}

#[tokio::test]
async fn exotic_inputs_decline_to_legacy() {
    let home = TestHome::new("notices-exotic");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    for target in [
        "/notices?after=%D9%A1",
        "/notices?limit=99999999999999999999999",
        "/notices/watch?wait=%D9%A1",
    ] {
        assert_eq!(
            get(front.port, target).await.text(),
            r#"{"legacy": true}"#,
            "{target}"
        );
    }
    for (path, body) in [
        ("/presence", r#"{"deviceId": "d", "kind": {"a": 1}}"#),
        ("/notices/sound", r#"{"eventId": 1.5, "deviceId": "d"}"#),
        ("/notices/read", r#"{"all": true, "project": 5}"#),
    ] {
        let wire = request_body(front.port, "POST", path, "", body).await;
        assert_eq!(wire.text(), r#"{"legacy": true}"#, "{path}");
    }
    let conn = comandos_runtime::open_state(&home.state_db(), 5000).unwrap();
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM client_presence", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0, "declinar nunca escribe");
    let prefs: i64 = conn
        .query_row("SELECT COUNT(*) FROM notice_prefs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(prefs, 0, "declinar nunca escribe");
    front.stop().await;
}

#[tokio::test]
async fn notices_forwarded_after_schema_bump() {
    let home = TestHome::new("notices-bump");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    assert_eq!(
        get(front.port, "/notifs/count").await.text(),
        r#"{"count": 0}"#
    );
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    conn.execute(
        "INSERT INTO schema_migrations (version, name, applied_at) VALUES (9999, 'futuro', 0)",
        [],
    )
    .unwrap();
    assert_eq!(
        get(front.port, "/notifs/count").await.text(),
        r#"{"legacy": true}"#
    );
    // Todo el conjunto nativo queda apagado, también lo que no toca la base.
    assert_eq!(
        get(front.port, "/prefs").await.text(),
        r#"{"legacy": true}"#
    );
    front.stop().await;
}

#[tokio::test]
async fn notices_match_python_oracle() {
    let home = TestHome::new("notices-oracle");
    seed_permission(&home, "e1");
    seed_permission(&home, "e2");
    let mut opts = home.options();
    opts.clock = Arc::new(|| support::NOW_MS);
    let front = front(&home, dead_port(), opts).await;
    seed_shared_native_startup(&home);
    let py = FrozenHttp::new_rooted_with(
        &home,
        "notices-shared",
        &[".local/state/comandos/app-state.sqlite3"],
        "",
    )
    .await;
    for target in [
        "/notices/prefs",
        "/notifs/count",
        "/notices/watch?wait=0",
        "/notices?after=0&limit=1",
        "/notices?after=z",
    ] {
        let (a, b) = (py.get(target).await, get(front.port, target).await);
        assert_eq!((a.status, a.text()), (b.status, b.text()), "{target}");
    }
    // Lo que escribe uno lo ve el otro.
    request_body(
        front.port,
        "POST",
        "/notices/read",
        "",
        r#"{"eventIds": ["e1"]}"#,
    )
    .await;
    assert_eq!(py.get("/notifs/count").await.text(), r#"{"count": 1}"#);
    let (a, b) = (
        py.request("POST", "/notices/read", "", r#"{"all": true}"#)
            .await,
        request_body(front.port, "POST", "/notices/read", "", r#"{"all": true}"#).await,
    );
    assert_eq!(a.text(), r#"{"ok": true, "read": ["e2"]}"#);
    assert_eq!(b.text(), r#"{"ok": true, "read": []}"#);
    front.stop().await;
}

/// Native background remains real; the confined original deliberately disables
/// its scheduler. Record that harness boundary instead of hiding it by seed.
#[tokio::test]
async fn cold_notices_retains_confined_original_scheduler_boundary() {
    let native_home = TestHome::new("notices-cold-native");
    seed_permission(&native_home, "e1");
    let count = |home: &TestHome| -> i64 {
        let conn = rusqlite::Connection::open(home.state_db()).unwrap();
        conn.query_row("SELECT COUNT(*) FROM focus_policies", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(count(&native_home), 0);
    let native = front(&native_home, dead_port(), native_home.options()).await;
    assert_eq!(get(native.port, "/notices/prefs").await.status, 200);
    let deadline = Instant::now() + Duration::from_secs(3);
    while count(&native_home) == 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let actual = count(&native_home);
    assert_eq!(actual, 1, "native background startup ensures policy_v1");
    let source_home = TestHome::new("notices-cold-source");
    seed_permission(&source_home, "e1");
    let source = FrozenHttp::new(&source_home, "notices-cold-source", &[]).await;
    let observed = source.source_port().map(|_| count(&source_home));
    let expected = comandos_oracle::oracle_at(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        "notices-cold-startup",
        &json!({"source":support::frozen::SOURCE_COMMIT,"python":"3.10.12","stage":"after-startup-before-notices","seed_event":"e1","disabled_original_loops":["start_pomodoro_scheduler","motor_queue_resume"]}),
        || {
            serde_json::to_vec(&observed.ok_or("original startup required in record/check")?)
                .map_err(|e| e.to_string())
        },
    );
    let original: i64 = serde_json::from_slice(&expected).unwrap();
    assert_eq!(
        original, 0,
        "original scheduler disabled by the retained confinement fixture"
    );
    assert_ne!(
        actual, original,
        "explicit harness scheduling boundary; no product defect inferred"
    );
    native.stop().await;
}

/// Una terminal viva en el tmux privado compartido: el filtro `is_live`.
fn start_private_session(home: &TestHome) -> bool {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta el filtro de terminales vivas");
        return false;
    }
    home.tmux_command()
        .args(["new-session", "-d", "-s", "s1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn pane_of_session(home: &TestHome) -> String {
    let out = home
        .tmux_command()
        .args(["list-panes", "-t", "s1", "-F", "#{pane_id}"])
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// Mismo HOME y misma base: el long-poll del Python y el del frente
/// responden los mismos bytes al despertar y al vencer.
#[tokio::test]
async fn long_poll_matches_python_oracle() {
    let home = TestHome::new("notices-oracle-watch");
    let live = start_private_session(&home);
    let pane = if live {
        pane_of_session(&home)
    } else {
        "%0".into()
    };
    seed_event(&home, "e1", "permission_requested", json!({"paneId": pane}));
    seed_event(
        &home,
        "e2",
        "input_requested",
        json!({"sessionKey": "muerta", "paneId": "%77"}),
    );
    seed_event(&home, "e3", "turn_completed", json!({"projectKey": "p"}));
    let mut opts = home.options();
    opts.clock = Arc::new(|| support::NOW_MS);
    let front = front(&home, dead_port(), opts).await;
    // The original watch uses time.time for its deadline; keep that timer
    // ticking, while document timestamps retain the deterministic wall clock.
    let timer = format!(
        "import sys\n_watch_origin = dash.time.monotonic()\ndef _watch_clock():\n    if sys._getframe(1).f_code.co_name == '_do_GET':\n        return {} + dash.time.monotonic() - _watch_origin\n    return {}\ndash.time.time = _watch_clock",
        support::NOW_MS / 1000,
        support::NOW_MS / 1000
    );
    seed_shared_native_startup(&home);
    let py = FrozenHttp::new_rooted_with(
        &home,
        "notices-watch-documents",
        &[".local/state/comandos/app-state.sqlite3"],
        &timer,
    )
    .await;
    let first = get(front.port, "/notices/watch?wait=0").await.text();
    assert_eq!(first, py.get("/notices/watch?wait=0").await.text());
    let pending = &parse(&first)["pending"];
    if live {
        assert_eq!(pending, &json!(["e1"]), "la terminal muerta no cuenta");
    } else {
        assert_eq!(pending, &json!(["e1", "e2"]), "sin tmux no se filtra");
    }
    for target in ["/notices", "/notifs/count", "/notices?limit=2&after=1"] {
        assert_eq!(
            py.get(target).await.text(),
            get(front.port, target).await.text(),
            "{target}"
        );
    }
    let rev = parse(&first)["rev"].as_str().unwrap().to_owned();
    // Vence a 1 s sin cambios: los dos esperan y responden lo mismo.
    let target = format!("/notices/watch?rev={rev}&wait=1");
    let front_port = front.port;
    let source_wait = |target: String| {
        py.source_port()
            .map(|port| tokio::spawn(async move { get(port, &target).await }))
    };
    let py_wait = source_wait(target.clone());
    let t_front = target.clone();
    let started = Instant::now();
    let front_wait = tokio::spawn(async move { get(front_port, &t_front).await });
    let source = if let Some(task) = py_wait {
        Some(task.await.unwrap())
    } else {
        None
    };
    let b = front_wait.await.unwrap();
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_secs(1), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    assert_eq!(freeze_watch(&target, source), (b.status, b.text()));
    assert_eq!(parse(&b.text())["rev"], rev.as_str());
    // Source document mutation in record/check; replay applies only its
    // recorded SQLite delta. The native long-poll actor always runs and wakes.
    let target = format!("/notices/watch?rev={rev}&wait=10");
    let py_wait = source_wait(target.clone());
    let t_front = target.clone();
    let front_wait = tokio::spawn(async move { get(front_port, &t_front).await });
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(!front_wait.is_finished());
    if let Some(task) = &py_wait {
        assert!(!task.is_finished());
    }
    let read = py
        .request("POST", "/notices/read", "", r#"{"eventIds": ["e3"]}"#)
        .await;
    assert_eq!(read.text(), r#"{"ok": true, "read": ["e3"]}"#);
    let woke = Instant::now();
    let source = if let Some(task) = py_wait {
        Some(task.await.unwrap())
    } else {
        None
    };
    let b = front_wait.await.unwrap();
    assert!(woke.elapsed() < Duration::from_secs(2));
    assert_eq!(freeze_watch(&target, source), (b.status, b.text()));
    assert_ne!(parse(&b.text())["rev"], rev.as_str());
    assert!(started.elapsed() >= Duration::from_secs(2));
    front.stop().await;
}

/// Cada ruta y cada error contra el Python, cada uno con su HOME y la misma
/// secuencia de peticiones.
#[tokio::test]
async fn every_route_and_error_matches_python_oracle() {
    let (py_home, rs_home) = (
        TestHome::new("notices-py-seq"),
        TestHome::new("notices-rs-seq"),
    );
    for home in [&py_home, &rs_home] {
        seed_permission(home, "e1");
        seed_event(home, "e2", "turn_completed", json!({"projectKey": "p"}));
        seed_event(home, "e3", "turn_failed", json!({"projectKey": "q"}));
        seed_event(home, "e4", "news_edition", json!({"projectKey": "p"}));
        seed_event(home, "e5", "turn_started", json!({}));
    }
    let mut opts = rs_home.options();
    opts.clock = Arc::new(|| support::NOW_MS);
    let front = front(&rs_home, dead_port(), opts).await;
    let py = FrozenHttp::new_rooted_with(
        &py_home,
        "notices-sequence",
        &[".local/state/comandos/app-state.sqlite3"],
        "",
    )
    .await;
    let long = format!(r#"{{"deviceId": "{}"}}"#, "x".repeat(201));
    let steps: &[(&str, &str, Option<&str>)] = &[
        ("GET", "/notices", None),
        ("GET", "/notices?after=x", None),
        ("GET", "/notices?after=1&limit=1.5", None),
        ("GET", "/notices?after=%20", None),
        ("GET", "/notices?after=+1&limit=1_0", None),
        ("GET", "/notices?after=-5&limit=-3", None),
        ("GET", "/notices?after=999", None),
        ("GET", "/notices?limit=0", None),
        ("GET", "/notices?after=&limit=", None),
        ("GET", "/notices?after=0x1", None),
        ("GET", "/notices?after=2&after=3&limit=2", None),
        ("GET", "/notices?deviceId=d1&deviceId=d2", None),
        ("GET", "/notices/prefs", None),
        ("GET", "/notices/prefs?x=1", None),
        ("GET", "/notifs/count", None),
        ("GET", "/notifs/count?x=1", None),
        ("GET", "/notices/watch?rev=zz&wait=abc", None),
        ("GET", "/notices/watch?rev=zz&wait=nan", None),
        ("GET", "/notices/watch?rev=zz&wait=1e400", None),
        ("POST", "/presence", Some(r#"{}"#)),
        ("POST", "/presence", Some(r#"{"deviceId": 5}"#)),
        ("POST", "/presence", Some(long.as_str())),
        (
            "POST",
            "/presence",
            Some(
                r#"{"deviceId": "d1", "visible": true, "canPlayAudio": true, "interaction": true, "kind": "desktop-muy-largo-de-veras"}"#,
            ),
        ),
        (
            "POST",
            "/presence",
            Some(r#"{"deviceId": "d2", "visible": 1, "kind": 7}"#),
        ),
        (
            "POST",
            "/notices/sound",
            Some(r#"{"eventId": "e1", "deviceId": "d2"}"#),
        ),
        (
            "POST",
            "/notices/sound",
            Some(r#"{"eventId": "e2", "deviceId": "d1"}"#),
        ),
        (
            "POST",
            "/notices/sound",
            Some(r#"{"eventId": "no", "deviceId": "d1"}"#),
        ),
        ("POST", "/notices/sound", Some(r#"{}"#)),
        (
            "POST",
            "/notices/sound",
            Some(r#"{"eventId": "e1", "deviceId": "d1"}"#),
        ),
        (
            "POST",
            "/notices/sound",
            Some(r#"{"eventId": "e1", "deviceId": "d1"}"#),
        ),
        ("POST", "/notices/prefs", Some(r#"{"volume": true}"#)),
        ("POST", "/notices/prefs", Some(r#"{"volume": 2}"#)),
        ("POST", "/notices/prefs", Some(r#"{"volume": null}"#)),
        ("POST", "/notices/prefs", Some(r#"{"modes": "x"}"#)),
        (
            "POST",
            "/notices/prefs",
            Some(r#"{"modes": {"zzz": ["x"]}}"#),
        ),
        (
            "POST",
            "/notices/prefs",
            Some(r#"{"modes": {"attention": ["x"]}}"#),
        ),
        (
            "POST",
            "/notices/prefs",
            Some(r#"{"modes": {"done": "sound", "error": {"a": 1}}}"#),
        ),
        (
            "POST",
            "/notices/prefs",
            Some(r#"{"modes": {"done": "loud"}}"#),
        ),
        ("POST", "/notices/prefs", Some(r#"{"muted": 1}"#)),
        ("POST", "/notices/prefs", Some(r#"{"volume": 1}"#)),
        ("POST", "/notices/prefs", Some(r#"{"volume": -0}"#)),
        ("POST", "/notices/prefs", Some(r#"{"volume": -0.0}"#)),
        ("POST", "/notices/prefs", Some(r#"{"volume": 1e400}"#)),
        (
            "POST",
            "/notices/prefs",
            Some(r#"{"volume": 0.30000000000000004}"#),
        ),
        (
            "POST",
            "/notices/prefs",
            Some(
                r#"{"volume": 1e-7, "modes": {"done": "sound", "info": "sound"}, "muted": false}"#,
            ),
        ),
        ("GET", "/notices/prefs", None),
        ("GET", "/notices", None),
        ("POST", "/notices/read", Some(r#"{}"#)),
        ("POST", "/notices/read", Some(r#"{"eventIds": "e1"}"#)),
        (
            "POST",
            "/notices/read",
            Some(r#"{"eventIds": null, "all": 1}"#),
        ),
        (
            "POST",
            "/notices/read",
            Some(r#"{"eventIds": ["e1", 3, "", null, "no-existe"]}"#),
        ),
        ("GET", "/notifs/count", None),
        (
            "POST",
            "/notices/read",
            Some(r#"{"all": true, "project": "p"}"#),
        ),
        (
            "POST",
            "/notices/read",
            Some(r#"{"all": true, "project": ""}"#),
        ),
        ("GET", "/notices", None),
        ("GET", "/notifs/count", None),
    ];
    for (index, (method, path, body)) in steps.iter().enumerate() {
        let (a, b) = match (method, body) {
            (&"GET", None) => (py.get(path).await, get(front.port, path).await),
            (method, Some(body)) => (
                py.request(method, path, "", body).await,
                request_body(front.port, method, path, "", body).await,
            ),
            _ => unreachable!(),
        };
        let mut texts = (a.text(), b.text());
        // La revisión lleva el instante del último leído: distinto en cada lado.
        if path.starts_with("/notices/watch") {
            let (mut x, mut y) = (parse(&texts.0), parse(&texts.1));
            x["rev"] = Value::Null;
            y["rev"] = Value::Null;
            texts = (x.to_string(), y.to_string());
        }
        assert_eq!(
            (a.status, a.header("content-type"), &texts.0),
            (b.status, b.header("content-type"), &texts.1),
            "paso {index}: {method} {path} {body:?}"
        );
    }
    front.stop().await;
}

/// Presencia escrita por un lado, sonido reclamado por el otro, sobre la misma
/// base: los dos eligen el mismo dispositivo y el evento suena una sola vez.
#[tokio::test]
async fn sound_device_matches_across_rust_and_python() {
    let home = TestHome::new("notices-oracle-sound");
    seed_permission(&home, "e1");
    seed_permission(&home, "e2");
    let mut opts = home.options();
    opts.clock = Arc::new(|| support::NOW_MS);
    let front = front(&home, dead_port(), opts).await;
    seed_shared_native_startup(&home);
    let py = FrozenHttp::new_rooted_with(
        &home,
        "notices-shared",
        &[".local/state/comandos/app-state.sqlite3"],
        "",
    )
    .await;
    let presence = |audio: bool, device: &str| {
        format!(
            r#"{{"deviceId": "{device}", "visible": true, "canPlayAudio": {audio}, "interaction": true}}"#
        )
    };
    let sound =
        |event: &str, device: &str| format!(r#"{{"eventId": "{event}", "deviceId": "{device}"}}"#);
    let elsewhere = r#"{"play": false, "reason": "Suena en otro dispositivo"}"#;
    let already = r#"{"play": false, "reason": "Ya son\u00f3"}"#;
    let plays = r#"{"play": true, "cue": "permission"}"#;
    // Rust registra la presencia; el Python reclama el sonido.
    for body in [presence(true, "d-audio"), presence(false, "d-mudo")] {
        let wire = request_body(front.port, "POST", "/presence", "", &body).await;
        assert_eq!(wire.text(), r#"{"ok": true}"#);
    }
    for (source, device, expected) in [
        (false, "d-mudo", elsewhere),
        (true, "d-mudo", elsewhere),
        (true, "d-audio", plays),
        (false, "d-audio", already),
    ] {
        let body = sound("e1", device);
        let wire = if source {
            py.request("POST", "/notices/sound", "", &body).await
        } else {
            request_body(front.port, "POST", "/notices/sound", "", &body).await
        };
        assert_eq!(wire.text(), expected, "e1 {device} source={source}");
    }
    // Al revés: el Python registra la presencia; el frente reclama.
    for body in [presence(false, "d-audio"), presence(true, "d-mudo")] {
        let wire = py.request("POST", "/presence", "", &body).await;
        assert_eq!(wire.text(), r#"{"ok": true}"#);
    }
    for (source, device, expected) in [
        (true, "d-audio", elsewhere),
        (false, "d-audio", elsewhere),
        (false, "d-mudo", plays),
        (true, "d-mudo", already),
    ] {
        let body = sound("e2", device);
        let wire = if source {
            py.request("POST", "/notices/sound", "", &body).await
        } else {
            request_body(front.port, "POST", "/notices/sound", "", &body).await
        };
        assert_eq!(wire.text(), expected, "e2 {device} source={source}");
    }
    front.stop().await;
}

#[tokio::test]
async fn disabled_event_switches_suppress_floats_but_preserve_history() {
    let home = TestHome::new("notice-switches");
    seed_event(&home, "done-disabled", "turn_completed", json!({}));
    seed_permission(&home, "attention-disabled");
    home.write(
        "cc-notify.conf",
        "NOTIFY_ON_DONE=0\nNOTIFY_ON_ATTENTION=0\n",
    );
    let front = front(&home, dead_port(), home.options()).await;
    let page = parse(&get(front.port, "/notices").await.text());
    assert_eq!(page["notices"].as_array().unwrap().len(), 2);
    for notice in page["notices"].as_array().unwrap() {
        assert_eq!(notice["float"]["show"], false, "{notice}");
    }
    let sound = request_body(
        front.port,
        "POST",
        "/notices/sound",
        "",
        r#"{"eventId":"attention-disabled","deviceId":"local-speaker"}"#,
    )
    .await;
    assert_eq!(parse(&sound.text())["play"], false);
    let revision = parse(&get(front.port, "/notices/watch?wait=0").await.text())["rev"].clone();
    home.write(
        "cc-notify.conf",
        "NOTIFY_ON_DONE=1\nNOTIFY_ON_ATTENTION=0\n",
    );
    let updated_revision =
        parse(&get(front.port, "/notices/watch?wait=0").await.text())["rev"].clone();
    assert_ne!(
        revision, updated_revision,
        "switch changes wake the other device without new events"
    );
    let enabled = parse(&get(front.port, "/notices").await.text());
    assert_eq!(enabled["notices"][0]["float"]["show"], true);
    assert_eq!(enabled["notices"][1]["float"]["show"], false);
    let muted = request_body(
        front.port,
        "POST",
        "/notices/prefs",
        "",
        r#"{"muted":true}"#,
    )
    .await;
    assert_eq!(muted.status, 200);
    home.write(
        "cc-notify.conf",
        "NOTIFY_ON_DONE=1\nNOTIFY_ON_ATTENTION=1\n",
    );
    let page = parse(&get(front.port, "/notices").await.text());
    assert!(
        page["notices"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["float"]["show"] == false)
    );
}
