//! Corte tabs, T7: POST `/terminal-panes` con `action:"close"` contra el
//! Python (gemelo): la copia de recuperación en
//! `~/.local/state/comandos/closed-panes` y después el cierre del pane.
//!
//! Confinamiento: cada lado del gemelo tiene su HOME temporal corto, su tmux
//! privado (`-S`) y su `fakebin` confinado (`PATH` = solo él). La sesión `s1`
//! (tres panes `cat`) la siembra la prueba por `support::run_tmux` (`-S` al
//! socket privado); el único pane que se cierra es uno de esa sesión, por el
//! `if-shell` de la 2c, que comprueba identidad y que no sea el último. La
//! copia va al `closed-panes` del HOME temporal de cada lado (el frente la
//! calcula desde `opts.home`; el Python, desde su `HOME` confinado). Cada
//! prueba empieza por `assert_confined` (canario: `-S` privado en el frente y
//! en el guardián del Python) ANTES de cualquier cierre. La limpieza es el
//! `Drop` de cada `TestHome` (`kill-server -S` y después borrar).
mod support;

use comandos_server::dash::native::Cut;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use support::{
    TestHome, run_tmux,
    tabs::normalize_home,
    twin::{Twin, TwinOpts, normalize, tmux_log},
};

/// `s1` con tres panes `cat` en una ventana.
fn seed(h: &TestHome) {
    run_tmux(
        h,
        &[
            "new-session",
            "-d",
            "-s",
            "s1",
            "-x",
            "120",
            "-y",
            "40",
            "cat",
        ],
    );
    run_tmux(h, &["split-window", "-t", "=s1:", "cat"]);
    run_tmux(h, &["split-window", "-t", "=s1:", "cat"]);
}

/// Canario: el tmux del frente y el guardián del Python llevan `-S` al
/// socket privado de su HOME; ningún socket es el del usuario.
fn assert_confined(t: &Twin) {
    for home in [&t.a, &t.b] {
        let socket = comandos_server::dash::native::tmux::private_socket(&home.tmux_dir());
        assert!(socket.starts_with(&home.root), "socket fuera del HOME");
        let guard = std::fs::read_to_string(home.root.join("fakebin/tmux")).unwrap();
        assert!(
            guard.contains(&format!("-S '{}'", socket.display())),
            "guardián sin -S privado: {guard}"
        );
        assert!(guard.contains(" -i "), "guardián sin env -i: {guard}");
    }
    let opts = &t.front_options;
    assert_eq!(opts.tmux.program.path, t.a.root.join("fakebin/tmux"));
    assert_eq!(opts.home, t.a.root, "la copia iría fuera del HOME temporal");
    support::assert_private_tmux(opts);
}

fn closed_dir(home: &TestHome) -> PathBuf {
    home.root.join(".local/state/comandos/closed-panes")
}

/// Archivos de `closed-panes` (vacío si la carpeta no existe).
fn copies(home: &TestHome) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(closed_dir(home))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

fn panes_of(t: &Twin, side: char) -> String {
    let args = ["list-panes", "-t", "=s1:", "-F", "#{pane_id}"];
    if side == 'a' {
        t.tmux_a(&args)
    } else {
        t.tmux_b(&args)
    }
}

/// La identidad pública de `pane` según un `action:"list"` del lado dado.
fn identity_in(list: &str, pane: &str) -> Value {
    let list: Value = serde_json::from_str(list).unwrap();
    list["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == pane)
        .map(|p| p["identity"].clone())
        .unwrap()
}

/// Cuerpo comparable: la identidad depende del servidor tmux de cada lado.
fn comparable_body(text: &str) -> String {
    let mut value: Value = serde_json::from_str(text).unwrap();
    if let Some(panes) = value.get_mut("panes").and_then(Value::as_array_mut) {
        for pane in panes {
            pane["identity"] = "".into();
        }
    }
    normalize(&serde_json::to_string(&value).unwrap())
}

/// Copia comparable: pids, inicios de proceso y marcas de reloj dependen de
/// cada lado; las rutas del HOME, también.
fn comparable_copy(home: &TestHome, path: &Path) -> Value {
    fn walk(home: &TestHome, value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, inner) in map.iter_mut() {
                    if ["pid", "start", "captured_at", "closedAt"].contains(&key.as_str()) {
                        *inner = Value::from("V");
                    } else {
                        walk(home, inner);
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| walk(home, v)),
            Value::String(text) => *text = normalize_home(home, text),
            _ => {}
        }
    }
    let text = std::fs::read_to_string(path).unwrap();
    let mut value: Value = serde_json::from_str(&text).unwrap();
    walk(home, &mut value);
    value
}

/// Una llamada de tmux comparable: los pids del servidor y del pane de la
/// condición del `if-shell` son de cada lado.
fn comparable_call(call: &[String]) -> Vec<String> {
    let pids = regex::Regex::new(r"#\{==:#\{(pid|pane_pid)\},[0-9]+\}").unwrap();
    call.iter()
        .map(|a| pids.replace_all(a, "#{==:#{$1},N}").into_owned())
        .collect()
}

fn take_log(home: &TestHome) -> Vec<Vec<String>> {
    let log = tmux_log(home).iter().map(|c| comparable_call(c)).collect();
    let _ = std::fs::remove_file(home.root.join("tmux.log"));
    log
}

#[tokio::test]
async fn pane_close_matches_python() {
    let Some(t) = Twin::start("pc", seed).await else {
        return;
    };
    assert_confined(&t);
    assert_eq!(panes_of(&t, 'a'), "%0\n%1\n%2\n");
    assert_eq!(panes_of(&t, 'b'), "%0\n%1\n%2\n");
    let list = r#"{"session":"s1","action":"list"}"#;
    let listed = t.post("/terminal-panes", list).await;
    assert_eq!(listed.front.status, 200, "{}", listed.front.text());
    assert_eq!(
        comparable_body(&listed.front.text()),
        comparable_body(&listed.oracle.text())
    );
    let _ = (take_log(&t.a), take_log(&t.b));
    let close = |identity: Value| {
        json!({"session":"s1","action":"close","pane":"%1","identity":identity}).to_string()
    };
    let front = t
        .post_front(
            "/terminal-panes",
            &close(identity_in(&listed.front.text(), "%1")),
        )
        .await;
    let oracle = t
        .post_oracle(
            "/terminal-panes",
            &close(identity_in(&listed.oracle.text(), "%1")),
        )
        .await;
    assert_eq!(oracle.status, 200, "{}", oracle.text());
    assert_eq!(
        (
            front.status,
            front.header("content-type").map(str::to_owned)
        ),
        (
            oracle.status,
            oracle.header("content-type").map(str::to_owned)
        )
    );
    assert_eq!(
        comparable_body(&front.text()),
        comparable_body(&oracle.text())
    );
    // El pane cerrado es el pedido y solo él.
    assert_eq!(panes_of(&t, 'a'), "%0\n%2\n");
    assert_eq!(panes_of(&t, 'b'), "%0\n%2\n");
    // Mismas órdenes de tmux, en el mismo orden (lecturas incluidas): la
    // copia (`list-windows`, `show-options`, `capture-pane`…) antes del
    // `if-shell` que mata el pane.
    let (log_a, log_b) = (take_log(&t.a), take_log(&t.b));
    assert_eq!(log_a, log_b);
    let kill = log_a
        .iter()
        .position(|c| c.first().is_some_and(|v| v == "if-shell"))
        .expect("sin if-shell");
    let capture = log_a
        .iter()
        .position(|c| c.first().is_some_and(|v| v == "capture-pane"))
        .expect("sin capture-pane");
    assert!(capture < kill, "la copia no va antes del cierre");
    let kill_call = log_a.get(kill).unwrap();
    assert_eq!(kill_call.get(3).map(String::as_str), Some("%1"));
    assert!(kill_call.iter().any(|a| a == "kill-pane -t %1"));
    // Una copia por lado, con el mismo contenido tras normalizar.
    let (ca, cb) = (copies(&t.a), copies(&t.b));
    assert_eq!((ca.len(), cb.len()), (1, 1), "{ca:?} {cb:?}");
    let name = |p: &PathBuf| p.file_name().unwrap().to_str().unwrap().to_owned();
    let pattern = regex::Regex::new(r"^[0-9]{19}-[0-9a-f]{32}\.json$").unwrap();
    assert!(pattern.is_match(&name(&ca[0])), "{}", name(&ca[0]));
    let front_body: Value = serde_json::from_str(&front.text()).unwrap();
    assert_eq!(front_body["snapshot"], name(&ca[0]).as_str());
    assert_eq!(front_body["closed"], "%1");
    assert_eq!(comparable_copy(&t.a, &ca[0]), comparable_copy(&t.b, &cb[0]));
    // Permisos: carpeta 0700, archivo 0600, como el Python.
    use std::os::unix::fs::PermissionsExt;
    for (home, file) in [(&t.a, &ca[0]), (&t.b, &cb[0])] {
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&closed_dir(home)), 0o700);
        assert_eq!(mode(file), 0o600);
    }
}

#[tokio::test]
async fn pane_close_identity_mismatch_writes_nothing() {
    let Some(t) = Twin::start("pcid", seed).await else {
        return;
    };
    assert_confined(&t);
    for body in [
        r#"{"session":"s1","action":"close","pane":"%1","identity":"falsa"}"#,
        r#"{"session":"s1","action":"close","pane":"%9","identity":"x"}"#,
        r#"{"session":"s1","action":"close","pane":"%1"}"#,
        r#"{"session":"nadie","action":"close","pane":"%1","identity":"x"}"#,
    ] {
        let run = t.post("/terminal-panes", body).await;
        assert_eq!(run.front.status, 400, "{body}: {}", run.front.text());
        run.assert_same();
    }
    for home in [&t.a, &t.b] {
        assert!(!closed_dir(home).exists(), "se escribió una copia");
    }
    assert_eq!(panes_of(&t, 'a'), "%0\n%1\n%2\n");
    assert_eq!(panes_of(&t, 'b'), "%0\n%1\n%2\n");
    let (log_a, log_b) = (take_log(&t.a), take_log(&t.b));
    assert_eq!(log_a, log_b);
    assert!(
        log_a.iter().all(|c| c
            .first()
            .is_none_or(|v| v != "if-shell" && v != "capture-pane")),
        "{log_a:?}"
    );
}

#[tokio::test]
async fn pane_close_respects_cuts_off() {
    let opts = TwinOpts {
        front: Some(Box::new(|o| {
            o.cuts_off.insert(Cut::Tabs);
        })),
        ..TwinOpts::default()
    };
    let Some(t) = Twin::start_with("pccut", seed, opts).await else {
        return;
    };
    assert_confined(&t);
    assert!(t.front_options.cuts_off.contains(&Cut::Tabs));
    // `list` es de la 2c (`Cut::Base`): sigue nativo con el corte apagado.
    let listed = t
        .post_front("/terminal-panes", r#"{"session":"s1","action":"list"}"#)
        .await;
    assert_eq!(listed.status, 200, "{}", listed.text());
    let _ = take_log(&t.a);
    let body = json!({
        "session":"s1","action":"close","pane":"%1",
        "identity": identity_in(&listed.text(), "%1"),
    })
    .to_string();
    let wire = t.post_front("/terminal-panes", &body).await;
    assert_eq!(wire.text(), r#"{"legacy": true}"#);
    // Declina antes de cualquier efecto: ni tmux, ni copia, ni pane cerrado.
    assert_eq!(take_log(&t.a), Vec::<Vec<String>>::new());
    assert!(!closed_dir(&t.a).exists());
    assert_eq!(panes_of(&t, 'a'), "%0\n%1\n%2\n");
}
