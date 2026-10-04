//! Resolución del subcomando a partir del nombre invocado o del primer argumento.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Ext(Vec<String>),
    Hook(Vec<String>),
    Events(Vec<String>),
    Version,
    Help,
    Unknown(String),
}

/// Nombres heredados (symlinks `cc-*`) y el subcomando al que corresponden.
const ALIASES: &[(&str, &[&str])] = &[
    ("cc-extensions", &["ext"]),
    ("cc-notify.sh", &["hook", "claude"]),
    ("cc-usage-tool.sh", &["hook", "claude-usage"]),
    ("cc-status.sh", &["hook", "claude-status"]),
    ("codex-notify.sh", &["hook", "codex"]),
    ("codex-hooks.sh", &["hook", "codex-hooks"]),
    ("gemini-hooks.sh", &["hook", "gemini"]),
    ("agy-hooks.sh", &["hook", "agy"]),
    ("grok-hooks.py", &["hook", "grok"]),
    ("comandos-events", &["events"]),
];

pub fn resolve(argv0: &str, args: &[String]) -> Command {
    let name = argv0.rsplit('/').next().unwrap_or(argv0);
    let mut words: Vec<String> = Vec::new();
    if let Some((_, prefix)) = ALIASES.iter().find(|(alias, _)| *alias == name) {
        words.extend(prefix.iter().map(|s| s.to_string()));
    }
    words.extend(args.iter().cloned());
    match words.first().map(String::as_str) {
        Some("ext") => Command::Ext(words[1..].to_vec()),
        Some("hook") => Command::Hook(words[1..].to_vec()),
        Some("events") => Command::Events(words[1..].to_vec()),
        Some("--version") | Some("version") => Command::Version,
        None | Some("--help") | Some("help") => Command::Help,
        Some(other) => Command::Unknown(other.to_string()),
    }
}
