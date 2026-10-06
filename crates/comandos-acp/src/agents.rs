//! Registry-owned argv templates. Every child launches directly, without a shell.
use super::Result;
use comandos_core::json::workspace_loads;
use comandos_runtime::hooks::py::str_of as python_string;
use comandos_runtime::{accounts, providers};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

pub fn registry() -> Result<Value> {
    let raw = workspace_loads(include_str!("../../../config/providers.json"))
        .map_err(|e| e.to_string())?;
    providers::validate_registry(&raw)
        .map_err(|_| "registro de proveedores inválido")?
        .map_err(|e| e.to_string())?;
    Ok(raw)
}
pub fn which(name: &str, home: &Path, path: Option<&OsStr>) -> Option<PathBuf> {
    providers::which_in_dirs(
        name,
        path,
        home,
        &[
            "~/.local/bin",
            "~/.bun/bin",
            "~/.cargo/bin",
            "~/.npm-global/bin",
            "~/.opencode/bin",
            "~/.grok/bin",
            "~/bin",
            "/usr/local/bin",
        ],
    )
}
fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(python_string)
        .collect()
}
pub fn available(spec: &Value, home: &Path, path: Option<&OsStr>) -> bool {
    spec["command"]
        .as_array()
        .and_then(|v| v.first())
        .is_some_and(|v| which(&python_string(v), home, path).is_some())
        && strings(&spec["requires"])
            .iter()
            .all(|v| which(v, home, path).is_some())
}
pub fn command(
    spec: &Value,
    home: &Path,
    path: Option<&OsStr>,
    model: &str,
    effort: &str,
    danger: bool,
    extra: &Value,
) -> Result<(Vec<String>, BTreeMap<String, String>)> {
    let template = strings(&spec["command"]);
    if template.is_empty() {
        return Err("agente sin comando".into());
    }
    let groups = [
        (
            "{model_args}",
            if model.is_empty() {
                vec![]
            } else {
                strings(&spec["modelArgs"])
                    .into_iter()
                    .map(|s| s.replace("{model}", model))
                    .collect()
            },
        ),
        (
            "{effort_args}",
            if effort.is_empty() {
                vec![]
            } else {
                strings(&spec["effortArgs"])
                    .into_iter()
                    .map(|s| s.replace("{effort}", effort))
                    .collect()
            },
        ),
        (
            "{danger_args}",
            if danger {
                strings(&spec["dangerArgs"])
            } else {
                vec![]
            },
        ),
    ];
    let mut argv = vec![];
    let mut placed = vec![];
    for token in template {
        if let Some((key, args)) = groups.iter().find(|(k, _)| *k == token) {
            argv.extend(args.clone());
            placed.push(*key);
        } else {
            argv.push(token);
        }
    }
    for (key, args) in groups {
        if !placed.contains(&key) {
            argv.extend(args);
        }
    }
    argv[0] = which(&argv[0], home, path)
        .ok_or_else(|| format!("{} no está instalado", argv[0]))?
        .to_str()
        .ok_or("binario ACP no es UTF-8")?
        .into();
    let mut env = BTreeMap::new();
    if let Some(values) = spec["env"].as_object() {
        for (k, v) in values {
            let mut v = python_string(v);
            if let Some(binary) = v.strip_prefix("which:") {
                let Some(hit) = which(binary, home, path) else {
                    continue;
                };
                v = hit.to_str().ok_or("binario ACP no es UTF-8")?.into();
            } else if let Some(rest) = v.strip_prefix("~/") {
                v = home.join(rest).to_str().ok_or("HOME no es UTF-8")?.into();
            }
            env.insert(k.clone(), v);
        }
    }
    if !model.is_empty()
        && let Some(key) = spec["modelEnv"].as_str()
    {
        // OpenCode receives JSON, so a vendor ID containing quotes must remain data.
        let value = if key == "OPENCODE_CONFIG_CONTENT" {
            serde_json::to_string(&serde_json::json!({"model":model})).map_err(|e| e.to_string())?
        } else {
            spec["modelEnvTemplate"]
                .as_str()
                .unwrap_or("{model}")
                .replace("{model}", model)
        };
        env.insert(key.into(), value);
    }
    if !effort.is_empty()
        && let Some(key) = spec["effortEnv"].as_str()
    {
        env.insert(
            key.into(),
            spec["effortEnvMap"][effort]
                .as_str()
                .unwrap_or(effort)
                .into(),
        );
    }
    if let Some(extra) = extra.as_object() {
        for (k, v) in extra {
            env.insert(k.clone(), python_string(v));
        }
    }
    Ok((argv, env))
}
pub fn account_env(
    registry: &Value,
    spec: &Value,
    account: &str,
    home: &Path,
    cwd: &Path,
) -> Result<Value> {
    match spec["accountsProvider"].as_str() {
        Some(provider) => accounts::account_environment(
            registry,
            provider,
            &Value::String(account.into()),
            &accounts::Paths::new(home, cwd),
        )
        .map_err(|e| e.to_string()),
        None => Ok(serde_json::json!({})),
    }
}
pub fn accounts(registry: &Value, spec: &Value, home: &Path, cwd: &Path) -> Result<Vec<Value>> {
    let provider = spec["accountsProvider"]
        .as_str()
        .ok_or("cuentas no soportadas")?;
    accounts::list_accounts(registry, provider, &accounts::Paths::new(home, cwd))
        .map_err(|e| e.to_string())
}
