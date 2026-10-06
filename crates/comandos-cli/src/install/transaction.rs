//! Bounded installation preimages. Only declared install destinations are captured;
//! sessions, agent histories and the rest of HOME are never traversed.
use super::plan::Action;
use comandos_store::files::write_atomic;
use std::{
    collections::BTreeMap,
    fs, io,
    io::Read,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Before {
    Absent,
    File(Vec<u8>, u32),
    Link(PathBuf),
    Directory(u32),
}
#[derive(Default)]
pub(crate) struct Journal {
    pub(super) entries: BTreeMap<PathBuf, Before>,
    pub(super) observed: BTreeMap<PathBuf, Before>,
    pub(super) pending: BTreeMap<PathBuf, Before>,
    pub(super) extensions: bool,
    pub(super) agents: bool,
    pub(super) agent_paths: std::collections::BTreeSet<PathBuf>,
    pub(super) extension_trees: Vec<PathBuf>,
    durable: Option<PathBuf>,
    committed: bool,
    reload_error: Option<String>,
    runtime: Vec<RuntimeEffect>,
    recompute: Vec<RecomputeEffect>,
    pub(super) documents: Vec<super::extension_mutations::DocumentEffect>,
}
impl Journal {
    pub(super) fn install_home(&self) -> Option<&Path> {
        self.durable.as_ref()?.ancestors().nth(5)
    }
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
        self.observed.insert(path.into(), before.clone());
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
    /// Admit only the selected file/link, then publish its rename destination
    /// before mutation. Retargeted sources and occupied destinations fail closed.
    pub(crate) fn prepare_archive(&mut self, source: &Path, target: &Path) -> Result<(), String> {
        let meta = source.symlink_metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() && !meta.file_type().is_symlink() {
            return Err(format!(
                "transactional archive requires a file or link: {}",
                source.display()
            ));
        }
        self.tree(source)?;
        self.overlay(source, target)?;
        let selected = self
            .entries
            .keys()
            .filter(|p| p.starts_with(source))
            .cloned()
            .collect::<Vec<_>>();
        for path in selected {
            let mut actual = Self::default();
            actual.capture(&path)?;
            if actual.entries.get(&path) != self.observed.get(&path)
                && actual.entries.get(&path) != self.entries.get(&path)
            {
                return Err(format!("archive source changed: {}", path.display()));
            }
            let dest = target.join(path.strip_prefix(source).map_err(|e| e.to_string())?);
            if self.entries.get(&dest) != Some(&Before::Absent) || dest.symlink_metadata().is_ok() {
                return Err(format!("archive destination occupied: {}", dest.display()));
            }
            let written = actual
                .entries
                .remove(&path)
                .ok_or("archive preimage missing")?;
            self.observed.insert(dest, written);
            self.observed.insert(path, Before::Absent);
        }
        if let Some(parent) = target.parent() {
            self.prepare_private_parents(parent)?;
        }
        self.persist()
    }
    pub(crate) fn prepare_private_parents(&mut self, path: &Path) -> Result<(), String> {
        self.file(path)?;
        let paths = self
            .entries
            .iter()
            .filter(|(p, v)| path.starts_with(p) && **v == Before::Absent)
            .map(|(p, _)| p.clone())
            .collect::<Vec<_>>();
        for parent in paths {
            self.observed.insert(parent, Before::Directory(0o700));
        }
        self.persist()
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
    pub(crate) fn tree(&mut self, path: &Path) -> Result<(), String> {
        self.file(path)?;
        if path.symlink_metadata().is_ok_and(|m| m.is_dir()) {
            for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
                self.tree(&entry.map_err(|e| e.to_string())?.path())?;
            }
        }
        Ok(())
    }
    pub(crate) fn overlay(&mut self, source: &Path, dest: &Path) -> Result<(), String> {
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
    pub(crate) fn checkpoint(&mut self, paths: &[PathBuf]) -> Result<(), String> {
        for path in paths {
            let mut snapshot = Self::default();
            snapshot.capture(path)?;
            if let Some(after) = snapshot.entries.remove(path) {
                let same_directory = matches!((&after,self.observed.get(path),self.entries.get(path)),(Before::Directory(actual),Some(Before::Directory(requested)),Some(Before::Absent)) if actual & requested == *actual);
                let same_bytes = matches!((&after,self.observed.get(path)),(Before::File(a,_),Some(Before::File(b,_))) if a==b);
                if Some(&after) != self.observed.get(path)
                    && Some(&after) != self.entries.get(path)
                    && !same_bytes
                    && !same_directory
                {
                    let modes = match (&after, self.observed.get(path)) {
                        (Before::Directory(a), Some(Before::Directory(b))) => {
                            format!(" (directory actual={a:o} expected={b:o})")
                        }
                        _ => String::new(),
                    };
                    return Err(format!(
                        "{} differs from the state written by installation; journal retained{modes}",
                        path.display()
                    ));
                }
                self.observed.insert(path.clone(), after);
                self.pending.remove(path);
            }
        }
        self.persist()
    }
    pub(crate) fn produced_file(&mut self, path: &Path) -> Result<(), String> {
        let mut snapshot = Self::default();
        snapshot.capture(path)?;
        if let Some(value) = snapshot.entries.remove(path) {
            self.observed.insert(path.into(), value);
        }
        self.persist()
    }
    pub(crate) fn expect_link(&mut self, path: &Path, target: &Path) -> Result<(), String> {
        self.observed
            .insert(path.into(), Before::Link(target.into()));
        self.persist()
    }
    pub(crate) fn prepare_release(&mut self, home: &Path, id: &str) -> Result<(), String> {
        use sha2::{Digest, Sha256};
        let pointer = home.join(".local/share/comandos/bin/comandos");
        let old = match self.entries.get(&pointer) {
            Some(Before::Link(to)) => to
                .parent()
                .and_then(Path::file_name)
                .and_then(|n| n.to_str())
                .map(str::to_string),
            Some(Before::File(bytes, _)) => Some(
                hex(&Sha256::digest(bytes))
                    .get(..12)
                    .ok_or("old release hash")?
                    .into(),
            ),
            _ => None,
        };
        if let Some(old) = old
            && old != id
        {
            self.expect_file(
                &home.join(".local/share/comandos/releases/previous"),
                format!("{old}\n").as_bytes(),
                0o644,
            )?;
        }
        self.expect_link(
            &pointer,
            &Path::new("../releases").join(id).join("comandos"),
        )
    }
    pub(crate) fn expect_file(
        &mut self,
        path: &Path,
        bytes: &[u8],
        mode: u32,
    ) -> Result<(), String> {
        self.observed
            .insert(path.into(), Before::File(bytes.into(), mode));
        self.persist()
    }
    pub(crate) fn prepare(&mut self, action: &Action) -> Result<(), String> {
        match action {
            Action::Mkdir(path, mode) => {
                if matches!(self.entries.get(path), Some(Before::Absent))
                    && path.symlink_metadata().is_err()
                {
                    self.observed.insert(path.clone(), Before::Directory(*mode));
                }
            }
            Action::Link { name, at, target } => {
                let home = at
                    .parent()
                    .and_then(Path::parent)
                    .and_then(Path::parent)
                    .ok_or("alias HOME")?;
                let target = if name == "cc-app" {
                    super::release::app_release(home)?
                } else {
                    target.clone()
                };
                self.observed.insert(at.clone(), Before::Link(target));
                let orig = home
                    .join(".local/share/comandos/rollback")
                    .join(format!("{name}.orig"));
                let record = super::record::path(home, name);
                let raw = match self.entries.get(at) {
                    Some(Before::File(bytes, mode)) => {
                        self.observed
                            .insert(orig.clone(), Before::File(bytes.clone(), *mode));
                        format!("FILE:{}\n", orig.display()).into_bytes()
                    }
                    Some(Before::Link(to)) => {
                        use std::os::unix::ffi::OsStrExt;
                        [b"LINK:".as_slice(), to.as_os_str().as_bytes(), b"\n"].concat()
                    }
                    _ => b"ABSENT\n".to_vec(),
                };
                self.observed.insert(record, Before::File(raw, 0o644));
            }
            Action::WriteIfAbsent { path, bytes, mode } => {
                let mut current = Self::default();
                current.capture(path)?;
                if current.entries.get(path) != self.observed.get(path) {
                    return Err(format!(
                        "{} changed before install write; preserved",
                        path.display()
                    ));
                }
                if matches!(self.entries.get(path), Some(Before::Absent)) {
                    self.observed
                        .insert(path.clone(), Before::File(bytes.to_vec(), *mode));
                }
            }
            Action::Write { path, bytes, mode } => {
                self.observed
                    .insert(path.clone(), Before::File(bytes.clone(), *mode));
                self.expect_backup(path)?;
            }
            Action::WriteUnit {
                path,
                bytes,
                original,
            } => {
                let prior = fs::read(path).ok();
                if prior
                    .as_deref()
                    .is_none_or(|v| v == *original || v == bytes.as_slice())
                {
                    self.observed
                        .insert(path.clone(), Before::File(bytes.clone(), 0o644));
                    self.expect_backup(path)?;
                }
            }
            Action::RegisterClaudeHooks(home) => {
                self.expect_backup(&home.join(".claude/settings.json"))?
            }
            _ => {}
        }
        self.persist()
    }
    fn expect_backup(&mut self, path: &Path) -> Result<(), String> {
        let name = path.file_name().ok_or("backup name")?.to_string_lossy();
        let backup = path.with_file_name(format!("{name}.pre-comandos"));
        if matches!(self.entries.get(&backup), Some(Before::Absent))
            && let Some(before) = self.entries.get(path)
        {
            self.observed.insert(backup, before.clone());
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn rollback(self) -> Result<(), String> {
        self.rollback_with(&mut |_| Err("runtime compensation runner unavailable".into()))
    }
    fn rollback_files(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        // Restore admitted directory parents before their child paths: skills
        // may have replaced an original tree root with our canonical symlink.
        for (path, before) in &self.entries {
            let Before::Directory(mode) = before else {
                continue;
            };
            let mut actual = Self::default();
            if let Err(e) = actual.capture(path) {
                errors.push(e);
                continue;
            }
            let current = actual.entries.get(path);
            if matches!(
                current,
                Some(Before::Absent | Before::Link(_) | Before::File(_, _))
            ) {
                if current != self.observed.get(path) {
                    errors.push(format!(
                        "{} directory parent changed; preserved",
                        path.display()
                    ));
                    continue;
                }
                if !matches!(current, Some(Before::Absent))
                    && let Err(e) = fs::remove_file(path)
                {
                    errors.push(format!("{}: {e}", path.display()));
                    continue;
                }
                if let Err(e) = fs::DirBuilder::new().mode(*mode).create(path) {
                    errors.push(format!("{}: {e}", path.display()));
                }
            }
        }
        // Newly created private trees have no pre-existing destination locks.
        // The shared installation lock covers recovery; avoid creating control
        // files inside trees whose empty directories must be removed below.
        let new_directories = self
            .entries
            .iter()
            .filter(|(path, before)| {
                **before == Before::Absent
                    && matches!(self.observed.get(*path), Some(Before::Directory(_)))
            })
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        for (path, before) in self.entries.clone().into_iter().rev() {
            let private_parent = path
                .parent()
                .is_some_and(|parent| new_directories.iter().any(|dir| parent.starts_with(dir)));
            let extension_tree = self
                .extension_trees
                .iter()
                .any(|root| path.starts_with(root));
            let _file_guard = if !private_parent
                && !extension_tree
                && !self.agent_paths.contains(&path)
                && path.parent().is_some_and(Path::is_dir)
            {
                let name = path
                    .file_name()
                    .ok_or("rollback destination name")?
                    .to_string_lossy();
                Some(
                    match comandos_store::files::FileLock::exclusive(
                        &path.with_file_name(format!("{name}.install.lock")),
                    ) {
                        Ok(guard) => guard,
                        Err(e) => {
                            errors.push(format!("{} rollback lock: {e}", path.display()));
                            continue;
                        }
                    },
                )
            } else {
                None
            };
            let mut now = Self::default();
            let captured = now.capture(&path);
            if now.entries.get(&path) == Some(&before) {
                continue;
            }
            if matches!(before, Before::Absent)
                && matches!(self.observed.get(&path), Some(Before::Absent))
                && now
                    .entries
                    .get(&path)
                    .is_some_and(|v| matches!(v, Before::Directory(_)))
            {
                if let Err(e) = restore(&path, before) {
                    errors.push(format!("{}: {e}", path.display()));
                }
                continue;
            }
            if captured.is_err()
                || (now.entries.get(&path) != self.observed.get(&path)
                    && now.entries.get(&path) != self.pending.get(&path))
            {
                errors.push(format!(
                    "{} differs from the state written by installation; preserved",
                    path.display()
                ));
                continue;
            }
            if !matches!(before, Before::Absent) {
                for parent in path
                    .ancestors()
                    .skip(1)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                {
                    if let Some(Before::Directory(mode)) = self.entries.get(parent)
                        && parent.symlink_metadata().is_err()
                        && let Err(e) = fs::DirBuilder::new().mode(*mode).create(parent)
                    {
                        errors.push(format!("{}: {e}", parent.display()));
                    }
                }
            }
            if let Err(e) = restore(&path, before) {
                errors.push(format!("{}: {e}", path.display()));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{}; retained recovery journal: {}",
                errors.join("; "),
                self.durable
                    .as_ref()
                    .map_or_else(|| "in-memory".into(), |p| p.display().to_string())
            ))
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
#[cfg(test)]
pub(crate) fn finish<T>(journal: Journal, result: Result<T, String>) -> Result<T, String> {
    match result {
        Ok(value) => {
            let mut journal = journal;
            if let Err(error) = journal.commit() {
                return finish(journal, Err(error));
            }
            journal.discard()?;
            Ok(value)
        }
        Err(mut error) => {
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
        fs::set_permissions(app.join("Contents/new"), fs::Permissions::from_mode(0o644)).unwrap();
        journal
            .observed
            .insert(app.join("Contents/old"), Before::Absent);
        journal
            .observed
            .insert(app.join("Contents/link"), Before::Absent);
        journal
            .expect_file(&app.join("Contents/new"), b"new input", 0o644)
            .unwrap();
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
        journal.expect_file(&file, b"installed", 0o644).unwrap();
        fs::write(&file, b"external edit").unwrap();
        let error = journal.rollback().unwrap_err();
        assert!(error.contains("preserved"));
        assert_eq!(fs::read(file).unwrap(), b"external edit");
        fs::remove_dir_all(root).unwrap();
    }
}

// Nested installer subplans share the same guard on their current thread;
// independent threads/processes still contend on the HOME directory inode.
thread_local! {static HELD_INSTALL_LOCKS:std::cell::RefCell<BTreeMap<(u64,u64),std::rc::Weak<fs::File>>>=const{std::cell::RefCell::new(BTreeMap::new())};}
pub(crate) fn installation_lock(home: &Path) -> Result<std::rc::Rc<fs::File>, String> {
    super::release::check_app_parents(&home.join("placeholder"))?;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(home)
        .map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    let key = (metadata.dev(), metadata.ino());
    if let Some(held) =
        HELD_INSTALL_LOCKS.with(|locks| locks.borrow().get(&key).and_then(std::rc::Weak::upgrade))
    {
        return Ok(held);
    }
    file.lock().map_err(|e| e.to_string())?;
    let held = file.metadata().map_err(|e| e.to_string())?;
    let current = home.symlink_metadata().map_err(|e| e.to_string())?;
    if !current.is_dir() || (held.dev(), held.ino()) != (current.dev(), current.ino()) {
        return Err("HOME changed during install lock admission".into());
    }
    let guard = std::rc::Rc::new(file);
    HELD_INSTALL_LOCKS.with(|locks| {
        let mut locks = locks.borrow_mut();
        locks.retain(|_, guard| guard.strong_count() > 0);
        locks.insert(key, std::rc::Rc::downgrade(&guard));
    });
    Ok(guard)
}
pub(crate) fn inherited_installation_lock(home: &Path) -> Result<std::rc::Rc<fs::File>, String> {
    let stdin = std::io::stdin();
    let file = fs::File::from(nix::unistd::dup(&stdin).map_err(|e| e.to_string())?);
    let held = file.metadata().map_err(|e| e.to_string())?;
    let current = home.symlink_metadata().map_err(|e| e.to_string())?;
    if !held.is_dir()
        || !current.is_dir()
        || (held.dev(), held.ino()) != (current.dev(), current.ino())
    {
        return Err("worker lacks inherited HOME installation lock".into());
    }
    file.try_lock().map_err(|e| e.to_string())?;
    let guard = std::rc::Rc::new(file);
    HELD_INSTALL_LOCKS.with(|locks| {
        locks
            .borrow_mut()
            .insert((held.dev(), held.ino()), std::rc::Rc::downgrade(&guard));
    });
    Ok(guard)
}
impl Journal {
    pub(crate) fn operation(
        &mut self,
        home: &Path,
        operation: &str,
        run: &mut dyn FnMut(&super::plan::Action) -> Result<(), String>,
    ) -> Result<(), String> {
        let quiescent = std::rc::Rc::new(std::cell::RefCell::new(false));
        let result = run(&super::plan::Action::ExtensionOperation {
            home: home.into(),
            journal: self.durable_path()?.into(),
            operation: operation.into(),
            quiescent: quiescent.clone(),
        });
        if !*quiescent.borrow() {
            self.refuse_rollback("owned worker group quiescence unproved");
            return Err("owned worker group quiescence unproved; journal retained".into());
        }
        self.reload(home)?;
        result
    }
    pub(crate) fn durable_path(&self) -> Result<&Path, String> {
        self.durable
            .as_deref()
            .ok_or_else(|| "durable extension journal absent".into())
    }
    pub(crate) fn refuse_rollback(&mut self, error: &str) {
        self.reload_error = Some(error.into());
    }
    pub(crate) fn reload(&mut self, home: &Path) -> Result<(), String> {
        let loaded = match load_journal(home, self.durable_path()?) {
            Ok(journal) => journal,
            Err(error) => {
                self.reload_error = Some(error.clone());
                return Err(error);
            }
        };
        if loaded.committed {
            self.reload_error = Some("worker committed outer journal".into());
            return Err("worker committed the outer journal".into());
        }
        *self = loaded;
        Ok(())
    }
    pub(crate) fn commit(&mut self) -> Result<(), String> {
        if self.committed {
            return Ok(());
        }
        self.committed = true;
        if let Err(e) = self.persist() {
            self.committed = false;
            return Err(e);
        }
        Ok(())
    }
    fn discard(self) -> Result<(), String> {
        if let Some(path) = self.durable {
            fs::remove_dir_all(&path).map_err(|e| {
                format!(
                    "installation journal cleanup failed; retained journal {}: {e}",
                    path.display()
                )
            })?;
        }
        Ok(())
    }
    pub(crate) fn durable(&mut self, home: &Path) -> Result<(), String> {
        for path in self.entries.keys() {
            if !path.starts_with(home) || path.to_str().is_none() {
                return Err("durable journal destinations must be UTF8 paths within HOME".into());
            }
        }
        let parent = home.join(".local/share/comandos/install-journals");
        super::release::check_app_parents(&parent.join("placeholder"))?;
        fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
        let path = parent.join(hex(&nonce));
        fs::create_dir(&path).map_err(|e| e.to_string())?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        self.durable = Some(path);
        self.persist()?;
        for ancestor in parent.ancestors().take_while(|p| p.starts_with(home)) {
            fs::File::open(ancestor)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub(crate) fn persist(&self) -> Result<(), String> {
        let Some(root) = &self.durable else {
            return Ok(());
        };
        let mut rows = Vec::new();
        for (path, before) in &self.entries {
            rows.push(serde_json::json!({"path":path.to_str().ok_or("journal requires UTF8 destination")?,"before":encode_before(root,before)?,"written":encode_before(root,self.observed.get(path).ok_or("journal written state")?)?}));
        }
        let mut pending = Vec::new();
        for (path, before) in &self.pending {
            pending.push(serde_json::json!({"path":path.to_str().ok_or("pending path requires UTF8")?,"before":encode_before(root,before)?}));
        }
        let path = root.join("manifest.json");
        let bytes = serde_json::to_vec(
            &serde_json::json!({"version":1,"committed":self.committed,"entries":rows,"runtime":self.runtime,"recompute":self.recompute,"documents":self.documents,"pending":pending,"extensions":self.extensions,"agents":self.agents,"agent_paths":self.agent_paths,"extension_trees":self.extension_trees}),
        )
        .map_err(|e| e.to_string())?;
        write_atomic(&path, &bytes).map_err(|e| e.to_string())?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
        fs::File::open(&path)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        fs::File::open(root)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        fs::File::open(root.parent().ok_or("journal parent")?)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn encode_before(root: &Path, value: &Before) -> Result<serde_json::Value, String> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    Ok(match value {
        Before::Absent => serde_json::json!({"kind":"absent"}),
        Before::Directory(mode) => serde_json::json!({"kind":"directory","mode":mode}),
        Before::Link(target) => {
            serde_json::json!({"kind":"link","target":hex(target.as_os_str().as_bytes())})
        }
        Before::File(bytes, mode) => {
            let hash = hex(&Sha256::digest(bytes));
            let path = root.join(&hash);
            if !path.exists() {
                let mut f = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&path)
                    .map_err(|e| e.to_string())?;
                f.write_all(bytes)
                    .and_then(|()| f.sync_all())
                    .map_err(|e| e.to_string())?;
            }
            serde_json::json!({"kind":"file","blob":hash,"mode":mode})
        }
    })
}
fn decode_before(root: &Path, value: &serde_json::Value) -> Result<Before, String> {
    use sha2::{Digest, Sha256};
    use std::os::unix::ffi::OsStringExt;
    let mode = || {
        value["mode"]
            .as_u64()
            .filter(|m| *m <= 0o7777)
            .map(|m| m as u32)
            .ok_or("journal mode".to_string())
    };
    match value["kind"].as_str() {
        Some("absent") => Ok(Before::Absent),
        Some("directory") => Ok(Before::Directory(mode()?)),
        Some("link") => {
            let raw = value["target"].as_str().ok_or("journal link")?;
            let mut bytes = Vec::new();
            for pair in raw.as_bytes().chunks(2) {
                if pair.len() != 2 {
                    return Err("journal link encoding".into());
                }
                bytes.push(
                    u8::from_str_radix(std::str::from_utf8(pair).map_err(|e| e.to_string())?, 16)
                        .map_err(|e| e.to_string())?,
                );
            }
            Ok(Before::Link(std::ffi::OsString::from_vec(bytes).into()))
        }
        Some("file") => {
            let hash = value["blob"]
                .as_str()
                .filter(|s| {
                    s.len() == 64
                        && s.bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
                .ok_or("journal blob hash")?;
            let path = root.join(hash);
            let mut file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(nix::libc::O_NOFOLLOW)
                .open(&path)
                .map_err(|e| e.to_string())?;
            if !file.metadata().map_err(|e| e.to_string())?.is_file() {
                return Err("journal blob not regular".into());
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            if hex(&Sha256::digest(&bytes)) != hash {
                return Err("journal blob hash mismatch".into());
            }
            Ok(Before::File(bytes, mode()?))
        }
        _ => Err("journal kind".into()),
    }
}
#[cfg(test)]
pub(crate) fn recover(home: &Path, path: &Path) -> Result<(), String> {
    recover_with(home, path, &mut |_| {
        Err("runtime compensation runner unavailable".into())
    })
}
pub(crate) fn recover_with(
    home: &Path,
    path: &Path,
    run: &mut dyn FnMut(&Action) -> Result<(), String>,
) -> Result<(), String> {
    let _guard = installation_lock(home)?;
    let journal = load_journal(home, path)?;
    if journal.committed {
        journal.discard()
    } else {
        journal.rollback_with(run)
    }
}
pub(crate) fn load_active_journal(home: &Path, path: &Path) -> Result<Journal, String> {
    let journal = load_journal(home, path)?;
    if journal.committed {
        return Err("extension worker cannot modify committed journal".into());
    }
    Ok(journal)
}
pub(crate) fn load_journal(home: &Path, path: &Path) -> Result<Journal, String> {
    let parent = home.join(".local/share/comandos/install-journals");
    if path.parent() != Some(parent.as_path())
        || !path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("recovery path must name an owned installation journal".into());
    }
    super::release::check_app_parents(&path.join("manifest.json"))?;
    let _guard = installation_lock(home)?;
    let meta = path.symlink_metadata().map_err(|e| e.to_string())?;
    if !meta.is_dir()
        || meta.uid() != nix::unistd::Uid::effective().as_raw()
        || meta.permissions().mode() & 0o077 != 0
    {
        return Err("recovery journal is not a private owned directory".into());
    }
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path.join("manifest.json"))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value["version"] != 1 {
        return Err("unknown installation journal version".into());
    }
    let mut journal = Journal {
        durable: Some(path.into()),
        committed: value["committed"].as_bool().ok_or("journal commit state")?,
        ..Default::default()
    };
    if let Some(documents) = value.get("documents") {
        journal.documents = serde_json::from_value(documents.clone()).map_err(|e| e.to_string())?;
        super::extension_mutations::validate_documents(home, &journal.documents)?;
    }
    if let Some(recompute) = value.get("recompute") {
        journal.recompute = serde_json::from_value(recompute.clone()).map_err(|e| e.to_string())?;
        let mut unique = std::collections::BTreeSet::new();
        for effect in &journal.recompute {
            if !effect.valid(home) || !unique.insert(effect.clone()) {
                return Err("invalid/duplicate runtime recomputation".into());
            }
        }
    }
    if let Some(runtime) = value.get("runtime") {
        journal.runtime = serde_json::from_value(runtime.clone()).map_err(|e| e.to_string())?;
        for effect in &journal.runtime {
            if effect.home != home
                || !(effect.unit == "cc-telegram.service" && !effect.activation
                    || effect.unit == "comandos-extensions-sync.timer" && effect.activation)
                || (!effect.before.enabled && effect.before.runtime)
            {
                return Err("unsupported runtime recovery effect".into());
            }
        }
        if journal
            .runtime
            .iter()
            .map(|e| &e.unit)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != journal.runtime.len()
        {
            return Err("duplicate runtime recovery effect".into());
        }
    }
    for row in value["entries"].as_array().ok_or("journal entries")? {
        let dest = PathBuf::from(row["path"].as_str().ok_or("journal destination")?);
        if !dest.starts_with(home)
            || dest.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err("journal destination outside HOME".into());
        }
        if journal.entries.contains_key(&dest) {
            return Err("duplicate journal destination".into());
        }
        journal
            .entries
            .insert(dest.clone(), decode_before(path, &row["before"])?);
        journal
            .observed
            .insert(dest, decode_before(path, &row["written"])?);
    }
    // Published skill roots can replace an original directory with our own
    // link. Validate that exact admitted root without following it to inspect
    // original child paths; rollback restores the directory before its children.
    for dest in journal.entries.keys() {
        let owned_parent = dest.ancestors().skip(1).find(|parent| {
            matches!(journal.entries.get(*parent), Some(Before::Directory(_)))
                && matches!(journal.observed.get(*parent), Some(Before::Link(_)))
                && parent.is_symlink()
        });
        if let Some(parent) = owned_parent {
            super::release::check_app_parents(parent)?;
            if !matches!(journal.observed.get(parent),Some(Before::Link(target)) if fs::read_link(parent).is_ok_and(|actual|actual==*target))
            {
                return Err(format!(
                    "{} published skill root changed; journal retained",
                    parent.display()
                ));
            }
        } else {
            super::release::check_app_parents(dest)?;
        }
    }
    journal.agents = match value.get("agents") {
        Some(v) => v.as_bool().ok_or("invalid agents journal scope")?,
        None => false,
    };
    if let Some(paths) = value.get("agent_paths") {
        journal.agent_paths = serde_json::from_value(paths.clone()).map_err(|e| e.to_string())?;
        if (!journal.agents && !journal.agent_paths.is_empty())
            || journal.agent_paths.iter().any(|p| {
                !journal.entries.contains_key(p)
                    && *p != home.join(".local/share/comandos/agents.lock")
            })
        {
            return Err("invalid agent mutation paths".into());
        }
    }
    journal.extensions = value
        .get("extensions")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if let Some(trees) = value.get("extension_trees") {
        journal.extension_trees =
            serde_json::from_value(trees.clone()).map_err(|e| e.to_string())?;
        for root in &journal.extension_trees {
            if !root.starts_with(home)
                || !matches!(journal.entries.get(root), Some(Before::Directory(_)))
            {
                return Err("invalid extension tree scope".into());
            }
        }
    }
    if !journal.extension_trees.is_empty() && !journal.extensions {
        return Err("extension tree without mutation scope".into());
    }
    if let Some(pending) = value.get("pending") {
        for row in pending.as_array().ok_or("pending journal entries")? {
            let path = PathBuf::from(row["path"].as_str().ok_or("pending journal path")?);
            if !journal.entries.contains_key(&path) || journal.pending.contains_key(&path) {
                return Err("invalid pending journal destination".into());
            }
            journal.pending.insert(
                path,
                decode_before(
                    journal.durable.as_ref().ok_or("pending journal missing")?,
                    &row["before"],
                )?,
            );
        }
    }
    Ok(journal)
}

#[cfg(test)]
mod durable_tests {
    use super::*;
    fn private_home(tag: &str) -> PathBuf {
        let home =
            std::env::temp_dir().join(format!("install-durable-{tag}-{}", std::process::id()));
        fs::create_dir(&home).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        home
    }
    #[test]
    fn committed_journal_recovery_only_cleans_and_never_restores_old_public_state() {
        let home = private_home("commit");
        let file = home.join("config");
        fs::write(&file, b"original").unwrap();
        let mut journal = Journal::default();
        journal.file(&file).unwrap();
        journal.durable(&home).unwrap();
        journal.expect_file(&file, b"installed", 0o600).unwrap();
        fs::write(&file, b"installed").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        journal.commit().unwrap();
        let path = journal.durable.clone().unwrap();
        drop(journal);
        recover(&home, &path).unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"installed");
        assert!(!path.exists());
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn a_corrupt_preimage_is_rejected_before_any_recovery_write_and_retains_journal() {
        let home = private_home("corrupt");
        let file = home.join("config");
        fs::write(&file, b"original").unwrap();
        let mut journal = Journal::default();
        journal.file(&file).unwrap();
        journal.durable(&home).unwrap();
        let path = journal.durable.clone().unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap();
        let blob = manifest["entries"][0]["before"]["blob"].as_str().unwrap();
        fs::write(path.join(blob), b"corrupt").unwrap();
        drop(journal);
        let error = recover(&home, &path).unwrap_err();
        assert!(error.contains("hash mismatch"));
        assert!(path.exists());
        assert_eq!(fs::read(file).unwrap(), b"original");
        fs::remove_dir_all(home).unwrap();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
enum RecomputeEffect {
    Reload(PathBuf),
    Fonts { home: PathBuf, path: PathBuf },
}
impl RecomputeEffect {
    fn valid(&self, home: &Path) -> bool {
        match self {
            Self::Reload(owned) => owned == home,
            Self::Fonts { home: owned, path } => {
                owned == home && *path == home.join(".local/share/fonts/comandos")
            }
        }
    }
    fn action(&self) -> Action {
        match self {
            Self::Reload(home) => Action::Systemctl {
                home: home.clone(),
                args: vec!["--user".into(), "daemon-reload".into()],
            },
            Self::Fonts { path, .. } => Action::InstallFonts(path.clone()),
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RuntimeEffect {
    home: PathBuf,
    unit: String,
    before: super::plan::UnitState,
    #[serde(default)]
    activation: bool,
    #[serde(default)]
    compensating: bool,
}
impl RuntimeEffect {
    fn admitted(&self, current: &super::plan::UnitState) -> bool {
        if self.activation {
            (!self.before.enabled || current.enabled || self.compensating)
                && (!self.before.active || current.active)
                && (!current.runtime || self.before.runtime)
        } else {
            (!current.enabled || self.before.enabled && current.runtime == self.before.runtime)
                && (!current.active || self.before.active)
        }
    }
}
pub(crate) fn query_unit(
    home: &Path,
    unit: &str,
    require_loaded: bool,
    run: &mut dyn FnMut(&Action) -> Result<(), String>,
) -> Result<super::plan::UnitState, String> {
    let response = std::rc::Rc::new(std::cell::RefCell::new(None));
    run(&Action::SystemctlState {
        home: home.into(),
        unit: unit.into(),
        require_loaded,
        response: response.clone(),
    })?;
    let state = response
        .borrow_mut()
        .take()
        .ok_or("runtime query capability did not return enabled/active state")?;
    if !state.enabled && state.runtime {
        return Err("invalid runtime enablement state".into());
    }
    Ok(state)
}
impl Journal {
    pub(crate) fn prepare_unit_retirement(
        &mut self,
        home: &Path,
        unit: &str,
        run: &mut dyn FnMut(&Action) -> Result<(), String>,
    ) -> Result<(), String> {
        if unit != "cc-telegram.service" || self.runtime.iter().any(|e| e.unit == unit) {
            return Err("unsupported/duplicate runtime retirement".into());
        }
        let before = query_unit(home, unit, true, run)?;
        self.runtime.push(RuntimeEffect {
            home: home.into(),
            unit: unit.into(),
            before,
            activation: false,
            compensating: false,
        });
        // disable removes this owned enablement link before archive selection.
        let wants = home.join(".config/systemd/user/default.target.wants/cc-telegram.service");
        if matches!(self.entries.get(&wants), Some(Before::Link(_))) {
            self.observed.insert(wants, Before::Absent);
        }
        self.persist()
    }
    pub(crate) fn prepare_recompute(&mut self, action: &Action) -> Result<(), String> {
        let Some(home) = self.install_home() else {
            if self.durable.is_none() {
                return Ok(());
            }
            return Err("runtime recomputation HOME absent".into());
        };
        let effect = match action {
            Action::Systemctl { home: owned, args } if args == &["--user", "daemon-reload"] => {
                RecomputeEffect::Reload(owned.clone())
            }
            Action::InstallFonts(path) => RecomputeEffect::Fonts {
                home: home.into(),
                path: path.clone(),
            },
            _ => return Ok(()),
        };
        if !effect.valid(home) {
            return Err("runtime recomputation outside installation".into());
        }
        if !self.recompute.contains(&effect) {
            self.recompute.push(effect);
            self.persist()?;
        }
        Ok(())
    }
    pub(crate) fn prepare_unit_activation(
        &mut self,
        home: &Path,
        unit: &str,
        run: &mut dyn FnMut(&Action) -> Result<(), String>,
    ) -> Result<(), String> {
        if unit != "comandos-extensions-sync.timer" || self.runtime.iter().any(|e| e.unit == unit) {
            return Err("unsupported/duplicate runtime activation".into());
        }
        let before = query_unit(home, unit, false, run)?;
        let wants = home
            .join(".config/systemd/user/timers.target.wants")
            .join(unit);
        self.file(&wants)?;
        let target = home.join(".config/systemd/user").join(unit);
        match self.entries.get(&wants) {
            Some(Before::Absent) => {
                self.observed
                    .insert(wants.clone(), Before::Link(target.clone()));
            }
            Some(Before::Link(link))
                if *link == target || link == Path::new(&format!("../{unit}")) => {}
            _ => {
                return Err(format!(
                    "custom timer enablement preserved: {}",
                    wants.display()
                ));
            }
        }
        for (path, original) in &self.entries {
            if wants.starts_with(path)
                && path != &wants
                && *original == Before::Absent
                && path.symlink_metadata().is_err()
            {
                self.observed.insert(path.clone(), Before::Directory(0o755));
            }
        }
        self.runtime.push(RuntimeEffect {
            home: home.into(),
            unit: unit.into(),
            before,
            activation: true,
            compensating: false,
        });
        self.persist()
    }
    fn runtime_error(&self, unit: &str, error: &str) -> String {
        format!(
            "{unit}: {error}; retained recovery journal: {}",
            self.durable
                .as_ref()
                .map_or_else(|| "in-memory".into(), |p| p.display().to_string())
        )
    }
    pub(crate) fn rollback_with(
        mut self,
        run: &mut dyn FnMut(&Action) -> Result<(), String>,
    ) -> Result<(), String> {
        if let Some(error) = &self.reload_error {
            return Err(self.runtime_error("extension worker journal reload", error));
        }
        let _agent_lock = if self.agents {
            let home = self.install_home().ok_or("agent journal home absent")?;
            Some(
                crate::agents::installation_lock(home)
                    .map_err(|e| self.runtime_error("agent setup lock", &e.to_string()))?,
            )
        } else {
            None
        };
        let _extension_locks = if self.extensions {
            let home = self.install_home().ok_or("extension journal home absent")?;
            let state = comandos_extensions::config::state_dir(home);
            super::release::check_app_parents(&state.join("sync.lock"))?;
            for name in ["sync.lock", "credentials.lock"] {
                if state
                    .join(name)
                    .symlink_metadata()
                    .is_ok_and(|m| !m.is_file())
                {
                    return Err(self.runtime_error("extension lock", "control path retargeted"));
                }
            }
            Some((
                comandos_extensions::config::SyncLock::new(home)
                    .map_err(|e| self.runtime_error("extension sync", &e))?,
                comandos_extensions::auth::credential_lock(home)
                    .map_err(|e| self.runtime_error("extension credentials", &e))?,
            ))
        } else {
            None
        };
        for effect in &self.runtime {
            let current = query_unit(&effect.home, &effect.unit, false, run)
                .map_err(|error| self.runtime_error(&effect.unit, &error))?;
            // disable --now can fail after either half. Admit only the original
            // bits or their retired false values, never another enablement mode.
            if !effect.admitted(&current) {
                return Err(self.runtime_error(
                    &effect.unit,
                    "runtime state changed outside retirement; preserved",
                ));
            }
        }
        // Stop newly activated timers before restoring their unit files. Persist
        // inverse intent so a crash between disable and re-enable is recoverable.
        for index in 0..self.runtime.len() {
            if !self.runtime[index].activation {
                continue;
            }
            self.runtime[index].compensating = true;
            self.persist()?;
            let effect = self.runtime[index].clone();
            let current = query_unit(&effect.home, &effect.unit, false, run)?;
            if current.active && !effect.before.active {
                run(&Action::Systemctl {
                    home: effect.home.clone(),
                    args: vec!["--user".into(), "stop".into(), effect.unit.clone()],
                })
                .map_err(|e| self.runtime_error(&effect.unit, &e))?;
            }
            if current.enabled
                && (!effect.before.enabled || current.runtime != effect.before.runtime)
            {
                run(&Action::Systemctl {
                    home: effect.home.clone(),
                    args: vec!["--user".into(), "disable".into(), effect.unit.clone()],
                })
                .map_err(|e| self.runtime_error(&effect.unit, &e))?;
            }
        }
        super::extension_mutations::rollback_documents(&self.documents)
            .map_err(|e| self.runtime_error("extension documents", &e))?;
        self.rollback_files()?;
        while let Some(effect) = self.recompute.last().cloned() {
            run(&effect.action())
                .map_err(|error| self.runtime_error("runtime recomputation", &error))?;
            self.recompute.pop();
            self.persist()?;
        }
        while let Some(effect) = self.runtime.last().cloned() {
            let compensation: Result<(), String> = (|| {
                let current = query_unit(&effect.home, &effect.unit, false, run)?;
                if current == effect.before {
                    return Ok(());
                }
                // This retirement only writes disabled/inactive. An unrelated
                // state is preserved, not guessed into a prior configuration.
                if !effect.admitted(&current) {
                    return Err("runtime state changed outside retirement; preserved".into());
                }
                let action = |args: Vec<String>| Action::Systemctl {
                    home: effect.home.clone(),
                    args,
                };
                if effect.before.enabled && !current.enabled {
                    let mut args = vec!["--user".into()];
                    if effect.before.runtime {
                        args.push("--runtime".into());
                    }
                    args.extend(["enable".into(), effect.unit.clone()]);
                    run(&action(args))?;
                }
                if effect.before.active && !current.active {
                    run(&action(vec![
                        "--user".into(),
                        "start".into(),
                        effect.unit.clone(),
                    ]))?;
                }
                if query_unit(&effect.home, &effect.unit, false, run)? != effect.before {
                    return Err("runtime compensation did not restore prior state".into());
                }
                Ok(())
            })();
            if let Err(error) = compensation {
                return Err(format!(
                    "{}: {error}; retained recovery journal: {}",
                    effect.unit,
                    self.durable
                        .as_ref()
                        .map_or_else(|| "in-memory".into(), |p| p.display().to_string())
                ));
            }
            self.runtime.pop();
            self.persist()?;
        }
        self.discard()
    }
}
pub(crate) fn finish_with_runtime<T>(
    mut journal: Journal,
    result: Result<T, String>,
    run: &mut dyn FnMut(&Action) -> Result<(), String>,
) -> Result<T, String> {
    match result {
        Ok(value) => {
            if let Err(error) = journal.commit() {
                return finish_with_runtime(journal, Err(error), run);
            }
            journal.discard()?;
            Ok(value)
        }
        Err(mut error) => {
            if let Err(rollback) = journal.rollback_with(run) {
                error.push_str(&format!("; installation rollback incomplete: {rollback}"));
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    use crate::install::plan::UnitState;
    fn home() -> PathBuf {
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).unwrap();
        let home =
            std::env::temp_dir().join(format!("runtime-journal-{:x}", u64::from_ne_bytes(id)));
        fs::create_dir(&home).unwrap();
        home
    }
    struct Service {
        state: UnitState,
        calls: Vec<Vec<String>>,
        fail_start: bool,
    }
    impl Service {
        fn run(&mut self, action: &Action) -> Result<(), String> {
            match action {
                Action::SystemctlState { unit, response, .. } => {
                    assert_eq!(unit, "cc-telegram.service");
                    *response.borrow_mut() = Some(self.state.clone());
                    Ok(())
                }
                Action::Systemctl { args, .. } => {
                    assert_eq!(args.last().unwrap(), "cc-telegram.service");
                    self.calls.push(args.clone());
                    if args.iter().any(|a| a == "enable") {
                        self.state.enabled = true;
                        self.state.runtime = args.iter().any(|a| a == "--runtime");
                    } else if args.iter().any(|a| a == "start") {
                        if self.fail_start {
                            return Err("private start failure".into());
                        }
                        self.state.active = true;
                    } else {
                        panic!("unexpected inverse: {args:?}");
                    }
                    Ok(())
                }
                _ => panic!("unexpected action"),
            }
        }
    }
    #[test]
    fn late_failure_restores_all_supported_prior_states_on_same_unit_only() {
        for (enabled, runtime) in [(false, false), (true, false), (true, true)] {
            for active in [false, true] {
                let home = home();
                let before = UnitState {
                    enabled,
                    runtime,
                    active,
                };
                let mut service = Service {
                    state: before.clone(),
                    calls: vec![],
                    fail_start: false,
                };
                let mut journal = Journal::default();
                journal.durable(&home).unwrap();
                journal
                    .prepare_unit_retirement(&home, "cc-telegram.service", &mut |a| service.run(a))
                    .unwrap();
                service.state = UnitState {
                    enabled: false,
                    runtime: false,
                    active: false,
                };
                let error =
                    finish_with_runtime::<()>(journal, Err("later phase".into()), &mut |a| {
                        service.run(a)
                    })
                    .unwrap_err();
                assert_eq!(error, "later phase");
                assert_eq!(service.state, before);
                assert_eq!(
                    service.calls.len(),
                    usize::from(enabled) + usize::from(active)
                );
                assert!(
                    fs::read_dir(home.join(".local/share/comandos/install-journals"))
                        .unwrap()
                        .next()
                        .is_none()
                );
                fs::remove_dir_all(home).unwrap();
            }
        }
    }
    #[test]
    fn durable_recovery_retries_partial_inverse_without_repeating_completed_action() {
        let home = home();
        let before = UnitState {
            enabled: true,
            runtime: false,
            active: true,
        };
        let mut service = Service {
            state: before.clone(),
            calls: vec![],
            fail_start: true,
        };
        let mut journal = Journal::default();
        journal.durable(&home).unwrap();
        journal
            .prepare_unit_retirement(&home, "cc-telegram.service", &mut |a| service.run(a))
            .unwrap();
        let path = journal.durable.clone().unwrap();
        service.state = UnitState {
            enabled: false,
            runtime: false,
            active: false,
        };
        drop(journal); // durable reload, with no live runtime process or service
        let error = recover_with(&home, &path, &mut |a| service.run(a)).unwrap_err();
        assert!(error.contains("retained recovery journal"));
        assert!(path.is_dir());
        assert!(service.state.enabled && !service.state.active);
        service.fail_start = false;
        recover_with(&home, &path, &mut |a| service.run(a)).unwrap();
        assert_eq!(service.state, before);
        assert!(!path.exists());
        assert_eq!(
            service
                .calls
                .iter()
                .filter(|args| args.iter().any(|a| a == "enable"))
                .count(),
            1
        );
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn unrelated_runtime_enablement_is_preserved_and_journal_retained() {
        let home = home();
        let mut service = Service {
            state: UnitState {
                enabled: false,
                runtime: false,
                active: false,
            },
            calls: vec![],
            fail_start: false,
        };
        let mut journal = Journal::default();
        journal.durable(&home).unwrap();
        journal
            .prepare_unit_retirement(&home, "cc-telegram.service", &mut |a| service.run(a))
            .unwrap();
        let path = journal.durable.clone().unwrap();
        service.state.enabled = true;
        let error =
            finish_with_runtime::<()>(journal, Err("late failure".into()), &mut |a| service.run(a))
                .unwrap_err();
        assert!(error.contains("runtime state changed outside retirement"));
        assert!(path.exists());
        assert!(service.calls.is_empty());
        assert!(service.state.enabled);
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn missing_query_capability_prevents_retirement_intent_and_commands() {
        let home = home();
        let mut journal = Journal::default();
        journal.durable(&home).unwrap();
        let mut calls = 0;
        let error = journal
            .prepare_unit_retirement(&home, "cc-telegram.service", &mut |action| {
                assert!(matches!(action, Action::SystemctlState { .. }));
                calls += 1;
                Ok(())
            })
            .unwrap_err();
        assert!(error.contains("query capability"));
        assert_eq!(calls, 1);
        assert!(journal.runtime.is_empty());
        journal.rollback().unwrap();
        fs::remove_dir_all(home).unwrap();
    }
}

#[cfg(test)]
mod timer_tests {
    use super::*;
    use crate::install::plan::UnitState;
    #[test]
    fn activation_failure_and_durable_recovery_restore_all_prior_timer_states() {
        for (enabled, runtime) in [(false, false), (true, false), (true, true)] {
            for active in [false, true] {
                let home = std::env::temp_dir().join(format!(
                    "timer-journal-{}-{enabled}-{runtime}-{active}",
                    std::process::id()
                ));
                fs::create_dir(&home).unwrap();
                let before = UnitState {
                    enabled,
                    runtime,
                    active,
                };
                let mut state = before.clone();
                let mut calls = Vec::new();
                let mut run = |action: &Action| -> Result<(), String> {
                    match action {
                        Action::SystemctlState { unit, response, .. } => {
                            assert_eq!(unit, "comandos-extensions-sync.timer");
                            *response.borrow_mut() = Some(state.clone());
                        }
                        Action::Systemctl { args, .. } => {
                            assert_eq!(args.last().unwrap(), "comandos-extensions-sync.timer");
                            calls.push(args.clone());
                            if args.iter().any(|a| a == "disable") {
                                state.enabled = false;
                                state.runtime = false;
                            }
                            if args.iter().any(|a| a == "enable") {
                                state.enabled = true;
                                state.runtime = args.iter().any(|a| a == "--runtime");
                            }
                            if args.iter().any(|a| a == "stop") {
                                state.active = false;
                            }
                            if args.iter().any(|a| a == "start")
                                || args.iter().any(|a| a == "--now")
                            {
                                state.active = true;
                            }
                        }
                        _ => panic!("unexpected timer action"),
                    }
                    Ok(())
                };
                let mut journal = Journal::default();
                journal.durable(&home).unwrap();
                journal
                    .prepare_unit_activation(&home, "comandos-extensions-sync.timer", &mut run)
                    .unwrap();
                let path = journal.durable.clone().unwrap();
                run(&Action::Systemctl {
                    home: home.clone(),
                    args: vec![
                        "--user".into(),
                        "enable".into(),
                        "--now".into(),
                        "comandos-extensions-sync.timer".into(),
                    ],
                })
                .unwrap();
                drop(journal);
                recover_with(&home, &path, &mut run).unwrap();
                assert_eq!(state, before);
                assert!(!path.exists());
                assert!(!calls.is_empty());
                fs::remove_dir_all(home).unwrap();
            }
        }
    }
}

#[cfg(test)]
mod recompute_tests {
    use super::*;
    fn home() -> PathBuf {
        let mut id = [0; 8];
        getrandom::fill(&mut id).unwrap();
        let home =
            std::env::temp_dir().join(format!("install-recompute-{:x}", u64::from_ne_bytes(id)));
        fs::create_dir(&home).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        home
    }
    #[test]
    fn failed_refresh_is_retried_after_durable_reload_and_restored_file_bytes() {
        let home = home();
        let fonts = home.join(".local/share/fonts/comandos");
        fs::create_dir_all(&fonts).unwrap();
        let font = fonts.join("private-font");
        fs::write(&font, b"original font").unwrap();
        fs::set_permissions(&font, fs::Permissions::from_mode(0o600)).unwrap();
        let mut journal = Journal::default();
        journal.file(&font).unwrap();
        journal.durable(&home).unwrap();
        journal.expect_file(&font, b"new font", 0o600).unwrap();
        fs::write(&font, b"new font").unwrap();
        journal
            .prepare_recompute(&Action::InstallFonts(fonts.clone()))
            .unwrap();
        journal
            .prepare_recompute(&Action::Systemctl {
                home: home.clone(),
                args: vec!["--user".into(), "daemon-reload".into()],
            })
            .unwrap();
        let durable = journal.durable_path().unwrap().to_path_buf();
        let mut fail = true;
        let mut calls = Vec::new();
        let mut runner = |a: &Action| -> Result<(), String> {
            assert_eq!(fs::read(&font).unwrap(), b"original font");
            calls.push(a.clone());
            if matches!(a, Action::InstallFonts(_)) && fail {
                fail = false;
                return Err("private font refresh failure".into());
            }
            Ok(())
        };
        let error =
            finish_with_runtime::<()>(journal, Err("later phase".into()), &mut runner).unwrap_err();
        assert!(error.contains("retained recovery journal"));
        assert!(durable.is_dir());
        recover_with(&home, &durable, &mut runner).unwrap();
        assert!(!durable.exists());
        assert_eq!(
            calls
                .iter()
                .filter(|a| matches!(a, Action::Systemctl { .. }))
                .count(),
            1
        );
        assert_eq!(
            calls
                .iter()
                .filter(|a| matches!(a, Action::InstallFonts(_)))
                .count(),
            2
        );
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn ordinary_file_failure_runs_no_unactivated_runtime_refresh() {
        let home = home();
        let mut journal = Journal::default();
        journal.durable(&home).unwrap();
        let mut calls = 0;
        finish_with_runtime::<()>(journal, Err("private failure".into()), &mut |_| {
            calls += 1;
            Ok(())
        })
        .unwrap_err();
        assert_eq!(calls, 0);
        fs::remove_dir_all(home).unwrap();
    }
}
