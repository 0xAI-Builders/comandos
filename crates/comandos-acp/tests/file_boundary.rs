//! Incoming file RPCs must return without opening a blocking stream.
#![cfg(unix)]
use comandos_acp::protocol::{Quiet, Session, Timeouts};
use nix::{sys::stat::Mode, unistd::mkfifo};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, symlink},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "acp-file-boundary-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        fs::write(root.join("peer.py"), r#"import sys,json
from pathlib import Path
root=Path(sys.argv[1]); kind=sys.argv[2]; path=sys.argv[3]
def send(v): print(json.dumps(v),flush=True)
for line in sys.stdin:
 m=json.loads(line); method=m.get('method'); rid=m.get('id')
 if method=='initialize': r={'protocolVersion':1,'agentCapabilities':{}}
 elif method=='session/new': r={'sessionId':'owned-file-session'}
 elif method=='session/prompt':
  send({'jsonrpc':'2.0','id':'owned-file-rpc','method':'fs/'+kind+'_text_file','params':{'sessionId':'owned-file-session','path':path,'content':'replacement'}})
  answer=json.loads(sys.stdin.readline()); (root/'answer.json').write_text(json.dumps(answer)); r={'stopReason':'end_turn'}
 elif method=='session/cancel': continue
 else: continue
 send({'jsonrpc':'2.0','id':rid,'result':r})
"#).unwrap();
        Self(root)
    }
    fn call(&self, kind: &str, path: &std::path::Path) -> (Result<String, String>, Duration) {
        let mut session = Session::open(
            &[
                "/usr/bin/python3".into(),
                self.0.join("peer.py").to_string_lossy().into(),
                self.0.to_string_lossy().into(),
                kind.into(),
                path.to_string_lossy().into(),
            ],
            &BTreeMap::new(),
            &self.0,
            false,
            Timeouts {
                prompt: Duration::from_millis(100),
                ..Timeouts::default()
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        session.initialize(&mut Quiet).unwrap();
        session.start("", &mut Quiet).unwrap();
        let begin = Instant::now();
        let result = session.prompt("private file boundary", &mut Quiet);
        let elapsed = begin.elapsed();
        drop(session);
        (result, elapsed)
    }
    fn answer(&self) -> Value {
        serde_json::from_slice(&fs::read(self.0.join("answer.json")).unwrap()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fifo_is_rejected(kind: &str, through_symlink: bool) {
    let fixture = Fixture::new();
    let fifo = fixture.0.join("own.fifo");
    mkfifo(&fifo, Mode::S_IRUSR | Mode::S_IWUSR).unwrap();
    let path = if through_symlink {
        let link = fixture.0.join("own.link");
        symlink(&fifo, &link).unwrap();
        link
    } else {
        fifo.clone()
    };
    // A bounded, private release makes the failing implementation safe to run.
    let reading = kind == "read";
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(350));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(nix::libc::O_NONBLOCK)
            .open(fifo)
            .unwrap();
        if reading {
            file.write_all(b"private release").unwrap();
        }
        thread::sleep(Duration::from_millis(20));
    });
    let (result, elapsed) = fixture.call(kind, &path);
    release.join().unwrap();
    assert!(
        elapsed < Duration::from_millis(200),
        "file RPC blocked prompt deadline: {elapsed:?}, {result:?}"
    );
    assert!(
        result.is_ok(),
        "peer must receive the file error and finish: {result:?}"
    );
    assert!(
        fixture.answer().get("error").is_some(),
        "nonregular file RPC must be rejected"
    );
}

#[test]
fn fifo_read_keeps_rpc_deadline_live() {
    fifo_is_rejected("read", false);
}
#[test]
fn fifo_write_keeps_rpc_deadline_live() {
    fifo_is_rejected("write", false);
}
#[test]
fn fifo_symlink_read_keeps_rpc_deadline_live() {
    fifo_is_rejected("read", true);
}
#[test]
fn fifo_symlink_write_keeps_rpc_deadline_live() {
    fifo_is_rejected("write", true);
}

#[test]
fn regular_files_symlinks_and_new_parent_keep_text_contract() {
    let fixture = Fixture::new();
    let path = fixture.0.join("regular");
    fs::write(&path, "one\r\ntwo\rthree\n").unwrap();
    let link = fixture.0.join("regular.link");
    symlink(&path, &link).unwrap();
    for p in [&path, &link] {
        assert!(fixture.call("read", p).0.is_ok());
        assert_eq!(fixture.answer()["result"]["content"], "one\ntwo\nthree\n");
    }
    assert!(fixture.call("write", &link).0.is_ok());
    assert_eq!(fs::read(&path).unwrap(), b"replacement");
    assert!(link.symlink_metadata().unwrap().is_symlink());
    let new = fixture.0.join("new-parent/new-file");
    assert!(fixture.call("write", &new).0.is_ok());
    assert_eq!(fs::read(new).unwrap(), b"replacement");
}
