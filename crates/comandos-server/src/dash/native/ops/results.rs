//! `MOTOR_RESULT` y `motor-results.json` (`bin/cc-dash` 3614–3672): el
//! progreso y el resultado de cada operación de sesión por `operationKey`
//! (`sesión|pane`). Dueño único en el frente (O3): un mapa por archivo, con
//! su candado, cargado una vez como el módulo del Python y reescrito entero
//! en cada cambio (`write_json_file`).
//!
//! Bloquea (escribe el archivo): las rutas lo llaman dentro de
//! `spawn_blocking`; los hilos de operación, directamente.
use super::super::{NativeOptions, files};
use comandos_core::text;
use comandos_runtime::hooks::py::float_value;
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

/// `time.time()`.
pub type Seconds = Arc<dyn Fn() -> f64 + Send + Sync>;

struct State {
    map: Map<String, Value>,
    /// El archivo tenía algo que el Python habría cargado y este port no
    /// reproduce (bytes que no son UTF-8 de forma incierta, sustitutos
    /// sueltos…), o un recorte que no se pudo calcular con certeza. Mientras
    /// tanto no se reescribe el archivo: se perdería lo que el Python
    /// conservaría. Quien inicia operaciones declina (`certain`).
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
    /// `_motor_result_load()`: cualquier error (o un JSON que no es objeto)
    /// es `{}`; lo incierto se marca.
    pub fn load(path: &Path) -> Self {
        Self::load_with(path, Arc::new(now))
    }

    pub fn load_with(path: &Path, clock: Seconds) -> Self {
        let (map, uncertain) = match files::read_json_strict(path) {
            files::Strict::Missing | files::Strict::Unreadable => (Map::new(), false),
            files::Strict::Unsure => (Map::new(), true),
            files::Strict::Value(Value::Object(map)) => (map, false),
            files::Strict::Value(_) => (Map::new(), false),
        };
        Self {
            path: path.to_owned(),
            clock,
            state: Mutex::new(State { map, uncertain }),
        }
    }

    /// El dueño de `H/motor-results.json` para todo el proceso.
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

    /// ¿Reproduce este mapa el `MOTOR_RESULT` del Python?
    pub fn certain(&self) -> bool {
        !self.lock().uncertain
    }

    /// Copia de `MOTOR_RESULT`.
    pub fn all(&self) -> Map<String, Value> {
        self.lock().map.clone()
    }

    /// `MOTOR_RESULT.get(key)`.
    pub fn get(&self, key: &str) -> Option<Value> {
        self.lock().map.get(key).cloned()
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

    /// `_set_motor_result(key, result)`: guarda, recorta a los 200 más
    /// recientes al pasar de 300 y reescribe el archivo (sus errores se
    /// tragan, como `_motor_result_write`).
    fn put(&self, key: &str, result: Map<String, Value>) -> Result<(), Uncertain> {
        let mut state = self.lock();
        state.map.insert(key.to_owned(), Value::Object(result));
        if state.map.len() > 300 {
            // `float(pair[1].get("ts") or 0)`: un valor que el Python no
            // ordena (no es objeto, `ts` no numérico) lanza tras guardar y
            // antes de escribir; un NaN da un orden que aquí no se repite.
            let mut keyed = Vec::with_capacity(state.map.len());
            for (k, v) in &state.map {
                let Some(ts) = sort_key(v) else {
                    state.uncertain = true;
                    return Err(Uncertain);
                };
                keyed.push((ts, k.clone()));
            }
            // `sorted` estable con `<` (sin NaN: -0.0 y 0.0 empatan).
            keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let drop = keyed.len() - 200;
            for (_, old) in keyed.into_iter().take(drop) {
                // `dict.pop`: el resto conserva su orden.
                state.map.shift_remove(&old);
            }
        }
        if state.uncertain {
            return Err(Uncertain);
        }
        let snapshot = Value::Object(state.map.clone());
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = files::write_json_atomic(&self.path, &snapshot);
        Ok(())
    }
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
