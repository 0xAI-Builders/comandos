//! `comandos hook claude-status`: transcripción de `hooks/cc-status.sh`, el resumen
//! de `status-right` de tmux (qué proyectos esperan y cuáles terminaron). Misma
//! caché de 20 s en `${XDG_RUNTIME_DIR:-/tmp}/cc-status.cache` y salida idéntica
//! byte a byte: tmux la pinta tal cual.
use super::bash::{arith, printf_b};
use super::input::env_bytes;
use super::jq::jq_join;
use super::text::{jq_lossy, strip_nl};
use serde_json::Value;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const WAIT_ICON: &str = "\u{f0f3}";
const DONE_ICON: &str = "\u{f00c}";

/// Los `*.json` (sin ocultos) del directorio, ordenados como el glob de bash: por
/// la colación del locale del entorno (`LC_ALL` → `LC_COLLATE` → `LANG`).
fn state_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<OsString> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.file_name()))
                .filter(|n| {
                    let b = n.as_bytes();
                    !b.starts_with(b".") && b.ends_with(b".json")
                })
                .collect()
        })
        .unwrap_or_default();
    let order = super::collate::GlobOrder::from_env(env_bytes);
    files.sort_by(|a, b| order.compare(a.as_bytes(), b.as_bytes()));
    files.into_iter().map(|n| dir.join(n)).collect()
}

/// El `jq -Rrjs` sobre los JSON separados por NUL: `proyecto|estado|ts` + NUL.
fn records(files: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::new();
    for file in files {
        if !file.is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(file) else {
            continue;
        };
        out.extend(record_bytes(bytes));
    }

    out
}

fn record_bytes(mut bytes: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    // `$(<archivo)`: sin NUL (bash los descarta) ni saltos de línea finales.
    bytes.retain(|&b| b != 0);
    let text = jq_lossy(strip_nl(&bytes));
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text) else {
        return out;
    };
    let ts = map
        .get("ts")
        .map_or_else(|| "null".to_string(), super::jq::jq_tostring);
    let fields = [
        map.get("project").cloned().unwrap_or(Value::Null),
        map.get("status").cloned().unwrap_or(Value::Null),
        Value::String(ts),
    ];
    out.extend_from_slice(jq_join(fields.iter(), "|").as_bytes());
    out.push(0);
    out
}

/// `short "$lista"`: hasta dos nombres y el resto contado (`a · b +3`).
fn short(list: &[u8]) -> Vec<u8> {
    let words: Vec<&[u8]> = list
        .split(|b| matches!(b, b' ' | b'\t' | b'\n'))
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = Vec::new();
    if let Some(first) = words.first() {
        out.extend_from_slice(first);
    }
    if let Some(second) = words.get(1) {
        out.extend_from_slice(" · ".as_bytes());
        out.extend_from_slice(second);
    }
    if words.len() > 2 {
        out.extend_from_slice(format!(" +{}", words.len() - 2).as_bytes());
    }
    out
}

fn mtime_age(cache: &Path, now: i64) -> Option<i64> {
    let meta = std::fs::metadata(cache).ok()?;
    meta.is_file().then(|| now - meta.mtime())
}

pub fn run(_args: &[String]) -> i32 {
    let home = PathBuf::from(OsStr::from_bytes(&env_bytes("HOME")));
    let state = home.join(".claude/hooks/state");
    #[cfg(target_os = "macos")]
    let runtime = match crate::platform::runtime_directory_from_env(true) {
        Ok(Some(path)) => path.as_os_str().as_bytes().to_vec(),
        _ => return 2,
    };
    #[cfg(not(target_os = "macos"))]
    let runtime = env_bytes("XDG_RUNTIME_DIR");
    let runtime = if runtime.is_empty() {
        b"/tmp".to_vec()
    } else {
        runtime
    };
    let mut cache_name = runtime;
    cache_name.extend_from_slice(b"/cc-status.cache");
    let cache = PathBuf::from(OsString::from_vec(cache_name));
    let now = super::input::clock().0;
    if mtime_age(&cache, now).is_some_and(|age| age < 20) {
        let _ = std::io::stdout().write_all(&std::fs::read(&cache).unwrap_or_default());
        return 0;
    }
    let access = match comandos_store::domains::caller::CallerAccess::open(&home, "session-status")
    {
        Ok(access) => access,
        Err(error) => {
            eprintln!("comandos hook claude-status: {error}");
            return 0;
        }
    };
    let stream = if matches!(
        access.mode(),
        comandos_store::unified::Mode::Unified | comandos_store::unified::Mode::Sealed
    ) {
        let Some(db) = access.db() else { return 0 };
        let found = (|| -> rusqlite::Result<Vec<(String, Vec<u8>)>> {
            db.prepare("SELECT file_key,body FROM session_status WHERE file_key NOT LIKE '.%' AND substr(file_key,-5)='.json'")?
                .query_map([],|r|Ok((r.get(0)?,r.get(1)?)))?.collect()
        })();
        let Ok(mut found) = found else { return 0 };
        let order = super::collate::GlobOrder::from_env(env_bytes);
        found.sort_by(|a, b| order.compare(a.0.as_bytes(), b.0.as_bytes()));
        let mut stream = Vec::new();
        for (_, body) in found {
            stream.extend(record_bytes(body));
        }
        stream
    } else {
        let files = state_files(&state);
        if !files.first().is_some_and(|f| f.exists()) {
            let _ = std::fs::write(&cache, b"");
            return 0;
        }
        records(&files)
    };
    if stream.is_empty() {
        let _ = std::fs::write(&cache, b"");
        return 0;
    }
    let (mut waiting, mut done) = (Vec::new(), Vec::new());
    let mut previous_age: Option<i64> = None;
    for line in stream.split(|&b| b == 0) {
        // El trozo tras el NUL final no es registro; uno vacío tendría `p` vacío.
        if line.is_empty() {
            continue;
        }
        let pipe = |b: &&u8| **b == b'|';
        let p = &line[..line.iter().position(|b| pipe(&b)).unwrap_or(line.len())];
        let rest = match line.iter().position(|b| pipe(&b)) {
            Some(i) => &line[i + 1..],
            None => line,
        };
        let s = &rest[..rest.iter().position(|b| pipe(&b)).unwrap_or(rest.len())];
        let t = match rest.iter().rposition(|b| pipe(&b)) {
            Some(i) => &rest[i + 1..],
            None => rest,
        };
        if p.is_empty() {
            continue;
        }
        let vars = super::bash::Vars {
            now,
            age: previous_age,
            p,
            s,
            t,
            rest,
            line,
            waiting: &waiting,
            done: &done,
        };
        // Un error aritmético de bash aborta el `while` entero.
        let Some(age) = arith(&vars) else { break };
        previous_age = Some(age);
        if age > 28800 {
            continue;
        }
        match s {
            b"waiting" => {
                waiting.extend_from_slice(p);
                waiting.push(b' ');
            }
            b"done" => {
                done.extend_from_slice(p);
                done.push(b' ');
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if !waiting.is_empty() {
        out.extend_from_slice(format!("#[fg=#D08770]{WAIT_ICON} ").as_bytes());
        out.extend(short(&waiting));
        out.extend_from_slice(b"#[default]");
    }
    if !done.is_empty() {
        out.extend_from_slice(format!("  #[fg=#A3BE8C]{DONE_ICON} ").as_bytes());
        out.extend(short(&done));
        out.extend_from_slice(b"#[default]");
    }
    let rendered = printf_b(&out);
    let mut tmp = cache.clone().into_os_string();
    tmp.push(format!(".tmp.{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    if std::fs::write(&tmp, &rendered).is_ok() {
        let _ = std::fs::rename(&tmp, &cache);
    }
    // `cat "$CACHE"` corre aunque la escritura haya fallado.
    let _ = std::io::stdout().write_all(&std::fs::read(&cache).unwrap_or_default());
    0
}
