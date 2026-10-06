//! Puertas del controlador; nunca sustituye el reloj real ni omite el censo.
use comandos_store::{migrate, unified};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
pub fn handles(args: &[String]) -> bool {
    matches!(
        args.first().map(String::as_str),
        Some("flip" | "seal" | "export-legacy" | "rollback")
    ) || (args.first().is_some_and(|v| v == "demote") && !args.iter().any(|s| s.starts_with("db-")))
}
fn capable(proc_root: &Path, home: &Path, repo: &Path, name: &str) -> Result<(), String> {
    super::preflight::can_unify(&super::preflight::domain_writers(
        proc_root, home, repo, name,
    ))
    .map_err(|e| e.join("; "))
}
fn execute(args: &[String]) -> Result<Value, String> {
    let mut iter = args.iter();
    let command = iter.next().ok_or("falta comando state")?;
    let mut home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME no definido")?;
    let mut proc_root = PathBuf::from("/proc");
    let mut repo = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut words = vec![];
    let mut yes = false;
    let mut loss = false;
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--home" => home = iter.next().ok_or("falta --home")?.into(),
            "--proc-root" => proc_root = iter.next().ok_or("falta --proc-root")?.into(),
            "--repo" => repo = iter.next().ok_or("falta --repo")?.into(),
            "--json" => {}
            "--yes" if command == "seal" => yes = true,
            "--perder-desde-el-respaldo" if command == "rollback" => loss = true,
            "--all" if command == "demote" => words.push(arg.as_str()),
            x if !x.starts_with('-') => words.push(x),
            _ => return Err(format!("opción state inválida: {arg}")),
        }
    }
    if !home.is_absolute() || !proc_root.is_absolute() || !repo.is_absolute() {
        return Err("state requiere rutas absolutas".into());
    }
    if command == "rollback" && !loss {
        return Err("rollback pierde datos desde el respaldo; use demote --all para volver sin pérdida, o --perder-desde-el-respaldo explícitamente".into());
    }
    if command == "seal" && !yes {
        return Err(
            "seal requiere --yes después del OK del usuario y siete días en unified".into(),
        );
    }
    let expected = if command == "flip" { 2 } else { 1 };
    if words.len() != expected {
        return Err(format!("argumentos inválidos para state {command}"));
    }
    let name = words.first().copied().ok_or("falta dominio/run_id")?;
    let now = migrate::journal::now_ms().map_err(|e| e.to_string())?;
    let path = unified::unified_path(&home);
    if !path.is_absolute() || !path.exists() {
        return Err("state requiere una base única existente con ruta absoluta".into());
    }
    if command == "rollback" {
        for d in comandos_store::domains::catalog::DOMAINS {
            let writers = super::preflight::domain_writers(&proc_root, &home, &repo, d.name);
            if !writers.is_empty() {
                return Err(format!(
                    "rollback destructivo requiere dominio {} sin escritores; no se detendrá ninguna sesión",
                    d.name
                ));
            }
        }
        let report =
            migrate::lifecycle::rollback(&home, &path, name, now).map_err(|e| e.to_string())?;
        return Ok(
            json!({"archived_db":report.archived_db,"data_lost_since_ms":report.data_lost_since_ms,"data_lost_until_ms":report.data_lost_until_ms}),
        );
    }
    let c = unified::open_unified(&path).map_err(|e| e.to_string())?;
    match command.as_str() {
        "flip" => {
            if words.get(1) != Some(&"unified") {
                return Err("flip solo admite unified".into());
            }
            capable(&proc_root, &home, &repo, name)?;
            migrate::lifecycle::flip(&home, &c, name, now).map_err(|e| e.to_string())?;
        }
        "demote" => {
            if name == "--all" {
                migrate::lifecycle::demote_all(&home, &c, now).map_err(|e| e.to_string())?;
            } else {
                migrate::lifecycle::demote(&home, &c, name, now).map_err(|e| e.to_string())?;
            }
        }
        "seal" => {
            capable(&proc_root, &home, &repo, name)?;
            let backup =
                migrate::lifecycle::seal(&home, &c, name, now).map_err(|e| e.to_string())?;
            return Ok(json!({"domain":name,"mode":"sealed","backup_dir":backup}));
        }
        "export-legacy" => {
            capable(&proc_root, &home, &repo, name)?;
            migrate::lifecycle::export_legacy(&home, &c, name, now).map_err(|e| e.to_string())?;
        }
        _ => return Err("operación desconocida".into()),
    }
    Ok(json!({"domain":name,"operation":command}))
}
pub fn main(args: &[String]) -> i32 {
    match execute(args) {
        Ok(value) => {
            println!("{value}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
