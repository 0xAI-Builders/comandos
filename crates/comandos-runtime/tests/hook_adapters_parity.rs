//! Paridad de los adaptadores que entregan a `cc-notify.sh` (codex, codex-hooks,
//! gemini, agy) contra sus scripts: el oráculo corre el adaptador bash con el
//! `cc-notify.sh` real detrás; el Rust, `comandos-hook <harness>`, que llama al
//! pipeline de `hook claude` en proceso. Se comparan TODOS los efectos de la cadena
//! (estado, timeline, evento N1, uso, POST, binarios falsos, `native-processes`,
//! stdout y código de salida) con `HOME` temporales y el notifyd falso.
#[path = "support/fake_notifyd.rs"]
mod fake_notifyd;
#[path = "support/parity.rs"]
mod parity;

use parity::{
    Window, collect, fake_bin, install_oracle_notify, install_rust_notify_stub, lines,
    native_records, wait_for_delivery,
};
use std::fs;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CONF: &str = "VOLUME=12\nCC_LANG=es\nTELEGRAM_ENABLED=0\nSPEAK_ATTENTION=0\nSPEAK_DONE=0\n";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn fixture(name: &str, key: Option<&str>) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hooks/adapters")
        .join(format!("{name}.json"));
    let text = fs::read_to_string(path).unwrap();
    match key {
        None => text.trim_end().to_string(),
        Some(key) => {
            let all: serde_json::Value = serde_json::from_str(&text).unwrap();
            serde_json::to_string(&all[key]).unwrap()
        }
    }
}
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

struct Case {
    name: &'static str,
    /// (oráculo relativo al repo, harness del Rust).
    script: &'static str,
    harness: &'static str,
    args: Vec<String>,
    stdin: String,
    env: Vec<(&'static str, &'static str)>,
    tmux: bool,
    /// Estado previo de codex: (clave, antigüedad en s, estado, agente).
    seed: Option<(&'static str, i64, &'static str, &'static str)>,
    /// Corre bajo un proceso cuyo `argv[0]` es `agy`.
    agy_parent: bool,
}

fn case(name: &'static str, script: &'static str, harness: &'static str) -> Case {
    Case {
        name,
        script,
        harness,
        args: Vec::new(),
        stdin: String::new(),
        env: Vec::new(),
        tmux: false,
        seed: None,
        agy_parent: false,
    }
}

fn cases() -> Vec<Case> {
    let codex_key = "proyecto-codex--fake-sess--7";
    let mut out = Vec::new();
    let notify = |name, tmux, seed| Case {
        args: vec![fixture("codex_notify", None)],
        tmux,
        seed,
        ..case(
            name,
            "crates/comandos-runtime/tests/fixtures/hooks/oracle/adapters/codex-notify.sh",
            "codex",
        )
    };
    out.push(notify("codex_notify", true, None));
    out.push(notify(
        "codex_notify_dedupe",
        true,
        Some((codex_key, 5, "done", "codex")),
    ));
    out.push(notify(
        "codex_notify_stale_done",
        true,
        Some((codex_key, 40, "done", "codex")),
    ));
    out.push(notify(
        "codex_notify_claude_done",
        false,
        Some(("proyecto-codex", 2, "done", "claude")),
    ));
    out.push(Case {
        args: vec![r#"{"type":"other","cwd":"/x"}"#.into()],
        ..case(
            "codex_notify_other",
            "crates/comandos-runtime/tests/fixtures/hooks/oracle/adapters/codex-notify.sh",
            "codex",
        )
    });
    out.push(Case {
        args: vec![
            r#"{"type":"agent-turn-complete","workspace-path":"/w/ws.x","turn-id":3} trailing"#
                .into(),
        ],
        ..case(
            "codex_notify_ws",
            "crates/comandos-runtime/tests/fixtures/hooks/oracle/adapters/codex-notify.sh",
            "codex",
        )
    });
    out.push(case(
        "codex_notify_empty",
        "crates/comandos-runtime/tests/fixtures/hooks/oracle/adapters/codex-notify.sh",
        "codex",
    ));
    for (key, tmux, seed) in [
        ("prompt", true, None),
        ("stop", true, None),
        ("stop", true, Some((codex_key, 3, "done", "codex"))),
        ("permission", true, None),
        ("permission_bare", false, None),
        ("no_cwd", false, None),
        ("other", true, None),
    ] {
        let name: &'static str = Box::leak(
            format!(
                "codex_hooks_{key}{}",
                if seed.is_some() { "_dedupe" } else { "" }
            )
            .into_boxed_str(),
        );
        out.push(Case {
            stdin: format!("{}\n", fixture("codex_hooks", Some(key))),
            tmux,
            seed,
            ..case(
                name,
                "crates/comandos-runtime/tests/fixtures/hooks/oracle/adapters/codex-hooks.sh",
                "codex-hooks",
            )
        });
    }
    for (key, env) in [
        ("before", vec![]),
        ("after", vec![]),
        ("notification", vec![("CC_AGENT", "agy")]),
        ("end", vec![]),
        ("ignored", vec![]),
    ] {
        let name: &'static str = Box::leak(format!("gemini_{key}").into_boxed_str());
        out.push(Case {
            stdin: fixture("gemini_hooks", Some(key)),
            env,
            tmux: key == "after",
            ..case(
                name,
                "crates/comandos-runtime/tests/fixtures/hooks/oracle/adapters/gemini-hooks.sh",
                "gemini",
            )
        });
    }
    for (key, event, agy_parent) in [
        ("stop", "done", true),
        ("stop", "working", true),
        ("stop", "", false),
        ("bad_sid", "waiting", true),
        ("bad_model", "end", true),
        ("no_paths", "working", true),
    ] {
        let name: &'static str =
            Box::leak(format!("agy_{key}_{event}_{agy_parent}").into_boxed_str());
        out.push(Case {
            stdin: fixture("agy_hooks", Some(key)),
            args: if event.is_empty() {
                vec![]
            } else {
                vec![event.into()]
            },
            agy_parent,
            tmux: key == "stop",
            ..case(
                name,
                "crates/comandos-runtime/tests/fixtures/hooks/oracle/adapters/agy-hooks.sh",
                "agy",
            )
        });
    }
    out.push(Case {
        stdin: fixture("agy_hooks", Some("stop")),
        args: vec!["done".into()],
        env: vec![("COMANDOS_SILENT_AGENT", "1")],
        ..case(
            "agy_silent",
            "crates/comandos-runtime/tests/fixtures/hooks/oracle/adapters/agy-hooks.sh",
            "agy",
        )
    });
    out
}

fn prepare(dir: &Path, c: &Case, side: &str, seed_now: i64) -> PathBuf {
    let home = dir.join(format!("{}-{side}", c.name));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join(".claude/hooks/state")).unwrap();
    fs::create_dir_all(home.join("tmp")).unwrap();
    fs::write(home.join(".claude/hooks/cc-notify.conf"), CONF).unwrap();
    if let Some((key, age, status, agent)) = c.seed {
        let state = serde_json::json!({"project":"proyecto-codex","status":status,"detail":"x",
            "cwd":"/home/usuario/proyecto-codex","ts":seed_now - age,"options":"","last":"",
            "agent":agent,"session":"proyecto-codex"});
        fs::write(
            home.join(format!(".claude/hooks/state/{key}.json")),
            format!("{state:#}\n"),
        )
        .unwrap();
    }
    if side == "bash" {
        install_oracle_notify(&home, &root());
    } else {
        install_rust_notify_stub(&home);
    }
    home
}

/// (código, stdout, pid del proceso `agy` si lo hubo).
fn run_side(
    dir: &Path,
    c: &Case,
    side: &str,
    home: &Path,
    url: &str,
) -> (Option<i32>, Vec<u8>, Option<u32>) {
    let mut program: Vec<String> = if side == "bash" {
        vec![
            "bash".into(),
            "-c".into(),
            "trap wait EXIT; . \"$0\" \"$@\"".into(),
            root().join(c.script).display().to_string(),
        ]
    } else {
        vec![env!("CARGO_BIN_EXE_comandos-hook").into(), c.harness.into()]
    };
    program.extend(c.args.iter().cloned());
    let mut command = if c.agy_parent {
        let mut cmd = Command::new("sh");
        cmd.arg0("agy")
            .arg("-c")
            .arg("\"$@\"; exit $?")
            .arg("agy-padre")
            .args(&program);
        cmd
    } else {
        let mut cmd = Command::new(&program[0]);
        cmd.args(&program[1..]);
        cmd
    };
    let cwd = dir.join("cwd/proyecto-pwd");
    fs::create_dir_all(&cwd).unwrap();
    command
        .env_clear()
        .current_dir(&cwd)
        .env("HOME", home)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", dir.join("fakebin").display()),
        )
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", home.join("tmp"))
        .env("FAKE_LOG", home.join("fake.log"))
        .env("FAKE_POSTS", home.join("posts.log"))
        .env("FAKE_PANE_PID", std::process::id().to_string())
        // Nunca el cc-notifyd real (4778): el Rust apunta al falso.
        .env("COMANDOS_NOTIFYD_URL", url)
        .envs(c.env.iter().copied());
    if c.tmux {
        command.env("TMUX_PANE", "%7");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = c.agy_parent.then(|| child.id());
    child
        .stdin
        .take()
        .unwrap()
        .write_all(c.stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code(), out.stdout, pid)
}

#[test]
fn adapters_match_their_scripts() {
    let dir = std::env::temp_dir().join(format!("comandos-adapters-{}", std::process::id()));
    fake_bin(&dir.join("fakebin"));
    let (port, _server, bodies) = fake_notifyd::spawn(0);
    for c in cases() {
        let start_ms = now_ms();
        let seed_now = start_ms / 1000;
        let h_bash = prepare(&dir, &c, "bash", seed_now);
        let h_rust = prepare(&dir, &c, "rust", seed_now);
        let (code_bash, out_bash, pid_bash) =
            run_side(&dir, &c, "bash", &h_bash, "http://127.0.0.1:9");
        let bash_posts = lines(&h_bash.join("posts.log"));
        for url in lines(&h_bash.join("posts.log.url")) {
            assert_eq!(url, "http://127.0.0.1:4778/notify", "{}", c.name);
        }
        bodies.lock().unwrap().clear();
        let url = format!("http://127.0.0.1:{port}");
        let (code_rust, out_rust, pid_rust) = run_side(&dir, &c, "rust", &h_rust, &url);
        wait_for_delivery(&h_rust);
        let window = Window {
            start_ms,
            end_ms: now_ms(),
        };
        let rust_posts = std::mem::take(&mut *bodies.lock().unwrap());
        let bash = collect(&h_bash, code_bash, bash_posts, window);
        let rust = collect(&h_rust, code_rust, rust_posts, window);
        assert_eq!(
            String::from_utf8_lossy(&out_bash),
            String::from_utf8_lossy(&out_rust),
            "stdout difiere en {}",
            c.name
        );
        assert_eq!(bash.state, rust.state, "estado difiere en {}", c.name);
        assert_eq!(
            bash.events, rust.events,
            "events.jsonl difiere en {}",
            c.name
        );
        assert_eq!(bash.posts, rust.posts, "POST difiere en {}", c.name);
        assert_eq!(bash.log, rust.log, "binarios falsos difieren en {}", c.name);
        assert_eq!(bash.intake, rust.intake, "evento N1 difiere en {}", c.name);
        assert_eq!(bash.usage, rust.usage, "base de uso difiere en {}", c.name);
        assert_eq!(bash, rust, "{}", c.name);
        let natives_bash = native_records(&h_bash, pid_bash, window);
        let natives_rust = native_records(&h_rust, pid_rust, window);
        assert_eq!(
            natives_bash, natives_rust,
            "native-processes difiere en {}",
            c.name
        );
        if c.name == "agy_stop_done_true" {
            assert!(
                natives_rust.len() == 2,
                "agy debía registrarse: {natives_rust:?}"
            );
        }
        let _ = fs::remove_dir_all(&h_bash);
        let _ = fs::remove_dir_all(&h_rust);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// El Rust entrega en proceso: sin `~/.claude/hooks/cc-notify.sh` (la puerta
/// `[ -x ]` del bash) codex-hooks igual deja el estado.
#[test]
fn codex_hooks_does_not_need_cc_notify_sh() {
    let dir = std::env::temp_dir().join(format!("comandos-codex-gate-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let home = dir.join("home");
    fs::create_dir_all(home.join(".claude/hooks/state")).unwrap();
    let fx: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/hooks/adapters/codex_hooks.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_comandos-hook"))
        .arg("codex-hooks")
        .env_clear()
        .current_dir(&home)
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("COMANDOS_NOTIFYD_URL", "http://127.0.0.1:9")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(fx["prompt"].to_string().as_bytes())
        .unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(0));
    let state = fs::read_to_string(home.join(".claude/hooks/state/proyecto-codex.json")).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state).unwrap();
    assert_eq!(state["status"], "working");
    assert_eq!(state["agent"], "codex");
    let _ = fs::remove_dir_all(&dir);
}
