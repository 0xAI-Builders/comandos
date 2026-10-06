//! `comandos hook agy [EVENTO]`: transcripción de `adapters/agy-hooks.sh` (hooks
//! planos de Antigravity CLI en `~/.gemini/config/hooks.json`). El payload es
//! camelCase y no trae el evento (llega como argumento). Registra el proceso `agy`
//! antecesor en `native-processes/<pid>.json` (el bloque `python3 -c` del bash),
//! entrega el evento al pipeline de `hook claude` y responde `{}` (no intervenir).
use super::adapter::{arg, jq_get, jq_values, notify, proc_stat, pwd};
use super::input::{clock, env_bytes};
use super::py;
use super::state_file::mktemp;
use serde_json::Value;
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

pub fn run(args: &[String]) -> i32 {
    let mut raw = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut raw);
    if std::env::var_os("COMANDOS_SILENT_AGENT").is_some_and(|v| v == "1") {
        println!("{{}}");
        return 0;
    }
    let event = args
        .first()
        .filter(|e| !e.is_empty())
        .map_or("done", String::as_str);
    let home = PathBuf::from(OsStr::from_bytes(&env_bytes("HOME")));
    let _ = record_process(&home, super::text::strip_nl(&raw), event);
    let values = jq_values(&raw);
    let mut cwd = jq_get(&values, |v| {
        let paths = super::adapter::get(v, "workspacePaths")?;
        let first = match paths {
            Value::Array(items) => items.first().unwrap_or(&Value::Null),
            Value::Null => &Value::Null,
            _ => return Err(()),
        };
        Ok(Some(if super::adapter::truthy(first) {
            first.clone()
        } else {
            Value::String(String::new())
        }))
    });
    if cwd.is_empty() {
        cwd = pwd();
    }
    // El bash lo lanzaba en segundo plano y callado; aquí corre en proceso.
    notify(&[
        arg("--agent"),
        arg("agy"),
        arg("--event"),
        arg(event),
        arg("--cwd"),
        cwd,
    ]);
    println!("{{}}");
    0
}

fn sid_ok(s: &str) -> bool {
    (1..=256).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn model_ok(s: &str) -> bool {
    (1..=200).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:/-".contains(&b))
}

/// `Path(os.fsdecode(argv[0])).name`.
fn path_name(argv0: &[u8]) -> &[u8] {
    let trimmed = &argv0[..argv0.iter().rposition(|&b| b != b'/').map_or(0, |i| i + 1)];
    let start = trimmed
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i + 1);
    &trimmed[start..]
}

/// El bloque de Python: liga la observación a un `agy` antecesor real (como mucho
/// diez niveles por encima de este proceso), nunca a la última sesión de la carpeta.
fn record_process(home: &Path, raw: &[u8], event: &str) -> Option<()> {
    let data = py::json_load(raw)?;
    let data = data.as_object()?;
    let sid = data
        .get("conversationId")
        .map_or(Some(""), Value::as_str)?
        .to_string();
    if !sid_ok(&sid) {
        return None;
    }
    // El `python3` del bash empezaba por su padre: el propio proceso del hook.
    let mut pid = std::process::id();
    let mut found = None;
    for _ in 0..10 {
        #[cfg(target_os = "macos")]
        let argv = {
            let args = crate::agent_procs::proc_cmdline(Path::new("/proc"), i64::from(pid));
            args.first()?.as_bytes().to_vec()
        };
        #[cfg(not(target_os = "macos"))]
        let argv = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let (parent, start) = proc_stat(pid)?;
        let argv0 = argv.split(|&b| b == 0).next().unwrap_or_default();
        if path_name(argv0) == b"agy" {
            found = Some(start);
            break;
        }
        pid = parent;
    }
    let start = found?;
    let mut text = format!(
        "{{\"pid\": {pid}, \"start\": \"{start}\", \"harness\": \"agy\", \"sessionId\": \"{sid}\", \"parentId\": \"\", \"busy\": {}, \"updatedAt\": {}",
        event == "working",
        clock().1
    );
    if let Some(model) = data.get("modelName").and_then(Value::as_str)
        && model_ok(model)
    {
        text.push_str(&format!(", \"model\": \"{model}\""));
    }
    text.push('}');
    let root = home.join(".claude/hooks/native-processes");
    if let Some(parent) = root.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    match std::fs::DirBuilder::new().mode(0o700).create(&root) {
        Ok(()) => {}
        Err(_) if root.is_dir() => {}
        Err(_) => return None,
    }
    let (temp, mut file) = mktemp(&root, b".agy-", 8, "")?;
    file.write_all(text.as_bytes()).ok()?;
    drop(file);
    std::fs::rename(&temp, root.join(format!("{pid}.json"))).ok()
}
