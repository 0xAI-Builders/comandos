//! Process observations. Start identities are local to their platform:
//! Linux clock ticks, Darwin UTC seconds. Never use them across platforms.
use std::path::PathBuf;
pub mod linux;
pub mod macos;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: i32,
    pub ppid: i32,
    pub start: u64,
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
}
pub trait ProcSource {
    fn snapshot(&self) -> Vec<ProcInfo>;
    fn open_files(&self, pid: i32) -> Vec<PathBuf>;
    /// Optional cwd enrichment for project attribution. An inspector needing
    /// only ancestry/argv avoids Darwin's lsof subprocess entirely.
    fn snapshot_with_cwd(&self) -> Vec<ProcInfo> {
        self.snapshot()
    }
    fn process(&self, pid: i32) -> Option<ProcInfo> {
        self.snapshot().into_iter().find(|p| p.pid == pid)
    }
}
#[cfg(target_os = "linux")]
pub fn system() -> impl ProcSource {
    linux::ProcFs::new(PathBuf::from("/proc"))
}
#[cfg(target_os = "macos")]
pub fn system() -> impl ProcSource {
    macos::PsSnapshot
}

/// Observe a child owned by the caller WITHOUT reaping it. The unreaped
/// leader reserves its PID/group until the caller closes the group and waits.
/// This is not a lookup or signal authority for arbitrary processes.
pub fn child_exited_unreaped(child: &std::process::Child) -> std::io::Result<bool> {
    let pid = i32::try_from(child.id())
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid child pid")
        })?;
    #[cfg(not(target_os = "macos"))]
    {
        use nix::{
            sys::wait::{Id, WaitPidFlag, WaitStatus, waitid},
            unistd::Pid,
        };
        waitid(
            Id::Pid(Pid::from_raw(pid)),
            WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT,
        )
        .map(|s| !matches!(s, WaitStatus::StillAlive))
        .map_err(std::io::Error::from)
    }
    #[cfg(target_os = "macos")]
    {
        use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
        let pid = Pid::from_raw(pid).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid child pid")
        })?;
        waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )
        .map(|s| s.is_some())
        .map_err(std::io::Error::from)
    }
}
