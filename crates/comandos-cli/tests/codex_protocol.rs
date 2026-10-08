//! Owned Python fake stdio proxy only; no real account, socket connection or daemon.
use comandos_cli::codex::{protocol::Control, release::Client};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
const SCRIPT: &str = r#"#!/usr/bin/python3
import json,sys,os,time
from pathlib import Path
r=Path(__file__).parent
(r/'pid').write_text(str(os.getpid()))
mode=(r/'mode').read_text()
with (r/'trace').open('a') as log:
 for line in sys.stdin:
  m=json.loads(line);log.write(json.dumps(m)+'\n');log.flush()
  if m.get('method')=='initialized':continue
  if 'method' not in m:continue
  if mode=='eof':sys.stdout.write('{"id":');sys.stdout.flush();sys.exit()
  if mode=='invalid':print('not-json',flush=True);continue
  if mode=='oversize':sys.stdout.write('x'*16000001);sys.stdout.flush();time.sleep(60)
  if mode=='stall':time.sleep(60)
  if m['method']=='initialize':
   print(json.dumps({'method':'approval','id':99,'params':{'command':'forbidden'}}),flush=True)
   print(json.dumps({'method':'notice','params':{}}),flush=True)
   print(json.dumps({'id':999999,'result':'stale'}),flush=True)
  print(json.dumps({'id':m['id'],'result':{'ok':True}}),flush=True)
"#;
struct Fixture(PathBuf);
impl Fixture {
    fn new(mode: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let r = std::env::temp_dir().join(format!(
            "codex-rpc-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&r).unwrap();
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(r.join("home/app-server-control"))
            .unwrap();
        fs::write(
            r.join("home/app-server-control/app-server-control.sock"),
            b"fake only",
        )
        .unwrap();
        fs::write(r.join("mode"), mode).unwrap();
        fs::write(r.join("proxy"), SCRIPT).unwrap();
        fs::set_permissions(r.join("proxy"), fs::Permissions::from_mode(0o700)).unwrap();
        Self(r)
    }
    fn plan(&self) -> Value {
        json!({"sid":"11111111-1111-1111-1111-111111111111","home":self.0.join("home"),"binary":self.0.join("proxy"),"transcript":self.0.join("home/sessions/x.jsonl")})
    }
    fn gone(&self) {
        let pid = fs::read_to_string(self.0.join("pid")).unwrap();
        assert!(
            !PathBuf::from("/proc").join(pid).exists(),
            "owned proxy not reaped"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn inbound_requests_are_denied_and_wrong_ids_notifications_ignored() {
    let f = Fixture::new("good");
    let mut c = Control::open(
        &f.plan(),
        Duration::from_secs(2),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(
        c.call("probe", json!({"literal":"雪 '$()"})).unwrap(),
        json!({"ok":true})
    );
    drop(c);
    f.gone();
    let trace = fs::read_to_string(f.0.join("trace"))
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str::<Value>(s).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        trace[1],
        json!({"id":99,"error":{"code":-32601,"message":"This maintenance client cannot approve or execute requests"}})
    );
    assert_eq!(trace[2], json!({"method":"initialized"}));
    assert_eq!(trace[3]["params"]["literal"], "雪 '$()");
}
#[test]
fn incomplete_eof_invalid_json_and_limit_fail_and_reap_owned_proxy() {
    for mode in ["eof", "invalid", "oversize"] {
        let f = Fixture::new(mode);
        let start = Instant::now();
        let e = Control::open(
            &f.plan(),
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        )
        .err()
        .unwrap();
        assert!(!e.is_empty());
        assert!(start.elapsed() < Duration::from_secs(3));
        f.gone();
    }
}
#[test]
fn cancellation_and_deadline_close_only_the_owned_proxy() {
    for cancel in [false, true] {
        let f = Fixture::new("stall");
        let flag = Arc::new(AtomicBool::new(false));
        let trigger = flag.clone();
        let t = cancel.then(|| {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                trigger.store(true, Ordering::SeqCst);
            })
        });
        let start = Instant::now();
        let e = Control::open(&f.plan(), Duration::from_millis(150), flag)
            .err()
            .unwrap();
        assert!(e.contains(if cancel { "cancelado" } else { "no respondió" }));
        assert!(start.elapsed() < Duration::from_secs(1));
        if let Some(t) = t {
            t.join().unwrap();
        }
        f.gone();
    }
}
#[test]
fn malformed_identity_and_missing_existing_control_fail_before_spawning() {
    let f = Fixture::new("good");
    let mut p = f.plan();
    p["sid"] = json!("last");
    assert!(Control::open(&p, Duration::from_secs(1), Arc::new(AtomicBool::new(false))).is_err());
    p = f.plan();
    fs::remove_file(f.0.join("home/app-server-control/app-server-control.sock")).unwrap();
    assert!(Control::open(&p, Duration::from_secs(1), Arc::new(AtomicBool::new(false))).is_err());
    assert!(!f.0.join("pid").exists());
}
