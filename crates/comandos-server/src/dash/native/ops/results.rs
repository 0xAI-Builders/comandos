//! `MOTOR_RESULT` y `motor-results.json` (`bin/cc-dash` 3614–3672): el
//! progreso y el resultado de cada operación de sesión por `operationKey`
//! (`sesión|pane`).
//!
//! Dos escritores (decisión I2 de la ronda 1 de T2): el frente atiende las
//! operaciones que porta y el Python heredado las que el frente declina, y
//! cada uno reescribe el archivo entero. El frente fusiona por clave:
//! - lee el archivo (en caché por `mtime`, tamaño e inodo) y le superpone sus
//!   propias entradas en memoria; en una misma clave gana el `ts` más nuevo;
//! - cada escritura suya relee el archivo, fusiona, recorta y lo sustituye de
//!   forma atómica (`write_json_file`);
//! - si el heredado pisa el archivo desde su memoria, el frente lo tolera: sus
//!   lecturas ya fusionan y su siguiente escritura restaura sus claves.
//!
//! El mapa fusionado se acota con las reglas de O3 (más de 300 → los 200 más
//! recientes por `float(ts or 0)`, orden estable; `dict.pop` conserva el
//! orden del resto). Con un único escritor, el archivo es byte a byte el del
//! Python (las pruebas lo comparan).
//!
//! Bloquea (lee y escribe el archivo): las rutas lo llaman dentro de
//! `spawn_blocking`; los hilos de operación, directamente.
use super::super::{NativeOptions, files};
use comandos_core::text;
use comandos_runtime::hooks::py::float_value;
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

/// `time.time()`.
pub type Seconds = Arc<dyn Fn() -> f64 + Send + Sync>;

/// Lo que identifica una versión del archivo sin leerlo.
type Stamp = (i64, i64, u64, u64);

#[derive(Default)]
struct State {
    /// Las entradas que escribió este proceso (acotadas como el archivo).
    own: Map<String, Value>,
    /// El archivo tal como se leyó por última vez.
    disk: Map<String, Value>,
    stamp: Option<Stamp>,
    /// La última lectura no se reprodujo con certeza (bytes que no son UTF-8,
    /// sustitutos sueltos…): no se reescribe el archivo, se perdería lo que el
    /// Python conservaría. Quien inicia operaciones declina (`certain`).
    uncertain: bool,
}

pub struct MotorResults {
    path: PathBuf,
    clock: Seconds,
    state: Mutex<State>,
}

/// El resultado de `set`/`stage` cuando no se pudo reproducir el Python.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uncertain;

impl MotorResults {
    pub fn load(path: &Path) -> Self {
        Self::load_with(path, Arc::new(now))
    }

    /// Sin E/S: el archivo se lee al primer uso.
    pub fn load_with(path: &Path, clock: Seconds) -> Self {
        Self {
            path: path.to_owned(),
            clock,
            state: Mutex::new(State::default()),
        }
    }

    /// El dueño de `H/motor-results.json` para todo el proceso (sin E/S).
    pub fn shared(opts: &NativeOptions) -> Arc<MotorResults> {
        static OWNERS: OnceLock<Mutex<HashMap<PathBuf, Arc<MotorResults>>>> = OnceLock::new();
        let path = opts.hooks.join("motor-results.json");
        let mut owners = OWNERS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        owners
            .entry(path.clone())
            .or_insert_with(|| {
                let clock = opts.clock_seconds.clone();
                Arc::new(Self::load_with(&path, Arc::new(move || clock())))
            })
            .clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// `_motor_result_load()` cuando el archivo cambió (o siempre con
    /// `force`): cualquier error o un JSON que no es objeto es `{}`; lo
    /// incierto conserva la última lectura y se marca.
    fn refresh(&self, state: &mut State, force: bool) {
        let stamp = std::fs::metadata(&self.path)
            .ok()
            .map(|m| (m.mtime(), m.mtime_nsec(), m.size(), m.ino()));
        if !force && state.stamp.is_some() && stamp == state.stamp {
            return;
        }
        match files::read_json_strict(&self.path) {
            files::Strict::Missing | files::Strict::Unreadable => {
                state.disk = Map::new();
                state.uncertain = false;
            }
            files::Strict::Unsure => state.uncertain = true,
            files::Strict::Value(Value::Object(map)) => {
                state.disk = map;
                state.uncertain = false;
            }
            files::Strict::Value(_) => {
                state.disk = Map::new();
                state.uncertain = false;
            }
        }
        state.stamp = stamp;
    }

    /// El archivo con las entradas propias superpuestas (gana el `ts` más
    /// nuevo; en una clave del archivo, su posición) y el recorte de O3.
    fn merged(state: &State) -> Result<Map<String, Value>, Uncertain> {
        let mut map = state.disk.clone();
        for (key, own) in &state.own {
            let newer = match map.get(key) {
                None => true,
                Some(theirs) => match (sort_key(own), sort_key(theirs)) {
                    (Some(a), Some(b)) => a >= b,
                    (Some(_), None) => true,
                    _ => false,
                },
            };
            if newer {
                map.insert(key.clone(), own.clone());
            }
        }
        trim(&mut map)?;
        Ok(map)
    }

    /// ¿Reproduce la última lectura del archivo lo que cargaría el Python?
    pub fn certain(&self) -> bool {
        let mut state = self.lock();
        self.refresh(&mut state, false);
        !state.uncertain
    }

    /// `MOTOR_RESULT` visto desde el frente: el archivo fusionado con lo propio.
    pub fn all(&self) -> Map<String, Value> {
        let mut state = self.lock();
        self.refresh(&mut state, false);
        Self::merged(&state).unwrap_or_else(|_| {
            let mut map = state.disk.clone();
            for (k, v) in &state.own {
                map.insert(k.clone(), v.clone());
            }
            map
        })
    }

    /// `MOTOR_RESULT.get(key)`.
    pub fn get(&self, key: &str) -> Option<Value> {
        self.all().get(key).cloned()
    }

    /// `motor_result_set(key, ok, detail, **fields)`: `detail[:200]`,
    /// `ts = time.time()`; los campos `None` no se guardan.
    pub fn set(
        &self,
        key: &str,
        ok: bool,
        detail: &str,
        fields: &[(&str, Value)],
    ) -> Result<(), Uncertain> {
        let ts = (self.clock)();
        self.set_with_ts(key, ok, detail, fields, ts)
    }

    pub fn set_with_ts(
        &self,
        key: &str,
        ok: bool,
        detail: &str,
        fields: &[(&str, Value)],
        ts: f64,
    ) -> Result<(), Uncertain> {
        let mut result = Map::new();
        result.insert("ok".into(), Value::Bool(ok));
        result.insert("detail".into(), Value::from(prefix(detail, 200)));
        result.insert("ts".into(), float_value(ts));
        extend(&mut result, fields);
        self.put(key, result)
    }

    /// `motor_stage(key, stage, code, **fields)`: `stage[:120]`,
    /// `stageCode[:40]` solo si hay código.
    pub fn stage(
        &self,
        key: &str,
        stage: &str,
        code: &str,
        fields: &[(&str, Value)],
    ) -> Result<(), Uncertain> {
        let mut result = Map::new();
        result.insert("stage".into(), Value::from(prefix(stage, 120)));
        result.insert("ts".into(), float_value((self.clock)()));
        if !code.is_empty() {
            result.insert("stageCode".into(), Value::from(prefix(code, 40)));
        }
        extend(&mut result, fields);
        self.put(key, result)
    }

    /// `_set_motor_result(key, result)` con fusión: relee el archivo, guarda
    /// la entrada propia, fusiona, recorta y sustituye el archivo (sus errores
    /// de escritura se tragan, como `_motor_result_write`).
    fn put(&self, key: &str, result: Map<String, Value>) -> Result<(), Uncertain> {
        let mut state = self.lock();
        self.refresh(&mut state, true);
        let entry = Value::Object(result);
        // `merged` decide por timestamp y conserva la posición del disco.
        // Sustituir aquí la entrada del disco perdería el resultado más nuevo.
        state.own.insert(key.to_owned(), entry);
        let _ = trim(&mut state.own);
        if state.uncertain {
            return Err(Uncertain);
        }
        let merged = match Self::merged(&state) {
            Ok(merged) => merged,
            Err(Uncertain) => {
                // `float(ts)` lanza en el Python tras guardar y antes de escribir.
                return Err(Uncertain);
            }
        };
        // Lo propio que el recorte dejó fuera ya no vuelve.
        state.own.retain(|k, _| merged.contains_key(k));
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if files::write_json_atomic(&self.path, &Value::Object(merged.clone())).is_ok() {
            state.disk = merged;
            // El heredado puede reemplazar el archivo después del rename.
            // Su metadata no identifica `merged`: la siguiente lectura debe
            // recargar antes de asociar contenido y sello.
            state.stamp = None;
        }
        Ok(())
    }
}

/// Recorte de O3: con más de 300 claves, quedan las 200 de `ts` más reciente
/// (`sorted` estable con `<`, sin NaN) y el resto conserva su orden.
fn trim(map: &mut Map<String, Value>) -> Result<(), Uncertain> {
    if map.len() <= 300 {
        return Ok(());
    }
    let mut keyed = Vec::with_capacity(map.len());
    for (k, v) in map.iter() {
        keyed.push((sort_key(v).ok_or(Uncertain)?, k.clone()));
    }
    keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let drop = keyed.len() - 200;
    for (_, old) in keyed.into_iter().take(drop) {
        map.shift_remove(&old);
    }
    Ok(())
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// `str(x)[:n]` en caracteres.
fn prefix(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

fn extend(result: &mut Map<String, Value>, fields: &[(&str, Value)]) {
    for (key, value) in fields {
        if !value.is_null() {
            result.insert((*key).to_owned(), value.clone());
        }
    }
}

/// `float(value.get("ts") or 0)`, sin NaN (su orden en `sorted` depende de
/// las comparaciones de timsort).
fn sort_key(value: &Value) -> Option<f64> {
    let ts = value.as_object()?.get("ts").unwrap_or(&Value::Null);
    let f = match ts {
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => n.as_f64()?,
        Value::String(s) if s.is_empty() => 0.0,
        Value::String(s) => text::float(s).ok()?,
        Value::Array(a) if a.is_empty() => 0.0,
        Value::Object(o) if o.is_empty() => 0.0,
        _ => return None,
    };
    (!f.is_nan()).then_some(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_keys_like_python_float() {
        assert_eq!(sort_key(&serde_json::json!({"ts": "1_5"})), Some(15.0));
        assert_eq!(sort_key(&serde_json::json!({"ts": null})), Some(0.0));
        assert_eq!(sort_key(&serde_json::json!({"ts": true})), Some(1.0));
        assert_eq!(sort_key(&serde_json::json!({})), Some(0.0));
        assert_eq!(sort_key(&serde_json::json!({"ts": "x"})), None);
        assert_eq!(sort_key(&serde_json::json!({"ts": "nan"})), None);
        assert_eq!(sort_key(&serde_json::json!("no")), None);
    }

    #[test]
    fn prefix_counts_characters() {
        assert_eq!(prefix("ñañaña", 3), "ñañ");
    }
}
