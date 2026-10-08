//! Real native actor, never an oracle recording: private PTY and owned child group.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]
use nix::{
    fcntl::{FcntlArg, OFlag, fcntl},
    sys::{
        signal::{Signal, killpg},
        wait::{Id, WaitPidFlag, WaitStatus, waitid},
    },
    unistd::Pid,
};
use std::{
    io::{ErrorKind, Read},
    os::fd::AsFd,
    path::Path,
    process::Child,
    time::{Duration, Instant},
};

struct OwnedActor(Child);
impl OwnedActor {
    fn observe(&self) -> nix::Result<WaitStatus> {
        waitid(
            Id::Pid(Pid::from_raw(i32::try_from(self.0.id()).unwrap())),
            WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT,
        )
    }
}
impl Drop for OwnedActor {
    fn drop(&mut self) {
        // WNOWAIT leaves this direct child/PID reserved until all group signals
        // finish. If ownership cannot be demonstrated, never signal a group.
        if self.observe().is_err() {
            return;
        }
        let pid = Pid::from_raw(i32::try_from(self.0.id()).unwrap());
        let _ = killpg(pid, Signal::SIGTERM);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.observe() {
                Ok(WaitStatus::Exited(..) | WaitStatus::Signaled(..)) => break,
                Err(_) => return,
                _ if Instant::now() >= deadline => {
                    let _ = killpg(pid, Signal::SIGKILL);
                    break;
                }
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        // No signals after this consuming wait.
        let _ = self.0.wait();
    }
}

pub fn attach_and_read(command: &str, home: &Path, send: impl FnOnce()) -> Vec<u8> {
    let (mut master, slave) = pty_process::blocking::open().unwrap();
    master.resize(pty_process::Size::new(24, 80)).unwrap();
    let flags = fcntl(master.as_fd(), FcntlArg::F_GETFL).unwrap();
    fcntl(
        master.as_fd(),
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .unwrap();
    let actor = OwnedActor(
        pty_process::blocking::Command::new("/bin/sh")
            .args(["-c", command])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", home)
            .env("TERM", "xterm-256color")
            .current_dir(home)
            .spawn(slave)
            .unwrap(),
    );
    std::thread::sleep(Duration::from_millis(200));
    send();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut data = Vec::new();
    let mut chunk = [0u8; 65536];
    while Instant::now() < deadline {
        match master.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                data.extend_from_slice(&chunk[..n]);
                if data.windows("雪".len()).any(|w| w == "雪".as_bytes()) {
                    break;
                }
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(_) => break,
        }
    }
    drop(actor);
    data
}
