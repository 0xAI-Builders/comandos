//! Reversible HOME cleanup. Selection/admission belongs to the retirement checker.
use comandos_store::files::{FileLock, write_atomic};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
};

fn relative(path: &Path) -> Result<(), String> {
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("cleanup paths must be relative normal components".into());
    }
    Ok(())
}

fn hash(path: &Path) -> Result<String, String> {
    fn visit(path: &Path, root: &Path, digest: &mut Sha256) -> io::Result<()> {
        let meta = path.symlink_metadata()?;
        digest.update(
            path.strip_prefix(root)
                .map_err(io::Error::other)?
                .as_os_str()
                .as_bytes(),
        );
        digest.update((meta.permissions().mode() & 0o7777).to_be_bytes());
        if meta.file_type().is_symlink() {
            digest.update(b"link\0");
            digest.update(fs::read_link(path)?.as_os_str().as_bytes());
        } else if meta.is_file() {
            digest.update(b"file\0");
            digest.update(meta.len().to_be_bytes());
            let mut file = fs::File::open(path)?;
            let mut buffer = [0u8; 65536];
            loop {
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                digest.update(
                    buffer
                        .get(..n)
                        .ok_or_else(|| io::Error::other("invalid read length"))?,
                );
            }
        } else if meta.is_dir() {
            digest.update(b"directory\0");
            let mut entries = fs::read_dir(path)?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<io::Result<Vec<_>>>()?;
            entries.sort();
            for entry in entries {
                visit(&entry, root, digest)?;
            }
        } else {
            return Err(io::Error::other("unsupported cleanup file type"));
        }
        Ok(())
    }
    let mut digest = Sha256::new();
    visit(path, path, &mut digest).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!("{:x}", digest.finalize()))
}

pub fn stage(home: &Path, paths: &[PathBuf], dry: bool) -> Result<Option<PathBuf>, String> {
    super::release::check_app_parents(&home.join("placeholder"))?;
    let mut entries = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        if paths
            .iter()
            .skip(i + 1)
            .any(|other| other.starts_with(path) || path.starts_with(other))
        {
            return Err("overlapping cleanup paths".into());
        }
    }
    for path in paths {
        relative(path)?;
        let source = home.join(path);
        super::release::check_app_parents(&source)?;
        if source.symlink_metadata().is_ok() {
            entries.push(json!({"path":path.to_str().ok_or("cleanup path must be UTF-8")?,"sha256":hash(&source)?}));
        }
    }
    if dry || entries.is_empty() {
        return Ok(None);
    }
    let _lock = FileLock::exclusive(&home.join(".local/share/comandos/install-cleanup.lock"))
        .map_err(|e| e.to_string())?;
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let id = format!(
        "home-legacy-{}",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let backup = home.join(".local/share/comandos/backups").join(id);
    super::release::check_app_parents(&backup.join("placeholder"))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&backup)
        .map_err(|e| e.to_string())?;
    let manifest = backup.join("manifest.json");
    let record =
        json!({"version":1,"home":home.to_str().ok_or("HOME must be UTF-8")?,"entries":entries});
    write_atomic(&manifest, format!("{record}\n").as_bytes()).map_err(|e| e.to_string())?;
    for entry in record["entries"]
        .as_array()
        .ok_or("invalid cleanup entries")?
    {
        let path = entry["path"].as_str().ok_or("missing cleanup path")?;
        let source = home.join(path);
        let target = backup.join("home-legacy").join(path);
        if hash(&source)? != entry["sha256"].as_str().ok_or("missing cleanup hash")? {
            return Err(format!(
                "{} changed; recover with {}",
                source.display(),
                manifest.display()
            ));
        }
        if let Some(parent) = target.parent() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .map_err(|e| e.to_string())?;
        }
        fs::rename(&source, &target).map_err(|e| {
            format!(
                "{}: {e}; recover with {}",
                source.display(),
                manifest.display()
            )
        })?;
    }
    Ok(Some(manifest))
}

pub fn restore(home: &Path, manifest: &Path, dry: bool) -> Result<Vec<String>, String> {
    super::release::check_app_parents(manifest)?;
    let backup = manifest.parent().ok_or("manifest without parent")?;
    if !backup.starts_with(home.join(".local/share/comandos/backups")) {
        return Err("manifest outside installation backups".into());
    }
    let record: Value = serde_json::from_slice(&fs::read(manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if record["version"] != 1 || record["home"].as_str() != home.to_str() {
        return Err("cleanup manifest does not match HOME/version".into());
    }
    let mut moves = Vec::new();
    for entry in record["entries"]
        .as_array()
        .ok_or("missing cleanup entries")?
    {
        let path = Path::new(entry["path"].as_str().ok_or("missing cleanup path")?);
        relative(path)?;
        let target = home.join(path);
        let source = backup.join("home-legacy").join(path);
        super::release::check_app_parents(&target)?;
        super::release::check_app_parents(&source)?;
        if source.symlink_metadata().is_err() {
            if target.symlink_metadata().is_ok()
                && hash(&target)? == entry["sha256"].as_str().ok_or("missing cleanup hash")?
            {
                continue;
            }
            return Err(format!("missing cleanup backup {}", source.display()));
        }
        if target.symlink_metadata().is_ok() {
            return Err(format!("restore would overwrite {}", target.display()));
        }
        if hash(&source)? != entry["sha256"].as_str().ok_or("missing cleanup hash")? {
            return Err(format!("backup changed: {}", source.display()));
        }
        moves.push((source, target));
    }
    let report = moves
        .iter()
        .map(|(_, target)| {
            format!(
                "{}restore {}",
                if dry { "dry-run: " } else { "" },
                target.display()
            )
        })
        .collect();
    if !dry {
        let _lock = FileLock::exclusive(&home.join(".local/share/comandos/install-cleanup.lock"))
            .map_err(|e| e.to_string())?;
        for (source, target) in moves {
            if target.symlink_metadata().is_ok() {
                return Err(format!("restore would overwrite {}", target.display()));
            }
            if let Some(parent) = target.parent() {
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(parent)
                    .map_err(|e| e.to_string())?;
            }
            fs::rename(source, target).map_err(|e| e.to_string())?;
        }
    }
    Ok(report)
}
