//! Native maintenance against injected control replies; Python oracle uses the same replies.
use comandos_cli::codex::release::{Client, release_with};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::Command,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};
const P: &str = "11111111-1111-1111-1111-111111111111";
const C: &str = "22222222-2222-2222-2222-222222222222";
const L: &str = "33333333-3333-3333-3333-333333333333";
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "release-c5-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        for name in [
            "home",
            "home/sessions",
            "home/archived_sessions",
            "bin",
            "tmp",
            "config",
            "data",
            "state",
            "cache",
            "runtime",
        ] {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(p.join(name))
                .unwrap();
        }
        Self(p)
    }
    fn plan(&self) -> Value {
        json!({"sid":P,"home":self.0.join("home"),"binary":self.0.join("bin/codex"),"transcript":self.0.join("home/sessions/parent.jsonl")})
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Fake {
    answers: Vec<Value>,
    trace: Rc<RefCell<Vec<Value>>>,
}
impl Client for Fake {
    fn call(&mut self, m: &str, p: Value) -> Result<Value, String> {
        self.trace.borrow_mut().push(json!([m, p]));
        let v = self.answers.remove(0);
        if let Some(e) = v.get("error") {
            Err(e.as_str().unwrap().into())
        } else {
            Ok(v)
        }
    }
}
fn oracle(f: &Fixture, plan: &Value, answers: &[Value], locked: &[bool]) -> Value {
    fs::write(
        f.0.join("release.py"),
        include_bytes!("../../../lib/codex_thread_release.py"),
    )
    .unwrap();
    let input = json!({"plan":plan,"answers":answers,"locks":locked});
    fs::write(f.0.join("input.json"), serde_json::to_vec(&input).unwrap()).unwrap();
    let script = "import importlib.util,json,sys\ns=importlib.util.spec_from_file_location('release',sys.argv[1]);m=importlib.util.module_from_spec(s);s.loader.exec_module(m)\nd=json.load(open(sys.argv[2]));p=d['plan'];a=d['answers'];locks=iter(d['locks']);trace=[];check=[]\nclass Fake:\n def __init__(self,p):pass\n def call(self,method,params):\n  trace.append([method,params]);v=a.pop(0)\n  if 'error' in v:raise RuntimeError(v['error'])\n  return v\n def close(self):pass\nm.ControlClient=Fake;m.writer_locked=lambda *args:next(locks)\ntry:r=m.release_writer(p,lambda:check.append(json.loads(json.dumps(p))));e=None\nexcept Exception as x:r=None;e=str(x)\nprint(json.dumps({'result':r,'error':e,'plan':p,'trace':trace,'checkpoints':check},ensure_ascii=False))";
    let mut c = Command::new("/usr/bin/python3");
    c.env_clear()
        .args(["-I", "-c", script])
        .arg(f.0.join("release.py"))
        .arg(f.0.join("input.json"))
        .env("HOME", f.0.join("home"))
        .env("PATH", f.0.join("bin"));
    for (k, v) in [
        ("TMPDIR", "tmp"),
        ("TMP", "tmp"),
        ("TEMP", "tmp"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_RUNTIME_DIR", "runtime"),
    ] {
        c.env(k, f.0.join(v));
    }
    let out = c.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn differential(f: &Fixture, mut plan: Value, answers: Vec<Value>, locked: Vec<bool>) {
    let expected = oracle(f, &plan, &answers, &locked);
    let trace = Rc::new(RefCell::new(vec![]));
    let t = trace.clone();
    let mut locks = locked.into_iter();
    let mut checks = vec![];
    let result = release_with(
        &mut plan,
        || Ok(locks.next().unwrap()),
        || Ok(Fake { answers, trace: t }),
        |p| {
            checks.push(p.clone());
            Ok(())
        },
    );
    let actual = match result {
        Ok(r) => {
            json!({"result":r,"error":null,"plan":plan,"trace":*trace.borrow(),"checkpoints":checks})
        }
        Err(e) => {
            json!({"result":null,"error":e,"plan":plan,"trace":*trace.borrow(),"checkpoints":checks})
        }
    };
    assert_eq!(actual, expected);
}
fn thread(f: &Fixture, id: &str, archived: bool) -> Value {
    json!({"thread":{"id":id,"path":f.0.join(if archived{"home/archived_sessions"}else{"home/sessions"}).join(if id==P{"parent.jsonl"}else if id==C{"child.jsonl"}else{"late.jsonl"})}})
}
#[test]
fn free_writer_does_not_open_control_or_change_plan() {
    let f = Fixture::new();
    differential(&f, f.plan(), vec![], vec![false]);
}
#[test]
fn unknown_lock_owner_and_wrong_transcript_never_archive() {
    let f = Fixture::new();
    differential(
        &f,
        f.plan(),
        vec![json!({"data":[],"nextCursor":null})],
        vec![true],
    );
    differential(
        &f,
        f.plan(),
        vec![json!({"data":[P]}), thread(&f, C, false)],
        vec![true],
    );
}
#[test]
fn archive_restores_exact_children_and_discovers_late_child_excluding_old_history() {
    let f = Fixture::new();
    let answers = vec![
        json!({"data":[P]}),
        thread(&f, P, false),
        json!({"data":[{"id":C}]}),
        json!({"data":[{"id":L}]}),
        json!({}),
        json!({"data":[{"id":C},{"id":L}]}),
        thread(&f, C, true),
        thread(&f, C, false),
        thread(&f, P, true),
        thread(&f, P, false),
    ];
    differential(&f, f.plan(), answers, vec![true, false]);
}
#[test]
fn ambiguous_archive_reply_preserves_pending_and_restores_all() {
    let f = Fixture::new();
    let answers = vec![
        json!({"data":[P]}),
        thread(&f, P, false),
        json!({"data":[{"id":C}]}),
        json!({"data":[]}),
        json!({"error":"archive reply lost"}),
        json!({"data":[{"id":C},{"id":L}]}),
        thread(&f, C, true),
        thread(&f, C, false),
        thread(&f, L, true),
        thread(&f, L, false),
        thread(&f, P, true),
        thread(&f, P, false),
    ];
    differential(&f, f.plan(), answers, vec![true]);
}
#[test]
fn restore_failure_keeps_checkpoint_pending_and_attempts_other_threads() {
    let f = Fixture::new();
    let answers = vec![
        json!({"data":[P]}),
        thread(&f, P, false),
        json!({"data":[{"id":C}]}),
        json!({"data":[]}),
        json!({}),
        json!({"data":[{"id":C}]}),
        thread(&f, C, true),
        json!({"error":"fixture deny"}),
        thread(&f, P, true),
        thread(&f, P, false),
    ];
    differential(&f, f.plan(), answers, vec![true]);
}
#[test]
fn pending_recovery_runs_even_after_writer_lock_has_become_free() {
    let f = Fixture::new();
    let mut p = f.plan();
    p["releaseRecovery"] = json!({"pending":true,"threadIds":[C,P]});
    differential(
        &f,
        p,
        vec![
            thread(&f, C, false),
            thread(&f, P, true),
            thread(&f, P, false),
        ],
        vec![false],
    );
}
#[test]
fn repeated_pagination_cursor_rejects_before_archive() {
    let f = Fixture::new();
    differential(
        &f,
        f.plan(),
        vec![
            json!({"data":[],"nextCursor":"same"}),
            json!({"data":[],"nextCursor":"same"}),
        ],
        vec![true],
    );
}
