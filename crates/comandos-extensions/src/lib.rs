pub mod auth;
pub mod python_json;
use serde_json::Value;
use std::{env, path::PathBuf, process::Command};

pub type Result<T> = std::result::Result<T, String>;

pub fn expand_vars(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut result = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '$' {
            result.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        let (name, end) = if chars.get(i) == Some(&'{') {
            let begin = i + 1;
            while i < chars.len() && chars[i] != '}' {
                i += 1;
            }
            if i == chars.len() {
                result.extend(&chars[start..]);
                break;
            }
            (chars[begin..i].iter().collect::<String>(), i + 1)
        } else {
            let begin = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            (chars[begin..i].iter().collect::<String>(), i)
        };
        result.push_str(&env::var(&name).unwrap_or_else(|_| chars[start..end].iter().collect()));
        i = end;
    }
    result
}

pub fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| {
            nix::unistd::User::from_uid(nix::unistd::getuid())
                .ok()
                .flatten()
                .map(|u| u.dir)
        })
        .ok_or_else(|| "Home unavailable".into())
}

pub fn expand_user(value: &str) -> String {
    if !value.starts_with('~') {
        return value.into();
    }
    let end = value.find('/').unwrap_or(value.len());
    let home = if end == 1 {
        home_dir().ok()
    } else {
        nix::unistd::User::from_name(&value[1..end])
            .ok()
            .flatten()
            .map(|u| u.dir)
    };
    home.map(|h| {
        format!(
            "{}{}",
            h.to_string_lossy().trim_end_matches('/'),
            &value[end..]
        )
    })
    .unwrap_or_else(|| value.into())
}

pub fn command(spec: &Value, direct: bool) -> Result<Command> {
    let mut command = Command::new(expand_user(
        spec["command"].as_str().ok_or("Invalid command")?,
    ));
    if let Some(args) = spec.get("args") {
        for arg in args.as_array().ok_or("Invalid arguments")? {
            command.arg(arg.as_str().ok_or("Invalid argument")?);
        }
    }
    if let Some(vars) = spec.get("env") {
        for (key, value) in vars.as_object().ok_or("Invalid environment")? {
            command.env(key, expand_vars(&python_string(value)));
        }
    }
    if let Some(cwd) = spec.get("cwd").and_then(Value::as_str) {
        command.current_dir(if direct { expand_user(cwd) } else { cwd.into() });
    }
    command.stderr(std::process::Stdio::null());
    Ok(command)
}

pub fn python_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) if n.as_str() == "NaN" => "nan".into(),
        Value::Number(n) if n.as_str() == "Infinity" => "inf".into(),
        Value::Number(n) if n.as_str() == "-Infinity" => "-inf".into(),
        _ => value.to_string(),
    }
}

pub mod serve;
pub mod transport;

pub mod metadata;
pub mod tokenizer;

pub mod catalog;
pub mod config;
pub mod skills;

pub mod check;
