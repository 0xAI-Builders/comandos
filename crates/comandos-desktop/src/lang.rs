use serde_json::Value;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Es,
    En,
}
impl Lang {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Es => "es",
            Self::En => "en",
        }
    }
}
pub fn ui_lang(conf_lang: Option<&str>, env_lang: &str) -> Lang {
    match conf_lang {
        Some("es") => Lang::Es,
        Some("en") => Lang::En,
        _ if env_lang.to_lowercase().starts_with("es") => Lang::Es,
        _ => Lang::En,
    }
}
pub const VALID_THEMES: &[&str] = &[
    "noche",
    "dia",
    "calido",
    "termius",
    "bruno",
    "superglass",
    "neon",
    "contraste",
    "ubuntu",
];
pub const DEFAULT_THEME: &str = "noche";
pub const DOT_COLORS: &[(&str, &str)] = &[
    ("waiting", "#FFAE1A"),
    ("working", "#7AA5FF"),
    ("done", "#2EE59D"),
    ("dead", "#FF6B6B"),
];
pub const DOT_IDLE: &str = "#39445C";
pub fn initial_theme(preferences: &Value) -> &str {
    preferences
        .get("theme")
        .and_then(Value::as_str)
        .filter(|t| VALID_THEMES.contains(t))
        .unwrap_or(DEFAULT_THEME)
}
