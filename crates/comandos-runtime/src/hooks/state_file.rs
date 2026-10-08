//! `~/.claude/hooks/state/<clave>.json`: escritura atómica (temporal en el mismo
//! directorio + `rename`, como `mktemp` + `mv` del bash) y las dos lecturas jq que
//! el hook hace del estado anterior.
use super::jq::{J, jq_join, jq_pretty, jq_r, jq_tostring};
use super::text::jq_lossy;
use super::transcript::{alt, field};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Los campos que `write_state` pasa a jq.
pub struct State<'a> {
    pub project: &'a [u8],
    pub status: &'a str,
    pub detail: &'a [u8],
    pub cwd: &'a [u8],
    pub ts: i64,
    pub options: &'a [u8],
    pub last: &'a [u8],
    pub agent: &'a [u8],
    pub session: &'a [u8],
    pub pane: &'a str,
}

/// El JSON exacto que deja `jq -n` (indentado, `\n` final).
pub fn render(state: &State) -> String {
    let texts = [
        state.project,
        state.detail,
        state.cwd,
        state.options,
        state.last,
        state.agent,
        state.session,
    ]
    .map(jq_lossy);
    let mut fields = vec![
        ("project", J::S(&texts[0])),
        ("status", J::S(state.status)),
        ("detail", J::S(&texts[1])),
        ("cwd", J::S(&texts[2])),
        ("ts", J::I(state.ts)),
        ("options", J::S(&texts[3])),
        ("last", J::S(&texts[4])),
        ("agent", J::S(&texts[5])),
        ("session", J::S(&texts[6])),
    ];
    if !state.pane.is_empty() {
        fields.push(("pane", J::S(state.pane)));
    }
    jq_pretty(&fields)
}

/// `mktemp DIR/PREFIJO` + `width` caracteres aleatorios + sufijo: archivo nuevo con modo 0600.
pub fn mktemp(
    dir: &Path,
    prefix: &[u8],
    width: usize,
    suffix: &str,
) -> Option<(PathBuf, fs::File)> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    for _ in 0..100 {
        let mut random = [0u8; 10];
        getrandom::fill(&mut random).ok()?;
        let mut name = prefix.to_vec();
        name.extend(
            random[..width]
                .iter()
                .map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()]),
        );
        name.extend_from_slice(suffix.as_bytes());
        let path = dir.join(std::ffi::OsStr::from_bytes(&name));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => return Some((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// Escribe el estado; `false` si no se pudo crear el temporal (el bash entonces
/// abandona `write_state` entero: ni timeline ni contabilidad de uso).
pub fn write(dir: &Path, key: &[u8], target: &Path, state: &State) -> bool {
    let mut prefix = b".".to_vec();
    prefix.extend_from_slice(key);
    prefix.push(b'.');
    let Some((tmp, mut file)) = mktemp(dir, &prefix, 6, "") else {
        return false;
    };
    let written = file.write_all(render(state).as_bytes()).is_ok();
    drop(file);
    if written {
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644));
        if fs::rename(&tmp, target).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    } else {
        let _ = fs::remove_file(&tmp);
    }
    true
}

/// Entrada por dominio; el escritor legado conserva su temporal, 0644 y bytes jq.
pub fn write_domain(home: &Path, dir: &Path, key: &[u8], target: &Path, state: &State) -> bool {
    let Some(name) = target.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let body = render(state);
    let mut lock = target.as_os_str().to_owned();
    lock.push(".lock");
    let result = comandos_store::domains::caller::write(
        home,
        "session-status",
        Path::new(&lock),
        || {
            if write(dir, key, target, state) {
                Ok(())
            } else {
                Err(comandos_store::Error::Validation(
                    "no se pudo crear temporal de estado".into(),
                ))
            }
        },
        |db, origin| {
            comandos_store::unified::status_put(
                db,
                name,
                body.as_bytes(),
                state.ts.saturating_mul(1_000_000_000),
                origin,
            )
        },
    );
    if let Err(error) = &result {
        eprintln!("comandos hook: {error}");
    }
    result.is_ok()
}
/// Registros del arnés ligados a PID e identidad; no inspecciona procesos.
pub(crate) fn write_process(
    home: &Path,
    root: &Path,
    pid: u32,
    body: &[u8],
    now_ms: i64,
    legacy: impl FnOnce() -> Option<()>,
) -> Option<()> {
    let target = root.join(format!("{pid}.json"));
    let mut lock = target.as_os_str().to_owned();
    lock.push(".lock");
    comandos_store::domains::caller::write(home,"processes",Path::new(&lock),
        ||legacy().ok_or_else(||comandos_store::Error::Validation("escritura de proceso falló".into())),
        |db,origin| {db.execute("INSERT INTO native_processes VALUES(?1,?2,?3,?4) ON CONFLICT(pid) DO UPDATE SET body=excluded.body,mtime_ns=excluded.mtime_ns,origin=excluded.origin",rusqlite::params![i64::from(pid),body,now_ms.saturating_mul(1_000_000),if origin==comandos_store::unified::Origin::Mirror {"mirror"}else{"unified"}])?;Ok(())}).ok()
}

pub(crate) fn read_state_bytes(home: &Path, path: &Path) -> Option<Vec<u8>> {
    let name = path.file_name()?.to_str()?;
    let access =
        comandos_store::domains::caller::CallerAccess::open(home, "session-status").ok()?;
    if matches!(
        access.mode(),
        comandos_store::unified::Mode::Unified | comandos_store::unified::Mode::Sealed
    ) {
        access
            .db()?
            .query_row(
                "SELECT body FROM session_status WHERE file_key=?1",
                [name],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .ok()
    } else {
        fs::read(path).ok()
    }
}
pub(crate) fn read_domain(home: &Path, path: &Path) -> Option<Value> {
    serde_json::from_str(&jq_lossy(&read_state_bytes(home, path)?)).ok()
}

/// `jq -r 'if ((.status=="done" or .status=="waiting") and ((.detail//"")!=""))
/// then .detail else (.last//"") end'`.
pub fn previous_answer_domain(home: &Path, path: &Path) -> Vec<u8> {
    answer(read_domain(home, path))
}
fn answer(data: Option<Value>) -> Vec<u8> {
    let run = |state: &Value| -> Result<Vec<u8>, ()> {
        let status = field(state, "status")?.as_str();
        let detail = field(state, "detail")?;
        let empty = Value::String(String::new());
        let finished = matches!(status, Some("done" | "waiting"));
        if finished && alt(detail, &empty) != &empty {
            Ok(jq_r(detail))
        } else {
            Ok(jq_r(alt(field(state, "last")?, &empty)))
        }
    };
    data.and_then(|s| run(&s).ok()).unwrap_or_default()
}

/// `jq -r '.detail // .last // ""'` (lo que conserva un `GrokIdle`). `//` de jq
/// se traga el error de un estado que no es objeto: entonces sale vacío.
pub fn detail_or_last_domain(home: &Path, path: &Path) -> Vec<u8> {
    let data = read_domain(home, path);
    let empty = Value::String(String::new());
    match data {
        Some(state @ Value::Object(_)) => {
            let detail = field(&state, "detail").unwrap_or(&Value::Null);
            let last = field(&state, "last").unwrap_or(&Value::Null);
            jq_r(alt(alt(detail, last), &empty))
        }
        _ => Vec::new(),
    }
}

/// `jq -r '[.status,(.ts|tostring)]|join(" ")'` partido como `${prev%% *}` y
/// `${prev##* }`.
pub fn previous_status_domain(home: &Path, path: &Path) -> (String, String) {
    let data = read_domain(home, path);
    let run = |state: &Value| -> Result<String, ()> {
        let ts = Value::String(jq_tostring(field(state, "ts")?));
        Ok(jq_join([field(state, "status")?, &ts], " "))
    };
    let prev = data.and_then(|s| run(&s).ok()).unwrap_or_default();
    let prev = String::from_utf8_lossy(super::text::strip_nl(prev.as_bytes())).into_owned();
    let first = prev.split(' ').next().unwrap_or("").to_owned();
    let last = prev.rsplit(' ').next().unwrap_or("").to_owned();
    (first, last)
}

#[cfg(test)]
mod domain_tests {
    use super::*;
    use comandos_store::unified::{self, Mode};
    use std::os::unix::fs::DirBuilderExt;
    #[test]
    fn hook_state_round_trip_every_mode() {
        let home = std::env::temp_dir().join(format!("hook-domain-{}", std::process::id()));
        let dir = home.join(".claude/hooks/state");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .unwrap();
        let db = unified::open_unified(&unified::unified_path(&home)).unwrap();
        let target = dir.join("pane.json");
        let state = State {
            project: b"p",
            status: "done",
            detail: b"answer",
            cwd: b"/private",
            ts: 2,
            options: b"",
            last: b"",
            agent: b"claude",
            session: b"s",
            pane: "%1",
        };
        for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
            unified::set_mode(&db, "session-status", mode, "test", 1).unwrap();
            if mode == Mode::Sealed {
                fs::remove_file(&target).unwrap();
            }
            assert!(write_domain(&home, &dir, b"pane", &target, &state));
            if mode != Mode::Legacy {
                assert_eq!(
                    db.query_row(
                        "SELECT body FROM session_status WHERE file_key='pane.json'",
                        [],
                        |r| r.get::<_, Vec<u8>>(0)
                    )
                    .unwrap(),
                    render(&state).into_bytes()
                );
            }
            assert_eq!(previous_answer_domain(&home, &target), b"answer");
            assert_eq!(target.exists(), mode != Mode::Sealed);
        }
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    #[ignore = "gate de rendimiento: ejecutar explícitamente; requiere mediana menor de 3 ms"]
    fn hook_write_in_mirror_costs_under_3ms() {
        let home = std::env::temp_dir().join(format!("hook-benchmark-{}", std::process::id()));
        let dir = home.join(".claude/hooks/state");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .unwrap();
        let db = unified::open_unified(&unified::unified_path(&home)).unwrap();
        let target = dir.join("pane.json");
        let state = State {
            project: b"p",
            status: "done",
            detail: b"answer",
            cwd: b"/private",
            ts: 2,
            options: b"",
            last: b"",
            agent: b"claude",
            session: b"s",
            pane: "%1",
        };
        unified::set_mode(&db, "session-status", Mode::Mirror, "test", 1).unwrap();
        let mut samples = Vec::new();
        for _ in 0..200 {
            let start = std::time::Instant::now();
            assert!(write_domain(&home, &dir, b"pane", &target, &state));
            samples.push(start.elapsed());
        }
        samples.sort();
        let median = samples[100];
        eprintln!("hook200writes median_us={}", median.as_micros());
        assert!(median < std::time::Duration::from_millis(3));
        fs::remove_dir_all(home).unwrap();
    }
}
