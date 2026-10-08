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
pub(super) fn run(
    program: &Path,
    args: &[&OsStr],
    input: Option<&[u8]>,
    timeout: Duration,
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
        // Keep the leader PID reserved until all group cleanup is done, even
        // when descendants closed their own stdio before the leader exited.
        if out_done && err_done && stdin.is_none() {
            let id = Pid::from_raw(i32::try_from(owned.child.id()).map_err(|e| e.to_string())?);
            let exited = comandos_runtime::procs::child_exited_unreaped(&owned.child)
                .map_err(|e| e.to_string())?;
            if exited {
                // Signal before wait/reap; the unreaped leader prevents PID
                // reuse from redirecting this signal to an unrelated group.
                let _ = killpg(id, Signal::SIGKILL);
                let status = owned.child.wait().map_err(|e| e.to_string())?;
                owned.finished = true;
                return Ok(Output {
                    status,
                    stdout: out,
                    stderr: err,
                });
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timeout_reaps_only_the_owned_child_without_a_shell() {
        let begin = Instant::now();
        let error = run(
            Path::new("/usr/bin/sleep"),
            &[OsStr::new("60")],
            None,
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(error.contains("tiempo agotado"));
        assert!(begin.elapsed() < Duration::from_secs(2));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn normal_completion_closes_owned_descendants_with_detached_stdio() {
        use std::{fs, os::unix::fs::DirBuilderExt};
        let mut nonce = [0; 8];
        getrandom::fill(&mut nonce).unwrap();
        let root = std::env::temp_dir().join(format!(
            "mobile-owned-group-{}-{}",
            std::process::id(),
            nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        fs::write(
            root.join("descendant.py"),
            r#"import sys,time
from pathlib import Path
p=Path(sys.argv[1]); (p/'ready').write_text('own child')
time.sleep(.2)
(p/'survived').write_text('owned descendant survived normal completion')
"#,
        )
        .unwrap();
        fs::write(root.join("leader.py"), r#"import sys,subprocess,time
from pathlib import Path
p=Path(sys.argv[1]); subprocess.Popen([sys.executable,str(p/'descendant.py'),str(p)],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
limit=time.monotonic()+1
while not (p/'ready').exists():
 if time.monotonic()>limit: sys.exit(99)
 time.sleep(.001)
print('owned output',flush=True)
sys.exit(7)
"#).unwrap();
        let output = run(
            Path::new("/usr/bin/python3"),
            &[root.join("leader.py").as_os_str(), root.as_os_str()],
            None,
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, b"owned output\n");
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            !root.join("survived").exists(),
            "normal completion left an owned descendant running"
        );
    }
}
