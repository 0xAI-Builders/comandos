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
