//! Literal pane input through a caller-owned argument-vector callback.
use std::collections::HashSet;
use std::sync::Mutex;

pub const MAX_CHARS: usize = 2000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TmuxResult {
    pub returncode: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypingError {
    pub message: String,
    pub typed: usize,
    pub code: &'static str,
}

impl std::fmt::Display for TypingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for TypingError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypingOptions {
    pub delay: f64,
    pub budget: f64,
}

impl Default for TypingOptions {
    fn default() -> Self {
        Self {
            delay: 0.022,
            budget: 1.2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Typed {
    pub typed: usize,
}

pub fn validate(text: &str) -> Result<&str, TypingError> {
    let invalid = |message: &str| TypingError {
        message: message.into(),
        typed: 0,
        code: "invalid",
    };
    if text
        .chars()
        .all(|ch| ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}'))
    {
        return Err(invalid("Texto vacío"));
    }
    if text.chars().count() > MAX_CHARS {
        return Err(invalid("Texto demasiado largo (máx. 2000)"));
    }
    if text.chars().any(|ch| ch < ' ' || ch == '\u{7f}') {
        return Err(invalid(
            "El texto no puede contener saltos de línea ni caracteres de control",
        ));
    }
    Ok(text)
}

pub fn type_literal(
    mut tmux: impl FnMut(&[&str]) -> TmuxResult,
    pane: &str,
    text: &str,
    mut sleep: impl FnMut(f64),
    options: TypingOptions,
) -> Result<Typed, TypingError> {
    let text = validate(text)?;
    let count = text.chars().count();
    let step = options
        .delay
        .min(options.budget / count.saturating_sub(1).max(1) as f64);
    let mut typed = 0;
    for ch in text.chars() {
        let literal = if ch == ';' {
            "\\;".into()
        } else {
            ch.to_string()
        };
        let result = tmux(&["send-keys", "-t", pane, "-l", "--", &literal]);
        if result.returncode != 0 {
            let message = if result.stderr.is_empty() {
                "tmux send-keys falló"
            } else {
                &result.stderr
            };
            return Err(TypingError {
                message: message.trim().into(),
                typed,
                code: "tmux",
            });
        }
        typed += 1;
        if typed < count {
            sleep(step);
        }
    }
    Ok(Typed { typed })
}

#[derive(Debug, Default)]
pub struct PaneTypingLocks {
    busy: Mutex<HashSet<String>>,
}

impl PaneTypingLocks {
    pub fn acquire(&self, pane: &str) -> bool {
        self.busy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(pane.into())
    }

    pub fn release(&self, pane: &str) {
        self.busy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(pane);
    }
}
