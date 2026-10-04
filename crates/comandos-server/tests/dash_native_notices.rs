//! Dominio A: avisos, campana, presencia, sonido y long-poll por el frente.
mod support;

use comandos_server::dash::native::{
    tmux::{Program, Tmux},
    wall_clock_ms,
};
use serde_json::{Value, json};
use std::{
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use support::{
    FakeLegacy, TestHome, dead_port, front, get, oracle::oracle, request_body, tmux_available,
};

fn seed_event(home: &TestHome, id: &str, kind: &str, extra: Value) {
    let conn = comandos_runtime::open_state(&home.state_db(), 5000).unwrap();
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
    opts.clock = Arc::new(wall_clock_ms);
    let front = front(&home, dead_port(), opts).await;
    let Some(py) = oracle(&home).await else {
        front.stop().await;
        return;
    };
    for target in [
        "/notices/prefs",
        "/notifs/count",
        "/notices/watch?wait=0",
        "/notices?after=0&limit=1",
        "/notices?after=z",
    ] {
        let (a, b) = (get(py.port, target).await, get(front.port, target).await);
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
    assert_eq!(
        get(py.port, "/notifs/count").await.text(),
        r#"{"count": 1}"#
    );
    let (a, b) = (
        request_body(py.port, "POST", "/notices/read", "", r#"{"all": true}"#).await,
        request_body(front.port, "POST", "/notices/read", "", r#"{"all": true}"#).await,
    );
    assert_eq!(a.text(), r#"{"ok": true, "read": ["e2"]}"#);
    assert_eq!(b.text(), r#"{"ok": true, "read": []}"#);
    front.stop().await;
}

/// Una terminal viva en el tmux privado compartido: el filtro `is_live`.
fn start_private_session(home: &TestHome) -> bool {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta el filtro de terminales vivas");
        return false;
    }
    Command::new("tmux")
        .args(["-f", "/dev/null", "new-session", "-d", "-s", "s1"])
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", home.tmux_dir())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn pane_of_session(home: &TestHome) -> String {
    let out = Command::new("tmux")
        .args(["list-panes", "-t", "s1", "-F", "#{pane_id}"])
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", home.tmux_dir())
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
    opts.clock = Arc::new(wall_clock_ms);
    let front = front(&home, dead_port(), opts).await;
    let Some(py) = oracle(&home).await else {
        front.stop().await;
        return;
    };
    let first = get(front.port, "/notices/watch?wait=0").await.text();
    assert_eq!(first, get(py.port, "/notices/watch?wait=0").await.text());
    let pending = &parse(&first)["pending"];
    if live {
        assert_eq!(pending, &json!(["e1"]), "la terminal muerta no cuenta");
    } else {
        assert_eq!(pending, &json!(["e1", "e2"]), "sin tmux no se filtra");
    }
    for target in ["/notices", "/notifs/count", "/notices?limit=2&after=1"] {
        assert_eq!(
            get(py.port, target).await.text(),
            get(front.port, target).await.text(),
            "{target}"
        );
    }
    let rev = parse(&first)["rev"].as_str().unwrap().to_owned();
    // Vence a 1 s sin cambios: los dos esperan y responden lo mismo.
    let target = format!("/notices/watch?rev={rev}&wait=1");
    let (py_port, front_port) = (py.port, front.port);
    let (t_py, t_front) = (target.clone(), target.clone());
    let started = Instant::now();
    let (a, b) = tokio::join!(
        tokio::spawn(async move { get(py_port, &t_py).await }),
        tokio::spawn(async move { get(front_port, &t_front).await }),
    );
    let elapsed = started.elapsed();
    let (a, b) = (a.unwrap(), b.unwrap());
    assert!(elapsed >= Duration::from_secs(1), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    assert_eq!((a.status, a.text()), (b.status, b.text()));
    assert_eq!(parse(&b.text())["rev"], rev.as_str());
    // Despierta con el cambio: un leído escrito por el Python despierta a los dos.
    let target = format!("/notices/watch?rev={rev}&wait=10");
    let (t_py, t_front) = (target.clone(), target);
    let py_wait = tokio::spawn(async move { get(py_port, &t_py).await });
    let front_wait = tokio::spawn(async move { get(front_port, &t_front).await });
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(!py_wait.is_finished() && !front_wait.is_finished());
    let read = request_body(
        py.port,
        "POST",
        "/notices/read",
        "",
        r#"{"eventIds": ["e3"]}"#,
    )
    .await;
    assert_eq!(read.text(), r#"{"ok": true, "read": ["e3"]}"#);
    let woke = Instant::now();
    let (a, b) = (py_wait.await.unwrap(), front_wait.await.unwrap());
    assert!(woke.elapsed() < Duration::from_secs(2));
    assert_eq!((a.status, a.text()), (b.status, b.text()));
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
    opts.clock = Arc::new(wall_clock_ms);
    let front = front(&rs_home, dead_port(), opts).await;
    let Some(py) = oracle(&py_home).await else {
        front.stop().await;
        return;
    };
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
            (&"GET", None) => (get(py.port, path).await, get(front.port, path).await),
            (method, Some(body)) => (
                request_body(py.port, method, path, "", body).await,
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
