//! Ajustes del tablero nativos (plan 2f-3, Tarea 1) contra el `cc-dash`
//! Python, con el gemelo (sin tmux en estas rutas).
//!
//! Confinamiento: cada lado tiene su HOME temporal; `cc-notify.conf`,
//! `dash-token` y las carpetas de `/fs/*` viven solo ahí. `xdg-open`,
//! `pw-play`, `spd-say`, `piper`… son los falsos del `fakebin` que solo anotan
//! (`<HOME>/fakebin.log`); `/bin/sh` real solo corre el guion de la voz, que
//! llama a esos falsos. `/notify-popup` válido va con `DESKTOP_NOTIFY=0` (sin
//! red en ningún lado: con popups encendidos el frente declina, R4).
mod support;

use comandos_server::dash::native::{files::FileLock, settings};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, process::Command};
use support::{
    FakeLegacy, TestHome, front, get, request_body,
    services::{calls, mode, seed_services, settle_calls, unhome},
    twin::Twin,
};

/// La misma petición a los dos lados: estado y cuerpo iguales con las rutas
/// de cada HOME normalizadas; después `cc-notify.conf` (bytes y modo) y las
/// llamadas a los falsos.
async fn same(t: &Twin, method: &str, path: &str, body: &str) -> (u16, String) {
    let run = t.request(method, path, body).await;
    let (fa, fb) = (
        unhome(&t.a, &run.front.text()),
        unhome(&t.b, &run.oracle.text()),
    );
    assert_eq!(
        run.front.status, run.oracle.status,
        "{method} {path} {body}: {fa} / {fb}"
    );
    assert_eq!(fa, fb, "{method} {path} {body}");
    t.files_equal(&["cc-notify.conf"]).unwrap();
    let conf = |h: &TestHome| mode(&h.hooks().join("cc-notify.conf"));
    assert_eq!(conf(&t.a), conf(&t.b), "modo de cc-notify.conf");
    (run.front.status, fa)
}

#[tokio::test]
async fn settings_routes_match_python() {
    let Some(t) = Twin::start("settings", seed_services).await else {
        return;
    };
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
        settle_calls(&t.a, &t.b, launched).await;
        assert_eq!(
            calls(&t.a),
            calls(&t.b),
            "programas tras {method} {path} {body}"
        );
    }
    // Lo creado por `/fs/mkdir`, igual en los dos lados (con su modo).
    for rel in ["codebase/nueva", "codebase/x/y/z", "Música/nueva"] {
        assert!(t.a.root.join(rel).is_dir(), "{rel}");
        assert_eq!(mode(&t.a.root.join(rel)), mode(&t.b.root.join(rel)));
    }
    // La voz con `piper` y su modelo: `/bin/sh -c` con el guion del Python.
    for home in [&t.a, &t.b] {
        let voices = home.root.join(".local/share/piper-voices");
        std::fs::create_dir_all(&voices).unwrap();
        std::fs::write(voices.join("es_MX-ald-medium.onnx"), "").unwrap();
    }
    same(&t, "POST", "/test", r#"{"kind":"voice"}"#).await;
    settle_calls(&t.a, &t.b, launched + 2).await;
    let (ca, cb) = (calls(&t.a), calls(&t.b));
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
    let Some(t) = Twin::start("settings-token", seed_services).await else {
        return;
    };
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
    let run = t.get("/webterm-token").await;
    assert_eq!(run.front.status, 200);
    assert_eq!(run.oracle.status, 200);
    let token = |text: String| -> String {
        let value: Value = serde_json::from_str(&text).unwrap();
        value["token"].as_str().unwrap().to_owned()
    };
    let (ta, tb) = (token(run.front.text()), token(run.oracle.text()));
    assert_eq!(ta.len(), 43);
    assert_eq!(tb.len(), 43);
    assert!(
        ta.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );
    for (home, tok) in [(&t.a, &ta), (&t.b, &tb)] {
        let path = home.hooks().join("dash-token");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), *tok);
        // `O_CREAT` sobre un archivo existente no cambia su modo.
        assert_eq!(mode(&path), "640");
    }
    let again = t.get("/webterm-token").await;
    assert_eq!(token(again.front.text()), ta);
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

/// Con popups encendidos el frente declina ANTES de tocar la red (R4): el
/// heredado recibe la petición tal cual.
#[tokio::test]
async fn notify_popup_enabled_declines_before_network() {
    let home = TestHome::new("popup-on");
    std::fs::write(home.hooks().join("cc-notify.conf"), "DESKTOP_NOTIFY=1\n").unwrap();
    let legacy = FakeLegacy::start().await;
    let fr = front(&home, legacy.port, home.options()).await;
    let body = r#"{"title":"t","body":"b"}"#;
    request_body(fr.port, "POST", "/notify-popup", "", body).await;
    let seen = legacy.requests();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(seen.iter().all(|r| r.contains("/notify-popup")), "{seen:?}");
    fr.stop().await;
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
        let Ok(out) = Command::new("python3")
            .env_clear()
            .env("PYTHONIOENCODING", "utf-8")
            .args(["-c", script, title, body, project, kind])
            .output()
        else {
            eprintln!("python3 no está instalado: se salta");
            return;
        };
        assert_eq!(rust, String::from_utf8(out.stdout).unwrap());
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
