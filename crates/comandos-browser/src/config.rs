//! Configuración validada; cargarla no arranca trabajadores ni crea estado.
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fmt, fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct ConfigError(String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ConfigError {}

fn invalid(field: &str) -> ConfigError {
    ConfigError(format!("Configuración de broker inválida: {field}"))
}

#[derive(Debug)]
pub struct BrokerConfig {
    pub command: Vec<String>,
    pub env: Vec<(String, String)>,
    pub catalog: Value,
    pub tools: BTreeSet<String>,
    pub state_dir: PathBuf,
    pub port: u16,
    pub max_workers: usize,
    pub idle_seconds: f64,
    pub tool_timeout: f64,
    pub queue_timeout: f64,
    pub stop_grace: f64,
    pub python_status: Option<PathBuf>,
}

fn read_json(path: &Path) -> Result<Value, ConfigError> {
    let bytes =
        fs::read(path).map_err(|error| ConfigError(format!("{}: {error}", path.display())))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| ConfigError(format!("{}: {error}", path.display())))
}

fn timer(value: &Value, key: &str, default: f64) -> Result<f64, ConfigError> {
    match value.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| invalid(key)),
    }
}

impl BrokerConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        Self::from_value(
            read_json(path)?,
            path.parent().unwrap_or_else(|| Path::new(".")),
        )
    }

    pub fn from_value(value: Value, base: &Path) -> Result<Self, ConfigError> {
        let command = value
            .get("command")
            .and_then(Value::as_array)
            .filter(|items| !items.is_empty())
            .ok_or_else(|| invalid("command"))?
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| invalid("command"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let env = match value.get("env") {
            None => Vec::new(),
            Some(env) => env
                .as_object()
                .ok_or_else(|| invalid("env"))?
                .iter()
                .map(|(key, val)| {
                    val.as_str()
                        .map(|val| (key.clone(), val.to_owned()))
                        .ok_or_else(|| invalid("env"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        let catalog = match value.get("catalog") {
            Some(Value::String(path)) => read_json(&base.join(path))?,
            Some(value) if value.is_object() => value.clone(),
            _ => return Err(invalid("catalog")),
        };
        let tools = catalog
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("catalog.tools"))?
            .iter()
            .map(|tool| {
                tool.get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| invalid("catalog.tools.name"))
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        let state_dir = value
            .get("state_dir")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("state_dir"))?;
        let port = match value.get("port") {
            None => 19441,
            Some(value) => value
                .as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .ok_or_else(|| invalid("port"))?,
        };
        let max_workers = match value.get("max_workers") {
            None => 2,
            Some(value) => value
                .as_i64()
                .map(|n| n.clamp(1, 2) as usize)
                .ok_or_else(|| invalid("max_workers"))?,
        };
        let python_status = match value.get("python_status") {
            None => None,
            Some(value) => Some(base.join(value.as_str().ok_or_else(|| invalid("python_status"))?)),
        };
        Ok(Self {
            command,
            env,
            catalog,
            tools,
            state_dir: PathBuf::from(if state_dir.is_empty() { "." } else { state_dir }),
            port,
            max_workers,
            python_status,
            idle_seconds: timer(&value, "idle_seconds", 300.0)?,
            tool_timeout: timer(&value, "tool_timeout", 120.0)?,
            queue_timeout: timer(&value, "queue_timeout", 0.0)?,
            stop_grace: timer(&value, "stop_grace", 7.0)?,
        })
    }
}
