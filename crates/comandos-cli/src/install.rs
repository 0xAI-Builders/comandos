//! Instalación en paralelo del binario: `--stage`, `--link` y `--rollback`, sin cutover implícito.
pub mod assets;
pub mod cleanup;
pub mod components;
pub mod darwin;
pub mod extensions;
pub mod full;
pub mod guard;
pub mod hooks_register;
pub mod manifest;
pub mod plan;
pub mod platform;
pub mod proxy;
mod record;
pub mod release;
pub mod retarget;
pub mod retirement;
pub mod telegram;
pub mod wsl;
pub use manifest::STATE_PROTOCOL;

use record::Record;
use std::{
    fs, io,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

/// Directorio de artefactos web que `--stage` copia a la release (T4).
const WEB_SOURCE_ENV: &str = "COMANDOS_WEB_SOURCE";
const HOOK_NAMES: &[&str] = &["cc-notify.sh", "cc-status.sh", "cc-usage-tool.sh"];
const USAGE: &str = "uso: comandos install [--home DIR] [--dry-run] (--stage [--web DIR] | --stage-app RUTA_ABSOLUTA | --link NOMBRE | --darwin-agent [--no-launchctl] | --app RUTA.app | --rollback NOMBRE [--no-launchctl] | --rollback-release | --releases)";

enum Action {
    Stage(Option<PathBuf>),
    StageApp(PathBuf),
    DarwinAgent,
    DarwinApp(PathBuf),
    Link(String),
    Rollback(String),
    RollbackRelease,
    Releases,
}

/// Devuelve el código de salida; los errores de uso salen con 2 y los de operación con `Err`.
pub fn run(args: &[String]) -> Result<i32, String> {
    if full::handles(args) {
        return full::run(args);
    }
    let Some((home, action, dry_run, no_launchctl)) = parse(args) else {
        eprintln!("{USAGE}");
        return Ok(2);
    };
    let staged = home.join(".local/share/comandos/bin/comandos");
    match action {
        Action::DarwinAgent => darwin::agent(&home, dry_run, no_launchctl)?,
        Action::DarwinApp(source) => darwin::install_app(&home, &source, dry_run)?,
        Action::Stage(flag) => {
            let me = std::env::current_exe()
                .map_err(|e| format!("no se pudo ubicar el ejecutable: {e}"))?;
            let web = web_source(flag, &me);
            if dry_run {
                let r = release::preview_release(&home, &me, &web)?;
                println!(
                    "dry-run: preparar release {} en {}; rollback: {}",
                    r.id,
                    r.path.display(),
                    home.join(".local/share/comandos/releases/previous")
                        .display()
                );
                return Ok(0);
            }
            let r = release::stage_release(&home, &me, &web)?;
            match (&web, r.web_files) {
                (
                    release::WebSource::Explicit(p) | release::WebSource::OwnRelease { dir: p, .. },
                    Some(n),
                ) => {
                    // Ruta canónica: el operador ve exactamente qué se instaló.
                    let shown = fs::canonicalize(p).unwrap_or_else(|_| p.clone());
                    println!("release {} (web: {}, {n} archivos)", r.id, shown.display())
                }
                _ => println!("release {} (sin web)", r.id),
            }
        }
        Action::StageApp(exe) => {
            let r = if dry_run {
                release::preview_app(&home, &exe)?
            } else {
                release::stage_app(&home, &exe)?
            };
            println!(
                "{}release App {} en {}; cc-app sin activar; rollback: {}",
                if dry_run { "dry-run: preparar " } else { "" },
                r.id,
                r.path.display(),
                record::path(&home, "cc-app").display()
            );
        }
        Action::RollbackRelease => {
            if dry_run {
                let r = release::preview_rollback_release(&home)?;
                println!(
                    "dry-run: restaurar release CLI {} en {}; rollback: {}; App sin activar",
                    r.id,
                    r.path.display(),
                    home.join(".local/share/comandos/releases/previous")
                        .display()
                );
                return Ok(0);
            }
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
        Action::Link(name) => {
            let name = valid(&name)?;
            if name == "cc-app" {
                link_app(&home, dry_run)?;
            } else if dry_run {
                if !staged.exists() {
                    return Err("primero: comandos install --stage".into());
                }
                preview_link(&home, &staged, name)?;
            } else {
                link(&home, &staged, name)?;
            }
        }
        Action::Rollback(name) => {
            if name == darwin::AGENT_NAME {
                darwin::rollback_agent(&home, dry_run, no_launchctl)?;
                return Ok(0);
            }
            if name == "ComandOS.app" {
                darwin::rollback_app(&home, dry_run)?;
                return Ok(0);
            }
            let name = valid(&name)?;
            if name == "cc-app" {
                check_app_rollback(&home)?;
            }
            if dry_run {
                preview_rollback(&home, name)?;
            } else if name == "cc-app" {
                let _lock = app_operation_lock(&home)?;
                check_app_rollback(&home)?;
                match check_app_record(&home)? {
                    Record::Link(target) => swap_app_symlink(&target, &link_path(&home, name))?,
                    _ => rollback(&home, name)?,
                }
            } else {
                rollback(&home, name)?;
            }
        }
    }
    Ok(0)
}

fn parse(args: &[String]) -> Option<(PathBuf, Action, bool, bool)> {
    let mut home = std::env::var("HOME").ok().map(PathBuf::from);
    let mut action = None;
    let mut web = None;
    let mut dry_run = false;
    let mut no_launchctl = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let next = match arg.as_str() {
            "--home" => {
                home = Some(PathBuf::from(it.next()?));
                continue;
            }
            "--web" => {
                // Repetido es un error de uso: no gana el último en silencio.
                if web.replace(PathBuf::from(it.next()?)).is_some() {
                    return None;
                }
                continue;
            }
            "--dry-run" => {
                if dry_run {
                    return None;
                }
                dry_run = true;
                continue;
            }
            "--stage" => Action::Stage(None),
            "--darwin-agent" => Action::DarwinAgent,
            "--app" => Action::DarwinApp(PathBuf::from(it.next()?)),
            "--no-launchctl" => {
                if no_launchctl {
                    return None;
                }
                no_launchctl = true;
                continue;
            }
            "--stage-app" => {
                let p = PathBuf::from(it.next()?);
                if !p.is_absolute() {
                    return None;
                }
                Action::StageApp(p)
            }
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
    // `--web` solo acompaña a `--stage`.
    let action = match (action?, web) {
        (Action::Stage(_), web) => Action::Stage(web),
        (_, Some(_)) => return None,
        (other, None) => other,
    };
    let home = home?;
    let app_action = matches!(&action, Action::StageApp(_))
        || matches!(&action, Action::DarwinAgent | Action::DarwinApp(_))
        || matches!(&action, Action::Link(n) | Action::Rollback(n) if n == "cc-app" || n == darwin::AGENT_NAME || n == "ComandOS.app");
    let agent_action = matches!(&action, Action::DarwinAgent)
        || matches!(&action, Action::Rollback(n) if n == darwin::AGENT_NAME);
    if no_launchctl && !agent_action {
        return None;
    }
    if (dry_run || app_action) && !home.is_absolute() {
        return None;
    }
    Some((home, action, dry_run, no_launchctl))
}

/// Origen de `web/` para `--stage` (T4). Siempre explícito: `--web DIR`, si no
/// `COMANDOS_WEB_SOURCE` (vacío = sin web). Nunca se deduce de `../web` ni de otro
/// vecino del binario: un `target/web` viejo o un `~/web` ajeno no deben colarse.
/// Única excepción: re-instalar desde `releases/<id>/comandos`, cuyo `web/` hermano
/// es de esa release (y `stage_release` comprueba que el id recalculado sea `<id>`).
fn web_source(flag: Option<PathBuf>, exe: &Path) -> release::WebSource {
    use release::WebSource;
    if let Some(p) = flag {
        return WebSource::Explicit(p);
    }
    if let Some(v) = std::env::var_os(WEB_SOURCE_ENV) {
        return if v.is_empty() {
            WebSource::None
        } else {
            WebSource::Explicit(PathBuf::from(v))
        };
    }
    let own = exe.parent().and_then(|dir| {
        let id = dir.file_name()?.to_str()?;
        let in_releases = dir.parent()?.file_name()? == "releases";
        let web = dir.join("web");
        (in_releases && valid(id).is_ok() && web.symlink_metadata().is_ok()).then(|| {
            WebSource::OwnRelease {
                dir: web,
                id: id.to_string(),
            }
        })
    });
    own.unwrap_or(WebSource::None)
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

/// The preview inspects the original but never creates parents, records or links.
fn preview_link(home: &Path, staged: &Path, name: &str) -> Result<(), String> {
    let path = link_path(home, name);
    let original = match path.symlink_metadata() {
        Ok(m) if m.file_type().is_symlink() => format!(
            "symlink {}",
            fs::read_link(&path).map_err(|e| e.to_string())?.display()
        ),
        Ok(m) if m.is_file() => format!(
            "archivo original -> {}",
            home.join(".local/share/comandos/rollback")
                .join(format!("{name}.orig"))
                .display()
        ),
        Ok(_) => return Err(format!("{} no es archivo ni symlink", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => "ausente".into(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    println!(
        "dry-run: enlazar {} -> {}; anterior: {original}; rollback: {}",
        path.display(),
        staged.display(),
        record::path(home, name).display()
    );
    Ok(())
}

fn preview_rollback(home: &Path, name: &str) -> Result<(), String> {
    let path = link_path(home, name);
    let action = match record::read(home, name)? {
        Record::File(orig) => {
            if !orig.symlink_metadata().is_ok_and(|m| m.is_file()) {
                return Err(format!("{} no es un respaldo regular", orig.display()));
            }
            format!("restaurar archivo desde {}", orig.display())
        }
        Record::Link(target) if !target.as_os_str().is_empty() => {
            format!("restaurar symlink -> {}", target.display())
        }
        Record::Link(_) => return Err("el destino anterior estaba vacío; restaurar a mano".into()),
        Record::Absent => "eliminar enlace (antes ausente)".into(),
    };
    println!(
        "dry-run: rollback {}: {action}; registro: {}",
        path.display(),
        record::path(home, name).display()
    );
    Ok(())
}

fn check_app_paths(home: &Path) -> Result<(), String> {
    for p in [
        link_path(home, "cc-app"),
        record::path(home, "cc-app"),
        home.join(".local/share/comandos/rollback/cc-app.orig"),
    ] {
        release::check_app_parents(&p)?;
    }
    Ok(())
}

fn check_app_record(home: &Path) -> Result<Record, String> {
    let p = record::path(home, "cc-app");
    if !p.symlink_metadata().is_ok_and(|m| m.is_file()) {
        return Err(format!(
            "{} no es un registro regular de rollback",
            p.display()
        ));
    }
    let rec = record::read(home, "cc-app")?;
    match &rec {
        Record::File(orig) if *orig != home.join(".local/share/comandos/rollback/cc-app.orig") => {
            return Err("el respaldo cc-app está fuera de su ruta de rollback".into());
        }
        Record::Link(target) if target.as_os_str().is_empty() => {
            return Err("destino anterior cc-app vacío".into());
        }
        _ => {}
    }
    Ok(rec)
}

fn check_app_rollback(home: &Path) -> Result<(), String> {
    check_app_paths(home)?;
    let rec = check_app_record(home)?;
    if let Record::File(orig) = &rec
        && !orig.symlink_metadata().is_ok_and(|m| m.is_file())
    {
        return Err(format!("{} no es un respaldo regular", orig.display()));
    }
    match link_path(home, "cc-app").symlink_metadata() {
        Ok(m) if m.file_type().is_symlink() => {
            let current = fs::read_link(link_path(home, "cc-app")).map_err(|e| e.to_string())?;
            if !release::is_app_artifact(home, &current) && rec != Record::Link(current) {
                return Err("cc-app fue modificado; el rollback no lo sobrescribe".into());
            }
            Ok(())
        }
        Ok(_) => Err("cc-app dejó de ser un enlace; el rollback no lo sobrescribe".into()),
        Err(e) => Err(format!("no se pudo inspeccionar cc-app: {e}")),
    }
}

fn app_link_plan(home: &Path) -> Result<(PathBuf, Option<Record>), String> {
    let staged = release::app_release(home)?;
    check_app_paths(home)?;
    let path = link_path(home, "cc-app");
    let orig = home.join(".local/share/comandos/rollback/cc-app.orig");
    let record_path = record::path(home, "cc-app");
    let prior = match record_path.symlink_metadata() {
        Ok(_) => Some(check_app_record(home)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{}: {e}", record_path.display())),
    };
    let rec = match path.symlink_metadata() {
        Ok(m) if m.file_type().is_symlink() => {
            let previous = fs::read_link(&path).map_err(|e| e.to_string())?;
            if let Some(prior) = &prior {
                if !release::is_app_artifact(home, &previous) && *prior != Record::Link(previous) {
                    return Err("cc-app fue modificado; se conserva el primer rollback".into());
                }
                if let Record::File(orig) = prior
                    && !orig.symlink_metadata().is_ok_and(|m| m.is_file())
                {
                    return Err("falta el respaldo original cc-app; no se cambia el enlace".into());
                }
                None // Updating App preserves the first original, not the last candidate.
            } else {
                Some(Record::Link(previous))
            }
        }
        Ok(m) if m.is_file() => {
            match orig.symlink_metadata() {
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                _ => {
                    return Err(format!(
                        "{} ya existe: no se sobrescribe el original",
                        orig.display()
                    ));
                }
            }
            if prior
                .as_ref()
                .is_some_and(|p| *p != Record::File(orig.clone()))
            {
                return Err("cc-app fue modificado; se conserva el primer rollback".into());
            }
            Some(Record::File(orig.clone()))
        }
        Ok(_) => return Err(format!("{} no es archivo ni symlink", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if prior.as_ref().is_some_and(|p| *p != Record::Absent) {
                return Err("cc-app está ausente; se conserva el primer rollback".into());
            }
            prior.is_none().then_some(Record::Absent)
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    Ok((staged, rec))
}

fn app_operation_lock(home: &Path) -> Result<fs::File, String> {
    let dir = home.join(".local/share/comandos/rollback");
    let file = fs::File::open(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    file.lock().map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(file)
}

fn link_app(home: &Path, dry_run: bool) -> Result<(), String> {
    check_app_paths(home)?;
    let rollback_dir = home.join(".local/share/comandos/rollback");
    let held = if !dry_run && rollback_dir.symlink_metadata().is_ok() {
        Some(app_operation_lock(home)?)
    } else {
        None
    };
    let (staged, _) = app_link_plan(home)?;
    if dry_run {
        return preview_link(home, &staged, "cc-app");
    }
    let path = link_path(home, "cc-app");
    let orig = home.join(".local/share/comandos/rollback/cc-app.orig");
    let record_path = record::path(home, "cc-app");
    for dir in [path.parent(), record_path.parent()].into_iter().flatten() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let _lock = match held {
        Some(lock) => lock,
        None => app_operation_lock(home)?,
    };
    // Another installer may have changed the original while this one waited.
    let (staged, rec) = app_link_plan(home)?;
    let mut moved = false;
    if let Some(rec) = &rec {
        if let Record::File(orig) = rec {
            move_file(&path, orig)?;
            moved = true;
        }
        if let Err(e) = write_app_record(home, rec) {
            if moved {
                let _ = fs::rename(&orig, &path);
            }
            return Err(e);
        }
    }
    swap_app_symlink(&staged, &path).inspect_err(|_| {
        if moved {
            let _ = fs::rename(&orig, &path);
        }
    })
}

fn swap_app_symlink(target: &Path, path: &Path) -> Result<(), String> {
    let tmp = path.with_file_name(format!("cc-app.comandos-tmp.{}", std::process::id()));
    symlink(target, &tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

fn write_app_record(home: &Path, rec: &Record) -> Result<(), String> {
    use std::{io::Write, os::unix::ffi::OsStrExt};
    let path = record::path(home, "cc-app");
    let tmp = path.with_file_name(format!("cc-app.target.tmp.{}", std::process::id()));
    let raw = match rec {
        Record::Absent => b"ABSENT\n".to_vec(),
        Record::File(p) => [b"FILE:".as_slice(), p.as_os_str().as_bytes(), b"\n"].concat(),
        Record::Link(p) => [b"LINK:".as_slice(), p.as_os_str().as_bytes(), b"\n"].concat(),
    };
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    let result = file
        .write_all(&raw)
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&tmp, &path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(|e| format!("{}: {e}", path.display()))
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
