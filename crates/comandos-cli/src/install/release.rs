//! Releases versionadas del binario: `releases/<sha12>/comandos` y el enlace relativo
//! `bin/comandos -> ../releases/<sha12>/comandos`. Un daemon en ejecución conserva su
//! inodo; `--rollback-release` intercambia la release actual con `releases/previous`.
//!
//! Con artefactos web (T4) la release es `releases/<id>/{comandos,web/}` y el id es
//! `sha12(binario ‖ "web\0" ‖ árbol de web/)`, que incluye `manifest.json` (preflight
//! R14): un `web/` distinto con el mismo binario nunca reutiliza una release, y un
//! `web/` vacío no equivale a «sin web». Sin `web/` el id sigue siendo
//! `sha12(binario)`, así las releases ya instaladas y su rollback valen. Un `web/` sin
//! `manifest.json` válido se rechaza.
use comandos_core::web_assets::{MANIFEST_FILE, Manifest};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt, symlink},
    },
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

pub struct Release {
    pub id: String,
    pub path: PathBuf,
    pub current: bool,
    /// Archivos de `web/` instalados con esta operación (`None`: sin web).
    pub web_files: Option<usize>,
}

/// De dónde sale el `web/` de una release.
pub enum WebSource {
    /// Sin artefactos web.
    None,
    /// `--web DIR` o `COMANDOS_WEB_SOURCE`.
    Explicit(PathBuf),
    /// El `web/` de la release instalada que se re-instala (`releases/<id>/web`): el
    /// id recalculado tiene que ser `id`, si no la release fue alterada.
    OwnRelease { dir: PathBuf, id: String },
}

/// Releases recientes que se conservan (la actual y `previous` siempre se conservan).
const KEEP: usize = 5;

/// Edad a partir de la cual una preparación o una release sin `comandos` se da por
/// abandonada (un `--stage` dura segundos).
const ABANDONED: Duration = Duration::from_secs(3600);

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

/// Instala `exe` (y su `web/`, si lo hay) como release y apunta `bin/comandos` a ella.
pub fn stage_release(home: &Path, exe: &Path, web: &WebSource) -> Result<Release, String> {
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
    let (src, expected) = match web {
        WebSource::None => (None, None),
        WebSource::Explicit(p) => (Some(p.as_path()), None),
        WebSource::OwnRelease { dir, id } => (Some(dir.as_path()), Some(id.as_str())),
    };
    let staging = releases.join(format!(".web-stage.{}", std::process::id()));
    let staged = src.map(|src| {
        let _ = fs::remove_dir_all(&staging);
        stage_web(src, &staging.join("web"), &mut hasher)
    });
    let id = hex12(hasher.finalize().as_slice());
    let placed = match staged {
        None => Ok(None),
        Some(Err(e)) => Err(e),
        Some(Ok(_)) if expected.is_some_and(|x| x != id) => Err(format!(
            "la release {} no coincide con su contenido (id recalculado {id}): no se re-instala",
            expected.unwrap_or_default()
        )),
        Some(Ok(n)) => place_web(&releases.join(&id), &staging.join("web")).map(|()| Some(n)),
    };
    if src.is_some() {
        let _ = fs::remove_dir_all(&staging);
    }
    let web_files = placed?;
    let dir = releases.join(&id);
    fs::create_dir_all(&dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
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
        web_files,
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
        web_files: None,
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
            web_files: None,
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

/// Deja en `dir/web` el `web/` preparado en `staged`. El id fija el contenido, así
/// que un `dir/web` existente es idéntico. Nunca se mueve nada dentro de una release
/// completa (con `comandos`) que no tenga ya su `web/`: esa release es otra cosa.
fn place_web(dir: &Path, staged: &Path) -> Result<(), String> {
    let dest = dir.join("web");
    if dest.symlink_metadata().is_ok() {
        return Ok(());
    }
    if dir.join("comandos").symlink_metadata().is_ok() {
        return Err(format!(
            "{} ya es una release completa sin web/: no se modifica",
            dir.display()
        ));
    }
    fs::create_dir_all(dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
    fs::rename(staged, &dest).map_err(|e| format!("no se pudo instalar {}: {e}", dest.display()))
}

/// Copia `src` a `dest` (ver `copy_web`) y valida la copia: `manifest.json` regular,
/// legible como `Manifest`, con rutas llanas que existen como archivos. Devuelve el
/// número de archivos copiados.
fn stage_web(src: &Path, dest: &Path, hasher: &mut Sha256) -> Result<usize, String> {
    let n = copy_web(src, dest, hasher)?;
    let shown = src.join(MANIFEST_FILE);
    let path = dest.join(MANIFEST_FILE);
    let bytes = match path.symlink_metadata() {
        Ok(m) if m.is_file() => {
            fs::read(&path).map_err(|e| format!("no se pudo leer {}: {e}", shown.display()))?
        }
        _ => {
            return Err(format!(
                "{} no existe: web/ necesita su manifiesto (¿falló `xtask web-build`?)",
                shown.display()
            ));
        }
    };
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|e| format!("{} no es un manifiesto válido: {e}", shown.display()))?;
    manifest
        .check_paths()
        .map_err(|e| format!("{}: {e}", shown.display()))?;
    for rel in manifest.files.values() {
        if !dest.join(rel).symlink_metadata().is_ok_and(|m| m.is_file()) {
            return Err(format!(
                "{}: {rel} no está en {}",
                shown.display(),
                src.display()
            ));
        }
    }
    Ok(n)
}

/// Entrada del árbol de `web/`: ruta relativa, si es directorio y su inodo (para
/// comprobar al leer que sigue siendo el mismo archivo).
struct WebEntry {
    rel: PathBuf,
    dir: bool,
    inode: (u64, u64),
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
            // `DirEntry::metadata` no sigue enlaces simbólicos.
            let meta = entry
                .metadata()
                .map_err(|e| format!("no se pudo leer {}: {e}", entry.path().display()))?;
            let inode = (meta.dev(), meta.ino());
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
                    inode,
                });
            } else if kind.is_file() {
                out.push(WebEntry {
                    rel: child,
                    dir: false,
                    inode,
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

/// Copia `src` a `dest` (nuevo) con permisos 0644/0755 y alimenta `hasher` con un
/// marcador de raíz y cada entrada: tipo, ruta relativa, longitud y contenido. Cada
/// archivo se lee una vez, así el hash es exactamente de lo copiado. Devuelve el
/// número de archivos.
fn copy_web(src: &Path, dest: &Path, hasher: &mut Sha256) -> Result<usize, String> {
    let entries = web_tree(src)?;
    make_dir(dest)?;
    // Marcador de raíz: un `web/` vacío no da el mismo id que «sin web».
    hasher.update(b"web\0");
    let mut files = 0;
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
        let bytes = read_same(&from, e.inode)?;
        files += 1;
        hasher.update(b"f");
        hasher.update(rel);
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
        fs::write(&to, &bytes).map_err(|e| format!("no se pudo escribir {}: {e}", to.display()))?;
        fs::set_permissions(&to, fs::Permissions::from_mode(0o644))
            .map_err(|e| format!("no se pudo ajustar {}: {e}", to.display()))?;
    }
    Ok(files)
}

/// Lee `path` solo si al abrirlo sigue siendo el archivo regular `inode` que vio el
/// recorrido: un cambio por un enlace simbólico entre recorrer y leer es un error.
fn read_same(path: &Path, inode: (u64, u64)) -> Result<Vec<u8>, String> {
    let err = |e: io::Error| format!("no se pudo leer {}: {e}", path.display());
    let mut file = fs::File::open(path).map_err(err)?;
    let meta = file.metadata().map_err(err)?;
    if !meta.is_file() || (meta.dev(), meta.ino()) != inode {
        return Err(format!("{} cambió durante la copia", path.display()));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(err)?;
    Ok(bytes)
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
    prune_abandoned(releases, SystemTime::now());
    Ok(())
}

/// Borra preparaciones `.web-stage.*` y directorios `<sha12>/` sin `comandos` más
/// viejos que `ABANDONED` (restos de un `--stage` cortado). Los recientes pueden ser
/// de otro `--stage` en curso y se respetan. Mejor esfuerzo: un fallo no aborta.
fn prune_abandoned(releases: &Path, now: SystemTime) {
    let Ok(rd) = fs::read_dir(releases) else {
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let leftover = name.starts_with(".web-stage.")
            || (is_sha12(&name) && path.join("comandos").symlink_metadata().is_err());
        let old = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > ABANDONED);
        if meta.is_dir() && leftover && old {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

fn is_sha12(name: &str) -> bool {
    name.len() == 12 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cmd-rel-unit-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn place_web_never_moves_into_a_complete_release() {
        let root = scratch("place");
        let release = root.join("0123456789ab");
        fs::create_dir_all(&release).unwrap();
        fs::write(release.join("comandos"), b"bin").unwrap();
        let staged = root.join("stage/web");
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("manifest.json"), b"{\"files\":{}}").unwrap();
        let err = place_web(&release, &staged).unwrap_err();
        assert!(err.contains("release completa"), "{err}");
        assert!(!release.join("web").exists());
        assert!(staged.join("manifest.json").exists());
        // Una release a medias (sin `comandos`) sí lo recibe.
        let half = root.join("ba9876543210");
        place_web(&half, &staged).unwrap();
        assert!(half.join("web/manifest.json").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_web_never_hashes_like_no_web() {
        let root = scratch("marker");
        let empty = root.join("empty");
        fs::create_dir_all(&empty).unwrap();
        let mut with = Sha256::new();
        with.update(b"bin");
        assert_eq!(copy_web(&empty, &root.join("copy"), &mut with).unwrap(), 0);
        let mut without = Sha256::new();
        without.update(b"bin");
        assert_ne!(
            hex12(with.finalize().as_slice()),
            hex12(without.finalize().as_slice())
        );
        let _ = fs::remove_dir_all(&root);
    }
}
