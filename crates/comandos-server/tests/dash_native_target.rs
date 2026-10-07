//! `native::target` (Tarea 3 del maestro 2f, Parte B) contra el Python:
//! `find_project_dir`, `state_agent`, `resolve_project_session`,
//! `_pane_identity`/`_identity_key` y el preámbulo de `do_POST`.
//!
//! Confinamiento: un solo HOME temporal con su tmux privado (`-S`); el
//! Python corre confinado (`run_dash`: `PATH` = el `fakebin`, cuyo `tmux` es el
//! guardián con el mismo `-S`). Todo lo que hacen las funciones probadas es
//! leer (`list-sessions`, `display-message`, `list-panes -a`). Las sesiones
//! las crea la siembra con `run_tmux`; el `Drop` de `TestHome` mata ese
//! servidor por su `-S`. El agente es una copia de `sleep` llamada `claude`.
mod support;

use comandos_core::json::response_dumps;
use comandos_server::{
    HandlerError, ReplyBody,
    dash::native::{
        Fault, Native,
        target::{
            self, PostTarget, TargetError, find_project_dir, identity_key, pane_identity,
            post_target, resolve_project_session, state_agent,
        },
    },
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use support::{TestHome, fake_agent, oracle, run_tmux, tmux_available};

struct Seeded {
    home: TestHome,
    native: Native,
    agent_dir: PathBuf,
}

fn seed(tag: &str) -> Option<Seeded> {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return None;
    }
    let home = TestHome::new_short(tag);
    oracle::confined_fakebin(&home, &[]);
    let code = home.root.join("codebase");
    for dir in [
        "Mi.Proyecto",
        "grupo/otro",
        ".oculto",
        "agente.x",
        "grupo/.nada",
    ] {
        std::fs::create_dir_all(code.join(dir)).unwrap();
    }
    std::fs::write(code.join("archivo.txt"), "").unwrap();
    let agent_dir = std::fs::canonicalize(code.join("agente.x")).unwrap();
    home.write(
        "state/a.json",
        r#"{"project": "Mi.Proyecto", "session": "s1", "pane": "%0"}"#,
    );
    home.write("state/b.json", r#"{"project": "otro"}"#);
    home.write(
        "state/c.json",
        &json!({"project": "agente.x", "cwd": agent_dir, "agent": "codex"}).to_string(),
    );
    home.write(
        "state/d.json",
        r#"{"project": "muerto", "session": "zz", "pane": "%5"}"#,
    );
    home.write("state/.oculto.json", r#"["no es un objeto"]"#);
    run_tmux(&home, &["new-session", "-d", "-s", "s1", "cat"]);
    // El «agente»: `sleep` con argv[0] = `claude`, en un pane de s2 cuyo cwd
    // es el del proyecto.
    let claude = fake_agent(&home, "claude");
    let dir = agent_dir.to_str().unwrap();
    run_tmux(
        &home,
        &[
            "new-session",
            "-d",
            "-s",
            "s2",
            "-c",
            dir,
            &format!("exec {} 300", claude.display()),
        ],
    );
    // Hasta que el proceso del pane ya sea `claude` (no el shell que lo lanza).
    let started = Instant::now();
    loop {
        let cmd = run_tmux(
            &home,
            &[
                "display-message",
                "-p",
                "-t",
                "=s2:",
                "#{pane_current_command}",
            ],
        );
        if cmd.trim() == "claude" {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "el agente no arrancó: {cmd}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let native = Native::new(home.options());
    Some(Seeded {
        home,
        native,
        agent_dir,
    })
}

const ORACLE: &str = r#"
import json
def run(f):
    try:
        return f()
    except Exception as e:
        return {"error": str(e)}
print(json.dumps([
    run(lambda: dash.find_project_dir("mi-proyecto")),
    run(lambda: dash.find_project_dir("otro")),
    run(lambda: dash.find_project_dir("AGENTE-X")),
    run(lambda: dash.find_project_dir("nada")),
    run(lambda: dash.find_project_dir("oculto")),
    run(lambda: dash.state_agent("mi-proyecto")),
    run(lambda: dash.state_agent("agente-x")),
    run(lambda: dash.state_agent("nada")),
    run(lambda: dash.resolve_project_session("Mi-Proyecto")),
    run(lambda: dash.resolve_project_session("mi-proyecto")),
    run(lambda: dash.state_agent("Mi-Proyecto")),
    run(lambda: dash.resolve_project_session("otro")),
    run(lambda: dash.resolve_project_session("agente-x")),
    run(lambda: dash.resolve_project_session("muerto")),
    run(lambda: dash.resolve_project_session("nada")),
    run(lambda: dash._pane_identity("s1", "%0")),
    run(lambda: dash._identity_key(dash._pane_identity("s1", "%0"))),
    run(lambda: dash._pane_identity("s1", "x")),
    run(lambda: dash._pane_identity("s1", "%99")),
    run(lambda: dash._pane_identity("otra", "%0")),
]))
"#;

fn path_value(found: Result<Option<PathBuf>, Fault>) -> Value {
    match found {
        Ok(Some(p)) => Value::from(p.to_str().unwrap()),
        Ok(None) => Value::Null,
        Err(_) => panic!("declinó"),
    }
}

fn identity_value(found: Result<serde_json::Map<String, Value>, TargetError>) -> Value {
    match found {
        Ok(map) => Value::Object(map),
        Err(TargetError::Value(text)) => json!({"error": text}),
        Err(TargetError::Fault(_)) => panic!("declinó"),
    }
}

#[tokio::test]
async fn target_helpers_match_python() {
    let Some(s) = seed("target") else {
        return;
    };
    let (root, state) = (&s.home.root, s.home.hooks().join("state"));
    let agent = |sess: &str| match state_agent(&state, sess) {
        Ok(a) => Value::from(a),
        Err(_) => panic!("declinó"),
    };
    let resolve = |sess: &'static str| {
        let native = &s.native;
        async move {
            match resolve_project_session(native, sess).await {
                Ok(Some((a, b))) => json!([a, b]),
                Ok(None) => Value::Null,
                Err(_) => panic!("declinó"),
            }
        }
    };
    let identity = pane_identity(&s.native, "s1", "%0").await;
    let key = match &identity {
        Ok(map) => Value::from(identity_key(map)),
        Err(_) => panic!("sin identidad"),
    };
    let rust = json!([
        path_value(find_project_dir(root, "mi-proyecto")),
        path_value(find_project_dir(root, "otro")),
        path_value(find_project_dir(root, "AGENTE-X")),
        path_value(find_project_dir(root, "nada")),
        path_value(find_project_dir(root, "oculto")),
        agent("mi-proyecto"),
        agent("agente-x"),
        agent("nada"),
        resolve("Mi-Proyecto").await,
        resolve("mi-proyecto").await,
        agent("Mi-Proyecto"),
        resolve("otro").await,
        resolve("agente-x").await,
        resolve("muerto").await,
        resolve("nada").await,
        identity_value(identity),
        key,
        identity_value(pane_identity(&s.native, "s1", "x").await),
        identity_value(pane_identity(&s.native, "s1", "%99").await),
        identity_value(pane_identity(&s.native, "otra", "%0").await),
    ]);
    // Lo que la prueba quiere ver, por si el oráculo no está.
    // `session_name` no pasa a minúsculas: solo «Mi-Proyecto» nombra el registro.
    assert_eq!(rust[8], json!(["s1", "%0"]));
    assert_eq!(rust[9], Value::Null);
    assert_eq!(rust[10], "claude");
    assert_eq!(rust[11], Value::Null);
    assert_eq!(rust[12][0], "s2", "{rust}");
    assert_eq!(rust[6], "codex");
    assert_eq!(
        rust[2],
        Value::from(s.agent_dir.to_str().unwrap()),
        "find_project_dir no canoniza"
    );
    // Aliases come from an independent query to the real private tmux actor,
    // not from the native value under test. No process state is restored.
    let pids = run_tmux(
        &s.home,
        &["display-message", "-p", "-t", "=s1:", "#{pid}\t#{pane_pid}"],
    );
    let pids: Vec<PathBuf> = pids.trim_end().split('\t').map(PathBuf::from).collect();
    assert_eq!(pids.len(), 2);
    let server_start = comandos_runtime::session_configuration::server_start(
        &s.native.options().proc_root,
        pids[0].to_str().unwrap(),
    )
    .unwrap();
    let server_start = PathBuf::from(server_start);
    let repo = std::fs::canonicalize(support::repo()).unwrap();
    let mut roots = vec![
        ("<REPO>", repo.as_path()),
        ("<HOME>", s.home.root.as_path()),
        ("<TMUX-PID>", pids[0].as_path()),
        ("<PANE-PID>", pids[1].as_path()),
    ];
    if !server_start.as_os_str().is_empty() {
        roots.push(("<SERVER-START>", server_start.as_path()));
    }
    let expected = comandos_oracle::oracle_at(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        "target-helpers",
        &json!({"source":support::frozen::SOURCE_COMMIT, "python":"3.10.12", "code":ORACLE,
            "fixture":"private tmux s1 cat, s2 claude sleep, seeded project registry"}),
        || {
            support::frozen::run_dash_original(&s.home, ORACLE, &Default::default())
                .map(|text| comandos_oracle::normalize(text.as_bytes(), &roots))
        },
    );
    let expected = String::from_utf8(comandos_oracle::restore(&expected, &roots)).unwrap();
    assert_eq!(response_dumps(&rust).unwrap(), expected.trim_end());
}

/// Un nombre de carpeta no ASCII que habría que comparar: se declina (el
/// `lower()` de Python es Unicode).
#[test]
fn find_project_dir_declines_non_ascii_names() {
    let home = TestHome::new_short("target-nonascii");
    std::fs::create_dir_all(home.root.join("codebase/Ñandú")).unwrap();
    assert!(matches!(
        find_project_dir(&home.root, "x"),
        Err(Fault::Decline)
    ));
    assert!(matches!(
        find_project_dir(&home.root, "ñ"),
        Err(Fault::Decline)
    ));
    // Un archivo (no carpeta) con nombre no ASCII no se compara.
    let other = TestHome::new_short("target-nonascii-f");
    std::fs::create_dir_all(other.root.join("codebase")).unwrap();
    std::fs::write(other.root.join("codebase/ñ.txt"), "").unwrap();
    assert!(matches!(find_project_dir(&other.root, "x"), Ok(None)));
}

/// Un registro que no es objeto antes del que casa: el Python lanza (500); se declina.
#[test]
fn state_records_that_python_cannot_read_decline() {
    let home = TestHome::new_short("target-badrec");
    home.write("state/a.json", "[1, 2]");
    assert!(matches!(
        state_agent(&home.hooks().join("state"), "x"),
        Err(Fault::Decline)
    ));
    let home = TestHome::new_short("target-badagent");
    home.write("state/a.json", r#"{"project": "x", "agent": 5}"#);
    assert!(matches!(
        state_agent(&home.hooks().join("state"), "x"),
        Err(Fault::Decline)
    ));
    home.write("state/a.json", r#"{"project": "x", "agent": ""}"#);
    assert_eq!(
        state_agent(&home.hooks().join("state"), "x")
            .ok()
            .as_deref(),
        Some("claude")
    );
}

fn body_of(answer: comandos_server::dash::native::Answer) -> (u16, String) {
    match answer {
        Ok(reply) => {
            let ReplyBody::Bytes(bytes) = reply.body else {
                panic!("cuerpo en flujo");
            };
            (
                reply.status.as_u16(),
                String::from_utf8(bytes.to_vec()).unwrap(),
            )
        }
        Err(Fault::Decline) => (0, "decline".into()),
        Err(Fault::Error(HandlerError::Failure)) => (500, "failure".into()),
        Err(Fault::Error(HandlerError::Timeout)) => (504, "timeout".into()),
    }
}

async fn target_of(native: &Native, path: &str, data: Value) -> Result<PostTarget, (u16, String)> {
    post_target(native, path, &data).await.map_err(body_of)
}

#[tokio::test]
async fn post_target_follows_the_do_post_preamble() {
    let Some(s) = seed("target-post") else {
        return;
    };
    let n = &s.native;
    let plain = target_of(n, "/send", json!({"session": "s1"}))
        .await
        .unwrap();
    assert_eq!(
        plain,
        PostTarget {
            sess: "s1".into(),
            target: "=s1".into(),
            pane: "=s1:".into(),
            resolved: None,
        }
    );
    // El proyecto se resuelve a su sesión real y a su pane.
    let resolved = target_of(n, "/send", json!({"session": "Mi-Proyecto", "pane": ""}))
        .await
        .unwrap();
    assert_eq!(resolved.sess, "s1");
    assert_eq!(resolved.target, "=s1");
    assert_eq!(resolved.pane, "%0");
    assert_eq!(resolved.resolved, Some(("s1".into(), "%0".into())));
    // Rutas de configuración: la sesión pedida se queda; el pane sí se toma.
    for path in target::CONFIG_ROUTES {
        let configured = target_of(n, path, json!({"session": "Mi-Proyecto"}))
            .await
            .unwrap();
        assert_eq!(configured.sess, "Mi-Proyecto", "{path}");
        assert_eq!(configured.target, "=Mi-Proyecto", "{path}");
        assert_eq!(configured.pane, "%0", "{path}");
    }
    // El resolutor del agente: sesión y pane del proceso.
    let agent = target_of(n, "/key", json!({"session": "agente-x"}))
        .await
        .unwrap();
    assert_eq!(agent.sess, "s2");
    assert!(agent.pane.starts_with('%'), "{agent:?}");
    // Pane muerto o falso → el de la sesión; un falso no texto cae al resuelto.
    let dead = target_of(n, "/send", json!({"session": "s1", "pane": "%42"}))
        .await
        .unwrap();
    assert_eq!(dead.pane, "=s1:");
    let falsy = target_of(n, "/send", json!({"session": "Mi-Proyecto", "pane": 0}))
        .await
        .unwrap();
    assert_eq!(falsy.pane, "%0");
    let truthy = target_of(n, "/send", json!({"session": "Mi-Proyecto", "pane": 7}))
        .await
        .unwrap();
    assert_eq!(truthy.pane, "=s1:");
    // Validación de la sesión.
    for bad in [json!({}), json!({"session": "a b"}), json!({"session": ""})] {
        assert_eq!(
            target_of(n, "/send", bad).await.unwrap_err(),
            (400, r#"{"error": "Nombre de sesion invalido"}"#.into())
        );
    }
    assert_eq!(
        target_of(n, "/send", json!({"session": 5}))
            .await
            .unwrap_err(),
        (500, "failure".into())
    );
    // `\d` Unicode de PANE_RE: no se reproduce, se declina.
    assert_eq!(
        target_of(n, "/send", json!({"session": "s1", "pane": "%٣"}))
            .await
            .unwrap_err(),
        (0, "decline".into())
    );
}

/// Nada de lo que hace el módulo crea o mata sesiones: las mismas antes y después.
#[tokio::test]
async fn target_only_reads_tmux() {
    let Some(s) = seed("target-ro") else {
        return;
    };
    let list = || {
        let mut out = run_tmux(
            &s.home,
            &["list-panes", "-a", "-F", "#{session_name} #{pane_id}"],
        );
        out.push_str(&run_tmux(
            &s.home,
            &["list-sessions", "-F", "#{session_name}"],
        ));
        out
    };
    let before = list();
    for sess in [
        "Mi-Proyecto",
        "mi-proyecto",
        "otro",
        "agente-x",
        "muerto",
        "nada",
    ] {
        let _ = resolve_project_session(&s.native, sess).await;
        let _ = post_target(&s.native, "/send", &json!({"session": sess, "pane": "%0"})).await;
    }
    let _ = pane_identity(&s.native, "s1", "%0").await;
    assert_eq!(before, list());
    // El tmux del frente es el privado de la prueba.
    support::assert_private_tmux(s.native.options());
}

/// Revisión de la Tarea 3: el frente emite las MISMAS órdenes de tmux, con los
/// mismos argumentos y en el mismo orden, que el Python. El `opts.tmux` del
/// frente apunta al guardián del `fakebin` (que anota en `<HOME>/tmux.log` y
/// conserva el `-S` privado en el prefijo); el Python confinado llega al mismo
/// guardián por su `PATH`. Se compara el registro de cada operación por
/// separado: `resolve_project_session` de `Mi-Proyecto` y `agente-x`, y el
/// preámbulo de los POST con un pane (`post_target` frente a `operator_pane`,
/// que hace la misma resolución y la misma comprobación del pane).
#[tokio::test]
async fn target_tmux_argv_matches_python() {
    let Some(s) = seed("target-argv") else {
        return;
    };
    let mut opts = s.home.options();
    opts.tmux.program.path = s.home.root.join("fakebin/tmux");
    support::assert_private_tmux(&opts);
    let native = Native::new(opts);
    let log = s.home.root.join("tmux.log");
    let take = || {
        let calls = support::twin::tmux_log(&s.home);
        let _ = std::fs::remove_file(&log);
        calls
    };
    let _ = std::fs::remove_file(&log);
    let mut rust = Vec::new();
    for sess in ["Mi-Proyecto", "agente-x"] {
        assert!(
            resolve_project_session(&native, sess).await.is_ok(),
            "{sess}"
        );
        rust.push(take());
    }
    for (sess, pane) in [("s1", "%0"), ("Mi-Proyecto", "%0"), ("s1", "%42")] {
        let target = post_target(&native, "/send", &json!({"session": sess, "pane": pane})).await;
        assert!(target.is_ok(), "{sess} {pane}");
        rust.push(take());
    }
    // Sin oráculo, al menos el frente pasó por el guardián.
    assert!(rust.iter().all(|calls| !calls.is_empty()), "{rust:?}");
    assert!(
        rust.iter()
            .flatten()
            .all(|args| args.first().is_some_and(|v| v != "kill-server")),
        "{rust:?}"
    );
    let steps = [
        "dash.resolve_project_session('Mi-Proyecto')",
        "dash.resolve_project_session('agente-x')",
        "dash.operator_pane('s1', '%0')",
        "dash.operator_pane('Mi-Proyecto', '%0')",
        "dash.operator_pane('s1', '%42')",
    ];
    let roots = [("<HOME>", s.home.root.as_path())];
    let expected = comandos_oracle::oracle_at(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        "target-tmux-argv",
        &json!({"source":support::frozen::SOURCE_COMMIT, "python":"3.10.12", "steps":steps,
            "fixture":"private tmux s1 cat, s2 claude sleep, seeded project registry"}),
        || {
            let mut python = Vec::new();
            for step in steps {
                support::frozen::run_dash_original(&s.home, step, &Default::default())?;
                python.push(take());
            }
            Ok(comandos_oracle::normalize(
                &serde_json::to_vec(&python).map_err(|e| e.to_string())?,
                &roots,
            ))
        },
    );
    let python: Vec<Vec<Vec<String>>> =
        serde_json::from_slice(&comandos_oracle::restore(&expected, &roots)).unwrap();
    for ((step, front), oracle) in steps.iter().zip(&rust).zip(&python) {
        assert_eq!(front, oracle, "{step}");
    }
}
