//! Port de `capture_session`, `remap_layout` y `_layout_checksum` de
//! `lib/tmux_snapshot.py`. La librería nunca lanza tmux: lo hace el llamador
//! con el callback, igual que el `tmux` que recibe el Python.
//!
//! Errores: `RuntimeError` del Python (`_checked`, layout que cambia) es
//! `SnapshotError::Runtime`; `ValueError` (desempaquetar una fila) es
//! `SnapshotError::Value`; lo que el port no reproduce con certeza (un `int()`
//! de un texto que no es un entero ASCII, la inspección incierta del pane) es
//! `SnapshotError::Unsure` y quien llama declina.
use crate::{
    Unsure,
    pane_snapshot::{PaneInspector, PaneRef},
    pane_typing::TmuxResult,
};
use comandos_core::text::{self, NumError, splitlines};
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

#[derive(Debug)]
pub enum SnapshotError {
    /// `ValueError` del Python, con su texto.
    Value(String),
    /// `RuntimeError` del Python, con su texto.
    Runtime(String),
    /// `OSError` del Python (carpeta o archivo de la copia).
    Io(std::io::Error),
    /// El port no puede reproducir con certeza lo que haría el Python.
    Unsure,
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::Value(text) | SnapshotError::Runtime(text) => f.write_str(text),
            SnapshotError::Io(error) => write!(f, "{error}"),
            SnapshotError::Unsure => f.write_str("instantánea incierta"),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl From<Unsure> for SnapshotError {
    fn from(_: Unsure) -> Self {
        SnapshotError::Unsure
    }
}

impl From<std::io::Error> for SnapshotError {
    fn from(error: std::io::Error) -> Self {
        SnapshotError::Io(error)
    }
}

pub type Result<T> = std::result::Result<T, SnapshotError>;

const WINDOW_FIELDS: [&str; 8] = [
    "window_id",
    "window_index",
    "window_name",
    "window_layout",
    "window_width",
    "window_height",
    "window_active",
    "window_zoomed_flag",
];
const PANE_FIELDS: [&str; 7] = [
    "pane_id",
    "pane_index",
    "pane_current_path",
    "pane_pid",
    "pane_current_command",
    "pane_active",
    "@comandos-pane-key",
];

/// `_LEAF = (\d+x\d+,\d+,\d+),(\d+)(?=[,}\]]|$)` sin la anticipación, que
/// `regex` no tiene: la comprueba `leaves` a mano. `\d` es Unicode en los dos.
static LEAF: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(\d+x\d+,\d+,\d+),(\d+)").ok());

/// Las coincidencias de `_LEAF` como `finditer`: `(inicio, fin, grupo 1, grupo 2)`.
///
/// Con un inicio fijo los `\d+` del patrón solo pueden tomar el tramo de
/// dígitos entero (los cierra una `x` o una coma), y el último, si es más
/// corto, va seguido de otro dígito: la anticipación falla igual. Así que si la
/// coincidencia más larga no la cumple, ninguna con ese inicio la cumple y la
/// búsqueda sigue en el carácter siguiente, como el motor de `re`.
fn leaves(layout: &str) -> Option<Vec<(usize, usize, &str, &str)>> {
    let leaf = LEAF.as_ref()?;
    let mut out = Vec::new();
    let mut pos = 0;
    while pos <= layout.len() {
        let Some(caps) = leaf.captures_at(layout, pos) else {
            break;
        };
        let (Some(whole), Some(geometry), Some(id)) = (caps.get(0), caps.get(1), caps.get(2))
        else {
            break;
        };
        let rest = layout.get(whole.end()..).unwrap_or("");
        // `$` sin MULTILINE: fin del texto o antes de un `\n` final.
        let ahead = rest.is_empty() || rest == "\n" || rest.starts_with([',', '}', ']']);
        if ahead {
            out.push((whole.start(), whole.end(), geometry.as_str(), id.as_str()));
            pos = whole.end();
        } else {
            let step = layout
                .get(whole.start()..)
                .and_then(|s| s.chars().next())
                .map_or(1, char::len_utf8);
            pos = whole.start() + step;
        }
    }
    Some(out)
}

/// `_layout_checksum` de `lib/tmux_snapshot.py` (el de `layout.c` de tmux).
/// `ord(c)` de un carácter fuera del BMP o mayor que 0xffff se suma entero y
/// después se recorta a 16 bits: equivale a sumar su punto de código módulo 2^16.
pub fn layout_checksum(body: &str) -> u16 {
    body.chars().fold(0u16, |csum, c| {
        let rotated = (csum >> 1) | ((csum & 1) << 15);
        rotated.wrapping_add(u32::from(c) as u16)
    })
}

/// `remap_layout(layout, panes)`: las hojas del árbol con los ids nuevos y su
/// checksum. `None` donde el Python lanza (`IndexError` sin coma, `KeyError`
/// de una hoja que no está en `panes`).
pub fn remap_layout(layout: &str, panes: &BTreeMap<String, String>) -> Option<String> {
    let (_, body) = layout.split_once(',')?;
    let mut out = String::with_capacity(body.len());
    let mut last = 0;
    for (start, end, geometry, id) in leaves(body)? {
        let new = panes.get(&format!("%{id}"))?;
        out.push_str(body.get(last..start)?);
        out.push_str(geometry);
        out.push(',');
        out.push_str(new.trim_start_matches('%'));
        last = end;
    }
    out.push_str(body.get(last..)?);
    Some(format!("{:04x},{out}", layout_checksum(&out)))
}

/// `_checked(tmux, *args)`: la salida sin blancos a los lados, o
/// `RuntimeError('tmux <orden>: <stderr>')`.
fn checked(tmux: &mut impl FnMut(&[&str]) -> Result<TmuxResult>, args: &[&str]) -> Result<String> {
    let r = tmux(args)?;
    if r.returncode != 0 {
        return Err(SnapshotError::Runtime(format!(
            "tmux {}: {}",
            args.first().copied().unwrap_or(""),
            text::strip(&r.stderr)
        )));
    }
    Ok(text::strip(&r.stdout).to_owned())
}

/// `int(text)`: los textos que no son un entero ASCII no se reproducen.
fn py_int(raw: &str) -> Result<i64> {
    match text::int(raw) {
        Ok(n) => Ok(n),
        // El texto de `ValueError` lleva el `repr`: no se reproduce.
        Err(NumError::Invalid | NumError::Exotic) => Err(SnapshotError::Unsure),
    }
}

/// `a, b, … = parts` con `n` nombres: los textos de CPython 3.10.
fn unpack(parts: Vec<&str>, n: usize) -> Result<Vec<&str>> {
    match parts.len() {
        len if len == n => Ok(parts),
        len if len < n => Err(SnapshotError::Value(format!(
            "not enough values to unpack (expected {n}, got {len})"
        ))),
        _ => Err(SnapshotError::Value(format!(
            "too many values to unpack (expected {n})"
        ))),
    }
}

fn format_of(fields: &[&str]) -> String {
    fields
        .iter()
        .map(|f| format!("#{{{f}}}"))
        .collect::<Vec<_>>()
        .join("\t")
}

/// `process_start_time(pid)`: el tick de arranque del proceso, o `None`.
fn start_time(pid: &str) -> Value {
    crate::process_start_time(pid).map_or(Value::Null, Value::from)
}

/// `capture_session(tmux, session, describe_pane)` con `PaneInspector` como
/// `describe_pane` y el reloj del sistema para `captured_at`.
pub fn capture_session(
    tmux: &mut impl FnMut(&[&str]) -> Result<TmuxResult>,
    session: &str,
    inspector: &PaneInspector,
) -> Result<Value> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    capture_session_at(tmux, session, inspector, now)
}

/// `capture_session` con `captured_at` (= `int(time.time())`) inyectado.
pub fn capture_session_at(
    tmux: &mut impl FnMut(&[&str]) -> Result<TmuxResult>,
    session: &str,
    inspector: &PaneInspector,
    captured_at: i64,
) -> Result<Value> {
    let target = format!("={session}");
    let raw = checked(
        tmux,
        &[
            "list-windows",
            "-t",
            &target,
            "-F",
            &format_of(&WINDOW_FIELDS),
        ],
    )?;
    let pane_format = format_of(&PANE_FIELDS);
    let mut windows = Vec::new();
    for line in splitlines(&raw) {
        let parts = unpack(line.split('\t').collect(), WINDOW_FIELDS.len())?;
        let field = |i: usize| parts.get(i).copied().unwrap_or("");
        let (wid, layout) = (field(0), field(3));
        let mut window = Map::new();
        window.insert("id".into(), Value::from(wid));
        window.insert("index".into(), Value::from(py_int(field(1))?));
        window.insert("name".into(), Value::from(field(2)));
        window.insert("layout".into(), Value::from(layout));
        window.insert("width".into(), Value::from(py_int(field(4))?));
        window.insert("height".into(), Value::from(py_int(field(5))?));
        window.insert("active".into(), Value::Bool(field(6) == "1"));
        window.insert("zoomed".into(), Value::Bool(field(7) == "1"));
        window.insert("panes".into(), Value::Array(Vec::new()));
        for (key, option, default) in [
            ("border_status", "pane-border-status", "off"),
            ("automatic_rename", "automatic-rename", "off"),
            ("window_size", "window-size", "latest"),
        ] {
            let value = checked(tmux, &["show-options", "-wAv", "-t", wid, option])?;
            let value = if value.is_empty() {
                default.to_owned()
            } else {
                value
            };
            window.insert(key.into(), Value::from(value));
        }
        let rows = checked(tmux, &["list-panes", "-t", wid, "-F", &pane_format])?;
        let mut panes = Vec::new();
        let mut ids = BTreeSet::new();
        for row in splitlines(&rows) {
            // `(row.split('\t') + [''])[:7]`.
            let mut parts: Vec<&str> = row.split('\t').collect();
            parts.push("");
            parts.truncate(PANE_FIELDS.len());
            let parts = unpack(parts, PANE_FIELDS.len())?;
            let field = |i: usize| parts.get(i).copied().unwrap_or("");
            let (id, process, command, tagged) = (field(0), field(3), field(4), field(6));
            let pid = py_int(process)?;
            let mut pane = Map::new();
            pane.insert("id".into(), Value::from(id));
            pane.insert("index".into(), Value::from(py_int(field(1))?));
            pane.insert("cwd".into(), Value::from(field(2)));
            pane.insert("pid".into(), Value::from(pid));
            pane.insert("start".into(), start_time(process));
            pane.insert("command".into(), Value::from(command));
            pane.insert("active".into(), Value::Bool(field(5) == "1"));
            if !tagged.is_empty() {
                pane.insert("tagged_key".into(), Value::from(tagged));
            }
            // `pane.update(describe_pane(pane))`: una clave que ya estaba
            // conserva su sitio, como en un `dict`.
            for (key, value) in inspector.inspect(&PaneRef { id, pid, command })? {
                pane.insert(key, value);
            }
            ids.insert(id.to_owned());
            panes.push(Value::Object(pane));
        }
        let leaf_ids: BTreeSet<String> = leaves(layout)
            .ok_or(SnapshotError::Unsure)?
            .into_iter()
            .map(|(_, _, _, id)| format!("%{id}"))
            .collect();
        if leaf_ids != ids {
            return Err(SnapshotError::Runtime(
                "Pane layout changed during capture".into(),
            ));
        }
        window.insert("panes".into(), Value::Array(panes));
        let current = checked(
            tmux,
            &["display-message", "-p", "-t", wid, "#{window_layout}"],
        )?;
        if current != layout {
            return Err(SnapshotError::Runtime(
                "Window resized during capture".into(),
            ));
        }
        windows.push(Value::Object(window));
    }
    if windows.is_empty() {
        return Err(SnapshotError::Runtime("No windows to capture".into()));
    }
    let mut out = Map::new();
    out.insert("windows".into(), Value::Array(windows));
    out.insert("captured_at".into(), Value::from(captured_at));
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_follow_the_lookahead() {
        let found = leaves("1x2,3,4,5x6,7,8,9}").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].2, "5x6,7,8");
        assert_eq!(found[0].3, "9");
        assert!(leaves("12x3,0,0,45a").unwrap().is_empty());
        assert_eq!(leaves("12x3,0,0,45\n").unwrap().len(), 1);
    }

    #[test]
    fn checksum_wraps_code_points() {
        // `ord('€') = 0x20ac`; un carácter astral se recorta a 16 bits.
        assert_eq!(layout_checksum("€"), 0x20ac);
        assert_eq!(layout_checksum("\u{1f600}"), 0xf600);
    }
}
