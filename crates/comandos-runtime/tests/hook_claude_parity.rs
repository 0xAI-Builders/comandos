//! Paridad de `comandos hook claude` contra el oráculo `hooks/cc-notify.sh`: mismo
//! payload, `HOME` temporales independientes, mismos binarios falsos en el `PATH`
//! (nunca el tmux, el sonido ni el cc-notifyd reales) y comparación de todo efecto
//! observable: estado, timeline, evento N1, base de uso, POST y binarios externos.
#[path = "support/fake_notifyd.rs"]
mod fake_notifyd;
#[path = "support/parity.rs"]
mod parity;

use parity::{Window, collect, fake_bin, lines, wait_for_delivery};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const BASE_CONF: &str =
    "VOLUME=12\nCC_LANG=es\nTELEGRAM_ENABLED=0\nSPEAK_ATTENTION=0\nSPEAK_DONE=0\n";
const NO_INTAKE: &[(&str, &str)] = &[("COMANDOS_STATE_DB", "/dev/null/sin-registro.sqlite3")];
const PANE_KEY: &str = "proyecto-prueba--fake-sess--7";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks")
}
/// Directorio propio de cada prueba (corren en paralelo y no comparten falsos).
fn base(test: &str) -> PathBuf {
    std::env::temp_dir().join(format!("comandos-hook-{test}-{}", std::process::id()))
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[derive(Default)]
struct Scenario {
    name: &'static str,
    payload: Option<&'static str>,
    args: &'static [&'static str],
    conf: &'static str,
    env: &'static [(&'static str, &'static str)],
    tmux: bool,
    /// Estado previo: (antigüedad en s, estado, detalle).
    seed: Option<(i64, &'static str, &'static str)>,
    piper_model: bool,
    curl_fails: bool,
}

fn scenarios() -> Vec<Scenario> {
    let usage: &[(&str, &str)] = &[
        ("COMANDOS_USAGE_INPUT_TOKENS", "1200"),
        ("COMANDOS_USAGE_OUTPUT_TOKENS", "340"),
        ("COMANDOS_USAGE_CACHE_READ_TOKENS", "5000"),
        ("COMANDOS_USAGE_CACHE_WRITE_TOKENS", "7.0"),
        ("COMANDOS_USAGE_COST_USD", "0.0123"),
        ("COMANDOS_USAGE_MODEL", "claude-prueba"),
        ("COMANDOS_USAGE_REASONING_EFFORT", "high"),
    ];
    vec![
        Scenario {
            name: "prompt",
            payload: Some("prompt"),
            tmux: true,
            seed: Some((30, "done", "respuesta **anterior**")),
            ..Default::default()
        },
        Scenario {
            name: "stop",
            payload: Some("stop"),
            tmux: true,
            env: usage,
            seed: Some((5, "working", "")),
            ..Default::default()
        },
        Scenario {
            name: "notification_permission",
            payload: Some("notification_permission"),
            ..Default::default()
        },
        Scenario {
            name: "notification_idle",
            payload: Some("notification_idle"),
            seed: Some((30, "waiting", "x")),
            ..Default::default()
        },
        Scenario {
            name: "notification_ask",
            payload: Some("notification_ask"),
            seed: Some((700, "waiting", "x")),
            ..Default::default()
        },
        Scenario {
            name: "session_end",
            payload: Some("session_end"),
            tmux: true,
            seed: Some((9, "done", "fin")),
            ..Default::default()
        },
        // La política de avisos deja los `done` sin sonido; con el registro N1 caído
        // (`COMANDOS_STATE_DB` imposible) el bash cae a `legacy` y suena: así se
        // cubren voz y chime de los `done`.
        Scenario {
            name: "stop_piper",
            payload: Some("stop_no_cwd"),
            conf: "VOLUME=40\nCC_LANG=en\nSPEAK_DONE=1\n",
            env: NO_INTAKE,
            piper_model: true,
            ..Default::default()
        },
        Scenario {
            name: "stop_spd",
            payload: Some("stop_no_cwd"),
            conf: "VOLUME=7\nCC_LANG=auto\nSPEAK_DONE=1\n",
            env: &[
                ("LANG", "es_MX.UTF-8"),
                ("COMANDOS_STATE_DB", "/dev/null/sin-registro.sqlite3"),
            ],
            ..Default::default()
        },
        Scenario {
            name: "stop_chime",
            payload: Some("stop"),
            conf: "VOLUME=250\n",
            env: NO_INTAKE,
            ..Default::default()
        },
        Scenario {
            name: "adapter_codex",
            args: &[
                "--agent",
                "codex",
                "--event",
                "waiting",
                "--cwd",
                "/tmp/otro.proyecto:x",
                "--msg",
                "Codex necesita permiso: Bash",
                "--full",
                "texto **completo**\ncon líneas",
                "--session-id",
                "thread-1",
                "--turn-id",
                "turn-1",
                "--request-id",
                "call-1",
                "--hook-event",
                "PermissionRequest",
                "--notification-type",
                "permission_prompt",
            ],
            tmux: true,
            ..Default::default()
        },
        // El camino vivo de `adapters/codex-notify.sh`.
        Scenario {
            name: "adapter_codex_done",
            args: &[
                "--agent",
                "codex",
                "--event",
                "done",
                "--cwd",
                "/tmp/proyecto-prueba",
                "--full",
                "Listo: **cambios** <hechos> & [doc](https://x.invalid)\nsegunda línea",
                "--hook-event",
                "Stop",
                "--session-id",
                "thread-2",
                "--turn-id",
                "turn-2",
            ],
            env: NO_INTAKE,
            ..Default::default()
        },
        Scenario {
            name: "curl_fails",
            payload: Some("stop"),
            curl_fails: true,
            ..Default::default()
        },
        Scenario {
            name: "precompact_quiet",
            payload: Some("precompact"),
            conf: "NOTIFY_ON_DONE=0\n",
            ..Default::default()
        },
        Scenario {
            name: "silent_agent",
            payload: Some("stop"),
            env: &[("COMANDOS_SILENT_AGENT", "1")],
            ..Default::default()
        },
    ]
}

fn prepare_home(dir: &Path, s: &Scenario, side: &str, seed_now: i64) -> PathBuf {
    let home = dir.join(format!("{}-{side}", s.name));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join(".claude/hooks/state")).unwrap();
    fs::create_dir_all(home.join("tmp")).unwrap();
    fs::write(
        home.join(".claude/hooks/cc-notify.conf"),
        format!("{BASE_CONF}{}", s.conf),
    )
    .unwrap();
    if let Some((age, status, detail)) = s.seed {
        let key = if s.tmux { PANE_KEY } else { "proyecto-prueba" };
        let state = serde_json::json!({"project":"proyecto-prueba","status":status,"detail":detail,"cwd":"/tmp/proyecto-prueba",
            "ts":seed_now - age,"options":"","last":"","agent":"claude","session":"proyecto-prueba"});
        fs::write(
            home.join(format!(".claude/hooks/state/{key}.json")),
            format!("{state}\n"),
        )
        .unwrap();
        fs::write(
            home.join(format!(".claude/hooks/state/.{key}.grok")),
            "{}\n",
        )
        .unwrap();
    }
    if s.piper_model {
        fs::create_dir_all(home.join(".local/share/piper-voices")).unwrap();
        fs::write(
            home.join(".local/share/piper-voices/es_MX-ald-medium.onnx"),
            "modelo",
        )
        .unwrap();
    }
    if side == "bash" {
        // El bash solo contabiliza uso si encuentra `cc_usage.py` (como en producción).
        fs::create_dir_all(home.join(".local/bin")).unwrap();
        std::os::unix::fs::symlink(
            root().join("bin/cc_usage.py"),
            home.join(".local/bin/cc_usage.py"),
        )
        .unwrap();
    }
    home
}

fn run_side(dir: &Path, s: &Scenario, side: &str, home: &Path, notifyd_url: &str) -> Option<i32> {
    let fake = dir.join("fakebin");
    let mut command = if side == "bash" {
        // `trap wait EXIT`: el oráculo espera a sus trabajos en segundo plano
        // (voz, popup, `cc_usage.py`) antes de que la prueba compare.
        let mut c = Command::new("bash");
        c.arg("-c")
            .arg("trap wait EXIT; . \"$0\" \"$@\"")
            .arg(root().join("hooks/cc-notify.sh"));
        c
    } else {
        let mut c = Command::new(env!("CARGO_BIN_EXE_comandos-hook"));
        c.arg("claude");
        c
    };
    let cwd = dir.join("cwd/proyecto-pwd");
    fs::create_dir_all(&cwd).unwrap();
    command
        .args(s.args)
        .env_clear()
        .current_dir(&cwd)
        .env("HOME", home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake.display()))
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", home.join("tmp"))
        .env("FAKE_LOG", home.join("fake.log"))
        .env("FAKE_POSTS", home.join("posts.log"))
        .env("FAKE_PANE_PID", std::process::id().to_string())
        // Nunca el cc-notifyd real (4778): el Rust apunta al falso.
        .env("COMANDOS_NOTIFYD_URL", notifyd_url)
        .envs(s.env.iter().copied());
    if s.tmux {
        command.env("TMUX_PANE", "%7");
    }
    if s.curl_fails {
        command.env("FAKE_CURL_FAIL", "1");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let payload = s.payload.map(|p| {
        fs::read_to_string(fixtures().join(format!("{p}.json")))
            .unwrap()
            .replace("@FIXTURES@", &fixtures().to_string_lossy())
    });
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.unwrap_or_default().as_bytes())
        .unwrap();
    child.wait().unwrap().code()
}

#[test]
fn claude_hook_matches_bash_for_all_fixtures() {
    let dir = base("parity");
    fake_bin(&dir.join("fakebin"));
    let (port, _server, bodies) = fake_notifyd::spawn(0);
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    for s in scenarios() {
        let start_ms = now_ms();
        let seed_now = now();
        let h_bash = prepare_home(&dir, &s, "bash", seed_now);
        let h_rust = prepare_home(&dir, &s, "rust", seed_now);
        let code_bash = run_side(&dir, &s, "bash", &h_bash, "http://127.0.0.1:9");
        let bash_posts = lines(&h_bash.join("posts.log"));
        for url in lines(&h_bash.join("posts.log.url")) {
            assert_eq!(url, "http://127.0.0.1:4778/notify", "{}", s.name);
        }
        bodies.lock().unwrap().clear();
        let url = format!(
            "http://127.0.0.1:{}",
            if s.curl_fails { closed } else { port }
        );
        let code_rust = run_side(&dir, &s, "rust", &h_rust, &url);
        wait_for_delivery(&h_rust);
        let window = Window {
            start_ms,
            end_ms: now_ms(),
        };
        let rust_posts = if s.curl_fails {
            Vec::new()
        } else {
            std::mem::take(&mut *bodies.lock().unwrap())
        };
        let bash = collect(
            &h_bash,
            code_bash,
            if s.curl_fails { Vec::new() } else { bash_posts },
            window,
        );
        let rust = collect(&h_rust, code_rust, rust_posts, window);
        assert_eq!(bash.code, Some(0), "{}", s.name);
        assert_eq!(bash.state, rust.state, "estado difiere en {}", s.name);
        assert_eq!(
            bash.events, rust.events,
            "events.jsonl difiere en {}",
            s.name
        );
        assert_eq!(
            bash.events_mode, rust.events_mode,
            "modo de events.jsonl en {}",
            s.name
        );
        assert_eq!(
            bash.posts, rust.posts,
            "POST a cc-notifyd difiere en {}",
            s.name
        );
        assert_eq!(
            bash.log, rust.log,
            "binarios externos difieren en {}",
            s.name
        );
        assert_eq!(bash.intake, rust.intake, "evento N1 difiere en {}", s.name);
        assert_eq!(bash.usage, rust.usage, "base de uso difiere en {}", s.name);
        assert_eq!(bash, rust, "{}", s.name);
        let _ = fs::remove_dir_all(&h_bash);
        let _ = fs::remove_dir_all(&h_rust);
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn truncated_payload_writes_nothing() {
    let dir = base("truncated");
    let fake = dir.join("fakebin");
    fake_bin(&fake);
    let s = Scenario {
        name: "truncated",
        ..Default::default()
    };
    let home = prepare_home(&dir, &s, "rust", now());
    let mut child = Command::new(env!("CARGO_BIN_EXE_comandos-hook"))
        .arg("claude")
        .env_clear()
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake.display()))
        .env("FAKE_LOG", home.join("fake.log"))
        .env("COMANDOS_NOTIFYD_URL", "http://127.0.0.1:9")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"hook_event_name":"Stop","session_id":"abc","transcript_pa"#)
        .unwrap();
    assert!(child.wait().unwrap().success());
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        fs::read_dir(home.join(".claude/hooks/state"))
            .unwrap()
            .count(),
        0
    );
    assert!(!home.join(".claude/hooks/events.jsonl").exists());
    assert!(!home.join("fake.log").exists());
    assert!(!home.join(".local/state").exists());
    assert!(!home.join(".claude/hooks/comandos-usage.sqlite").exists());
    let _ = fs::remove_dir_all(&dir);
}
