//! El `cc-dash` Python del repositorio sobre el MISMO HOME que el frente:
//! se comparan bytes y se comprueba que lo que escribe uno lo lee el otro.
//! Sin `python3` las pruebas que lo usan se saltan con un aviso.
#![allow(dead_code)]
use super::TestHome;
use std::{
    ffi::OsStr,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tokio::{net::TcpStream, time::sleep};

pub struct Oracle {
    pub port: u16,
    child: Child,
}

impl Drop for Oracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub async fn oracle(home: &TestHome) -> Option<Oracle> {
    let python = Command::new("python3")
        .args(["-c", "import sys"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !python {
        eprintln!("python3 no está instalado: se salta la comparación con el oráculo");
        return None;
    }
    // `restore_requested_webterm()` al arrancar lanzaría el `cc-webterm` real
    // del PATH y `tailscale serve`: la marca solo se crea con el oráculo vivo.
    assert!(
        !home.hooks().join("webterm-enabled").exists(),
        "webterm-enabled antes de arrancar el oráculo tocaría el terminal web real"
    );
    let port = super::dead_port();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Igual que `xtask parity`: los ejecutables con efectos fuera del HOME temporal
    // (terminal web, tailscale, systemd, sonido, ventanas) son enlaces a `true`, y el
    // oráculo no ve el systemd ni el DBus de la sesión real.
    let fakebin = home.root.join("fakebin");
    let runtime = home.root.join("xdg-runtime");
    std::fs::create_dir_all(&fakebin).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
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
    ] {
        let link = fakebin.join(name);
        if !link.exists() {
            std::os::unix::fs::symlink("/bin/true", &link).unwrap();
        }
    }
    // El `tmux` del oráculo va siempre con `-S` al socket privado de la
    // prueba: solo `TMUX_TMPDIR` no basta (tmux 3.2a cae en el servidor real
    // del usuario si ese directorio desaparece).
    // Sin tmux instalado el envoltorio llama a `true` (nunca a otro servidor).
    let real_tmux = ["/usr/bin/tmux", "/bin/tmux", "/usr/local/bin/tmux"]
        .into_iter()
        .find(|p| Path::new(p).exists())
        .unwrap_or("/bin/true");
    let socket = comandos_server::dash::native::tmux::private_socket(&home.tmux_dir());
    if let Some(parent) = socket.parent() {
        use std::os::unix::fs::DirBuilderExt;
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent);
    }
    let wrapper = fakebin.join("tmux");
    let _ = std::fs::remove_file(&wrapper);
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nexec {real_tmux} -S '{}' \"$@\"\n",
            socket.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(
        &wrapper,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let path = format!(
        "{}:{}",
        fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let err = std::fs::File::create(home.root.join("oracle.err")).unwrap();
    let mut child = Command::new("python3")
        .arg(repo.join("bin/cc-dash"))
        .arg(port.to_string())
        .arg("--no-open")
        .env_remove("TMUX")
        .env_remove("COMANDOS_STATE_DB")
        .env_remove("COMANDOS_USAGE_DB")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("HOME", &home.root)
        .env("PATH", &path)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_STATE_HOME", home.root.join(".local/state"))
        .env("TMUX_TMPDIR", home.tmux_dir())
        .env("COMANDOS_DASH_DIR", repo.join("dash"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(err)
        .spawn()
        .unwrap();
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).await.is_err() {
        if let Ok(Some(status)) = child.try_wait() {
            let log = std::fs::read_to_string(home.root.join("oracle.err")).unwrap_or_default();
            panic!("cc-dash salió con {status}: {log}");
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "cc-dash no arrancó"
        );
        sleep(Duration::from_millis(100)).await;
    }
    Some(Oracle { port, child })
}

/// `python3 -c <guion> <repo> <args…>` sobre el HOME temporal `home`; `None`
/// sin python3. Mismo entorno que `oracle` (y que `run_python` del runtime):
/// los ejecutables con efectos fuera del HOME son enlaces a `true` (también
/// `tmux`), y el guion no ve el tmux, el systemd ni el DBus de la sesión real.
pub fn run_python(script: &str, args: &[&OsStr], home: &Path) -> Option<String> {
    let python = Command::new("python3")
        .args(["-c", "import sys"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !python {
        eprintln!("python3 no está instalado: se salta la comparación con el oráculo");
        return None;
    }
    let repo = super::repo();
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
        "ssh",
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
        .arg(&repo)
        .args(args)
        .current_dir(&repo)
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
