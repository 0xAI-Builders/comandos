//! Instalación en paralelo del binario: `--stage`, `--link` y `--rollback`, sin cutover implícito.
use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

const HOOK_NAMES: &[&str] = &["cc-notify.sh", "cc-status.sh", "cc-usage-tool.sh"];

pub fn run(args: &[String]) -> Result<i32, String> {
    let mut home = std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| "HOME no definido")?;
    let mut i = 0;
    let mut action: Option<(&str, Option<String>)> = None;
    while i < args.len() {
        match args[i].as_str() {
            "--home" => {
                home = PathBuf::from(args.get(i + 1).ok_or("--home requiere ruta")?);
                i += 2;
            }
            "--stage" => {
                action = Some(("stage", None));
                i += 1;
            }
            "--link" | "--rollback" => {
                let name = args.get(i + 1).ok_or("falta nombre")?.clone();
                action = Some((args[i].trim_start_matches("--"), Some(name)));
                i += 2;
            }
            other => return Err(format!("argumento desconocido: {other}")),
        }
    }
    let staged = home.join(".local/share/comandos/bin/comandos");
    match action {
        Some(("stage", _)) => stage(&staged),
        Some(("link", Some(name))) => link(&home, &staged, &name),
        Some(("rollback", Some(name))) => rollback(&home, &name),
        _ => Err(
            "uso: comandos install [--home DIR] (--stage | --link NOMBRE | --rollback NOMBRE)"
                .into(),
        ),
    }?;
    Ok(0)
}

fn link_path(home: &Path, name: &str) -> PathBuf {
    if HOOK_NAMES.contains(&name) {
        home.join(".claude/hooks").join(name)
    } else {
        home.join(".local/bin").join(name)
    }
}

/// Reemplazo atómico: symlink temporal junto al destino y `rename` encima.
fn swap_symlink(target: &Path, path: &Path) -> Result<(), String> {
    let tmp = path.with_extension("comandos-tmp");
    let _ = fs::remove_file(&tmp);
    symlink(target, &tmp).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn stage(staged: &Path) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    fs::create_dir_all(staged.parent().ok_or("ruta sin directorio")?).map_err(|e| e.to_string())?;
    let tmp = staged.with_extension("tmp");
    // `fs::copy` conserva los permisos (bit ejecutable) del origen.
    fs::copy(&me, &tmp).map_err(|e| e.to_string())?;
    fs::rename(&tmp, staged).map_err(|e| e.to_string())
}

fn link(home: &Path, staged: &Path, name: &str) -> Result<(), String> {
    if !staged.exists() {
        return Err("primero: comandos install --stage".into());
    }
    let path = link_path(home, name);
    fs::create_dir_all(path.parent().ok_or("ruta sin directorio")?).map_err(|e| e.to_string())?;
    let rollback_dir = home.join(".local/share/comandos/rollback");
    fs::create_dir_all(&rollback_dir).map_err(|e| e.to_string())?;
    let previous = fs::read_link(&path).unwrap_or_default();
    // Si ya apunta al binario instalado, conservar el registro previo (no perder el destino original).
    if previous != staged {
        let record = format!("{}\n", previous.to_string_lossy());
        fs::write(rollback_dir.join(format!("{name}.target")), record)
            .map_err(|e| e.to_string())?;
    }
    swap_symlink(staged, &path)
}

fn rollback(home: &Path, name: &str) -> Result<(), String> {
    let record = home
        .join(".local/share/comandos/rollback")
        .join(format!("{name}.target"));
    let target =
        fs::read_to_string(record).map_err(|_| format!("sin registro de rollback para {name}"))?;
    let target = target.trim();
    if target.is_empty() {
        return Err("el destino anterior estaba vacío; restaurar a mano".into());
    }
    swap_symlink(Path::new(target), &link_path(home, name))
}
