//! Reuse admitted live SQLite connections; every caller still acquires and
//! validates its own domain lease. Inspection snapshots never use this pool.
use crate::{Error, Result, unified};
use rusqlite::Connection;
use std::{
    cell::RefCell,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

#[derive(PartialEq, Eq)]
struct Identity(u64, u64, u32);

fn identity(path: &Path) -> Result<Option<Identity>> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() || meta.is_dir() => {
            Ok(Some(Identity(meta.dev(), meta.ino(), meta.mode())))
        }
        Ok(_) => Err(Error::Validation(
            "lector SQLite requiere archivos regulares".into(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[derive(PartialEq, Eq)]
struct Source {
    database: Option<Identity>,
    parent: Option<Identity>,
    wal: Option<Identity>,
    shm: Option<Identity>,
    journal: Option<Identity>,
}

impl Source {
    fn capture(path: &Path) -> Result<Self> {
        let sibling = |suffix: &str| {
            let mut name = path.as_os_str().to_owned();
            name.push(suffix);
            identity(Path::new(&name))
        };
        Ok(Self {
            database: identity(path)?,
            parent: identity(path.parent().unwrap_or(Path::new(".")))?,
            wal: sibling("-wal")?,
            shm: sibling("-shm")?,
            journal: sibling("-journal")?,
        })
    }
}

pub(super) struct Token {
    path: PathBuf,
    source: Source,
}

struct Idle {
    token: Token,
    db: Connection,
}

thread_local! {
    static READERS: RefCell<Vec<Idle>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn open(path: &Path) -> Result<(Connection, Token)> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let before = Source::capture(&path)?;
    let idle = READERS.with(|readers| {
        let mut readers = readers.borrow_mut();
        readers
            .iter()
            .position(|idle| idle.token.path == path)
            .map(|index| readers.swap_remove(index))
    });
    if let Some(idle) = idle
        && idle.token.source == before
    {
        // Versions and migrations may change in the WAL without replacing a
        // file. Validate them on the live connection on every admission.
        unified::validate_caller(&idle.db)?;
        return Ok((idle.db, idle.token));
    }
    let db = unified::open_caller(&path)?;
    let source = Source::capture(&path)?;
    if source.database != before.database || source.parent != before.parent {
        return Err(Error::Validation(
            "base cambió durante admisión del lector".into(),
        ));
    }
    Ok((db, Token { path, source }))
}

pub(super) fn release(db: Connection, token: Token) {
    // A callback which left a transaction open must not lend its snapshot to
    // the next caller. Eviction/teardown retains NO_CKPT_ON_CLOSE from admission.
    if !db.is_autocommit() {
        return;
    }
    let _ = READERS.try_with(|readers| {
        let mut readers = readers.borrow_mut();
        if readers.len() == 4 {
            readers.remove(0);
        }
        readers.push(Idle { token, db });
    });
}
