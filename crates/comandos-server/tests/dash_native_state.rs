//! GET /state respondido por Rust: oráculo cc-dash real sobre el mismo HOME
//! y el mismo tmux privado, vuelo único, declinar sin escribir, 504 con el
//! runtime libre y el contexto de sugerencias contra el heredado.
mod support;
use serde_json::{Value, json};
use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};
use support::{
    FakeLegacy, FixedLegacy, NOW_MS, TestHome, dead_port, fake_agent, front, get, oracle::oracle,
    start_session, tmux_available,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

fn masked(body: &[u8]) -> String {
    let mut v: Value = serde_json::from_slice(body).unwrap();
    for item in v.as_array_mut().unwrap() {
        if let Some(obs) = item
            .get_mut("observedConfig")
            .and_then(Value::as_object_mut)
        {
            for k in ["observedAt", "evidenceAt"] {
                if obs.contains_key(k) {
                    obs.insert(k.into(), json!("<volátil>"));
                }
            }
        }
    }
    comandos_core::json::response_dumps(&v).unwrap()
}

/// Solo el servidor privado de la prueba (`-S`, nunca el del usuario).
fn tmux(home: &TestHome, args: &[&str]) -> std::process::Output {
    home.tmux_command().args(args).output().unwrap()
}

/// HOME con un agente claude falso en su propio pane, su sesión registrada,
/// un transcript con modelo, un split de shells y registros de estado. El
/// registro del pane vivo está en `waiting` (B8): sin sugerencias que
/// dependan del minuto.
fn seed(home: &TestHome) -> Option<()> {
    if !tmux_available() {
        eprintln!("tmux no está: se salta");
        return None;
    }
    let claude = fake_agent(home, "claude");
    start_session(home, "proj", &format!("{} 600", claude.display()));
    start_session(home, "shells", "sh");
    assert!(
        tmux(home, &["split-window", "-t", "=shells:", "sh"])
            .status
            .success()
    );
    std::thread::sleep(Duration::from_millis(300));
    let pid = tmux(
        home,
        &["display-message", "-p", "-t", "=proj:", "#{pane_pid}"],
    );
    let pid: i64 = String::from_utf8_lossy(&pid.stdout).trim().parse().unwrap();
    fs::create_dir_all(home.root.join(".claude/sessions")).unwrap();
    fs::write(
        home.root.join(".claude/sessions/x.json"),
        json!({"pid": pid, "sessionId": "abc"}).to_string(),
    )
    .unwrap();
    fs::create_dir_all(home.root.join(".claude/projects/p")).unwrap();
    fs::write(
        home.root.join(".claude/projects/p/abc.jsonl"),
        "{\"type\":\"assistant\",\"uuid\":\"u1\",\"sessionId\":\"abc\",\"message\":{\"model\":\"claude-sonnet-5\"}}\n",
    )
    .unwrap();
    let state = home.hooks().join("state");
    fs::write(
        state.join("a.json"),
        json!({"session":"proj","agent":"claude","status":"waiting","detail":"¿sigo?","ts":1})
            .to_string(),
    )
    .unwrap();
    fs::write(
        state.join("b.json"),
        json!({"session":"gone","status":"waiting","detail":"¿sigo?","ts":2.5}).to_string(),
    )
    .unwrap();
    home.write("app-tabs.json", r#"{"proj": "Proyecto"}"#);
    Some(())
}

/// Relevo hacia el oráculo que anota la línea de cada petición: prueba que
/// el frente NO reenvió `/state` y que sí pidió el contexto.
struct Relay {
    port: u16,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Relay {
    async fn start(upstream: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut client, _)) = listener.accept().await {
                let log = log.clone();
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        match client.read(&mut byte).await {
                            Ok(0) | Err(_) => return,
                            Ok(_) => head.push(byte[0]),
                        }
                    }
                    let text = String::from_utf8_lossy(&head).to_string();
                    log.lock()
                        .unwrap()
                        .push(text.lines().next().unwrap_or("").to_owned());
                    let Ok(mut server) = TcpStream::connect(("127.0.0.1", upstream)).await else {
                        return;
                    };
                    if server.write_all(&head).await.is_err() {
                        return;
                    }
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                });
            }
        });
        Self { port, seen }
    }
    fn requests(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn state_matches_python_oracle_and_writes_same_models() {
    let home = TestHome::new("state-oracle");
    if seed(&home).is_none() {
        return;
    }
    let Some(py) = oracle(&home).await else {
        return;
    };
    let expected = get(py.port, "/state").await;
    assert_eq!(expected.status, 200);
    let models_py = fs::read(home.hooks().join("app-tab-models.json")).unwrap();
    fs::remove_file(home.hooks().join("app-tab-models.json")).unwrap();
    // El frente reenvía lo no nativo (y las subconsultas) a ese mismo Python.
    let relay = Relay::start(py.port).await;
    let mut opts = home.options();
    // El oráculo usa la hora real (B9).
    opts.clock = Arc::new(comandos_server::dash::native::wall_clock_ms);
    let front = front(&home, relay.port, opts).await;
    let got = get(front.port, "/state").await;
    assert_eq!(got.status, 200);
    assert_eq!(masked(&got.body), masked(&expected.body));
    // La comparación cubre de verdad la observación: tarjeta claude viva con
    // el modelo del transcript, etiqueta de pestaña, split e historial.
    let cards: Value = serde_json::from_slice(&got.body).unwrap();
    let claude = cards
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["session"] == "proj")
        .unwrap();
    assert_eq!(claude["agent"], "claude");
    assert_eq!(claude["project"], "Proyecto");
    assert_eq!(claude["status"], "waiting");
    assert!(claude["observedConfig"].is_object());
    assert_eq!(claude["model"], "claude-sonnet-5");
    assert_eq!(claude["modelSource"], "conversation");
    let text = got.text();
    assert!(text.contains("\"split\": true"), "{text}");
    assert!(text.contains("\"session\": \"gone\""), "{text}");
    assert_eq!(
        fs::read(home.hooks().join("app-tab-models.json")).unwrap(),
        models_py
    );
    let seen = relay.requests();
    assert!(
        !seen.iter().any(|l| l.starts_with("GET /state")),
        "/state se reenvió: {seen:?}"
    );
    // La tarjeta claude viva necesita el contexto: el heredado lo dio.
    assert!(seen.iter().any(|l| l.starts_with("GET /usage/guard ")));
    assert!(
        seen.iter()
            .any(|l| l.starts_with("GET /usage/analytics?days=7 "))
    );
    // Una consulta también es nativa y sale de la caché.
    let again = get(front.port, "/state?x=1").await;
    assert_eq!(again.body, got.body);
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn state_unsure_record_declines_without_writing() {
    let home = TestHome::new("state-unsure");
    if seed(&home).is_none() {
        return;
    }
    home.write(
        "state/c.json",
        r#"{"session":"s","detail":"\ud800","ts":3}"#,
    );
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let got = get(front.port, "/state").await;
    assert_eq!(got.text(), r#"{"legacy": true}"#);
    assert!(
        legacy
            .requests()
            .iter()
            .any(|l| l.starts_with("GET /state"))
    );
    assert!(!home.hooks().join("app-tab-models.json").exists());
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn state_flight_survives_leader_cancel() {
    let home = TestHome::new("state-flight");
    if seed(&home).is_none() {
        return;
    }
    let clock = Arc::new(AtomicI64::new(NOW_MS));
    let mut opts = home.options();
    let shared = clock.clone();
    opts.clock = Arc::new(move || shared.load(Ordering::SeqCst));
    // Contexto: `guard` y analítica vacíos; nada se reenvía a este heredado.
    let legacy = FixedLegacy::start(200, "{}").await;
    let front = front(&home, legacy.port, opts).await;
    // Líder que se desconecta enseguida y dos seguidores.
    let port = front.port;
    let leader = tokio::spawn(async move {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(
            format!(
                "GET /state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Comandos-Token: {}\r\n\r\n",
                support::TOKEN
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        drop(s);
    });
    let (a, b) = tokio::join!(get(port, "/state"), get(port, "/state"));
    leader.await.unwrap();
    assert_eq!((a.status, b.status), (200, 200));
    assert_eq!(a.body, b.body);
    // Dentro de 1,2 s: caché (mismo cuerpo aunque cambie un registro).
    home.write(
        "state/d.json",
        r#"{"session":"otro","status":"waiting","ts":4}"#,
    );
    clock.fetch_add(1_000, Ordering::SeqCst);
    assert_eq!(get(port, "/state").await.body, a.body);
    clock.fetch_add(300, Ordering::SeqCst);
    let fresh = get(port, "/state").await;
    assert_eq!(fresh.status, 200);
    assert_ne!(fresh.body, a.body);
    assert!(fresh.text().contains("\"otro\""));
    assert!(
        !legacy
            .requests()
            .iter()
            .any(|l| l.starts_with("GET /state")),
        "{:?}",
        legacy.requests()
    );
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn state_hung_tmux_answers_504_runtime_free() {
    let home = TestHome::new("state-hung");
    if seed(&home).is_none() {
        return;
    }
    // Un pane «codex» (pista con plazo 2 s) y un tmux que se cuelga en capture-pane.
    let codex = fake_agent(&home, "codex");
    start_session(&home, "cx", &format!("{} 600", codex.display()));
    std::thread::sleep(Duration::from_millis(300));
    let wrapper = home.root.join("bin/tmux-colgado");
    fs::write(
        &wrapper,
        "#!/bin/sh\ncase \"$*\" in *capture-pane*) exec sleep 30;; esac\nexec tmux \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(
        &wrapper,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let mut opts = home.options();
    opts.tmux.program.path = wrapper;
    let front = front(&home, dead_port(), opts).await;
    let port = front.port;
    let slow = tokio::spawn(async move { get(port, "/state").await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let started = std::time::Instant::now();
    assert_eq!(get(port, "/prefs").await.status, 200);
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "el runtime quedó bloqueado"
    );
    assert_eq!(slow.await.unwrap().status, 504);
    assert!(!home.hooks().join("app-tab-models.json").exists());
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn suggestion_context_legacy_500_is_empty_and_down_declines() {
    use comandos_server::dash::native::states::{context::Context, serial::Serial};
    // Heredado caído → Decline (y el fallo se recuerda 5 s); heredado que
    // responde 500 → guard {} y latencia vacía, cacheado 60 s.
    let home = TestHome::new("state-ctx");
    let mut opts = home.options();
    opts.legacy = std::net::SocketAddr::from(([127, 0, 0, 1], dead_port()));
    let ctx = Context::default();
    let registry = json!({});
    assert!(
        ctx.get(&opts, &Serial::default(), &registry, NOW_MS)
            .await
            .is_err()
    );
    let failing = FixedLegacy::start(500, r#"{"error": "Error interno del tablero"}"#).await;
    opts.legacy = std::net::SocketAddr::from(([127, 0, 0, 1], failing.port));
    // Dentro de la memoria del fallo: declina sin preguntar.
    assert!(
        ctx.get(&opts, &Serial::default(), &registry, NOW_MS + 4_999)
            .await
            .is_err()
    );
    assert!(failing.requests().is_empty());
    let t0 = NOW_MS + 5_000;
    let got = ctx
        .get(&opts, &Serial::default(), &registry, t0)
        .await
        .unwrap();
    assert_eq!(got.guard, json!({}));
    assert!(got.latency.is_empty());
    assert!(got.routes.is_empty());
    assert_eq!(failing.requests().len(), 2);
    drop(failing);
    // 60 s después sigue siendo el mismo (el heredado ya no existe).
    assert!(
        ctx.get(&opts, &Serial::default(), &registry, t0 + 60_000)
            .await
            .is_ok()
    );
    assert!(
        ctx.get(&opts, &Serial::default(), &registry, t0 + 60_001)
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn suggestion_context_unreadable_200_declines() {
    use comandos_server::dash::native::states::{context::Context, serial::Serial};
    // C11: un 200 que el frente no puede leer como el Python (un sustituto
    // suelto, que `json` sí acepta) declina; `NaN` sí se lee (`workspace_loads`).
    let home = TestHome::new("state-ctx-unreadable");
    let legacy = FixedLegacy::start(200, r#"{"x": "\ud800"}"#).await;
    let mut opts = home.options();
    opts.legacy = std::net::SocketAddr::from(([127, 0, 0, 1], legacy.port));
    assert!(
        Context::default()
            .get(&opts, &Serial::default(), &json!({}), NOW_MS)
            .await
            .is_err()
    );
}

/// Heredado cuyas respuestas a `/usage/*` cambian durante la prueba; el resto
/// (el `/state` reenviado) responde `{"legacy": true}`. Anota cada línea.
struct SwitchLegacy {
    port: u16,
    usage: Arc<Mutex<String>>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl SwitchLegacy {
    async fn start(usage: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let usage = Arc::new(Mutex::new(usage.to_owned()));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (body_of, log) = (usage.clone(), seen.clone());
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let (body_of, log) = (body_of.clone(), log.clone());
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        match stream.read(&mut byte).await {
                            Ok(0) | Err(_) => return,
                            Ok(_) => head.push(byte[0]),
                        }
                    }
                    let line = String::from_utf8_lossy(&head)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_owned();
                    let body = if line.starts_with("GET /usage/") {
                        body_of.lock().unwrap().clone()
                    } else {
                        r#"{"legacy": true}"#.to_owned()
                    };
                    log.lock().unwrap().push(line);
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        });
        Self { port, usage, seen }
    }
    fn set_usage(&self, body: &str) {
        *self.usage.lock().unwrap() = body.to_owned();
    }
    fn requests(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
    fn usage_requests(&self) -> usize {
        self.requests()
            .iter()
            .filter(|l| l.starts_with("GET /usage/"))
            .count()
    }
}

/// `seed` con el registro del pane vivo en `working`: la tarjeta pide el
/// contexto de sugerencias. Devuelve el envoltorio de tmux que anota cada
/// llamada (y sigue al tmux privado con su `-S`, que va en los argumentos).
fn seed_needing_context(home: &TestHome) -> Option<std::path::PathBuf> {
    seed(home)?;
    home.write(
        "state/a.json",
        &json!({"session":"proj","agent":"claude","status":"working","detail":"x","ts":1})
            .to_string(),
    );
    let log = home.root.join("tmux-calls.log");
    let wrapper = home.root.join("bin/tmux-anota");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\nexec tmux \"$@\"\n",
            log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(
        &wrapper,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    Some(wrapper)
}

fn tmux_calls(home: &TestHome) -> usize {
    fs::read_to_string(home.root.join("tmux-calls.log"))
        .map(|t| t.lines().count())
        .unwrap_or(0)
}

#[tokio::test(flavor = "current_thread")]
async fn state_context_failure_memory_declines_before_tmux() {
    // Revisión de la Tarea 5: con un fallo del contexto recordado, GET /state
    // se reenvía antes de llamar a tmux o leer `/proc`.
    let home = TestHome::new("state-ctx-memory");
    let Some(wrapper) = seed_needing_context(&home) else {
        return;
    };
    let legacy = SwitchLegacy::start(r#"{"x": "\ud800"}"#).await;
    let mut opts = home.options();
    opts.tmux.program.path = wrapper;
    let front = front(&home, legacy.port, opts).await;
    // 1: calcula, el contexto no se puede leer → declina y recuerda el fallo.
    assert_eq!(
        get(front.port, "/state").await.text(),
        r#"{"legacy": true}"#
    );
    let calls = tmux_calls(&home);
    assert!(calls > 0, "el primer cómputo no llamó a tmux");
    assert_eq!(legacy.usage_requests(), 2, "{:?}", legacy.requests());
    // 2: dentro de los 5 s del fallo, se reenvía sin tmux ni subconsultas.
    assert_eq!(
        get(front.port, "/state").await.text(),
        r#"{"legacy": true}"#
    );
    assert_eq!(tmux_calls(&home), calls);
    assert_eq!(legacy.usage_requests(), 2, "{:?}", legacy.requests());
    assert_eq!(
        legacy
            .requests()
            .iter()
            .filter(|l| l.starts_with("GET /state"))
            .count(),
        2
    );
    assert!(!home.hooks().join("app-tab-models.json").exists());
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn state_tracker_commits_only_emitted_results() {
    // Revisión de la Tarea 5: un cómputo que observó panes y luego declinó no
    // deja su observación en el rastreador. El siguiente cómputo que emite ve
    // la conversación por primera vez: `evidenceAt` es su instante, no el del
    // cómputo declinado.
    let home = TestHome::new("state-tracker-commit");
    let Some(wrapper) = seed_needing_context(&home) else {
        return;
    };
    let legacy = SwitchLegacy::start(r#"{"x": "\ud800"}"#).await;
    let clock = Arc::new(AtomicI64::new(NOW_MS));
    let mut opts = home.options();
    opts.tmux.program.path = wrapper;
    let shared = clock.clone();
    opts.clock = Arc::new(move || shared.load(Ordering::SeqCst));
    let front = front(&home, legacy.port, opts).await;
    assert_eq!(
        get(front.port, "/state").await.text(),
        r#"{"legacy": true}"#
    );
    assert!(tmux_calls(&home) > 0);
    // Pasada la memoria del fallo, el contexto ya se puede leer.
    legacy.set_usage("{}");
    clock.fetch_add(6_000, Ordering::SeqCst);
    let got = get(front.port, "/state").await;
    assert_eq!(got.status, 200, "{}", got.text());
    let cards: Value = serde_json::from_slice(&got.body).unwrap();
    let observed: Vec<&Value> = cards
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c.get("observedConfig"))
        .collect();
    assert!(!observed.is_empty(), "{}", got.text());
    let fresh = json!((NOW_MS + 6_000) as f64 / 1000.0);
    for obs in observed {
        assert_eq!(obs.get("evidenceAt"), Some(&fresh), "{obs}");
    }
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn suggestion_context_routes_failure_skips_legacy_and_is_remembered() {
    use comandos_server::dash::native::states::{context::Context, serial::Serial};
    // Revisión de la Tarea 5: las rutas se calculan antes de preguntar al
    // heredado; si fallan no hay subconsultas y el fallo se recuerda 5 s.
    let home = TestHome::new("state-ctx-routes");
    let legacy = FixedLegacy::start(200, "{}").await;
    let mut opts = home.options();
    opts.legacy = std::net::SocketAddr::from(([127, 0, 0, 1], legacy.port));
    let repo = opts.repo_root.take();
    let ctx = Context::default();
    let registry = json!({});
    assert!(!ctx.failing(NOW_MS));
    assert!(
        ctx.get(&opts, &Serial::default(), &registry, NOW_MS)
            .await
            .is_err()
    );
    assert!(legacy.requests().is_empty());
    assert!(ctx.failing(NOW_MS + 4_999));
    opts.repo_root = repo;
    assert!(
        ctx.get(&opts, &Serial::default(), &registry, NOW_MS + 4_999)
            .await
            .is_err()
    );
    assert!(legacy.requests().is_empty());
    assert!(!ctx.failing(NOW_MS + 5_000));
    assert!(
        ctx.get(&opts, &Serial::default(), &registry, NOW_MS + 5_000)
            .await
            .is_ok()
    );
    assert_eq!(legacy.requests().len(), 2);
    assert!(!ctx.failing(NOW_MS + 5_001));
}

#[tokio::test(flavor = "current_thread")]
async fn state_persistent_decline_is_cached_for_the_ttl() {
    // Revisión final, I1: con una incertidumbre persistente (un registro con
    // un sustituto suelto), el segundo sondeo dentro de 1,2 s se reenvía sin
    // repetir el trabajo de tmux; vencido el TTL, se recalcula.
    let home = TestHome::new("state-decline-cache");
    let Some(wrapper) = seed_needing_context(&home) else {
        return;
    };
    home.write(
        "state/c.json",
        r#"{"session":"s","detail":"\ud800","ts":3}"#,
    );
    let legacy = FakeLegacy::start().await;
    let clock = Arc::new(AtomicI64::new(NOW_MS));
    let mut opts = home.options();
    opts.tmux.program.path = wrapper;
    let shared = clock.clone();
    opts.clock = Arc::new(move || shared.load(Ordering::SeqCst));
    let front = front(&home, legacy.port, opts).await;
    assert_eq!(
        get(front.port, "/state").await.text(),
        r#"{"legacy": true}"#
    );
    let calls = tmux_calls(&home);
    assert!(calls > 0, "el primer cómputo no llamó a tmux");
    clock.fetch_add(1_199, Ordering::SeqCst);
    assert_eq!(
        get(front.port, "/state").await.text(),
        r#"{"legacy": true}"#
    );
    assert_eq!(tmux_calls(&home), calls, "el Decline vigente se recalculó");
    clock.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        get(front.port, "/state").await.text(),
        r#"{"legacy": true}"#
    );
    assert!(tmux_calls(&home) > calls, "vencido el TTL debía recalcular");
    let forwarded = legacy
        .requests()
        .iter()
        .filter(|l| l.starts_with("GET /state"))
        .count();
    assert_eq!(forwarded, 3);
    assert!(!home.hooks().join("app-tab-models.json").exists());
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn suggestion_context_other_status_declines_and_is_remembered() {
    use comandos_server::dash::native::states::{context::Context, serial::Serial};
    // Revisión final, M2: solo el 500 de la excepción de la función es el
    // `except` del Python; un 404 (o 401, 502…) del heredado declina y se
    // recuerda como un fallo, sin dar sugerencias distintas en silencio.
    let home = TestHome::new("state-ctx-404");
    let legacy = FixedLegacy::start(404, r#"{"error": "No encontrado"}"#).await;
    let mut opts = home.options();
    opts.legacy = std::net::SocketAddr::from(([127, 0, 0, 1], legacy.port));
    let ctx = Context::default();
    assert!(
        ctx.get(&opts, &Serial::default(), &json!({}), NOW_MS)
            .await
            .is_err()
    );
    assert!(ctx.failing(NOW_MS + 4_999));
    assert_eq!(legacy.requests().len(), 2);
}
