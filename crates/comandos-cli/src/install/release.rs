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

const APP_ARTIFACT: &str = "comandos-app";
const APP_VERSION: u64 = 1;

/// App shares the release layout, but has a distinct hash domain and artifact.
/// Preparing this pointer never changes the live `cc-app` link.
struct AppCandidate {
    bytes: Vec<u8>,
    id: String,
    sha256: String,
    dir: PathBuf,
    pointer: PathBuf,
}

/// Reject symlinked ancestors before reading or writing an App installation.
/// Missing parents are allowed, including during a read-only preview.
pub(crate) fn check_app_parents(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "se necesita una ruta absoluta sin '..': {}",
            path.display()
        ));
    }
    let parent = path.parent().ok_or("ruta sin directorio")?;
    for dir in parent.ancestors() {
        match dir.symlink_metadata() {
            Ok(m) if m.is_dir() => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Ok(_) => return Err(format!("{} no es un directorio regular", dir.display())),
            Err(e) => return Err(format!("no se pudo inspeccionar {}: {e}", dir.display())),
        }
    }
    Ok(())
}

fn app_hash(bytes: &[u8]) -> (String, String) {
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    let mut h = Sha256::new();
    h.update(b"comandos-app\0");
    h.update(bytes);
    (hex12(h.finalize().as_slice()), sha256)
}

fn app_binary(path: &Path) -> Result<Vec<u8>, String> {
    check_app_parents(path)?;
    let meta = path
        .symlink_metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
        return Err(format!(
            "{} no es un archivo ejecutable regular",
            path.display()
        ));
    }
    let bytes = read_same(path, (meta.dev(), meta.ino()))?;
    if bytes.is_empty() {
        return Err(format!("{} está vacío", path.display()));
    }
    Ok(bytes)
}

fn app_manifest(bytes: &[u8]) -> Result<(), String> {
    let v: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("manifiesto App inválido: {e}"))?;
    let compatible = v.as_object().is_some_and(|o| o.len() == 4)
        && v.get("state_protocol").and_then(|p| p.as_u64())
            == Some(u64::from(super::STATE_PROTOCOL))
        && v.get("artifact_version").and_then(|p| p.as_u64()) == Some(APP_VERSION)
        && v.get("artifact").and_then(|p| p.as_str()) == Some(APP_ARTIFACT)
        && v.get("sha256").and_then(|p| p.as_str()).is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        });
    if !compatible {
        return Err("manifiesto App incompatible o desconocido".into());
    }
    Ok(())
}

fn verify_app(dir: &Path, id: &str) -> Result<PathBuf, String> {
    check_app_parents(&dir.join(APP_ARTIFACT))?;
    if !is_sha12(id) {
        return Err("id de release App inválido".into());
    }
    let binary = dir.join(APP_ARTIFACT);
    let bytes = app_binary(&binary)?;
    let manifest = dir.join("manifest.json");
    if !manifest.symlink_metadata().is_ok_and(|m| m.is_file()) {
        return Err(format!(
            "{} no es un manifiesto regular",
            manifest.display()
        ));
    }
    let raw = fs::read(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    app_manifest(&raw)?;
    let v: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let (got, sha256) = app_hash(&bytes);
    if got != id || v.get("sha256").and_then(|p| p.as_str()) != Some(sha256.as_str()) {
        return Err(format!(
            "{} no coincide con el hash de la release App {id}",
            dir.display()
        ));
    }
    Ok(binary)
}

fn prepare_app(home: &Path, exe: &Path) -> Result<AppCandidate, String> {
    let bytes = app_binary(exe)?;
    let (id, sha256) = app_hash(&bytes);
    // Reinstallation from a release must prove its own manifest and id too.
    if exe.file_name().is_some_and(|n| n == APP_ARTIFACT)
        && let Some(parent) = exe.parent()
        && parent
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n == "releases")
    {
        let own_id = parent
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("id App inválido")?;
        verify_app(parent, own_id)?;
    }
    let share = home.join(".local/share/comandos");
    let dir = share.join("releases").join(&id);
    let pointer = share.join("bin/comandos-app");
    check_app_parents(&dir.join(APP_ARTIFACT))?;
    check_app_parents(&pointer)?;
    if dir.symlink_metadata().is_ok() {
        verify_app(&dir, &id)?;
    }
    match pointer.symlink_metadata() {
        Ok(m) if m.file_type().is_symlink() => {
            app_release(home)?;
        }
        Ok(_) => {
            return Err(format!(
                "{} no es un enlace de release App",
                pointer.display()
            ));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", pointer.display())),
    }
    Ok(AppCandidate {
        bytes,
        id,
        sha256,
        dir,
        pointer,
    })
}

/// Fully validated immutable App artifact; an arbitrary pointer is never trusted.
pub fn app_release(home: &Path) -> Result<PathBuf, String> {
    let share = home.join(".local/share/comandos");
    let pointer = share.join("bin/comandos-app");
    check_app_parents(&pointer)?;
    let target = fs::read_link(&pointer).map_err(|e| {
        format!(
            "primero: comandos install --stage-app RUTA ({}: {e})",
            pointer.display()
        )
    })?;
    let id = target
        .parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        .ok_or("enlace App inválido")?;
    if !is_sha12(id) || target != Path::new("../releases").join(id).join(APP_ARTIFACT) {
        return Err("el candidato no apunta exclusivamente a una release App".into());
    }
    verify_app(&share.join("releases").join(id), id)
}

/// Recognize an installed App path without accepting arbitrary files as App releases.
pub(crate) fn is_app_artifact(home: &Path, path: &Path) -> bool {
    let releases = home.join(".local/share/comandos/releases");
    path.parent().is_some_and(|dir| {
        dir.parent() == Some(releases.as_path())
            && path.file_name().is_some_and(|n| n == APP_ARTIFACT)
            && dir
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_sha12)
    })
}

pub fn preview_app(home: &Path, exe: &Path) -> Result<Release, String> {
    let c = prepare_app(home, exe)?;
    Ok(Release {
        id: c.id,
        path: c.dir.join(APP_ARTIFACT),
        current: false,
        web_files: None,
    })
}

pub fn stage_app(home: &Path, exe: &Path) -> Result<Release, String> {
    let _guard = super::transaction::installation_lock(home)?;
    stage_app_without_install_lock(home, exe)
}
pub(crate) fn stage_app_without_install_lock(home: &Path, exe: &Path) -> Result<Release, String> {
    let c = prepare_app(home, exe)?;
    let bin_dir = c.pointer.parent().ok_or("ruta App sin directorio")?;
    fs::create_dir_all(bin_dir).map_err(|e| format!("{}: {e}", bin_dir.display()))?;
    let _lock = lock_release_dir(&c.dir)?;
    let target = c.dir.join(APP_ARTIFACT);
    if target.symlink_metadata().is_err() {
        let manifest = serde_json::json!({"state_protocol": super::STATE_PROTOCOL, "artifact_version": APP_VERSION, "artifact": APP_ARTIFACT, "sha256": c.sha256});
        comandos_store::files::write_atomic(
            &c.dir.join("manifest.json"),
            format!("{manifest}\n").as_bytes(),
        )
        .map_err(|e| e.to_string())?;
        fs::set_permissions(
            c.dir.join("manifest.json"),
            fs::Permissions::from_mode(0o644),
        )
        .map_err(|e| e.to_string())?;
        let tmp = c
            .dir
            .join(format!("comandos-app.tmp.{}", std::process::id()));
        use std::io::Write;
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
                .map_err(|e| e.to_string())?;
            file.write_all(&c.bytes)
                .and_then(|()| file.set_permissions(fs::Permissions::from_mode(0o755)))
                .and_then(|()| file.sync_all())
                .map_err(|e| e.to_string())?;
            fs::rename(&tmp, &target).map_err(|e| e.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result?;
    }
    verify_app(&c.dir, &c.id)?;
    fs::File::open(&c.dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    let tmp = c
        .pointer
        .with_extension(format!("tmp-link.{}", std::process::id()));
    symlink(
        Path::new("../releases").join(&c.id).join(APP_ARTIFACT),
        &tmp,
    )
    .map_err(|e| format!("{}: {e}", tmp.display()))?;
    if let Err(e) = fs::rename(&tmp, &c.pointer) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("{}: {e}", c.pointer.display()));
    }
    fs::File::open(bin_dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(Release {
        id: c.id,
        path: target,
        current: true,
        web_files: None,
    })
}

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
    let _guard = super::transaction::installation_lock(home)?;
    stage_release_without_install_lock(home, exe, web)
}
pub(crate) fn stage_release_without_install_lock(
    home: &Path,
    exe: &Path,
    web: &WebSource,
) -> Result<Release, String> {
    stage_release_with_pruning(home, exe, web, true)
}

/// Full-install staging retains every previous release until downstream success.
pub(crate) fn stage_release_unpruned(
    home: &Path,
    exe: &Path,
    web: &WebSource,
) -> Result<Release, String> {
    stage_release_with_pruning(home, exe, web, false)
}
fn stage_release_with_pruning(
    home: &Path,
    exe: &Path,
    web: &WebSource,
    pruning: bool,
) -> Result<Release, String> {
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
    let dir = releases.join(&id);
    // Todo lo que puede rechazar el `web/` ocurre antes de tocar `releases/<id>`.
    let checked = match staged {
        None => Ok(None),
        Some(Err(e)) => Err(e),
        Some(Ok(_)) if expected.is_some_and(|x| x != id) => Err(format!(
            "la release {} no coincide con su contenido (id recalculado {id}): no se re-instala",
            expected.unwrap_or_default()
        )),
        Some(Ok(n)) => Ok(Some(n)),
    };
    let web_files = match checked {
        Ok(n) => n,
        Err(e) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    let has_web = web_files.is_some();
    let existed = dir.symlink_metadata().is_ok();
    // Candado del id mientras se completa y se enlaza: `prune` de otro `--stage` no
    // borra un `<id>/` con candado. Se suelta al salir de la función.
    let lock = lock_release_dir(&dir);
    let completed = match &lock {
        Ok(_) => complete_release(&dir, &id, &binary, has_web.then(|| staging.join("web"))),
        Err(e) => Err(e.clone()),
    };
    if src.is_some() {
        let _ = fs::remove_dir_all(&staging);
    }
    if let Err(e) = completed {
        // Lo que creó esta ejecución y quedó a medias se borra con el candado aún
        // tomado, para no pisar a otro `--stage` que espere este id.
        if !existed && dir.join("comandos").symlink_metadata().is_err() {
            let _ = fs::remove_dir_all(&dir);
        }
        drop(lock);
        return Err(e);
    }
    // 1) La release está completa (`complete_release`); el candado sigue hasta enlazar.
    let _lock = lock;
    let target = dir.join("comandos");
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
    // 3) Justo antes de enlazar: la release sigue entera en disco (binario y, si hay,
    //    `web/` con su manifiesto) y su contenido da su id.
    verify_release(&dir, &id, has_web)?;
    //    El swap del symlink temporal sobre `bin` es atómico.
    if previous.as_deref() != Some(id.as_str())
        && let Some(p) = &previous
    {
        write_previous(&releases, p)?;
    }
    if current_id(&releases, &bin).as_deref() != Some(id.as_str()) {
        swap_link(&bin, &id)?;
    }
    if pruning {
        prune(&releases, &id)?;
    }
    Ok(Release {
        id,
        path: target,
        current: true,
        web_files,
    })
}

pub(crate) fn prune_after_install(home: &Path, id: &str) -> Result<(), String> {
    prune(&layout(home).releases, id)
}

/// Read-only counterpart of CLI staging: hash and validate the source in place.
pub fn preview_release(home: &Path, exe: &Path, web: &WebSource) -> Result<Release, String> {
    let Layout { releases, .. } = layout(home);
    let binary = fs::read(exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    let (source, expected) = match web {
        WebSource::None => (None, None),
        WebSource::Explicit(p) => (Some(p.as_path()), None),
        WebSource::OwnRelease { dir, id } => (Some(dir.as_path()), Some(id.as_str())),
    };
    let id = release_hash(&binary, source)?;
    let web_files = if let Some(source) = source {
        let path = source.join(MANIFEST_FILE);
        let meta = path
            .symlink_metadata()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !meta.is_file() {
            return Err(format!("{} no es un manifiesto regular", path.display()));
        }
        let raw = read_same(&path, (meta.dev(), meta.ino()))?;
        let manifest: Manifest =
            serde_json::from_slice(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
        manifest.check_paths().map_err(|e| e.to_string())?;
        for rel in manifest.files.values() {
            if !source
                .join(rel)
                .symlink_metadata()
                .is_ok_and(|m| m.is_file())
            {
                return Err(format!("{}: falta {rel}", path.display()));
            }
        }
        Some(web_tree(source)?.into_iter().filter(|e| !e.dir).count())
    } else {
        None
    };
    if expected.is_some_and(|expected| expected != id) {
        return Err("la release propia no coincide con su contenido".into());
    }
    let dir = releases.join(&id);
    if dir.join("comandos").symlink_metadata().is_ok() {
        if super::manifest::release_protocol(&dir) != super::STATE_PROTOCOL {
            return Err("release CLI sin protocolo compatible".into());
        }
        verify_release(&dir, &id, source.is_some())?;
    }
    Ok(Release {
        path: dir.join("comandos"),
        id,
        current: false,
        web_files,
    })
}

pub fn preview_rollback_release(home: &Path) -> Result<Release, String> {
    let Layout { releases, bin } = layout(home);
    let prev = fs::read_to_string(releases.join("previous"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| valid_id(s) && releases.join(s).join("comandos").is_file())
        .ok_or("no hay release anterior a la que volver")?;
    current_id(&releases, &bin).ok_or("bin/comandos no es un enlace a una release")?;
    Ok(Release {
        path: releases.join(&prev).join("comandos"),
        id: prev,
        current: false,
        web_files: None,
    })
}

pub fn rollback_release(home: &Path) -> Result<Release, String> {
    let _guard = super::transaction::installation_lock(home)?;
    rollback_release_without_install_lock(home)
}
pub(crate) fn rollback_release_without_install_lock(home: &Path) -> Result<Release, String> {
    let Layout { releases, bin } = layout(home);
    let prev = fs::read_to_string(releases.join("previous"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| valid_id(s) && releases.join(s).join("comandos").is_file())
        .ok_or("no hay release anterior a la que volver")?;
    let current =
        current_id(&releases, &bin).ok_or("bin/comandos no es un enlace a una release")?;
    super::guard::rollback_state(home, &releases.join(&prev))?;
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

/// Completa `<id>/` con el candado tomado: coloca el `web/` preparado (si lo hay),
/// comprueba un `web/` reutilizado contra los bytes del binario y escribe
/// `comandos` si falta (bytes del hash, 0755, `rename`: completa o ausente).
fn complete_release(
    dir: &Path,
    id: &str,
    binary: &[u8],
    staged_web: Option<PathBuf>,
) -> Result<(), String> {
    if let Some(staged) = &staged_web {
        place_web(dir, staged)?;
        // Un `web/` reutilizado de un `<id>/` a medias pudo alterarse.
        let got = release_hash(binary, Some(&dir.join("web")))?;
        if got != id {
            return Err(format!(
                "{} no coincide con su id {id} (recalculado {got}): no se completa ni se enlaza",
                dir.display()
            ));
        }
    }
    let target = dir.join("comandos");
    if target.is_file() {
        if super::manifest::release_protocol(dir) != super::manifest::STATE_PROTOCOL {
            return Err(format!(
                "{}: release existente sin protocolo compatible; no se modifica",
                dir.display()
            ));
        }
    } else {
        super::manifest::write_manifest(dir)?;
        let tmp = dir.join(format!("comandos.tmp.{}", std::process::id()));
        fs::write(&tmp, binary)
            .and_then(|()| fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755)))
            .map_err(|e| {
                let _ = fs::remove_file(&tmp);
                format!("no se pudo copiar a {}: {e}", tmp.display())
            })?;
        fs::rename(&tmp, &target)
            .map_err(|e| format!("no se pudo instalar {}: {e}", target.display()))?;
    }
    Ok(())
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
    walk_web(src, Some(dest), hasher)
}

/// Recorre `src` alimentando `hasher` con el marco de `copy_web`; con `dest`, copia.
fn walk_web(src: &Path, dest: Option<&Path>, hasher: &mut Sha256) -> Result<usize, String> {
    let entries = web_tree(src)?;
    if let Some(dest) = dest {
        make_dir(dest)?;
    }
    // Marcador de raíz: un `web/` vacío no da el mismo id que «sin web».
    hasher.update(b"web\0");
    let mut files = 0;
    for e in entries {
        let to = dest.map(|d| d.join(&e.rel));
        let rel = e.rel.as_os_str().as_bytes();
        if e.dir {
            hasher.update(b"d");
            hasher.update(rel);
            hasher.update([0]);
            if let Some(to) = &to {
                make_dir(to)?;
            }
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
        let Some(to) = to else {
            continue;
        };
        fs::write(&to, &bytes).map_err(|e| format!("no se pudo escribir {}: {e}", to.display()))?;
        fs::set_permissions(&to, fs::Permissions::from_mode(0o644))
            .map_err(|e| format!("no se pudo ajustar {}: {e}", to.display()))?;
    }
    Ok(files)
}

/// Id de una release: `sha12(binario)` sin web, `sha12(binario ‖ "web\0" ‖ árbol)`
/// con web (el mismo marco que `copy_web`).
fn release_hash(binary: &[u8], web: Option<&Path>) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(binary);
    if let Some(web) = web {
        walk_web(web, None, &mut hasher)?;
    }
    Ok(hex12(hasher.finalize().as_slice()))
}

/// La release `dir` está entera: `comandos` es un archivo regular, con web su
/// `manifest.json` también, y el contenido recalculado da `id`.
fn verify_release(dir: &Path, id: &str, has_web: bool) -> Result<(), String> {
    let bin = dir.join("comandos");
    if !bin.symlink_metadata().is_ok_and(|m| m.is_file()) {
        return Err(format!(
            "{} falta: la release {id} no se enlaza",
            bin.display()
        ));
    }
    let web = dir.join("web");
    if has_web {
        let manifest = web.join(MANIFEST_FILE);
        if !manifest.symlink_metadata().is_ok_and(|m| m.is_file()) {
            return Err(format!(
                "{} falta: la release {id} no se enlaza",
                manifest.display()
            ));
        }
    }
    let binary = fs::read(&bin).map_err(|e| format!("no se pudo leer {}: {e}", bin.display()))?;
    let got = release_hash(&binary, has_web.then_some(web.as_path()))?;
    if got != id {
        return Err(format!(
            "{} no coincide con su id {id} (recalculado {got}): no se enlaza",
            dir.display()
        ));
    }
    Ok(())
}

/// Crea `dir` y toma su candado exclusivo (`flock`), esperando si otro `--stage` lo
/// tiene. Si `prune` borró el directorio entre crearlo y bloquearlo, se reintenta.
fn lock_release_dir(dir: &Path) -> Result<fs::File, String> {
    let err = |e: io::Error| format!("no se pudo bloquear {}: {e}", dir.display());
    for _ in 0..10 {
        fs::create_dir_all(dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
        let file = fs::File::open(dir).map_err(err)?;
        file.lock().map_err(err)?;
        let held = file.metadata().map_err(err)?;
        let same = dir
            .symlink_metadata()
            .is_ok_and(|m| (m.dev(), m.ino()) == (held.dev(), held.ino()));
        if same {
            return Ok(file);
        }
    }
    Err(format!("{} desaparece mientras se bloquea", dir.display()))
}

/// Borra `dir` solo si nadie tiene su candado (el que tenga un `--stage` en curso).
fn remove_unlocked(dir: &Path) -> io::Result<bool> {
    let file = fs::File::open(dir)?;
    match file.try_lock() {
        Ok(()) => {
            fs::remove_dir_all(dir)?;
            Ok(true)
        }
        Err(fs::TryLockError::WouldBlock) => Ok(false),
        Err(fs::TryLockError::Error(e)) => Err(e),
    }
}

/// Lee `path` solo si al abrirlo sigue siendo el archivo regular `inode` que vio el
/// recorrido: un cambio por un enlace simbólico entre recorrer y leer es un error.
fn read_same(path: &Path, inode: (u64, u64)) -> Result<Vec<u8>, String> {
    let err = |e: io::Error| format!("no se pudo leer {}: {e}", path.display());
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(err)?;
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
        remove_unlocked(&dir).map_err(|e| format!("no se pudo borrar {}: {e}", dir.display()))?;
    }
    prune_abandoned(releases, current, SystemTime::now());
    Ok(())
}

/// Borra preparaciones `.web-stage.*` y directorios `<sha12>/` sin `comandos` más
/// viejos que `ABANDONED` (restos de un `--stage` cortado). Los recientes pueden ser
/// de otro `--stage` en curso y se respetan, igual que el id que se prepara ahora
/// (`current`) y cualquier `<id>/` con candado. Mejor esfuerzo: un fallo no aborta.
fn prune_abandoned(releases: &Path, current: &str, now: SystemTime) {
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
            || (is_sha12(&name)
                && path.join("comandos").symlink_metadata().is_err()
                && verify_app(&path, &name).is_err());
        let old = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > ABANDONED);
        if !meta.is_dir() || !leftover || !old || name == current {
            continue;
        }
        if name.starts_with(".web-stage.") {
            let _ = fs::remove_dir_all(&path);
        } else {
            let _ = remove_unlocked(&path);
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

#[cfg(test)]
mod round2 {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cmd-rel-r2-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn age(p: &Path) {
        let old = SystemTime::now() - Duration::from_secs(3 * 3600);
        fs::File::open(p).unwrap().set_modified(old).unwrap();
    }

    #[test]
    fn prune_never_removes_a_locked_or_current_half_release() {
        let rel = scratch("lock");
        for id in ["0123456789ab", "ba9876543210", "cccccccccccc"] {
            fs::create_dir_all(rel.join(id).join("web")).unwrap();
            age(&rel.join(id));
        }
        // Otro `--stage` tiene el candado de este id.
        let held = lock_release_dir(&rel.join("0123456789ab")).unwrap();
        age(&rel.join("0123456789ab"));
        prune_abandoned(&rel, "cccccccccccc", SystemTime::now());
        assert!(rel.join("0123456789ab").exists(), "con candado: se respeta");
        assert!(
            rel.join("cccccccccccc").exists(),
            "el id en curso: se respeta"
        );
        assert!(!rel.join("ba9876543210").exists(), "abandonado: se borra");
        drop(held);
        prune_abandoned(&rel, "cccccccccccc", SystemTime::now());
        assert!(!rel.join("0123456789ab").exists(), "sin candado: se borra");
        let _ = fs::remove_dir_all(&rel);
    }

    #[test]
    fn verify_release_recomputes_the_id_from_disk() {
        let rel = scratch("verify");
        let web = rel.join("src");
        fs::create_dir_all(web.join("abc")).unwrap();
        fs::write(web.join("abc/boot.js"), "// boot\n").unwrap();
        fs::write(
            web.join("manifest.json"),
            r#"{"files":{"x_boot.js":"abc/boot.js"}}"#,
        )
        .unwrap();
        let mut h = Sha256::new();
        h.update(b"bin");
        let dir = rel.join("release");
        fs::create_dir_all(&dir).unwrap();
        stage_web(&web, &dir.join("web"), &mut h).unwrap();
        let id = hex12(h.finalize().as_slice());
        fs::write(dir.join("comandos"), b"bin").unwrap();
        verify_release(&dir, &id, true).unwrap();
        // Binario alterado.
        fs::write(dir.join("comandos"), b"BIN").unwrap();
        assert!(verify_release(&dir, &id, true).unwrap_err().contains(&id));
        fs::write(dir.join("comandos"), b"bin").unwrap();
        // Web alterado.
        fs::write(dir.join("web/abc/boot.js"), "// otro\n").unwrap();
        assert!(verify_release(&dir, &id, true).is_err());
        // Manifiesto ausente.
        fs::write(dir.join("web/abc/boot.js"), "// boot\n").unwrap();
        fs::remove_file(dir.join("web/manifest.json")).unwrap();
        let err = verify_release(&dir, &id, true).unwrap_err();
        assert!(err.contains("manifest.json"), "{err}");
        // Sin web: solo el binario.
        let plain = hex12(Sha256::digest(b"bin").as_slice());
        verify_release(&dir, &plain, false).unwrap();
        fs::remove_file(dir.join("comandos")).unwrap();
        assert!(verify_release(&dir, &plain, false).is_err());
        let _ = fs::remove_dir_all(&rel);
    }
}
