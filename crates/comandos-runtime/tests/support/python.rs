//! `python3 -c <texto> <repo> <args…>` sobre un HOME temporal; `None` sin python3.
//! Mismo entorno que el oráculo de `comandos-server` (`tests/support/oracle.rs`):
//! los ejecutables con efectos fuera del HOME son enlaces a `true`, y el guion no
//! ve el tmux, el systemd ni el DBus de la sesión real.
#![allow(dead_code)]
use std::{ffi::OsStr, path::Path, process::Command};

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
    let out = Command::new("python3")
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
        .env_remove("TMUX")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME")
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
