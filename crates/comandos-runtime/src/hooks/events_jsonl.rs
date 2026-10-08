//! `~/.claude/hooks/events.jsonl`: append y recorte bajo el mismo `flock` que el
//! bash (`events.jsonl.lock`, espera de 3 s y, si vence, se escribe igual). El
//! recorte deja las últimas 500 líneas cuando el archivo pasa de 2000.
use super::state_file::mktemp;
use super::text::tail_lines;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

const MAX_LINES: usize = 2000;
const KEEP_LINES: usize = 500;

/// `events_append`: el `flock` es real (`flock(2)` vía `File::try_lock`), así que
/// excluye también a los hooks bash que sigan corriendo.
pub fn append(events: &Path, line: &str) {
    let mut lock_path = events.as_os_str().to_owned();
    lock_path.push(".lock");
    let lock = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&lock_path)
        .ok();
    if let Some(lock) = &lock {
        let deadline = Instant::now() + Duration::from_secs(3);
        while lock.try_lock().is_err() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    append_locked(events, line);
    // El candado se suelta al cerrar el descriptor.
    drop(lock);
}

/// La misma retención se aplica a las filas. El flock abarca ambos formatos.
pub fn append_domain(home: &Path, events: &Path, line: &str) {
    use comandos_store::{
        domains::caller::CallerAccess,
        unified::{self, LogName, Mode},
    };
    let result = (|| -> comandos_store::Result<()> {
        let access = CallerAccess::open(home, "logs")?;
        if access.mode() == Mode::Legacy {
            append(events, line);
            return Ok(());
        }
        let mut lock_path = events.as_os_str().to_owned();
        lock_path.push(".lock");
        let lock = if access.mode() == Mode::Sealed {
            None
        } else {
            let file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(&lock_path)?;
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                match file.try_lock() {
                    Ok(()) => break,
                    Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(fs::TryLockError::Error(e)) => return Err(e.into()),
                    _ => return Err(comandos_store::Error::ModeBusy),
                }
            }
            Some(file)
        };
        access.write(||{append_locked(events,line);Ok(())},|db,_| {
            unified::log_append(db,LogName::Events,line.as_bytes())?;
            let count:i64=db.query_row("SELECT count(*) FROM log_lines WHERE log='events'",[],|r|r.get(0))?;
            if count>MAX_LINES as i64 {db.execute("DELETE FROM log_lines WHERE log='events' AND seq NOT IN (SELECT seq FROM log_lines WHERE log='events' ORDER BY seq DESC LIMIT ?1)",[KEEP_LINES as i64])?;}
            Ok(())
        })?;
        drop(lock);
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("comandos hook: {error}");
    }
}

fn append_locked(events: &Path, line: &str) {
    if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(events) {
        let mut record = line.as_bytes().to_vec();
        record.push(b'\n');
        let _ = file.write_all(&record);
    }
    let Ok(content) = fs::read(events) else {
        return;
    };
    if content.iter().filter(|&&b| b == b'\n').count() <= MAX_LINES {
        return;
    }
    let (Some(dir), Some(name)) = (events.parent(), events.file_name()) else {
        return;
    };
    let mut prefix = name.as_encoded_bytes().to_vec();
    prefix.push(b'.');
    let Some((tmp, mut file)) = mktemp(dir, &prefix, 6, "") else {
        return;
    };
    if file.write_all(tail_lines(&content, KEEP_LINES)).is_ok() {
        drop(file);
        if fs::rename(&tmp, events).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    } else {
        let _ = fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn trims_to_last_500_lines_past_2000() {
        let dir = std::env::temp_dir().join(format!("comandos-events-trim-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let events = dir.join("events.jsonl");
        let lines: String = (0..2000).map(|n| format!("{n}\n")).collect();
        fs::write(&events, lines).unwrap();
        append(&events, "nueva");
        let content = fs::read_to_string(&events).unwrap();
        assert_eq!(content.lines().count(), 500);
        assert_eq!(content.lines().next(), Some("1501"));
        assert_eq!(content.lines().last(), Some("nueva"));
        // Como `mktemp` + `mv`: el archivo recortado queda en 0600.
        assert_eq!(
            fs::metadata(&events).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(dir.join("events.jsonl.lock").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod domain_tests {
    use super::*;
    use comandos_store::unified::{self, LogName, Mode};
    use std::os::unix::fs::DirBuilderExt;
    #[test]
    fn jsonl_trim_rule_matches_legacy_writer_every_mode() {
        for (i, mode) in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed]
            .into_iter()
            .enumerate()
        {
            let home =
                std::env::temp_dir().join(format!("events-domain-{}-{i}", std::process::id()));
            let root = home.join(".claude/hooks");
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&root)
                .unwrap();
            let db = unified::open_unified(&unified::unified_path(&home)).unwrap();
            unified::set_mode(&db, "logs", mode, "test", 1).unwrap();
            let events = root.join("events.jsonl");
            let lines: String = (0..2000).map(|n| format!("{n}\n")).collect();
            if mode != Mode::Sealed {
                fs::write(&events, lines).unwrap();
            }
            let tx = db.unchecked_transaction().unwrap();
            for n in 0..2000 {
                unified::log_append(&tx, LogName::Events, n.to_string().as_bytes()).unwrap();
            }
            tx.commit().unwrap();
            append_domain(&home, &events, "nueva");
            let expected: Vec<Vec<u8>> = (1501..2000)
                .map(|n| n.to_string().into_bytes())
                .chain([b"nueva".to_vec()])
                .collect();
            if mode != Mode::Legacy {
                assert_eq!(
                    unified::log_tail(&db, LogName::Events, 3000).unwrap(),
                    expected
                );
            }
            assert_eq!(events.exists(), mode != Mode::Sealed);
            if mode != Mode::Sealed {
                assert_eq!(
                    fs::read_to_string(&events)
                        .unwrap()
                        .lines()
                        .map(|s| s.as_bytes().to_vec())
                        .collect::<Vec<_>>(),
                    expected
                );
            }
            fs::remove_dir_all(home).unwrap();
        }
    }
}
