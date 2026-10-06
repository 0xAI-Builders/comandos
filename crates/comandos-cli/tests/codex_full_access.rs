//! Fresh real-source Python differential and native executable probes, private proc and PATH only.
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
const SID: &str = "11111111-1111-1111-1111-111111111111";
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let r = std::env::temp_dir().join(format!(
            "full-access-c5-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&r).unwrap();
        for sub in [
            "home", "config", "data", "state", "cache", "runtime", "tmp", "bin", "proc", "lib",
            "repo/bin",
        ] {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(r.join(sub))
                .unwrap();
        }
        let f = Self(r);
        for name in [
            "codex_full_access.py",
            "codex_thread_release.py",
            "codex_yolo_install.py",
            "codex_yolo_policy.py",
            "pane_snapshot.py",
        ] {
            fs::write(
                f.0.join("lib").join(name),
                fs::read(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../lib")
                        .join(name),
                )
                .unwrap(),
            )
            .unwrap();
        }
        fs::write(
            f.0.join("repo/bin/cc-codex-full-access"),
            include_bytes!("../../../bin/cc-codex-full-access"),
        )
        .unwrap();
        symlink(f.0.join("lib"), f.0.join("repo/lib")).unwrap();
        f
    }
    fn command(&self, path: &Path) -> Command {
        let mut c = Command::new(path);
        c.env_clear()
            .env("HOME", self.0.join("home"))
            .env("PATH", self.0.join("bin"));
        for (k, v) in [
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_RUNTIME_DIR", "runtime"),
            ("TMPDIR", "tmp"),
            ("TMP", "tmp"),
            ("TEMP", "tmp"),
        ] {
            c.env(k, self.0.join(v));
        }
        c
    }
    fn tool(&self, name: &str, body: &str) {
        let p = self.0.join("bin").join(name);
        fs::write(&p, format!("#!/usr/bin/python3\n{body}")).unwrap();
        fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fn process(&self, pid: i64, parent: i64, birth: &str, args: &[&str]) {
        let p = self.0.join("proc").join(pid.to_string());
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        let mut stat = format!("{pid} (fixture) S {parent}");
        for _ in 0..17 {
            stat.push_str(" 0");
        }
        stat.push_str(&format!(" {birth}"));
        fs::write(p.join("stat"), stat).unwrap();
        fs::write(p.join("cmdline"), format!("{}\0", args.join("\0"))).unwrap();
        fs::write(
            p.join("environ"),
            format!("CODEX_HOME={}\0", self.0.join("account").display()),
        )
        .unwrap();
    }
    fn transcript(&self) -> PathBuf {
        self.0
            .join("account/sessions/2026/10/06")
            .join(format!("rollout-fixture-{SID}.jsonl"))
    }
    fn setup(&self) {
        let path = self.transcript();
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path.parent().unwrap())
            .unwrap();
        fs::write(&path,format!("{}\n{}\n",json!({"type":"session_meta","payload":{"id":SID,"source":"cli"}}),json!({"type":"turn_context","payload":{"model":"fixture-model","effort":"ultra","approval_policy":"never","sandbox_policy":{"type":"danger-full-access"}}}))).unwrap();
        self.process(100, 99, "1000", &["/bin/bash"]);
        let binary = self.0.join("bin/codex");
        self.process(
            101,
            100,
            "1001",
            &[binary.to_str().unwrap(), "resume", SID, "--yolo"],
        );
        self.tool("tmux",&format!(r#"import sys,json,os
from pathlib import Path
r=Path(__file__).parent.parent
args=sys.argv[1:]
with (r/'tmux-trace').open('a') as f:f.write(json.dumps(args)+'\n')
if args[0]=='list-panes':print('s\x1f%1\x1f100\x1fcodex\x1f'+str(r/'cwd literal \' 雪'))
elif args[0]=='display-message':
 key=args[-1];print({{'#{{session_name}}':'s','#{{pane_pid}}':'100','#{{pane_current_command}}':'codex' if (r/'proc/101').exists() else 'bash','#{{pane_tty}}':'/dev/fake-owned'}}[key])
elif args[0]=='send-keys':
 reports=list((r/'state/comandos/codex-full-access').glob('*.json'))
 assert reports,'missing durable plan before keys'
 assert all(json.loads(p.read_text()).get('plans') for p in reports)
 if args[-1]=='C-c':
  import shutil;shutil.rmtree(r/'proc/101',ignore_errors=True)
 if args[-1]=='Enter':
  p=Path({transcript:?})
  with p.open('a') as f:f.write(json.dumps({{'type':'turn_context','payload':{{'approval_policy':'never','sandbox_policy':{{'type':'danger-full-access'}}}}}})+'\n')
elif args[0]=='capture-pane':print('fixture')
else:sys.exit(99)
"#,transcript=path.to_str().unwrap()));
        self.tool("codex","import sys\nif sys.argv[1:] in [['--help'],['resume','--help']]:print('--no-daemon --dangerously-bypass-approvals-and-sandbox')\nelse:raise SystemExit(99)\n");
        self.tool(
            "stty",
            "import sys\nif sys.argv[1:3]!=['sane','-F']:raise SystemExit(99)\n",
        );
    }
    fn rust(&self, args: &[&str]) -> Output {
        self.command(Path::new(env!("CARGO_BIN_EXE_comandos")))
            .args(["codex", "full-access", "--proc-root"])
            .arg(self.0.join("proc"))
            .args(args)
            .output()
            .unwrap()
    }
    fn python(&self, args: &[&str]) -> Output {
        let bootstrap = r#"import sys,runpy
from pathlib import Path
root=Path(sys.argv[1]);sys.path.insert(0,str(root/'lib'))
import codex_full_access as m
from pane_snapshot import PaneInspector
P=type(Path())
class SafePath(P):
 def __new__(cls,*parts):
  if parts and str(parts[0])=='/proc':parts=(root/'proc',*parts[1:])
  return super().__new__(cls,*parts)
m.Path=SafePath
class Private(m.LocalRuntime):
 def __init__(self):self.inspector=PaneInspector(home=root/'home',proc_root=root/'proc')
m.LocalRuntime=Private
sys.argv=[str(root/'repo/bin/cc-codex-full-access'),*sys.argv[2:]]
runpy.run_path(sys.argv[0],run_name='__main__')
"#;
        self.command(Path::new("/usr/bin/python3"))
            .args(["-I", "-c", bootstrap])
            .arg(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn same(a: &Output, b: &Output) {
    assert_eq!(
        a.status.code(),
        b.status.code(),
        "{}",
        String::from_utf8_lossy(&a.stderr)
    );
    assert_eq!(a.stdout, b.stdout);
    assert_eq!(a.stderr, b.stderr);
}
#[test]
fn default_inspection_matches_original_bytes_and_alias_without_mutation() {
    let f = Fixture::new();
    f.setup();
    let original = f.python(&[]);
    let rust = f.rust(&[]);
    same(&rust, &original);
    symlink(
        env!("CARGO_BIN_EXE_comandos"),
        f.0.join("bin/cc-codex-full-access"),
    )
    .unwrap();
    let alias = f
        .command(&f.0.join("bin/cc-codex-full-access"))
        .arg("--proc-root")
        .arg(f.0.join("proc"))
        .output()
        .unwrap();
    same(&alias, &original);
    assert!(!f.0.join("home/.local").exists());
    assert!(!f.0.join("state/comandos").exists());
}
#[test]
fn malformed_metadata_blocks_entire_batch_before_install_or_keystrokes() {
    let f = Fixture::new();
    f.setup();
    fs::write(
        f.transcript(),
        format!(
            "{}\n",
            json!({"type":"session_meta","payload":{"id":"other"}})
        ),
    )
    .unwrap();
    same(&f.rust(&["--apply"]), &f.python(&["--apply"]));
    assert!(!f.0.join("home/.local").exists());
    let trace = fs::read_to_string(f.0.join("tmux-trace")).unwrap();
    assert!(!trace.contains("send-keys"));
}
#[test]
fn native_apply_checkpoints_before_keystrokes_and_retries_only_pending() {
    let f = Fixture::new();
    f.setup();
    let o = f.rust(&["--apply"]);
    assert!(
        o.status.success(),
        "{} {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let dir = f.0.join("state/comandos/codex-full-access");
    let reports = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            (p.extension().and_then(|s| s.to_str()) == Some("json")).then_some(p)
        })
        .collect::<Vec<_>>();
    assert_eq!(reports.len(), 1);
    let report: Value = serde_json::from_slice(&fs::read(&reports[0]).unwrap()).unwrap();
    assert_eq!(report["results"][0]["status"], "confirmed");
    assert_eq!(report["results"][0]["conversationId"], SID);
    assert_eq!(report["results"][0]["prompt"], "continua");
    let trace = fs::read(f.0.join("tmux-trace")).unwrap();
    let retry = f.rust(&["--retry-failed", "--apply"]);
    assert!(retry.status.success());
    assert_eq!(fs::read(f.0.join("tmux-trace")).unwrap(), trace);
}
#[test]
fn future_report_authority_rejects_before_any_tool_or_write() {
    let f = Fixture::new();
    f.setup();
    let db = comandos_store::unified::open_unified(&comandos_store::unified::unified_path(
        &f.0.join("home"),
    ))
    .unwrap();
    db.pragma_update(None, "user_version", 999999).unwrap();
    drop(db);
    let o = f.rust(&["--apply"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(!f.0.join("tmux-trace").exists());
    assert!(!f.0.join("home/.local/share/comandos/codex-yolo").exists());
    assert!(!f.0.join("state/comandos").exists());
}

#[test]
fn failed_launch_retry_reuses_exact_saved_shell_and_preserves_prior_report() {
    let f = Fixture::new();
    f.setup();
    f.tool("stty", "raise SystemExit(9)\n");
    let failed = f.rust(&["--apply"]);
    assert_eq!(failed.status.code(), Some(1));
    let dir = f.0.join("state/comandos/codex-full-access");
    let old = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .unwrap();
    let old_bytes = fs::read(&old).unwrap();
    let first: Value = serde_json::from_slice(&old_bytes).unwrap();
    assert_eq!(first["results"][0]["status"], "failed");
    let artifact = f.0.join("home/.local/share/comandos/codex-yolo/codex-yolo");
    use std::os::unix::fs::MetadataExt;
    let inode = fs::metadata(&artifact).unwrap().ino();
    f.tool("stty", "pass\n");
    let retry = f.rust(&["--retry-report", old.to_str().unwrap(), "--apply"]);
    assert!(
        retry.status.success(),
        "{} {}",
        String::from_utf8_lossy(&retry.stdout),
        String::from_utf8_lossy(&retry.stderr)
    );
    assert_eq!(fs::read(&old).unwrap(), old_bytes);
    assert_eq!(fs::metadata(artifact).unwrap().ino(), inode);
    let latest = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p != &old && p.extension().and_then(|s| s.to_str()) == Some("json"))
        .unwrap();
    let report: Value = serde_json::from_slice(&fs::read(latest).unwrap()).unwrap();
    assert_eq!(report["previousReport"], old.to_str().unwrap());
    assert_eq!(report["results"][0]["status"], "confirmed");
    assert_eq!(report["plans"][0]["sid"], SID);
    assert!(report["plans"][0]["pid"].is_null());
    let keys = fs::read_to_string(f.0.join("tmux-trace")).unwrap();
    assert_eq!(keys.lines().filter(|s| s.contains("C-c")).count(), 1);
}
#[test]
fn argument_errors_exit_two_before_inventory_like_original() {
    let f = Fixture::new();
    for args in [
        vec!["--unknown"],
        vec!["--retry-failed", "--retry-report", "/private/report.json"],
        vec!["--retry-report"],
    ] {
        let py = f.python(&args);
        let rust = f.rust(&args);
        assert_eq!(rust.status.code(), py.status.code());
        assert_eq!(rust.status.code(), Some(2));
        assert!(rust.stdout.is_empty());
        assert!(!rust.stderr.is_empty());
        assert!(!f.0.join("tmux-trace").exists());
        assert!(fs::read_dir(f.0.join("home")).unwrap().next().is_none());
    }
}
