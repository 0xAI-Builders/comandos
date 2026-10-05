//! `python3 -c <texto> <repo> <args…>` sobre un HOME temporal; `None` sin python3.
//! Copia del ayudante de `comandos-runtime` (`tests/support/python.rs`): los
//! ejecutables con efectos fuera del HOME son enlaces a `true` (incluido `tmux`), el
//! guion no ve el systemd ni el DBus de la sesión real, y se quitan las claves del
//! entorno que cambian lo que calcula el Python (D7 del plan 2e). Zona fija
//! `America/Mexico_City` y `LANG=C.UTF-8`, iguales en el lado Rust.
#![allow(dead_code)]
use std::{ffi::OsStr, path::Path, process::Command};

pub const TZ: &str = "America/Mexico_City";

/// Claves que el Python lee del entorno y que el Rust recibe por parámetro (D7).
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
    for dir in [&fakebin, &runtime] {
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
        "git",
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
    command
        .arg("-c")
        .arg(script)
        .arg(repo())
        .args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("PATH", &path)
        .env("TZ", TZ)
        .env("LANG", "C.UTF-8")
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("TMUX")
        .env_remove("TMUX_TMPDIR")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME")
        .env_remove("COMANDOS_STATE_DB")
        .env_remove("COMANDOS_USAGE_DB")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY");
    for key in D7_KEYS {
        command.env_remove(key);
    }
    let out = command.output().unwrap();
    assert!(
        out.status.success(),
        "oráculo: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(String::from_utf8(out.stdout).unwrap())
}
