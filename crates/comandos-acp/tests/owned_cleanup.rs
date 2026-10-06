//! Private adapter wrappers must not leave descendants holding protocol pipes.
#![cfg(target_os = "linux")]
use comandos_acp::protocol::{Quiet, Session, Timeouts};
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct Probe(PathBuf);
impl Probe {
    fn new(exit_first: bool) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "acp-owned-probe-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        fs::write(
            root.join("child.py"),
            r#"import os,sys,json,time,signal
from pathlib import Path
signal.signal(signal.SIGTERM,signal.SIG_IGN)
p=Path(sys.argv[1]);pid=os.getpid()
birth=Path('/proc/'+str(pid)+'/stat').read_text().rsplit(') ',1)[1].split()[19]
(p/'child.json').write_text(json.dumps({'pid':pid,'birth':birth,'pgid':os.getpgrp()}))
time.sleep(30)
"#,
        )
        .unwrap();
        fs::write(
            root.join("leader.py"),
            format!(
                r#"import sys,subprocess,time
from pathlib import Path
p=Path(sys.argv[1]);subprocess.Popen([sys.executable,str(p/'child.py'),str(p)])
while not (p/'child.json').exists():time.sleep(.001)
{}
"#,
                if exit_first {
                    "sys.exit(0)"
                } else {
                    "for line in sys.stdin:pass\ntime.sleep(30)"
                }
            ),
        )
        .unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self(root)
    }
    fn session(&self) -> Session {
        Session::open(
            &[
                "/usr/bin/python3".into(),
                self.0.join("leader.py").to_string_lossy().into(),
                self.0.to_string_lossy().into(),
            ],
            &BTreeMap::new(),
            &self.0,
            false,
            Timeouts {
                initialize: Duration::from_millis(250),
                ..Timeouts::default()
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap()
    }
    fn receipt(&self) -> Value {
        let limit = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(bytes) = fs::read(self.0.join("child.json"))
                && let Ok(value) = serde_json::from_slice(&bytes)
            {
                return value;
            }
            assert!(Instant::now() < limit, "private adapter did not start");
            thread::sleep(Duration::from_millis(2));
        }
    }
    fn running(&self, receipt: &Value) -> bool {
        let pid = receipt["pid"].as_i64().unwrap();
        let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        let fields: Vec<_> = stat
            .rsplit_once(") ")
            .unwrap()
            .1
            .split_whitespace()
            .collect();
        if fields[19] != receipt["birth"].as_str().unwrap() {
            return false;
        }
        fields[0] != "Z"
    }
    fn assert_closed(&self, receipt: &Value) {
        let limit = Instant::now() + Duration::from_secs(1);
        while self.running(receipt) && Instant::now() < limit {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            !self.running(receipt),
            "owned adapter descendant retained protocol pipes after close"
        );
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        if let Ok(bytes) = fs::read(self.0.join("child.json"))
            && let Ok(receipt) = serde_json::from_slice::<Value>(&bytes)
            && self.running(&receipt)
        {
            let pid = receipt["pid"].as_i64().unwrap();
            let argv = fs::read(format!("/proc/{pid}/cmdline")).unwrap();
            assert!(
                argv.split(|b| *b == 0)
                    .any(|a| a == self.0.join("child.py").as_os_str().as_encoded_bytes())
            );
            let _ = kill(Pid::from_raw(i32::try_from(pid).unwrap()), Signal::SIGKILL);
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn dropping_transport_closes_descendants_even_after_leader_exit() {
    let probe = Probe::new(true);
    let session = probe.session();
    let receipt = probe.receipt();
    thread::sleep(Duration::from_millis(30));
    drop(session);
    probe.assert_closed(&receipt);
}

#[test]
fn dropping_transport_kills_descendants_that_ignore_term() {
    let probe = Probe::new(false);
    let session = probe.session();
    let receipt = probe.receipt();
    drop(session);
    probe.assert_closed(&receipt);
}

#[test]
fn failed_initialize_preserves_group_ownership_until_cleanup() {
    let probe = Probe::new(true);
    let mut session = probe.session();
    let receipt = probe.receipt();
    assert!(session.initialize(&mut Quiet).is_err());
    drop(session);
    probe.assert_closed(&receipt);
}
