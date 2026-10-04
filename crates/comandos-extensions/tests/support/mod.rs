//! Utilidades compartidas por las pruebas de integración.
pub mod fake_mcp_http;

use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// Fija en `cmd` un `XDG_RUNTIME_DIR` propio del proceso de pruebas (ruta corta: un socket
/// Unix admite como mucho 108 bytes) y comprueba el aislamiento. Sin daemon allí, `serve`
/// usa el proxy directo en silencio, como en una máquina sin broker.
pub fn isolate(cmd: &mut Command) {
    use std::os::unix::fs::DirBuilderExt;
    let run = std::env::temp_dir().join(format!("cxr-{}", std::process::id()));
    let _ = std::fs::DirBuilder::new().mode(0o700).create(&run);
    cmd.env("XDG_RUNTIME_DIR", &run);
    assert_isolated(cmd);
}

/// Falla si `cmd` pudiera ver el broker real del usuario: exige un `XDG_RUNTIME_DIR`
/// explícito y fuera de `/run/user` (allí vive `comandos/broker.sock` del daemon en vivo).
/// Todo ayudante que lance `serve`, `broker` o `cc-extensions` debe pasar por aquí.
pub fn assert_isolated(cmd: &Command) {
    let run = cmd
        .get_envs()
        .find(|(k, _)| *k == "XDG_RUNTIME_DIR")
        .and_then(|(_, v)| v)
        .map(PathBuf::from);
    let Some(run) = run else {
        panic!("la prueba debe fijar XDG_RUNTIME_DIR: heredaría el broker real");
    };
    assert!(
        !run.starts_with("/run/user") && run.is_absolute(),
        "XDG_RUNTIME_DIR de prueba inseguro: {}",
        run.display()
    );
}

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
        super::isolate(cmd);
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
