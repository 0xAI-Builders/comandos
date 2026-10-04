//! Paridad de `comandos hook claude-usage` contra `hooks/cc-usage-tool.sh` (filas de
//! la base de uso) y de `comandos hook claude-status` contra `hooks/cc-status.sh`
//! (stdout byte a byte, que tmux pinta tal cual, y la caché). `HOME` y
//! `XDG_RUNTIME_DIR` temporales; tmux falso.
#[allow(dead_code)]
#[path = "support/parity.rs"]
mod parity;

use parity::{Window, dump_db, fake_bin};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// tmux falso con la sesión de `FAKE_SESSION` (por omisión `fake-sess`).
fn fakebin(dir: &Path) -> PathBuf {
    let fake = dir.join("fakebin");
    fake_bin(&fake);
    let tmux = fake.join("tmux");
    fs::write(
        &tmux,
        "#!/bin/sh\nprintf '%s\\n' \"tmux $*\" >> \"$FAKE_LOG\"\ncase \"$*\" in *'#S'*) printf '%s\\n' \"${FAKE_SESSION-fake-sess}\" ;; esac\n",
    )
    .unwrap();
    fs::set_permissions(&tmux, fs::Permissions::from_mode(0o755)).unwrap();
    fake
}

fn run(
    script: &str,
    harness: &str,
    side: &str,
    home: &Path,
    fake: &Path,
    env: &[(&str, &str)],
    stdin: &[u8],
) -> (Option<i32>, Vec<u8>) {
    let mut command = if side == "bash" {
        let mut c = Command::new("bash");
        c.arg("-c")
            .arg("trap wait EXIT; . \"$0\" \"$@\"")
            .arg(root().join(script));
        c
    } else {
        let mut c = Command::new(env!("CARGO_BIN_EXE_comandos-hook"));
        c.arg(harness);
        c
    };
    let mut child = command
        .env_clear()
        .current_dir(home)
        .env("HOME", home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake.display()))
        .env("LANG", "C.UTF-8")
        .env("FAKE_LOG", home.join("fake.log"))
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code(), out.stdout)
}

/// Base de uso con una interacción abierta en `fake-sess`/`%7` (la que el hook claude
/// deja con `working`), hecha con el `cc_usage.py` real y copiada a cada `HOME`.
fn seed_usage(dir: &Path) -> PathBuf {
    let seed = dir.join("seed");
    fs::create_dir_all(seed.join(".claude/hooks")).unwrap();
    let payload = serde_json::json!({"status":"working","harness":"claude","tmux_session":"fake-sess",
        "tmux_pane":"%7","prompt_id":"p1","agent_session_id":"s1","source":"hook:claude",
        "confidence":"exact","at_ms":now_ms()});
    let mut child = Command::new("python3")
        .arg(root().join("bin/cc_usage.py"))
        .arg("lifecycle")
        .env_clear()
        .env("HOME", &seed)
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    seed.join(".claude/hooks/comandos-usage.sqlite")
}

/// La duración de una herramienta (fin − inicio) depende de cuándo corrió cada
/// lado; los dos instantes ya se normalizan como marcas de tiempo.
fn without_duration(row: String) -> String {
    match row.strip_prefix("  usage_tool_calls: ") {
        Some(cells) => {
            let mut cells: Vec<&str> = cells.split(" | ").collect();
            if cells.len() > 7 && cells[7].parse::<i64>().is_ok() {
                cells[7] = "D";
            }
            format!("  usage_tool_calls: {}", cells.join(" | "))
        }
        None => row,
    }
}

#[test]
fn claude_usage_matches_cc_usage_tool() {
    let dir = std::env::temp_dir().join(format!("comandos-usage-tool-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let fake = fakebin(&dir);
    let seed = seed_usage(&dir);
    let fx: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/hooks/adapters/claude_usage.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let doc = |key: &str| serde_json::to_vec(&fx[key]).unwrap();
    let mut events: Vec<Vec<u8>> = [
        "pre_bash",
        "post_bash",
        "pre_skill",
        "post_skill_error",
        "pre_noid",
        "post_noid_error",
        "pre_numeric",
        "post_string_response",
        "stop",
        "empty_tool",
        "skill_bad",
    ]
    .iter()
    .map(|k| doc(k))
    .collect();
    // Dos documentos: `cc_usage.py` lee uno solo y falla; no se registra ninguno.
    let fresh =
        br#"{"hook_event_name":"PreToolUse","tool_name":"Write","tool_use_id":"toolu_multi"}"#;
    events.push([&fresh[..], b"\n", &doc("pre_skill")].concat());
    events.push(b"{roto".to_vec());
    let scenarios: [(&str, Vec<(&str, &str)>); 5] = [
        ("pane", vec![("TMUX_PANE", "%7")]),
        ("sin_pane", vec![]),
        ("pane_raro", vec![("TMUX_PANE", "%7\n")]),
        (
            "sesion_mala",
            vec![("TMUX_PANE", "%7"), ("FAKE_SESSION", "bad sess")],
        ),
        (
            "sesion_vacia",
            vec![("TMUX_PANE", "%7"), ("FAKE_SESSION", "")],
        ),
    ];
    for (name, env) in scenarios {
        let start_ms = now_ms();
        let homes: Vec<PathBuf> = ["bash", "rust"]
            .iter()
            .map(|s| dir.join(format!("{name}-{s}")))
            .collect();
        for home in &homes {
            fs::create_dir_all(home.join(".claude/hooks")).unwrap();
            fs::copy(&seed, home.join(".claude/hooks/comandos-usage.sqlite")).unwrap();
        }
        fs::create_dir_all(homes[0].join(".local/bin")).unwrap();
        std::os::unix::fs::symlink(
            root().join("bin/cc_usage.py"),
            homes[0].join(".local/bin/cc_usage.py"),
        )
        .unwrap();
        for event in &events {
            let outs: Vec<_> = ["bash", "rust"]
                .iter()
                .zip(&homes)
                .map(|(side, home)| {
                    run(
                        "hooks/cc-usage-tool.sh",
                        "claude-usage",
                        side,
                        home,
                        &fake,
                        &env,
                        event,
                    )
                })
                .collect();
            assert_eq!(
                outs[0],
                outs[1],
                "{name}: {}",
                String::from_utf8_lossy(event)
            );
            let window = Window {
                start_ms,
                end_ms: now_ms(),
            };
            let dumps: Vec<Vec<String>> = homes
                .iter()
                .map(|h| {
                    dump_db(&h.join(".claude/hooks/comandos-usage.sqlite"), window)
                        .into_iter()
                        .map(without_duration)
                        .collect()
                })
                .collect();
            assert_eq!(
                dumps[0],
                dumps[1],
                "{name}: {}",
                String::from_utf8_lossy(event)
            );
        }
        let logs: Vec<String> = homes
            .iter()
            .map(|h| parity::read_lossy(&h.join("fake.log")))
            .collect();
        assert_eq!(logs[0], logs[1], "{name}: llamadas a tmux");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// (nombre, archivos de estado, caché previa con su antigüedad en s).
type StatusScenario<'a> = (&'a str, &'a [(&'a str, String)], Option<(&'a str, u64)>);

/// (stdout, código, archivos del directorio de la caché con modo y contenido).
type StatusRun = (Vec<u8>, Option<i32>, Vec<(String, u32, Vec<u8>)>);

fn status_side(
    dir: &Path,
    name: &str,
    side: &str,
    states: &[(&str, String)],
    cache: Option<(&str, u64)>,
) -> StatusRun {
    let home = dir.join(format!("{name}-{side}"));
    let runtime = home.join("run");
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&runtime).unwrap();
    let state = home.join(".claude/hooks/state");
    if !states.is_empty() {
        fs::create_dir_all(&state).unwrap();
    }
    for (file, content) in states {
        match content.strip_prefix("@LINK@") {
            Some(target) => std::os::unix::fs::symlink(target, state.join(file)).unwrap(),
            None if content == "@DIR@" => fs::create_dir_all(state.join(file)).unwrap(),
            None => fs::write(state.join(file), content).unwrap(),
        }
    }
    if let Some((content, age)) = cache {
        let path = runtime.join("cc-status.cache");
        fs::write(&path, content).unwrap();
        let file = fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(age))
            .unwrap();
    }
    let fake = dir.join("fakebin");
    let runtime_env = runtime.to_string_lossy().into_owned();
    let (code, stdout) = run(
        "hooks/cc-status.sh",
        "claude-status",
        side,
        &home,
        &fake,
        &[("XDG_RUNTIME_DIR", &runtime_env)],
        b"",
    );
    let mut files: Vec<(String, u32, Vec<u8>)> = fs::read_dir(&runtime)
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                mode,
                fs::read(&path).unwrap(),
            )
        })
        .collect();
    files.sort();
    (stdout, code, files)
}

#[test]
fn claude_status_matches_cc_status() {
    let dir = std::env::temp_dir().join(format!("comandos-cc-status-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fakebin(&dir);
    let now = now_ms() / 1000;
    let st = |project: &str, status: &str, age: i64| {
        format!(
            "{{\n  \"project\": {project:?},\n  \"status\": {status:?},\n  \"ts\": {}\n}}\n",
            now - age
        )
    };
    let raw = |text: &str| text.replace("@NOW@", &now.to_string());
    let mixed: Vec<(&str, String)> = vec![
        ("a--s--1.json", st("alfa", "waiting", 10)),
        ("b--s--2.json", st("beta", "done", 20)),
        ("c.json", st("gama delta", "waiting", 30)),
        ("d.json", st("viejo", "done", 30000)),
        ("e.json", st("trabajando", "working", 5)),
        (
            "f.json",
            raw(r#"{"project":"tab\\tbarra\\\\y\\101","status":"done","ts":@NOW@}"#),
        ),
        (
            "g.json",
            raw(r#"{"project":"con|tubo","status":"done","ts":@NOW@}"#),
        ),
        ("h.json", raw(r#"{"project":"sin-ts","status":"done"}"#)),
        (
            "i.json",
            raw(r#"{"project":"ts-cadena","status":"waiting","ts":"@NOW@"}"#),
        ),
        ("j.json", "no es json\n".into()),
        ("k.json", "[1,2]\n".into()),
        (
            "l.json",
            raw(r#"{"project":42,"status":"done","ts":@NOW@} "#),
        ),
        (
            "m.json",
            raw(r#"{"project":"","status":"done","ts":@NOW@}"#),
        ),
        (
            "n.json",
            raw(r#"{"project":"ñandú😀","status":"done","ts":@NOW@}"#),
        ),
        (
            "o.json",
            raw(r#"{"project":"ts-now","status":"waiting","ts":"now"}"#),
        ),
        ("p.json", "@DIR@".into()),
        (".oculto.json", st("oculto", "done", 1)),
        ("q.txt", st("no-json", "done", 1)),
    ];
    let abort: Vec<(&str, String)> = vec![
        ("a.json", st("antes", "done", 1)),
        (
            "b.json",
            raw(r#"{"project":"float","status":"done","ts":@NOW@.5}"#),
        ),
        ("c.json", st("despues", "waiting", 1)),
    ];
    let many: Vec<(&str, String)> = (0..6)
        .map(|i| {
            (
                ["1.json", "2.json", "3.json", "4.json", "5.json", "6.json"][i],
                st(
                    &format!("p{i}"),
                    if i % 2 == 0 { "waiting" } else { "done" },
                    1,
                ),
            )
        })
        .collect();
    let dangling: Vec<(&str, String)> = vec![
        ("0.json", "@LINK@/no/existe".into()),
        ("a.json", st("alfa", "done", 1)),
    ];
    let only_old: Vec<(&str, String)> = vec![("a.json", st("viejo", "waiting", 28801))];
    let empty_dir: Vec<(&str, String)> = vec![("leeme.txt", "x".into())];
    let scenarios: Vec<StatusScenario> = vec![
        ("mixto", &mixed, None),
        ("aborta", &abort, None),
        ("muchos", &many, None),
        ("colgante", &dangling, None),
        ("solo_viejos", &only_old, None),
        ("sin_estado", &[], None),
        ("estado_vacio", &empty_dir, None),
        ("cache_fresca", &mixed, Some(("CACHÉ\n", 0))),
        ("cache_vieja", &mixed, Some(("CACHÉ\n", 30))),
    ];
    for (name, states, cache) in scenarios {
        let bash = status_side(&dir, name, "bash", states, cache);
        let rust = status_side(&dir, name, "rust", states, cache);
        assert_eq!(
            String::from_utf8_lossy(&bash.0),
            String::from_utf8_lossy(&rust.0),
            "stdout de {name}"
        );
        assert_eq!(bash, rust, "{name}");
    }
    let _ = fs::remove_dir_all(&dir);
}
