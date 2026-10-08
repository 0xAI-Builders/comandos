//! GET /state respondido por Rust: oráculo cc-dash real sobre el mismo HOME
//! y el mismo tmux privado, vuelo único, declinar sin escribir, 504 con el
//! runtime libre y el contexto de sugerencias nativo (Tarea 5a de la 2e).
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
    FakeLegacy, FixedLegacy, NOW_MS, TestHome, dead_port, fake_agent, front, get,
    http_golden::FrozenHttp, start_session, tmux_available,
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

// Original observers run only in explicit record/check. Their volatile
// observation timestamps are masked exactly as in the retained comparison;
// the actual native actor and written model document are never replayed.
async fn frozen_state<'a>(home: &'a TestHome, tag: &str) -> (Value, FrozenHttp<'a>) {
    let py = FrozenHttp::new(home, "state-route-source", &[]).await;
    let source = if let Some(port) = py.source_port() {
        let wire = get(port, "/state").await;
        Some(json!({"status":wire.status, "body":masked(&wire.body),
            "models":fs::read_to_string(home.hooks().join("app-tab-models.json")).unwrap()}))
    } else {
        None
    };
    let actor = tmux(
        home,
        &[
            "display-message",
            "-p",
            "-t",
            "=proj:",
            "#{pid}\t#{pane_pid}",
        ],
    );
    let actor = String::from_utf8(actor.stdout).unwrap();
    let actor: Vec<std::path::PathBuf> = actor
        .trim_end()
        .split('\t')
        .map(std::path::PathBuf::from)
        .collect();
    assert_eq!(actor.len(), 2);
    let start = comandos_runtime::session_configuration::server_start(
        &home.options().proc_root,
        actor[0].to_str().unwrap(),
    )
    .unwrap();
    let start = std::path::PathBuf::from(start);
    let created = tmux(
        home,
        &[
            "display-message",
            "-p",
            "-t",
            "=shells:",
            "#{session_created}",
        ],
    );
    let created = std::path::PathBuf::from(String::from_utf8(created.stdout).unwrap().trim());
    let name = std::path::PathBuf::from(home.root.file_name().unwrap());
    let repo = fs::canonicalize(support::repo()).unwrap();
    let mut roots = vec![
        ("<HOME>", home.root.as_path()),
        ("<HOME-NAME>", name.as_path()),
        ("<REPO>", repo.as_path()),
        ("<TMUX-PID>", actor[0].as_path()),
        ("<PANE-PID>", actor[1].as_path()),
        ("<SHELL-CREATED>", created.as_path()),
    ];
    if !start.as_os_str().is_empty() {
        roots.push(("<SERVER-START>", start.as_path()));
    }
    let input = json!({"source":support::frozen::SOURCE_COMMIT,"python":"3.10.12",
        "clock_ms":NOW_MS,"target":"/state","fixture":tag,
        "state":fs::read_to_string(home.hooks().join("state/a.json")).unwrap()});
    let output = comandos_oracle::oracle_at(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        "state-route",
        &input,
        || {
            let source = source.ok_or("original HTTP required in record/check")?;
            Ok(comandos_oracle::normalize(
                &serde_json::to_vec(&source).map_err(|e| e.to_string())?,
                &roots,
            ))
        },
    );
    (
        serde_json::from_slice(&comandos_oracle::restore(&output, &roots)).unwrap(),
        py,
    )
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
/// el frente NO reenvió `/state` ni pidió nada de `/usage/*`.
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
    let (expected, _py) = frozen_state(&home, "waiting-models").await;
    assert_eq!(expected["status"], 200);
    let models_py = expected["models"].as_str().unwrap().as_bytes();
    let _ = fs::remove_file(home.hooks().join("app-tab-models.json"));
    assert!(!home.hooks().join("app-tab-models.json").exists());
    // El frente reenvía lo no nativo (y las subconsultas) a ese mismo Python.
    let legacy = FakeLegacy::start().await;
    let relay = Relay::start(legacy.port).await;
    let mut opts = home.options();
    opts.clock_seconds = Arc::new(|| NOW_MS as f64 / 1000.0);
    let front = front(&home, relay.port, opts).await;
    let got = get(front.port, "/state").await;
    assert_eq!(got.status, 200);
    assert_eq!(masked(&got.body), expected["body"].as_str().unwrap());
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
    // La tarjeta claude viva necesita el contexto: lo calculó el frente, sin
    // pedirle nada al heredado (Tarea 5a de la 2e).
    assert!(
        !seen.iter().any(|l| l.starts_with("GET /usage/")),
        "subconsulta al heredado: {seen:?}"
    );
    // Una consulta también es nativa y sale de la caché.
    let again = get(front.port, "/state?x=1").await;
    assert_eq!(again.body, got.body);
    front.stop().await;
}

/// GET /state con una tarjeta `working` de 17 min (`round` estable): sin
/// llamadas recientes en la base de uso la sugerencia es «parece colgado»;
/// con un turno reciente del proyecto, la guardia del frente la quita. Las dos
/// contra el cc-dash real sobre el mismo HOME (Tarea 5a de la 2e).
async fn state_guard_suggestion_matches_python(tag: &str, recent_turn: bool) {
    let home = TestHome::new(tag);
    if seed(&home).is_none() {
        return;
    }
    let now = NOW_MS;
    let ts = now as f64 / 1000.0 - 17.25 * 60.0;
    home.write(
        "state/a.json",
        &json!({"session":"proj","agent":"claude","status":"working","detail":"x","ts":ts})
            .to_string(),
    );
    let sql = if recent_turn {
        turn_sql(RECENT_TURN, now)
    } else {
        String::new()
    };
    support::seed_usage(&home, &sql);
    let (expected, _py) = frozen_state(&home, tag).await;
    assert_eq!(expected["status"], 200);
    let legacy = FakeLegacy::start().await;
    let relay = Relay::start(legacy.port).await;
    let mut opts = home.options();
    opts.clock_seconds = Arc::new(|| NOW_MS as f64 / 1000.0);
    let front = front(&home, relay.port, opts).await;
    let got = get(front.port, "/state").await;
    assert_eq!(got.status, 200, "{}", got.text());
    assert_eq!(masked(&got.body), expected["body"].as_str().unwrap());
    let cards: Value = serde_json::from_slice(&got.body).unwrap();
    let claude = cards
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["session"] == "proj")
        .unwrap();
    assert_eq!(claude["status"], "working");
    let hung = claude["suggestion"]["text"]
        .as_str()
        .is_some_and(|t| t.contains("parece colgado"));
    assert_eq!(hung, !recent_turn, "{claude}");
    let seen = relay.requests();
    assert!(
        !seen
            .iter()
            .any(|l| l.starts_with("GET /state") || l.starts_with("GET /usage/")),
        "{seen:?}"
    );
    front.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn state_hung_suggestion_matches_python_without_recent_calls() {
    state_guard_suggestion_matches_python("state-guard-hung", false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn state_native_guard_suppresses_hung_like_python() {
    state_guard_suggestion_matches_python("state-guard-calls", true).await;
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
    // El contexto es nativo; nada se reenvía a este heredado.
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

/// Un turno de Claude reciente del proyecto `Proyecto` (la etiqueta de la
/// pestaña de `seed`): la guardia lo cuenta en `calls10m`.
const RECENT_TURN: &str = "insert into usage_turns (id, provider, agent, tmux_session, tmux_pane, \
     pane_pwd, git_root, model, turn_started_at, turn_finished_at, total_tokens, source, confidence) \
     values ('t1', 'claude', 'claude', 'proj', '%1', '/w/Proyecto', '/w/Proyecto', 'claude-opus-5', \
     {start}, {end}, 1000, 'claude_jsonl', 'exact');";

/// Un turno con `model` BLOB: el frente no sabe qué haría el Python → declina.
const BLOB_TURN: &str = "insert into usage_turns (id, provider, agent, tmux_session, tmux_pane, \
     pane_pwd, git_root, model, turn_started_at, turn_finished_at, source, confidence) values \
     ('blob', 'claude', 'claude', 'proj', '%1', '/w', '/w', X'636c617564652d6f707573', {start}, {end}, \
     'claude_jsonl', 'exact');";

fn turn_sql(template: &str, now_ms: i64) -> String {
    let end = now_ms / 1000 - 60;
    template
        .replace("{start}", &(end - 30).to_string())
        .replace("{end}", &end.to_string())
}

#[tokio::test(flavor = "current_thread")]
async fn suggestion_context_is_native() {
    use comandos_server::dash::native::{Native, states::context::Context, usage::guard};
    // La guardia y la latencia salen de la base de uso del frente, se cachean
    // 60 s y no se pide nada al heredado (que ni existe: puerto muerto).
    let home = TestHome::new("state-ctx-native");
    support::seed_usage(&home, &turn_sql(RECENT_TURN, NOW_MS));
    let mut opts = home.options();
    opts.legacy = std::net::SocketAddr::from(([127, 0, 0, 1], dead_port()));
    let native = Native::new(opts);
    let ctx = Context::default();
    let registry = json!({});
    let got = ctx.get(&native, &registry, NOW_MS).await.unwrap();
    assert_eq!(got.guard["projects"][0]["project"], json!("Proyecto"));
    assert_eq!(got.guard["projects"][0]["calls10m"], json!(1));
    assert_eq!(got.guard["forecastLevel"], json!("normal"));
    assert_eq!(
        Some(&got.guard),
        guard::token_guard_with_forecast(&native)
            .await
            .ok()
            .flatten()
            .as_ref()
    );
    assert!(got.latency.is_empty());
    assert!(got.routes.is_empty());
    // Dentro de 60 s, el mismo contexto aunque la base cambie; después, nuevo.
    support::seed_usage(&home, "delete from usage_turns;");
    let cached = ctx.get(&native, &registry, NOW_MS + 60_000).await.unwrap();
    assert!(Arc::ptr_eq(&got, &cached));
    let fresh = ctx.get(&native, &registry, NOW_MS + 60_001).await.unwrap();
    assert_eq!(fresh.guard["projects"], json!([]));
}

#[tokio::test(flavor = "current_thread")]
async fn suggestion_context_slow_lane_declines_within_two_seconds() {
    use comandos_server::dash::native::{
        Native,
        states::context::{Context, QUERY_TIMEOUT},
    };
    // El carril de uso ocupado por otro trabajo largo: las consultas del
    // contexto tienen el plazo de la subconsulta de la 2d (2 s) y, al vencer,
    // declinan y el fallo se recuerda. El runtime sigue libre.
    let home = TestHome::new("state-ctx-slow");
    support::seed_usage(&home, &turn_sql(RECENT_TURN, NOW_MS));
    let native = Native::new(home.options());
    let lane = native.usage_lane();
    // El carril ya abierto: el trabajo lento entra primero.
    assert!(lane.with(|_| ()).await.is_ok());
    let ctx = Context::default();
    let registry = json!({});
    let slow = async {
        assert!(
            lane.with(|_| std::thread::sleep(Duration::from_secs(4)))
                .await
                .is_ok()
        );
    };
    let timed = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let started = std::time::Instant::now();
        let got = ctx.get(&native, &registry, NOW_MS).await;
        (got.is_err(), started.elapsed())
    };
    let ((), (declined, waited)) = tokio::join!(slow, timed);
    assert!(declined);
    assert!(
        waited >= QUERY_TIMEOUT && waited < QUERY_TIMEOUT + Duration::from_millis(700),
        "{waited:?}"
    );
    assert!(ctx.failing(NOW_MS));
    // Libre el carril y pasado el recuerdo del fallo, se calcula.
    let got = ctx.get(&native, &registry, NOW_MS + 5_000).await.unwrap();
    assert_eq!(got.guard["projects"][0]["project"], json!("Proyecto"));
}

#[tokio::test(flavor = "current_thread")]
async fn suggestion_context_uncertain_declines_and_is_remembered() {
    use comandos_server::dash::native::{Native, states::context::Context};
    // Un valor no decodificable en la base declina y el fallo se recuerda
    // 5 s; pasado ese plazo, con la base limpia, el contexto se calcula.
    let home = TestHome::new("state-ctx-blob");
    support::seed_usage(&home, &turn_sql(BLOB_TURN, NOW_MS));
    let native = Native::new(home.options());
    let ctx = Context::default();
    let registry = json!({});
    assert!(ctx.get(&native, &registry, NOW_MS).await.is_err());
    assert!(ctx.failing(NOW_MS + 4_999));
    support::seed_usage(&home, "delete from usage_turns;");
    assert!(ctx.get(&native, &registry, NOW_MS + 4_999).await.is_err());
    assert!(!ctx.failing(NOW_MS + 5_000));
    let got = ctx.get(&native, &registry, NOW_MS + 5_000).await.unwrap();
    assert_eq!(got.guard["projects"], json!([]));
    assert!(!ctx.failing(NOW_MS + 5_001));

    // Carril de uso apagado (una base más nueva): declina.
    let home = TestHome::new("state-ctx-lane-down");
    support::seed_usage(&home, "pragma user_version = 12;");
    let native = Native::new(home.options());
    let ctx = Context::default();
    assert!(ctx.get(&native, &registry, NOW_MS).await.is_err());
    assert!(ctx.failing(NOW_MS));

    // Sin efectos de uso (la sombra) la caché de límites nunca se llena: el
    // frente no sabe qué pronóstico ve el Python → declina.
    let home = TestHome::new("state-ctx-no-limits");
    let mut opts = home.options();
    opts.usage_effects = false;
    let native = Native::new(opts);
    let ctx = Context::default();
    assert!(ctx.get(&native, &registry, NOW_MS).await.is_err());
    assert!(ctx.failing(NOW_MS + 4_999));
}

/// Heredado que responde `usage` a `/usage/*` (el frente ya no las pide: la
/// prueba lo comprueba) y `{"legacy": true}` al resto (el `/state` reenviado).
/// Anota cada línea.
struct SwitchLegacy {
    port: u16,
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
        Self { port, seen }
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
    support::seed_usage(&home, &turn_sql(BLOB_TURN, NOW_MS));
    let legacy = SwitchLegacy::start("{}").await;
    let mut opts = home.options();
    opts.tmux.program.path = wrapper;
    let front = front(&home, legacy.port, opts).await;
    // 1: calcula, la guardia no se puede reproducir → declina y recuerda el fallo.
    assert_eq!(
        get(front.port, "/state").await.text(),
        r#"{"legacy": true}"#
    );
    let calls = tmux_calls(&home);
    assert!(calls > 0, "el primer cómputo no llamó a tmux");
    // 2: dentro de los 5 s del fallo, se reenvía sin tmux.
    assert_eq!(
        get(front.port, "/state").await.text(),
        r#"{"legacy": true}"#
    );
    assert_eq!(tmux_calls(&home), calls);
    assert_eq!(legacy.usage_requests(), 0, "{:?}", legacy.requests());
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
    support::seed_usage(&home, &turn_sql(BLOB_TURN, NOW_MS));
    let legacy = SwitchLegacy::start("{}").await;
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
    // Pasada la memoria del fallo, el contexto ya se puede calcular.
    support::seed_usage(&home, "delete from usage_turns;");
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
async fn suggestion_context_routes_failure_skips_usage_and_is_remembered() {
    use comandos_server::dash::native::{Native, states::context::Context};
    // Revisión de la Tarea 5: las rutas se calculan antes de la guardia; si
    // fallan no se abre la base de uso ni se piden los límites, y el fallo se
    // recuerda 5 s.
    let home = TestHome::new("state-ctx-routes");
    let mut opts = home.options();
    let repo = opts.repo_root.take();
    let native = Native::new(opts.clone());
    let ctx = Context::default();
    let registry = json!({});
    assert!(!ctx.failing(NOW_MS));
    assert!(ctx.get(&native, &registry, NOW_MS).await.is_err());
    assert!(!home.usage_db().exists(), "la guardia abrió la base");
    assert!(!native.limits().refreshing());
    assert!(!native.limits().current().loaded);
    assert!(ctx.failing(NOW_MS + 4_999));
    opts.repo_root = repo;
    let native = Native::new(opts);
    assert!(ctx.get(&native, &registry, NOW_MS + 4_999).await.is_err());
    assert!(!home.usage_db().exists());
    assert!(!ctx.failing(NOW_MS + 5_000));
    assert!(ctx.get(&native, &registry, NOW_MS + 5_000).await.is_ok());
    assert!(home.usage_db().exists());
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
