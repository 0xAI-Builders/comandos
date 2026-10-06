use comandos_cli::snapshot::{Config, Source};
use comandos_runtime::{pane_typing::TmuxResult, tmux_snapshot};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static SEQ: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "comandos-snapshot-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        for dir in [
            "home", "data", "config", "cache", "state", "run", "tmp", "bin", "proc",
        ] {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(root.join(dir))
                .unwrap();
        }
        symlink(
            env!("CARGO_BIN_EXE_comandos"),
            root.join("bin/cc-session-snapshot"),
        )
        .unwrap();
        Self(root)
    }
    fn home(&self) -> PathBuf {
        self.0.join("home")
    }
    fn command(&self, program: &Path) -> Command {
        let mut c = Command::new(program);
        c.env_clear()
            .current_dir(&self.0)
            .env("PATH", self.0.join("bin"))
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("HOME", self.home())
            .env("TMPDIR", self.0.join("tmp"))
            .env("XDG_DATA_HOME", self.0.join("data"))
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("XDG_CACHE_HOME", self.0.join("cache"))
            .env("XDG_STATE_HOME", self.0.join("state"))
            .env("XDG_RUNTIME_DIR", self.0.join("run"));
        c
    }
    fn oracle_args(&self, program: &str, args: &[&str]) -> Output {
        self.command(Path::new("/usr/bin/python3")).arg("-c").arg(
            "import runpy,sys; m=runpy.run_path(sys.argv[1]); sys.argv=sys.argv[2:]; raise SystemExit(m['main']())"
        ).arg("/home/someguy/codebase/0xJesus/ComandOS/bin/cc-session-snapshot")
            .arg(program).args(args).output().unwrap()
    }
    fn seed(&self) {
        fs::create_dir_all(self.home().join(".claude/hooks")).unwrap();
        fs::write(
            self.home().join(".claude/hooks/app-tabs.json"),
            br#"{"term-b":"B","ignored":"skip","term-a":"A","local":"Local"}"#,
        )
        .unwrap();
        fs::create_dir_all(self.0.join("proc/42")).unwrap();
        fs::write(
            self.0.join("proc/42/stat"),
            format!("42 (private shell) S 0 {} 123", vec!["0"; 17].join(" ")),
        )
        .unwrap();
        fs::write(self.0.join("proc/42/cmdline"), b"/bin/bash\0").unwrap();
    }
    fn scenario(&self) -> Value {
        let body = "100x30,0,0,1";
        json!({"layout":format!("{:04x},{body}", tmux_snapshot::layout_checksum(body)),"command":"bash","now":600,"watch":[],"fail":[]})
    }
    fn oracle_snapshot(&self, scenario: &Value, args: &[String]) -> Output {
        let case = self.0.join("case.json");
        fs::write(&case, scenario.to_string()).unwrap();
        self.command(Path::new("/usr/bin/python3")).arg("-c").arg(r#"
import json,runpy,sys,time,subprocess
from pathlib import Path
m=runpy.run_path(sys.argv[1]); g=m['main'].__globals__; root=Path(sys.argv[2]); case=json.loads((root/'case.json').read_text()); calls=[]; sleeps=[]
import pane_snapshot,tmux_snapshot
watch_state={'initial':False,'iteration':0,'capturing':False}
def started(pid):
    if str(pid)=='777':
        if not watch_state['initial']: index=0; watch_state['initial']=True
        else: index=1+2*watch_state['iteration']+int(watch_state['capturing'])
        ticks=case['watch']; return ticks[min(index,len(ticks)-1)] if ticks else None
    try: return (root/'proc'/str(int(pid))/'stat').read_text().rsplit(')',1)[1].split()[19]
    except (OSError,IndexError,ValueError): return None
def pane_started(pid):
    tick=started(pid)
    return int(tick) if tick is not None else None
def tmux(*args):
    case['now']+=case.get('tick',0)
    watch_state['capturing']=True
    calls.append(list(args)); layout=case['layout']; op=args[0]
    if op=='list-windows':
        key=args[2][1:]
        if key in case['fail']: return subprocess.CompletedProcess(args,1,'','no session')
        out='@1\t0\t'+key+'\t'+layout+'\t100\t30\t1\t0'
    elif op=='show-options': out=''
    elif op=='list-panes': out='%1\t0\t/private\t42\t'+case['command']+'\t1\t'
    elif op=='display-message': out=layout
    else: raise AssertionError(args)
    return subprocess.CompletedProcess(args,0,out,'')
def sleep(seconds):
    watch_state['iteration']+=1; watch_state['capturing']=False
    sleeps.append(seconds); case['now']+=seconds
time.time=lambda: case['now']; time.sleep=sleep
g['tmux']=tmux; g['process_start']=started; g['PaneInspector']=lambda: pane_snapshot.PaneInspector(root/'home',root/'proc')
tmux_snapshot.process_start_time=pane_started
sys.argv=['cc-session-snapshot']+sys.argv[3:]
code=g['main'](); (root/'calls.json').write_text(json.dumps({'calls':calls,'sleeps':sleeps})); raise SystemExit(code)
"#).arg("/home/someguy/codebase/0xJesus/ComandOS/bin/cc-session-snapshot").arg(&self.0).args(args).output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn snapshot_help_and_errors_match_actual_original_for_command_and_alias() {
    let f = Fixture::new();
    for (binary, prefix, program) in [
        (
            PathBuf::from(env!("CARGO_BIN_EXE_comandos")),
            vec!["snapshot"],
            "comandos snapshot",
        ),
        (
            f.0.join("bin/cc-session-snapshot"),
            vec![],
            "cc-session-snapshot",
        ),
    ] {
        for args in [
            vec!["--help"],
            vec!["-h"],
            vec!["--h"],
            vec!["--he"],
            vec!["--hel"],
            vec!["--help=bad"],
            vec!["--unknown"],
            vec!["--output"],
            vec!["--watch-pid", "bad"],
            vec!["extra"],
        ] {
            let oracle = f.oracle_args(program, &args);
            let native = f
                .command(&binary)
                .args(&prefix)
                .args(&args)
                .output()
                .unwrap();
            assert_eq!(
                native.status.code(),
                oracle.status.code(),
                "{program} {args:?}"
            );
            assert_eq!(native.stdout, oracle.stdout, "{program} {args:?}");
            assert_eq!(native.stderr, oracle.stderr, "{program} {args:?}");
            assert!(!f.home().join(".claude").exists());
            assert!(!f.home().join(".local").exists());
        }
    }
}

struct FakeSource {
    root: PathBuf,
    case: Value,
    calls: Vec<Vec<String>>,
    sleeps: Vec<u64>,
    watch_at: usize,
    previous: Option<String>,
    capturing: bool,
}
impl Source for FakeSource {
    fn now(&mut self) -> i64 {
        self.case["now"].as_i64().unwrap()
    }
    fn process_start(&mut self, pid: &str) -> Option<String> {
        if pid == "777" {
            let at = if self.watch_at == 0 {
                self.watch_at = 1;
                0
            } else {
                1 + 2 * self.sleeps.len() + usize::from(self.capturing)
            };
            let ticks = self.case["watch"].as_array().unwrap();
            self.previous = ticks
                .get(at.min(ticks.len().saturating_sub(1)))
                .and_then(Value::as_str)
                .map(str::to_owned);
            return self.previous.clone();
        }
        fs::read_to_string(self.root.join("proc").join(pid).join("stat"))
            .ok()?
            .rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(19)
            .map(str::to_owned)
    }
    fn sleep(&mut self, seconds: u64) {
        self.capturing = false;
        self.sleeps.push(seconds);
        self.case["now"] = json!(self.now() + seconds as i64);
    }
    fn tmux(&mut self, args: &[&str]) -> tmux_snapshot::Result<TmuxResult> {
        self.case["now"] = json!(self.now() + self.case["tick"].as_i64().unwrap_or(0));
        self.capturing = true;
        self.calls
            .push(args.iter().map(|s| (*s).to_owned()).collect());
        let layout = self.case["layout"].as_str().unwrap();
        let stdout = match args[0] {
            "list-windows" => {
                let key = &args[2][1..];
                if self.case["fail"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v == key)
                {
                    return Ok(TmuxResult {
                        returncode: 1,
                        stdout: String::new(),
                        stderr: "no session".into(),
                    });
                }
                format!("@1\t0\t{key}\t{layout}\t100\t30\t1\t0")
            }
            "show-options" => String::new(),
            "list-panes" => format!(
                "%1\t0\t/private\t42\t{}\t1\t",
                self.case["command"].as_str().unwrap()
            ),
            "display-message" => layout.to_owned(),
            _ => panic!("unexpected fake tmux operation"),
        };
        Ok(TmuxResult {
            returncode: 0,
            stdout,
            stderr: String::new(),
        })
    }
}
fn normalized(bytes: &[u8], root: &Path) -> String {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .replace(root.to_str().unwrap(), "<fixture>")
}
#[test]
fn snapshot_capture_and_session_order_match_actual_original() {
    let oracle = Fixture::new();
    oracle.seed();
    let native = Fixture::new();
    native.seed();
    let scenario = oracle.scenario();
    let expected = oracle.oracle_snapshot(&scenario, &[]);
    let mut source = FakeSource {
        root: native.0.clone(),
        case: scenario,
        calls: vec![],
        sleeps: vec![],
        watch_at: 0,
        previous: None,
        capturing: false,
    };
    let actual = comandos_cli::snapshot::run(
        &Config {
            home: native.home(),
            cwd: native.0.clone(),
            proc_root: native.0.join("proc"),
        },
        &mut source,
        &[],
        "cc-session-snapshot",
    );
    assert_eq!(actual.code, expected.status.code().unwrap());
    assert_eq!(
        normalized(actual.stdout.as_bytes(), &native.0),
        normalized(&expected.stdout, &oracle.0)
    );
    assert_eq!(
        normalized(actual.stderr.as_bytes(), &native.0),
        normalized(&expected.stderr, &oracle.0)
    );
    let recorded: Value =
        serde_json::from_slice(&fs::read(oracle.0.join("calls.json")).unwrap()).unwrap();
    assert_eq!(json!(source.calls), recorded["calls"]);
    assert_eq!(json!(source.sleeps), recorded["sleeps"]);
    let path = ".claude/hooks/app-sessions-v2.json";
    assert_eq!(
        fs::read(native.home().join(path)).unwrap(),
        fs::read(oracle.home().join(path)).unwrap()
    );
    assert_eq!(
        source
            .calls
            .iter()
            .filter(|a| a[0] == "list-windows")
            .map(|a| a[2].as_str())
            .collect::<Vec<_>>(),
        vec!["=local", "=term-b", "=term-a"]
    );
}

#[test]
fn watch_pid_checks_start_before_capture_and_before_persist_and_sleeps_five_seconds() {
    for watch in [
        json!([null]),
        json!(["123", null]),
        json!(["123", "123", null]),
        json!(["123", "123", "456"]),
        json!(["123", "123", "123", null]),
        json!(["123", "123", "123", "123", "123", null]),
    ] {
        let oracle = Fixture::new();
        oracle.seed();
        let native = Fixture::new();
        native.seed();
        let mut scenario = oracle.scenario();
        scenario["watch"] = watch;
        let args = vec!["--watch-pid".into(), "777".into()];
        let expected = oracle.oracle_snapshot(&scenario, &args);
        let mut source = FakeSource {
            root: native.0.clone(),
            case: scenario,
            calls: vec![],
            sleeps: vec![],
            watch_at: 0,
            previous: None,
            capturing: false,
        };
        let actual = comandos_cli::snapshot::run(
            &Config {
                home: native.home(),
                cwd: native.0.clone(),
                proc_root: native.0.join("proc"),
            },
            &mut source,
            &args,
            "cc-session-snapshot",
        );
        assert_eq!(actual.code, expected.status.code().unwrap());
        assert_eq!(actual.stdout.as_bytes(), expected.stdout);
        assert_eq!(actual.stderr.as_bytes(), expected.stderr);
        let recorded: Value =
            serde_json::from_slice(&fs::read(oracle.0.join("calls.json")).unwrap()).unwrap();
        assert_eq!(json!(source.calls), recorded["calls"]);
        assert_eq!(json!(source.sleeps), recorded["sleeps"]);
        for tail in ["", ".bak"] {
            let path = format!(".claude/hooks/app-sessions-v2.json{tail}");
            assert_eq!(
                fs::read(native.home().join(&path)).ok(),
                fs::read(oracle.home().join(&path)).ok()
            );
        }
        let history = ".claude/hooks/app-sessions-v2.json.history";
        assert_eq!(
            native.home().join(history).exists(),
            oracle.home().join(history).exists()
        );
        if native.home().join(history).exists() {
            for entry in fs::read_dir(oracle.home().join(history)).unwrap() {
                let entry = entry.unwrap();
                assert_eq!(
                    fs::read(native.home().join(history).join(entry.file_name())).unwrap(),
                    fs::read(entry.path()).unwrap()
                );
            }
        }
        if actual.code == 1 {
            assert!(
                !native
                    .home()
                    .join(".claude/hooks/app-sessions-v2.json.bridge.lock")
                    .exists()
            );
        }
        if !native
            .home()
            .join(".claude/hooks/app-sessions-v2.json")
            .exists()
        {
            assert!(
                !native.home().join(".local").exists(),
                "lost/reused PID must not create mode-lock parents"
            );
            assert!(
                !native
                    .home()
                    .join(".claude/hooks/app-sessions-v2.json.lock")
                    .exists()
            );
        }
    }
}

fn native_run(
    f: &Fixture,
    scenario: Value,
    args: &[String],
) -> (comandos_cli::snapshot::Outcome, FakeSource) {
    let mut source = FakeSource {
        root: f.0.clone(),
        case: scenario,
        calls: vec![],
        sleeps: vec![],
        watch_at: 0,
        previous: None,
        capturing: false,
    };
    let outcome = comandos_cli::snapshot::run(
        &Config {
            home: f.home(),
            cwd: f.0.clone(),
            proc_root: f.0.join("proc"),
        },
        &mut source,
        args,
        "cc-session-snapshot",
    );
    (outcome, source)
}
fn assert_result(
    oracle: &Fixture,
    native: &Fixture,
    expected: &Output,
    actual: &comandos_cli::snapshot::Outcome,
    source: &FakeSource,
) {
    assert_eq!(actual.code, expected.status.code().unwrap());
    assert_eq!(
        normalized(actual.stdout.as_bytes(), &native.0),
        normalized(&expected.stdout, &oracle.0)
    );
    assert_eq!(
        normalized(actual.stderr.as_bytes(), &native.0),
        normalized(&expected.stderr, &oracle.0)
    );
    let recorded: Value =
        serde_json::from_slice(&fs::read(oracle.0.join("calls.json")).unwrap()).unwrap();
    assert_eq!(json!(source.calls), recorded["calls"]);
    assert_eq!(json!(source.sleeps), recorded["sleeps"]);
}
#[test]
fn labels_missing_or_invalid_fail_like_original_without_capture_or_persistence() {
    for labels in [None, Some("{"), Some("null"), Some("[1]"), Some("false")] {
        let oracle = Fixture::new();
        oracle.seed();
        let native = Fixture::new();
        native.seed();
        for f in [&oracle, &native] {
            let path = f.home().join(".claude/hooks/app-tabs.json");
            if let Some(labels) = labels {
                fs::write(path, labels).unwrap()
            } else {
                fs::remove_file(path).unwrap()
            }
        }
        let case = oracle.scenario();
        let expected = oracle.oracle_snapshot(&case, &[]);
        let (actual, source) = native_run(&native, case, &[]);
        assert_result(&oracle, &native, &expected, &actual, &source);
        assert!(source.calls.is_empty());
        assert!(!native.home().join(".local").exists());
        assert!(
            !native
                .home()
                .join(".claude/hooks/app-sessions-v2.json")
                .exists()
        );
    }
}
#[test]
fn previous_sessions_survive_failed_capture_and_unready_agent_identities() {
    for agent in ["bash", "claude", "codex", "grok"] {
        let oracle = Fixture::new();
        oracle.seed();
        let native = Fixture::new();
        native.seed();
        let mut case = oracle.scenario();
        for f in [&oracle, &native] {
            assert!(f.oracle_snapshot(&case, &[]).status.success());
            fs::write(f.0.join("proc/42/cmdline"), format!("/bin/{agent}\0")).unwrap();
        }
        case["now"] = json!(660);
        case["command"] = json!(agent);
        if agent == "bash" {
            case["fail"] = json!(["local", "term-b"])
        }
        let expected = oracle.oracle_snapshot(&case, &[]);
        let (actual, source) = native_run(&native, case, &[]);
        assert_result(&oracle, &native, &expected, &actual, &source);
        for tail in ["", ".bak"] {
            let path = format!(".claude/hooks/app-sessions-v2.json{tail}");
            assert_eq!(
                fs::read(native.home().join(&path)).unwrap(),
                fs::read(oracle.home().join(&path)).unwrap()
            );
        }
        let archive = ".claude/hooks/app-sessions-v2.json.history/000000000600.json";
        for f in [&oracle, &native] {
            let main = fs::metadata(f.home().join(".claude/hooks/app-sessions-v2.json")).unwrap();
            let backup =
                fs::metadata(f.home().join(".claude/hooks/app-sessions-v2.json.bak")).unwrap();
            let archived = fs::metadata(f.home().join(archive)).unwrap();
            assert_eq!(main.mode() & 0o777, 0o600);
            assert_eq!(backup.ino(), archived.ino());
            assert_ne!(main.ino(), backup.ino());
            assert_eq!(
                fs::metadata(f.home().join(".claude/hooks/app-sessions-v2.json.history"))
                    .unwrap()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        assert_eq!(
            fs::read(native.home().join(archive)).unwrap(),
            fs::read(oracle.home().join(archive)).unwrap()
        );
        if agent != "bash" {
            assert!(actual.stderr.contains("agent identity not ready"));
        }
    }
}
#[test]
fn custom_output_and_corrupt_current_recover_valid_backup_like_original() {
    for custom in [
        "private-output/snapshot.json",
        "private-output/../snapshot.json",
    ] {
        let oracle = Fixture::new();
        oracle.seed();
        let native = Fixture::new();
        native.seed();
        let args = vec!["--output".into(), custom.into()];
        let mut case = oracle.scenario();
        for f in [&oracle, &native] {
            assert!(f.oracle_snapshot(&case, &args).status.success());
            case["now"] = json!(660);
            assert!(f.oracle_snapshot(&case, &args).status.success());
            case["now"] = json!(600);
            fs::write(f.0.join(custom), b"corrupt").unwrap();
        }
        case["now"] = json!(720);
        case["fail"] = json!(["local", "term-b", "term-a"]);
        let expected = oracle.oracle_snapshot(&case, &args);
        let (actual, source) = native_run(&native, case, &args);
        assert_result(&oracle, &native, &expected, &actual, &source);
        for tail in ["", ".bak"] {
            let path = format!("{custom}{tail}");
            assert_eq!(
                fs::read(native.0.join(&path)).unwrap(),
                fs::read(oracle.0.join(&path)).unwrap()
            );
        }
        assert!(
            !native
                .home()
                .join(".claude/hooks/app-sessions-v2.json")
                .exists()
        );
    }
}
#[test]
fn private_bridge_lock_contention_exits_without_capture_like_original() {
    let oracle = Fixture::new();
    oracle.seed();
    let native = Fixture::new();
    native.seed();
    let mut held = vec![];
    for f in [&oracle, &native] {
        let path = f
            .home()
            .join(".claude/hooks/app-sessions-v2.json.bridge.lock");
        fs::write(&path, b"old lock bytes").unwrap();
        let file = fs::File::open(&path).unwrap();
        file.lock().unwrap();
        held.push(file);
    }
    let case = oracle.scenario();
    let expected = oracle.oracle_snapshot(&case, &[]);
    let (actual, source) = native_run(&native, case, &[]);
    assert_result(&oracle, &native, &expected, &actual, &source);
    assert_eq!(actual.code, 0);
    assert!(source.calls.is_empty());
    for f in [&oracle, &native] {
        assert_eq!(
            fs::read(
                f.home()
                    .join(".claude/hooks/app-sessions-v2.json.bridge.lock")
            )
            .unwrap(),
            b""
        );
        assert!(!f.home().join(".local").exists());
    }
}

#[test]
fn ready_agent_identities_are_saved_like_original_from_private_inventories() {
    for agent in ["claude", "codex", "grok"] {
        let oracle = Fixture::new();
        oracle.seed();
        let native = Fixture::new();
        native.seed();
        let mut case = oracle.scenario();
        case["command"] = json!(agent);
        for f in [&oracle, &native] {
            let argv = if agent == "codex" {
                [
                    "/bin/codex",
                    "resume",
                    "00000000-0000-0000-0000-000000000000",
                    "",
                ]
                .join("\0")
            } else {
                format!("/bin/{agent}\0")
            };
            fs::write(f.0.join("proc/42/cmdline"), argv).unwrap();
            if agent == "claude" {
                fs::create_dir_all(f.home().join(".claude/sessions")).unwrap();
                fs::write(
                    f.home().join(".claude/sessions/private.json"),
                    br#"{"pid":42,"sessionId":"private-resume"}"#,
                )
                .unwrap();
            }
            if agent == "grok" {
                fs::create_dir_all(f.home().join(".grok")).unwrap();
                fs::write(
                    f.home().join(".grok/active_sessions.json"),
                    br#"[{"pid":42,"session_id":"private-resume"}]"#,
                )
                .unwrap();
            }
        }
        let expected = oracle.oracle_snapshot(&case, &[]);
        let (actual, source) = native_run(&native, case, &[]);
        assert_result(&oracle, &native, &expected, &actual, &source);
        let path = ".claude/hooks/app-sessions-v2.json";
        assert_eq!(
            normalized(&fs::read(native.home().join(path)).unwrap(), &native.0),
            normalized(&fs::read(oracle.home().join(path)).unwrap(), &oracle.0)
        );
        assert!(actual.stderr.is_empty());
        let value: Value =
            serde_json::from_slice(&fs::read(native.home().join(path)).unwrap()).unwrap();
        assert_eq!(
            value["sessions"]["local"]["windows"][0]["panes"][0]["agent"],
            agent
        );
        assert!(value["sessions"]["local"]["windows"][0]["panes"][0]["resume_id"].is_string());
    }
}
#[test]
fn minute_collision_and_seven_day_pruning_match_actual_original_files() {
    let oracle = Fixture::new();
    oracle.seed();
    let native = Fixture::new();
    native.seed();
    let mut case = oracle.scenario();
    assert!(oracle.oracle_snapshot(&case, &[]).status.success());
    assert_eq!(native_run(&native, case.clone(), &[]).0.code, 0);
    for f in [&oracle, &native] {
        let history = f.home().join(".claude/hooks/app-sessions-v2.json.history");
        fs::create_dir(&history).unwrap();
        fs::write(history.join("000000000600.json"), b"first archive wins").unwrap();
        fs::write(history.join("000000000100.json"), b"ancient").unwrap();
    }
    for now in [740105, 740165] {
        case["now"] = json!(now);
        let expected = oracle.oracle_snapshot(&case, &[]);
        let (actual, source) = native_run(&native, case.clone(), &[]);
        assert_result(&oracle, &native, &expected, &actual, &source);
        for tail in ["", ".bak"] {
            let path = format!(".claude/hooks/app-sessions-v2.json{tail}");
            assert_eq!(
                fs::read(native.home().join(&path)).unwrap(),
                fs::read(oracle.home().join(&path)).unwrap()
            );
        }
        let history = ".claude/hooks/app-sessions-v2.json.history";
        let files = |f: &Fixture| {
            let mut files = fs::read_dir(f.home().join(history))
                .unwrap()
                .map(|e| {
                    let e = e.unwrap();
                    (e.file_name(), fs::read(e.path()).unwrap())
                })
                .collect::<Vec<_>>();
            files.sort();
            files
        };
        assert_eq!(files(&native), files(&oracle));
        assert_eq!(
            native
                .home()
                .join(history)
                .join("000000000100.json")
                .exists(),
            now == 740105
        );
    }
}

#[test]
fn snapshot_uses_store_owned_tab_and_layout_authority_in_all_modes() {
    use comandos_store::{
        domains::LayoutSnapshot,
        unified::{self, Mode},
    };
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let f = Fixture::new();
        f.seed();
        let case = f.scenario();
        assert!(f.oracle_snapshot(&case, &[]).status.success());
        let path = f.home().join(".claude/hooks/app-sessions-v2.json");
        let original = fs::read(&path).unwrap();
        let mut stored: Value = serde_json::from_slice(&original).unwrap();
        stored["saved_at"] = json!(700);
        let db = unified::open_unified(&f.home().join(".local/share/comandos/comandos.sqlite3"))
            .unwrap();
        unified::layout_put(
            &db,
            unified::Generation::Current,
            700,
            stored.to_string().as_bytes(),
        )
        .unwrap();
        unified::doc_put(
            &db,
            "hooks/app-tabs.json",
            "tabs",
            br#"{"term-db":"database"}"#,
            unified::Origin::Import,
            700,
        )
        .unwrap();
        for domain in ["tabs", "layout"] {
            unified::set_mode(&db, domain, mode, "fixture", 1).unwrap();
        }
        let (actual, source) = native_run(&f, case, &[]);
        assert_eq!(actual.code, 0, "{}", actual.stderr);
        let requests = source
            .calls
            .iter()
            .filter(|a| a[0] == "list-windows")
            .map(|a| a[2].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            requests,
            if matches!(mode, Mode::Unified | Mode::Sealed) {
                vec!["=local", "=term-db"]
            } else {
                vec!["=local", "=term-b", "=term-a"]
            }
        );
        let home = f.home();
        let layout = LayoutSnapshot {
            home: &home,
            file: path.clone(),
        };
        let saved = layout.read_readonly().unwrap();
        assert_eq!(saved["saved_at"], json!(600));
        if mode == Mode::Sealed {
            assert_eq!(fs::read(path).unwrap(), original);
            assert!(
                !f.home()
                    .join(".claude/hooks/app-sessions-v2.json.bak")
                    .exists()
            );
            assert!(
                !f.home()
                    .join(".claude/hooks/app-sessions-v2.json.lock")
                    .exists()
            );
        }
        if matches!(mode, Mode::Unified | Mode::Sealed) {
            assert!(saved["sessions"].get("term-db").is_some());
            assert!(saved["sessions"].get("term-a").is_some());
        }
    }
}

#[test]
fn watched_global_errors_retry_after_fake_five_second_clock_like_original() {
    let oracle = Fixture::new();
    oracle.seed();
    let native = Fixture::new();
    native.seed();
    for f in [&oracle, &native] {
        fs::write(f.home().join(".claude/hooks/app-tabs.json"), b"{").unwrap();
    }
    let mut case = oracle.scenario();
    case["watch"] = json!(["123", "123", "123", null]);
    let args = vec!["--watch-pid".into(), "777".into()];
    let expected = oracle.oracle_snapshot(&case, &args);
    let (actual, source) = native_run(&native, case, &args);
    assert_result(&oracle, &native, &expected, &actual, &source);
    assert_eq!(actual.code, 0);
    assert!(actual.stderr.starts_with("snapshot failed:"));
    assert_eq!(source.sleeps, vec![5]);
    assert!(source.calls.is_empty());
    assert!(!native.home().join(".local").exists());
}

#[test]
fn captured_at_matches_original_completion_clock_and_retains_failed_session_metadata() {
    for failed in [false, true] {
        let oracle = Fixture::new();
        oracle.seed();
        let native = Fixture::new();
        native.seed();
        let mut case = oracle.scenario();
        if failed {
            for fixture in [&oracle, &native] {
                assert!(fixture.oracle_snapshot(&case, &[]).status.success());
            }
            case["fail"] = json!(["term-b"]);
        }
        case["tick"] = json!(1);
        let expected = oracle.oracle_snapshot(&case, &[]);
        let (actual, source) = native_run(&native, case, &[]);
        assert_result(&oracle, &native, &expected, &actual, &source);
        let path = ".claude/hooks/app-sessions-v2.json";
        let expected_bytes = fs::read(oracle.home().join(path)).unwrap();
        let actual_bytes = fs::read(native.home().join(path)).unwrap();
        assert_eq!(actual_bytes, expected_bytes);
        let value: Value = serde_json::from_slice(&actual_bytes).unwrap();
        for (key, stamp) in if failed {
            [("local", 606), ("term-b", 600), ("term-a", 613)]
        } else {
            [("local", 606), ("term-b", 612), ("term-a", 618)]
        } {
            assert_eq!(value["sessions"][key]["captured_at"], json!(stamp));
        }
        assert_eq!(value["saved_at"], json!(if failed { 613 } else { 618 }));
        if failed {
            let backup = ".claude/hooks/app-sessions-v2.json.bak";
            assert_eq!(
                fs::read(native.home().join(backup)).unwrap(),
                fs::read(oracle.home().join(backup)).unwrap()
            );
            assert!(actual.stderr.contains("term-b: retained previous snapshot"));
        }
    }
}
