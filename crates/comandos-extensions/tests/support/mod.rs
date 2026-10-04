//! Utilidades compartidas por las pruebas de integración.
pub mod fake_mcp_http;

use std::path::Path;

/// Enlaza (solo lectura) el venv real del oráculo Python bajo un HOME temporal: con
/// HOME distinto el oráculo no ve el `mcp` del site de usuario y `bin/cc-extensions`
/// cae al venv de extensiones bajo HOME.
pub fn link_oracle_venv(home: &Path) {
    let real_home = std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let venv = real_home.join(".local/share/comandos/extensions-venv");
    assert!(venv.exists(), "falta el venv del oráculo en {venv:?}");
    std::fs::create_dir_all(home.join(".local/share/comandos")).unwrap();
    std::os::unix::fs::symlink(&venv, home.join(".local/share/comandos/extensions-venv")).unwrap();
}

pub mod oracle {
    //! Ejecución del proxy Rust y del oráculo Python con el mismo HOME temporal.
    use std::{
        path::{Path, PathBuf},
        process::{Command, Output},
    };

    /// `python3.11` resuelto con el PATH del proceso de prueba (el del hijo se restringe).
    pub fn python_bin() -> PathBuf {
        std::env::split_paths(&std::env::var_os("PATH").expect("PATH"))
            .map(|dir| dir.join("python3.11"))
            .find(|p| p.is_file())
            .expect("python3.11 no está en PATH")
    }

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn configure(cmd: &mut Command, home: &Path) {
        cmd.current_dir("/")
            .env("HOME", home)
            .env("COMANDOS_INHERITED", "padre")
            .env("PATH", format!("{}/bin:/usr/bin:/bin", home.display()))
            .stdin(std::process::Stdio::null());
    }

    pub fn python(home: &Path, args: &[&str]) -> Output {
        let mut cmd = Command::new(python_bin());
        cmd.arg(root().join("bin/cc-extensions")).args(args);
        configure(&mut cmd, home);
        cmd.output().unwrap()
    }

    pub fn rust(home: &Path, args: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_comandos-extensions"));
        cmd.args(args);
        configure(&mut cmd, home);
        cmd.output().unwrap()
    }
}
