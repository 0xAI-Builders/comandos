//! Bounded installation preimages. Only declared install destinations are captured;
//! sessions, agent histories and the rest of HOME are never traversed.
use super::plan::Action;
use comandos_store::files::write_atomic;
use std::{
    collections::BTreeMap,
    fs, io,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
};
#[derive(Debug, PartialEq, Eq)]
enum Before {
    Absent,
    File(Vec<u8>, u32),
    Link(PathBuf),
    Directory(u32),
}
#[derive(Default)]
pub(crate) struct Journal {
    entries: BTreeMap<PathBuf, Before>,
    observed: BTreeMap<PathBuf, Before>,
}
impl Journal {
    pub(crate) fn capture(&mut self, path: &Path) -> Result<(), String> {
        if self.entries.contains_key(path) {
            return Ok(());
        }
        super::release::check_app_parents(path)?;
        let before = match path.symlink_metadata() {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Before::Absent,
            Err(e) => return Err(format!("{}: {e}", path.display())),
            Ok(m) if m.file_type().is_symlink() => {
                Before::Link(fs::read_link(path).map_err(|e| e.to_string())?)
            }
            Ok(m) if m.is_file() => {
                let mut file = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(nix::libc::O_NOFOLLOW)
                    .open(path)
                    .map_err(|e| e.to_string())?;
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
                let identity = |meta: &fs::Metadata| {
                    (
                        meta.dev(),
                        meta.ino(),
                        meta.len(),
                        meta.mtime(),
                        meta.mtime_nsec(),
                        meta.ctime(),
                        meta.ctime_nsec(),
                    )
                };
                if identity(&m) != identity(&file.metadata().map_err(|e| e.to_string())?)
                    || identity(&m)
                        != identity(&path.symlink_metadata().map_err(|e| e.to_string())?)
                {
                    return Err(format!(
                        "{} changed during install preimage admission",
                        path.display()
                    ));
                }
                Before::File(bytes, m.permissions().mode() & 0o7777)
            }
            Ok(m) if m.is_dir() => Before::Directory(m.permissions().mode() & 0o7777),
            Ok(_) => {
                return Err(format!(
                    "unsupported install destination: {}",
                    path.display()
                ));
            }
        };
        self.entries.insert(path.into(), before);
        Ok(())
    }
    pub(crate) fn file(&mut self, path: &Path) -> Result<(), String> {
        self.capture(path)?;
        let mut parent = path.parent();
        while let Some(p) = parent {
            if p.symlink_metadata().is_ok() {
                break;
            }
            self.capture(p)?;
            parent = p.parent();
        }
        Ok(())
    }
    fn managed_file(&mut self, path: &Path) -> Result<(), String> {
        self.file(path)?;
        let name = path
            .file_name()
            .ok_or("install file without name")?
            .to_string_lossy();
        self.file(&path.with_file_name(format!("{name}.pre-comandos")))?;
        Ok(())
    }
    pub(crate) fn actions(&mut self, actions: &[Action]) -> Result<(), String> {
        for action in actions {
            match action {
                Action::Mkdir(path, _) => self.file(path)?,
                Action::Link { name, at, .. } => {
                    self.file(at)?;
                    let home = at
                        .parent()
                        .and_then(Path::parent)
                        .and_then(Path::parent)
                        .ok_or("alias without HOME")?;
                    for suffix in ["target", "orig"] {
                        self.file(
                            &home
                                .join(".local/share/comandos/rollback")
                                .join(format!("{name}.{suffix}")),
                        )?;
                    }
                }
                Action::Write { path, .. }
                | Action::WriteUnit { path, .. }
                | Action::WriteIfAbsent { path, .. } => self.managed_file(path)?,
                Action::RegisterClaudeHooks(home) => {
                    self.managed_file(&home.join(".claude/settings.json"))?
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn tree(&mut self, path: &Path) -> Result<(), String> {
        self.file(path)?;
        if path.symlink_metadata().is_ok_and(|m| m.is_dir()) {
            for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
                self.tree(&entry.map_err(|e| e.to_string())?.path())?;
            }
        }
        Ok(())
    }
    fn overlay(&mut self, source: &Path, dest: &Path) -> Result<(), String> {
        self.file(dest)?;
        if source.symlink_metadata().is_ok_and(|m| m.is_dir()) {
            for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                self.overlay(&entry.path(), &dest.join(entry.file_name()))?;
            }
        }
        Ok(())
    }
    pub(crate) fn full(home: &Path, source: &Path, actions: &[Action]) -> Result<Self, String> {
        let mut journal = Self::default();
        journal.actions(actions)?;
        for relative in [
            ".local/share/comandos/bin/comandos",
            ".local/share/comandos/bin/comandos-app",
            ".local/share/comandos/releases/previous",
            ".local/share/comandos/opencode-bridge.mjs",
            ".config/opencode/plugin/comandos.js",
            ".local/bin/grok-hooks.py",
            ".local/share/comandos/build/claude-codex/release/claude-codex",
        ] {
            journal.managed_file(&home.join(relative))?;
        }
        for name in ["cc-app", "cc-model-proxy", "cc-notifyd"] {
            journal.actions(&[Action::Link {
                name: name.into(),
                at: home.join(".local/bin").join(name),
                target: PathBuf::new(),
            }])?;
        }
        let agent = home
            .join("Library/LaunchAgents")
            .join(super::darwin::AGENT_NAME);
        journal.managed_file(&agent)?;
        for name in [
            format!("{}.state", super::darwin::AGENT_NAME),
            format!("{}.orig", super::darwin::AGENT_NAME),
            "ComandOS.app.state".into(),
        ] {
            journal.file(&home.join(".local/share/comandos/rollback").join(name))?;
        }
        for path in super::retarget::config_paths(home)? {
            journal.managed_file(&path)?;
            if path
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
            {
                journal.file(&fs::canonicalize(&path).map_err(|e| e.to_string())?)?;
            }
            if let Ok(bytes) = fs::read(&path) {
                use sha2::{Digest, Sha256};
                let hash = format!("{:x}", Sha256::digest(bytes));
                let short = hash.get(..12).ok_or("hash prefix")?;
                let name = path.file_name().ok_or("config name")?.to_string_lossy();
                journal.file(&path.with_file_name(format!("{name}.pre-comandos-{short}")))?;
            }
        }
        for relative in [".codex/hooks.json", ".gemini/antigravity-cli/settings.json"] {
            journal.managed_file(&home.join(relative))?;
        }
        for relative in [
            "Applications/ComandOS.app",
            "Applications/ComandOS.app.previous",
        ] {
            let dest = home.join(relative);
            journal.tree(&dest)?;
            journal.overlay(&source.join("ComandOS.app"), &dest)?;
        }
        journal.overlay(
            &home.join("Applications/ComandOS.app"),
            &home.join("Applications/ComandOS.app.previous"),
        )?;
        journal.tree(&home.join(".local/share/comandos/rollback"))?;
        Ok(journal)
    }
    /// Restore in reverse path order so newly-created children disappear before
    /// their parents. Immutable staged releases are retained for retry; no pruning
    /// occurs until the full transaction has succeeded.
    fn observe(&mut self) -> Result<(), String> {
        for path in self.entries.keys() {
            let mut snapshot = Self::default();
            snapshot.capture(path)?;
            if let Some(before) = snapshot.entries.remove(path) {
                self.observed.insert(path.clone(), before);
            }
        }
        Ok(())
    }
    pub(crate) fn rollback(self) -> Result<(), String> {
        let mut errors = Vec::new();
        for (path, before) in self.entries.into_iter().rev() {
            let mut now = Self::default();
            let captured = now.capture(&path);
            if captured.is_err() || now.entries.get(&path) != self.observed.get(&path) {
                errors.push(format!(
                    "{} changed after the failed install; preserved",
                    path.display()
                ));
                continue;
            }
            if let Err(e) = restore(&path, before) {
                errors.push(format!("{}: {e}", path.display()));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}
fn restore(path: &Path, before: Before) -> Result<(), String> {
    super::release::check_app_parents(path)?;
    match before {
        Before::Absent => match path.symlink_metadata() {
            Ok(m) if m.is_dir() => match fs::remove_dir(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::DirectoryNotEmpty => Ok(()),
                Err(e) => Err(e.to_string()),
            },
            Ok(_) => fs::remove_file(path).map_err(|e| e.to_string()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        },
        Before::Directory(mode) => {
            if path.symlink_metadata().is_err() {
                fs::create_dir_all(path).map_err(|e| e.to_string())?;
            }
            if !path.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                return Err("directory changed during install".into());
            }
            fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())
        }
        Before::File(bytes, mode) => {
            if path.symlink_metadata().is_ok_and(|m| m.is_file())
                && fs::read(path).is_ok_and(|b| b == bytes)
                && path
                    .metadata()
                    .is_ok_and(|m| m.permissions().mode() & 0o7777 == mode)
            {
                return Ok(());
            }
            fs::create_dir_all(path.parent().ok_or("restore file without parent")?)
                .map_err(|e| e.to_string())?;
            write_atomic(path, &bytes).map_err(|e| e.to_string())?;
            fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())
        }
        Before::Link(target) => {
            if fs::read_link(path).is_ok_and(|p| p == target) {
                return Ok(());
            }
            fs::create_dir_all(path.parent().ok_or("restore link without parent")?)
                .map_err(|e| e.to_string())?;
            let tmp = path.with_file_name(format!(".install-restore-{}", std::process::id()));
            symlink(target, &tmp).map_err(|e| e.to_string())?;
            let result = fs::rename(&tmp, path).map_err(|e| e.to_string());
            if result.is_err() {
                let _ = fs::remove_file(tmp);
            }
            result
        }
    }
}
pub(crate) fn finish<T>(mut journal: Journal, result: Result<T, String>) -> Result<T, String> {
    match result {
        Ok(value) => Ok(value),
        Err(mut error) => {
            if let Err(e) = journal.observe() {
                return Err(format!("{error}; cannot admit file rollback: {e}"));
            }
            if let Err(e) = journal.rollback() {
                error.push_str(&format!("; installation file rollback incomplete: {e}"));
            }
            Err(error)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn root(name: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("install-preimage-{name}-{}", std::process::id()));
        fs::create_dir(&p).unwrap();
        p
    }
    #[test]
    fn app_overlay_restores_removed_files_modes_links_and_deletes_new_inputs() {
        let root = root("overlay");
        let app = root.join("Applications/ComandOS.app");
        fs::create_dir_all(app.join("Contents")).unwrap();
        fs::write(app.join("Contents/old"), b"old input").unwrap();
        fs::set_permissions(app.join("Contents/old"), fs::Permissions::from_mode(0o751)).unwrap();
        symlink("old", app.join("Contents/link")).unwrap();
        let source = root.join("source");
        fs::create_dir_all(source.join("Contents")).unwrap();
        fs::write(source.join("Contents/new"), b"new input").unwrap();
        let mut journal = Journal::default();
        journal.tree(&app).unwrap();
        journal.overlay(&source, &app).unwrap();
        fs::remove_dir_all(&app).unwrap();
        fs::create_dir_all(app.join("Contents")).unwrap();
        fs::write(app.join("Contents/new"), b"new input").unwrap();
        finish::<()>(journal, Err("fixture failure".into())).unwrap_err();
        assert_eq!(fs::read(app.join("Contents/old")).unwrap(), b"old input");
        assert_eq!(
            app.join("Contents/old")
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o751
        );
        assert_eq!(
            fs::read_link(app.join("Contents/link")).unwrap(),
            Path::new("old")
        );
        assert!(!app.join("Contents/new").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rollback_preserves_a_file_changed_after_the_failed_phase_observation() {
        let root = root("concurrent");
        let file = root.join("config");
        fs::write(&file, b"original").unwrap();
        let mut journal = Journal::default();
        journal.capture(&file).unwrap();
        fs::write(&file, b"installed").unwrap();
        journal.observe().unwrap();
        fs::write(&file, b"external edit").unwrap();
        let error = journal.rollback().unwrap_err();
        assert!(error.contains("preserved"));
        assert_eq!(fs::read(file).unwrap(), b"external edit");
        fs::remove_dir_all(root).unwrap();
    }
}
