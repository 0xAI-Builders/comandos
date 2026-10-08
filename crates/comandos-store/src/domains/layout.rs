//! Layout authority and snapshot generation writes under the shared mode lock.
use super::{lock_path, mirror};
use crate::{
    Error, Result,
    files::FileLock,
    snapshot_files::{self, Backend, Files, Policy},
    unified::{self, Mode},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
};

pub struct LayoutSnapshot<'a> {
    pub home: &'a Path,
    pub file: PathBuf,
}
fn generation(db: &Connection, kind: &str) -> Result<Option<Vec<u8>>> {
    Ok(db
        .query_row(
            "SELECT body FROM layout_snapshots WHERE generation=?1 ORDER BY stamp DESC LIMIT 1",
            [kind],
            |r| r.get(0),
        )
        .optional()?)
}
impl LayoutSnapshot<'_> {
    pub fn read_readonly(&self) -> Result<Value> {
        unified::modes::with_readonly_access(self.home, "layout", |mode, db| {
            if matches!(mode, Mode::Unified | Mode::Sealed) {
                let db = db.ok_or_else(|| Error::Validation("estado único sin base".into()))?;
                for kind in ["current", "previous"] {
                    if let Some(bytes) = generation(db, kind)?
                        && let Some(value) = snapshot_files::valid(&bytes)
                    {
                        return Ok(value);
                    }
                }
                return Ok(serde_json::json!({"version":2,"sessions":{}}));
            }
            Ok(snapshot_files::read_with(&Files, &self.file))
        })
    }
    /// Check identity before creating controls, after locks, and at each publish
    /// boundary (including after acquiring the SQLite transaction).
    pub fn write_when(
        &self,
        value: &Value,
        now: i64,
        allowed: impl FnMut() -> bool,
    ) -> Result<bool> {
        self.write_with_backend_when(
            value,
            now,
            Policy::Bridge,
            &Files,
            || FileLock::exclusive(&lock_path(&self.file)).map_err(Error::from),
            allowed,
        )
    }
    /// Preserve the caller's legacy serializer policy, guard and shared flock.
    pub fn write_with_backend_when<B: Backend<Error = Error>, L>(
        &self,
        value: &Value,
        now: i64,
        policy: Policy,
        backend: &B,
        acquire_legacy_lock: impl FnOnce() -> Result<L>,
        mut allowed: impl FnMut() -> bool,
    ) -> Result<bool> {
        if !allowed() {
            return Ok(false);
        }
        let path = unified::unified_path(self.home);
        let existing =
            unified::modes::with_readonly_access(self.home, "layout", |_, db| Ok(db.is_some()))?;
        let db = if existing {
            Some(unified::open_existing(&path)?)
        } else {
            None
        };
        let (mode, _mode_lock) = unified::modes::access_mode(self.home, db.as_ref(), "layout")?;
        if db.is_none() && path.exists() {
            return Err(Error::Validation(
                "base apareció durante escritura de layout".into(),
            ));
        }
        let _file_lock = if mode == Mode::Sealed {
            None
        } else {
            Some(acquire_legacy_lock()?)
        };
        if !allowed() {
            return Ok(false);
        }
        let old = if matches!(mode, Mode::Unified | Mode::Sealed) {
            generation(
                db.as_ref()
                    .ok_or_else(|| Error::Validation("estado único sin base".into()))?,
                "current",
            )?
        } else {
            backend.read(&self.file)?
        };
        let plan =
            snapshot_files::plan(value, old.as_deref(), now, policy).map_err(Error::Validation)?;
        let allowed = RefCell::new(allowed);
        let stopped = Cell::new(false);
        let may_publish = || {
            if stopped.get() || !allowed.borrow_mut()() {
                stopped.set(true);
                false
            } else {
                true
            }
        };
        mirror::write(
            mode,
            db.as_ref(),
            || {
                if may_publish() {
                    snapshot_files::apply_with(backend, &self.file, &plan, policy)?;
                }
                Ok(())
            },
            |db, _| {
                if !may_publish() {
                    return Ok(());
                }
                crate::migrate::move_db::admit_write(db)?;
                if let (Some(previous), Some(minute)) = (&plan.previous, plan.minute) {
                    let minute = i64::try_from(minute).map_err(|_| {
                        Error::Validation("layout minute outside SQLite range".into())
                    })?;
                    db.execute(
                        "DELETE FROM layout_snapshots WHERE generation='previous'",
                        [],
                    )?;
                    db.execute(
                        "INSERT INTO layout_snapshots VALUES (?1,'previous',?2)",
                        params![now, previous],
                    )?;
                    let archived = db.execute(
                        "INSERT OR IGNORE INTO layout_snapshots VALUES (?1,'minute',?2)",
                        params![minute, previous],
                    )?;
                    if archived != 0 {
                        let cutoff = i64::try_from(plan.cutoff).unwrap_or(i64::MIN);
                        unified::layout_prune_minutes(db, cutoff)?;
                    }
                }
                db.execute(
                    "DELETE FROM layout_snapshots WHERE generation='current'",
                    [],
                )?;
                db.execute(
                    "INSERT INTO layout_snapshots VALUES (?1,'current',?2)",
                    params![now, plan.current],
                )?;
                Ok(())
            },
        )?;
        Ok(!stopped.get())
    }
}
