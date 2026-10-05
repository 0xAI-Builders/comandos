//! Releases versionadas del binario: `releases/<sha12>/comandos` y el enlace relativo
//! `bin/comandos -> ../releases/<sha12>/comandos`. Un daemon en ejecución conserva su
//! inodo; `--rollback-release` intercambia la release actual con `releases/previous`.
//!
//! Con artefactos web (T4) la release es `releases/<id>/{comandos,web/}` y el id es
//! `sha12(binario ‖ árbol de web/)`, que incluye `manifest.json` (preflight R14): un
//! `web/` distinto con el mismo binario nunca reutiliza una release. Sin `web/` el id
//! sigue siendo `sha12(binario)`, así las releases ya instaladas y su rollback valen.
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    os::unix::{
        ffi::OsStrExt,
        fs::{PermissionsExt, symlink},
    },
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

/// Instala `exe` (y `web`, si se da) como release y apunta `bin/comandos` a ella.
pub fn stage_release(home: &Path, exe: &Path, web: Option<&Path>) -> Result<Release, String> {
    let Layout { releases, bin } = layout(home);
    let bin_dir = bin.parent().ok_or("ruta de destino sin directorio")?;
    for dir in [&releases, bin_dir] {
        fs::create_dir_all(dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
    }
    let binary = fs::read(exe).map_err(|e| format!("no se pudo leer {}: {e}", exe.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&binary);
    // `web/` se copia a una preparación mientras se calcula el id; luego entra con un
    // `rename` atómico, antes que el binario (la release está completa cuando existe
    // `comandos`).
    let staged_web = match web {
        Some(src) => {
            let staging = releases.join(format!(".web-stage.{}", std::process::id()));
            let _ = fs::remove_dir_all(&staging);
            let copied = copy_web(src, &staging.join("web"), &mut hasher);
            if let Err(e) = copied {
                let _ = fs::remove_dir_all(&staging);
                return Err(e);
            }
            Some(staging)
        }
        None => None,
    };
    let id = hex12(hasher.finalize().as_slice());
    let dir = releases.join(&id);
    let placed = place_web(&dir, staged_web.as_deref());
    if let Some(staging) = &staged_web {
        let _ = fs::remove_dir_all(staging);
    }
    placed?;
    let target = dir.join("comandos");
    // 1) La release nueva queda completa antes de tocar `bin/comandos`.
    if !target.is_file() {
        // Se escriben los bytes ya leídos (los del hash), con 0755; rename deja la
        // release completa o ausente.
        let tmp = dir.join(format!("comandos.tmp.{}", std::process::id()));
        fs::write(&tmp, &binary)
            .and_then(|()| fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755)))
            .map_err(|e| {
                let _ = fs::remove_file(&tmp);
                format!("no se pudo copiar a {}: {e}", tmp.display())
            })?;
        fs::rename(&tmp, &target)
            .map_err(|e| format!("no se pudo instalar {}: {e}", target.display()))?;
    }
    // 2) Una instalación de la Fase 1 (archivo regular) pasa a ser la release anterior,
    //    con hard link (mismo inodo) para que `bin/comandos` nunca deje de existir.
    let previous = match bin.symlink_metadata() {
        Ok(m) if m.file_type().is_symlink() => current_id(&releases, &bin),
        Ok(_) => {
            let old = sha12(&bin)?;
            let old_dir = releases.join(&old);
            fs::create_dir_all(&old_dir)
                .map_err(|e| format!("no se pudo crear {}: {e}", old_dir.display()))?;
            let dest = old_dir.join("comandos");
            if !dest.is_file() {
                keep_legacy(&bin, &dest, &old_dir)?;
            }
            Some(old)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("no se pudo leer {}: {e}", bin.display())),
    };
    // 3) El swap del symlink temporal sobre `bin` es atómico.
    if previous.as_deref() != Some(id.as_str())
        && let Some(p) = &previous
    {
        write_previous(&releases, p)?;
    }
    if current_id(&releases, &bin).as_deref() != Some(id.as_str()) {
        swap_link(&bin, &id)?;
    }
    prune(&releases, &id)?;
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
    let current =
        current_id(&releases, &bin).ok_or("bin/comandos no es un enlace a una release")?;
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
    let current = current_id(&releases, &bin);
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
        if !meta.is_dir() || !entry.path().join("comandos").is_file() {
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
    let tmp = bin.with_extension(format!("tmp-link.{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    symlink(Path::new("../releases").join(id).join("comandos"), &tmp)
        .map_err(|e| format!("no se pudo crear {}: {e}", tmp.display()))?;
    fs::rename(&tmp, bin).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("no se pudo enlazar {}: {e}", bin.display())
    })
}

/// Id al que apunta `bin`, solo si es una release existente.
fn current_id(releases: &Path, bin: &Path) -> Option<String> {
    let target = fs::read_link(bin).ok()?;
    let id = target.parent()?.file_name()?.to_str()?.to_string();
    (valid_id(&id) && releases.join(&id).join("comandos").is_file()).then_some(id)
}

/// Hard link al inodo vivo; si el sistema de archivos no lo permite, copia.
fn keep_legacy(bin: &Path, dest: &Path, dir: &Path) -> Result<(), String> {
    if fs::hard_link(bin, dest).is_ok() {
        return Ok(());
    }
    let tmp = dir.join(format!("comandos.tmp.{}", std::process::id()));
    fs::copy(bin, &tmp).map_err(|e| format!("no se pudo copiar a {}: {e}", tmp.display()))?;
    fs::rename(&tmp, dest).map_err(|e| format!("no se pudo instalar {}: {e}", dest.display()))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && !id.contains('/') && id != "." && id != ".."
}

fn sha12(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("no se pudo leer {}: {e}", path.display()))?;
    Ok(hex12(Sha256::digest(bytes).as_slice()))
}

/// Primeros 12 dígitos hexadecimales de un digest.
fn hex12(digest: &[u8]) -> String {
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Crea `dir` y deja en `dir/web` el `web/` preparado, si lo hay y aún no está. El
/// id incluye el contenido de `web/`, así que un `dir/web` existente es idéntico.
fn place_web(dir: &Path, staging: Option<&Path>) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
    let Some(staging) = staging else {
        return Ok(());
    };
    let dest = dir.join("web");
    if dest.symlink_metadata().is_ok() {
        return Ok(());
    }
    let from = staging.join("web");
    fs::rename(&from, &dest).map_err(|e| format!("no se pudo instalar {}: {e}", dest.display()))
}

/// Entrada del árbol de `web/`: ruta relativa y si es directorio.
struct WebEntry {
    rel: PathBuf,
    dir: bool,
}

/// Recorre `root` sin seguir enlaces simbólicos: un enlace simbólico (o un fifo,
/// socket, dispositivo) en `web/` es un error, no se copia ni se sigue. Orden por
/// bytes de la ruta relativa, para que el hash no dependa de `read_dir`.
fn web_tree(root: &Path) -> Result<Vec<WebEntry>, String> {
    let meta = root
        .symlink_metadata()
        .map_err(|e| format!("no se pudo leer {}: {e}", root.display()))?;
    if !meta.is_dir() {
        return Err(format!(
            "{} no es un directorio (los enlaces simbólicos no se siguen)",
            root.display()
        ));
    }
    let mut out = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(rel) = pending.pop() {
        let abs = root.join(&rel);
        let rd =
            fs::read_dir(&abs).map_err(|e| format!("no se pudo leer {}: {e}", abs.display()))?;
        for entry in rd {
            let entry = entry.map_err(|e| format!("no se pudo leer {}: {e}", abs.display()))?;
            let kind = entry
                .file_type()
                .map_err(|e| format!("no se pudo leer {}: {e}", entry.path().display()))?;
            let child = rel.join(entry.file_name());
            if kind.is_symlink() {
                return Err(format!(
                    "{} es un enlace simbólico: web/ solo admite archivos y directorios",
                    entry.path().display()
                ));
            } else if kind.is_dir() {
                pending.push(child.clone());
                out.push(WebEntry {
                    rel: child,
                    dir: true,
                });
            } else if kind.is_file() {
                out.push(WebEntry {
                    rel: child,
                    dir: false,
                });
            } else {
                return Err(format!(
                    "{} no es archivo ni directorio",
                    entry.path().display()
                ));
            }
        }
    }
    out.sort_by(|a, b| {
        a.rel
            .as_os_str()
            .as_bytes()
            .cmp(b.rel.as_os_str().as_bytes())
    });
    Ok(out)
}

/// Copia `src` a `dest` (nuevo) con permisos 0644/0755 y alimenta `hasher` con cada
/// entrada: tipo, ruta relativa, longitud y contenido. Cada archivo se lee una vez,
/// así el hash es exactamente de lo copiado.
fn copy_web(src: &Path, dest: &Path, hasher: &mut Sha256) -> Result<(), String> {
    let entries = web_tree(src)?;
    make_dir(dest)?;
    for e in entries {
        let to = dest.join(&e.rel);
        let rel = e.rel.as_os_str().as_bytes();
        if e.dir {
            hasher.update(b"d");
            hasher.update(rel);
            hasher.update([0]);
            make_dir(&to)?;
            continue;
        }
        let from = src.join(&e.rel);
        let bytes =
            fs::read(&from).map_err(|e| format!("no se pudo leer {}: {e}", from.display()))?;
        hasher.update(b"f");
        hasher.update(rel);
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
        fs::write(&to, &bytes).map_err(|e| format!("no se pudo escribir {}: {e}", to.display()))?;
        fs::set_permissions(&to, fs::Permissions::from_mode(0o644))
            .map_err(|e| format!("no se pudo ajustar {}: {e}", to.display()))?;
    }
    Ok(())
}

fn make_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("no se pudo ajustar {}: {e}", dir.display()))
}

/// Conserva las `KEEP` más recientes por mtime más la actual y `previous`.
fn prune(releases: &Path, current: &str) -> Result<(), String> {
    // `previous` se lee de disco: es el que `--rollback-release` necesitará.
    let on_disk = fs::read_to_string(releases.join("previous")).ok();
    let previous = on_disk.as_deref().map(str::trim);
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
