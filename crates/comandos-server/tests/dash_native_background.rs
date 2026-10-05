//! 2f-3/T6: GET `/models/latest`, el vigilante de modelos (ciclo, ciclo
//! forzado de `?refresh=1`, bucle de 90/600 s) y el bucle de límites de 300 s.
//!
//! Confinamiento: los CLIs son guiones del `fakebin` (o del `bin` del HOME)
//! que anotan cada llamada en `$HOME/cli.log`; los feeds de noticias y el
//! cc-notifyd son servidores HTTP de la propia prueba en puertos efímeros de
//! `127.0.0.1` (el oráculo reescribe sus URLs fijas hacia ellos; el frente
//! recibe sus `Endpoints` y un `NotifyPost` falso). El OAuth de los límites es
//! el falso de `support`. Los relojes de los bucles van inyectados
//! (`start_with`). Sin tmux salvo el privado del gemelo.
mod support;

use comandos_runtime::news_watch::Endpoints;
use comandos_server::dash::native::{
    Background, Native, background,
    background::models::{self, Feeds},
};
use regex::Regex;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};
use support::{
    FakeAnswer, FakeLegacy, FakeNotify, FakeOauth, NOW_MS, TestHome, front, get,
    twin::{Twin, TwinOpts},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

// ---------------------------------------------------------------------------
// Servidor HTTP mínimo de la prueba (feeds y cc-notifyd)
// ---------------------------------------------------------------------------

type Route = (u16, Vec<(String, String)>, Vec<u8>);

struct Http {
    port: u16,
    log: Arc<Mutex<Vec<(String, String, String)>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Http {
    async fn start(routes: impl Fn(&str, &str) -> Route + Send + Sync + 'static) -> Http {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let routes = Arc::new(routes);
        let task = tokio::spawn({
            let log = Arc::clone(&log);
            async move {
                while let Ok((mut conn, _)) = listener.accept().await {
                    let (log, routes) = (Arc::clone(&log), Arc::clone(&routes));
                    tokio::spawn(async move {
                        let mut buf = Vec::new();
                        let mut chunk = [0u8; 4096];
                        let head_end = loop {
                            let Ok(n) = conn.read(&mut chunk).await else {
                                return;
                            };
                            if n == 0 {
                                return;
                            }
                            buf.extend_from_slice(&chunk[..n]);
                            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                break i + 4;
                            }
                        };
                        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
                        let mut lines = head.lines();
                        let mut first = lines.next().unwrap_or_default().split(' ');
                        let method = first.next().unwrap_or_default().to_owned();
                        let path = first.next().unwrap_or_default().to_owned();
                        let length: usize = lines
                            .filter_map(|l| l.split_once(':'))
                            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, v)| v.trim().parse().ok())
                            .unwrap_or(0);
                        while buf.len() < head_end + length {
                            let Ok(n) = conn.read(&mut chunk).await else {
                                return;
                            };
                            if n == 0 {
                                break;
                            }
                            buf.extend_from_slice(&chunk[..n]);
                        }
                        let body = String::from_utf8_lossy(&buf[head_end..]).into_owned();
                        log.lock()
                            .unwrap()
                            .push((method.clone(), path.clone(), body));
                        let (status, headers, payload) = routes(&method, &path);
                        let mut out = format!(
                            "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                            payload.len()
                        );
                        for (k, v) in headers {
                            out.push_str(&format!("{k}: {v}\r\n"));
                        }
                        out.push_str("\r\n");
                        let _ = conn.write_all(out.as_bytes()).await;
                        let _ = conn.write_all(&payload).await;
                        let _ = conn.shutdown().await;
                    });
                }
            }
        });
        Http { port, log, task }
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn requests(&self) -> Vec<(String, String, String)> {
        self.log.lock().unwrap().clone()
    }
}

impl Drop for Http {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn now_secs() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
}

fn ok_json(value: Value) -> Route {
    (200, vec![], value.to_string().into_bytes())
}

/// Los feeds: tres consultas de HN (directa, 301 y 308) y una de Reddit; el
/// resto 404 (cuenta como fallo de esa fuente en los dos lados).
fn feed_routes(recent: i64) -> impl Fn(&str, &str) -> Route + Send + Sync + 'static {
    move |_method, path| match path {
        "/api/v1/search_by_date?query=%22claude+code%22+skill&tags=story&hitsPerPage=8" => {
            ok_json(json!({"hits": [
                {"title": "Claude Code skill pack", "url": "https://ex.test/skill", "created_at_i": recent},
                {"title": "Show HN: MCP server", "objectID": 77, "created_at_i": recent - 5}
            ]}))
        }
        "/api/v1/search_by_date?query=%22claude%22+skills&tags=story&hitsPerPage=8" => (
            301,
            vec![("Location".into(), "/moved/hn".into())],
            Vec::new(),
        ),
        "/moved/hn" => ok_json(json!({"hits": [
            {"title": "Anthropic skills via 301", "url": "https://ex.test/301", "created_at_i": recent - 1}
        ]})),
        "/api/v1/search_by_date?query=mcp+server&tags=story&hitsPerPage=8" => (
            308,
            vec![("Location".into(), "/moved308/hn".into())],
            Vec::new(),
        ),
        "/moved308/hn" => ok_json(json!({"hits": [
            {"title": "MCP via 308", "url": "https://ex.test/308", "created_at_i": recent - 2}
        ]})),
        "/r/ClaudeAI/new.json?limit=15" => ok_json(json!({"data": {"children": [
            {"data": {"title": "New MCP server released", "permalink": "/r/ClaudeAI/9", "created_utc": recent as f64 + 0.5}}
        ]}})),
        _ => (404, vec![], b"{}".to_vec()),
    }
}

fn notify_routes(_method: &str, _path: &str) -> Route {
    (200, vec![], b"ok".to_vec())
}

/// Prólogo del oráculo: las URLs fijas de los feeds y de cc-notifyd van a los
/// servidores de la prueba (el resto de la red sigue cerrada).
fn prelude(feed: u16, notify: u16) -> String {
    format!(
        r#"
import urllib.request as _ur
_twin_open = _ur.urlopen
_twin_feed = "http://127.0.0.1:{feed}"
_twin_hosts = ("https://hn.algolia.com", "https://www.reddit.com", "https://api.github.com",
               "https://devpost.com", "https://earn.superteam.fun", "https://dorahacks.io")
def _twin_rw(url):
    for host in _twin_hosts:
        if url.startswith(host):
            return _twin_feed + url[len(host):]
    if url.startswith("http://127.0.0.1:4778/"):
        return "http://127.0.0.1:{notify}/" + url[len("http://127.0.0.1:4778/"):]
    return url
def _twin_urlopen(req, *args, **kwargs):
    if isinstance(req, _ur.Request):
        req.full_url = _twin_rw(req.full_url)
    else:
        req = _twin_rw(req)
    return _twin_open(req, *args, **kwargs)
_ur.urlopen = _twin_urlopen
"#
    )
}

/// CLI falso: anota `nombre argv` en `$HOME/cli.log` y responde.
fn fake_cli(version: &str, extra: &str) -> String {
    format!(
        "#!/bin/sh\nprintf '%s\\n' \"$(basename \"$0\") $*\" >> \"$HOME/cli.log\"\ncase \"$1\" in\n--version) echo '{version}';;\n--help) printf 'Usage: x [options]\\n\\nOptions:\\n  --model <m>   Model\\n';;\n{extra}esac\n"
    )
}

fn fakes() -> Vec<(String, String)> {
    vec![
        ("claude".into(), fake_cli("2.1.286 (Claude Code)", "")),
        (
            "grok".into(),
            fake_cli(
                "grok 1.0.44",
                "models) printf '  - grok-4.7\\n  - grok-99.0\\n';;\n",
            ),
        ),
    ]
}

fn seed_cycle(home: &TestHome) {
    let versions = home.root.join(".local/share/claude/versions");
    std::fs::create_dir_all(&versions).unwrap();
    std::fs::write(
        versions.join("2.1.286"),
        "\0claude-opus-9-1\0claude-opus-5-5\0",
    )
    .unwrap();
    // Un snapshot de noticias previo: lo nuevo de esta vuelta es noticia.
    home.write(
        "news-watch.json",
        r#"{"checkedAt": 1, "seen": [], "recent": []}"#,
    );
}

fn unhome(home: &TestHome, text: &str) -> String {
    text.replace(&home.root.display().to_string(), "<HOME>")
}

/// Lo que depende del reloj de cada lado en los snapshots.
fn clockless(text: &str) -> String {
    let re = Regex::new(r#""(checkedAt|heartbeatAt|at|versionsAt)": [0-9.]+"#).unwrap();
    re.replace_all(text, r#""$1": T"#).into_owned()
}

fn read(home: &TestHome, name: &str) -> String {
    std::fs::read_to_string(home.hooks().join(name)).unwrap_or_default()
}

fn cli_log(home: &TestHome) -> Vec<String> {
    std::fs::read_to_string(home.root.join("cli.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

// ---------------------------------------------------------------------------
// GET /models/latest
// ---------------------------------------------------------------------------

#[tokio::test]
async fn models_latest_matches_python() {
    let Some(t) = Twin::start("models-latest", |_| {}).await else {
        return;
    };
    let cases = [
        None,
        Some(
            r#"{"checkedAt": 1759680000, "versions": {"claude": "2.1.286"}, "heartbeatAt": 5, "nota": "ñ"}"#,
        ),
        Some("[]"),
        Some("[1, 2.50]"),
        Some("{roto"),
        Some("0"),
    ];
    for case in cases {
        for home in [&t.a, &t.b] {
            let path = home.hooks().join("model-watch.json");
            match case {
                Some(text) => std::fs::write(&path, text).unwrap(),
                None => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        for path in ["/models/latest", "/models/latest?x=1"] {
            let run = t.get(path).await;
            assert_eq!(run.front.status, 200, "{case:?}");
            assert_eq!(run.front.status, run.oracle.status, "{case:?}");
            assert_eq!(run.front.text(), run.oracle.text(), "{case:?}");
        }
    }
}

// ---------------------------------------------------------------------------
// El ciclo forzado de ?refresh=1 contra el Python
// ---------------------------------------------------------------------------

/// `GET /commands/catalog?refresh=1`: un ciclo completo en cada lado (CLIs,
/// binario versionado, `grok models`, feeds con 301 y 308, avisos a
/// cc-notifyd) y la misma respuesta, los mismos snapshots, el mismo
/// historial de noticias, las mismas llamadas a los CLIs y los mismos avisos.
#[tokio::test]
async fn refresh_cycle_matches_python() {
    let recent = now_secs() - 600;
    let feed = Http::start(feed_routes(recent)).await;
    let notifyd = Http::start(notify_routes).await;
    let notify = Arc::new(FakeNotify::default());
    let base = feed.base();
    let front_notify = Arc::clone(&notify);
    let front_base = base.clone();
    let opts = TwinOpts {
        fakebin_extra: fakes(),
        front: Some(Box::new(move |o| {
            o.notifyd = front_notify;
            o.news_feeds = Arc::new(Feeds::new(Endpoints::local(&front_base)));
        })),
        python_prelude: prelude(feed.port, notifyd.port),
        allow_ports: vec![feed.port, notifyd.port],
        oracle_env: vec![("COMANDOS_SEARX".into(), base.clone())],
        ..TwinOpts::default()
    };
    let Some(t) = Twin::start_with("mw-refresh", seed_cycle, opts).await else {
        return;
    };
    let run = t.get("/commands/catalog?refresh=1&session=s").await;
    assert_eq!(run.front.status, 200, "{}", run.front.text());
    assert_eq!(run.oracle.status, 200);
    assert_eq!(
        clockless(&unhome(&t.a, &run.front.text())),
        clockless(&unhome(&t.b, &run.oracle.text()))
    );
    let snap = read(&t.a, "model-watch.json");
    assert!(
        snap.contains("claude-opus-9-1") && snap.contains("grok-99.0"),
        "{snap}"
    );
    assert!(snap.contains("\"heartbeatAt\""), "{snap}");
    assert_eq!(clockless(&snap), clockless(&read(&t.b, "model-watch.json")));
    let news = read(&t.a, "news-watch.json");
    assert!(
        news.contains("https://ex.test/308") && news.contains("https://ex.test/301"),
        "{news}"
    );
    assert_eq!(clockless(&news), clockless(&read(&t.b, "news-watch.json")));
    assert_eq!(cli_log(&t.a), cli_log(&t.b));
    // Avisos: los cuerpos que el frente manda a su cc-notifyd falso son los que
    // el Python mandó al de la prueba.
    let python: Vec<String> = notifyd
        .requests()
        .into_iter()
        .filter(|(m, p, _)| m == "POST" && p == "/notify")
        .map(|(_, _, body)| body)
        .collect();
    assert_eq!(python.len(), 3, "{python:?}");
    assert_eq!(notify.bodies(), python);
    // Los mismos feeds pedidos por los dos lados (con sus redirecciones).
    let mut paths: Vec<String> = feed.requests().into_iter().map(|(_, p, _)| p).collect();
    paths.sort();
    let mut once: Vec<String> = paths.clone();
    once.dedup();
    assert_eq!(paths.len(), once.len() * 2, "{paths:?}");
    // Una segunda petición sin cambios coalesce en un ciclo nuevo pero sin
    // noticias: ningún aviso más.
    let run = t.get("/commands/catalog?refresh=1").await;
    assert_eq!(run.front.status, 200);
    assert_eq!(notify.bodies().len(), 3);
}

// ---------------------------------------------------------------------------
// Frente solo
// ---------------------------------------------------------------------------

/// Un `Native` con los CLIs en `<HOME>/bin` y los feeds en un puerto que no
/// escucha.
fn lone_native(home: &TestHome, background: Background) -> Arc<Native> {
    for (name, text) in fakes() {
        support::oracle::write_executable(&home.root.join("bin").join(name), &text);
    }
    let mut opts = home.options();
    opts.user_bin_dirs = support::twin::home_bin_dirs();
    opts.background = background;
    opts.news_feeds = Arc::new(Feeds::new(Endpoints::local("http://127.0.0.1:1")));
    Arc::new(Native::new(opts))
}

fn versions_calls(home: &TestHome) -> usize {
    cli_log(home)
        .iter()
        .filter(|l| l.ends_with("--version"))
        .count()
}

/// Dos `force_cycle` a la vez: un solo escaneo (el segundo coalesce porque el
/// primero terminó después de que él llegara).
#[tokio::test]
async fn cycle_coalesces() {
    let home = TestHome::new_short("mw-coalesce");
    let native = lone_native(&home, Background::legacy());
    assert!(native.ready().await);
    tokio::join!(models::force_cycle(&native), models::force_cycle(&native));
    // Un ciclo completo: `installed_versions` del ciclo y el de `watch_models`.
    assert_eq!(versions_calls(&home), 4, "{:?}", cli_log(&home));
    assert_eq!(
        cli_log(&home)
            .iter()
            .filter(|l| l.as_str() == "grok models")
            .count(),
        1
    );
    // Uno nuevo, llegado después: otro escaneo.
    models::force_cycle(&native).await;
    assert_eq!(versions_calls(&home), 8);
    // Un ciclo no forzado sin cambios: solo versiones y el latido.
    let before = read(&home, "model-watch.json");
    models::cycle(&native, false).await;
    assert_eq!(versions_calls(&home), 10);
    let after: Value = serde_json::from_str(&read(&home, "model-watch.json")).unwrap();
    let before: Value = serde_json::from_str(&before).unwrap();
    assert_eq!(after["checkedAt"], before["checkedAt"]);
    assert!(after["heartbeatAt"].as_i64().unwrap() >= before["heartbeatAt"].as_i64().unwrap());
    native.shutdown().await;
}

/// D6: con `legacy` solo arranca el planificador de Pomodoro; con `front`,
/// también el vigilante de modelos y el bucle de límites. `stop` los para.
#[tokio::test]
async fn background_legacy_starts_only_pomodoro() {
    for (background, owner) in [(Background::legacy(), false), (Background::front(), true)] {
        let home = TestHome::new_short(if owner { "bg-front" } else { "bg-legacy" });
        let native = lone_native(&home, background);
        assert!(native.ready().await);
        let runner = background::start(&native);
        assert!(runner.pomodoro());
        assert_eq!(runner.models(), owner);
        assert_eq!(runner.limits(), owner);
        assert_eq!(native.tasks().len(), if owner { 3 } else { 1 });
        runner.stop();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !native.tasks().is_empty() {
            assert!(Instant::now() < deadline, "los bucles no pararon");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // Ningún ciclo llegó a correr (espera inicial de 90 s).
        assert_eq!(versions_calls(&home), 0);
        native.shutdown().await;
    }
}

/// El bucle del vigilante con reloj inyectado: ciclo completo, después
/// ciclos baratos (solo versiones) con su latido.
#[tokio::test]
async fn model_loop_runs_cycles_with_heartbeat() {
    let home = TestHome::new_short("mw-loop");
    let native = lone_native(&home, Background::front());
    assert!(native.ready().await);
    let stop = Arc::new(background::Stop::default());
    assert!(models::start_with(
        &native,
        Arc::clone(&stop),
        Duration::from_millis(10),
        Duration::from_millis(50)
    ));
    let deadline = Instant::now() + Duration::from_secs(20);
    // Completo (4) + al menos dos baratos (2 cada uno).
    while versions_calls(&home) < 8 {
        assert!(Instant::now() < deadline, "{:?}", cli_log(&home));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    stop.set();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !native.tasks().is_empty() {
        assert!(Instant::now() < deadline, "el bucle no paró");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let grok = cli_log(&home)
        .iter()
        .filter(|l| l.as_str() == "grok models")
        .count();
    assert_eq!(grok, 1, "solo el primer ciclo es completo");
    let snap: Value = serde_json::from_str(&read(&home, "model-watch.json")).unwrap();
    assert!(snap["heartbeatAt"].as_i64().unwrap() > 1_700_000_000);
    assert_eq!(snap["versions"]["claude"], "2.1.286");
    native.shutdown().await;
}

/// El bucle de límites: cada periodo, `usage_provider_limits()` (un refresco
/// si la caché venció). Con el reloj movido entre vueltas, cada vuelta pide
/// los límites de nuevo.
#[tokio::test]
async fn limits_loop_refreshes_every_period() {
    let home = TestHome::new_short("lim-loop");
    std::fs::create_dir_all(home.root.join(".claude")).unwrap();
    std::fs::write(
        home.root.join(".claude/.credentials.json"),
        r#"{"claudeAiOauth": {"accessToken": "tok-de-prueba"}}"#,
    )
    .unwrap();
    let oauth = Arc::new(FakeOauth::default());
    oauth.set(
        "tok-de-prueba",
        FakeAnswer::Error("HTTP Error 500: x".into()),
    );
    let clock = Arc::new(AtomicI64::new(NOW_MS));
    let mut opts = home.options();
    opts.oauth = oauth.clone();
    opts.background = Background::front();
    opts.clock = {
        let clock = Arc::clone(&clock);
        Arc::new(move || clock.load(Ordering::SeqCst))
    };
    let native = Arc::new(Native::new(opts));
    assert!(native.ready().await);
    let stop = Arc::new(background::Stop::default());
    assert!(background::limits::start_with(
        &native,
        Arc::clone(&stop),
        Duration::from_millis(30)
    ));
    let deadline = Instant::now() + Duration::from_secs(10);
    for want in 1..=3 {
        while oauth.calls() < want {
            assert!(
                Instant::now() < deadline,
                "vuelta {want}: {}",
                oauth.calls()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // Con el reloj quieto la caché sigue fresca: ninguna petición más.
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(oauth.calls(), want);
        clock.fetch_add(10 * 60 * 1000, Ordering::SeqCst);
    }
    stop.set();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !native.tasks().is_empty() {
        assert!(Instant::now() < deadline, "el bucle no paró");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    native.shutdown().await;
}

/// `?refresh=1` ya no declina: corre el ciclo forzado y responde el frente.
#[tokio::test]
async fn catalog_refresh_is_native() {
    let home = TestHome::new_short("mw-cat-refresh");
    let native = lone_native(&home, Background::legacy());
    let opts = native.options().clone();
    let legacy = FakeLegacy::start().await;
    let server = front(&home, legacy.port, opts).await;
    let wire = get(server.port, "/commands/catalog?refresh=1&session=s").await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert!(legacy.requests().is_empty());
    assert_eq!(versions_calls(&home), 4, "{:?}", cli_log(&home));
    assert!(home.hooks().join("model-watch.json").exists());
    let doc: Value = serde_json::from_str(&wire.text()).unwrap();
    let snap: Value = serde_json::from_str(&read(&home, "model-watch.json")).unwrap();
    // `float(snap["checkedAt"])`.
    assert_eq!(doc["versionsAt"].as_f64(), snap["checkedAt"].as_f64());
    assert!(
        wire.text()
            .contains(&format!("\"versionsAt\": {}.0", snap["checkedAt"]))
    );
    server.stop().await;
}
