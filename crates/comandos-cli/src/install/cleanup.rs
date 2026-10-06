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
    stage_admitted(home, paths, dry, &mut || Ok(()))
}

/// Recheck consumers under the cleanup lock before recording or moving files.
pub fn stage_admitted(
    home: &Path,
    paths: &[PathBuf],
    dry: bool,
    admission: &mut dyn FnMut() -> Result<(), String>,
) -> Result<Option<PathBuf>, String> {
    stage_with_journal(home, paths, dry, admission, None)
}

pub(crate) fn stage_with_journal(
    home: &Path,
    paths: &[PathBuf],
    dry: bool,
    admission: &mut dyn FnMut() -> Result<(), String>,
    mut journal: Option<&mut super::transaction::Journal>,
) -> Result<Option<PathBuf>, String> {
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
        admission()?;
        return Ok(None);
    }
    let _lock = FileLock::exclusive(&home.join(".local/share/comandos/install-cleanup.lock"))
        .map_err(|e| e.to_string())?;
    admission()?;
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let id = format!(
        "home-legacy-{}",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let backup = home.join(".local/share/comandos/backups").join(id);
    super::release::check_app_parents(&backup.join("placeholder"))?;
    let manifest = backup.join("manifest.json");
    let record =
        json!({"version":1,"home":home.to_str().ok_or("HOME must be UTF-8")?,"entries":entries});
    let bytes = format!("{record}\n");
    if let Some(j) = journal.as_deref_mut() {
        j.prepare_private_parents(&backup)?;
        j.file(&manifest)?;
        j.expect_file(&manifest, bytes.as_bytes(), 0o600)?;
        for entry in record["entries"]
            .as_array()
            .ok_or("invalid cleanup entries")?
        {
            let path = entry["path"].as_str().ok_or("missing cleanup path")?;
            j.prepare_archive(&home.join(path), &backup.join("home-legacy").join(path))?;
        }
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&backup)
        .map_err(|e| e.to_string())?;
    write_atomic(&manifest, bytes.as_bytes()).map_err(|e| e.to_string())?;
    if let Some(j) = journal {
        j.checkpoint(std::slice::from_ref(&manifest))?;
    }
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
    if !manifest.symlink_metadata().is_ok_and(|m| m.is_file()) {
        return Err("cleanup manifest must be a regular file".into());
    }
    let _lock = if dry {
        None
    } else {
        Some(
            FileLock::exclusive(&home.join(".local/share/comandos/install-cleanup.lock"))
                .map_err(|e| e.to_string())?,
        )
    };
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

#[cfg(test)]
mod transaction_tests {
    use super::*;
    use crate::install::transaction::{Journal, finish};
    use std::os::unix::fs::symlink;
    fn home() -> PathBuf {
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).unwrap();
        let path =
            std::env::temp_dir().join(format!("cleanup-journal-{:x}", u64::from_ne_bytes(id)));
        fs::create_dir(&path).unwrap();
        path
    }
    #[test]
    fn late_failure_restores_selected_links_and_removes_new_archive_tree() {
        let home = home();
        let source = home.join(".local/bin/cc-telegram");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        symlink("/private/legacy/bin/cc-telegram", &source).unwrap();
        let credentials = home.join("telegram-credentials");
        fs::write(&credentials, b"private secret").unwrap();
        let mut journal = Journal::default();
        journal.durable(&home).unwrap();
        let manifest = stage_with_journal(
            &home,
            &[PathBuf::from(".local/bin/cc-telegram")],
            false,
            &mut || Ok(()),
            Some(&mut journal),
        )
        .unwrap()
        .unwrap();
        assert!(!source.is_symlink());
        assert!(manifest.is_file());
        let error = finish::<()>(journal, Err("injected late failure".into())).unwrap_err();
        assert!(!error.contains("rollback failed"), "{error}");
        assert_eq!(
            fs::read_link(&source).unwrap(),
            Path::new("/private/legacy/bin/cc-telegram")
        );
        assert!(!manifest.parent().unwrap().exists());
        assert_eq!(fs::read(&credentials).unwrap(), b"private secret");
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn edited_archive_is_retained_with_durable_conflict() {
        let home = home();
        let source = home.join("owned-link");
        symlink("old-target", &source).unwrap();
        let mut journal = Journal::default();
        journal.durable(&home).unwrap();
        let manifest = stage_with_journal(
            &home,
            &[PathBuf::from("owned-link")],
            false,
            &mut || Ok(()),
            Some(&mut journal),
        )
        .unwrap()
        .unwrap();
        let archived = manifest.parent().unwrap().join("home-legacy/owned-link");
        fs::remove_file(&archived).unwrap();
        symlink("user-new-target", &archived).unwrap();
        let error = finish::<()>(journal, Err("late failure".into())).unwrap_err();
        assert!(error.contains(&archived.display().to_string()), "{error}");
        assert!(error.contains("retained recovery journal"), "{error}");
        assert_eq!(
            fs::read_link(archived).unwrap(),
            Path::new("user-new-target")
        );
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn retargeted_source_fails_before_any_archive_mutation() {
        let home = home();
        let source = home.join("owned-link");
        symlink("old-target", &source).unwrap();
        let mut journal = Journal::default();
        journal.capture(&source).unwrap();
        journal.durable(&home).unwrap();
        fs::remove_file(&source).unwrap();
        symlink("user-target", &source).unwrap();
        let error = stage_with_journal(
            &home,
            &[PathBuf::from("owned-link")],
            false,
            &mut || Ok(()),
            Some(&mut journal),
        )
        .unwrap_err();
        assert!(error.contains("archive source changed"), "{error}");
        let error = finish::<()>(journal, Err(error)).unwrap_err();
        assert!(error.contains("retained recovery journal"));
        assert_eq!(fs::read_link(&source).unwrap(), Path::new("user-target"));
        assert!(!home.join(".local/share/comandos/backups").exists());
        fs::remove_dir_all(home).unwrap();
    }
}
