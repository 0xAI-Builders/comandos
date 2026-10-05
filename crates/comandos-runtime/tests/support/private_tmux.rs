//! Servidor tmux privado de las pruebas de `comandos-runtime` (P23 del
//! pre-flight 2f). Toda invocación lleva `-f /dev/null -S <dir>/tmux-<uid>/default`
//! y un entorno limpio (sin `TMUX`, sin escritorio ni DBus, HOME temporal): el
//! servidor que arranque y sus panes nunca ven el entorno del desarrollador.
//! El `Drop` hace `kill-server` con ese mismo `-S` y solo después borra el
//! directorio. Nunca se llama a tmux sin `-S`.
#![allow(dead_code)]
use std::{
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// El tmux real por ruta absoluta (nunca un `tmux` del `PATH`, que en las
/// pruebas puede ser un falso), o `None` si no está instalado.
pub fn real_tmux() -> Option<PathBuf> {
    ["/usr/bin/tmux", "/usr/local/bin/tmux", "/bin/tmux"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

pub struct PrivateTmux {
    /// Raíz de la prueba: `tmux/`, `home/` y lo que la prueba quiera.
    pub dir: PathBuf,
    tmux: PathBuf,
}

impl PrivateTmux {
    /// `None` (con aviso) si no hay tmux: la prueba se salta.
    pub fn new(tag: &str) -> Option<Self> {
        let Some(tmux) = real_tmux() else {
            eprintln!("tmux no está instalado: se salta la prueba");
            return None;
        };
        // Corto: el socket debe caber en los 107 bytes de `sun_path`.
        let tmp = std::env::temp_dir();
        let base = if tmp.as_os_str().len() <= 24 {
            tmp
        } else {
            PathBuf::from("/tmp")
        };
        let dir = base.join(format!("cmd-rt-{tag}-{}", std::process::id()));
        let this = Self { dir, tmux };
        // Un resto de otra corrida: primero SU servidor, por `-S`.
        if this.socket().exists() {
            let _ = this.command().arg("kill-server").output();
        }
        let _ = std::fs::remove_dir_all(&this.dir);
        std::fs::create_dir_all(this.home()).unwrap();
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(this.socket().parent().unwrap())
            .unwrap();
        Some(this)
    }

    pub fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    pub fn socket(&self) -> PathBuf {
        self.dir
            .join("tmux")
            .join(format!("tmux-{}", nix_uid()))
            .join("default")
    }

    pub fn tmux_path(&self) -> &Path {
        &self.tmux
    }

    /// `tmux -f /dev/null -S <socket>` con entorno limpio.
    pub fn command(&self) -> Command {
        assert!(
            self.socket().parent().is_some_and(Path::is_dir),
            "socket privado sin directorio: se negaría a caer en el servidor real"
        );
        let mut cmd = Command::new(&self.tmux);
        cmd.args(["-f", "/dev/null", "-S"])
            .arg(self.socket())
            .env_clear()
            .env("HOME", self.home())
            .env("PATH", "/usr/bin:/bin")
            .env("SHELL", "/bin/sh")
            .env("LANG", "C.UTF-8")
            .env("TMUX_TMPDIR", self.dir.join("tmux"))
            .stdin(Stdio::null());
        cmd
    }

    /// `tmux <args>`; la prueba falla si tmux falla. Devuelve la salida.
    pub fn run(&self, args: &[&str]) -> String {
        let out = self.command().args(args).output().unwrap();
        assert!(
            out.status.success(),
            "tmux {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    /// Guardián de `tmux` para el `fakebin` del oráculo (copia de
    /// `tmux_guard` de `comandos-server`): añade `-f /dev/null -S` y sale con 97,
    /// sin lanzar tmux, si el directorio del socket ya no existe.
    pub fn guard_script(&self) -> String {
        let socket = self.socket();
        let dir = socket.parent().unwrap();
        format!(
            "#!/bin/sh\n[ -d '{dir}' ] || {{ echo 'tmux guardián: sin socket privado' >&2; exit 97; }}\n\
             unset TMUX\nexec '{tmux}' -f /dev/null -S '{socket}' \"$@\"\n",
            dir = dir.display(),
            tmux = self.tmux.display(),
            socket = socket.display(),
        )
    }

    /// Instala el guardián como `<dir>/tmux` (0755).
    pub fn install_guard(&self, dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join("tmux");
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, self.guard_script()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

impl Drop for PrivateTmux {
    fn drop(&mut self) {
        // Primero el servidor de ESTA prueba, por su `-S`; después el directorio.
        let _ = Command::new(&self.tmux)
            .arg("-S")
            .arg(self.socket())
            .arg("kill-server")
            .env_remove("TMUX")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn nix_uid() -> u32 {
    std::fs::metadata("/proc/self")
        .map(|m| std::os::unix::fs::MetadataExt::uid(&m))
        .unwrap_or(0)
}
