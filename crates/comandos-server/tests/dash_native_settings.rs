//! Ajustes del tablero nativos (plan 2f-3, Tarea 1) contra el `cc-dash`
//! Python, con el gemelo (sin tmux en estas rutas).
//!
//! Confinamiento: cada lado tiene su HOME temporal; `cc-notify.conf`,
//! `dash-token` y las carpetas de `/fs/*` viven solo ahí. `xdg-open`,
//! `pw-play`, `spd-say`, `piper`… son los falsos del `fakebin` que solo anotan
//! (`<HOME>/fakebin.log`); `/bin/sh` real solo corre el guion de la voz, que
//! llama a esos falsos. `/notify-popup` válido va con `DESKTOP_NOTIFY=0` (sin
//! red en esas filas); el caso encendido usa un notifyd HTTP efímero privado.
mod support;

use comandos_server::dash::native::{files::FileLock, settings};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use support::{
    FakeLegacy, TestHome, front, get, request_body,
    services::{calls, mode, seed_services, unhome},
};

struct FrozenSettings<'a> {
    a: &'a TestHome,
    b: &'a TestHome,
    front: support::Front,
    py: support::http_golden::FrozenHttp<'a>,
    family: &'static str,
}
impl<'a> FrozenSettings<'a> {
    async fn new(a: &'a TestHome, b: &'a TestHome, family: &'static str) -> Self {
        let opts = support::http_golden::confined_front_options(a, &[]);
        let front = front(a, support::dead_port(), opts).await;
        let mut py =
            support::http_golden::FrozenHttp::new(b, family, &["cc-notify.conf", "dash-token"])
                .await;
        py.sequence_requests();
        Self {
            a,
            b,
            front,
            py,
            family,
        }
    }
    async fn request(&self, method: &str, path: &str, body: &str) -> support::twin::TwinRun {
        let oracle = self.py.request(method, path, "", body).await;
        let front = request_body(self.front.port, method, path, "", body).await;
        support::twin::TwinRun { front, oracle }
    }
    fn files_equal(&self, files: &[&str]) -> Result<(), String> {
        for file in files {
            assert_eq!(
                std::fs::read(self.a.hooks().join(file)).unwrap(),
                std::fs::read(self.b.hooks().join(file)).unwrap()
            );
        }
        Ok(())
    }
    async fn calls(&self, expected: usize, step: usize) -> Vec<(String, Vec<String>)> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let source_ready = self.py.source_port().is_none()
                || (calls(self.b).len() >= expected && calls(self.a) == calls(self.b));
            if calls(self.a).len() >= expected && source_ready {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "private command actors did not produce expected calls"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let bytes = comandos_oracle::oracle_at(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            "settings-calls",
            &json!({"source":support::frozen::SOURCE_COMMIT,"python":"3.10.12","fixture":self.family,"step":step,"minimum_calls":expected}),
            || serde_json::to_vec(&calls(self.b)).map_err(|e| e.to_string()),
        );
        let expected: Vec<(String, Vec<String>)> = serde_json::from_slice(&bytes).unwrap();
        while calls(self.a) != expected && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(
            calls(self.a),
            expected,
            "actual private Native command transcript"
        );
        expected
    }
}

/// La misma petición a los dos lados: estado y cuerpo iguales con las rutas
/// de cada HOME normalizadas; después `cc-notify.conf` (bytes y modo) y las
/// llamadas a los falsos.
async fn same(t: &FrozenSettings<'_>, method: &str, path: &str, body: &str) -> (u16, String) {
    let run = t.request(method, path, body).await;
    let (fa, fb) = (
        unhome(t.a, &run.front.text()),
        unhome(t.b, &run.oracle.text()),
    );
    assert_eq!(
        run.front.status, run.oracle.status,
        "{method} {path} {body}: {fa} / {fb}"
    );
    assert_eq!(fa, fb, "{method} {path} {body}");
    t.files_equal(&["cc-notify.conf"]).unwrap();
    let conf = |h: &TestHome| mode(&h.hooks().join("cc-notify.conf"));
    assert_eq!(conf(t.a), conf(t.b), "modo de cc-notify.conf");
    (run.front.status, fa)
}

#[tokio::test]
async fn settings_routes_match_python() {
    let (a, b) = (TestHome::new("settings-a"), TestHome::new("settings-b"));
    for h in [&a, &b] {
        seed_services(h);
    }
    let t = FrozenSettings::new(&a, &b, "settings-routes").await;
    let mut step = 0;
    let mut launched = 0;
    for (method, path, body, launches) in [
        ("GET", "/conf", "", 0),
        ("GET", "/conf?x=1", "", 0),
        ("POST", "/conf-set", r#"{"key":"VOLUME","value":"55"}"#, 0),
        ("POST", "/conf-set", r#"{"key":"VOLUME","value":"101"}"#, 0),
        ("POST", "/conf-set", r#"{"key":"VOLUME","value":"007"}"#, 0),
        ("POST", "/conf-set", r#"{"key":"VOLUME","value":""}"#, 0),
        ("POST", "/conf-set", r#"{"key":"CC_LANG","value":"en"}"#, 0),
        ("POST", "/conf-set", r#"{"key":"CC_LANG","value":"fr"}"#, 0),
        ("POST", "/conf-set", r#"{"key":"SPEAK_DONE","value":0}"#, 0),
        (
            "POST",
            "/conf-set",
            r#"{"key":"SPEAK_DONE","value":"2"}"#,
            0,
        ),
        ("POST", "/conf-set", r#"{"key":"OTRA","value":"1"}"#, 0),
        ("POST", "/conf-set", r#"{"key":5,"value":"1"}"#, 0),
        ("POST", "/conf-set", r#"{"key":["VOLUME"],"value":"1"}"#, 0),
        ("POST", "/conf-set", r#"{"value":"1"}"#, 0),
        ("POST", "/conf-set", r#"{"key":"VOLUME","value":"55"}"#, 0),
        ("GET", "/conf", "", 0),
        ("GET", "/fs/dirs?path=~/codebase", "", 0),
        ("GET", "/fs/dirs?path=~/codebase/nue", "", 0),
        ("GET", "/fs/dirs?path=~/codebase/.o", "", 0),
        ("GET", "/fs/dirs?path=~/codebase/zz/yy", "", 0),
        ("GET", "/fs/dirs", "", 0),
        ("GET", "/fs/dirs?path=", "", 0),
        ("GET", "/fs/dirs?path=/etc", "", 0),
        ("GET", "/fs/dirs?path=~/../..", "", 0),
        ("POST", "/fs/mkdir", r#"{"path":"~/codebase/nueva"}"#, 0),
        ("POST", "/fs/mkdir", r#"{"path":"~/codebase/nueva"}"#, 0),
        ("POST", "/fs/mkdir", r#"{"path":"~/codebase/x/y/z"}"#, 0),
        ("POST", "/fs/mkdir", r#"{"path":"~/codebase/archivo"}"#, 0),
        (
            "POST",
            "/fs/mkdir",
            r#"{"path":"~/codebase/archivo/sub"}"#,
            0,
        ),
        ("POST", "/fs/mkdir", r#"{"path":"~/Música/nueva"}"#, 0),
        ("POST", "/fs/mkdir", r#"{"path":""}"#, 0),
        ("POST", "/fs/mkdir", r#"{"path":"/tmp/fuera"}"#, 0),
        ("GET", "/fs/dirs?path=~/codebase/nue", "", 0),
        ("POST", "/open-path", r#"{"path":"~/codebase"}"#, 1),
        (
            "POST",
            "/open-path",
            r#"{"path":"~/codebase/archivo:12"}"#,
            1,
        ),
        ("POST", "/open-path", r#"{"path":"~/codebase/no-esta"}"#, 0),
        ("POST", "/open-path", r#"{"path":"relativa"}"#, 0),
        ("POST", "/open-path", r#"{"path":""}"#, 0),
        ("POST", "/open-path", r#"{"path":"/a\u0000b"}"#, 0),
        ("POST", "/open-url", r#"{"url":"ftp://x"}"#, 0),
        ("POST", "/open-url", r#"{}"#, 0),
        ("POST", "/open-url", r#"{"url":"https://example.com"}"#, 1),
        ("POST", "/notify-popup", r#"{"title":"  ","body":"x"}"#, 0),
        ("POST", "/notify-popup", r#"{"title":"t","body":5}"#, 0),
        ("POST", "/notify-popup", r#"{"title":"t"}"#, 0),
        ("POST", "/notify-popup", r#"{"title":"t","body":"b"}"#, 0),
        ("POST", "/test", r#"{"kind":"ruido"}"#, 0),
        ("POST", "/test", r#"{"kind":["done"]}"#, 0),
        ("POST", "/test", r#"{"kind":"done"}"#, 1),
        ("POST", "/test", r#"{"kind":"chime"}"#, 0),
        ("POST", "/test", r#"{"kind":"voice"}"#, 1),
        ("GET", "/webterm-token", "", 0),
    ] {
        same(&t, method, path, body).await;
        launched += launches;
        t.calls(launched, step).await;
        step += 1;
    }
    // Lo creado por `/fs/mkdir`, igual en los dos lados (con su modo).
    for rel in ["codebase/nueva", "codebase/x/y/z", "Música/nueva"] {
        assert!(t.a.root.join(rel).is_dir(), "{rel}");
        let expected = comandos_oracle::oracle_at(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            "settings-directory-modes",
            &json!({"source":support::frozen::SOURCE_COMMIT,"fixture":"settings-routes","directory":rel}),
            || Ok(mode(&t.b.root.join(rel)).into_bytes()),
        );
        assert_eq!(
            mode(&t.a.root.join(rel)),
            std::str::from_utf8(&expected).unwrap()
        );
    }
    // La voz con `piper` y su modelo: `/bin/sh -c` con el guion del Python.
    for home in [&t.a, &t.b] {
        let voices = home.root.join(".local/share/piper-voices");
        std::fs::create_dir_all(&voices).unwrap();
        std::fs::write(voices.join("es_MX-ald-medium.onnx"), "").unwrap();
    }
    same(&t, "POST", "/test", r#"{"kind":"voice"}"#).await;
    let cb = t.calls(launched + 2, step).await;
    let ca = calls(t.a);
    assert_eq!(ca, cb);
    assert!(
        ca.iter().any(|(name, _)| name == "piper"),
        "la voz pasó por piper: {ca:?}"
    );
    // Lo lanzado: `xdg-open` con la ruta sin `:12`, la URL y el reproductor
    // con el volumen escrito por `/conf-set`.
    let names: Vec<&str> = ca.iter().map(|(name, _)| name.as_str()).collect();
    assert!(
        names.contains(&"xdg-open") && names.contains(&"pw-play"),
        "{ca:?}"
    );
    assert!(
        ca.contains(&(
            "pw-play".to_owned(),
            vec!["--volume=0.55".to_owned(), "/bin/sh".to_owned()]
        )),
        "{ca:?}"
    );
    assert!(
        ca.contains(&(
            "xdg-open".to_owned(),
            vec!["<HOME>/codebase/archivo".to_owned()]
        )),
        "{ca:?}"
    );
    let conf = std::fs::read_to_string(t.a.hooks().join("cc-notify.conf")).unwrap();
    assert!(
        conf.contains("\nVOLUME=55\n") && conf.ends_with("CC_LANG=en\nSPEAK_DONE=0\n"),
        "{conf}"
    );
}

#[tokio::test]
async fn webterm_token_is_created_like_python() {
    let (a, b) = (
        TestHome::new("settings-token-a"),
        TestHome::new("settings-token-b"),
    );
    for h in [&a, &b] {
        seed_services(h);
    }
    let t = FrozenSettings::new(&a, &b, "settings-token").await;
    // El token del tablero, tal cual (con espacios a los lados quitados).
    for home in [&t.a, &t.b] {
        std::fs::write(home.hooks().join("dash-token"), "  token-de-prueba \n").unwrap();
    }
    let (_, body) = same(&t, "GET", "/webterm-token", "").await;
    assert_eq!(body, r#"{"token": "token-de-prueba"}"#);
    // Vacío: los dos generan uno de 43 caracteres URL-safe, 0600, y lo reusan.
    for home in [&t.a, &t.b] {
        let path = home.hooks().join("dash-token");
        std::fs::write(&path, "\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    let token = |text: String| -> String {
        let value: Value = serde_json::from_str(&text).unwrap();
        value["token"].as_str().unwrap().to_owned()
    };
    let source = if let Some(port) = t.py.source_port() {
        let first = get(port, "/webterm-token").await;
        assert_eq!(first.status, 200);
        let tok = token(first.text());
        let path = t.b.hooks().join("dash-token");
        let again = get(port, "/webterm-token").await;
        Some(
            json!({"length":tok.len(), "urlsafe":tok.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'),
            "file_matches":std::fs::read_to_string(&path).unwrap()==tok,
            "mode":mode(&path),"reused":token(again.text())==tok}),
        )
    } else {
        None
    };
    let expected = comandos_oracle::oracle_at(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        "settings-token-properties",
        &json!({"source":support::frozen::SOURCE_COMMIT,"python":"3.10.12","before_token":"\n","before_mode":"640","target":"/webterm-token"}),
        || {
            serde_json::to_vec(&source.ok_or("source token properties required in record/check")?)
                .map_err(|e| e.to_string())
        },
    );
    let expected: Value = serde_json::from_slice(&expected).unwrap();
    let made = get(t.front.port, "/webterm-token").await;
    assert_eq!(made.status, 200);
    let ta = token(made.text());
    assert_eq!(ta.len(), 43);
    assert!(
        ta.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );
    let path = t.a.hooks().join("dash-token");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), ta);
    assert_eq!(mode(&path), "640");
    let again = get(t.front.port, "/webterm-token").await;
    let reused = token(again.text()) == ta;
    assert!(reused);
    assert_eq!(
        expected,
        json!({"length":ta.len(),"urlsafe":true,"file_matches":true,"mode":mode(&path),"reused":reused})
    );
    // Ausente: se crea 0600.
    let dir = TestHome::new("settings-token-new");
    std::fs::remove_file(dir.hooks().join("dash-token")).unwrap();
    let made = settings::access_token(&dir.hooks()).unwrap();
    assert_eq!(made.len(), 43);
    assert_eq!(mode(&dir.hooks().join("dash-token")), "600");
    assert_eq!(settings::access_token(&dir.hooks()).unwrap(), made);
}

#[tokio::test]
async fn conf_set_waits_for_lock() {
    let home = TestHome::new("conf-lock");
    seed_services(&home);
    let conf = home.hooks().join("cc-notify.conf");
    let lock = FileLock::acquire(&conf).unwrap();
    let legacy = FakeLegacy::start().await;
    let fr = front(&home, legacy.port, home.options()).await;
    let pending = tokio::spawn({
        let port = fr.port;
        async move {
            request_body(
                port,
                "POST",
                "/conf-set",
                "",
                r#"{"key":"VOLUME","value":"40"}"#,
            )
            .await
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!pending.is_finished(), "espera al candado de cc-app");
    assert_eq!(
        get(fr.port, "/snippets").await.status,
        200,
        "runtime libre mientras espera"
    );
    std::fs::write(&conf, "OTRA=1\n").unwrap();
    drop(lock);
    let done = pending.await.unwrap();
    assert_eq!(done.status, 200, "{}", done.text());
    assert_eq!(
        std::fs::read_to_string(&conf).unwrap(),
        "OTRA=1\nVOLUME=40\n"
    );
    assert!(legacy.requests().is_empty(), "nada se reenvió");
    fr.stop().await;
}

/// El popup usa exclusivamente un notifyd de prueba en puerto efímero.
#[tokio::test]
async fn notify_popup_enabled_matches_python_without_fallback() {
    use comandos_server::dash::native::usage::pane_models::HyperNotify;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for (status, stall) in [(204, false), (503, false), (200, true)] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let received = Arc::clone(&bodies);
        // Original HTTP capture is blocking; the genuine notifyd actor owns
        // a separate reactor so it can serve while that capture waits.
        let (server_stop, mut stopped) = tokio::sync::watch::channel(false);
        let server = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let mut jobs = tokio::task::JoinSet::new();
                loop {
                    let (mut socket, _) = tokio::select! {
                        result = listener.accept() => result.unwrap(),
                        _ = stopped.changed() => return,
                    };
                    let received = Arc::clone(&received);
                    jobs.spawn(async move {
                    let mut bytes = Vec::new();
                    let mut chunk = [0; 1024];
                    loop {
                        let n = socket.read(&mut chunk).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&chunk[..n]);
                        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&bytes[..end]);
                            let length: usize = headers
                                .lines()
                                .find_map(|h| {
                                    h.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|s| s.trim().parse().unwrap())
                                })
                                .unwrap();
                            if bytes.len() >= end + 4 + length {
                                received.lock().unwrap().push(
                                    String::from_utf8(bytes[end + 4..end + 4 + length].to_vec())
                                        .unwrap(),
                                );
                                break;
                            }
                        }
                    }
                    if stall {
                        tokio::time::sleep(Duration::from_secs(10)).await;
                    }
                    let reply = format!(
                        "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
                }
            });
        });
        let (a, b) = (
            TestHome::new("popup-native-a"),
            TestHome::new("popup-native-b"),
        );
        for home in [&a, &b] {
            home.write("cc-notify.conf", "DESKTOP_NOTIFY=1\n");
        }
        let mut opts = support::http_golden::confined_front_options(&a, &[]);
        opts.notifyd = Arc::new(HyperNotify { addr });
        let native = front(&a, support::dead_port(), opts).await;
        let prelude = format!(
            r#"
_fixture_popup_status = {status}
_fixture_popup_stall = '{stall}'
_original_popup_urlopen = dash.urllib.request.urlopen
def _popup_urlopen(req, **kwargs):
    assert req.full_url == 'http://127.0.0.1:4778/notify'
    req = dash.urllib.request.Request('http://127.0.0.1:{}/notify', data=req.data, headers=dict(req.header_items()))
    return _original_popup_urlopen(req, **kwargs)
dash.urllib.request.urlopen = _popup_urlopen
"#,
            addr.port()
        );
        let py = support::http_golden::FrozenHttp::new_rooted_with_ports(
            &b,
            "settings-popup-enabled",
            &[".claude/hooks/cc-notify.conf"],
            &prelude,
            &[addr.port()],
        )
        .await;
        let body =
            json!({"title":"Título ñ", "body":"b".repeat(430), "project":false, "kind":"other"})
                .to_string();
        let oracle = py.request("POST", "/notify-popup", "", &body).await;
        let source_payload = if py.source_port().is_some() {
            Some(bodies.lock().unwrap()[0].clone())
        } else {
            None
        };
        let expected_payload = comandos_oracle::oracle_at(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            "settings-popup-payload",
            &json!({"source":support::frozen::SOURCE_COMMIT,"python":"3.10.12","status":status,"stall":stall,"body":body}),
            || {
                source_payload
                    .ok_or_else(|| "source notifyd payload required in record/check".to_owned())
                    .map(String::into_bytes)
            },
        );
        let actual = request_body(native.port, "POST", "/notify-popup", "", &body).await;
        let pair = support::twin::TwinRun {
            front: actual,
            oracle,
        };
        pair.assert_same();
        assert_eq!(pair.front.status, 200);
        assert_eq!(
            serde_json::from_str::<Value>(&pair.front.text()).unwrap(),
            json!({"ok":true,"popup":status == 204 && !stall})
        );
        let sent = bodies.lock().unwrap().clone();
        assert_eq!(sent.len(), if py.source_port().is_some() { 2 } else { 1 });
        assert_eq!(
            sent.last().unwrap().as_bytes(),
            expected_payload,
            "actual native notifyd payload byte parity"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&sent[0]).unwrap()["project"],
            "ComandOS"
        );
        for h in [&a, &b] {
            h.write("cc-notify.conf", "DESKTOP_NOTIFY=0\n");
        }
        let disabled = support::twin::TwinRun {
            oracle: py.request("POST", "/notify-popup", "", &body).await,
            front: request_body(native.port, "POST", "/notify-popup", "", &body).await,
        };
        disabled.assert_same();
        assert!(disabled.front.text().contains("false"));
        assert_eq!(
            bodies.lock().unwrap().len(),
            sent.len(),
            "disabled: no network"
        );
        native.stop().await;
        let _ = server_stop.send(true);
        server.join().unwrap();
    }
}

/// El cuerpo para cc-notifyd, byte a byte el de `json.dumps` del Python.
#[test]
fn popup_payload_matches_json_dumps() {
    let cases = [
        ("Título \"x\"", "cuerpo\nñ 🙂", "ComandOS", "waiting"),
        (
            &"t".repeat(300) as &str,
            &"b".repeat(500),
            &"p".repeat(90),
            "otro",
        ),
    ];
    for (title, body, project, kind) in cases {
        let rust = settings::popup_payload(title, body, project, kind).unwrap();
        let script = "import json,sys\n\
            title, body, project, kind = sys.argv[1:5]\n\
            sys.stdout.write(json.dumps({\"title\": str(title)[:200], \"body\": str(body)[:400], \
            \"session\": \"\", \"kind\": \"waiting\" if kind == \"waiting\" else \"done\", \
            \"project\": str(project)[:80], \"options\": \"\", \"full\": str(body)[:400]}))";
        let home = TestHome::new("popup-json-dumps");
        // run_python_original prepends its source root; this pure script uses
        // only the four supplied values, so discard that repository argument.
        let script = script.replace("sys.argv[1:5]", "sys.argv[2:6]");
        let expected = comandos_oracle::oracle_at(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            "settings-popup-json",
            &json!({"python":"3.10.12", "script":script,
                "title":title, "body":body, "project":project, "kind":kind}),
            || {
                support::frozen::run_python_original(
                    &script,
                    &[
                        title.as_ref(),
                        body.as_ref(),
                        project.as_ref(),
                        kind.as_ref(),
                    ],
                    &home.root,
                )
                .map(String::into_bytes)
            },
        );
        assert_eq!(rust, String::from_utf8(expected).unwrap());
    }
    // Sanidad del orden de claves.
    let value: Value =
        serde_json::from_str(&settings::popup_payload("a", "b", "c", "waiting").unwrap()).unwrap();
    assert_eq!(
        value,
        json!({"title": "a", "body": "b", "session": "", "kind": "waiting",
                              "project": "c", "options": "", "full": "b"})
    );
}

/// Petición «remota» (por el proxy de tailscale: `X-Forwarded-For` no local y
/// `Host` de la tailnet): la puerta exige el token.
async fn remote_get(port: u16, target: &str, token: &str) -> support::Wire {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let wire = format!(
        "GET {target} HTTP/1.1\r\nHost: maquina.tail1.ts.net\r\nX-Forwarded-For: 100.64.0.9\r\n\
         X-Comandos-Token: {token}\r\nConnection: close\r\n\r\n"
    );
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream.write_all(wire.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    tokio::time::timeout(support::WAIT, stream.read_to_end(&mut out))
        .await
        .unwrap()
        .unwrap();
    let text = String::from_utf8_lossy(&out).into_owned();
    let status = text.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap();
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.as_bytes().to_vec())
        .unwrap_or_default();
    support::Wire {
        status,
        headers: Vec::new(),
        body,
    }
}

/// C (revisión de la Tarea 1): el frente relee `dash-token` como el Python. Se
/// rota el archivo con los dos sirviendo el MISMO HOME y sin reiniciar nada:
/// el token viejo deja de valer, el nuevo vale y `/webterm-token` entrega el
/// que la puerta acepta, en los dos lados.
#[tokio::test]
async fn token_rotation_takes_effect_without_restart() {
    use comandos_server::dash::{serve_with, token_path};
    let home = TestHome::new_short("token-rot");
    seed_services(&home);
    let py =
        support::http_golden::FrozenHttp::new(&home, "settings-token-rotation-source", &[]).await;
    let legacy = FakeLegacy::start().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut cfg = support::config(&home, legacy.port);
    cfg.token_file = Some(token_path(&home.root));
    let (stop, shutdown) = tokio::sync::watch::channel(false);
    let opts = home.options();
    support::assert_private_tmux(&opts);
    let task = tokio::spawn(serve_with(listener, cfg, Some(opts), shutdown));
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let both = async |target: &'static str, token: String| {
        let current = std::fs::read_to_string(token_path(&home.root)).unwrap();
        let source = if let Some(source_port) = py.source_port() {
            Some(remote_get(source_port, target, &token).await)
        } else {
            None
        };
        let expected = comandos_oracle::oracle_at(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            "settings-token-rotation",
            &json!({"source":support::frozen::SOURCE_COMMIT,"python":"3.10.12","target":target,"authorization":token,"current_token":current}),
            || {
                let wire = source.ok_or("source token rotation required in record/check")?;
                serde_json::to_vec(&(wire.status, wire.text())).map_err(|e| e.to_string())
            },
        );
        let (status, text): (u16, String) = serde_json::from_slice(&expected).unwrap();
        (
            remote_get(port, target, &token).await,
            support::Wire {
                status,
                headers: vec![],
                body: text.into_bytes(),
            },
        )
    };
    let old = support::TOKEN.to_owned();
    let (a, b) = both("/conf", old.clone()).await;
    assert_eq!((a.status, b.status), (200, 200));
    assert_eq!(a.text(), b.text());
    // Rotación atómica (otro inodo), con espacios que `str.strip()` quita.
    let file = token_path(&home.root);
    let tmp = home.hooks().join("dash-token.tmp");
    std::fs::write(&tmp, "\u{1f}token-nuevo-rotado\n").unwrap();
    std::fs::rename(&tmp, &file).unwrap();
    let (a, b) = both("/conf", old.clone()).await;
    assert_eq!((a.status, b.status), (401, 401), "el viejo ya no vale");
    assert_eq!(a.text(), b.text());
    let new = "token-nuevo-rotado".to_owned();
    let (a, b) = both("/conf", new.clone()).await;
    assert_eq!((a.status, b.status), (200, 200), "el nuevo vale");
    let (a, b) = both("/webterm-token", new.clone()).await;
    assert_eq!((a.status, b.status), (200, 200));
    assert_eq!(a.text(), r#"{"token": "token-nuevo-rotado"}"#);
    assert_eq!(a.text(), b.text());
    // Reescritura en el sitio (mismo inodo, otro tamaño).
    std::fs::write(&file, "otro-token-mas-largo").unwrap();
    let (a, b) = both("/conf", new).await;
    assert_eq!((a.status, b.status), (401, 401));
    let (a, b) = both("/conf", "otro-token-mas-largo".to_owned()).await;
    assert_eq!((a.status, b.status), (200, 200));
    let _ = stop.send(true);
    let _ = task.await;
}
