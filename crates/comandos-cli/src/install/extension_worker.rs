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
const LIMIT: Duration = Duration::from_secs(90);

/// Injected native execution seam. The production runner executes this in an
/// owned worker process; private Action runners may call it synchronously.
pub fn execute(home: &Path, journal: &Path, operation: &str) -> Result<(), String> {
    if !matches!(operation, "import" | "sync") {
        return Err("invalid extension worker operation".into());
    }
    let _guard = transaction::installation_lock(home)?;
    let owned = Rc::new(RefCell::new(transaction::load_active_journal(
        home, journal,
    )?));
    let adapter = Rc::new(RefCell::new(Adapter { journal: owned }));
    comandos_extensions::mutations::with(adapter, || {
        comandos_extensions::cli::catalog_command(home, None, operation)
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
    std::thread::spawn(|| {
        std::thread::sleep(LIMIT);
        std::process::exit(124);
    });
    execute(home, Path::new(&args[1]), &args[2])?;
    Ok(0)
}
pub(crate) fn run(home: &Path, journal: &Path, operation: &str) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = Command::new(me);
    command
        .args(["install", "--extension-worker"])
        .arg(home)
        .arg(journal)
        .arg(operation);
    supervise(home, &mut command, LIMIT)
}
fn supervise(home: &Path, command: &mut Command, limit: Duration) -> Result<(), String> {
    let guard = transaction::installation_lock(home)?;
    command.env("HOME", home).current_dir(home);
    command
        .stdin(Stdio::from(guard.try_clone().map_err(|e| e.to_string())?))
        .process_group(0);
    let deadline = Instant::now() + limit;
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    // Command retains configured Stdio after spawn. Drop its parent-side clone
    // while the child keeps the inherited open description and this guard lives.
    command.stdin(Stdio::null());
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("extension worker failed: {status}")),
            Ok(None) => {}
            Err(e) => {
                let _ = nix::sys::signal::killpg(
                    nix::unistd::Pid::from_raw(child.id() as i32),
                    nix::sys::signal::Signal::SIGKILL,
                );
                child
                    .wait()
                    .map_err(|wait| format!("worker wait failed after {e}: {wait}"))?;
                return Err(e.to_string());
            }
        }
        if Instant::now() >= deadline {
            let killed = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(child.id() as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
            if killed.is_err() {
                let _ = child.kill();
            }
            child.wait().map_err(|e| e.to_string())?;
            return Err(
                "extension phase exceeded its 90-second deadline; owned worker killed and reaped"
                    .into(),
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
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
        let adapter = Rc::new(RefCell::new(Adapter { journal: owned }));
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
}
