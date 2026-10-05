//! Ensamblado puro del estado de uso de `bin/cc_usage.py` (`build_usage_state` sin la
//! E/S) y las conversiones de Python 3.10 que lo sostienen. Sin reloj, sin disco, sin
//! red: quien llama pasa el instante, la zona local y las filas ya leídas.
//!
//! Lo que el Python lanzaría sin capturar se devuelve como `Overflow` o `Raises` (la
//! ruta responde 500); lo que no se puede reproducir con certeza (enteros fuera de
//! `i64`, dígitos Unicode, `repr` de contenedores) es `Unsure` y la ruta declina.
use crate::json::{dumps, float_repr, python_eq, truthy, workspace_loads};
use crate::text::{self, NumError};
use chrono::{Datelike, NaiveDate, NaiveDateTime, Offset, TimeZone, Timelike};
use serde_json::{Map, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageError {
    /// `OverflowError` sin capturar (`int(float('inf'))`).
    Overflow,
    /// `ValueError`/`TypeError` sin capturar.
    Raises,
    /// No reproducible con certeza: se declina.
    Unsure,
}

pub type Result<T> = std::result::Result<T, UsageError>;

type Row = Map<String, Value>;

const NULL: Value = Value::Null;

// ---------------------------------------------------------------- números de Python

/// Número de Python: `int` (acotado a `i64`) o `float`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PyNum {
    Int(i64),
    Float(f64),
}

fn float_raw(raw: &str) -> bool {
    raw.contains(['.', 'e', 'E']) || matches!(raw, "NaN" | "Infinity" | "-Infinity")
}

impl PyNum {
    /// El número de un valor JSON (`bool` cuenta como `int`); `None` si no es número.
    pub fn of(value: &Value) -> Result<Option<PyNum>> {
        Ok(Some(match value {
            Value::Bool(b) => PyNum::Int(i64::from(*b)),
            Value::Number(n) => {
                let raw = n.as_str();
                if float_raw(raw) {
                    PyNum::Float(match raw {
                        "NaN" => f64::NAN,
                        "Infinity" => f64::INFINITY,
                        "-Infinity" => f64::NEG_INFINITY,
                        _ => raw.parse().map_err(|_| UsageError::Unsure)?,
                    })
                } else {
                    PyNum::Int(raw.parse().map_err(|_| UsageError::Unsure)?)
                }
            }
            _ => return Ok(None),
        }))
    }

    pub fn to_value(self) -> Value {
        match self {
            PyNum::Int(n) => Value::from(n),
            PyNum::Float(x) => float_value(x),
        }
    }

    pub fn as_f64(self) -> f64 {
        match self {
            PyNum::Int(n) => n as f64,
            PyNum::Float(x) => x,
        }
    }

    pub fn truthy(self) -> bool {
        match self {
            PyNum::Int(n) => n != 0,
            PyNum::Float(x) => x != 0.0,
        }
    }

    /// `a + b`.
    pub fn plus(self, other: PyNum) -> Result<PyNum> {
        match (self, other) {
            (PyNum::Int(a), PyNum::Int(b)) => {
                a.checked_add(b).map(PyNum::Int).ok_or(UsageError::Unsure)
            }
            (a, b) => Ok(PyNum::Float(exact_f64(a)? + exact_f64(b)?)),
        }
    }

    /// `a > b` con la comparación exacta de Python entre `int` y `float`.
    pub fn gt(self, other: PyNum) -> Result<bool> {
        match (self, other) {
            (PyNum::Int(a), PyNum::Int(b)) => Ok(a > b),
            (a, b) => Ok(exact_f64(a)? > exact_f64(b)?),
        }
    }
}

/// El `float` de un `int` que se convierte sin redondeo; si no, no es seguro mezclarlo.
fn exact_f64(n: PyNum) -> Result<f64> {
    match n {
        PyNum::Float(x) => Ok(x),
        PyNum::Int(i) if i.unsigned_abs() <= 1 << 53 => Ok(i as f64),
        PyNum::Int(_) => Err(UsageError::Unsure),
    }
}

/// Un `float` de Python como valor JSON (`Infinity`/`NaN` como los escribe `json.dumps`).
pub fn float_value(x: f64) -> Value {
    if x.is_finite() {
        return Value::from(x);
    }
    let raw = if x.is_nan() {
        "NaN"
    } else if x > 0.0 {
        "Infinity"
    } else {
        "-Infinity"
    };
    workspace_loads(raw).unwrap_or(Value::Null)
}

/// `int(x)` de un `float`: `inf` → `OverflowError`, `nan` → `ValueError`.
fn trunc_float(x: f64) -> Result<i64> {
    if x.is_nan() {
        return Err(UsageError::Raises);
    }
    if x.is_infinite() {
        return Err(UsageError::Overflow);
    }
    let t = x.trunc();
    let limit = 2f64.powi(63);
    if (-limit..limit).contains(&t) {
        Ok(t as i64)
    } else {
        Err(UsageError::Unsure)
    }
}

/// `round(x, n)` de Python para `float` (redondeo decimal correcto, mitad al par).
pub fn round_float(x: f64, digits: usize) -> f64 {
    crate::allocation::round_digits(x, digits)
}

/// `int(value)` sin capturar nada (`int(raw.get("pid") or 0)`).
fn py_int(value: &Value) -> Result<i64> {
    match PyNum::of(value)? {
        Some(PyNum::Int(n)) => Ok(n),
        Some(PyNum::Float(x)) => trunc_float(x),
        None => match value {
            Value::String(s) => match text::int(s) {
                Ok(n) => Ok(n),
                Err(NumError::Invalid) => Err(UsageError::Raises),
                Err(NumError::Exotic) => Err(UsageError::Unsure),
            },
            _ => Err(UsageError::Raises),
        },
    }
}

/// `_as_int` (cc_usage.py:3126).
pub fn as_int(value: &Value, default: i64) -> Result<i64> {
    match value {
        Value::Null => Ok(default),
        Value::String(s) if s.is_empty() => Ok(default),
        Value::String(s) => match text::int(s) {
            Ok(n) => Ok(n),
            Err(NumError::Invalid) => Ok(default),
            Err(NumError::Exotic) => Err(UsageError::Unsure),
        },
        Value::Array(_) | Value::Object(_) => Ok(default),
        _ => match PyNum::of(value)? {
            Some(PyNum::Int(n)) => Ok(n),
            Some(PyNum::Float(x)) if x.is_nan() => Ok(default),
            Some(PyNum::Float(x)) => trunc_float(x),
            None => Ok(default),
        },
    }
}

/// `_as_float` (cc_usage.py:3135).
pub fn as_float(value: &Value, default: f64) -> Result<f64> {
    match value {
        Value::Null => Ok(default),
        Value::String(s) if s.is_empty() => Ok(default),
        Value::String(s) => match text::float(s) {
            Ok(x) => Ok(x),
            Err(NumError::Invalid) => Ok(default),
            Err(NumError::Exotic) => Err(UsageError::Unsure),
        },
        Value::Array(_) | Value::Object(_) => Ok(default),
        _ => match PyNum::of(value)? {
            // `float(int)` redondea al par más cercano, como `as f64`.
            Some(n) => Ok(n.as_f64()),
            None => Ok(default),
        },
    }
}

/// `_text` (cc_usage.py:3172): `str(value)` con `None` → `""`. El `repr` de listas y
/// diccionarios no se reproduce: `Unsure`.
pub fn text(value: &Value) -> Result<String> {
    match value {
        Value::Null => Ok(String::new()),
        Value::String(s) => Ok(s.clone()),
        Value::Bool(true) => Ok("True".into()),
        Value::Bool(false) => Ok("False".into()),
        Value::Number(_) => match PyNum::of(value)? {
            Some(PyNum::Int(n)) => Ok(n.to_string()),
            Some(PyNum::Float(x)) => Ok(float_repr(x)),
            None => Err(UsageError::Unsure),
        },
        Value::Array(_) | Value::Object(_) => Err(UsageError::Unsure),
    }
}

/// `str(value or "")`.
fn text_or_empty(value: &Value) -> Result<String> {
    if truthy(value) {
        text(value)
    } else {
        Ok(String::new())
    }
}

fn take_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `_clean_str` (cc_usage.py:446): `str(value or "")[:limit]`.
fn clean_str(value: &Value, limit: usize) -> Result<String> {
    Ok(take_chars(&text_or_empty(value)?, limit))
}

/// `_real_model` (cc_usage.py:450): modelo utilizable o `""` (sin `<synthetic>` ni flags).
pub fn real_model(value: &Value) -> Result<String> {
    let name = take_chars(text::strip(&text_or_empty(value)?), 120);
    if name.is_empty() || name.starts_with(['<', '-']) {
        return Ok(String::new());
    }
    Ok(name)
}

/// `a or b` de Python sobre valores JSON.
fn or<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    if truthy(a) { a } else { b }
}

fn get<'a>(row: &'a Row, key: &str) -> &'a Value {
    row.get(key).unwrap_or(&NULL)
}

// ---------------------------------------------------------------- fechas de Python 3.10

/// Zona local de `datetime.fromtimestamp`/`datetime.timestamp`: el desfase UTC (en
/// segundos) en cada instante. Lo demás sigue el algoritmo de CPython (`_mktime`).
pub trait LocalZone {
    /// Desfase local − UTC en `epoch`; `None` si no se puede calcular.
    fn offset_at(&self, epoch: i64) -> Option<i64>;

    /// `naive.timestamp()` (con `fold=0`) truncado a segundos enteros.
    fn epoch_of(&self, naive: NaiveDateTime) -> Option<i64> {
        let t = naive.and_utc().timestamp();
        let s = mktime(t, false, &|u| self.offset_at(u))?;
        let us = naive.nanosecond() / 1000;
        if us == 0 {
            return Some(s);
        }
        trunc_float(s as f64 + f64::from(us) / 1e6).ok()
    }

    /// `datetime.fromtimestamp(ts).replace(hour=0, minute=0, second=0, microsecond=0).timestamp()`.
    fn day_start(&self, epoch: i64) -> Option<i64> {
        let offset = |u: i64| self.offset_at(u);
        let result = local_seconds(epoch, &offset)?;
        // Detección de `fold` de `_fromtimestamp`: segundo paso de una hora repetida.
        let probe1 = local_seconds(epoch.checked_sub(86_400)?, &offset)?;
        let trans = result - probe1 - 86_400;
        let fold = trans < 0 && local_seconds(epoch + trans, &offset)? == result;
        let midnight = result - result.rem_euclid(86_400);
        mktime(midnight, fold, &offset)
    }
}

/// `local(u)` de `_mktime`: segundos «de pared» de `time.localtime(u)`; `None` si el
/// año sale de 1..=9999 (el `ValueError` de `datetime(...)`).
fn local_seconds(u: i64, offset: &dyn Fn(i64) -> Option<i64>) -> Option<i64> {
    let wall = u.checked_add(offset(u)?)?;
    let year = chrono::DateTime::from_timestamp(wall, 0)?.year();
    (1..=9999).contains(&year).then_some(wall)
}

/// `datetime._mktime` de CPython: resuelve `t = local(u)` y elige la solución por `fold`.
fn mktime(t: i64, fold: bool, offset: &dyn Fn(i64) -> Option<i64>) -> Option<i64> {
    let local = |u: i64| local_seconds(u, offset);
    let a = local(t)? - t;
    let u1 = t - a;
    let t1 = local(u1)?;
    let b = if t1 == t {
        let u2 = u1 + if fold { 86_400 } else { -86_400 };
        let b = local(u2)? - u2;
        if a == b {
            return Some(u1);
        }
        b
    } else {
        t1 - u1
    };
    let u2 = t - b;
    let t2 = local(u2)?;
    if t2 == t {
        return Some(u2);
    }
    if t1 == t {
        return Some(u1);
    }
    // `t` cae en un hueco: `max` con `fold=0`, `min` con `fold=1`.
    Some(if fold { u1.min(u2) } else { u1.max(u2) })
}

impl LocalZone for chrono::Local {
    fn offset_at(&self, epoch: i64) -> Option<i64> {
        let utc = chrono::DateTime::from_timestamp(epoch, 0)?.naive_utc();
        Some(i64::from(
            self.offset_from_utc_datetime(&utc).fix().local_minus_utc(),
        ))
    }
}

impl LocalZone for chrono_tz::Tz {
    fn offset_at(&self, epoch: i64) -> Option<i64> {
        let utc = chrono::DateTime::from_timestamp(epoch, 0)?.naive_utc();
        Some(i64::from(
            self.offset_from_utc_datetime(&utc).fix().local_minus_utc(),
        ))
    }
}

/// Byte `i` como lo lee el C de CPython: `\0` al final de la cadena.
fn at(b: &[u8], i: usize) -> u8 {
    b.get(i).copied().unwrap_or(0)
}

/// `parse_digits` de `_datetimemodule.c`.
fn parse_digits(b: &[u8], p: usize, n: usize) -> Option<(usize, u32)> {
    let mut value = 0u32;
    for k in 0..n {
        let c = at(b, p + k);
        if !c.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u32::from(c - b'0');
    }
    Some((p + n, value))
}

/// `parse_hh_mm_ss_ff`: código de retorno (<0 error, 0 fin, 1 queda texto) y valores.
fn parse_hh_mm_ss_ff(b: &[u8], start: usize, end: usize) -> (i32, [u32; 4]) {
    let mut vals = [0u32; 4];
    let mut p = start;
    for slot in vals.iter_mut().take(3) {
        let Some((next, v)) = parse_digits(b, p, 2) else {
            return (-3, vals);
        };
        *slot = v;
        let c = at(b, next);
        p = next + 1;
        if p >= end {
            return (i32::from(c != 0), vals);
        } else if c == b':' {
            continue;
        } else if c == b'.' {
            break;
        } else {
            return (-4, vals);
        }
    }
    // Sin `.` tras SS el C sigue igual: lee la fracción tras el tercer separador.
    let remains = end - p;
    if remains != 6 && remains != 3 {
        return (-3, vals);
    }
    let Some((next, mut us)) = parse_digits(b, p, remains) else {
        return (-3, vals);
    };
    if remains == 3 {
        us *= 1000;
    }
    vals[3] = us;
    (i32::from(at(b, next) != 0), vals)
}

/// Resultado de `datetime.fromisoformat` de Python 3.10 sin validar rangos.
struct IsoParts {
    date: (i32, u32, u32),
    time: [u32; 4],
    /// Desfase en microsegundos, si hay zona.
    offset_us: Option<i64>,
}

fn parse_iso(s: &str) -> Option<IsoParts> {
    let b = s.as_bytes();
    let (p, year) = parse_digits(b, 0, 4)?;
    if at(b, p) != b'-' {
        return None;
    }
    let (p, month) = parse_digits(b, p + 1, 2)?;
    if at(b, p) != b'-' {
        return None;
    }
    let (_, day) = parse_digits(b, p + 1, 2)?;
    let mut parts = IsoParts {
        date: (i32::try_from(year).ok()?, month, day),
        time: [0; 4],
        offset_us: None,
    };
    let len = b.len();
    if len <= 10 {
        return Some(parts);
    }
    // Un separador cualquiera: el ancho UTF-8 del carácter en la posición 10.
    let lead = at(b, 10);
    let skip = if lead & 0x80 == 0 {
        1
    } else {
        match lead & 0xf0 {
            0xe0 => 3,
            0xf0 => 4,
            _ => 2,
        }
    };
    let start = 10 + skip;
    if start > len {
        return None;
    }
    let mut tz = start;
    loop {
        if matches!(at(b, tz), b'+' | b'-') {
            break;
        }
        tz += 1;
        if tz >= len {
            break;
        }
    }
    let (rv, time) = parse_hh_mm_ss_ff(b, start, tz);
    if rv < 0 {
        return None;
    }
    parts.time = time;
    if tz == len {
        return (rv == 0).then_some(parts);
    }
    if !matches!(len - tz, 6 | 9 | 16) {
        return None;
    }
    let sign: i64 = if at(b, tz) == b'-' { -1 } else { 1 };
    let (rv, zone) = parse_hh_mm_ss_ff(b, tz + 1, len);
    if rv != 0 {
        return None;
    }
    let [h, m, sec, us] = zone.map(i64::from);
    let seconds = h * 3600 + m * 60 + sec;
    // `tzinfo_from_isoformat_results`: con 0 segundos de desfase el C devuelve UTC
    // e ignora los microsegundos (`+00:00:00.5` es `+00:00`).
    parts.offset_us = Some(if seconds == 0 {
        0
    } else {
        sign * (seconds * 1_000_000 + us)
    });
    Some(parts)
}

/// `int(datetime.fromisoformat(s).timestamp())` de Python 3.10; `None` donde el Python
/// lanza `ValueError` (forma no admitida, campo fuera de rango, año local fuera de rango).
pub fn fromisoformat_epoch(s: &str, zone: &dyn LocalZone) -> Option<i64> {
    let parts = parse_iso(s)?;
    let (year, month, day) = parts.date;
    let [hour, minute, second, us] = parts.time;
    if !(1..=9999).contains(&year) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let naive =
        NaiveDate::from_ymd_opt(year, month, day)?.and_hms_micro_opt(hour, minute, second, us)?;
    match parts.offset_us {
        None => zone.epoch_of(naive),
        Some(offset) => {
            if offset.unsigned_abs() >= 86_400 * 1_000_000 {
                return None;
            }
            let wall_us = i128::from(naive.and_utc().timestamp()) * 1_000_000 + i128::from(us);
            let total = wall_us - i128::from(offset);
            // `timedelta.total_seconds()`: el cociente entero / 10**6 correctamente redondeado.
            let (q, r) = (total / 1_000_000, (total % 1_000_000).abs());
            let sign = if total < 0 { "-" } else { "" };
            let seconds: f64 = format!("{sign}{}.{r:06}", q.abs()).parse().ok()?;
            trunc_float(seconds).ok()
        }
    }
}

/// `_iso_epoch` (cc_usage.py:1928): 0 si `fromisoformat` falla.
pub fn iso_epoch(s: &str, zone: &dyn LocalZone) -> i64 {
    fromisoformat_epoch(s, zone).unwrap_or(0)
}

/// `_as_epoch` (cc_usage.py:3144).
pub fn as_epoch(value: &Value, zone: &dyn LocalZone) -> Result<i64> {
    match value {
        Value::Bool(_) | Value::Number(_) => match PyNum::of(value)? {
            Some(PyNum::Int(n)) => Ok(n),
            Some(PyNum::Float(x)) => trunc_float(x),
            None => Err(UsageError::Unsure),
        },
        _ if !truthy(value) => Ok(0),
        // `str()` de un contenedor nunca es un número ni una fecha ISO.
        Value::Array(_) | Value::Object(_) => Ok(0),
        Value::String(s) => {
            match text::float(s) {
                Ok(x) if x.is_nan() => {}
                Ok(x) => return trunc_float(x),
                Err(NumError::Invalid) => {}
                Err(NumError::Exotic) => return Err(UsageError::Unsure),
            }
            let iso = match s.strip_suffix('Z') {
                Some(head) => format!("{head}+00:00"),
                None => s.clone(),
            };
            Ok(iso_epoch(&iso, zone))
        }
        _ => Ok(0),
    }
}

// ---------------------------------------------------------------- identidad de panes

fn provider_for_agent(agent: &str) -> String {
    match agent {
        "codex" => "codex".into(),
        "claude" => "claude".into(),
        "" => "unknown".into(),
        other => other.into(),
    }
}

/// `normalize_pane_identity` (cc_usage.py:469).
pub fn normalize_pane_identity(
    raw: &Value,
    labels: &HashMap<String, String>,
    now: i64,
) -> Result<Row> {
    let Value::Object(fields) = raw else {
        return Err(UsageError::Unsure);
    };
    let session = clean_str(get(fields, "session"), 80)?;
    let pane = clean_str(get(fields, "pane"), 32)?;
    let pane_pwd = clean_str(or(get(fields, "cwd"), get(fields, "pane_pwd")), 1000)?;
    let claude = Value::String("claude".into());
    let agent = clean_str(or(get(fields, "agent"), &claude), 32)?;
    let provider = match get(fields, "provider") {
        v if truthy(v) => clean_str(v, 32)?,
        _ => take_chars(&provider_for_agent(&agent), 32),
    };
    let pwd_value = Value::String(pane_pwd.clone());
    let git_root = clean_str(or(get(fields, "git_root"), &pwd_value), 1000)?;
    let label = labels
        .get(&session)
        .filter(|l| !l.is_empty())
        .map(|l| Value::String(l.clone()));
    let tab_label = match label {
        Some(l) => clean_str(&l, 120)?,
        None => clean_str(get(fields, "tab_label"), 120)?,
    };
    let zero = Value::from(0);
    let pid = py_int(or(or(get(fields, "pid"), get(fields, "agent_pid")), &zero))?;
    let ts = Value::from(now);
    let started = py_int(or(get(fields, "started_at"), &ts))?;
    let raw_json = dumps(raw, true, false).map_err(|_| UsageError::Unsure)?;
    let mut out = Row::new();
    out.insert("tmux_session".into(), session.into());
    out.insert("tmux_pane".into(), pane.into());
    out.insert("pane_pwd".into(), pane_pwd.into());
    out.insert("git_root".into(), git_root.into());
    out.insert("tab_label".into(), tab_label.into());
    out.insert("agent".into(), agent.into());
    out.insert("provider".into(), provider.into());
    out.insert("agent_pid".into(), pid.into());
    out.insert("model".into(), real_model(get(fields, "model"))?.into());
    out.insert(
        "reasoning_effort".into(),
        clean_str(get(fields, "reasoning_effort"), 40)?.into(),
    );
    out.insert("started_at".into(), started.into());
    out.insert("last_seen_at".into(), now.into());
    out.insert("raw".into(), raw_json.into());
    Ok(out)
}

// ---------------------------------------------------------------- build_usage_state

/// Clave de diccionario de Python: igualdad numérica de Python (`1 == 1.0 == True`).
trait PyKey: Clone {
    fn same(&self, other: &Self) -> bool;
}

impl PyKey for Value {
    fn same(&self, other: &Self) -> bool {
        python_eq(self, other)
    }
}

impl PyKey for (Value, Value) {
    fn same(&self, other: &Self) -> bool {
        python_eq(&self.0, &other.0) && python_eq(&self.1, &other.1)
    }
}

/// Diccionario de Python en orden de inserción. Los conjuntos de aquí son pequeños
/// (panes, proveedores, proyectos), así que basta una búsqueda lineal.
struct PyDict<K, V> {
    entries: Vec<(K, V)>,
}

impl<K: PyKey, V> PyDict<K, V> {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
    fn position(&self, key: &K) -> Option<usize> {
        self.entries.iter().position(|(k, _)| k.same(key))
    }
    fn contains(&self, key: &K) -> bool {
        self.position(key).is_some()
    }
    fn get(&self, key: &K) -> Option<&V> {
        self.entries
            .iter()
            .find(|(k, _)| k.same(key))
            .map(|(_, v)| v)
    }
    fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.entries
            .iter_mut()
            .find(|(k, _)| k.same(key))
            .map(|(_, v)| v)
    }
    /// `f(d.setdefault(key, make()))`.
    fn with_entry<R>(
        &mut self,
        key: &K,
        make: impl FnOnce() -> V,
        f: impl FnOnce(&mut V) -> R,
    ) -> R {
        if let Some(v) = self.get_mut(key) {
            return f(v);
        }
        let mut v = make();
        let out = f(&mut v);
        self.entries.push((key.clone(), v));
        out
    }
    /// `d[key] = value` (conserva la posición si ya existía).
    fn set(&mut self, key: &K, value: V) {
        match self.get_mut(key) {
            Some(slot) => *slot = value,
            None => self.entries.push((key.clone(), value)),
        }
    }
}

/// Suma de Python que empieza en el entero `0`: vacía sigue siendo `int`.
#[derive(Clone, Copy)]
struct FloatSum(Option<f64>);

impl FloatSum {
    /// `sum()` de Python empieza en el entero 0: `0 + -0.0` es `0.0`.
    fn add(&mut self, x: f64) {
        self.0 = Some(self.0.map_or(0.0 + x, |s| s + x));
    }
    fn gt_zero(self) -> bool {
        self.0.is_some_and(|x| x > 0.0)
    }
    /// `round(total, 6)`: `round(0, 6)` del entero devuelve `0`.
    fn rounded(self) -> Value {
        match self.0 {
            None => Value::from(0),
            Some(x) => float_value(round_float(x, 6)),
        }
    }
}

fn checked_add(a: i64, b: i64) -> Result<i64> {
    a.checked_add(b).ok_or(UsageError::Unsure)
}

/// `pane.get("provider") or pane.get("agent") or default`.
fn provider_of(row: &Row, default: &str) -> Value {
    let v = or(get(row, "provider"), get(row, "agent"));
    if truthy(v) {
        v.clone()
    } else {
        Value::String(default.into())
    }
}

fn or_str(value: &Value, default: &str) -> Value {
    if truthy(value) {
        value.clone()
    } else {
        Value::String(default.into())
    }
}

/// Columnas de texto de los turnos de `build_usage_state` que se guardan internadas.
const TURN_TEXT: [&str; 8] = [
    "tmux_session",
    "tmux_pane",
    "pane_pwd",
    "git_root",
    "agent",
    "provider",
    "model",
    "confidence",
];
/// Clave ausente en la fila (distinta de `null`: `turn.get(k, "")`).
const ABSENT: u32 = u32::MAX;

/// Un turno compacto (64 B): las columnas de texto como índices de una tabla de
/// valores distintos y las tres numéricas ya convertidas como las convierte cada
/// uso (`_as_int(…, 0)`, `_as_float(…, 0.0)`); su error, si lo hay, va aparte en
/// `faults` (finalizado, tokens, coste).
struct Turn {
    text: [u32; 8],
    finished: i64,
    tokens: i64,
    cost: f64,
    faults: [Option<UsageError>; 3],
}

impl Turn {
    fn new(text: [u32; 8], finished: Result<i64>, tokens: Result<i64>, cost: Result<f64>) -> Self {
        Self {
            text,
            finished: *finished.as_ref().unwrap_or(&0),
            tokens: *tokens.as_ref().unwrap_or(&0),
            cost: *cost.as_ref().unwrap_or(&0.0),
            faults: [finished.err(), tokens.err(), cost.err()],
        }
    }
}

/// Una columna de texto de un turno tal como sale de SQLite: un texto prestado
/// (se interna sin copiarlo si ya está) o cualquier otro valor.
pub enum TurnCell<'a> {
    Text(&'a str),
    Value(Value),
}

/// Los turnos de 14 días de `build_usage_state` sin guardar cada fila como objeto
/// JSON (decenas de miles con la base real): las columnas de texto se repiten mucho
/// (sesión, pane, carpeta, agente, modelo) y se internan; cada turno ocupa ~80 B.
/// El orden de los turnos es el de lectura.
#[derive(Default)]
pub struct StateTurns {
    values: Vec<Value>,
    strings: HashMap<String, u32>,
    /// Una sola casilla para `null` y otra por número o booleano distinto (por su
    /// texto): una columna casi vacía no repite el valor en cada turno.
    null: Option<u32>,
    scalars: HashMap<String, u32>,
    /// En trozos de `TURN_CHUNK` (64 KiB): un solo vector de 5 MiB lo sirve
    /// glibc con `mmap` y, al soltarlo, sube su umbral de `mmap` y el de
    /// recorte a ese tamaño; desde entonces las arenas no se recortan.
    turns: Vec<Vec<Turn>>,
    len: usize,
}

/// Turnos por trozo: 1024 × 64 B = 64 KiB, bajo el umbral de `mmap` de glibc.
const TURN_CHUNK: usize = 1024;

impl StateTurns {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_rows(rows: &[Row]) -> Self {
        let mut turns = Self::new();
        for row in rows {
            turns.push(row);
        }
        turns
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Un turno leído columna a columna, sin el objeto de la fila: las ocho de
    /// texto en el orden de `TURN_TEXT` y las tres numéricas. Lo mismo que
    /// `push` de la fila con esas once claves.
    pub fn push_cells(
        &mut self,
        text: [TurnCell<'_>; 8],
        finished: &Value,
        tokens: &Value,
        cost: &Value,
    ) {
        let mut ids = [ABSENT; 8];
        for (slot, cell) in ids.iter_mut().zip(text) {
            *slot = match cell {
                TurnCell::Text(s) => self.intern_str(s),
                TurnCell::Value(value) => self.intern(&value),
            };
        }
        self.push_turn(Turn::new(
            ids,
            as_int(finished, 0),
            as_int(tokens, 0),
            as_float(cost, 0.0),
        ));
    }

    /// Fin de la lectura: suelta los índices de internado y el sitio sobrante.
    pub fn finish(&mut self) {
        self.strings = HashMap::new();
        self.scalars = HashMap::new();
        self.values.shrink_to_fit();
        self.turns.shrink_to_fit();
        if let Some(last) = self.turns.last_mut() {
            last.shrink_to_fit();
        }
    }

    fn push_turn(&mut self, turn: Turn) {
        match self.turns.last_mut() {
            Some(chunk) if chunk.len() < TURN_CHUNK => chunk.push(turn),
            _ => {
                let mut chunk = Vec::with_capacity(TURN_CHUNK);
                chunk.push(turn);
                self.turns.push(chunk);
            }
        }
        self.len += 1;
    }

    fn intern_str(&mut self, s: &str) -> u32 {
        if let Some(&i) = self.strings.get(s) {
            return i;
        }
        self.intern(&Value::String(s.to_owned()))
    }

    /// Un turno leído (`dict(sqlite3.Row)`); solo se quedan las columnas que usa
    /// `build_usage_state`.
    pub fn push(&mut self, row: &Row) {
        let mut text = [ABSENT; 8];
        for (slot, key) in text.iter_mut().zip(TURN_TEXT) {
            if let Some(value) = row.get(key) {
                *slot = self.intern(value);
            }
        }
        self.push_turn(Turn::new(
            text,
            as_int(get(row, "turn_finished_at"), 0),
            as_int(get(row, "total_tokens"), 0),
            as_float(get(row, "cost_usd"), 0.0),
        ));
    }

    fn intern(&mut self, value: &Value) -> u32 {
        // Más de 2³² - 1 valores distintos no caben en memoria antes que en `u32`.
        let next = u32::try_from(self.values.len()).unwrap_or(ABSENT);
        match value {
            Value::String(s) => {
                if let Some(&i) = self.strings.get(s) {
                    return i;
                }
                self.strings.insert(s.clone(), next);
            }
            Value::Null => {
                if let Some(i) = self.null {
                    return i;
                }
                self.null = Some(next);
            }
            // El texto JSON identifica el valor (mismo texto, mismo `Value`).
            Value::Bool(_) | Value::Number(_) => {
                let key = value.to_string();
                if let Some(&i) = self.scalars.get(&key) {
                    return i;
                }
                self.scalars.insert(key, next);
            }
            // Una columna de SQLite nunca da un contenedor: no se comparte.
            Value::Array(_) | Value::Object(_) => {}
        }
        self.values.push(value.clone());
        next
    }

    fn iter(&self) -> impl Iterator<Item = TurnRef<'_>> {
        self.turns
            .iter()
            .flatten()
            .map(move |turn| TurnRef { store: self, turn })
    }
}

#[derive(Clone, Copy)]
struct TurnRef<'a> {
    store: &'a StateTurns,
    turn: &'a Turn,
}

impl<'a> TurnRef<'a> {
    fn slot(&self, key: &str) -> Option<&'a Value> {
        let i = TURN_TEXT.iter().position(|k| *k == key)?;
        let id = *self.turn.text.get(i)?;
        self.store.values.get(usize::try_from(id).ok()?)
    }
    /// `turn.get(key)` (`None` si falta).
    fn get(&self, key: &str) -> &'a Value {
        self.slot(key).unwrap_or(&NULL)
    }
    /// `turn.get(key, "")`.
    fn field(&self, key: &str) -> Value {
        self.slot(key)
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()))
    }
    /// `_as_int(turn.get("turn_finished_at"), 0)`.
    fn finished(&self) -> Result<i64> {
        let [f, _, _] = self.turn.faults;
        f.map_or(Ok(self.turn.finished), Err)
    }
    /// `_as_int(turn.get("total_tokens"), 0)`.
    fn tokens(&self) -> Result<i64> {
        let [_, f, _] = self.turn.faults;
        f.map_or(Ok(self.turn.tokens), Err)
    }
    /// `_as_float(turn.get("cost_usd"), 0.0)`.
    fn cost(&self) -> Result<f64> {
        let [_, _, f] = self.turn.faults;
        f.map_or(Ok(self.turn.cost), Err)
    }
    /// `turn.get("provider") or turn.get("agent") or default`.
    fn provider_or(&self, default: &str) -> Value {
        let v = or(self.get("provider"), self.get("agent"));
        if truthy(v) {
            v.clone()
        } else {
            Value::String(default.into())
        }
    }
}

/// `_attach_pane_turn_usage` (cc_usage.py:823).
fn attach_pane_turn_usage(turns: &StateTurns, panes: &mut [Row], now: i64) -> Result<()> {
    let day_start = now - 24 * 3600;
    let mut groups: PyDict<(Value, Value), Vec<usize>> = PyDict::new();
    for (i, pane) in panes.iter().enumerate() {
        let key = (provider_of(pane, ""), or_str(get(pane, "pane_pwd"), ""));
        groups.with_entry(&key, Vec::new, |g| g.push(i));
    }
    let mut sums: PyDict<(Value, Value), (i64, f64)> = PyDict::new();
    let mut latest_model: PyDict<(Value, Value), String> = PyDict::new();
    let mut day_model_tokens: PyDict<(Value, Value), Vec<(String, i64)>> = PyDict::new();
    for turn in turns.iter() {
        let key = (turn.provider_or(""), or_str(turn.get("pane_pwd"), ""));
        if !groups.contains(&key) {
            continue;
        }
        let turn_model = real_model(turn.get("model"))?;
        if !turn_model.is_empty() && !latest_model.contains(&key) {
            latest_model.set(&key, turn_model.clone());
        }
        if turn.finished()? < day_start {
            continue;
        }
        if !turn_model.is_empty() {
            let weight = turn.tokens()?.max(1);
            day_model_tokens.with_entry(&key, Vec::new, |mt| {
                match mt.iter_mut().find(|(m, _)| *m == turn_model) {
                    Some((_, n)) => *n = checked_add(*n, weight)?,
                    None => mt.push((turn_model, weight)),
                }
                Ok::<(), UsageError>(())
            })?;
        }
        let tokens = turn.tokens()?;
        let cost = turn.cost()?;
        sums.with_entry(
            &key,
            || (0, 0.0),
            |item| {
                item.0 = checked_add(item.0, tokens)?;
                item.1 += cost;
                Ok::<(), UsageError>(())
            },
        )?;
    }
    // Modelo dominante del día: el primero con el máximo, como `max(mt.items(), key=…)`.
    for (key, mt) in &day_model_tokens.entries {
        let mut best: Option<&(String, i64)> = None;
        for entry in mt {
            if best.is_none_or(|b| entry.1 > b.1) {
                best = Some(entry);
            }
        }
        if let Some((model, _)) = best {
            latest_model.set(key, model.clone());
        }
    }
    for (key, group) in &groups.entries {
        let item = sums.get(key).copied();
        let model = latest_model.get(key).cloned().unwrap_or_default();
        for &i in group {
            let Some(pane) = panes.get_mut(i) else {
                continue;
            };
            if !model.is_empty() && !truthy(get(pane, "model")) {
                pane.insert("model".into(), model.clone().into());
            }
            let Some((tokens, cost)) = item else {
                continue;
            };
            pane.insert("total_tokens".into(), tokens.into());
            pane.insert("cost_usd".into(), float_value(round_float(cost, 6)));
            pane.insert("usage_window".into(), "24h".into());
            let shared = if group.len() > 1 {
                "compartido"
            } else {
                "local"
            };
            pane.insert("confidence".into(), shared.into());
        }
    }
    Ok(())
}

struct Project {
    root: Value,
    cost: f64,
    tokens: i64,
    confidence: Value,
    panes: Vec<usize>,
}

/// `_project_rollups` (cc_usage.py:682). Los panes viven en un arena porque el Python
/// los comparte entre el índice y la lista de su proyecto (un turno de otra carpeta
/// actualiza el pane de la carpeta original).
fn project_rollups(turns: &StateTurns, panes: &[Row]) -> Result<Vec<Value>> {
    let mut arena: Vec<Row> = Vec::new();
    let mut projects: PyDict<Value, Project> = PyDict::new();
    let detected = Value::String("detected".into());
    let exact = Value::String("exact".into());
    for pane in panes {
        let root = or_str(or(get(pane, "git_root"), get(pane, "pane_pwd")), "");
        let confidence = or(get(pane, "confidence"), &detected).clone();
        let mut copy = pane.clone();
        copy.entry("cost_usd").or_insert_with(|| float_value(0.0));
        copy.entry("total_tokens").or_insert_with(|| Value::from(0));
        copy.entry("confidence")
            .or_insert_with(|| confidence.clone());
        arena.push(copy);
        let index = arena.len() - 1;
        projects.with_entry(
            &root,
            || Project {
                root: root.clone(),
                cost: 0.0,
                tokens: 0,
                confidence,
                panes: Vec::new(),
            },
            |p| p.panes.push(index),
        );
    }
    let mut pane_index: PyDict<(Value, Value), usize> = PyDict::new();
    for (_, project) in &projects.entries {
        for &i in &project.panes {
            if let Some(p) = arena.get(i) {
                let key = (get(p, "tmux_session").clone(), get(p, "tmux_pane").clone());
                pane_index.set(&key, i);
            }
        }
    }
    for turn in turns.iter() {
        let root = or_str(or(turn.get("git_root"), turn.get("pane_pwd")), "");
        let turn_confidence = turn.get("confidence");
        if !projects.contains(&root) {
            let project = Project {
                root: root.clone(),
                cost: 0.0,
                tokens: 0,
                confidence: or(turn_confidence, &exact).clone(),
                panes: Vec::new(),
            };
            projects.set(&root, project);
        }
        let key = (
            turn.get("tmux_session").clone(),
            turn.get("tmux_pane").clone(),
        );
        let created = match pane_index.get(&key) {
            Some(&i) => (i, false),
            None => {
                let field = |k: &str| turn.field(k);
                let mut pane = Row::new();
                pane.insert("tmux_session".into(), field("tmux_session"));
                pane.insert("tmux_pane".into(), field("tmux_pane"));
                pane.insert("pane_pwd".into(), field("pane_pwd"));
                pane.insert("git_root".into(), root.clone());
                pane.insert("agent".into(), field("agent"));
                pane.insert("provider".into(), field("provider"));
                pane.insert("model".into(), field("model"));
                pane.insert("cost_usd".into(), float_value(0.0));
                pane.insert("total_tokens".into(), 0.into());
                pane.insert("confidence".into(), or(turn_confidence, &exact).clone());
                arena.push(pane);
                let i = arena.len() - 1;
                pane_index.set(&key, i);
                (i, true)
            }
        };
        let (index, is_new) = created;
        let cost = turn.cost()?;
        let tokens = turn.tokens()?;
        if let Some(pane) = arena.get_mut(index) {
            let pane_cost = as_float(get(pane, "cost_usd"), 0.0)?;
            let pane_tokens = as_int(get(pane, "total_tokens"), 0)?;
            pane.insert(
                "cost_usd".into(),
                float_value(round_float(pane_cost + cost, 6)),
            );
            pane.insert(
                "total_tokens".into(),
                checked_add(pane_tokens, tokens)?.into(),
            );
            let confidence = or(or(turn_confidence, get(pane, "confidence")), &exact).clone();
            pane.insert("confidence".into(), confidence);
        }
        if let Some(item) = projects.get_mut(&root) {
            if is_new {
                item.panes.push(index);
            }
            item.cost = round_float(item.cost + cost, 6);
            item.tokens = checked_add(item.tokens, tokens)?;
            item.confidence = or(or(turn_confidence, &item.confidence), &exact).clone();
        }
    }
    Ok(projects
        .entries
        .into_iter()
        .map(|(_, p)| {
            let mut out = Row::new();
            out.insert("git_root".into(), p.root);
            out.insert("cost_usd".into(), float_value(p.cost));
            out.insert("total_tokens".into(), p.tokens.into());
            out.insert("confidence".into(), p.confidence);
            let panes = p
                .panes
                .iter()
                .filter_map(|&i| arena.get(i).cloned().map(Value::Object))
                .collect();
            out.insert("panes".into(), Value::Array(panes));
            Value::Object(out)
        })
        .collect())
}

/// `_setting_float` / `_setting_int`: la primera clave con valor no vacío.
fn setting<'a>(settings: &'a Row, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .filter_map(|k| settings.get(*k))
        .find(|v| !matches!(v, Value::Null) && v.as_str() != Some(""))
}

/// `_token_window` (cc_usage.py:759).
fn token_window(
    turns: &StateTurns,
    provider: &str,
    label: &str,
    window: &str,
    start: i64,
    limit: i64,
) -> Result<Value> {
    let aliases: &[&str] = match provider {
        "codex" => &["codex", "openai"],
        "claude" => &["claude", "anthropic"],
        other => &[other],
    };
    let mut used = 0i64;
    for t in turns.iter() {
        let who = or(t.get("provider"), t.get("agent"));
        let member = who.as_str().is_some_and(|w| aliases.contains(&w));
        if member && t.finished()? >= start {
            used = checked_add(used, t.tokens()?)?;
        }
    }
    let mut item = Row::new();
    item.insert("id".into(), format!("{provider}_{window}_tokens").into());
    item.insert("provider".into(), provider.into());
    item.insert("label".into(), label.into());
    item.insert("window".into(), window.into());
    item.insert("metric".into(), "tokens".into());
    item.insert("used".into(), used.into());
    item.insert("limit".into(), limit.into());
    item.insert("source".into(), "local_usage".into());
    if limit > 0 {
        let remaining = limit.checked_sub(used).ok_or(UsageError::Unsure)?.max(0);
        let ratio = exact_f64(PyNum::Int(used))? / exact_f64(PyNum::Int(limit))?;
        let rounded = round_float(ratio * 100.0, 1);
        // `min(100, r)` devuelve el entero 100 salvo que `r` sea menor.
        let percent = if rounded < 100.0 {
            float_value(rounded)
        } else {
            Value::from(100)
        };
        item.insert("status".into(), "configured".into());
        item.insert("remaining".into(), remaining.into());
        item.insert("percent".into(), percent);
    } else {
        item.insert("status".into(), "missing_limit".into());
        item.insert("remaining".into(), Value::Null);
        item.insert("percent".into(), Value::Null);
    }
    Ok(Value::Object(item))
}

/// `_usage_windows` (cc_usage.py:795).
fn usage_windows(turns: &StateTurns, settings: &Row, now: i64) -> Result<Value> {
    let day = now - 24 * 3600;
    let week = now - 7 * 24 * 3600;
    let budget = match setting(
        settings,
        &[
            "COMANDOS_DAILY_BUDGET_USD",
            "COMANDOS_USAGE_DAILY_BUDGET_USD",
        ],
    ) {
        Some(v) => as_float(v, 0.0)?,
        None => 0.0,
    };
    let limit = |keys: &[&str]| -> Result<i64> {
        match setting(settings, keys) {
            Some(v) => as_int(v, 0),
            None => Ok(0),
        }
    };
    let items = vec![
        token_window(
            turns,
            "codex",
            "Codex diario",
            "daily",
            day,
            limit(&[
                "COMANDOS_CODEX_DAILY_TOKEN_LIMIT",
                "CODEX_DAILY_TOKEN_LIMIT",
            ])?,
        )?,
        token_window(
            turns,
            "codex",
            "Codex semanal",
            "weekly",
            week,
            limit(&[
                "COMANDOS_CODEX_WEEKLY_TOKEN_LIMIT",
                "CODEX_WEEKLY_TOKEN_LIMIT",
            ])?,
        )?,
        token_window(
            turns,
            "claude",
            "Claude diario",
            "daily",
            day,
            limit(&[
                "COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT",
                "CLAUDE_DAILY_TOKEN_LIMIT",
            ])?,
        )?,
        token_window(
            turns,
            "claude",
            "Claude semanal",
            "weekly",
            week,
            limit(&[
                "COMANDOS_CLAUDE_WEEKLY_TOKEN_LIMIT",
                "CLAUDE_WEEKLY_TOKEN_LIMIT",
            ])?,
        )?,
    ];
    let mut out = Row::new();
    out.insert("daily_budget_usd".into(), float_value(budget));
    out.insert("items".into(), Value::Array(items));
    Ok(Value::Object(out))
}

fn provider_entry(provider: &Value) -> Row {
    let mut p = Row::new();
    p.insert("provider".into(), provider.clone());
    p.insert("cost_usd".into(), float_value(0.0));
    p.insert("total_tokens".into(), 0.into());
    p.insert("active_panes".into(), 0.into());
    p
}

/// `p["total_tokens"] += tokens` sobre `providers.setdefault(name, …)`.
fn add_tokens(providers: &mut PyDict<Value, Row>, name: &Value, tokens: i64) -> Result<()> {
    providers.with_entry(
        name,
        || provider_entry(name),
        |p| {
            let current = as_int(get(p, "total_tokens"), 0)?;
            p.insert("total_tokens".into(), checked_add(current, tokens)?.into());
            Ok(())
        },
    )
}

/// `row.get(key, default)`.
fn get_or(row: &Row, key: &str, default: Value) -> Value {
    row.get(key).cloned().unwrap_or(default)
}

/// `build_usage_state` (cc_usage.py:871-961) sin la E/S: el llamador ya eligió `panes`
/// (vivos o `list_panes`) y leyó los turnos de `state_rows` (`StateTurns`).
pub fn build_state(
    now: i64,
    mut panes: Vec<Row>,
    turns: &StateTurns,
    provider_usage: &[Row],
    provider_costs: &[Row],
    settings: &Row,
) -> Result<Value> {
    attach_pane_turn_usage(turns, &mut panes, now)?;
    let projects = project_rollups(turns, &panes)?;
    let mut turn_cost = FloatSum(None);
    for t in turns.iter() {
        turn_cost.add(t.cost()?);
    }
    let mut provider_cost = FloatSum(None);
    for c in provider_costs {
        provider_cost.add(as_float(get(c, "cost_usd"), 0.0)?);
    }
    let mut turn_tokens = 0i64;
    for t in turns.iter() {
        turn_tokens = checked_add(turn_tokens, t.tokens()?)?;
    }
    let mut provider_tokens = 0i64;
    for u in provider_usage {
        provider_tokens = checked_add(provider_tokens, as_int(get(u, "total_tokens"), 0)?)?;
    }
    let mut unattributed = Vec::new();
    for row in provider_costs {
        let mut item = Row::new();
        item.insert("provider".into(), get_or(row, "provider", "".into()));
        item.insert("start_time".into(), get_or(row, "start_time", 0.into()));
        item.insert("end_time".into(), get_or(row, "end_time", 0.into()));
        let cost = as_float(get(row, "cost_usd"), 0.0)?;
        item.insert("cost_usd".into(), float_value(round_float(cost, 6)));
        item.insert("currency".into(), get_or(row, "currency", "usd".into()));
        for key in [
            "project_id",
            "workspace_id",
            "api_key_id",
            "line_item",
            "model",
        ] {
            item.insert(key.into(), get_or(row, key, "".into()));
        }
        item.insert("confidence".into(), "unattributed".into());
        unattributed.push(Value::Object(item));
    }
    let mut providers: PyDict<Value, Row> = PyDict::new();
    for row in provider_costs {
        let name = get_or(row, "provider", "".into());
        let cost = as_float(get(row, "cost_usd"), 0.0)?;
        providers.with_entry(
            &name,
            || provider_entry(&name),
            |p| {
                let current = as_float(get(p, "cost_usd"), 0.0)?;
                p.insert(
                    "cost_usd".into(),
                    float_value(round_float(current + cost, 6)),
                );
                Ok::<(), UsageError>(())
            },
        )?;
    }
    for row in provider_usage {
        let name = get_or(row, "provider", "".into());
        let tokens = as_int(get(row, "total_tokens"), 0)?;
        add_tokens(&mut providers, &name, tokens)?;
    }
    if provider_usage.is_empty() {
        for turn in turns.iter() {
            let name = turn.provider_or("unknown");
            let tokens = turn.tokens()?;
            add_tokens(&mut providers, &name, tokens)?;
        }
    }
    for pane in &panes {
        let name = provider_of(pane, "unknown");
        providers.with_entry(
            &name,
            || provider_entry(&name),
            |p| {
                let current = as_int(get(p, "active_panes"), 0)?;
                p.insert("active_panes".into(), checked_add(current, 1)?.into());
                Ok::<(), UsageError>(())
            },
        )?;
    }
    let cost_total = if provider_cost.gt_zero() {
        provider_cost
    } else {
        turn_cost
    };
    let token_total = if provider_tokens > 0 {
        provider_tokens
    } else {
        turn_tokens
    };
    let mut series = Vec::new();
    for row in provider_costs {
        let mut point = Row::new();
        point.insert("ts".into(), get_or(row, "end_time", 0.into()));
        let cost = as_float(get(row, "cost_usd"), 0.0)?;
        point.insert("cost_usd".into(), float_value(round_float(cost, 6)));
        series.push(Value::Object(point));
    }
    let windows = usage_windows(turns, settings, now)?;
    let mut totals = Row::new();
    totals.insert("cost_usd".into(), cost_total.rounded());
    totals.insert("total_tokens".into(), token_total.into());
    let mut out = Row::new();
    out.insert("generated_at".into(), now.into());
    out.insert("totals".into(), Value::Object(totals));
    out.insert(
        "providers".into(),
        Value::Array(
            providers
                .entries
                .into_iter()
                .map(|(_, p)| Value::Object(p))
                .collect(),
        ),
    );
    out.insert("projects".into(), Value::Array(projects));
    out.insert(
        "panes".into(),
        Value::Array(panes.into_iter().map(Value::Object).collect()),
    );
    out.insert("unattributed".into(), Value::Array(unattributed));
    out.insert("alerts".into(), Value::Array(Vec::new()));
    out.insert("limits".into(), Value::Array(Vec::new()));
    out.insert("windows".into(), windows);
    out.insert("series".into(), Value::Array(series));
    out.insert("credential_health".into(), Value::Object(Row::new()));
    Ok(Value::Object(out))
}

/// `attach_token_counts` (cc_usage.py:964): solo rellena claves ausentes o vacías y
/// solo con valores verdaderos.
pub fn attach_token_counts(limits: &mut [Row], windows: &Value) {
    let mut by_provider: PyDict<Value, Vec<(String, Value)>> = PyDict::new();
    let items = windows
        .get("items")
        .filter(|v| truthy(v))
        .and_then(Value::as_array);
    for w in items.into_iter().flatten() {
        let Some(w) = w.as_object() else {
            continue;
        };
        let provider = get(w, "provider");
        if get(w, "metric").as_str() != Some("tokens") || !truthy(provider) {
            continue;
        }
        let key = match get(w, "window").as_str() {
            Some("weekly") => Some("tokens_7d"),
            Some("daily") => Some("tokens_today"),
            _ => None,
        };
        let used = get(w, "used").clone();
        by_provider.with_entry(provider, Vec::new, |slot| {
            let Some(key) = key else {
                return;
            };
            match slot.iter_mut().find(|(k, _)| k == key) {
                Some((_, v)) => *v = used,
                None => slot.push((key.into(), used)),
            }
        });
    }
    for row in limits.iter_mut() {
        let provider = get(row, "provider").clone();
        let Some(extra) = by_provider.get_mut(&provider) else {
            continue;
        };
        for (key, value) in extra.iter() {
            if truthy(value) && !truthy(get(row, key)) {
                row.insert(key.clone(), value.clone());
            }
        }
    }
}

/// `usage_credential_health` (`bin/cc-dash:216`).
pub fn credential_health(env: &Row) -> Value {
    let entry = |provider: &str, key: &str| {
        let configured = truthy(get(env, key));
        let mut out = Row::new();
        out.insert("provider".into(), provider.into());
        out.insert("configured".into(), configured.into());
        let status = if configured { "configured" } else { "missing" };
        out.insert("status".into(), status.into());
        Value::Object(out)
    };
    let mut out = Row::new();
    out.insert("openai".into(), entry("openai", "OPENAI_ADMIN_KEY"));
    out.insert(
        "anthropic".into(),
        entry("anthropic", "ANTHROPIC_ADMIN_KEY"),
    );
    Value::Object(out)
}

/// `_parse_env_file` (cc_usage.py:2157) sobre el texto ya leído. Las claves con algún
/// carácter no ASCII se descartan: nadie las consulta.
pub fn parse_env_text(raw: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text::splitlines(raw) {
        let mut s = text::strip(line);
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        if let Some(rest) = s.strip_prefix("export ") {
            s = text::strip(rest);
        }
        let Some((key, value)) = s.split_once('=') else {
            continue;
        };
        let key = text::strip(key);
        let mut value = text::strip(value);
        let valid = key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if key.is_empty() || !valid {
            continue;
        }
        let mut chars = value.chars();
        if let (Some(first), Some(last)) = (chars.next(), chars.next_back())
            && first == last
            && matches!(first, '\'' | '"')
        {
            value = value.get(1..value.len() - 1).unwrap_or(value);
        }
        out.push((key.to_owned(), value.to_owned()));
    }
    out
}

/// `percentile` (cc_usage.py:2751), mismas operaciones de punto flotante.
pub fn percentile(values: &[f64], q: f64) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    match sorted.len() {
        0 => return None,
        1 => return sorted.first().copied(),
        _ => {}
    }
    let upper = if q < 1.0 { q } else { 1.0 };
    let clamped = if upper > 0.0 { upper } else { 0.0 };
    let last = sorted.len() - 1;
    let position = clamped * last as f64;
    let low = position as usize;
    let high = last.min(low + 1);
    let fraction = position - low as f64;
    let lo = *sorted.get(low)?;
    let hi = *sorted.get(high)?;
    Some(lo + (hi - lo) * fraction)
}

/// `wilson_interval` (cc_usage.py:2764) con `z = 1.96`.
pub fn wilson_interval(successes: i64, total: i64) -> (f64, f64) {
    if total <= 0 {
        return (0.0, 1.0);
    }
    let z = 1.96f64;
    let n = total as f64;
    let p = successes as f64 / n;
    let den = 1.0 + z * z / n;
    let center = (p + z * z / (2 * total) as f64) / den;
    let square = 4 * i128::from(total) * i128::from(total);
    let margin = z * (p * (1.0 - p) / n + z * z / square as f64).powf(0.5) / den;
    let low = center - margin;
    let high = center + margin;
    (
        if low > 0.0 { low } else { 0.0 },
        if high < 1.0 { high } else { 1.0 },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_turns_share_null_and_scalar_slots() {
        // `git_root` casi siempre vacío y `tmux_pane` numérico: una casilla por valor.
        let rows: Vec<Row> = (0..5000)
            .map(|i| {
                let mut row = Row::new();
                row.insert("tmux_session".into(), "s".into());
                row.insert(
                    "git_root".into(),
                    if i % 100 == 0 {
                        "/r".into()
                    } else {
                        Value::Null
                    },
                );
                row.insert("tmux_pane".into(), Value::from(i % 3));
                row.insert("model".into(), Value::Bool(i % 2 == 0));
                row.insert("total_tokens".into(), Value::from(i));
                row
            })
            .collect();
        let turns = StateTurns::from_rows(&rows);
        // s, /r, null, 0, 1, 2, true, false.
        assert_eq!(turns.values.len(), 8);
        for (row, turn) in rows.iter().zip(turns.iter()) {
            for key in ["tmux_session", "git_root", "tmux_pane", "model", "agent"] {
                assert_eq!(row.get(key), turn.slot(key), "{key}");
            }
        }
    }

    #[test]
    fn push_cells_matches_push_of_the_row() {
        use serde_json::json;
        assert_eq!(std::mem::size_of::<Turn>(), 64);
        assert!(std::mem::size_of::<Turn>() * TURN_CHUNK <= 64 * 1024);
        let texts = [
            json!("s"),
            json!("%1"),
            Value::Null,
            json!(3),
            json!("/r"),
            json!(true),
            json!("m"),
            json!(""),
        ];
        let nums = [
            (json!(1_791_000_000), json!(12), json!(0.5)),
            (json!("x"), Value::Null, json!("1e400")),
            (json!(1.5e300), json!(7.9), json!(true)),
        ];
        let mut by_row = StateTurns::new();
        let mut by_cell = StateTurns::new();
        for (finished, tokens, cost) in &nums {
            let mut row = Row::new();
            for (key, value) in TURN_TEXT.iter().zip(&texts) {
                row.insert((*key).into(), value.clone());
            }
            row.insert("turn_finished_at".into(), finished.clone());
            row.insert("total_tokens".into(), tokens.clone());
            row.insert("cost_usd".into(), cost.clone());
            by_row.push(&row);
            let cells = texts.clone().map(|v| match v {
                Value::String(s) => TurnCell::Value(Value::String(s)),
                other => TurnCell::Value(other),
            });
            by_cell.push_cells(cells, finished, tokens, cost);
        }
        by_cell.push_cells(
            texts.clone().map(|v| match v {
                Value::String(s) if s == "m" => TurnCell::Text("m"),
                other => TurnCell::Value(other),
            }),
            &nums[0].0,
            &nums[0].1,
            &nums[0].2,
        );
        by_cell.finish();
        assert!(by_cell.strings.is_empty());
        for (a, b) in by_row.iter().zip(by_cell.iter()) {
            for key in TURN_TEXT {
                assert_eq!(a.slot(key), b.slot(key), "{key}");
            }
            assert_eq!(a.finished(), b.finished());
            assert_eq!(a.tokens(), b.tokens());
            assert_eq!(a.cost().map(f64::to_bits), b.cost().map(f64::to_bits));
        }
        assert_eq!(by_cell.len(), 4);
        assert_eq!(by_cell.values.len(), by_row.values.len());
    }

    #[test]
    fn float_sum_starts_at_integer_zero() {
        let mut sum = FloatSum(None);
        sum.add(-0.0);
        assert_eq!(
            crate::json::response_dumps(&sum.rounded()).as_deref(),
            Ok("0.0")
        );
        let mut sum = FloatSum(None);
        sum.add(-0.0);
        sum.add(-0.0);
        assert_eq!(
            crate::json::response_dumps(&sum.rounded()).as_deref(),
            Ok("0.0")
        );
        assert_eq!(
            crate::json::response_dumps(&FloatSum(None).rounded()).as_deref(),
            Ok("0")
        );
    }
}
