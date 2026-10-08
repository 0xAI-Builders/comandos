//! Snapshot bridge orchestration; tmux, process identity and time are injected.
use comandos_core::json::{self, PythonLoads};
use comandos_runtime::{pane_snapshot::PaneInspector, pane_typing::TmuxResult, tmux_snapshot};
use comandos_store::domains::{DomainStore, LayoutSnapshot};
use serde_json::Value;
use std::{
    cell::RefCell,
    env, fs,
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub struct Config {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub proc_root: PathBuf,
}
pub trait Source {
    fn now(&mut self) -> i64;
    fn process_start(&mut self, pid: &str) -> Option<String>;
    fn tmux(&mut self, args: &[&str]) -> tmux_snapshot::Result<TmuxResult>;
    fn sleep(&mut self, seconds: u64);
}
#[derive(Debug)]
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}
struct Args {
    watch: Option<String>,
    output: PathBuf,
}
fn usage(program: &str) -> String {
    format!("usage: {program} [-h] [--watch-pid WATCH_PID] [--output OUTPUT]\n")
}
fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = quote.to_string();
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c)
            }
            c if c.is_ascii_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}
fn parse(home: &Path, args: &[String], program: &str) -> Result<Args, Outcome> {
    let error = |message: String| Outcome {
        code: 2,
        stdout: String::new(),
        stderr: format!("{}{program}: error: {message}\n", usage(program)),
    };
    let mut result = Args {
        watch: None,
        output: home.join(".claude/hooks/app-sessions-v2.json"),
    };
    let mut at = 0;
    let mut unknown = vec![];
    while let Some(arg) = args.get(at) {
        at += 1;
        let (flag, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(a, b)| (a, Some(b)));
        if matches!(flag, "-h" | "--help" | "--h" | "--he" | "--hel") {
            if let Some(value) = inline {
                return Err(error(format!(
                    "argument -h/--help: ignored explicit argument {}",
                    repr(value)
                )));
            }
            return Err(Outcome {
                code: 0,
                stderr: String::new(),
                stdout: format!(
                    "{}\nSave split layouts now; optionally bridge an already-running older GTK app.\n\noptions:\n  -h, --help            show this help message and exit\n  --watch-pid WATCH_PID\n                        capture every 5 seconds only while this existing app\n                        lives\n  --output OUTPUT\n",
                    usage(program)
                ),
            });
        }
        if arg == "--" {
            unknown.extend(args[at..].iter().cloned());
            break;
        }
        let kind = if flag.starts_with("--") && "--watch-pid".starts_with(flag) {
            Some("--watch-pid")
        } else if flag.starts_with("--") && "--output".starts_with(flag) {
            Some("--output")
        } else {
            None
        };
        let Some(kind) = kind else {
            unknown.push(arg.clone());
            continue;
        };
        let value = if let Some(value) = inline {
            value
        } else {
            let value = args
                .get(at)
                .filter(|v| !v.starts_with('-') || comandos_core::text::int(v).is_ok())
                .ok_or_else(|| error(format!("argument {kind}: expected one argument")))?;
            at += 1;
            value.as_str()
        };
        if kind == "--watch-pid" {
            let pid = comandos_core::text::int(value).map_err(|_| {
                error(format!(
                    "argument {kind}: invalid int value: {}",
                    repr(value)
                ))
            })?;
            result.watch = (pid != 0).then(|| pid.to_string());
        } else {
            result.output = PathBuf::from(value);
        }
    }
    if !unknown.is_empty() {
        return Err(error(format!(
            "unrecognized arguments: {}",
            unknown.join(" ")
        )));
    }
    Ok(result)
}
fn file_error(error: io::Error, path: &Path) -> String {
    if let Some(code) = error.raw_os_error() {
        format!(
            "[Errno {code}] {}: {}",
            error
                .to_string()
                .split(" (os error")
                .next()
                .unwrap_or("file error"),
            repr(&path.to_string_lossy())
        )
    } else {
        error.to_string()
    }
}
fn keys(config: &Config) -> Result<Vec<String>, String> {
    let file = config.home.join(".claude/hooks/app-tabs.json");
    let raw = DomainStore { home: &config.home }
        .document("hooks/app-tabs.json", "tabs", file.clone())
        .read_readonly()
        .map_err(|e| match e {
            comandos_store::Error::Io(e) => file_error(e, &file),
            e => e.to_string(),
        })?
        .ok_or_else(|| file_error(io::Error::from_raw_os_error(2), &file))?;
    let text = std::str::from_utf8(&raw).map_err(|e| e.to_string())?;
    let labels = json::workspace_loads(text).map_err(|_| match json::python_loads(text) {
        PythonLoads::Error(e) => e,
        _ => "invalid JSON".into(),
    })?;
    let mut keys = vec!["local".to_owned()];
    let names = match labels {
        Value::Object(labels) => labels.into_iter().map(|(key, _)| key).collect::<Vec<_>>(),
        Value::Array(labels) => labels
            .into_iter()
            .map(|v| {
                v.as_str().map(str::to_owned).ok_or_else(|| {
                    format!("'{}' object has no attribute 'startswith'", py_type(&v))
                })
            })
            .collect::<Result<_, _>>()?,
        Value::String(_) => vec![],
        value => return Err(format!("'{}' object is not iterable", py_type(&value))),
    };
    keys.extend(names.into_iter().filter(|key| key.starts_with("term-")));
    Ok(keys)
}
fn py_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_i64() || n.is_u64() => "int",
        Value::Number(_) => "float",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
        Value::String(_) => "str",
    }
}
fn identity_ready(captured: &Value) -> bool {
    captured
        .get("windows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|w| {
            w.get("panes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .all(|p| {
            !matches!(
                p.get("agent").and_then(Value::as_str),
                Some("claude" | "codex" | "grok")
            ) || p.get("resume_id").is_some_and(json::truthy)
        })
}
fn run_inner(
    config: &Config,
    source: &mut impl Source,
    args: &[String],
    program: &str,
    emit: &mut impl FnMut(bool, &str),
) -> i32 {
    let args = match parse(&config.home, args, program) {
        Ok(a) => a,
        Err(o) => {
            emit(false, &o.stdout);
            emit(true, &o.stderr);
            return o.code;
        }
    };
    let source = RefCell::new(source);
    let start = args
        .watch
        .as_deref()
        .and_then(|pid| source.borrow_mut().process_start(pid));
    if args.watch.is_some() && start.is_none() {
        return 1;
    }
    let alive = || {
        args.watch
            .as_deref()
            .is_none_or(|pid| source.borrow_mut().process_start(pid) == start)
    };
    let file = if args.output.is_absolute() {
        args.output.clone()
    } else {
        config.cwd.join(&args.output)
    };
    let lockpath = comandos_store::snapshot_files::suffix(&file, ".bridge.lock");
    let lock = match (|| -> io::Result<fs::File> {
        if let Some(parent) = lockpath.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o666)
            .open(&lockpath)
    })() {
        Ok(lock) => lock,
        Err(error) => {
            emit(
                true,
                &format!("snapshot failed: {}\n", file_error(error, &lockpath)),
            );
            return 1;
        }
    };
    match lock.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => return 0,
        Err(fs::TryLockError::Error(error)) => {
            emit(true, &format!("snapshot failed: {error}\n"));
            return 1;
        }
    }
    loop {
        if !alive() {
            return 0;
        }
        let attempt = (|| -> Result<Option<usize>, String> {
            let keys = keys(config)?;
            let layout = LayoutSnapshot {
                home: &config.home,
                file: file.clone(),
            };
            let previous = layout.read_readonly().map_err(|e| e.to_string())?;
            let mut saved = previous
                .get("sessions")
                .and_then(Value::as_object)
                .cloned()
                .ok_or("invalid snapshot sessions")?;
            let inspector = PaneInspector::new(&config.home, &config.proc_root)
                .map_err(|_| "pane inventory is uncertain")?;
            for key in keys {
                let captured = tmux_snapshot::capture_session_with_start_at(
                    &mut |args| source.borrow_mut().tmux(args),
                    &key,
                    &inspector,
                    0,
                    &mut |pid| {
                        source
                            .borrow_mut()
                            .process_start(pid)
                            .and_then(|s| s.parse::<u64>().ok())
                            .map(Value::from)
                            .unwrap_or(Value::Null)
                    },
                )
                .map_err(|e| e.to_string())
                .and_then(|mut captured| {
                    // The helper takes a fixed timestamp; the bridge samples
                    // completion after its tmux operations, as Python does.
                    captured["captured_at"] = Value::from(source.borrow_mut().now());
                    if identity_ready(&captured) {
                        Ok(captured)
                    } else {
                        Err("agent identity not ready".into())
                    }
                });
                match captured {
                    Ok(captured) => {
                        saved.insert(key, captured);
                    }
                    Err(error) => emit(
                        true,
                        &format!("{key}: retained previous snapshot: {error}\n"),
                    ),
                }
            }
            let count = saved.len();
            let value = serde_json::json!({"version":2,"saved_at":source.borrow_mut().now(),"sessions":saved});
            let write_time = source.borrow_mut().now();
            if !layout
                .write_when(&value, write_time, alive)
                .map_err(|e| e.to_string())?
            {
                return Ok(None);
            }
            Ok(Some(count))
        })();
        match attempt {
            Ok(None) => return 0,
            Ok(Some(count)) if args.watch.is_none() => {
                emit(
                    false,
                    &format!("Saved {count} sessions to {}\n", args.output.display()),
                );
                return 0;
            }
            Ok(Some(_)) => {}
            Err(error) => {
                emit(true, &format!("snapshot failed: {error}\n"));
                if args.watch.is_none() {
                    return 1;
                }
            }
        }
        source.borrow_mut().sleep(5);
    }
}
pub fn run(config: &Config, source: &mut impl Source, args: &[String], program: &str) -> Outcome {
    let mut stdout = String::new();
    let mut stderr = String::new();
    let code = run_inner(config, source, args, program, &mut |error, text| {
        if error {
            stderr.push_str(text)
        } else {
            stdout.push_str(text)
        }
    });
    Outcome {
        code,
        stdout,
        stderr,
    }
}
struct Native {
    proc_root: PathBuf,
}
impl Source for Native {
    fn now(&mut self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
    }
    fn process_start(&mut self, pid: &str) -> Option<String> {
        fs::read_to_string(self.proc_root.join(pid).join("stat"))
            .ok()?
            .rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(19)
            .map(str::to_owned)
    }
    fn sleep(&mut self, seconds: u64) {
        thread::sleep(Duration::from_secs(seconds));
    }
    fn tmux(&mut self, args: &[&str]) -> tmux_snapshot::Result<TmuxResult> {
        let mut child = Command::new("tmux")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| tmux_snapshot::SnapshotError::Runtime(file_error(e, Path::new("tmux"))))?;
        let reader = |mut pipe: Box<dyn Read + Send>| {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let mut bytes = vec![];
                let result = pipe.read_to_end(&mut bytes).map(|_| bytes);
                let _ = tx.send(result);
            });
            rx
        };
        let out = reader(Box::new(
            child
                .stdout
                .take()
                .ok_or(tmux_snapshot::SnapshotError::Unsure)?,
        ));
        let err = reader(Box::new(
            child
                .stderr
                .take()
                .ok_or(tmux_snapshot::SnapshotError::Unsure)?,
        ));
        let deadline = Instant::now() + Duration::from_secs(5);
        let (mut stdout, mut stderr, mut status) = (None, None, None);
        loop {
            if stdout.is_none()
                && let Ok(bytes) = out.try_recv()
            {
                stdout = Some(bytes?);
            }
            if stderr.is_none()
                && let Ok(bytes) = err.try_recv()
            {
                stderr = Some(bytes?);
            }
            if status.is_none() {
                status = child.try_wait()?;
            }
            if let (Some(stdout), Some(stderr), Some(status)) = (&stdout, &stderr, &status) {
                return Ok(TmuxResult {
                    returncode: status.code().unwrap_or(1),
                    stdout: String::from_utf8(stdout.clone())
                        .map_err(|_| tmux_snapshot::SnapshotError::Unsure)?,
                    stderr: String::from_utf8(stderr.clone())
                        .map_err(|_| tmux_snapshot::SnapshotError::Unsure)?,
                });
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(tmux_snapshot::SnapshotError::Runtime(timeout_message(args)));
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
}
pub fn main(args: &[String]) -> i32 {
    let config = Config {
        home: env::var_os("HOME").map(PathBuf::from).unwrap_or_default(),
        cwd: env::current_dir().unwrap_or_default(),
        proc_root: PathBuf::from("/proc"),
    };
    let alias = env::args().next().is_some_and(|a| {
        Path::new(&a)
            .file_name()
            .is_some_and(|n| n == "cc-session-snapshot")
    });
    let program = if alias {
        "cc-session-snapshot"
    } else {
        "comandos snapshot"
    };
    let mut source = Native {
        proc_root: config.proc_root.clone(),
    };
    run_inner(&config, &mut source, args, program, &mut |error, text| {
        if error {
            let _ = io::stderr().write_all(text.as_bytes());
        } else {
            let _ = io::stdout().write_all(text.as_bytes());
        }
    })
}

fn timeout_message(args: &[&str]) -> String {
    let argv = std::iter::once("tmux")
        .chain(args.iter().copied())
        .map(repr)
        .collect::<Vec<_>>()
        .join(", ");
    format!("Command '[{argv}]' timed out after 5 seconds")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tmux_timeout_message_matches_actual_original_without_starting_tmux() {
        for args in [
            vec!["list-windows", "-t", "=local"],
            vec!["list-panes", "-t", "=term-café"],
        ] {
            let mut command = Command::new("/usr/bin/python3");
            command
                .env_clear()
                .current_dir(env::temp_dir())
                .env("PYTHONDONTWRITEBYTECODE", "1");
            for key in [
                "HOME",
                "TMPDIR",
                "XDG_DATA_HOME",
                "XDG_CONFIG_HOME",
                "XDG_CACHE_HOME",
                "XDG_STATE_HOME",
                "XDG_RUNTIME_DIR",
            ] {
                command.env(key, env::var_os(key).expect("private test environment"));
            }
            let result = command
                .arg("-c")
                .arg(
                    r#"import runpy,subprocess,sys,json
m=runpy.run_path(sys.argv[1])
def expired(argv,**kwargs): raise subprocess.TimeoutExpired(argv,5)
subprocess.run=expired
try: m['tmux'](*json.loads(sys.argv[2]))
except subprocess.TimeoutExpired as exc: print(str(exc),end='')
"#,
                )
                .arg("/home/someguy/codebase/0xJesus/ComandOS/bin/cc-session-snapshot")
                .arg(serde_json::to_string(&args).unwrap())
                .output()
                .unwrap();
            assert!(result.status.success());
            assert_eq!(timeout_message(&args).as_bytes(), result.stdout);
        }
    }
}
