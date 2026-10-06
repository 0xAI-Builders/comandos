//! Artefactos del HOME privado. SQLite se graba como esquema y valores, no páginas.
use crate::{normalize, restore};
use base64::{Engine, engine::general_purpose::STANDARD};
use rusqlite::{
    Connection, OpenFlags, params_from_iter,
    types::{Value as SqlValue, ValueRef},
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Component, Path, PathBuf},
};
fn private(home: &Path) -> Result<(), String> {
    if !home.is_absolute()
        || home == std::env::temp_dir()
        || !(home.starts_with(std::env::temp_dir())
            || (home.starts_with("/tmp") && home != Path::new("/tmp")))
        || !fs::symlink_metadata(home)
            .map_err(|e| e.to_string())?
            .is_dir()
    {
        return Err("artefactos requieren un HOME privado dentro del temporal".into());
    }
    Ok(())
}
fn ignored(path: &Path) -> bool {
    path.components().next().is_some_and(|c|matches!(c,Component::Normal(n) if n==".oracle"||n=="fakebin"||n=="xdg-runtime"||n=="tmux"||n=="bin"))
}
pub fn snapshot_tree(home: &Path, roots: &[(&str, &Path)]) -> Result<Value, String> {
    private(home)?;
    let mut out = serde_json::Map::new();
    walk(home, home, roots, &mut out)?;
    Ok(Value::Object(out))
}
fn walk(
    home: &Path,
    dir: &Path,
    roots: &[(&str, &Path)],
    out: &mut serde_json::Map<String, Value>,
) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let relative = path.strip_prefix(home).map_err(|e| e.to_string())?;
        if ignored(relative) {
            continue;
        }
        let key = relative
            .to_str()
            .ok_or("nombre de artefacto no UTF-8")?
            .to_owned();
        let m = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        let mode = m.permissions().mode() & 0o7777;
        if m.is_dir() {
            out.insert(key, json!({"kind":"directory","mode":mode}));
            walk(home, &path, roots, out)?;
        } else if m.file_type().is_symlink() {
            let target = fs::read_link(&path).map_err(|e| e.to_string())?;
            let target = target.to_str().ok_or("enlace no UTF-8")?;
            out.insert(key,json!({"kind":"symlink","target":String::from_utf8(normalize(target.as_bytes(),roots)).map_err(|e|e.to_string())?}));
        } else if m.is_file() {
            if is_sidecar(&path) {
                continue;
            }
            let bytes = fs::read(&path).map_err(|e| e.to_string())?;
            let value = if bytes.starts_with(b"SQLite format 3\0") {
                json!({"kind":"sqlite","mode":mode,"db":snapshot_sqlite(&path,roots)?})
            } else {
                json!({"kind":"file","mode":mode,"body_b64":STANDARD.encode(normalize(&bytes,roots))})
            };
            out.insert(key, value);
        } else { /* Los sockets de fixtures no son artefactos reproducibles. */
        }
    }
    Ok(())
}
fn is_sidecar(path: &Path) -> bool {
    let raw = path.to_string_lossy();
    for suffix in ["-wal", "-shm", "-journal"] {
        if let Some(base) = raw.strip_suffix(suffix)
            && fs::read(base).is_ok_and(|b| b.starts_with(b"SQLite format 3\0"))
        {
            return true;
        }
    }
    false
}
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
pub fn snapshot_sqlite(path: &Path, roots: &[(&str, &Path)]) -> Result<Value, String> {
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let schema: Vec<(String, String, Option<String>)> = c
        .prepare("SELECT type,name,sql FROM sqlite_master ORDER BY type,name")
        .map_err(|e| e.to_string())?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())?;
    let mut tables = serde_json::Map::new();
    for (kind, name, _) in &schema {
        if kind != "table" {
            continue;
        }
        let mut statement = c
            .prepare(&format!("SELECT * FROM {}", quote(name)))
            .map_err(|e| e.to_string())?;
        let n = statement.column_count();
        let mut data = vec![];
        let mut rows = statement.query([]).map_err(|e| e.to_string())?;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let mut cells = vec![];
            for column in 0..n {
                cells.push(match row.get_ref(column).map_err(|e| e.to_string())? {
                    ValueRef::Null => json!(["null"]),
                    ValueRef::Integer(v) => json!(["integer", v]),
                    ValueRef::Real(v) => json!(["real", v.to_bits()]),
                    ValueRef::Text(v) => json!(["text", STANDARD.encode(normalize(v, roots))]),
                    ValueRef::Blob(v) => json!(["blob", STANDARD.encode(v)]),
                });
            }
            data.push(Value::Array(cells));
        }
        tables.insert(name.clone(), Value::Array(data));
    }
    let version: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    Ok(json!({"schema":schema,"tables":tables,"user_version":version}))
}
fn destination(home: &Path, key: &str) -> Result<PathBuf, String> {
    let path = Path::new(key);
    if key.is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("ruta de artefacto fuera de HOME".into());
    }
    let path = home.join(path);
    let mut parent = home.to_owned();
    for part in Path::new(key)
        .parent()
        .unwrap_or(Path::new(""))
        .components()
    {
        parent.push(part);
        match fs::symlink_metadata(&parent) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err("padre de artefacto no regular".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::DirBuilder::new()
                .mode(0o700)
                .create(&parent)
                .map_err(|e| e.to_string())?,
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(path)
}
pub fn restore_tree(home: &Path, tree: &Value, roots: &[(&str, &Path)]) -> Result<(), String> {
    private(home)?;
    let tree = tree.as_object().ok_or("árbol de artefactos inválido")?;
    let current = snapshot_tree(home, roots)?;
    let mut old: Vec<_> = current
        .as_object()
        .ok_or("árbol actual inválido")?
        .iter()
        .collect();
    old.sort_by_key(|(key, _)| std::cmp::Reverse(key.len()));
    for (key, value) in old {
        if !tree.contains_key(key) {
            let path = destination(home, key)?;
            if value.get("kind").and_then(Value::as_str) == Some("directory") {
                fs::remove_dir(path).map_err(|e| e.to_string())?;
            } else {
                fs::remove_file(path).map_err(|e| e.to_string())?;
            }
        }
    }
    restore_entries(home, tree, roots)
}
pub(crate) fn apply_delta(
    home: &Path,
    before: &Value,
    after: &Value,
    roots: &[(&str, &Path)],
) -> Result<(), String> {
    private(home)?;
    let before = before.as_object().ok_or("árbol inicial inválido")?;
    let after = after.as_object().ok_or("árbol final inválido")?;
    let mut removed: Vec<_> = before
        .iter()
        .filter(|(key, _)| !after.contains_key(*key))
        .collect();
    removed.sort_by_key(|(key, _)| std::cmp::Reverse(key.len()));
    for (key, value) in removed {
        let path = destination(home, key)?;
        if value.get("kind").and_then(Value::as_str) == Some("directory") {
            fs::remove_dir(path).map_err(|e| e.to_string())?;
        } else {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    let changed = after
        .iter()
        .filter(|(key, value)| before.get(*key) != Some(*value))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    restore_entries(home, &changed, roots)
}
fn restore_entries(
    home: &Path,
    tree: &serde_json::Map<String, Value>,
    roots: &[(&str, &Path)],
) -> Result<(), String> {
    for (key, value) in tree {
        let path = destination(home, key)?;
        match fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() || m.file_type().is_symlink() => {
                fs::remove_file(&path).map_err(|e| e.to_string())?
            }
            Ok(m)
                if m.is_dir() && value.get("kind").and_then(Value::as_str) == Some("directory") => {
            }
            Ok(_) => return Err("destino de artefacto no regular".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        match value.get("kind").and_then(Value::as_str) {
            Some("directory") => {
                fs::DirBuilder::new()
                    .mode(0o700)
                    .recursive(true)
                    .create(&path)
                    .map_err(|e| e.to_string())?;
            }
            Some("symlink") => {
                let target = value
                    .get("target")
                    .and_then(Value::as_str)
                    .ok_or("enlace sin destino")?;
                let bytes = restore(target.as_bytes(), roots);
                std::os::unix::fs::symlink(
                    std::str::from_utf8(&bytes).map_err(|e| e.to_string())?,
                    &path,
                )
                .map_err(|e| e.to_string())?;
            }
            Some("file") => {
                let bytes = STANDARD
                    .decode(
                        value
                            .get("body_b64")
                            .and_then(Value::as_str)
                            .ok_or("archivo sin bytes")?,
                    )
                    .map_err(|e| e.to_string())?;
                fs::write(&path, restore(&bytes, roots)).map_err(|e| e.to_string())?;
            }
            Some("sqlite") => {
                restore_sqlite(&path, value.get("db").ok_or("SQLite sin esquema")?, roots)?
            }
            _ => return Err("tipo de artefacto inválido".into()),
        }
        if let Some(mode) = value.get("mode").and_then(Value::as_u64) {
            fs::set_permissions(
                path,
                fs::Permissions::from_mode(u32::try_from(mode).map_err(|e| e.to_string())?),
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn restore_sqlite(path: &Path, value: &Value, roots: &[(&str, &Path)]) -> Result<(), String> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let p = PathBuf::from(format!("{}{suffix}", path.display()));
        match fs::remove_file(p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    let c = Connection::open(path).map_err(|e| e.to_string())?;
    // El snapshot contiene el estado completo; las tablas pueden estar ordenadas
    // con la hija antes que la padre. Restaurar tampoco debe reparar el oráculo.
    c.pragma_update(None, "foreign_keys", false)
        .map_err(|e| e.to_string())?;
    let schema = value
        .get("schema")
        .and_then(Value::as_array)
        .ok_or("SQLite sin esquema")?;
    for row in schema {
        if row.get(0).and_then(Value::as_str) == Some("table")
            && row.get(1).and_then(Value::as_str) != Some("sqlite_sequence")
            && let Some(sql) = row.get(2).and_then(Value::as_str)
        {
            c.execute_batch(sql).map_err(|e| e.to_string())?;
        }
    }
    let tables = value
        .get("tables")
        .and_then(Value::as_object)
        .ok_or("SQLite sin tablas")?;
    for (name, rows) in tables {
        c.execute(&format!("DELETE FROM {}", quote(name)), [])
            .map_err(|e| e.to_string())?;
        for row in rows.as_array().ok_or("filas SQLite inválidas")? {
            let mut cells = vec![];
            for cell in row.as_array().ok_or("celdas SQLite inválidas")? {
                cells.push(match cell.get(0).and_then(Value::as_str) {
                    Some("null") => SqlValue::Null,
                    Some("integer") => SqlValue::Integer(
                        cell.get(1)
                            .and_then(Value::as_i64)
                            .ok_or("integer inválido")?,
                    ),
                    Some("real") => SqlValue::Real(f64::from_bits(
                        cell.get(1).and_then(Value::as_u64).ok_or("real inválido")?,
                    )),
                    Some("text" | "blob") => {
                        let bytes = STANDARD
                            .decode(
                                cell.get(1)
                                    .and_then(Value::as_str)
                                    .ok_or("bytes SQLite inválidos")?,
                            )
                            .map_err(|e| e.to_string())?;
                        if cell.get(0).and_then(Value::as_str) == Some("text") {
                            SqlValue::Text(
                                String::from_utf8(restore(&bytes, roots))
                                    .map_err(|e| e.to_string())?,
                            )
                        } else {
                            SqlValue::Blob(bytes)
                        }
                    }
                    _ => return Err("tipo SQLite inválido".into()),
                });
            }
            let placeholders = std::iter::repeat_n("?", cells.len())
                .collect::<Vec<_>>()
                .join(",");
            c.execute(
                &format!("INSERT INTO {} VALUES ({placeholders})", quote(name)),
                params_from_iter(cells),
            )
            .map_err(|e| e.to_string())?;
        }
    }
    for row in schema {
        if row.get(0).and_then(Value::as_str) != Some("table")
            && let Some(sql) = row.get(2).and_then(Value::as_str)
        {
            c.execute_batch(sql).map_err(|e| e.to_string())?;
        }
    }
    let uv = value
        .get("user_version")
        .and_then(Value::as_i64)
        .ok_or("user_version inválido")?;
    c.pragma_update(None, "user_version", uv)
        .map_err(|e| e.to_string())?;
    Ok(())
}
