//! `python3 -c <texto> <repo> <args…>` sobre un HOME temporal; `None` sin python3.
//! Mismo entorno que el oráculo de `comandos-server` (`tests/support/oracle.rs`):
//! los ejecutables con efectos fuera del HOME son enlaces a `true`, y el guion no
//! ve el tmux, el systemd ni el DBus de la sesión real.
#![allow(dead_code)]
use std::{ffi::OsStr, path::Path, process::Command};
#[path = "python_golden.rs"]
mod golden;

/// Claves del entorno que cambian lo que calcula el Python de uso (D7 del plan 2e):
/// el lado Rust las recibe por parámetro, así que el oráculo no debe heredarlas.
pub const D7_KEYS: &[&str] = &[
    "COMANDOS_DAILY_BUDGET_USD",
    "COMANDOS_USAGE_DAILY_BUDGET_USD",
    "COMANDOS_CODEX_DAILY_TOKEN_LIMIT",
    "COMANDOS_CODEX_WEEKLY_TOKEN_LIMIT",
    "COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT",
    "COMANDOS_CLAUDE_WEEKLY_TOKEN_LIMIT",
    "CODEX_DAILY_TOKEN_LIMIT",
    "CODEX_WEEKLY_TOKEN_LIMIT",
    "CLAUDE_DAILY_TOKEN_LIMIT",
    "CLAUDE_WEEKLY_TOKEN_LIMIT",
    "COMANDOS_USAGE_LOCAL_DAYS",
    "COMANDOS_USAGE_CLAUDE_MAX_FILES",
    "COMANDOS_USAGE_CODEX_MAX_FILES",
    "COMANDOS_CLAUDE_PROJECTS_DIR",
    "COMANDOS_OPENCODE_DB",
    "OPENAI_ADMIN_KEY",
    "ANTHROPIC_ADMIN_KEY",
];

pub fn repo() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn run_python(script: &str, args: &[&OsStr], home: &Path) -> Option<String> {
    if matches!(
        env!("CARGO_CRATE_NAME"),
        "extension_prepare_oracle"
            | "tmux_snapshot_oracle"
            | "extension_inventory_oracle"
            | "session_profiles_oracle"
            | "pane_extensions_oracle"
            | "session_configuration_oracle"
            | "cli_catalog_oracle"
            | "extension_observations_oracle"
            | "limits_oracle"
            | "model_watch_oracle"
            | "news_editions_oracle"
            | "pane_snapshot_oracle"
            | "providers_oracle"
            | "providers_public_oracle"
            | "ssh_config_oracle"
            | "tui_state_oracle"
    ) {
        return golden::run_python(script, args, home);
    }
    let python = Command::new("python3")
        .args(["-c", "import sys"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !python {
        eprintln!("python3 no está instalado: se salta la comparación con el oráculo");
        return None;
    }
    let fakebin = home.join("fakebin");
    let runtime = home.join("xdg-runtime");
    let tmux = home.join("tmux");
    for dir in [&fakebin, &runtime, &tmux] {
        std::fs::create_dir_all(dir).unwrap();
    }
    for name in [
        "systemctl",
        "wmctrl",
        "cc-webterm",
        "cc-webterm-attach",
        "systemd-run",
        "tailscale",
        "notify-send",
        "pw-play",
        "paplay",
        "spd-say",
        "piper",
        "xdg-open",
        "tmux",
    ] {
        let link = fakebin.join(name);
        if !link.exists() {
            std::os::unix::fs::symlink("/bin/true", &link).unwrap();
        }
    }
    let path = format!(
        "{}:{}",
        fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new("python3");
    for key in D7_KEYS {
        command.env_remove(key);
    }
    let out = command
        .arg("-c")
        .arg(script)
        .arg(repo())
        .args(args)
        .current_dir(repo())
        .env("HOME", home)
        .env("PATH", &path)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("TMUX_TMPDIR", &tmux)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        // Codificación de `open()` fija: la del lado Rust (UTF-8).
        .env("LANG", "C.UTF-8")
        .env_remove("LC_ALL")
        .env_remove("LC_CTYPE")
        .env_remove("TMUX")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("OPENCODE_CONFIG")
        .env_remove("OPENCODE_CONFIG_DIR")
        .env_remove("COMANDOS_STATE_DB")
        .env_remove("COMANDOS_USAGE_DB")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "oráculo: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(String::from_utf8(out.stdout).unwrap())
}
