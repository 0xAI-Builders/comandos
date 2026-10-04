//! Dominio G1: paneles e historial de tmux por el frente, sobre un servidor
//! tmux privado compartido con el oráculo (mismas identidades de pane).
mod support;

use comandos_server::dash::native::tmux::{Program, Tmux};
use std::{
    ffi::OsString,
    time::{Duration, Instant},
};
use support::{
    FakeLegacy, TestHome, Wire, dead_port, front, get, oracle::oracle, request_body, tmux_available,
};

fn tmux(home: &TestHome, args: &[&str]) -> String {
    let out = std::process::Command::new("tmux")
        .args(["-f", "/dev/null"])
        .args(args)
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", home.tmux_dir())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "tmux {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn seen(wire: &Wire) -> (u16, Option<String>, String) {
    (
        wire.status,
        wire.header("content-type").map(str::to_owned),
        wire.text(),
    )
}

/// Sesión `s1` con dos paneles (%0 y %1) que muestran texto conocido.
fn two_panes(home: &TestHome) {
    tmux(
        home,
        &[
            "new-session",
            "-d",
            "-s",
            "s1",
            "-x",
            "120",
            "-y",
            "40",
            "sh -c 'printf \"uno\\ndos ñ\\n\"; exec cat'",
        ],
    );
    tmux(home, &["split-window", "-h", "-t", "=s1:", "cat"]);
}

/// `tmux` falso: `sh -c <script> sh <args de tmux>`.
fn fake_tmux(script: &str, timeout: Duration) -> Tmux {
    Tmux {
        program: Program {
            path: "sh".into(),
            prefix: vec![
                OsString::from("-c"),
                OsString::from(script),
                OsString::from("sh"),
            ],
            env: vec![],
            env_remove: vec![],
        },
        timeout,
    }
}

#[tokio::test]
async fn terminal_routes_match_python_oracle() {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new("term-oracle");
    two_panes(&home);
    let Some(py) = oracle(&home).await else {
        return;
    };
    let front = front(&home, dead_port(), home.options()).await;
    let list = r#"{"session": "s1"}"#;
    let listed = request_body(front.port, "POST", "/terminal-panes", "", list).await;
    assert_eq!(listed.status, 200, "{}", listed.text());
    let identity =
        serde_json::from_str::<serde_json::Value>(&listed.text()).unwrap()["panes"][0]["identity"]
            .as_str()
            .unwrap()
            .to_owned();
    let cases = [
        ("/terminal-panes", list.to_owned()),
        ("/terminal-panes", r#"{"session": "a b"}"#.to_owned()),
        ("/terminal-panes", r#"{"session": 5}"#.to_owned()),
        ("/terminal-panes", r#"{"session": "nadie"}"#.to_owned()),
        (
            "/terminal-panes",
            r#"{"session": "s1", "action": "zz"}"#.to_owned(),
        ),
        (
            "/terminal-panes",
            r#"{"session": "s1", "action": "resize", "axis": "x", "size": 1}"#.to_owned(),
        ),
        (
            "/terminal-panes",
            r#"{"session": "s1", "action": "resize", "pane": "%9", "axis": "y", "size": 10}"#
                .to_owned(),
        ),
        (
            "/terminal-panes",
            r#"{"session": "s1", "action": "split", "direction": "up"}"#.to_owned(),
        ),
        (
            "/terminal-panes",
            r#"{"session": "s1", "action": "select", "pane": "%0", "identity": "otra"}"#.to_owned(),
        ),
        (
            "/terminal-panes",
            format!(
                r#"{{"session": "s1", "action": "select", "pane": "%0", "identity": "{identity}", "scope": "client"}}"#
            ),
        ),
        (
            "/terminal-history",
            r#"{"session": "s1", "pane": "%0", "lines": 50}"#.to_owned(),
        ),
        ("/terminal-history", r#"{"session": "s1"}"#.to_owned()),
        (
            "/terminal-history",
            r#"{"session": "s1", "lines": 0}"#.to_owned(),
        ),
        (
            "/terminal-history",
            r#"{"session": "s1", "lines": true}"#.to_owned(),
        ),
        (
            "/terminal-history",
            r#"{"session": "s1", "pane": "%9"}"#.to_owned(),
        ),
        (
            "/terminal-history",
            r#"{"session": "s1", "col": 0, "row": 0}"#.to_owned(),
        ),
        (
            "/terminal-history",
            r#"{"session": "s1", "col": 999, "row": 0}"#.to_owned(),
        ),
        (
            "/terminal-history",
            r#"{"session": "s1", "col": -1, "row": 0}"#.to_owned(),
        ),
        ("/terminal-history", r#"{"session": "nadie"}"#.to_owned()),
        ("/terminal-history", r#"{"session": 5}"#.to_owned()),
    ];
    for (path, body) in &cases {
        assert_eq!(
            seen(&request_body(py.port, "POST", path, "", body).await),
            seen(&request_body(front.port, "POST", path, "", body).await),
            "{path} {body}"
        );
    }
    front.stop().await;
}

#[tokio::test]
async fn terminal_panes_mutations_are_native() {
    if !tmux_available() {
        return;
    }
    let home = TestHome::new("term-mut");
    two_panes(&home);
    let front = front(&home, dead_port(), home.options()).await;
    let listed = request_body(
        front.port,
        "POST",
        "/terminal-panes",
        "",
        r#"{"session": "s1"}"#,
    )
    .await;
    let value: serde_json::Value = serde_json::from_str(&listed.text()).unwrap();
    let identity = value["panes"][0]["identity"].as_str().unwrap().to_owned();
    let split = format!(
        r#"{{"session": "s1", "action": "split", "pane": "%0", "identity": "{identity}", "direction": "down"}}"#
    );
    let wire = request_body(front.port, "POST", "/terminal-panes", "", &split).await;
    assert_eq!(wire.status, 200, "{}", wire.text());
    assert!(
        wire.text().ends_with(r#", "opened": "%2"}"#),
        "{}",
        wire.text()
    );
    let resize = r#"{"session": "s1", "action": "resize", "pane": "%1", "axis": "x", "size": 30}"#;
    assert_eq!(
        request_body(front.port, "POST", "/terminal-panes", "", resize)
            .await
            .status,
        200
    );
    assert_eq!(
        tmux(
            &home,
            &["display-message", "-p", "-t", "%1", "#{pane_width}"]
        )
        .trim(),
        "30"
    );
    front.stop().await;
}

#[tokio::test]
async fn terminal_panes_close_declines() {
    let home = TestHome::new("term-close");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let body = r#"{"session": "s1", "action": "close", "pane": "%0", "identity": "x"}"#;
    assert_eq!(
        request_body(front.port, "POST", "/terminal-panes", "", body)
            .await
            .text(),
        r#"{"legacy": true}"#
    );
    assert_eq!(
        legacy.requests(),
        vec!["POST /terminal-panes HTTP/1.1".to_owned()]
    );
    front.stop().await;
}

#[tokio::test]
async fn terminal_undecodable_tmux_output_declines() {
    let home = TestHome::new("term-bytes");
    let legacy = FakeLegacy::start().await;
    let mut opts = home.options();
    // Salida no UTF-8 (el `UnicodeDecodeError` del Python): nada mutó todavía.
    opts.tmux = fake_tmux(r"printf '%%0\t1\t\377\t0\t/p\n'", Duration::from_secs(5));
    let front = front(&home, legacy.port, opts).await;
    for path in ["/terminal-panes", "/terminal-history"] {
        let wire = request_body(front.port, "POST", path, "", r#"{"session": "s1"}"#).await;
        assert_eq!(wire.text(), r#"{"legacy": true}"#, "{path}");
    }
    assert_eq!(
        legacy.requests(),
        vec![
            "POST /terminal-panes HTTP/1.1".to_owned(),
            "POST /terminal-history HTTP/1.1".to_owned()
        ]
    );
    front.stop().await;
}

#[tokio::test]
async fn terminal_missing_tmux_answers_503() {
    let home = TestHome::new("term-missing");
    let mut opts = home.options();
    opts.tmux = Tmux {
        program: Program::named("/no-existe/tmux"),
        timeout: Duration::from_secs(5),
    };
    let front = front(&home, dead_port(), opts).await;
    let wire = request_body(
        front.port,
        "POST",
        "/terminal-panes",
        "",
        r#"{"session": "s1"}"#,
    )
    .await;
    assert_eq!(
        (wire.status, wire.text().as_str()),
        (
            503,
            r#"{"error": "No se pudo completar la acci\u00f3n del panel. Comprueba Paneles antes de reintentar"}"#
        )
    );
    let wire = request_body(
        front.port,
        "POST",
        "/terminal-history",
        "",
        r#"{"session": "s1"}"#,
    )
    .await;
    assert_eq!(
        (wire.status, wire.text().as_str()),
        (503, r#"{"error": "Historial temporalmente no disponible"}"#)
    );
    front.stop().await;
}

#[tokio::test]
async fn terminal_hung_tmux_answers_503_and_frees_runtime() {
    let home = TestHome::new("term-hung");
    let mut opts = home.options();
    // `sh -c 'sleep 10' sh <args de tmux>`: cuelga sin mirar los argumentos.
    opts.tmux = fake_tmux("sleep 10", Duration::from_secs(2));
    let front = front(&home, dead_port(), opts).await;
    let port = front.port;
    let started = Instant::now();
    let hung = tokio::spawn(async move {
        request_body(port, "POST", "/terminal-panes", "", r#"{"session": "s1"}"#).await
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let quick = Instant::now();
    assert_eq!(get(port, "/prefs").await.status, 200);
    assert!(
        quick.elapsed() < Duration::from_secs(1),
        "el runtime sigue libre"
    );
    let wire = hung.await.unwrap();
    assert_eq!(
        (wire.status, wire.text().as_str()),
        (
            503,
            r#"{"error": "No se pudo completar la acci\u00f3n del panel. Comprueba Paneles antes de reintentar"}"#
        )
    );
    assert!(started.elapsed() >= Duration::from_secs(2));
    let wire = request_body(
        port,
        "POST",
        "/terminal-history",
        "",
        r#"{"session": "s1"}"#,
    )
    .await;
    assert_eq!(
        (wire.status, wire.text().as_str()),
        (503, r#"{"error": "Historial temporalmente no disponible"}"#)
    );
    front.stop().await;
}
