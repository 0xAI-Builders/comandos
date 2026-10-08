//! Ejecuta el controlador en un proceso sin COMANDOS_DB heredado.
use std::{path::PathBuf, process::Command};
pub fn main(args: &[String]) -> i32 {
    let mut iter = args.iter();
    let mut source = None;
    let mut binary = None;
    let mut keep = false;
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--source-home" => source = iter.next().map(PathBuf::from),
            "--binary" => binary = iter.next().map(PathBuf::from),
            "--keep" => keep = true,
            _ => {
                eprintln!(
                    "uso: cargo xtask state-drill --source-home DIR [--binary COMANDOS] [--keep]"
                );
                return 2;
            }
        }
    }
    let Some(source) = source.filter(|p| p.is_absolute()) else {
        eprintln!("--source-home requiere ruta absoluta");
        return 2;
    };
    let binary = binary.or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("comandos")))
    });
    let Some(binary) = binary.filter(|p| p.is_absolute() && p.is_file()) else {
        eprintln!(
            "compile comandos-cli o indique --binary con la ruta absoluta del binario privado"
        );
        return 2;
    };
    let mut cmd = Command::new(binary);
    cmd.env_remove("COMANDOS_DB")
        .args(["state", "drill", "--source-home"])
        .arg(source);
    if keep {
        cmd.arg("--keep");
    }
    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
