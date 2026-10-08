//! `glob(STATE/*.json)` + `json.load` de `read_states`, con caché por firma
//! (D5): un archivo sin cambios no se reparsea; los que desaparecen se sueltan.
use super::{PyFloat, StateFault, py_float};
use crate::dash::native::files::{Strict, loads_strict};
use comandos_core::json::truthy;
use serde_json::{Map, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};

/// Un `H/state/*.json` que el Python acepta: un objeto y su `float(ts or 0)`.
#[derive(Debug, Clone)]
pub struct Record {
    pub value: Map<String, Value>,
    pub timestamp: f64,
}

/// `(dev, ino, size, mtime, mtime_ns, ctime, ctime_ns)`.
type Signature = (u64, u64, u64, i64, i64, i64, i64);

#[derive(Default)]
pub struct RecordCache {
    entries: HashMap<PathBuf, (Signature, Option<Record>)>,
    /// Archivos parseados (las pruebas comprueban que un acierto no reparsea).
    parses: usize,
    domain_entries: HashMap<String, (Vec<u8>, Option<Record>)>,
}

/// Un archivo como lo trata el `try` del bucle: `Ok(None)` = el `except`
/// (`OSError`, `ValueError`, `TypeError`) o un no-objeto lo saltan.
fn parse_record(path: &Path) -> Result<Option<Record>, StateFault> {
    let Ok(bytes) = fs::read(path) else {
        return Ok(None);
    };
    parse_record_bytes(&bytes)
}
fn parse_record_bytes(bytes: &[u8]) -> Result<Option<Record>, StateFault> {
    // `open()` en modo texto UTF-8: `UnicodeDecodeError` es `ValueError`.
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Ok(None);
    };
    let value = match loads_strict(text) {
        Strict::Value(v) => v,
        Strict::Unsure => return Err(StateFault::Decline),
        Strict::Missing | Strict::Unreadable => return Ok(None),
    };
    let Value::Object(map) = value else {
        return Ok(None);
    };
    let timestamp = match map.get("ts").filter(|v| truthy(v)) {
        None => 0.0,
        Some(v) => match py_float(v) {
            PyFloat::Value(f) if f.is_finite() => f,
            // `inf`/`nan` llegarían a la salida y al orden: incierto (D2).
            PyFloat::Value(_) | PyFloat::Unsure => return Err(StateFault::Decline),
            PyFloat::Raises => return Ok(None),
        },
    };
    Ok(Some(Record {
        value: map,
        timestamp,
    }))
}

impl RecordCache {
    /// Cuántos archivos se han parseado desde que se creó la caché.
    pub fn parses(&self) -> usize {
        self.parses
    }

    pub fn scan_domain(&mut self, home: &Path, dir: &Path) -> Result<Vec<Record>, StateFault> {
        comandos_store::domains::caller::read(home, "session-status", |mode, db| {
            if !matches!(mode, comandos_store::unified::Mode::Unified | comandos_store::unified::Mode::Sealed) { return Ok(self.scan(dir)); }
            let Some(db) = db else { return Ok(Err(StateFault::Decline)); };
            let rows = db.prepare("SELECT file_key,body FROM session_status WHERE file_key NOT LIKE '.%' AND substr(file_key,-5)='.json' ORDER BY file_key")?.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,Vec<u8>>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let mut seen = HashSet::new(); let mut out = Vec::new();
            for (key, body) in rows {
                seen.insert(key.clone());
                if let Some((old, record)) = self.domain_entries.get(&key) && old == &body { out.extend(record.clone()); continue; }
                self.parses += 1;
                let record = match parse_record_bytes(&body) { Ok(r) => r, Err(e) => return Ok(Err(e)) };
                out.extend(record.clone()); self.domain_entries.insert(key, (body,record));
            }
            self.domain_entries.retain(|key,_| seen.contains(key));
            Ok(Ok(out))
        }).map_err(|_| StateFault::Decline)?
    }

    /// Los registros en el orden de `read_dir` (el `getdents` de `glob`).
    /// Bloquea: llamar dentro de `spawn_blocking`.
    pub fn scan(&mut self, dir: &Path) -> Result<Vec<Record>, StateFault> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        // `glob` devuelve `[]` si el directorio no se puede listar.
        let Ok(listing) = fs::read_dir(dir) else {
            self.entries.clear();
            return Ok(out);
        };
        for entry in listing.flatten() {
            let name = entry.file_name();
            let bytes = name.as_bytes();
            // `glob("*.json")`: sin ocultos.
            if bytes.first() == Some(&b'.') || !bytes.ends_with(b".json") {
                continue;
            }
            let path = entry.path();
            // `open()` sigue enlaces; un directorio (o lo que no es archivo) se salta.
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            let signature = (
                meta.dev(),
                meta.ino(),
                meta.size(),
                meta.mtime(),
                meta.mtime_nsec(),
                meta.ctime(),
                meta.ctime_nsec(),
            );
            seen.insert(path.clone());
            if let Some((old, record)) = self.entries.get(&path)
                && *old == signature
            {
                out.extend(record.clone());
                continue;
            }
            self.parses += 1;
            let record = parse_record(&path)?;
            out.extend(record.clone());
            self.entries.insert(path, (signature, record));
        }
        self.entries.retain(|p, _| seen.contains(p));
        Ok(out)
    }
}
