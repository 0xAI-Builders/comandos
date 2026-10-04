//! `usage_capture` y `usage_lifecycle` del bash: mismos payloads que construye con
//! jq, aplicados a la base de uso (`comandos_store::usage`) por el proceso de
//! entrega desacoplado. Como el `cc_usage.py ... &` del bash, el hook nunca espera
//! a SQLite (la base es compartida y puede estar bloqueada).
use serde_json::{Map, Number, Value, json};
use std::path::{Path, PathBuf};

/// Identidad común de los dos payloads.
pub struct Identity {
    pub agent: String,
    pub session: String,
    pub pane: String,
    pub cwd: String,
    pub prompt_id: String,
    pub agent_session_id: String,
    pub now: i64,
    pub now_ms: i64,
}

/// `usage_num`: el valor de la variable si solo tiene dígitos y puntos; si no, `0`.
fn usage_num(name: &str) -> String {
    let value = std::env::var_os(name)
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        "0".into()
    } else {
        value
    }
}

/// `--argjson` de jq sobre un `usage_num`: `None` si jq lo rechazaría.
fn as_json_number(text: &str) -> Option<Value> {
    if text.matches('.').count() > 1 || !text.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    let float: f64 = if text.ends_with('.') {
        format!("{text}0")
    } else if text.starts_with('.') {
        format!("0{text}")
    } else {
        text.into()
    }
    .parse()
    .ok()?;
    // jq lo imprime como doble: los enteros salen sin decimales.
    if float.fract() == 0.0 && float.abs() < 9.0e18 {
        Some(Value::Number(Number::from(float as i64)))
    } else {
        Number::from_f64(float).map(Value::Number)
    }
}

/// Payload de `usage_capture`, o `None` cuando el bash no lo enviaría.
pub fn capture_payload(id: &Identity, status: &str) -> Option<Value> {
    let names = [
        "COMANDOS_USAGE_INPUT_TOKENS",
        "COMANDOS_USAGE_OUTPUT_TOKENS",
        "COMANDOS_USAGE_CACHE_READ_TOKENS",
        "COMANDOS_USAGE_CACHE_WRITE_TOKENS",
        "COMANDOS_USAGE_COST_USD",
    ];
    let numbers: Vec<String> = names.iter().map(|n| usage_num(n)).collect();
    let mut started = usage_num("COMANDOS_USAGE_TURN_STARTED_AT");
    if started == "0" {
        started = id.now.to_string();
    }
    if numbers.concat() == "00000" {
        return None;
    }
    let mut parsed = numbers
        .iter()
        .map(|n| as_json_number(n))
        .collect::<Option<Vec<_>>>()?;
    let started = as_json_number(&started)?;
    let cost = parsed.pop()?;
    let env = |n: &str| {
        std::env::var_os(n)
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let mut map = Map::new();
    for (key, value) in [
        ("provider", json!(id.agent)),
        ("agent", json!(id.agent)),
        ("tmux_session", json!(id.session)),
        ("tmux_pane", json!(id.pane)),
        ("pane_pwd", json!(id.cwd)),
        ("git_root", json!(id.cwd)),
        ("model", json!(env("COMANDOS_USAGE_MODEL"))),
        (
            "reasoning_effort",
            json!(env("COMANDOS_USAGE_REASONING_EFFORT")),
        ),
        ("input_tokens", parsed[0].clone()),
        ("output_tokens", parsed[1].clone()),
        ("cache_read_tokens", parsed[2].clone()),
        ("cache_write_tokens", parsed[3].clone()),
        ("cost_usd", cost),
        ("turn_started_at", started),
        ("turn_finished_at", json!(id.now)),
        ("source", json!(format!("hook:{status}"))),
        ("confidence", json!("exact")),
    ] {
        map.insert(key.into(), value);
    }
    Some(Value::Object(map))
}

/// Payload de `usage_lifecycle`.
pub fn lifecycle_payload(id: &Identity, status: &str) -> Value {
    json!({
        "status": status,
        "harness": id.agent,
        "tmux_session": id.session,
        "tmux_pane": id.pane,
        "prompt_id": id.prompt_id,
        "agent_session_id": id.agent_session_id,
        "source": format!("hook:{}", id.agent),
        "confidence": "exact",
        "at_ms": id.now_ms,
    })
}

/// Base de uso: `COMANDOS_USAGE_DB` si está definida (como `usage_db_path()` de
/// `cc_usage.py`); si no, la ruta por defecto de `open_usage_db`.
fn open(home: &Path) -> comandos_store::Result<rusqlite::Connection> {
    match std::env::var_os("COMANDOS_USAGE_DB").filter(|p| !p.is_empty()) {
        Some(path) => comandos_store::usage::open_usage_db_at(&PathBuf::from(path)),
        None => comandos_store::usage::open_usage_db(home),
    }
}

/// Un paso de contabilidad tal como viaja al proceso de entrega.
pub fn step(capture: Option<Value>, lifecycle: Value) -> Value {
    json!({"capture": capture, "lifecycle": lifecycle})
}

/// Aplica los pasos en orden (captura, si la hay, y luego ciclo de vida); los
/// errores se callan, como el `>/dev/null 2>&1 &` del bash.
pub fn apply(home: &Path, steps: &[Value]) {
    if steps.is_empty() {
        return;
    }
    let Ok(conn) = open(home) else { return };
    for step in steps {
        if step["capture"].is_object() {
            let _ = comandos_store::usage::capture_hook(&conn, &step["capture"]);
        }
        let _ = comandos_store::usage::lifecycle(&conn, &step["lifecycle"]);
    }
}
