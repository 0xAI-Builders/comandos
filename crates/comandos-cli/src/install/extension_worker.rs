//! Owned, cancelable extension phase. Only persisted before-mutation intents are
//! reloaded after the worker exits; the same HOME inode lease crosses exec.
use super::{extension_mutations::Adapter, transaction};
use std::os::unix::process::CommandExt;
use std::{
    cell::RefCell,
    path::Path,
    process::{Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};
fn operation_limit(operation: &str) -> Result<Duration, String> {
    match operation {
        "import" | "sync" => Ok(Duration::from_secs(90)),
        "agents-setup" => Ok(Duration::from_secs(30)),
        _ => Err("invalid extension worker operation".into()),
    }
}

/// Injected native execution seam. The production runner executes this in an
/// owned worker process; private Action runners may call it synchronously.
pub fn execute(home: &Path, journal: &Path, operation: &str) -> Result<(), String> {
    operation_limit(operation)?;
    let _guard = transaction::installation_lock(home)?;
    let owned = Rc::new(RefCell::new(transaction::load_active_journal(
        home, journal,
    )?));
    let adapter = Rc::new(RefCell::new(if operation == "agents-setup" {
        Adapter::agents(owned)
    } else {
        Adapter {
            journal: owned,
            agents: false,
        }
    }));
    comandos_extensions::mutations::with(adapter, || {
        if operation == "agents-setup" {
            let mut out = String::new();
            let result =
                crate::agents::setup_transaction(home, &mut out).map_err(|e| e.to_string());
            print!("{out}");
            result
        } else {
            comandos_extensions::cli::catalog_command(home, None, operation)
        }
    })
}
pub(crate) fn entry(args: &[String]) -> Result<i32, String> {
    if args.len() != 3 {
        return Err("invalid extension worker arguments".into());
    }
    let home = Path::new(&args[0]);
    let _guard = transaction::inherited_installation_lock(home)?;
    // This process watchdog also closes the lease if the parent disappears.
    // A blocked IO operation cannot continue in a detached mutation thread.
    start_watchdog(operation_limit(&args[2])?)?;
    execute(home, Path::new(&args[1]), &args[2])?;
    Ok(0)
}
fn start_watchdog(limit: Duration) -> Result<(), String> {
    let group = nix::unistd::getpgrp();
    if group != nix::unistd::getpid() {
        return Err("worker is not its owned group leader".into());
    }
    std::thread::spawn(move || {
        std::thread::sleep(limit);
        // The live leader still owns/reserves this group; terminate descendants
        // and ourselves together if the parent can no longer supervise us.
        let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
        std::process::exit(124);
    });
    Ok(())
}
pub(crate) fn run(
    home: &Path,
    journal: &Path,
    operation: &str,
    quiescent: &Rc<RefCell<bool>>,
) -> Result<(), String> {
    *quiescent.borrow_mut() = true;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = Command::new(me);
    command
        .args(["install", "--extension-worker"])
        .arg(home)
        .arg(journal)
        .arg(operation);
    supervise_with_ack(home, &mut command, operation_limit(operation)?, quiescent)
}
#[cfg(test)]
fn supervise(home: &Path, command: &mut Command, limit: Duration) -> Result<(), String> {
    supervise_with_ack(home, command, limit, &Rc::new(RefCell::new(false)))
}
fn supervise_with_ack(
    home: &Path,
    command: &mut Command,
    limit: Duration,
    quiescent: &Rc<RefCell<bool>>,
) -> Result<(), String> {
    *quiescent.borrow_mut() = true; // No worker exists on pre-spawn errors.
    let guard = transaction::installation_lock(home)?;
    command.env("HOME", home).current_dir(home);
    command
        .stdin(Stdio::from(guard.try_clone().map_err(|e| e.to_string())?))
        .process_group(0);
    let deadline = Instant::now() + limit;
    let spawned = command.spawn();
    // Close Command's parent-side clone on spawn errors as well as success.
    command.stdin(Stdio::null());
    let mut child = spawned.map_err(|e| e.to_string())?;
    *quiescent.borrow_mut() = false;
    loop {
        let observed = comandos_runtime::procs::child_exited_unreaped(&child);
        if observed
            .as_ref()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::Interrupted)
        {
            continue;
        }
        if observed
            .as_ref()
            .is_err_and(|e| e.raw_os_error() == Some(nix::libc::ECHILD))
        {
            // Ownership was lost to another reaper: never signal a reused group.
            return Err(
                "owned worker was reaped outside supervisor; group quiescence unproved".into(),
            );
        }
        let timed_out = Instant::now() >= deadline;
        if observed.as_ref().is_ok_and(|exited| *exited) || observed.is_err() || timed_out {
            // WNOWAIT preserves the leader PID through this signal. No killpg
            // is permitted after wait/reap, including normal success/error exits.
            let killed = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(child.id() as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
            if let Err(error) = killed
                && error != nix::errno::Errno::ESRCH
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("owned worker group quiescence unproved: {error}"));
            }
            let status = child.wait().map_err(|e| e.to_string())?;
            *quiescent.borrow_mut() = true;
            if let Err(error) = observed {
                return Err(error.to_string());
            }
            if timed_out {
                return Err("extension phase exceeded its 90-second deadline; owned worker killed and reaped".into());
            }
            return if status.success() {
                Ok(())
            } else {
                Err(format!("extension worker failed: {status}"))
            };
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    #[test]
    fn admitted_operation_deadlines_preserve_installer_contract() {
        assert_eq!(
            operation_limit("agents-setup").unwrap(),
            Duration::from_secs(30)
        );
        for operation in ["import", "sync"] {
            assert_eq!(operation_limit(operation).unwrap(), Duration::from_secs(90));
        }
        assert!(operation_limit("unknown").is_err());
    }
    #[test]
    fn worker_fixture() {
        let Some(home) = std::env::var_os("COMANDOS_TEST_EXTENSION_WORKER_HOME") else {
            return;
        };
        let home = std::path::PathBuf::from(home);
        let journal = std::path::PathBuf::from(
            std::env::var_os("COMANDOS_TEST_EXTENSION_WORKER_JOURNAL").unwrap(),
        );
        assert_eq!(std::env::current_dir().unwrap(), home);
        assert_eq!(std::env::var_os("HOME").unwrap(), home.as_os_str());
        let _guard = transaction::inherited_installation_lock(&home).unwrap();
        let owned = Rc::new(RefCell::new(
            transaction::load_active_journal(&home, &journal).unwrap(),
        ));
        let adapter = Rc::new(RefCell::new(Adapter {
            journal: owned,
            agents: false,
        }));
        comandos_extensions::mutations::with(adapter, || {
            comandos_extensions::config::save_json(
                &home.join(".config/comandos/extensions/snapshot.json"),
                &serde_json::json!({"owned":"write"}),
            )?;
            fs::write(home.join("worker-ready"), b"ready").map_err(|e| e.to_string())?;
            std::thread::sleep(Duration::from_secs(60));
            fs::write(home.join("late-write"), b"must never happen").map_err(|e| e.to_string())?;
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn timeout_reaps_worker_before_reload_and_restores_only_durable_intents() {
        let mut random = [0; 8];
        getrandom::fill(&mut random).unwrap();
        let home =
            std::env::temp_dir().join(format!("extension-worker-{:x}", u64::from_ne_bytes(random)));
        fs::create_dir(&home).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        let path = home.join(".config/comandos/extensions/snapshot.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"original private bytes").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let mut journal = transaction::Journal::default();
        journal.durable(&home).unwrap();
        let durable = journal.durable_path().unwrap().to_path_buf();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "install::extension_worker::tests::worker_fixture",
                "--nocapture",
            ])
            .env("COMANDOS_TEST_EXTENSION_WORKER_HOME", &home)
            .env("COMANDOS_TEST_EXTENSION_WORKER_JOURNAL", &durable);
        let error = supervise(&home, &mut command, Duration::from_millis(700)).unwrap_err();
        assert!(error.contains("killed and reaped"), "{error}");
        assert!(home.join("worker-ready").exists());
        journal.reload(&home).unwrap();
        journal.rollback().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"original private bytes");
        std::thread::sleep(Duration::from_millis(50));
        assert!(!home.join("late-write").exists());
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn child_keeps_home_lease_after_parent_guard_drops() {
        let mut random = [0; 8];
        getrandom::fill(&mut random).unwrap();
        let home =
            std::env::temp_dir().join(format!("extension-lease-{:x}", u64::from_ne_bytes(random)));
        fs::create_dir(&home).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        let mut journal = transaction::Journal::default();
        journal.durable(&home).unwrap();
        let guard = transaction::installation_lock(&home).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "install::extension_worker::tests::worker_fixture",
                "--nocapture",
            ])
            .env("COMANDOS_TEST_EXTENSION_WORKER_HOME", &home)
            .env(
                "COMANDOS_TEST_EXTENSION_WORKER_JOURNAL",
                journal.durable_path().unwrap(),
            )
            .env("HOME", &home)
            .current_dir(&home)
            .stdin(Stdio::from(guard.try_clone().unwrap()))
            .process_group(0);
        let mut child = command.spawn().unwrap();
        command.stdin(Stdio::null());
        drop(guard);
        let deadline = Instant::now() + Duration::from_secs(3);
        while !home.join("worker-ready").exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let contender = fs::File::open(&home).unwrap();
        assert!(matches!(
            contender.try_lock(),
            Err(fs::TryLockError::WouldBlock)
        ));
        nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(child.id() as i32),
            nix::sys::signal::Signal::SIGKILL,
        )
        .unwrap();
        child.wait().unwrap();
        contender.try_lock().unwrap();
        drop(contender);
        journal.reload(&home).unwrap();
        journal.rollback().unwrap();
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn failed_worker_journal_reload_retains_manifest_and_never_rolls_back_stale_intents() {
        let mut random = [0; 8];
        getrandom::fill(&mut random).unwrap();
        let home =
            std::env::temp_dir().join(format!("extension-reload-{:x}", u64::from_ne_bytes(random)));
        fs::create_dir(&home).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        let mut journal = transaction::Journal::default();
        journal.durable(&home).unwrap();
        let path = journal.durable_path().unwrap().to_path_buf();
        fs::write(path.join("manifest.json"), b"invalid interrupted fixture").unwrap();
        assert!(journal.reload(&home).is_err());
        let error = journal.rollback().unwrap_err();
        assert!(error.contains("retained recovery journal"));
        assert_eq!(
            fs::read(path.join("manifest.json")).unwrap(),
            b"invalid interrupted fixture"
        );
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn late_descendant_fixture() {
        let Some(home) = std::env::var_os("COMANDOS_TEST_EXITED_LEADER_HOME") else {
            return;
        };
        let home = std::path::PathBuf::from(home);
        fs::write(home.join("descendant-ready"), b"ready").unwrap();
        std::thread::sleep(Duration::from_millis(1000));
        fs::write(home.join("descendant-late-write"), b"must not happen").unwrap();
    }
    #[test]
    fn exited_leader_fixture() {
        let Some(home) = std::env::var_os("COMANDOS_TEST_EXITED_LEADER_HOME") else {
            return;
        };
        let home = std::path::PathBuf::from(home);
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "install::extension_worker::tests::late_descendant_fixture",
                "--nocapture",
            ])
            .stdin(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !home.join("descendant-ready").exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(child); // The supervisor owns this inherited group, not this leader.
        if std::env::var("COMANDOS_TEST_LEADER_EXIT").unwrap() == "watchdog" {
            start_watchdog(Duration::from_millis(150)).unwrap();
            std::thread::sleep(Duration::from_secs(60));
        }
        if std::env::var("COMANDOS_TEST_LEADER_EXIT").unwrap() == "17" {
            std::process::exit(17);
        }
    }
    #[test]
    fn exited_success_and_error_leaders_cannot_leave_a_late_mutating_descendant() {
        for code in ["0", "17", "watchdog"] {
            let mut random = [0; 8];
            getrandom::fill(&mut random).unwrap();
            let home = std::env::temp_dir().join(format!(
                "exited-extension-leader-{:x}",
                u64::from_ne_bytes(random)
            ));
            fs::create_dir(&home).unwrap();
            fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "install::extension_worker::tests::exited_leader_fixture",
                    "--nocapture",
                ])
                .env("COMANDOS_TEST_EXITED_LEADER_HOME", &home)
                .env("COMANDOS_TEST_LEADER_EXIT", code);
            let result = supervise(&home, &mut command, Duration::from_secs(3));
            assert_eq!(result.is_ok(), code == "0");
            assert!(home.join("descendant-ready").exists());
            std::thread::sleep(Duration::from_millis(1200));
            assert!(
                !home.join("descendant-late-write").exists(),
                "exited leader {code} left a mutating descendant"
            );
            fs::File::open(&home).unwrap().try_lock().unwrap();
            fs::remove_dir_all(home).unwrap();
        }
    }
    #[test]
    fn unknown_worker_quiescence_retains_journal_and_prevents_file_rollback() {
        let mut random = [0; 8];
        getrandom::fill(&mut random).unwrap();
        let home = std::env::temp_dir().join(format!(
            "worker-quiescence-{:x}",
            u64::from_ne_bytes(random)
        ));
        fs::create_dir(&home).unwrap();
        let path = home.join("owned-file");
        fs::write(&path, b"before").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let mut journal = transaction::Journal::default();
        journal.file(&path).unwrap();
        journal.durable(&home).unwrap();
        journal.expect_file(&path, b"written", 0o600).unwrap();
        fs::write(&path, b"written").unwrap();
        let durable = journal.durable_path().unwrap().to_path_buf();
        journal.refuse_rollback("owned worker group quiescence unproved");
        assert!(
            journal
                .rollback()
                .unwrap_err()
                .contains("retained recovery journal")
        );
        assert!(durable.is_dir());
        assert_eq!(fs::read(path).unwrap(), b"written");
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn failed_spawn_acknowledges_no_worker_and_drops_command_home_lease() {
        let mut random = [0; 8];
        getrandom::fill(&mut random).unwrap();
        let home = std::env::temp_dir().join(format!(
            "worker-spawn-failure-{:x}",
            u64::from_ne_bytes(random)
        ));
        fs::create_dir(&home).unwrap();
        let mut command = Command::new(home.join("absent-owned-program"));
        let ack = Rc::new(RefCell::new(false));
        assert!(supervise_with_ack(&home, &mut command, Duration::from_secs(1), &ack).is_err());
        assert!(*ack.borrow());
        fs::File::open(&home).unwrap().try_lock().unwrap();
        fs::remove_dir_all(home).unwrap();
    }
}
