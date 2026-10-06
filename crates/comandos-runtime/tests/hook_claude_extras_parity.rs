//! Paridad de `comandos hook claude-usage` contra `hooks/cc-usage-tool.sh` (filas de
//! la base de uso) y de `comandos hook claude-status` contra `hooks/cc-status.sh`
//! (stdout byte a byte, que tmux pinta tal cual, y la caché). `HOME` y
//! `XDG_RUNTIME_DIR` temporales; tmux falso.
#[allow(dead_code)]
#[path = "support/parity.rs"]
mod parity;

use parity::{Window, dump_db, fake_bin, wait_for_delivery};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

/// Escribe la entrada del hijo y cierra su stdin. Un hijo que sale sin leerla
/// da `EPIPE`: no es un fallo de la prueba (lo que cuenta es su salida).
fn feed_stdin(child: &mut std::process::Child, input: &[u8]) {
    let mut stdin = child.stdin.take().unwrap();
    match stdin.write_all(input) {
        Err(err) if err.kind() != std::io::ErrorKind::BrokenPipe => panic!("stdin: {err}"),
        _ => {}
    }
}

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
    feed_stdin(&mut child, stdin);
    let out = child.wait_with_output().unwrap();
    (out.status.code(), out.stdout)
}

type UsageObservation = ((Option<i32>, Vec<u8>), Vec<String>, String);

fn usage_reference(
    home: &Path,
    fake: &Path,
    env: &[(&str, &str)],
    events: &[Vec<u8>],
    seed: &[String],
    start_ms: i64,
) -> UsageObservation {
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle-src/hooks/cc-usage-tool.sh");
    let bytes = comandos_oracle::oracle_at(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        "runtime-shell-claude-usage",
        &serde_json::json!({"provenance":include_str!("oracle-src/hooks/USAGE_PROVENANCE.json"),
            "python_provenance":include_str!("oracle-src/usage-seed/PROVENANCE.json"),
            "seed":seed,"env":env,"event_history":events}),
        || {
            let output = run(
                source.to_str().unwrap(),
                "claude-usage",
                "bash",
                home,
                fake,
                env,
                events.last().unwrap(),
            );
            let rows: Vec<String> = dump_db(
                &home.join(".claude/hooks/comandos-usage.sqlite"),
                Window {
                    start_ms,
                    end_ms: now_ms(),
                },
            )
            .into_iter()
            .map(without_duration)
            .collect();
            let transcript = parity::read_lossy(&home.join("fake.log"));
            serde_json::to_vec(&(output, rows, transcript)).map_err(|e| e.to_string())
        },
    );
    serde_json::from_slice(&bytes).unwrap()
}

/// Base de uso con una interacción abierta en `fake-sess`/`%7` (la que el hook claude
/// deja con `working`), hecha con el `cc_usage.py` real y copiada a cada `HOME`.
fn seed_usage(dir: &Path) -> PathBuf {
    let seed = dir.join("seed");
    fs::create_dir_all(seed.join(".claude/hooks")).unwrap();
    let payload = serde_json::json!({"status":"working","harness":"claude","tmux_session":"fake-sess",
        "tmux_pane":"%7","prompt_id":"p1","agent_session_id":"s1","source":"hook:claude",
        "confidence":"exact","at_ms":now_ms()});
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle-src/usage-seed/cc_usage.py");
    let roots = [("<HOME>", seed.as_path()), ("<SOURCE>", source.as_path())];
    let at_ms = payload["at_ms"].as_i64().unwrap();
    let mut input = payload.clone();
    input["at_ms"] = serde_json::json!("<SEED_MS>");
    let before = comandos_oracle::snapshot_tree(&seed, &roots).unwrap();
    let bytes = comandos_oracle::oracle_at(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
        "runtime-hook-usage-seed",
        &serde_json::json!({"provenance":include_str!("oracle-src/usage-seed/PROVENANCE.json"),"payload":input,"before":before}),
        || {
            let mut child = Command::new(
                std::env::var("COMANDOS_RUNTIME_ORACLE_PYTHON")
                    .unwrap_or_else(|_| "python3".into()),
            )
            .arg(&source)
            .arg("lifecycle")
            .env_clear()
            .env("HOME", &seed)
            .env("PATH", "/usr/bin:/bin")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
            feed_stdin(&mut child, payload.to_string().as_bytes());
            if !child.wait().map_err(|e| e.to_string())?.success() {
                return Err("original usage lifecycle failed".into());
            }
            let mut tree = comandos_oracle::snapshot_tree(&seed, &roots)?;
            seed_clock(&mut tree, at_ms, false);
            serde_json::to_vec(&tree).map_err(|e| e.to_string())
        },
    );
    if !matches!(
        std::env::var("COMANDOS_ORACLE").as_deref(),
        Ok("record" | "check")
    ) {
        let mut tree = serde_json::from_slice(&bytes).unwrap();
        seed_clock(&mut tree, at_ms, true);
        comandos_oracle::restore_tree(&seed, &tree, &roots).unwrap();
    }
    seed.join(".claude/hooks/comandos-usage.sqlite")
}

/// The original seed has one interaction. Normalize only its two fixture-clock
/// columns; replay restores their live anchor, retaining the original age contract.
fn seed_clock(tree: &mut serde_json::Value, at_ms: i64, restore: bool) {
    let rows = tree[".claude/hooks/comandos-usage.sqlite"]["db"]["tables"]["usage_interactions"]
        .as_array_mut()
        .unwrap();
    assert_eq!(rows.len(), 1);
    for (index, token, value) in [
        (7, "<SEED_MS>", at_ms),
        (15, "<SEED_SECONDS>", at_ms / 1000),
    ] {
        let cell = &mut rows[0][index];
        assert_eq!(cell[0], "integer");
        if restore {
            assert_eq!(cell[1], token);
            cell[1] = serde_json::json!(value);
        } else {
            assert_eq!(cell[1], value);
            cell[1] = serde_json::json!(token);
        }
    }
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
    let seed_at: i64 = rusqlite::Connection::open(&seed)
        .unwrap()
        .query_row("select started_at_ms from usage_interactions", [], |r| {
            r.get(0)
        })
        .unwrap();
    let seed_dump = dump_db(
        &seed,
        Window {
            start_ms: seed_at,
            end_ms: now_ms(),
        },
    );
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
        let start_ms = seed_at;
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
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle-src/usage-seed/cc_usage.py"),
            homes[0].join(".local/bin/cc_usage.py"),
        )
        .unwrap();
        let mut expected_trace = String::new();
        for (index, event) in events.iter().enumerate() {
            let (expected, expected_rows, transcript) = usage_reference(
                &homes[0],
                &fake,
                &env,
                &events[..=index],
                &seed_dump,
                start_ms,
            );
            expected_trace = transcript;
            let actual = run("", "claude-usage", "rust", &homes[1], &fake, &env, event);
            // The actual native detached delivery and SQLite queries still run.
            wait_for_delivery(&homes[1]);
            assert_eq!(
                expected,
                actual,
                "{name}: {}",
                String::from_utf8_lossy(event)
            );
            let actual_rows: Vec<String> = dump_db(
                &homes[1].join(".claude/hooks/comandos-usage.sqlite"),
                Window {
                    start_ms,
                    end_ms: now_ms(),
                },
            )
            .into_iter()
            .map(without_duration)
            .collect();
            assert_eq!(
                expected_rows,
                actual_rows,
                "{name}: {}",
                String::from_utf8_lossy(event)
            );
        }
        assert_eq!(
            expected_trace,
            parity::read_lossy(&homes[1].join("fake.log")),
            "{name}: llamadas a tmux"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Un `PreToolUse` no espera a una base de uso ocupada: la escritura va al proceso
/// de entrega desacoplado (como el `python3 cc_usage.py tool-event &` del bash) y
/// llega en cuanto se libera el bloqueo.
#[test]
fn claude_usage_does_not_wait_for_a_busy_db() {
    let dir = std::env::temp_dir().join(format!("comandos-usage-busy-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let fake = fakebin(&dir);
    let seed = seed_usage(&dir);
    let home = dir.join("rust");
    fs::create_dir_all(home.join(".claude/hooks")).unwrap();
    let db = home.join(".claude/hooks/comandos-usage.sqlite");
    fs::copy(&seed, &db).unwrap();
    let lock = rusqlite::Connection::open(&db).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let event =
        br#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"toolu_busy"}"#;
    let started = std::time::Instant::now();
    let (code, _) = run(
        "",
        "claude-usage",
        "rust",
        &home,
        &fake,
        &[("TMUX_PANE", "%7")],
        event,
    );
    let elapsed = started.elapsed();
    assert_eq!(code, Some(0));
    assert!(
        elapsed < Duration::from_secs(2),
        "el hook esperó {elapsed:?}"
    );
    std::thread::sleep(Duration::from_millis(300));
    lock.execute_batch("ROLLBACK").unwrap();
    wait_for_delivery(&home);
    let rows: i64 = lock
        .query_row(
            "select count(*) from usage_tool_calls where tool_name = 'Bash'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1);
    let _ = fs::remove_dir_all(&dir);
}

/// (nombre, archivos de estado, caché previa con su antigüedad en s).
type StatusScenario<'a> = (
    &'a str,
    &'a [(&'a str, String)],
    Option<(&'a str, u64)>,
    &'a [(&'a str, &'a str)],
);

/// (stdout, código, archivos del directorio de la caché con modo y contenido).
type StatusRun = (Vec<u8>, Option<i32>, Vec<(String, u32, Vec<u8>)>);

fn status_side(
    dir: &Path,
    name: &str,
    side: &str,
    states: &[(&str, String)],
    cache: Option<(&str, u64)>,
    locale: &[(&str, &str)],
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
    let mut env = vec![("XDG_RUNTIME_DIR", runtime_env.as_str())];
    env.extend_from_slice(locale);
    let (code, stdout) = run(
        "hooks/cc-status.sh",
        "claude-status",
        side,
        &home,
        &fake,
        &env,
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
    // El glob de bash ordena por la colación del locale: en bytes `Zeta` < `alfa` <
    // `_b`, en `en_US`/`es_MX` `_b` < `alfa` < `éclair` < `Zeta`.
    let collation: Vec<(&str, String)> = vec![
        ("Zeta--s--1.json", st("Zeta", "waiting", 1)),
        ("alfa--s--2.json", st("alfa", "waiting", 1)),
        ("_b.json", st("guion", "done", 1)),
        ("éclair.json", st("eclair", "done", 1)),
        ("Abc.json", st("Abc", "waiting", 1)),
        ("abc.json", st("abc", "done", 1)),
    ];
    let en: &[(&str, &str)] = &[("LANG", "en_US.UTF-8")];
    let es: &[(&str, &str)] = &[("LANG", "es_MX.UTF-8")];
    let lc_all: &[(&str, &str)] = &[("LANG", "C.UTF-8"), ("LC_ALL", "en_US.UTF-8")];
    let lc_collate: &[(&str, &str)] = &[("LANG", "en_US.UTF-8"), ("LC_COLLATE", "C")];
    let scenarios: Vec<StatusScenario> = vec![
        ("mixto", &mixed, None, &[]),
        ("aborta", &abort, None, &[]),
        ("muchos", &many, None, &[]),
        ("colgante", &dangling, None, &[]),
        ("solo_viejos", &only_old, None, &[]),
        ("sin_estado", &[], None, &[]),
        ("estado_vacio", &empty_dir, None, &[]),
        ("cache_fresca", &mixed, Some(("CACHÉ\n", 0)), &[]),
        ("cache_vieja", &mixed, Some(("CACHÉ\n", 30)), &[]),
        ("colacion_en", &collation, None, en),
        ("colacion_es", &collation, None, es),
        ("colacion_lc_all", &collation, None, lc_all),
        ("colacion_lc_collate", &collation, None, lc_collate),
    ];
    for (name, states, cache, locale) in scenarios {
        let bash = status_side(&dir, name, "bash", states, cache, locale);
        let rust = status_side(&dir, name, "rust", states, cache, locale);
        assert_eq!(
            String::from_utf8_lossy(&bash.0),
            String::from_utf8_lossy(&rust.0),
            "stdout de {name}"
        );
        assert_eq!(bash, rust, "{name}");
    }
    let _ = fs::remove_dir_all(&dir);
}
