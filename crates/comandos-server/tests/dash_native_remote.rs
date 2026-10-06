//! Acceso remoto y terminal web nativos (plan 2f-3, Tarea 2) contra el
//! `cc-dash` Python, con el gemelo.
//!
//! Confinamiento (lo comprueban los canarios de cada prueba):
//! - `tailscale` y `cc-webterm` son los falsos de `support::services` en el
//!   `fakebin` de LOS DOS lados (PATH = solo el `fakebin`): anotan cada llamada
//!   en `<HOME>/tailscale.log` y `<HOME>/fakebin.log` y responden con el guion
//!   de `<HOME>/tailscale/`. Ningún `tailscale serve` real, ningún
//!   `cc-webterm`/ttyd real, ningún `pkill` real (el del `fakebin` solo anota).
//! - `qrencode` es el real si está instalado (puro y local: escribe un PNG en
//!   el temporal); si no, los dos lados responden 404.
//! - La salud de la terminal web: el frente sondea `opts.webterm_health_ports`
//!   (puerto 1 o servidores de la prueba) y el oráculo, con la red cerrada,
//!   solo los puertos de la prueba (R5). Nunca 4779/4780 reales.
//! - `remote::start` (restaurar la terminal web) solo corre en
//!   `restore_runs_only_with_front_background`, sobre un `Native` propio con
//!   el `fakebin` de su HOME.
mod support;

use comandos_server::dash::native::{Background, Native, procs::which_in, remote};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use support::{
    TestHome,
    oracle::{OracleOpts, confined_fakebin, run_dash_with},
    services::{calls_of, remote_fakes, tailscale_log, tailscale_set, unhome},
    twin::{Twin, TwinOpts},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const STATUS_JSON: &str = r#"{"Self":{"DNSName":"maquina.tail1.ts.net."},"Peer":{}}"#;

/// Lo mismo en los dos lados del gemelo.
fn both(t: &Twin, f: impl Fn(&TestHome)) {
    f(&t.a);
    f(&t.b);
}

/// Canario: todo programa que el frente resuelve está en el `fakebin` de A
/// (que solo contiene falsos y herramientas inocuas), nunca en `/usr/bin`.
fn assert_confined(t: &Twin) {
    let search = t.front_options.search_path.clone().unwrap();
    let fakebin = t.a.root.join("fakebin");
    for name in ["tailscale", "cc-webterm", "pkill"] {
        let found = which_in(Some(&search), name).unwrap();
        assert_eq!(found, fakebin.join(name), "{name} debe ser el falso");
    }
    assert!(!search.to_string_lossy().contains("/usr/bin"));
    for home in [&t.a, &t.b] {
        assert!(!home.root.join(".local/bin/cc-webterm").exists());
        let text = std::fs::read_to_string(home.root.join("fakebin/tailscale")).unwrap();
        assert!(
            text.contains("tailscale.log"),
            "tailscale de {} no es el falso",
            home.root.display()
        );
    }
}

/// La misma petición a los dos lados: estado y cuerpo iguales con las rutas
/// de cada HOME normalizadas; devuelve el cuerpo del frente.
async fn same(t: &Twin, method: &str, path: &str) -> (u16, String) {
    let body = if method == "GET" { "" } else { "{}" };
    let run = t.request(method, path, body).await;
    let (fa, fb) = (
        unhome(&t.a, &run.front.text()),
        unhome(&t.b, &run.oracle.text()),
    );
    assert_eq!(
        run.front.status, run.oracle.status,
        "{method} {path}: {fa} / {fb}"
    );
    assert_eq!(fa, fb, "{method} {path}");
    (run.front.status, fa)
}

fn logs_equal(t: &Twin) {
    assert_eq!(tailscale_log(&t.a), tailscale_log(&t.b), "tailscale.log");
    // Retired script/process calls are intentionally absent from the native backend.
    assert!(calls_of(&t.a, "cc-webterm").is_empty());
    assert!(calls_of(&t.a, "pkill").is_empty());
}

/// `qrencode` falso: anota sus argumentos, guarda su stdin en
/// `<HOME>/qrencode.stdin` y escribe un «PNG» fijo en el `-o`.
const QRENCODE: &str = r#"#!/bin/sh
printf '%s\0' qrencode "$@" "$(printf '\036')" >> "$HOME/fakebin.log"
out=""; prev=""
for a in "$@"; do [ "$prev" = "-o" ] && out=$a; prev=$a; done
cat > "$HOME/qrencode.stdin"
printf 'PNG-FALSO' > "$out"
"#;

/// La URL del QR lleva el token: el frente la pasa por stdin (nunca en `argv`,
/// visible en `/proc`); el Python, como argumento. Desviación intencionada: el
/// PNG es el mismo (con el `qrencode` real lo compara `remote_routes_match_python`).
#[tokio::test]
async fn qr_url_goes_by_stdin_not_argv() {
    let mut fakes = remote_fakes();
    fakes.push(("qrencode".into(), QRENCODE.into()));
    let opts = TwinOpts {
        fakebin_extra: fakes,
        ..TwinOpts::default()
    };
    let Some(t) = Twin::start_with("remote-qr", |_| {}, opts).await else {
        return;
    };
    assert_confined(&t);
    both(&t, |h| tailscale_set(h, "status.json", Some(STATUS_JSON)));
    let (code, body) = same(&t, "GET", "/remote-state").await;
    assert_eq!(code, 200);
    let state: Value = serde_json::from_str(&body).unwrap();
    let url = state["urls"]["dashboard"].as_str().unwrap().to_owned();
    assert!(url.contains(support::TOKEN), "{url}");
    let run = t.get("/remote-qr.png").await;
    assert_eq!((run.front.status, run.oracle.status), (200, 200));
    assert_eq!(run.front.body, run.oracle.body);
    assert_eq!(&run.front.body[..], b"PNG-FALSO");
    let front = calls_of(&t.a, "qrencode");
    let oracle = calls_of(&t.b, "qrencode");
    assert_eq!(front.len(), 1);
    assert_eq!(oracle.len(), 1);
    let front = &front[0];
    assert_eq!(front.len(), 6, "{front:?}");
    assert!(
        front.iter().all(|a| !a.contains(support::TOKEN)),
        "{front:?}"
    );
    assert_eq!(
        std::fs::read_to_string(t.a.root.join("qrencode.stdin")).unwrap(),
        url
    );
    // El Python: la misma forma más la URL al final, y stdin vacío.
    assert_eq!(oracle[0].len(), 7);
    assert_eq!(oracle[0][6], url);
    assert_eq!(
        std::fs::read_to_string(t.b.root.join("qrencode.stdin")).unwrap(),
        ""
    );
    assert_eq!(front[..1], oracle[0][..1]);
    assert_eq!(front[2..], oracle[0][2..6]);
}

#[tokio::test]
async fn remote_routes_match_python() {
    let opts = TwinOpts {
        fakebin_extra: remote_fakes(),
        ..TwinOpts::default()
    };
    let Some(t) = Twin::start_with("remote", |_| {}, opts).await else {
        return;
    };
    assert_confined(&t);
    both(&t, |h| {
        tailscale_set(h, "status.json", Some(STATUS_JSON));
        tailscale_set(
            h,
            "serve.txt",
            Some("https://maquina.tail1.ts.net (tailnet only)\n"),
        );
    });

    // Estado: la primera petición calcula; la segunda sale de la caché (no
    // llama a tailscale aunque el guion cambie).
    let (code, body) = same(&t, "GET", "/remote-state").await;
    assert_eq!(code, 200);
    let state: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(state["host"], "maquina.tail1.ts.net");
    assert_eq!(
        state["urls"]["terminal"],
        format!("https://maquina.tail1.ts.net/term/?auth={}", support::TOKEN)
    );
    assert_eq!(state["terminalState"], "off");
    let calls = tailscale_log(&t.a).len();
    both(&t, |h| {
        tailscale_set(h, "serve.txt", Some("|-- proxy http://127.0.0.1:4777\n"))
    });
    let (_, again) = same(&t, "GET", "/remote-state?x=1").await;
    assert_eq!(again, body, "la segunda foto sale de la caché");
    assert_eq!(
        tailscale_log(&t.a).len(),
        calls,
        "sin tailscale con la caché fresca"
    );
    logs_equal(&t);

    // QR: byte a byte (o el 404 sin qrencode en los dos lados).
    let run = t.get("/remote-qr.png").await;
    assert_eq!(run.front.status, run.oracle.status);
    assert_eq!(run.front.body, run.oracle.body, "PNG");
    for name in ["content-type", "content-length", "cache-control"] {
        assert_eq!(
            run.front.header(name),
            run.oracle.header(name),
            "cabecera {name}"
        );
    }
    // Con el `qrencode` real instalado el PNG sí se compara (el frente le
    // pasa la URL por stdin; el Python, por argumento).
    if support::oracle::real_program("qrencode").is_some() {
        assert_eq!(run.front.status, 200, "{}", run.front.text());
    }
    if run.front.status == 200 {
        assert_eq!(run.front.header("content-type"), Some("image/png"));
        assert!(run.front.body.starts_with(b"\x89PNG"));
    }

    // Encender sin sesión → 400; con sesión → 200 con el estado nuevo.
    let (code, body) = same(&t, "POST", "/remote-on").await;
    assert_eq!(
        (code, body.as_str()),
        (400, r#"{"error": "Tailscale no ha iniciado sesion"}"#)
    );
    both(&t, |h| tailscale_set(h, "logged-in", Some("")));
    let (code, body) = same(&t, "POST", "/remote-on").await;
    assert_eq!(code, 200, "{body}");
    assert!(body.contains(r#""remoteOn": true"#), "{body}");
    logs_equal(&t);

    // `serve` que falla los tres intentos: su stderr es el error.
    both(&t, |h| {
        tailscale_set(h, "serve-fail", Some("  permiso denegado por la tailnet\n"))
    });
    let (code, body) = same(&t, "POST", "/remote-on").await;
    assert_eq!(
        (code, body.as_str()),
        (400, r#"{"error": "permiso denegado por la tailnet"}"#)
    );
    // stderr de solo espacios: `"".strip()` es falso y el Python sigue.
    both(&t, |h| tailscale_set(h, "serve-fail", Some(" \n")));
    let (code, _) = same(&t, "POST", "/remote-on").await;
    assert_eq!(code, 200);
    both(&t, |h| tailscale_set(h, "serve-fail", None));

    // Native lifecycle replaces the retired script. Same route body and tailscale calls.
    t.a.write(
        "webterm-mode.json",
        r#"{"mode":"native","ports":[4779,4780]}"#,
    );
    let (code, _) = same(&t, "POST", "/remote-webterm-on").await;
    assert_eq!(code, 200);
    let (code, _) = same(&t, "POST", "/remote-webterm-off").await;
    assert_eq!(code, 200);
    logs_equal(&t);
    // Absence of the retired dependency is no longer an activation error.
    both(&t, |h| {
        std::fs::remove_file(h.root.join("fakebin/cc-webterm")).unwrap();
    });
    let run = t.request("POST", "/remote-webterm-on", "{}").await;
    assert_eq!(run.front.status, 200);
    assert_eq!(run.oracle.status, 400);
    assert!(calls_of(&t.a, "cc-webterm").is_empty());
    // Next assertions compare only the subsequent host/status phase.
    both(&t, |h| {
        std::fs::write(h.root.join("tailscale.log"), b"").unwrap();
        tailscale_set(h, "serve.txt", Some("|-- proxy http://127.0.0.1:4777\n"));
    });

    // Sin `status --json`: el host sale de `status --self --json` con la
    // expresión del Python; sin nada, host vacío → QR 400.
    both(&t, |h| {
        tailscale_set(h, "status.json", None);
        tailscale_set(
            h,
            "self.txt",
            Some("100.1.2.3 otra-maquina.tail9.ts.net. linux -\n"),
        );
    });
    let (code, body) = same(&t, "POST", "/remote-webterm-off").await;
    assert_eq!(code, 200);
    assert!(
        body.contains(r#""host": "otra-maquina.tail9.ts.net""#),
        "{body}"
    );
    both(&t, |h| tailscale_set(h, "self.txt", None));
    same(&t, "POST", "/remote-webterm-off").await;
    let (code, body) = same(&t, "GET", "/remote-qr.png").await;
    assert_eq!(
        (code, body.as_str()),
        (400, r#"{"error": "No pude leer el host de Tailscale"}"#)
    );
    logs_equal(&t);
}

#[tokio::test]
async fn remote_off_never_resets() {
    let opts = TwinOpts {
        fakebin_extra: remote_fakes(),
        ..TwinOpts::default()
    };
    let Some(t) = Twin::start_with("remote-off", |_| {}, opts).await else {
        return;
    };
    assert_confined(&t);
    both(&t, |h| {
        tailscale_set(h, "status.json", Some(STATUS_JSON));
        tailscale_set(h, "logged-in", Some(""));
        tailscale_set(h, "serve.txt", Some("|-- proxy http://127.0.0.1:4777\n"));
    });
    // Mismo cuerpo (la foto posterior); el registro difiere a propósito (D9).
    let (code, body) = same(&t, "POST", "/remote-off").await;
    assert_eq!(code, 200);
    assert!(body.contains(r#""remoteOn": false"#), "{body}");
    let front = tailscale_log(&t.a);
    let off = |port: &str| {
        vec![
            "serve".to_owned(),
            format!("--https={port}"),
            "off".to_owned(),
        ]
    };
    assert!(front.contains(&off("443")), "{front:?}");
    assert!(front.contains(&off("8443")), "{front:?}");
    assert!(
        !front.iter().flatten().any(|a| a == "reset"),
        "el frente nunca ejecuta `tailscale serve reset`: {front:?}"
    );
    let oracle = tailscale_log(&t.b);
    assert!(oracle.contains(&vec!["serve".to_owned(), "reset".to_owned()]));
    // Orden del frente: 443 off, 8443 off, después la terminal web.
    let pos = |v: &Vec<String>| front.iter().position(|c| c == v).unwrap();
    assert!(pos(&off("443")) < pos(&off("8443")));
    assert!(calls_of(&t.a, "cc-webterm").is_empty());
    assert_eq!(calls_of(&t.b, "cc-webterm"), vec![vec!["off".to_owned()]]);
}

/// Servidor HTTP de la prueba: `200` en `/term/token` y `404` en lo demás.
async fn health_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let status = if head.starts_with("GET /term/token ") {
                    "200 OK"
                } else {
                    "404 Not Found"
                };
                let _ = stream
                    .write_all(
                        format!("HTTP/1.1 {status}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}")
                            .as_bytes(),
                    )
                    .await;
            });
        }
    });
    port
}

#[tokio::test]
async fn remote_on_with_healthy_webterm_matches_python() {
    let port = health_server().await;
    // R5: el oráculo sondea el servidor de la prueba (primario sano, respaldo
    // con 404) en vez de 4779/4780; el frente, por sus opciones.
    let prelude = format!(
        "def _twin_health(probe=None):\n\
         \x20   check = probe or dash.http_healthy\n\
         \x20   return {{\"primaryHealthy\": bool(check(\"http://127.0.0.1:{port}/term/token\", timeout=0.4)),\n\
         \x20           \"fallbackHealthy\": bool(check(\"http://127.0.0.1:{port}/token\", timeout=0.4))}}\n\
         dash.webterm_health = _twin_health\n"
    );
    let opts = TwinOpts {
        fakebin_extra: remote_fakes(),
        python_prelude: prelude,
        allow_ports: vec![port],
        front: Some(Box::new(move |o| o.webterm_health_ports = [port, port])),
        ..TwinOpts::default()
    };
    let Some(t) = Twin::start_with("remote-ok", |_| {}, opts).await else {
        return;
    };
    assert_confined(&t);
    both(&t, |h| {
        tailscale_set(h, "status.json", Some(STATUS_JSON));
        tailscale_set(h, "logged-in", Some(""));
    });
    let (code, body) = same(&t, "POST", "/remote-on").await;
    assert_eq!(code, 200, "{body}");
    // Primario sano y rutas puestas → activo.
    assert!(body.contains(r#""terminalState": "active""#), "{body}");
    assert!(
        body.contains(r#""primaryHealthy": true, "fallbackHealthy": false"#),
        "{body}"
    );
    let log = tailscale_log(&t.a);
    assert!(
        log.iter()
            .any(|c| c.last().map(String::as_str) == Some("http://127.0.0.1:4780/term"))
    );
    assert!(
        log.iter()
            .any(|c| c.last().map(String::as_str) == Some("http://127.0.0.1:4779"))
    );
    logs_equal(&t);
    let (code, _) = same(&t, "GET", "/remote-state").await;
    assert_eq!(code, 200);
}

/// Espera hasta que `f` sea cierto (o vence el plazo).
async fn until(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    f()
}

fn native_for(home: &TestHome, background: Background) -> Arc<Native> {
    let fakebin = confined_fakebin(home, &remote_fakes());
    let mut opts = home.options();
    opts.search_path = Some(fakebin.into_os_string());
    opts.background = background;
    Arc::new(Native::new(opts))
}

#[tokio::test]
async fn restore_runs_only_with_front_background() {
    let home = TestHome::new_short("remote-restore");
    let legacy = native_for(&home, Background::legacy());
    home.write("webterm-enabled", "");
    // `legacy` (el valor por omisión de la 2f): nada, aunque exista la marca.
    assert!(!remote::start(&legacy));
    let front = native_for(&home, Background::front());
    std::fs::remove_file(home.hooks().join("webterm-enabled")).unwrap();
    assert!(!remote::start(&front), "sin marca no se restaura");
    assert!(calls_of(&home, "cc-webterm").is_empty());
    assert!(tailscale_log(&home).is_empty());
    home.write("webterm-enabled", "");
    home.write(
        "webterm-mode.json",
        r#"{"mode":"native","ports":[4779,4780]}"#,
    );
    assert!(remote::start(&front));
    assert!(until(|| tailscale_log(&home).len() >= 2).await);
    assert!(until(|| front.tasks().is_empty()).await);
    assert!(calls_of(&home, "cc-webterm").is_empty());

    // El Python hace lo mismo con su `restore_requested_webterm()`.
    let python = TestHome::new_short("remote-restore-py");
    let code = "import threading\n\
                print(dash.restore_requested_webterm())\n\
                for th in threading.enumerate():\n\
                \x20   if th is not threading.main_thread():\n\
                \x20       th.join(30)\n";
    python.write("webterm-enabled", "");
    let opts = OracleOpts {
        fakebin_extra: remote_fakes(),
        ..OracleOpts::default()
    };
    let Some(out) = run_dash_with(&python, code, &opts) else {
        return;
    };
    assert_eq!(out.trim(), "True");
    assert_eq!(tailscale_log(&python), tailscale_log(&home));
    assert_eq!(calls_of(&python, "cc-webterm"), vec![Vec::<String>::new()]);
    assert!(calls_of(&home, "cc-webterm").is_empty());
}
