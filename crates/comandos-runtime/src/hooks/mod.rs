//! Hooks de los harness (`comandos hook <harness>`), con paridad contra los
//! scripts que reemplazan. `claude` = `hooks/cc-notify.sh`; los demás llegan en la
//! Tarea 11.
mod claude;
mod conf;
mod events_jsonl;
mod input;
mod jq;
mod notify_http;
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
        Some(notify_http::WORKER) => notify_http::worker_main(),
        Some(other) => {
            eprintln!("hook {other}: no migrado todavía");
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
