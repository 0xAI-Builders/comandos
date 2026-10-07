//! Dominio G1: paneles e historial de tmux por el frente, sobre un servidor
//! tmux privado compartido con el oráculo (mismas identidades de pane).
mod support;

use comandos_server::dash::native::tmux::{Program, Tmux};
use std::{
    ffi::OsString,
    time::{Duration, Instant},
};
use support::{
    FakeLegacy, TestHome, Wire, dead_port, front, get, http_golden::FrozenHttp, request_body,
    tmux_available,
};

fn tmux(home: &TestHome, args: &[&str]) -> String {
    // `-S` al socket privado: nunca el servidor del usuario.
    support::run_tmux(home, args)
}

// Read actor identities independently from the owned private tmux/kernel.
fn terminal_roots(home: &TestHome) -> Vec<(&'static str, std::path::PathBuf)> {
    let mut roots = vec![
        ("<HOME>", home.root.clone()),
        ("<REPO>", std::fs::canonicalize(support::repo()).unwrap()),
    ];
    let ids = tmux(
        home,
        &["list-panes", "-t", "=s1:", "-F", "#{pid}\t#{pane_pid}"],
    );
    for (index, line) in ids.lines().enumerate() {
        let (server, pane) = line.split_once('\t').unwrap();
        if index == 0 {
            roots.push(("<TMUX-PID>", server.into()));
            let start = comandos_runtime::session_configuration::server_start(
                &home.options().proc_root,
                server,
            )
            .unwrap();
            if !start.is_empty() {
                roots.push(("<SERVER-START>", start.into()));
            }
        }
        roots.push((
            if index == 0 {
                "<PANE0-PID>"
            } else {
                "<PANE1-PID>"
            },
            pane.into(),
        ));
    }
    for (pane, token) in [("%0", "<PANE0-VERSION>"), ("%1", "<PANE1-VERSION>")] {
        let output = tmux(
            home,
            &[
                "display-message",
                "-p",
                "-t",
                pane,
                &comandos_runtime::session_configuration::identity_format(),
            ],
        );
        let mut identity = comandos_runtime::session_configuration::identity_from_output(
            true, &output, "s1", pane,
        )
        .unwrap();
        let pid = identity["pid"].as_str().unwrap();
        let start =
            comandos_runtime::session_configuration::server_start(&home.options().proc_root, pid)
                .unwrap();
        identity.insert("server_start".into(), start.into());
        let version =
            comandos_runtime::terminal_panes::version(&serde_json::Value::Object(identity))
                .unwrap();
        roots.push((token, version.into()));
    }
    roots
}
fn terminal_aliases(home: &TestHome, py: &mut FrozenHttp<'_>) {
    for (token, path) in terminal_roots(home) {
        py.alias(token, path.to_str().unwrap());
    }
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
            env_clear: false,
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
    let mut py = FrozenHttp::new(&home, "terminal-routes", &[]).await;
    terminal_aliases(&home, &mut py);
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
            seen(&py.request("POST", path, "", body).await),
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

/// Cuerpo de `/terminal-panes` sin lo que cambia entre dos procesos: las
/// identidades y, del panel que abre un split, su id y su título (el proceso
/// del panel recién creado puede no haber hecho `exec` todavía).
fn panes_without_volatile(text: &str) -> String {
    let mut value: serde_json::Value = serde_json::from_str(text).unwrap();
    let opened = value
        .get("opened")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    if opened.is_some() {
        value["opened"] = "%N".into();
    }
    for pane in value["panes"].as_array_mut().unwrap() {
        pane["identity"] = "".into();
        if pane["id"].as_str() == opened.as_deref() {
            pane["id"] = "%N".into();
            pane["title"] = "".into();
        }
    }
    serde_json::to_string(&value).unwrap()
}

/// `select`, `resize` y `split` que sí se aplican, comparados con el oráculo
/// sobre el mismo tmux privado: primero el Python, luego el Rust desde el mismo
/// estado (el panel que abre el split del Python se cierra antes del Rust).
#[tokio::test]
async fn terminal_panes_mutations_match_python_oracle() {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new("term-mut-oracle");
    two_panes(&home);
    tmux(&home, &["set-option", "-g", "default-command", "cat"]);
    let mut py = FrozenHttp::new(&home, "terminal-mutations", &[]).await;
    terminal_aliases(&home, &mut py);
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
    let select = format!(
        r#"{{"session": "s1", "action": "select", "pane": "%0", "identity": "{identity}"}}"#
    );
    let resize = r#"{"session": "s1", "action": "resize", "pane": "%1", "axis": "x", "size": 30}"#
        .to_owned();
    for body in [&select, &resize] {
        let a = py.request("POST", "/terminal-panes", "", body).await;
        let b = request_body(front.port, "POST", "/terminal-panes", "", body).await;
        assert_eq!(a.status, 200, "{body}: {}", a.text());
        assert_eq!(seen(&a), seen(&b), "{body}");
    }
    let split = format!(
        r#"{{"session": "s1", "action": "split", "pane": "%0", "identity": "{identity}", "direction": "down"}}"#
    );
    let source = if let Some(port) = py.source_port() {
        let a = request_body(port, "POST", "/terminal-panes", "", &split).await;
        assert_eq!(a.status, 200, "{}", a.text());
        let opened: serde_json::Value = serde_json::from_str(&a.text()).unwrap();
        let opened = opened["opened"].as_str().unwrap().to_owned();
        tmux(&home, &["kill-pane", "-t", &opened]);
        tmux(&home, &["select-pane", "-t", "%0"]);
        Some((a, opened))
    } else {
        None
    };
    let roots = terminal_roots(&home);
    let roots: Vec<_> = roots
        .iter()
        .map(|(token, path)| (*token, path.as_path()))
        .collect();
    let input: serde_json::Value = serde_json::from_slice(&comandos_oracle::normalize(
        &serde_json::to_vec(&serde_json::json!({"source":support::frozen::SOURCE_COMMIT,
            "python":"3.10.12", "path":"/terminal-panes", "body":split,
            "before_panes":["%0","%1"], "normalize":"panes_without_volatile"}))
        .unwrap(),
        &roots,
    ))
    .unwrap();
    let expected = comandos_oracle::oracle_at(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        "terminal-split",
        &input,
        || {
            let (a, _) = source
                .as_ref()
                .ok_or("original split required in record/check")?;
            serde_json::to_vec(&serde_json::json!({"status":a.status,
                "type":a.header("content-type"),"body":panes_without_volatile(&a.text())}))
            .map_err(|e| e.to_string())
        },
    );
    let expected: serde_json::Value = serde_json::from_slice(&expected).unwrap();
    let b = request_body(front.port, "POST", "/terminal-panes", "", &split).await;
    assert_eq!(expected["status"], b.status);
    assert_eq!(expected["type"].as_str(), b.header("content-type"));
    if let Some((a, _)) = source {
        assert_ne!(a.text(), b.text(), "el panel nuevo tiene otro id");
    }
    let opened: serde_json::Value = serde_json::from_str(&b.text()).unwrap();
    assert!(!["%0", "%1"].contains(&opened["opened"].as_str().unwrap()));
    assert_eq!(opened["panes"].as_array().unwrap().len(), 3);
    assert_eq!(
        expected["body"].as_str().unwrap(),
        panes_without_volatile(&b.text())
    );
    front.stop().await;
}
/// `close` es nativo desde la 2f-1 (T7, `dash_native_pane_close.rs`): sin
/// sesión responde el 400 del Python y no se reenvía.
#[tokio::test]
async fn terminal_panes_close_is_native() {
    let home = TestHome::new("term-close");
    let legacy = FakeLegacy::start().await;
    let front = front(&home, legacy.port, home.options()).await;
    let body = r#"{"session": "s1", "action": "close", "pane": "%0", "identity": "x"}"#;
    assert_eq!(
        request_body(front.port, "POST", "/terminal-panes", "", body)
            .await
            .text(),
        r#"{"error": "No se encuentra la sesi\u00f3n"}"#
    );
    assert!(legacy.requests().is_empty());
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
