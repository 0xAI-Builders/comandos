//! El `cc-dash` Python del repositorio sobre el MISMO HOME que el frente:
//! se comparan bytes y se comprueba que lo que escribe uno lo lee el otro.
//! Sin `python3` las pruebas que lo usan se saltan con un aviso.
#![allow(dead_code)]
use super::TestHome;
use std::{
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
