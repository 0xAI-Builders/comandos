//! Port de `save_closed_pane_snapshot` (`bin/cc-dash` 5684): antes de cerrar
//! un pane a petición, una copia acotada de la sesión y de su texto en
//! `~/.local/state/comandos/closed-panes` (las 50 más nuevas).
//!
//! Reloj y `uuid4` inyectados: `now_ns` = `time.time_ns()`, `uuid_hex` =
//! `uuid.uuid4().hex`, `now_s` = `time.time()` (que también da `captured_at`).
use crate::{
    pane_snapshot::PaneInspector,
    pane_typing::TmuxResult,
    tmux_snapshot::{Result, SnapshotError, capture_session_at},
};
use comandos_core::json::response_dumps_unicode;
use serde_json::{Map, Value};
use std::{
    fs,
    io::Write,
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    },
    path::Path,
};

/// `captured.stdout[-500_000:]` (caracteres).
const TEXT_CHARS: usize = 500_000;
/// Copias explícitas que se conservan.
const KEEP: usize = 50;

/// `save_closed_pane_snapshot(sess, pane)`: devuelve el nombre del archivo.
#[allow(clippy::too_many_arguments)]
pub fn save(
    home: &Path,
    tmux: &mut impl FnMut(&[&str]) -> Result<TmuxResult>,
    inspector: &PaneInspector,
    sess: &str,
    pane: &str,
    now_ns: i128,
    uuid_hex: &str,
    now_s: f64,
) -> Result<String> {
    // `int(time.time())`: trunca hacia cero.
    let captured_at = now_s.trunc() as i64;
    let layout = capture_session_at(tmux, sess, inspector, captured_at)?;
    let present = layout
        .get("windows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|w| w.get("panes").and_then(Value::as_array))
        .flatten()
        .any(|p| p.get("id").and_then(Value::as_str) == Some(pane));
    if !present {
        return Err(SnapshotError::Value(
            "El panel cambió antes de guardar la copia".into(),
        ));
    }
    let captured = tmux(&["capture-pane", "-p", "-J", "-t", pane, "-S", "-2000"])?;
    if captured.returncode != 0 {
        return Err(SnapshotError::Value(
            "No se pudo guardar el texto. El panel sigue abierto".into(),
        ));
    }
    let root = home.join(".local/state/comandos/closed-panes");
    // `mkdir(parents=True, exist_ok=True, mode=0o700)`: los padres con el modo
    // por omisión, la carpeta con 0700 (menos la umask) y después `chmod`.
    if let Some(parent) = root.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::DirBuilder::new().mode(0o700).create(&root) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && root.is_dir() => {}
        Err(e) => return Err(e.into()),
    }
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    let name = format!("{now_ns}-{uuid_hex}.json");
    let text = &captured.stdout;
    let count = text.chars().count();
    let tail = match text.char_indices().nth(count.saturating_sub(TEXT_CHARS)) {
        Some((at, _)) => text.get(at..).unwrap_or(""),
        None => "",
    };
    let mut record = Map::new();
    record.insert("session".into(), Value::from(sess));
    record.insert("pane".into(), Value::from(pane));
    record.insert("layout".into(), layout);
    record.insert("text".into(), Value::from(tail));
    record.insert("closedAt".into(), Value::from(now_s));
    // Se serializa antes de crear el archivo: lo que el codificador portado no
    // escribe se declina sin dejar una copia a medias.
    let body = response_dumps_unicode(&Value::Object(record)).map_err(|_| SnapshotError::Unsure)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join(&name))?;
    file.write_all(body.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    prune(&root);
    Ok(name)
}

/// `sorted(root.glob('[0-9]*-*.json'), reverse=True)[50:]` y `unlink` de cada
/// uno, ignorando los `OSError`. Los nombres se ordenan por punto de código
/// como `str`; un byte que no es UTF-8 cuenta como su sustituto
/// (`surrogateescape`, U+DC80–U+DCFF).
fn prune(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut names: Vec<(Vec<u32>, std::path::PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let bytes = name.as_bytes();
            matches_pattern(bytes).then(|| (code_points(bytes), entry.path()))
        })
        .collect();
    names.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in names.into_iter().skip(KEEP) {
        let _ = fs::remove_file(path);
    }
}

/// `fnmatch('[0-9]*-*.json')`: un dígito ASCII, luego un `-` en algún sitio y
/// `.json` al final (sin solaparse con el `-`).
fn matches_pattern(name: &[u8]) -> bool {
    let Some((first, rest)) = name.split_first() else {
        return false;
    };
    if !first.is_ascii_digit() {
        return false;
    }
    let Some(stem) = rest.strip_suffix(b".json") else {
        return false;
    };
    stem.contains(&b'-')
}

fn code_points(bytes: &[u8]) -> Vec<u32> {
    let mut out = Vec::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        out.extend(chunk.valid().chars().map(u32::from));
        out.extend(chunk.invalid().iter().map(|b| 0xdc00 + u32::from(*b)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_is_fnmatch() {
        assert!(matches_pattern(b"1-a.json"));
        assert!(matches_pattern(b"12-.json"));
        assert!(!matches_pattern(b"1.json"));
        assert!(!matches_pattern(b"a1-x.json"));
        assert!(!matches_pattern(b"1-x.json.bak"));
        // `*-*` antes de `.json`: el guion no puede ser el del sufijo.
        assert!(!matches_pattern(b"1.json"));
    }

    #[test]
    fn surrogates_sort_like_python() {
        assert!(code_points(b"9\xff") > code_points("9\u{d7ff}".as_bytes()));
    }
}
