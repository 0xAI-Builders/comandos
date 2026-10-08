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
    if !matches!(
        std::env::var("COMANDOS_ORACLE").as_deref(),
        Ok("record" | "check")
    ) {
        return;
    }
    let real_home = std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let venv = std::env::var_os("COMANDOS_ORACLE_VENV")
        .map(PathBuf::from)
        .unwrap_or_else(|| real_home.join(".local/share/comandos/extensions-venv"));
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
        // El volcado heredado debe ser fijo: el modo del oráculo y variables del
        // controlador no pertenecen al entorno de este escenario de prueba.
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("COMANDOS_") {
                cmd.env_remove(key);
            }
        }
        cmd.current_dir("/")
            .env("HOME", home)
            .env("COMANDOS_INHERITED", "padre")
            .env("PATH", format!("{}/bin:/usr/bin:/bin", home.display()))
            .stdin(std::process::Stdio::null());
        super::isolate(cmd);
    }

    /// Reproduce solo la expectativa: el proceso Rust y sus efectos se ejecutan siempre.
    pub fn python(home: &Path, args: &[&str]) -> Output {
        use std::os::unix::process::ExitStatusExt;
        let repo = root();
        let roots = [("<HOME>", home), ("<REPO>", repo.as_path())];
        let catalog = std::fs::read(home.join(".config/comandos/extensions/catalog.json")).unwrap();
        let input = serde_json::json!({"args":args, "catalog":String::from_utf8(catalog).unwrap()});
        let bytes = comandos_oracle::oracle_at(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            "extensions-command",
            &input,
            || {
                let mut cmd = Command::new(python_bin());
                cmd.arg(repo.join("bin/cc-extensions")).args(args);
                configure(&mut cmd, home);
                let output = cmd.output().map_err(|e| e.to_string())?;
                let stdout = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
                // El PID esperado es variable; la prueba nativa de exec conserva el PID real.
                let stdout = regex::Regex::new(r"(?m)^pid=\d+$")
                    .unwrap()
                    .replace_all(&stdout, "pid=0");
                let artifact = serde_json::json!({
                    "status":output.status.into_raw(), "stdout":stdout,
                    "stderr":String::from_utf8(output.stderr).map_err(|e| e.to_string())?
                });
                Ok(comandos_oracle::normalize(
                    &serde_json::to_vec(&artifact).unwrap(),
                    &roots,
                ))
            },
        );
        let artifact: serde_json::Value =
            serde_json::from_slice(&comandos_oracle::restore(&bytes, &roots)).unwrap();
        Output {
            status: std::process::ExitStatus::from_raw(artifact["status"].as_i64().unwrap() as i32),
            stdout: artifact["stdout"].as_str().unwrap().as_bytes().to_vec(),
            stderr: artifact["stderr"].as_str().unwrap().as_bytes().to_vec(),
        }
    }

    pub fn rust(home: &Path, args: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_comandos-extensions"));
        cmd.args(args);
        configure(&mut cmd, home);
        cmd.output().unwrap()
    }
}
