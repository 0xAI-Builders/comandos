//! HOME y servidor desechables. Ninguna llamada puede elegir el socket default.
#![allow(dead_code)]
use comandos_server::dash::{
    native::tmux::{Tmux, private_socket},
    term::attach::TmuxTarget,
};
use std::{
    path::PathBuf,
    process::{Command, Output},
};
pub struct TestHome {
    pub root: PathBuf,
}
impl TestHome {
    pub fn new() -> Self {
        let mut nonce = [0; 8];
        getrandom::fill(&mut nonce).unwrap();
        let root = std::env::temp_dir().join(format!(
            "comandos-a3-{}-{}",
            std::process::id(),
            u64::from_le_bytes(nonce)
        ));
        std::fs::create_dir_all(root.join("home")).unwrap();
        let _ = Tmux::private(&root);
        Self { root }
    }
    pub fn command(&self) -> Command {
        let mut cmd = Command::new("/usr/bin/tmux");
        cmd.env_clear()
            .env("HOME", self.root.join("home"))
            .env("PATH", "/usr/bin:/bin")
            .env("SHELL", "/bin/sh")
            .env("TERM", "xterm-256color")
            .arg("-f")
            .arg("/dev/null")
            .arg("-S")
            .arg(private_socket(&self.root));
        cmd
    }
}
impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = self.command().arg("kill-server").output();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
pub struct PrivateTmux {
    pub home: TestHome,
}
impl PrivateTmux {
    pub fn start(sessions: &[&str]) -> Option<Self> {
        if !std::path::Path::new("/usr/bin/tmux").exists() {
            eprintln!("sin tmux, prueba privada omitida");
            return None;
        }
        let home = TestHome::new();
        let me = Self { home };
        for session in sessions {
            assert!(
                me.tmux(&[
                    "new-session",
                    "-d",
                    "-s",
                    session,
                    "-x",
                    "80",
                    "-y",
                    "24",
                    "/bin/sh"
                ])
                .status
                .success()
            );
        }
        Some(me)
    }
    pub fn target(&self) -> TmuxTarget {
        TmuxTarget::Private(self.home.root.clone())
    }
    pub fn tmux(&self, args: &[&str]) -> Output {
        {
            let out = self.home.command().args(args).output().unwrap();
            if !out.status.success() {
                eprintln!(
                    "private tmux {args:?}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            out
        }
    }
    pub fn clients(&self) -> usize {
        String::from_utf8_lossy(&self.tmux(&["list-clients", "-F", "#{client_pid}"]).stdout)
            .lines()
            .count()
    }
    pub fn capture(&self) -> String {
        String::from_utf8_lossy(&self.tmux(&["capture-pane", "-p", "-t", "=t1:0.0"]).stdout)
            .into_owned()
    }
    pub fn size(&self) -> String {
        String::from_utf8_lossy(
            &self
                .tmux(&["list-clients", "-F", "#{client_width}x#{client_height}"])
                .stdout,
        )
        .trim()
        .into()
    }
}
