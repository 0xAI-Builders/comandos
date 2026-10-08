//! Explicit extension IO intents and logical row preimages. No after-failure reads
//! are used to infer which bytes belong to the installation.
use super::transaction::{Before, Journal};
use comandos_extensions::mutations::{Mutation, Observer};
use comandos_store::{
    domains::caller::CallerAccess,
    unified::{self, Mode},
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    cell::RefCell,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    rc::Rc,
};
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Row {
    domain: String,
    body: Vec<u8>,
    revision: i64,
    updated: i64,
    origin: String,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct DocumentEffect {
    home: PathBuf,
    name: String,
    device: u64,
    inode: u64,
    mode: String,
    before: Option<Row>,
    #[serde(default)]
    previous: Option<Row>,
    #[serde(default)]
    pending: bool,
    written: Row,
}
fn row(db: &Connection, name: &str) -> Result<Option<Row>, String> {
    db.query_row(
        "SELECT domain,body,revision,updated_at_ms,origin FROM documents WHERE name=?1",
        [name],
        |r| {
            Ok(Row {
                domain: r.get(0)?,
                body: r.get(1)?,
                revision: r.get(2)?,
                updated: r.get(3)?,
                origin: r.get(4)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}
fn mode(db: &Connection) -> Result<String, String> {
    Ok(
        match unified::mode_of(Some(db), "extensions").map_err(|e| e.to_string())? {
            Mode::Legacy => "legacy",
            Mode::Mirror => "mirror",
            Mode::Unified => "unified",
            Mode::Sealed => "sealed",
        }
        .into(),
    )
}
fn valid_name(name: &str) -> bool {
    name.starts_with("state/extensions/")
        && !name.split('/').any(|p| matches!(p, "" | "." | ".."))
        && !name.contains('\0')
}
pub(super) fn validate_documents(home: &Path, effects: &[DocumentEffect]) -> Result<(), String> {
    let mut names = std::collections::BTreeSet::new();
    for effect in effects {
        if effect.home != home
            || !valid_name(&effect.name)
            || !names.insert(&effect.name)
            || effect.written.domain != "extensions"
            || effect
                .before
                .as_ref()
                .is_some_and(|r| r.domain != "extensions")
            || !matches!(effect.mode.as_str(), "mirror" | "unified" | "sealed")
            || [
                &Some(effect.written.clone()),
                &effect.before,
                &effect.previous,
            ]
            .into_iter()
            .flatten()
            .any(|r| {
                r.revision < 1
                    || !matches!(r.origin.as_str(), "import" | "mirror" | "unified")
                    || r.domain != "extensions"
            })
        {
            return Err("invalid extension document journal".into());
        }
    }
    Ok(())
}
pub(super) fn rollback_documents(effects: &[DocumentEffect]) -> Result<(), String> {
    for effect in effects.iter().rev() {
        let path = unified::unified_path(&effect.home);
        super::release::check_app_parents(&path)?;
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !meta.is_file() || (meta.dev(), meta.ino()) != (effect.device, effect.inode) {
            return Err(format!(
                "{} document DB identity changed; journal retained",
                path.display()
            ));
        }
        let access = CallerAccess::open(&effect.home, "extensions").map_err(|e| e.to_string())?;
        let db = access.db().ok_or("extension document database missing")?;
        if mode(db)? != effect.mode {
            return Err("extension document mode changed; journal retained".into());
        }
        let result:Result<(),String>=access.with_write_transaction(|| {
            let current=row(db,&effect.name)?;
            if current==effect.before { return Ok(()); }
            if current.as_ref()!=Some(&effect.written) { return Err(format!("{} document changed; preserved",effect.name)); }
            if let Some(before)=&effect.before {
                db.execute("UPDATE documents SET domain=?2,body=?3,revision=?4,updated_at_ms=?5,origin=?6 WHERE name=?1",params![effect.name,before.domain,before.body,before.revision,before.updated,before.origin]).map_err(|e|e.to_string())?;
            } else { db.execute("DELETE FROM documents WHERE name=?1",[&effect.name]).map_err(|e|e.to_string())?; }
            Ok(())
        }).map_err(|e|e.to_string())?;
        result?;
    }
    Ok(())
}
pub(crate) struct Adapter {
    pub(crate) journal: Rc<RefCell<Journal>>,
    pub(crate) agents: bool,
}
impl Adapter {
    pub(crate) fn agents(journal: Rc<RefCell<Journal>>) -> Self {
        Self {
            journal,
            agents: true,
        }
    }
}
impl Observer for Adapter {
    fn before(&mut self, mutation: &Mutation<'_>) -> Result<(), String> {
        let mut j = self.journal.borrow_mut();
        let paths = match mutation {
            Mutation::Move { source, target } => vec![*source, *target],
            Mutation::File { path, .. }
            | Mutation::AtomicFile { path, .. }
            | Mutation::Directory { path, .. }
            | Mutation::Link { path, .. }
            | Mutation::Remove { path }
            | Mutation::Control { path } => vec![*path],
        };
        for path in &paths {
            if !j.install_home().is_some_and(|home| path.starts_with(home))
                || path.components().any(|c| {
                    matches!(
                        c,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
            {
                return Err(format!(
                    "extension mutation outside admitted HOME: {}",
                    path.display()
                ));
            }
        }
        if self.agents {
            j.agents = true;
            j.agent_paths.extend(paths.iter().map(|p| p.to_path_buf()));
        } else {
            j.extensions = true;
        }
        match mutation {
            Mutation::Control { path } => {
                super::release::check_app_parents(path)?;
                if path.symlink_metadata().is_ok_and(|m| !m.is_file()) {
                    return Err(format!(
                        "extension control path must be regular: {}",
                        path.display()
                    ));
                }
            }
            Mutation::File { path, bytes, mode } | Mutation::AtomicFile { path, bytes, mode } => {
                j.file(path)?;
                verify(&j, path)?;
                if path.is_symlink() && !matches!(mutation, Mutation::AtomicFile { .. }) {
                    return Err(format!(
                        "extension file symlink requires explicit target: {}",
                        path.display()
                    ));
                }
                if let Some(previous) = j.observed.get(*path).cloned() {
                    j.pending.insert((*path).into(), previous);
                }
                j.expect_file(path, bytes, *mode)?;
            }
            Mutation::Directory { path, mode } => {
                j.file(path)?;
                verify(&j, path)?;
                if path.symlink_metadata().is_err() {
                    for parent in path
                        .ancestors()
                        .take_while(|p| j.entries.contains_key(*p))
                        .collect::<Vec<_>>()
                    {
                        if matches!(j.entries.get(parent), Some(Before::Absent))
                            && parent.symlink_metadata().is_err()
                        {
                            j.observed.insert(parent.into(), Before::Directory(*mode));
                        }
                    }
                } else if !path.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                    return Err(format!("extension directory changed: {}", path.display()));
                }
                j.persist()?;
            }
            Mutation::Link { path, target } => {
                j.file(path)?;
                verify(&j, path)?;
                if path.symlink_metadata().is_ok() {
                    return Err(format!("extension link occupied: {}", path.display()));
                }
                j.expect_link(path, target)?;
            }
            Mutation::Move { source, target } => {
                if matches!(j.entries.get(*source), Some(Before::Absent)) {
                    known_tree(&j, source)?;
                }
                j.tree(source)?;
                if matches!(j.entries.get(*source), Some(Before::Directory(_)))
                    && !j.extension_trees.iter().any(|root| root == source)
                {
                    j.extension_trees.push((*source).into());
                }
                j.overlay(source, target)?;
                if target.symlink_metadata().is_ok() {
                    verify(&j, target)?;
                    let mut actual = Journal::default();
                    actual.capture(source)?;
                    if !matches!((actual.entries.get(*source),j.observed.get(*target)),(Some(Before::File(a,am)),Some(Before::File(b,bm))) if a==b && am==bm)
                    {
                        return Err(format!("extension move occupied: {}", target.display()));
                    }
                }
                let paths = j
                    .entries
                    .keys()
                    .filter(|p| p.starts_with(source))
                    .cloned()
                    .collect::<Vec<_>>();
                for path in paths {
                    verify(&j, &path)?;
                    let mut actual = Journal::default();
                    actual.capture(&path)?;
                    let dest = target.join(path.strip_prefix(source).map_err(|e| e.to_string())?);
                    j.observed.insert(
                        dest,
                        actual
                            .entries
                            .remove(&path)
                            .ok_or("extension move source missing")?,
                    );
                    j.observed.insert(path, Before::Absent);
                }
                j.persist()?;
            }
            Mutation::Remove { path } => {
                // A cleanup can remove only explicitly admitted entries; an
                // unexpected child is a concurrent edit, not our scratch.
                fn known(j: &Journal, p: &Path) -> Result<(), String> {
                    if !j.entries.contains_key(p) {
                        return Err(format!("unadmitted extension cleanup: {}", p.display()));
                    }
                    if p.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                        for child in fs::read_dir(p).map_err(|e| e.to_string())? {
                            known(j, &child.map_err(|e| e.to_string())?.path())?;
                        }
                    }
                    Ok(())
                }
                if !j.entries.contains_key(*path)
                    && path.symlink_metadata().is_ok_and(|m| !m.is_dir())
                {
                    j.file(path)?;
                }
                known(&j, path)?;
                let paths = j
                    .entries
                    .keys()
                    .filter(|p| p.starts_with(path))
                    .cloned()
                    .collect::<Vec<_>>();
                for p in paths {
                    verify(&j, &p)?;
                    j.observed.insert(p, Before::Absent);
                }
                j.persist()?;
            }
        }
        Ok(())
    }
    fn after(&mut self, path: &Path) -> Result<(), String> {
        let mut j = self.journal.borrow_mut();
        let paths = path
            .ancestors()
            .filter(|p| {
                j.entries.contains_key(*p)
                    && (**p == *path
                        || (matches!(j.entries.get(*p), Some(Before::Absent))
                            && matches!(j.observed.get(*p), Some(Before::Directory(_)))))
            })
            .map(Path::to_path_buf)
            .collect::<Vec<_>>();
        j.checkpoint(&paths)
    }
    fn document_committed(&mut self, name: &str) -> Result<(), String> {
        let mut j = self.journal.borrow_mut();
        if let Some(effect) = j.documents.iter_mut().find(|e| e.name == name) {
            effect.previous = None;
            effect.pending = false;
        }
        j.persist()
    }
    fn document(
        &mut self,
        home: &Path,
        db: &Connection,
        name: &str,
        bytes: &[u8],
        origin: unified::Origin,
        now: i64,
    ) -> Result<(), String> {
        if !valid_name(name) {
            return Err("unadmitted extension logical document".into());
        }
        let current = row(db, name)?;
        if current.as_ref().is_some_and(|r| r.domain != "extensions") {
            return Err("extension document domain mismatch".into());
        }
        if self.journal.borrow().install_home() != Some(home) {
            return Err("extension document HOME mismatch".into());
        }
        let path = unified::unified_path(home);
        if db.path() != path.to_str() {
            return Err("extension document DB path mismatch".into());
        }
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !meta.is_file() {
            return Err("extension document DB must be regular".into());
        }
        let written = Row {
            domain: "extensions".into(),
            body: bytes.into(),
            revision: current
                .as_ref()
                .map_or(Some(1), |r| r.revision.checked_add(1))
                .ok_or("document revision exhausted")?,
            updated: now,
            origin: origin.as_str().into(),
        };
        let mut j = self.journal.borrow_mut();
        if let Some(effect) = j.documents.iter_mut().find(|e| e.name == name) {
            if current.as_ref() != Some(&effect.written)
                && (!effect.pending || current != effect.previous)
            {
                return Err("extension document changed during installation".into());
            }
            if effect.home != home
                || (effect.device, effect.inode) != (meta.dev(), meta.ino())
                || effect.mode != mode(db)?
            {
                return Err(
                    "extension document DB identity/mode changed during installation".into(),
                );
            }
            effect.previous = current;
            effect.pending = true;
            effect.written = written;
        } else {
            j.documents.push(DocumentEffect {
                home: home.into(),
                name: name.into(),
                device: meta.dev(),
                inode: meta.ino(),
                mode: mode(db)?,
                before: current.clone(),
                previous: current,
                pending: true,
                written,
            });
        }
        j.persist()
    }
}
fn verify(j: &Journal, path: &Path) -> Result<(), String> {
    let mut actual = Journal::default();
    actual.capture(path)?;
    if actual.entries.get(path) != j.observed.get(path)
        && actual.entries.get(path) != j.entries.get(path)
        && actual.entries.get(path) != j.pending.get(path)
    {
        return Err(format!(
            "{} changed during extension transaction; preserved",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use comandos_extensions::{config, mutations, skills};
    use std::os::unix::fs::{PermissionsExt, symlink};
    struct Home(PathBuf);
    impl Home {
        fn new() -> Self {
            let mut id = [0u8; 8];
            getrandom::fill(&mut id).unwrap();
            let p = std::env::temp_dir().join(format!("extension-tx-{:x}", u64::from_ne_bytes(id)));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn setup(home: &Path) -> (Rc<RefCell<Journal>>, Rc<RefCell<Adapter>>) {
        let mut journal = Journal::default();
        journal.durable(home).unwrap();
        let owned = Rc::new(RefCell::new(journal));
        (
            owned.clone(),
            Rc::new(RefCell::new(Adapter {
                journal: owned,
                agents: false,
            })),
        )
    }
    fn journal(owned: Rc<RefCell<Journal>>) -> Journal {
        Rc::try_unwrap(owned).ok().unwrap().into_inner()
    }
    #[test]
    fn all_mode_logical_document_and_legacy_bytes_restore_on_late_failure() {
        for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
            let home = Home::new();
            let path = config::state_dir(&home.0).join("snapshot.json");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"legacy-original").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
            let db = unified::open_unified(&unified::unified_path(&home.0)).unwrap();
            unified::doc_put(
                &db,
                "state/extensions/snapshot.json",
                "extensions",
                b"row-original",
                unified::Origin::Import,
                123,
            )
            .unwrap();
            unified::set_mode(&db, "extensions", mode, "private", 1).unwrap();
            let before = row(&db, "state/extensions/snapshot.json").unwrap();
            drop(db);
            let (owned, adapter) = setup(&home.0);
            mutations::with(adapter, || config::private_write(&path, b"installed")).unwrap();
            let error =
                super::super::transaction::finish::<()>(journal(owned), Err("late failure".into()))
                    .unwrap_err();
            assert_eq!(error, "late failure");
            assert_eq!(fs::read(&path).unwrap(), b"legacy-original");
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
                0o640
            );
            let access = CallerAccess::open(&home.0, "extensions").unwrap();
            assert_eq!(
                row(access.db().unwrap(), "state/extensions/snapshot.json").unwrap(),
                before
            );
        }
    }
    #[test]
    fn logical_user_edit_preserves_row_and_retains_journal() {
        let home = Home::new();
        let db = unified::open_unified(&unified::unified_path(&home.0)).unwrap();
        unified::set_mode(&db, "extensions", Mode::Sealed, "private", 1).unwrap();
        drop(db);
        let (owned, adapter) = setup(&home.0);
        let path = config::state_dir(&home.0).join("skills.json");
        mutations::with(adapter, || config::private_write(&path, b"installed")).unwrap();
        let access = CallerAccess::open(&home.0, "extensions").unwrap();
        unified::doc_put(
            access.db().unwrap(),
            "state/extensions/skills.json",
            "extensions",
            b"user edit",
            unified::Origin::Unified,
            999,
        )
        .unwrap();
        drop(access);
        let error = super::super::transaction::finish::<()>(journal(owned), Err("failure".into()))
            .unwrap_err();
        assert!(error.contains("document changed; preserved"), "{error}");
        assert!(error.contains("retained recovery journal"));
        let access = CallerAccess::open(&home.0, "extensions").unwrap();
        assert_eq!(
            row(access.db().unwrap(), "state/extensions/skills.json")
                .unwrap()
                .unwrap()
                .body,
            b"user edit"
        );
    }
    #[test]
    fn skills_resources_links_and_new_backups_roll_back_through_explicit_moves() {
        let home = Home::new();
        let source = home.0.join(".claude/skills/demo");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("SKILL.md"), b"skill original").unwrap();
        fs::write(source.join("nested/resource"), b"resource").unwrap();
        fs::set_permissions(source.join("nested"), fs::Permissions::from_mode(0o700)).unwrap();
        let before_fingerprint = skills::fingerprint(&source).unwrap();
        let (owned, adapter) = setup(&home.0);
        mutations::with(adapter, || skills::sync_skills(&home.0).map(|_| ())).unwrap();
        assert!(source.is_symlink());
        let error =
            super::super::transaction::finish::<()>(journal(owned), Err("later failure".into()))
                .unwrap_err();
        assert_eq!(error, "later failure");
        assert!(!source.is_symlink());
        assert_eq!(
            fs::read(source.join("SKILL.md")).unwrap(),
            b"skill original"
        );
        assert_eq!(
            fs::read(source.join("nested/resource")).unwrap(),
            b"resource"
        );
        assert_eq!(
            fs::metadata(source.join("nested"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o700
        );
        assert!(!home.0.join(".agents/skills/demo").exists());
        assert_eq!(skills::fingerprint(&source).unwrap(), before_fingerprint);
    }
    #[test]
    fn file_retarget_and_after_write_user_edit_are_not_adopted() {
        let home = Home::new();
        let path = home.0.join("managed.json");
        fs::write(&path, b"original").unwrap();
        let (owned, adapter) = setup(&home.0);
        mutations::with(adapter, || config::private_write(&path, b"installed")).unwrap();
        fs::remove_file(&path).unwrap();
        symlink("user-target", &path).unwrap();
        let error =
            super::super::transaction::finish::<()>(journal(owned), Err("later failure".into()))
                .unwrap_err();
        assert!(error.contains("preserved"));
        assert_eq!(fs::read_link(path).unwrap(), Path::new("user-target"));
    }
}

fn known_tree(j: &Journal, path: &Path) -> Result<(), String> {
    if !j.entries.contains_key(path) {
        return Err(format!(
            "unadmitted child in extension scratch: {}",
            path.display()
        ));
    }
    if path.symlink_metadata().is_ok_and(|m| m.is_dir()) {
        for child in fs::read_dir(path).map_err(|e| e.to_string())? {
            known_tree(j, &child.map_err(|e| e.to_string())?.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use comandos_extensions::{config, mutations};
    fn home() -> PathBuf {
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).unwrap();
        let p =
            std::env::temp_dir().join(format!("extension-recover-{:x}", u64::from_ne_bytes(id)));
        fs::create_dir(&p).unwrap();
        p
    }
    #[test]
    fn completed_logical_write_does_not_restore_over_a_user_deletion() {
        let home = home();
        let db = unified::open_unified(&unified::unified_path(&home)).unwrap();
        unified::set_mode(&db, "extensions", Mode::Sealed, "private", 1).unwrap();
        unified::doc_put(
            &db,
            "state/extensions/snapshot.json",
            "extensions",
            b"original",
            unified::Origin::Import,
            7,
        )
        .unwrap();
        drop(db);
        let mut journal = Journal::default();
        journal.durable(&home).unwrap();
        let owned = Rc::new(RefCell::new(journal));
        mutations::with(
            Rc::new(RefCell::new(Adapter {
                journal: owned.clone(),
                agents: false,
            })),
            || {
                config::private_write(
                    &config::state_dir(&home).join("snapshot.json"),
                    b"installed",
                )
            },
        )
        .unwrap();
        let access = CallerAccess::open(&home, "extensions").unwrap();
        access
            .db()
            .unwrap()
            .execute(
                "DELETE FROM documents WHERE name='state/extensions/snapshot.json'",
                [],
            )
            .unwrap();
        drop(access);
        let error = super::super::transaction::finish::<()>(
            Rc::try_unwrap(owned).ok().unwrap().into_inner(),
            Err("later failure".into()),
        )
        .unwrap_err();
        assert!(error.contains("document changed; preserved"), "{error}");
        assert!(error.contains("retained recovery journal"));
        let access = CallerAccess::open(&home, "extensions").unwrap();
        assert!(
            row(access.db().unwrap(), "state/extensions/snapshot.json")
                .unwrap()
                .is_none()
        );
        drop(access);
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn durable_recovery_restores_explicit_previous_intent_without_after_failure_adoption() {
        let home = home();
        let path = home.join("file.json");
        fs::write(&path, b"original").unwrap();
        let mut journal = Journal::default();
        journal.durable(&home).unwrap();
        let owned = Rc::new(RefCell::new(journal));
        let adapter = Rc::new(RefCell::new(Adapter {
            journal: owned.clone(),
            agents: false,
        }));
        mutations::with(adapter.clone(), || {
            config::private_write(&path, b"first-owned-write")
        })
        .unwrap();
        adapter
            .borrow_mut()
            .before(&Mutation::File {
                path: &path,
                bytes: b"next-owned-intent",
                mode: 0o600,
            })
            .unwrap();
        let recovery = fs::read_dir(home.join(".local/share/comandos/install-journals"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        drop(adapter);
        drop(owned);
        super::super::transaction::recover(&home, &recovery).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert!(!recovery.exists());
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn committed_document_preimages_survive_durable_reload_and_restore_exact_row_metadata() {
        let home = home();
        let db = unified::open_unified(&unified::unified_path(&home)).unwrap();
        unified::set_mode(&db, "extensions", Mode::Sealed, "private", 1).unwrap();
        unified::doc_put(
            &db,
            "state/extensions/snapshot.json",
            "extensions",
            b"original",
            unified::Origin::Import,
            77,
        )
        .unwrap();
        let before = row(&db, "state/extensions/snapshot.json").unwrap();
        drop(db);
        let mut journal = Journal::default();
        journal.durable(&home).unwrap();
        let owned = Rc::new(RefCell::new(journal));
        mutations::with(
            Rc::new(RefCell::new(Adapter {
                journal: owned.clone(),
                agents: false,
            })),
            || {
                config::private_write(
                    &config::state_dir(&home).join("snapshot.json"),
                    b"installed",
                )
            },
        )
        .unwrap();
        let recovery = fs::read_dir(home.join(".local/share/comandos/install-journals"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        drop(owned);
        super::super::transaction::recover(&home, &recovery).unwrap();
        let access = CallerAccess::open(&home, "extensions").unwrap();
        assert_eq!(
            row(access.db().unwrap(), "state/extensions/snapshot.json").unwrap(),
            before
        );
        drop(access);
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn native_all_agent_setup_restores_configs_links_backups_and_target_modes() {
        use std::os::unix::fs::PermissionsExt;
        let home = (home(),);
        let files = [
            (
                ".codex/config.toml",
                b"# preserve\n[other]\nflag = 1\n".as_slice(),
            ),
            (".grok/hooks/comandos.json", b"{\"foreign\":1}\n".as_slice()),
            (".gemini/config/hooks.json", b"{}\n".as_slice()),
            (
                ".config/private-gemini.json",
                b"{\"foreign\":2}\n".as_slice(),
            ),
        ];
        for (name, body) in files {
            let path = home.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, body).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o624)).unwrap();
        }
        let gemini = home.0.join(".gemini/settings.json");
        std::os::unix::fs::symlink(home.0.join(".config/private-gemini.json"), &gemini).unwrap();
        let bin = home.0.join(".local/bin");
        fs::create_dir_all(&bin).unwrap();
        let alias = bin.join("grok-hooks.py");
        let old = home.0.join("legacy/adapters/grok-hooks.py");
        std::os::unix::fs::symlink(&old, &alias).unwrap();
        let mut journal = Journal::default();
        journal.durable(&home.0).unwrap();
        let owned = Rc::new(RefCell::new(journal));
        let mut out = String::new();
        comandos_extensions::mutations::with(
            Rc::new(RefCell::new(Adapter::agents(owned.clone()))),
            || {
                crate::agents::setup_transaction_with(&home.0, &mut out, &|_| true)
                    .map_err(|e| e.to_string())
            },
        )
        .unwrap();
        assert_ne!(
            fs::read(home.0.join(".config/private-gemini.json")).unwrap(),
            files[3].1
        );
        assert_ne!(fs::read_link(&alias).unwrap(), old);
        let mut journal = Rc::try_unwrap(owned).ok().unwrap().into_inner();
        journal.reload(&home.0).unwrap();
        journal.rollback().unwrap();
        for (name, body) in files {
            let path = home.0.join(name);
            assert_eq!(fs::read(&path).unwrap(), body);
            assert_eq!(
                path.metadata().unwrap().permissions().mode() & 0o7777,
                0o624
            );
        }
        assert_eq!(fs::read_link(alias).unwrap(), old);
        assert_eq!(
            fs::read_link(gemini).unwrap(),
            home.0.join(".config/private-gemini.json")
        );
        assert!(!home.0.join(".codex/hooks.json").exists());
        assert!(!home.0.join(".config/opencode/plugin/comandos.js").exists());
        assert!(
            !home
                .0
                .join(".local/share/comandos/opencode-bridge.mjs")
                .exists()
        );
        for parent in [".codex", ".grok/hooks", ".gemini", ".gemini/config"] {
            assert!(fs::read_dir(home.0.join(parent)).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".bak-comandos-")
            }));
        }
        assert!(!home.0.join(".config/comandos/extensions").exists());
        fs::remove_dir_all(home.0).unwrap();
    }
    #[test]
    fn later_malformed_agent_config_restores_earlier_native_writes() {
        let home = home();
        let codex = home.join(".codex/config.toml");
        let grok = home.join(".grok/hooks/comandos.json");
        for (path, bytes) in [
            (&codex, b"# original\n".as_slice()),
            (&grok, b"{invalid".as_slice()),
        ] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let mut j = Journal::default();
        j.durable(&home).unwrap();
        let owned = Rc::new(RefCell::new(j));
        let mut out = String::new();
        let error = mutations::with(
            Rc::new(RefCell::new(Adapter::agents(owned.clone()))),
            || {
                crate::agents::setup_transaction_with(&home, &mut out, &|name| {
                    matches!(name, "codex" | "grok")
                })
                .map_err(|e| e.to_string())
            },
        )
        .unwrap_err();
        assert!(error.contains("JSON"), "{error}");
        assert_ne!(fs::read(&codex).unwrap(), b"# original\n");
        let mut j = Rc::try_unwrap(owned).ok().unwrap().into_inner();
        j.reload(&home).unwrap();
        j.rollback().unwrap();
        assert_eq!(fs::read(&codex).unwrap(), b"# original\n");
        assert_eq!(fs::read(grok).unwrap(), b"{invalid");
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn followed_agent_config_outside_home_is_rejected_before_target_write() {
        let home = home();
        let outside = home.with_extension("outside");
        fs::write(&outside, b"{\"foreign\":true}\n").unwrap();
        let config = home.join(".gemini/settings.json");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, &config).unwrap();
        let mut j = Journal::default();
        j.durable(&home).unwrap();
        let owned = Rc::new(RefCell::new(j));
        let mut out = String::new();
        let error = mutations::with(
            Rc::new(RefCell::new(Adapter::agents(owned.clone()))),
            || {
                crate::agents::setup_transaction_with(&home, &mut out, &|name| name == "gemini")
                    .map_err(|e| e.to_string())
            },
        )
        .unwrap_err();
        assert!(error.contains("outside admitted HOME"), "{error}");
        let mut j = Rc::try_unwrap(owned).ok().unwrap().into_inner();
        j.reload(&home).unwrap();
        j.rollback().unwrap();
        assert_eq!(fs::read(&outside).unwrap(), b"{\"foreign\":true}\n");
        assert_eq!(fs::read_link(config).unwrap(), outside);
        fs::remove_file(outside).unwrap();
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn atomic_agent_shim_replacement_restores_link_without_mutating_old_target() {
        let home = home();
        let outside = home.with_extension("shim");
        fs::write(&outside, b"foreign shim\n").unwrap();
        let shim = home.join(".config/opencode/plugin/comandos.js");
        fs::create_dir_all(shim.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, &shim).unwrap();
        let mut j = Journal::default();
        j.durable(&home).unwrap();
        let owned = Rc::new(RefCell::new(j));
        let mut out = String::new();
        mutations::with(
            Rc::new(RefCell::new(Adapter::agents(owned.clone()))),
            || {
                crate::agents::setup_transaction_with(&home, &mut out, &|name| name == "opencode")
                    .map_err(|e| e.to_string())
            },
        )
        .unwrap();
        assert!(!shim.is_symlink());
        let mut j = Rc::try_unwrap(owned).ok().unwrap().into_inner();
        j.reload(&home).unwrap();
        j.rollback().unwrap();
        assert_eq!(fs::read_link(shim).unwrap(), outside);
        assert_eq!(fs::read(&outside).unwrap(), b"foreign shim\n");
        fs::remove_file(outside).unwrap();
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn agent_rollback_preserves_later_config_edit_and_retains_durable_journal() {
        let home = home();
        let config = home.join(".codex/config.toml");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, b"# original\n").unwrap();
        let mut j = Journal::default();
        j.durable(&home).unwrap();
        let owned = Rc::new(RefCell::new(j));
        let mut out = String::new();
        mutations::with(
            Rc::new(RefCell::new(Adapter::agents(owned.clone()))),
            || {
                crate::agents::setup_transaction_with(&home, &mut out, &|name| name == "codex")
                    .map_err(|e| e.to_string())
            },
        )
        .unwrap();
        let mut j = Rc::try_unwrap(owned).ok().unwrap().into_inner();
        let durable = j.durable_path().unwrap().to_path_buf();
        j.reload(&home).unwrap();
        fs::write(&config, b"# later user edit\n").unwrap();
        assert!(j.rollback().is_err());
        assert_eq!(fs::read(config).unwrap(), b"# later user edit\n");
        assert!(durable.exists());
        fs::remove_dir_all(home).unwrap();
    }
}
