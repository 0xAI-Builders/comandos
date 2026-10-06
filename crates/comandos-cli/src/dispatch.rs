//! Resolución del subcomando a partir del nombre invocado o del primer argumento.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Ext(Vec<String>),
    Hook(Vec<String>),
    Events(Vec<String>),
    Install(Vec<String>),
    Dash(Vec<String>),
    Web(Vec<String>),
    Webterm(Vec<String>),
    WebtermAttach(Vec<String>),
    State(Vec<String>),
    Keys(Vec<String>),
    Browser(Vec<String>),
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
    ("agy-statusline.py", &["hook", "agy-status"]),
    ("grok-hooks.py", &["hook", "grok"]),
    ("comandos-events", &["events"]),
    ("cc-dash", &["dash"]),
    ("cc-webterm", &["webterm"]),
    ("cc-webterm-attach", &["webterm-attach"]),
    ("cc-keys", &["keys"]),
    ("cc-browser-remote", &["browser", "remote"]),
    ("cc-browser-expose", &["browser", "expose"]),
    ("cc-browser-npx-guard", &["browser", "npx-guard"]),
];

pub(crate) fn alias_prefix(name: &str) -> Option<&'static [&'static str]> {
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == name)
        .map(|(_, prefix)| *prefix)
}

pub fn resolve(argv0: &str, args: &[String]) -> Command {
    let name = argv0.rsplit('/').next().unwrap_or(argv0);
    let mut words: Vec<String> = Vec::new();
    if let Some(prefix) = alias_prefix(name) {
        words.extend(prefix.iter().map(|s| s.to_string()));
    }
    words.extend(args.iter().cloned());
    match words.first().map(String::as_str) {
        Some("ext") => Command::Ext(words[1..].to_vec()),
        Some("hook") => Command::Hook(words[1..].to_vec()),
        Some("events") => Command::Events(words[1..].to_vec()),
        Some("install") => Command::Install(words[1..].to_vec()),
        Some("dash") => Command::Dash(words[1..].to_vec()),
        Some("web") => Command::Web(words[1..].to_vec()),
        Some("webterm") => Command::Webterm(words[1..].to_vec()),
        Some("webterm-attach") => Command::WebtermAttach(words[1..].to_vec()),
        Some("state") => Command::State(words[1..].to_vec()),
        Some("keys") => Command::Keys(words[1..].to_vec()),
        Some("browser") => Command::Browser(words[1..].to_vec()),
        Some("--version") | Some("version") => Command::Version,
        None | Some("--help") | Some("help") => Command::Help,
        Some(other) => Command::Unknown(other.to_string()),
    }
}
