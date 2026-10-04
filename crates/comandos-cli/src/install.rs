//! Instalación en paralelo del binario: `--stage`, `--link` y `--rollback`, sin cutover implícito.
mod record;
pub mod release;

use record::Record;
use std::{
    fs, io,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

const HOOK_NAMES: &[&str] = &["cc-notify.sh", "cc-status.sh", "cc-usage-tool.sh"];
const USAGE: &str = "uso: comandos install [--home DIR] (--stage | --link NOMBRE | --rollback NOMBRE | --rollback-release | --releases)";

enum Action {
    Stage,
    Link(String),
    Rollback(String),
    RollbackRelease,
    Releases,
}

/// Devuelve el código de salida; los errores de uso salen con 2 y los de operación con `Err`.
pub fn run(args: &[String]) -> Result<i32, String> {
    let Some((home, action)) = parse(args) else {
        eprintln!("{USAGE}");
        return Ok(2);
    };
    let staged = home.join(".local/share/comandos/bin/comandos");
    match action {
        Action::Stage => {
            let me = std::env::current_exe()
                .map_err(|e| format!("no se pudo ubicar el ejecutable: {e}"))?;
            release::stage_release(&home, &me)?;
        }
        Action::RollbackRelease => {
            let r = release::rollback_release(&home)?;
            println!("release activa: {}", r.id);
        }
        Action::Releases => {
            for r in release::list_releases(&home)? {
                println!(
                    "{} {}  {}",
                    if r.current { '*' } else { ' ' },
                    r.id,
                    r.path.display()
                );
            }
        }
        Action::Link(name) => link(&home, &staged, valid(&name)?)?,
        Action::Rollback(name) => rollback(&home, valid(&name)?)?,
    }
    Ok(0)
}

fn parse(args: &[String]) -> Option<(PathBuf, Action)> {
    let mut home = std::env::var("HOME").ok().map(PathBuf::from);
    let mut action = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let next = match arg.as_str() {
            "--home" => {
                home = Some(PathBuf::from(it.next()?));
                continue;
            }
            "--stage" => Action::Stage,
            "--link" => Action::Link(it.next()?.clone()),
            "--rollback-release" => Action::RollbackRelease,
            "--releases" => Action::Releases,
            "--rollback" => Action::Rollback(it.next()?.clone()),
            _ => return None,
        };
        if action.replace(next).is_some() {
            return None;
        }
    }
    Some((home?, action?))
}

fn valid(name: &str) -> Result<&str, String> {
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err(format!("nombre inválido: {name:?}"));
    }
    Ok(name)
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
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("link");
    let tmp = path.with_file_name(format!("{name}.comandos-tmp.{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    symlink(target, &tmp).map_err(|e| {
        format!(
            "no se pudo crear el symlink temporal {}: {e}",
            tmp.display()
        )
    })?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("no se pudo reemplazar {}: {e}", path.display())
    })
}

/// Mueve un archivo real; si cruza dispositivos (EXDEV) copia y borra.
fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(18) => fs::copy(from, to)
            .and_then(|_| fs::remove_file(from))
            .map_err(|e| {
                format!(
                    "no se pudo mover {} a {}: {e}",
                    from.display(),
                    to.display()
                )
            }),
        Err(e) => Err(format!(
            "no se pudo mover {} a {}: {e}",
            from.display(),
            to.display()
        )),
    }
}

fn link(home: &Path, staged: &Path, name: &str) -> Result<(), String> {
    if !staged.exists() {
        return Err("primero: comandos install --stage".into());
    }
    let path = link_path(home, name);
    let dir = path.parent().ok_or("ruta de enlace sin directorio")?;
    fs::create_dir_all(dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
    let rollback_dir = home.join(".local/share/comandos/rollback");
    fs::create_dir_all(&rollback_dir)
        .map_err(|e| format!("no se pudo crear {}: {e}", rollback_dir.display()))?;
    let mut backup = None;
    match fs::symlink_metadata(&path) {
        Ok(m) if m.file_type().is_symlink() => {
            let previous = fs::read_link(&path)
                .map_err(|e| format!("no se pudo leer {}: {e}", path.display()))?;
            // Ya apunta al binario instalado: conservar el registro original.
            if previous != staged || !record::exists(home, name) {
                record::write(home, name, &Record::Link(previous))?;
            }
        }
        Ok(m) if m.is_file() => {
            let orig = rollback_dir.join(format!("{name}.orig"));
            move_file(&path, &orig)?;
            record::write(home, name, &Record::File(orig.clone()))?;
            backup = Some(orig);
        }
        Ok(_) => return Err(format!("{} no es archivo ni symlink", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            record::write(home, name, &Record::Absent)?;
        }
        Err(e) => return Err(format!("no se pudo inspeccionar {}: {e}", path.display())),
    }
    swap_symlink(staged, &path).inspect_err(|_| {
        if let Some(orig) = &backup {
            let _ = fs::rename(orig, &path);
        }
    })
}

fn rollback(home: &Path, name: &str) -> Result<(), String> {
    let path = link_path(home, name);
    match record::read(home, name)? {
        Record::File(orig) => fs::rename(&orig, &path).map_err(|e| {
            format!(
                "no se pudo restaurar {} desde {}: {e}",
                path.display(),
                orig.display()
            )
        }),
        Record::Absent => {
            fs::remove_file(&path)
                .map_err(|e| format!("no se pudo eliminar {}: {e}", path.display()))?;
            println!("no había nada antes; symlink eliminado");
            Ok(())
        }
        Record::Link(target) if target.as_os_str().is_empty() => {
            Err("el destino anterior estaba vacío; restaurar a mano".into())
        }
        Record::Link(target) => swap_symlink(&target, &path),
    }
}
