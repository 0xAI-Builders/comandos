//! Valida sin abrir SQLite sobre la fuente: immutable sólo sin WAL;
//! con WAL, copia privada estable que sí aplica todos los commits.
use super::open::validate;
use crate::{Error, Result};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};
const CHANGED: &str = "DB/WAL/journal cambió durante validación";
#[derive(Debug, PartialEq, Eq)]
struct Identity {
    dev: u64,
    ino: u64,
    len: u64,
    mode: u32,
    mtime: (i64, i64),
    ctime: (i64, i64),
}
impl Identity {
    fn of(m: &fs::Metadata) -> Self {
        Self {
            dev: m.dev(),
            ino: m.ino(),
            len: m.len(),
            mode: m.mode(),
            mtime: (m.mtime(), m.mtime_nsec()),
            ctime: (m.ctime(), m.ctime_nsec()),
        }
    }
}
fn identity(path: &Path) -> Result<Option<Identity>> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => Ok(Some(Identity::of(&m))),
        Ok(_) => Err(Error::Validation(format!(
            "{}: SQLite requiere archivo regular",
            path.display()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn suffix(path: &Path, tail: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(tail);
    name.into()
}
pub(super) struct StableSource {
    db: Option<Identity>,
    wal: Option<Identity>,
    journal: Option<Identity>,
    parent: Identity,
}
impl StableSource {
    fn capture(path: &Path) -> Result<Self> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        Ok(Self {
            db: identity(path)?,
            wal: identity(&suffix(path, "-wal"))?,
            journal: identity(&suffix(path, "-journal"))?,
            parent: Identity::of(&fs::metadata(parent)?),
        })
    }
    pub(super) fn check(&self, path: &Path) -> Result<()> {
        let current = Self::capture(path)?;
        // La creación de SHM por otro proceso puede cambiar mtime del padre;
        // sólo su identidad y permisos afectan a la aceptación del directorio.
        if self.db != current.db
            || self.wal != current.wal
            || self.journal != current.journal
            || self.parent.dev != current.parent.dev
            || self.parent.ino != current.parent.ino
            || self.parent.mode != current.parent.mode
        {
            return Err(Error::Validation(CHANGED.into()));
        }
        Ok(())
    }
}
struct Scratch(PathBuf);
impl Scratch {
    fn new_outside(path: &Path, home: Option<&Path>) -> Result<Self> {
        let home = home.map(fs::canonicalize).transpose()?;
        let parent = fs::canonicalize(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?;
        for root in [Path::new("/tmp"), Path::new("/var/tmp")] {
            let root = fs::canonicalize(root)?;
            if root.starts_with(&parent) || home.as_ref().is_some_and(|home| root.starts_with(home))
            {
                continue;
            }
            for _ in 0..8 {
                let mut random = [0u8; 16];
                getrandom::fill(&mut random).map_err(|e| Error::Validation(e.to_string()))?;
                let name = random
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                let dir = root.join(format!("comandos-open-{}-{name}", std::process::id()));
                match fs::DirBuilder::new().mode(0o700).create(&dir) {
                    Ok(()) => return Ok(Self(dir)),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Err(Error::Validation(
            "no hay scratch privado fuera del directorio fuente".into(),
        ))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn transfer(path: &Path, mut target: Option<&mut File>, expected: &Identity) -> Result<[u8; 32]> {
    let mut source = File::open(path)?;
    if Identity::of(&source.metadata()?) != *expected {
        return Err(Error::Validation(CHANGED.into()));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut size = 0u64;
    loop {
        let n = source.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        size += n as u64;
        if let Some(out) = target.as_mut() {
            out.write_all(&buffer[..n])?;
        }
    }
    if size != expected.len
        || Identity::of(&source.metadata()?) != *expected
        || identity(path)?.as_ref() != Some(expected)
    {
        return Err(Error::Validation(CHANGED.into()));
    }
    Ok(hash.finalize().into())
}
fn copy(path: &Path, target: &Path, expected: &Identity) -> Result<[u8; 32]> {
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(target)?;
    let hash = transfer(path, Some(&mut out), expected)?;
    // Snapshot privado efímero: write_all basta para la lectura inmediata;
    // no se publica ni requiere sobrevivir a un crash.
    Ok(hash)
}
fn immutable_uri(path: &Path) -> Result<String> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut uri = String::from("file:");
    for byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(byte) {
            uri.push(char::from(*byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri.push_str("?immutable=1");
    Ok(uri)
}
fn open_wal_snapshot(staged: &Path) -> Result<Connection> {
    let conn = Connection::open(staged)?;
    // La copia se elimina tras inspeccionarla. Checkpoint al cerrar reescribe
    // todas sus páginas WAL, aunque nunca se publicará esa base temporal.
    conn.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        true,
    )?;
    // Sólo la copia temporal: su recuperación/cierre no requiere durabilidad.
    conn.pragma_update(None, "synchronous", "OFF")?;
    conn.pragma_update(None, "query_only", true)?;
    Ok(conn)
}
fn read_snapshot<T>(
    path: &Path,
    home: Option<&Path>,
    private: bool,
    body: impl FnOnce(&Connection) -> Result<T>,
) -> Result<(StableSource, T)> {
    let source = StableSource::capture(path)?;
    let db = source
        .db
        .as_ref()
        .ok_or_else(|| Error::Validation("base ausente durante validación".into()))?;
    if private && (db.mode & 0o7777 != 0o600 || source.parent.mode & 0o7777 != 0o700) {
        return Err(Error::Validation(
            "SQLite sin privacidad 0600/0700 durante validación".into(),
        ));
    }
    if source.journal.as_ref().is_some_and(|j| j.len > 0) {
        return Err(Error::Validation(
            "journal activo: apertura requiere fuente estable".into(),
        ));
    }
    let value = if let Some(wal) = source.wal.as_ref().filter(|wal| wal.len != 0) {
        let scratch = Scratch::new_outside(path, home)?;
        let staged = scratch.0.join("db.sqlite3");
        let dbhash = copy(path, &staged, db)?;
        let walpath = suffix(path, "-wal");
        let walhash = copy(&walpath, &suffix(&staged, "-wal"), wal)?;
        source.check(path)?;
        let conn = open_wal_snapshot(&staged)?;
        let value = body(&conn)?;
        // Comprobar bytes además de identidad después de interpretar el WAL.
        if transfer(path, None, db)? != dbhash || transfer(&walpath, None, wal)? != walhash {
            return Err(Error::Validation(CHANGED.into()));
        }
        source.check(path)?;
        value
    } else {
        // Evita copiar una base grande en la apertura habitual sin WAL.
        // La URI se escapa byte a byte; '?' y '%' son nombres, no opciones.
        let conn = Connection::open_with_flags(
            immutable_uri(path)?,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;
        let value = body(&conn)?;
        source.check(path)?;
        value
    };
    Ok((source, value))
}
fn inspect(path: &Path) -> Result<StableSource> {
    let source = StableSource::capture(path)?;
    if source.wal.as_ref().is_some_and(|wal| wal.len != 0)
        && source
            .db
            .as_ref()
            .is_some_and(|db| db.mode & 0o7777 == 0o600)
        && source.parent.mode & 0o7777 == 0o700
        && source
            .journal
            .as_ref()
            .is_none_or(|journal| journal.len == 0)
        && let Some(stable) = inspect_schema(path, source)?
    {
        return Ok(stable);
    }
    read_snapshot(path, None, true, validate).map(|(source, _)| source)
}

/// SQLite remains the schema authority. Only its metadata pages are staged;
/// unrelated payloads do not determine admission latency. Unsupported layouts
/// discard this candidate and use the existing complete snapshot instead.
fn inspect_schema(path: &Path, source: StableSource) -> Result<Option<StableSource>> {
    let Some(db) = source.db.as_ref() else {
        return Ok(None);
    };
    let Some(wal) = source.wal.as_ref() else {
        return Ok(None);
    };
    let scratch = Scratch::new_outside(path, None)?;
    let staged = scratch.0.join("db.sqlite3");
    let walpath = suffix(path, "-wal");
    let staged_wal = suffix(&staged, "-wal");
    let walhash = copy(&walpath, &staged_wal, wal)?;
    let base = File::open(path)?;
    if Identity::of(&base.metadata()?) != *db {
        return Err(Error::Validation(CHANGED.into()));
    }
    let mut snapshot = match super::schema_snapshot::prepare(base, &staged, &staged_wal) {
        Ok(snapshot) => snapshot,
        Err(_) => return Ok(None),
    };
    source.check(path)?;
    // The first tree contains sqlite_schema. Let SQLite locate version and
    // planner tables, including covering indices on the migration table.
    let roots = (|| -> Result<Vec<u32>> {
        let conn = open_wal_snapshot(&staged)?;
        Ok(conn.prepare("SELECT rootpage FROM sqlite_schema WHERE rootpage>0 AND (tbl_name='schema_migrations' OR name IN ('sqlite_stat1','sqlite_stat2','sqlite_stat3','sqlite_stat4'))")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    })();
    let roots = match roots {
        Ok(roots) => roots,
        Err(_) => return Ok(None),
    };
    for root in roots {
        if snapshot.copy_tree(root).is_err() {
            return Ok(None);
        }
    }
    if snapshot.finish().is_err() {
        source.check(path)?;
        return Ok(None);
    }
    source.check(path)?;
    let conn = open_wal_snapshot(&staged)?;
    let result = validate(&conn);
    if transfer(&walpath, None, wal)? != walhash {
        return Err(Error::Validation(CHANGED.into()));
    }
    source.check(path)?;
    match result {
        Ok(_) => Ok(Some(source)),
        // A planner or extension may require pages outside these trees. The
        // complete snapshot determines its result instead of accepting holes.
        Err(Error::Sql(_)) => Ok(None),
        Err(error) => Err(error),
    }
}
/// Snapshot de lectura sin crear DB, SHM ni candados en la fuente/HOME.
/// Los callbacks sólo consultan SQLite; las escrituras están prohibidas.
pub fn with_readonly_db<T>(
    home: &Path,
    path: &Path,
    body: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    if !home.is_absolute() || !path.is_absolute() {
        return Err(Error::Validation(
            "snapshot requiere rutas absolutas".into(),
        ));
    }
    read_snapshot(path, Some(home), false, body).map(|(_, value)| value)
}
/// Aplica también privacidad, versión completa y guardias de la base única.
pub fn with_readonly_unified<T>(
    home: &Path,
    path: &Path,
    body: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    if !home.is_absolute() || !path.is_absolute() {
        return Err(Error::Validation(
            "snapshot requiere rutas absolutas".into(),
        ));
    }
    read_snapshot(path, Some(home), true, |conn| {
        validate(conn)?;
        for domain in crate::domains::catalog::DOMAINS {
            if super::modes::guarded(path, domain.name)?
                != (super::mode_of(Some(conn), domain.name)? == super::Mode::Sealed)
            {
                return Err(Error::Validation(
                    "guardia y modo sin confirmar en snapshot".into(),
                ));
            }
        }
        body(conn)
    })
    .map(|(_, value)| value)
}
// La identidad puede cambiar por escritores SQLite que no toman nuestro
// flock. Reintentar su snapshot, sin abrir la fuente, respeta esa concurrencia.
pub(super) fn accept(path: &Path) -> Result<Connection> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let attempt = (|| {
            let stable = inspect(path)?;
            let conn = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            conn.set_db_config(
                rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
                true,
            )?;
            stable.check(path)?;
            Ok(conn)
        })();
        let unstable = matches!(&attempt, Err(Error::Validation(detail)) if detail == CHANGED)
            || matches!(&attempt, Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound);
        if !unstable || std::time::Instant::now() >= deadline {
            return attempt;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_wal_reader_does_not_checkpoint_staged_database() {
        let scratch = Scratch::new_outside(Path::new("/tmp/nonexistent-db"), None).unwrap();
        let source = scratch.0.join("source/db.sqlite3");
        let writer = crate::unified::open_unified(&source).unwrap();
        writer.execute_batch("CREATE TABLE snapshot_value(value TEXT); INSERT INTO snapshot_value VALUES('from WAL')").unwrap();
        let stamp = StableSource::capture(&source).unwrap();
        let staged = scratch.0.join("staged.sqlite3");
        copy(&source, &staged, stamp.db.as_ref().unwrap()).unwrap();
        let staged_wal = suffix(&staged, "-wal");
        copy(
            &suffix(&source, "-wal"),
            &staged_wal,
            stamp.wal.as_ref().unwrap(),
        )
        .unwrap();
        let before_db = fs::read(&staged).unwrap();
        let before_wal = fs::read(&staged_wal).unwrap();
        let reader = open_wal_snapshot(&staged).unwrap();
        assert_eq!(
            reader
                .query_row("SELECT value FROM snapshot_value", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "from WAL"
        );
        assert!(
            reader
                .execute("INSERT INTO snapshot_value VALUES('forbidden')", [])
                .is_err()
        );
        drop(reader);
        assert!(
            fs::read(&staged).unwrap() == before_db,
            "snapshot close must not copy WAL pages into its disposable DB"
        );
        assert!(
            fs::read(&staged_wal).unwrap() == before_wal,
            "snapshot close must not remove or reset its disposable WAL"
        );
        stamp.check(&source).unwrap();
        drop(writer);
    }
    #[test]
    fn stable_source_rejects_replacement_and_new_wal() {
        let scratch = Scratch::new_outside(Path::new("/tmp/nonexistent-db"), None).unwrap();
        let path = scratch.0.join("db");
        fs::write(&path, b"source").unwrap();
        let stamp = StableSource::capture(&path).unwrap();
        fs::write(suffix(&path, "-wal"), b"wal").unwrap();
        assert!(stamp.check(&path).is_err());
        fs::remove_file(suffix(&path, "-wal")).unwrap();
        let replacement = scratch.0.join("other");
        fs::write(&replacement, b"source").unwrap();
        fs::rename(replacement, &path).unwrap();
        assert!(stamp.check(&path).is_err());
    }
}
