//! Paridad de los dos adaptadores que eran Python: `comandos hook grok` contra
//! `tests/fixtures/hooks/oracle/adapters/grok-hooks.py` (copia del original) (normalización y `--accept`) y `comandos hook agy-status`
//! contra `…/oracle/adapters/agy-statusline.py`. `python3` solo corre como oráculo, con un
//! entorno controlado y `HOME` temporales.
#[allow(dead_code)]
#[path = "support/parity.rs"]
mod parity;

use parity::{Window, normalize, read_lossy};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hooks/adapters")
        .join(format!("{name}.json"));
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// (código, stdout, stderr vacío o no).
fn run(
    oracle: bool,
    script: &str,
    harness: &str,
    args: &[&str],
    home: &Path,
    stdin: &[u8],
) -> (Option<i32>, String) {
    let mut command = if oracle {
        let mut c = Command::new("python3");
        c.arg(root().join(script));
        c
    } else {
        let mut c = Command::new(env!("CARGO_BIN_EXE_comandos-hook"));
        c.arg(harness);
        c
    };
    let mut child = command
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        // Un locale UTF-8 normal (como el del usuario): con `C.UTF-8` Python lee
        // stdin con `surrogateescape` y aceptaría bytes que en producción rechaza.
        .env("LANG", "en_US.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn grok(oracle: bool, args: &[&str], home: &Path, stdin: &[u8]) -> (Option<i32>, String) {
    run(
        oracle,
        "crates/comandos-runtime/tests/fixtures/hooks/oracle/adapters/grok-hooks.py",
        "grok",
        args,
        home,
        stdin,
    )
}

#[test]
fn grok_normalize_matches_python() {
    let home = std::env::temp_dir().join(format!("comandos-grok-{}", std::process::id()));
    fs::create_dir_all(&home).unwrap();
    let fx = fixture("grok_events");
    let mut inputs: Vec<Vec<u8>> = fx["normalize"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| serde_json::to_vec(p).unwrap())
        .collect();
    inputs.extend(
        fx["raw"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap().as_bytes().to_vec()),
    );
    inputs.push(b"{\"hookEventName\":\"Stop\",\"lastAssistantMessage\":\"\xff\"}".to_vec());
    for input in inputs {
        let python = grok(true, &[], &home, &input);
        let rust = grok(false, &[], &home, &input);
        assert_eq!(python, rust, "{}", String::from_utf8_lossy(&input));
    }
    // Con argumentos que no son `--accept RUTA` se normaliza igual.
    let input = br#"{"hookEventName":"Stop","reason":"x"}"#;
    assert_eq!(
        grok(true, &["--otro"], &home, input),
        grok(false, &["--otro"], &home, input)
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn grok_accept_matches_python() {
    let dir = std::env::temp_dir().join(format!("comandos-grok-accept-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let fx = fixture("grok_events");
    let sequences = fx["accept"].as_array().unwrap();
    // Archivos de partida: ausente, vacío, basura, lista y un evento con `event` lista.
    let seeds: [Option<&[u8]>; 5] = [
        None,
        Some(b""),
        Some(b"no json"),
        Some(b"[1]"),
        Some(br#"{"event":"Stop","sessionId":"s1","promptId":"pA"}"#),
    ];
    for (n, seed) in seeds.iter().enumerate() {
        for (m, sequence) in sequences.iter().enumerate() {
            let paths: Vec<PathBuf> = ["py", "rs"]
                .iter()
                .map(|side| dir.join(format!("{n}-{m}-{side}.grok")))
                .collect();
            for path in &paths {
                let _ = fs::remove_file(path);
                if let Some(seed) = seed {
                    fs::write(path, seed).unwrap();
                }
            }
            for candidate in sequence.as_array().unwrap() {
                let stdin = serde_json::to_vec(candidate).unwrap();
                let results: Vec<_> = paths
                    .iter()
                    .zip([true, false])
                    .map(|(path, oracle)| {
                        let path = path.to_str().unwrap();
                        let result = grok(oracle, &["--accept", path], &dir, &stdin);
                        let mode = fs::metadata(path)
                            .map(|m| m.permissions().mode() & 0o777)
                            .ok();
                        (result, read_lossy(Path::new(path)), mode)
                    })
                    .collect();
                assert_eq!(results[0], results[1], "semilla {n}, candidato {candidate}");
            }
        }
    }
    // `--accept` con stdin que no es objeto: 0 y el archivo intacto.
    for stdin in [&b"[1]"[..], b"basura", b""] {
        let path = dir.join("x.grok");
        let path = path.to_str().unwrap();
        assert_eq!(
            grok(true, &["--accept", path], &dir, stdin),
            grok(false, &["--accept", path], &dir, stdin)
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Lo observable de agy-status: el directorio `hooks` entero.
fn agy_files(home: &Path, window: Window) -> Vec<(String, u32, String)> {
    let hooks = home.join(".claude/hooks");
    let mut files: Vec<(String, u32, String)> = fs::read_dir(&hooks)
        .map(|entries| {
            entries
                .map(|e| {
                    let path = e.unwrap().path();
                    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
                    let name = path.file_name().unwrap().to_string_lossy().into_owned();
                    (name, mode, normalize(&read_lossy(&path), window))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

#[test]
fn agy_status_matches_python() {
    let dir = std::env::temp_dir().join(format!("comandos-agy-status-{}", std::process::id()));
    let fx = fixture("agy_statusline");
    let real = serde_json::to_vec(&fx["real"]).unwrap();
    let saved = |captured: &str| {
        format!(
            r#"{{"plan_tier": "Google AI Pro", "quota": {{"3p-5h": {{"remaining_fraction": 1.0, "reset_time": "2026-10-04T08:12:22Z"}}, "3p-weekly": {{"remaining_fraction": 1, "reset_time": "2026-10-11T03:12:22Z"}}, "gemini-5h": {{"remaining_fraction": 0.9725484, "reset_time": "2026-10-04T06:28:13Z"}}, "gemini-weekly": {{"remaining_fraction": 0.59870625, "reset_time": "2026-10-07T17:21:23Z"}}}}, "captured_at": {captured}}}"#
        )
    };
    let fresh = (now() - 10).to_string();
    let stale = (now() - 100).to_string();
    let edge = (now() - 55).to_string();
    // (nombre, stdin, archivo previo).
    let mut cases: Vec<(String, Vec<u8>, Option<String>)> = vec![
        ("real".into(), real.clone(), None),
        ("sin_cambios".into(), real.clone(), Some(saved(&fresh))),
        ("viejo".into(), real.clone(), Some(saved(&stale))),
        ("borde_55s".into(), real.clone(), Some(saved(&edge))),
        ("captured_cadena".into(), real.clone(), Some(saved("\"x\""))),
        (
            "captured_falta".into(),
            real.clone(),
            Some(saved("0").replace(", \"captured_at\": 0", "")),
        ),
        ("previo_lista".into(), real.clone(), Some("[1]".into())),
        ("previo_roto".into(), real.clone(), Some("{".into())),
        ("vacio".into(), Vec::new(), None),
        ("espacios".into(), b"  \n".to_vec(), None),
        ("no_json".into(), b"nada".to_vec(), None),
        (
            "utf8_roto".into(),
            b"{\"quota\":{\"a\":{\"remaining_fraction\":1,\"reset_time\":\"\xff\"}}}".to_vec(),
            None,
        ),
    ];
    for key in ["edge", "no_buckets", "no_quota", "list"] {
        cases.push((key.into(), serde_json::to_vec(&fx[key]).unwrap(), None));
    }
    for (name, stdin, previous) in cases {
        let start_ms = now() * 1000;
        let homes: Vec<PathBuf> = ["py", "rs"]
            .iter()
            .map(|s| dir.join(format!("{name}-{s}")))
            .collect();
        for home in &homes {
            let _ = fs::remove_dir_all(home);
            if let Some(previous) = &previous {
                fs::create_dir_all(home.join(".claude/hooks")).unwrap();
                fs::write(home.join(".claude/hooks/agy-quota.json"), previous).unwrap();
            }
        }
        let agy = |oracle, home| {
            run(
                oracle,
                "crates/comandos-runtime/tests/fixtures/hooks/oracle/adapters/agy-statusline.py",
                "agy-status",
                &[],
                home,
                &stdin,
            )
        };
        let (python, rust) = (agy(true, &homes[0]), agy(false, &homes[1]));
        let window = Window {
            start_ms,
            end_ms: now() * 1000 + 999,
        };
        assert_eq!(python, rust, "{name}");
        assert_eq!(rust.1, "", "agy-status no imprime nada ({name})");
        assert_eq!(
            agy_files(&homes[0], window),
            agy_files(&homes[1], window),
            "{name}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}
