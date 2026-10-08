//! Autonomous Rust launcher, validated recovery metadata and first-original backups.
use super::{Result, policy, process};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs,
    io::{ErrorKind, Read},
    os::unix::{
        fs::{PermissionsExt, symlink},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
const START: &str = "# BEGIN COMANDOS CODEX YOLO";
const END: &str = "# END COMANDOS CODEX YOLO";
const TYPE: &str = "comandos-codex-yolo";
fn abs(p: &Path) -> Result<PathBuf> {
    if p.is_absolute() {
        Ok(p.into())
    } else {
        std::env::current_dir()
            .map(|d| d.join(p))
            .map_err(|e| e.to_string())
    }
}
pub(super) fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME ausente".into())
        .and_then(|p| fs::canonicalize(p).map_err(|e| e.to_string()))
}
pub(super) fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p).map(|d| d.join(name)).find(|p| {
            p.is_file() && fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
        })
    })
}
fn regular(path: &Path, max: u64) -> Result<Vec<u8>> {
    super::runtime::read_regular(path, max)
}
fn hash(path: &Path) -> Result<String> {
    const LIMIT: u64 = 512 * 1024 * 1024;
    let file = super::runtime::open_regular(path)?;
    if file.metadata().map_err(|e| e.to_string())?.len() > LIMIT {
        return Err("archivo excede límite".into());
    }
    let mut reader = file.take(LIMIT + 1);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.to_string()),
        };
        total += n as u64;
        if total > LIMIT {
            return Err("archivo excede límite".into());
        }
        digest.update(buffer.get(..n).ok_or("lectura inválida")?);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn text(m: &Value, k: &str) -> Result<String> {
    m[k].as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("manifiesto inválido: {k}"))
}
fn verify(m: &Value, dir: &Path) -> Result<()> {
    if m["version"] != 2 || m["type"] != TYPE {
        return Err("manifiesto launcher desconocido".into());
    }
    let o = m.as_object().ok_or("manifiesto no es objeto")?;
    if o.keys().any(|k| {
        ![
            "version",
            "type",
            "wrapper",
            "original",
            "launcher",
            "originalLink",
            "artifact",
            "sha256",
        ]
        .contains(&k.as_str())
    }) {
        return Err("campo de manifiesto desconocido".into());
    }
    let artifact = PathBuf::from(text(m, "artifact")?);
    if artifact != dir.join("codex-yolo") || text(m, "sha256")? != hash(&artifact)? {
        return Err("hash/ruta de launcher no coincide".into());
    }
    let wrapper = PathBuf::from(text(m, "wrapper")?);
    let home = dir.ancestors().nth(4).ok_or("raíz launcher inválida")?;
    if wrapper != home.join(".local/bin/codex") {
        return Err("wrapper de manifiesto fuera de la instalación".into());
    }
    for key in ["original", "launcher"] {
        if !Path::new(&text(m, key)?).is_absolute() {
            return Err("rutas de manifiesto deben ser absolutas".into());
        }
    }
    if !(m["originalLink"].is_null() || m["originalLink"].is_string()) {
        return Err("originalLink inválido".into());
    }
    Ok(())
}
fn parents(path: &Path) -> Result<()> {
    for p in path.ancestors() {
        match fs::symlink_metadata(p) {
            Ok(m) if m.is_dir() => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => return Err(format!("{}: padre no directorio regular", p.display())),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
fn atomic(path: &Path, body: &[u8], mode: u32) -> Result<()> {
    comandos_store::files::write_atomic(path, body).map_err(|e| e.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())
}
fn link(path: &Path, target: &Path) -> Result<()> {
    let mut rnd = [0; 8];
    getrandom::fill(&mut rnd).map_err(|e| e.to_string())?;
    let temporary = path.with_file_name(format!(".codex-{:x}", u64::from_ne_bytes(rnd)));
    let result = symlink(target, &temporary).and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| e.to_string())
}
pub(super) fn install(home: &Path, executable: Option<&Path>, dry: bool) -> Result<Value> {
    let cancellation = super::cli::Cancellation::new()?;
    let home = fs::canonicalize(home).map_err(|e| e.to_string())?;
    let dir = home.join(".local/share/comandos/codex-yolo");
    let wrapper = home.join(".local/bin/codex");
    let manifest = dir.join("launcher.json");
    parents(&dir)?;
    parents(wrapper.parent().ok_or("wrapper sin padre")?)?;
    let previous = match fs::symlink_metadata(&manifest) {
        Ok(_) => serde_json::from_slice::<Value>(&regular(&manifest, 1024 * 1024)?)
            .map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e.to_string()),
    };
    let has_previous = previous.as_object().is_some_and(|m| !m.is_empty());
    if has_previous && previous["version"] != 1 {
        verify(&previous, &dir)?;
    }
    if has_previous && previous["version"] == 1 {
        if previous.as_object().is_none_or(|o| {
            o.keys().any(|k| {
                !["version", "wrapper", "original", "launcher", "originalLink"]
                    .contains(&k.as_str())
            })
        }) || Path::new(&text(&previous, "wrapper")?) != wrapper
        {
            return Err("manifiesto previo desconocido".into());
        }
        for key in ["original", "launcher"] {
            if !Path::new(&text(&previous, key)?).is_absolute() {
                return Err("manifiesto previo no absoluto".into());
            }
        }
    }
    match fs::symlink_metadata(&wrapper) {
        Ok(m) => {
            let owned = if has_previous && previous["version"] == 2 {
                m.file_type().is_symlink()
                    && fs::read_link(&wrapper).is_ok_and(|p| p == dir.join("codex-yolo"))
            } else if m.is_file() {
                regular(&wrapper, 1024 * 1024)?
                    .get(..512.min(m.len() as usize))
                    .is_some_and(|b| {
                        String::from_utf8_lossy(b).contains("# COMANDOS_CODEX_YOLO_LAUNCHER")
                    })
            } else {
                false
            };
            if !owned {
                return Err(format!(
                    "Ya existe un lanzador ajeno en {}; se conserva sin sobrescribir",
                    wrapper.display()
                ));
            }
            if !has_previous {
                return Err("lanzador owned sin manifiesto de recuperación".into());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let entry = abs(&executable
        .map(PathBuf::from)
        .or_else(|| previous["launcher"].as_str().map(PathBuf::from))
        .or_else(|| which("codex"))
        .ok_or("No se encontró el ejecutable de Codex")?)?;
    let resolved = fs::canonicalize(&entry).map_err(|e| e.to_string())?;
    let original = if resolved == dir.join("codex-yolo")
        || entry == wrapper
        || fs::canonicalize(&wrapper).is_ok_and(|p| p == resolved)
    {
        PathBuf::from(text(&previous, "original")?)
    } else {
        resolved
    };
    let m = fs::metadata(&original).map_err(|_| {
        "No se pudo identificar el lanzador original; se evita un bucle de ejecución".to_string()
    })?;
    if !m.is_file()
        || m.permissions().mode() & 0o111 == 0
        || original == dir.join("codex-yolo")
        || original == wrapper
    {
        return Err(
            "No se pudo identificar el lanzador original; se evita un bucle de ejecución".into(),
        );
    }
    let help = process::run_when(
        &original,
        &[OsStr::new("--help")],
        None,
        Duration::from_secs(15),
        &cancellation.flag,
    )?;
    if !help.status.success() {
        return Err(format!(
            "Codex --help falló: {}",
            String::from_utf8_lossy(&help.stderr)
        ));
    }
    let help = String::from_utf8(help.stdout).map_err(|e| e.to_string())?;
    if !help.contains("--no-daemon") || !help.contains(policy::YOLO) {
        return Err(
            "Esta versión no admite las opciones necesarias; no se instaló la regla".into(),
        );
    }
    let block = format!(
        "{START}\nexport PATH={}:\"$PATH\"\n{END}",
        policy::quote(
            wrapper
                .parent()
                .ok_or("wrapper sin padre")?
                .to_str()
                .ok_or("PATH no UTF-8")?
        )
    );
    let mut shells = vec![];
    for name in [".zshrc", ".bashrc"] {
        let path = home.join(name);
        let target = match fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => {
                fs::canonicalize(&path).map_err(|e| e.to_string())?
            }
            _ => path.clone(),
        };
        parents(target.parent().ok_or("shell sin padre")?)?;
        let (raw, mode) = match fs::symlink_metadata(&target) {
            Ok(m) => (
                regular(&target, 4 * 1024 * 1024)?,
                m.permissions().mode() & 0o777,
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (vec![], 0o644),
            Err(e) => return Err(e.to_string()),
        };
        let text = String::from_utf8(raw.clone()).map_err(|e| e.to_string())?;
        let a = text.matches(START).count();
        let b = text.matches(END).count();
        if a != b || a > 1 || a == 1 && text.find(START) > text.find(END) {
            return Err(format!(
                "Bloque de PATH inválido en {}; se conserva sin cambiar",
                path.display()
            ));
        }
        let updated = if let Some(a) = text.find(START) {
            let b = text.find(END).ok_or("bloque inválido")? + END.len();
            format!("{}{block}{}", &text[..a], &text[b..])
        } else {
            format!(
                "{}{}\n{block}\n",
                text,
                if text.is_empty() || text.ends_with('\n') {
                    ""
                } else {
                    "\n"
                }
            )
        };
        let saved = dir.join(format!("{}.before", name.trim_start_matches('.')));
        if saved.symlink_metadata().is_ok() {
            regular(&saved, 4 * 1024 * 1024)?;
        }
        shells.push((target, raw, updated, saved, mode));
    }
    let artifact = dir.join("codex-yolo");
    if artifact.symlink_metadata().is_ok() {
        regular(&artifact, 512 * 1024 * 1024)?;
    }
    let source = std::env::current_exe().map_err(|e| e.to_string())?;
    let bytes = regular(&source, 512 * 1024 * 1024)?;
    let original_link = if has_previous {
        previous["originalLink"].clone()
    } else {
        fs::read_link(&entry)
            .ok()
            .map(|p| p.to_str().map(str::to_owned).ok_or("symlink no UTF-8"))
            .transpose()?
            .map_or(Value::Null, Value::String)
    };
    let metadata = json!({"version":2,"type":TYPE,"wrapper":wrapper,"original":original,"launcher":if has_previous {PathBuf::from(text(&previous,"launcher")?)} else{entry.clone()},"originalLink":original_link,"artifact":artifact,"sha256":format!("{:x}",Sha256::digest(&bytes))});
    if dry {
        return Ok(
            json!({"dryRun":true,"metadata":metadata,"backup":dir.join("zshrc.before"),"rollback":format!("restaurar originalLink de {} y backups {} / {}",entry.display(),dir.join("zshrc.before").display(),dir.join("bashrc.before").display())}),
        );
    }
    if cancellation.flag.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("mantenimiento cancelado".into());
    }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    // Backups and recovery information precede changes to launch paths or shell config.
    for (_, raw, _, saved, _) in &shells {
        if !saved.exists() {
            atomic(saved, raw, 0o600)?;
        }
    }
    if previous["sha256"] != metadata["sha256"] {
        atomic(&artifact, &bytes, 0o755)?;
    }
    atomic(
        &manifest,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&metadata).map_err(|e| e.to_string())?
        )
        .as_bytes(),
        0o600,
    )?;
    fs::create_dir_all(wrapper.parent().ok_or("wrapper sin padre")?).map_err(|e| e.to_string())?;
    link(&wrapper, &artifact)?;
    if entry != wrapper
        && entry
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
    {
        link(&entry, &wrapper)?;
    }
    for (target, raw, updated, _, mode) in shells {
        if raw != updated.as_bytes() {
            atomic(&target, updated.as_bytes(), mode)?;
        }
    }
    let mut result = metadata;
    result["backup"] = json!(dir.join("zshrc.before"));
    Ok(result)
}
pub(super) fn cli(args: &[String]) -> Result<i32> {
    let mut root = home()?;
    let mut executable = None;
    let mut dry = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" => dry = true,
            "--home" | "--executable" => {
                let k = &args[i];
                i += 1;
                let value = args.get(i).ok_or("opción sin ruta")?;
                if k == "--home" {
                    root = PathBuf::from(value)
                } else {
                    executable = Some(PathBuf::from(value));
                }
            }
            other => return Err(format!("opción desconocida: {other}")),
        };
        i += 1;
    }
    println!(
        "{}",
        serde_json::to_string(&install(&root, executable.as_deref(), dry)?)
            .map_err(|e| e.to_string())?
    );
    Ok(0)
}
pub(super) fn launch(args: &[String]) -> Result<i32> {
    // Darwin may report the invoking symlink. Locate recovery metadata beside
    // its actual target, then retain the regular-file/hash/manifest checks.
    let artifact = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|e| e.to_string())?;
    let dir = artifact.parent().ok_or("launcher sin directorio")?;
    let m: Value = serde_json::from_slice(&regular(&dir.join("launcher.json"), 1024 * 1024)?)
        .map_err(|e| e.to_string())?;
    verify(&m, dir)?;
    let original = PathBuf::from(text(&m, "original")?);
    if original == artifact || fs::canonicalize(&original).is_ok_and(|p| p == artifact) {
        return Err("bucle de ejecución de launcher".into());
    }
    let args = policy::normalize(args)?;
    Err(Command::new(original).args(args).exec().to_string())
}
