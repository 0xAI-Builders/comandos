//! Servidor tmux de prueba: siempre con `-S` a un socket dentro de un directorio
//! temporal propio. Nunca toca `/tmp/tmux-<uid>/default`. Al soltarse hace
//! `kill-server` con su `-S` y DESPUÉS borra el directorio (CLAUDE.md del repo).
use comandos_app::config::{AppConfig, RunMode, parse_args};
use comandos_app::guard::WriteGuard;
use comandos_app::tmux::TmuxCtl;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

pub struct TestTmux {
    dir: PathBuf,
    socket: PathBuf,
}

pub struct Fixture {
    pub tmux: TestTmux,
    pub config: AppConfig,
    pub ctl: TmuxCtl,
    pub guard: WriteGuard,
    pub env: Vec<(String, String)>,
}

fn have_tmux() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .is_ok_and(|o| o.status.success())
}

impl TestTmux {
    /// Configuración, `TmuxCtl` y servidor de prueba coherentes para `mode`.
    /// Sandbox: socket `<dir>/run/comandos-app-sbx/tmux/t`. Sombra y live: etiqueta
    /// `t` con `TMUX_TMPDIR=<dir>/tt`, socket `<dir>/tt/tmux-<uid>/t`.
    /// `None` (la prueba se salta) si no hay tmux.
    pub fn for_mode(mode: RunMode) -> Option<Fixture> {
        Self::build(mode, true)
    }

    pub fn cold_for_mode(mode: RunMode) -> Option<Fixture> {
        Self::build(mode, false)
    }

    fn build(mode: RunMode, anchor: bool) -> Option<Fixture> {
        if !have_tmux() {
            eprintln!("tmux no está instalado: prueba saltada");
            return None;
        }
        let dir = std::env::temp_dir().join(format!(
            "comandos-app-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        for sub in ["home/.claude/hooks", "run", "tmp", "tt"] {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir.join(sub))
                .unwrap();
        }
        let env: Vec<(String, String)> = vec![
            ("HOME".into(), dir.join("home").display().to_string()),
            (
                "XDG_RUNTIME_DIR".into(),
                dir.join("run").display().to_string(),
            ),
            ("TMPDIR".into(), dir.join("tmp").display().to_string()),
            ("TMUX_TMPDIR".into(), dir.join("tt").display().to_string()),
        ];
        let lookup = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let mut args = vec![
            "--mode".to_string(),
            match mode {
                RunMode::Sandbox => "sandbox",
                RunMode::Shadow => "shadow",
                RunMode::Live => "live",
            }
            .to_string(),
        ];
        args.extend(["--tmux-socket".to_string(), "t".to_string()]);
        if mode != RunMode::Sandbox {
            args.extend([
                "--hooks-dir".to_string(),
                dir.join("home/.claude/hooks").display().to_string(),
            ]);
        }
        let config = parse_args(&args, false, &lookup).unwrap();
        let guard = WriteGuard::from_config(&config, ":99");
        let ctl = TmuxCtl::from_config(&config, &lookup).unwrap();
        if mode == RunMode::Sandbox {
            ctl.prepare_socket_dir(&guard).unwrap();
        }
        let socket = ctl.socket_path().to_path_buf();
        assert!(
            socket.starts_with(&dir),
            "el socket de prueba {} tiene que vivir en {}",
            socket.display(),
            dir.display()
        );
        if let Some(parent) = socket.parent() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .unwrap();
        }
        let tmux = TestTmux { dir, socket };
        // Sesión ancla para que el servidor no se apague al cerrar la última de la prueba.
        if anchor {
            tmux.new_session("__keep", 80, 24);
        }
        Some(Fixture {
            tmux,
            config,
            ctl,
            guard,
            env,
        })
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn raw(&self, args: &[&str]) -> Output {
        Command::new("tmux")
            .arg("-S")
            .arg(&self.socket)
            .args(["-f", "/dev/null"])
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.dir.join("home"))
            .env("SHELL", "/bin/sh")
            .env("TERM", "xterm-256color")
            .output()
            .unwrap()
    }

    pub fn new_session(&self, name: &str, cols: u16, rows: u16) {
        let out = self.raw(&[
            "new-session",
            "-d",
            "-s",
            name,
            "-x",
            &cols.to_string(),
            "-y",
            &rows.to_string(),
            "/bin/sh",
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    pub fn session_size(&self, name: &str) -> (u16, u16) {
        let out = self.raw(&[
            "display-message",
            "-p",
            "-t",
            &format!("={name}:"),
            "#{window_width} #{window_height}",
        ]);
        let text = String::from_utf8_lossy(&out.stdout);
        let mut it = text
            .split_whitespace()
            .map(|v| v.parse::<u16>().unwrap_or(0));
        (it.next().unwrap_or(0), it.next().unwrap_or(0))
    }
}

impl Drop for TestTmux {
    fn drop(&mut self) {
        // Primero kill-server con NUESTRO -S; después borrar el directorio.
        let _ = self.raw(&["kill-server"]);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
