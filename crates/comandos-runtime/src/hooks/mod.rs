//! Hooks de los harness (`comandos hook <harness>`), con paridad contra los
//! scripts que reemplazan. `claude` = `hooks/cc-notify.sh`; los adaptadores de la
//! Tarea 11 (`codex`, `codex-hooks`, `gemini`, `agy`, `agy-status`, `grok`,
//! `opencode`, `claude-usage`, `claude-status`) transcriben `adapters/*` y los
//! otros dos scripts de `hooks/`.
mod adapter;
mod agy;
mod agy_status;
mod bash;
mod claude;
mod claude_status;
mod claude_usage;
mod codex;
mod codex_hooks;
mod collate;
mod conf;
mod events_jsonl;
mod gemini;
mod grok;
mod grok_redact;
mod input;
mod jq;
mod notify_http;
pub mod opencode;
pub mod py;
mod state_file;
mod text;
mod transcript;
mod usage_hook;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

/// Punto de entrada: el primer argumento es el harness.
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("claude") => claude::run(&args[1..]),
        Some("codex") => codex::run(&args[1..]),
        Some("codex-hooks") => codex_hooks::run(&args[1..]),
        Some("gemini") => gemini::run(&args[1..]),
        Some("agy") => agy::run(&args[1..]),
        Some("agy-status") => agy_status::run(&args[1..]),
        Some("grok") => grok::run(&args[1..]),
        Some("opencode") => opencode::run(&args[1..]),
        Some("claude-usage") => claude_usage::run(&args[1..]),
        Some("claude-status") => claude_status::run(&args[1..]),
        Some(notify_http::WORKER) => notify_http::worker_main(),
        Some(other) => {
            eprintln!("hook {other}: harness desconocido");
            2
        }
        None => {
            eprintln!("uso: comandos hook <harness> [argumentos]");
            2
        }
    }
}

/// `command -v NOMBRE` para ejecutables del `PATH`.
pub(crate) fn which(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in paths.as_encoded_bytes().split(|&b| b == b':') {
        let dir = if dir.is_empty() {
            PathBuf::from(".")
        } else {
            PathBuf::from(<std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(dir))
        };
        let candidate = dir.join(name);
        if let Ok(meta) = std::fs::metadata(&candidate)
            && meta.is_file()
            && meta.permissions().mode() & 0o111 != 0
        {
            return Some(candidate);
        }
    }
    None
}
