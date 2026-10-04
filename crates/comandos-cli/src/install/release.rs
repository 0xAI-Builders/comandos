//! Releases versionadas del binario: `releases/<sha12>/comandos` y el enlace relativo
//! `bin/comandos -> ../releases/<sha12>/comandos`. Un daemon en ejecución conserva su
//! inodo; `--rollback-release` intercambia la release actual con `releases/previous`.
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    time::SystemTime,
};

pub struct Release {
    pub id: String,
    pub path: PathBuf,
    pub current: bool,
}

/// Releases recientes que se conservan (la actual y `previous` siempre se conservan).
const KEEP: usize = 5;

struct Layout {
    releases: PathBuf,
    bin: PathBuf,
}

fn layout(home: &Path) -> Layout {
    let share = home.join(".local/share/comandos");
    Layout {
        releases: share.join("releases"),
        bin: share.join("bin/comandos"),
    }
}

pub fn stage_release(home: &Path, exe: &Path) -> Result<Release, String> {
    let Layout { releases, bin } = layout(home);
    let bin_dir = bin.parent().ok_or("ruta de destino sin directorio")?;
    for dir in [&releases, bin_dir] {
        fs::create_dir_all(dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
    }
    // Una instalación de la Fase 1 (archivo regular) pasa a ser la release anterior.
    let previous = match bin.symlink_metadata() {
        Ok(m) if m.file_type().is_symlink() => current_id(&bin),
        Ok(_) => {
            let id = sha12(&bin)?;
            let dir = releases.join(&id);
            fs::create_dir_all(&dir)
                .map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
            let dest = dir.join("comandos");
            if dest.exists() {
                fs::remove_file(&bin)
                    .map_err(|e| format!("no se pudo borrar {}: {e}", bin.display()))?;
            } else {
                super::move_file(&bin, &dest)?;
            }
            Some(id)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("no se pudo leer {}: {e}", bin.display())),
    };
    let id = sha12(exe)?;
    let dir = releases.join(&id);
    fs::create_dir_all(&dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
    let target = dir.join("comandos");
    if !target.is_file() {
        // `fs::copy` conserva el bit ejecutable; rename deja la release completa o ausente.
        let tmp = dir.join("comandos.tmp");
        fs::copy(exe, &tmp).map_err(|e| format!("no se pudo copiar a {}: {e}", tmp.display()))?;
        fs::rename(&tmp, &target)
            .map_err(|e| format!("no se pudo instalar {}: {e}", target.display()))?;
    }
    if previous.as_deref() != Some(id.as_str()) {
        if let Some(p) = &previous {
            write_previous(&releases, p)?;
        }
        swap_link(&bin, &id)?;
    } else if current_id(&bin).as_deref() != Some(id.as_str()) {
        // Caso del archivo regular idéntico al binario nuevo: solo falta el enlace.
        swap_link(&bin, &id)?;
    }
    prune(&releases, &id, previous.as_deref())?;
    Ok(Release {
        id,
        path: target,
        current: true,
    })
}

pub fn rollback_release(home: &Path) -> Result<Release, String> {
    let Layout { releases, bin } = layout(home);
    let prev = fs::read_to_string(releases.join("previous"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| valid_id(s) && releases.join(s).join("comandos").is_file())
        .ok_or("no hay release anterior a la que volver")?;
    let current = current_id(&bin).ok_or("bin/comandos no es un enlace a una release")?;
    swap_link(&bin, &prev)?;
    write_previous(&releases, &current)?;
    Ok(Release {
        path: releases.join(&prev).join("comandos"),
        id: prev,
        current: true,
    })
}

/// Actual primero y luego las demás de más a menos reciente.
pub fn list_releases(home: &Path) -> Result<Vec<Release>, String> {
    let Layout { releases, bin } = layout(home);
    let current = current_id(&bin);
    let mut found = release_dirs(&releases)?;
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut out: Vec<Release> = found
        .into_iter()
        .map(|(id, _)| Release {
            current: current.as_deref() == Some(id.as_str()),
            path: releases.join(&id).join("comandos"),
            id,
        })
        .collect();
    out.sort_by_key(|r| !r.current); // estable: conserva el orden por mtime
    Ok(out)
}

fn release_dirs(releases: &Path) -> Result<Vec<(String, SystemTime)>, String> {
    let rd = match fs::read_dir(releases) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("no se pudo leer {}: {e}", releases.display())),
    };
    let mut found = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| e.to_string())?;
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        if !meta.is_dir() {
            continue;
        }
        let Some(id) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        found.push((id, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
    }
    Ok(found)
}

fn write_previous(releases: &Path, id: &str) -> Result<(), String> {
    let path = releases.join("previous");
    fs::write(&path, format!("{id}\n"))
        .map_err(|e| format!("no se pudo escribir {}: {e}", path.display()))
}

fn swap_link(bin: &Path, id: &str) -> Result<(), String> {
    let tmp = bin.with_extension("tmp-link");
    let _ = fs::remove_file(&tmp);
    symlink(Path::new("../releases").join(id).join("comandos"), &tmp)
        .map_err(|e| format!("no se pudo crear {}: {e}", tmp.display()))?;
    fs::rename(&tmp, bin).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("no se pudo enlazar {}: {e}", bin.display())
    })
}

fn current_id(bin: &Path) -> Option<String> {
    let target = fs::read_link(bin).ok()?;
    target.parent()?.file_name()?.to_str().map(str::to_string)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && !id.contains('/') && id != "." && id != ".."
}

fn sha12(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("no se pudo leer {}: {e}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes))[..12].to_string())
}

/// Conserva las `KEEP` más recientes por mtime más la actual y `previous`.
fn prune(releases: &Path, current: &str, previous: Option<&str>) -> Result<(), String> {
    let mut found = release_dirs(releases)?;
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (i, (id, _)) in found.iter().enumerate() {
        if i < KEEP || id == current || Some(id.as_str()) == previous {
            continue;
        }
        let dir = releases.join(id);
        fs::remove_dir_all(&dir)
            .map_err(|e| format!("no se pudo borrar {}: {e}", dir.display()))?;
    }
    Ok(())
}
