#![allow(clippy::unwrap_used)]
use comandos_desktop::proc::{ProcSpec, run_when};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[test]
fn closing_owner_interrupts_only_its_spawned_child_and_joins_pipes() {
    let allowed = Arc::new(AtomicBool::new(true));
    let worker_flag = allowed.clone();
    let start = Instant::now();
    let worker = std::thread::spawn(move || {
        run_when(
            &ProcSpec {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "printf own; sleep 10".into()],
                stdin: None,
                env: vec![],
                clear_env: false,
                env_remove: vec![],
                cwd: None,
                timeout: Duration::from_secs(20),
            },
            &|| worker_flag.load(Ordering::Acquire),
        )
    });
    std::thread::sleep(Duration::from_millis(100));
    allowed.store(false, Ordering::Release);
    let error = worker.join().unwrap().unwrap_err();
    assert!(
        matches!(error,comandos_desktop::proc::ProcError::Spawn(ref message) if message=="cancelled")
    );
    assert!(start.elapsed() < Duration::from_secs(2));
}
#[test]
fn cancelled_before_spawn_never_runs_the_command() {
    let out = run_when(
        &ProcSpec {
            program: "/does/not/exist".into(),
            args: vec![],
            stdin: None,
            env: vec![],
            clear_env: false,
            env_remove: vec![],
            cwd: None,
            timeout: Duration::from_secs(1),
        },
        &|| false,
    )
    .unwrap_err();
    assert!(
        matches!(out,comandos_desktop::proc::ProcError::Spawn(ref message) if message=="cancelled")
    );
}

#[test]
fn completed_leader_cannot_leave_owned_descendants_holding_pipes() {
    let start = Instant::now();
    let result = run_when(
        &ProcSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 10 & printf owned; exit 7".into()],
            stdin: None,
            env: vec![],
            clear_env: false,
            env_remove: vec![],
            cwd: None,
            timeout: Duration::from_secs(5),
        },
        &|| true,
    )
    .unwrap();
    assert_eq!(result.code, Some(7));
    assert_eq!(result.stdout, b"owned");
    assert!(!result.timed_out);
    assert!(start.elapsed() < Duration::from_secs(2));
}
