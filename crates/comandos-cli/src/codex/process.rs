//! Bounded, noninteractive argv/stdin transport. Never invokes a shell.
use nix::{
    fcntl::{FcntlArg, OFlag, fcntl},
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use std::{
    ffi::OsStr,
    io::{self, Read, Write},
    os::{fd::AsFd, unix::process::CommandExt},
    path::Path,
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
const LIMIT: usize = 1024 * 1024;
struct OwnedChild {
    child: Child,
    finished: bool,
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.finished {
            if let Ok(id) = i32::try_from(self.child.id()) {
                let _ = killpg(Pid::from_raw(id), Signal::SIGKILL);
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
fn nonblocking(fd: impl AsFd) -> Result<(), String> {
    let flags = fcntl(&fd, FcntlArg::F_GETFL).map_err(|e| e.to_string())?;
    fcntl(
        &fd,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn read(pipe: &mut impl Read, out: &mut Vec<u8>) -> Result<bool, String> {
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => return Ok(true),
            Ok(n) => {
                if out.len().saturating_add(n) > LIMIT {
                    return Err("salida de comando supera 1 MiB".into());
                }
                out.extend_from_slice(buf.get(..n).ok_or("lectura inválida")?);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.to_string()),
        }
    }
}
pub(super) fn run_when(
    program: &Path,
    args: &[&OsStr],
    input: Option<&[u8]>,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<Output, String> {
    if input.is_some_and(|b| b.len() > LIMIT) {
        return Err("stdin supera 1 MiB".into());
    }
    let child = Command::new(program)
        .args(args)
        .process_group(0)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    let mut owned = OwnedChild {
        child,
        finished: false,
    };
    let mut stdout = owned.child.stdout.take().ok_or("stdout ausente")?;
    let mut stderr = owned.child.stderr.take().ok_or("stderr ausente")?;
    let mut stdin = owned.child.stdin.take();
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    if let Some(pipe) = &stdin {
        nonblocking(pipe)?;
    }
    let mut sent = 0;
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut out_done = false;
    let mut err_done = false;
    let deadline = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("mantenimiento cancelado".into());
        }
        if let Some(pipe) = &mut stdin {
            let bytes = input.unwrap_or_default();
            if sent < bytes.len() {
                match pipe.write(bytes.get(sent..).ok_or("stdin inválido")?) {
                    Ok(0) => return Err("stdin cerrado antes de enviar la configuración".into()),
                    Ok(n) => sent += n,
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(e) => return Err(format!("stdin: {e}")),
                }
            }
            if sent == bytes.len() {
                stdin = None;
            }
        }
        if !out_done {
            out_done = read(&mut stdout, &mut out)?;
        }
        if !err_done {
            err_done = read(&mut stderr, &mut err)?;
        }
        // Observe without reaping so the leader identity still reserves its
        // process group while closed-pipe descendants are cleaned up.
        let pid = Pid::from_raw(i32::try_from(owned.child.id()).map_err(|e| e.to_string())?);
        if out_done
            && err_done
            && stdin.is_none()
            && comandos_runtime::procs::child_exited_unreaped(&owned.child)
                .map_err(|e| e.to_string())?
        {
            killpg(pid, Signal::SIGKILL)
                .or_else(|e| {
                    if e == nix::errno::Errno::ESRCH {
                        Ok(())
                    } else {
                        Err(e)
                    }
                })
                .map_err(|e| e.to_string())?;
            let status = owned.child.wait().map_err(|e| e.to_string())?;
            owned.finished = true;
            return Ok(Output {
                status,
                stdout: out,
                stderr: err,
            });
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "{}: tiempo agotado ({} s)",
                program.display(),
                timeout.as_secs_f32()
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
