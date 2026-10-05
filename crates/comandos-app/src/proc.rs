//! Único sitio donde se lanzan procesos (clippy `disallowed-methods`). Lee stdout
//! y stderr a la vez y escribe stdin desde otro hilo: una salida de más de 64 KiB
//! (el búfer de una tubería) nunca bloquea al hijo (F08).
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Tope de lo que se guarda de cada salida.
pub const OUTPUT_CAP: usize = 16 << 20;

#[derive(Debug, Clone)]
pub struct ProcSpec {
    pub program: String,
    pub args: Vec<OsString>,
    pub stdin: Option<Vec<u8>>,
    pub env: Vec<(String, OsString)>,
    pub clear_env: bool,
    pub env_remove: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct ProcOutput {
    /// `None` si murió por señal o por el plazo.
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub truncated: bool,
}

#[derive(Debug)]
pub enum ProcError {
    Spawn(String),
}

fn command(program: &str) -> Command {
    // Única llamada permitida (con `spawn_detached`).
    #[allow(clippy::disallowed_methods)]
    Command::new(program)
}

fn drain(mut src: impl Read, deadline: Instant) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    let mut truncated = false;
    loop {
        if Instant::now() >= deadline {
            break;
        }
        match src.read(&mut buf) {
            Ok(0) => break,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
            Ok(n) => {
                let room = OUTPUT_CAP.saturating_sub(out.len());
                out.extend_from_slice(buf.get(..n.min(room)).unwrap_or_default());
                truncated |= n > room;
            }
        }
    }
    (out, truncated)
}

pub fn run(spec: &ProcSpec) -> Result<ProcOutput, ProcError> {
    use std::os::unix::process::CommandExt;
    let deadline = Instant::now()
        .checked_add(spec.timeout)
        .ok_or_else(|| ProcError::Spawn("plazo fuera de rango".into()))?;
    let mut cmd = command(&spec.program);
    cmd.process_group(0);
    cmd.args(&spec.args)
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if spec.clear_env {
        cmd.env_clear();
    }
    for key in &spec.env_remove {
        cmd.env_remove(key);
    }
    for (key, value) in &spec.env {
        cmd.env(key, value);
    }
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| ProcError::Spawn(format!("{}: {e}", spec.program)))?;
    let group = nix::unistd::Pid::from_raw(
        i32::try_from(child.id()).map_err(|e| ProcError::Spawn(e.to_string()))?,
    );
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    fn nonblocking(fd: &impl std::os::fd::AsFd) -> Result<(), ProcError> {
        use nix::fcntl::{FcntlArg, OFlag, fcntl};
        let flags = fcntl(fd, FcntlArg::F_GETFL).map_err(|e| ProcError::Spawn(e.to_string()))?;
        fcntl(
            fd,
            FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
        )
        .map(|_| ())
        .map_err(|e| ProcError::Spawn(e.to_string()))
    }
    let prepared = (|| {
        if let Some(p) = &stdin {
            nonblocking(p)?;
        }
        if let Some(p) = &stdout {
            nonblocking(p)?;
        }
        if let Some(p) = &stderr {
            nonblocking(p)?;
        }
        Ok::<(), ProcError>(())
    })();
    if let Err(error) = prepared {
        let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    std::thread::scope(|scope| {
        if let (Some(mut pipe), Some(data)) = (stdin, spec.stdin.as_deref()) {
            scope.spawn(move || {
                let mut rest = data;
                while !rest.is_empty() && Instant::now() < deadline {
                    match pipe.write(rest) {
                        Ok(0) => break,
                        Ok(n) => rest = rest.get(n..).unwrap_or_default(),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(2))
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
            });
        }
        let out = stdout.map(|p| scope.spawn(move || drain(p, deadline)));
        let err = stderr.map(|p| scope.spawn(move || drain(p, deadline)));
        let mut result = ProcOutput::default();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    result.code = status.code();
                    break;
                }
                Ok(None) if Instant::now() >= deadline => {
                    let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
                    let _ = child.kill();
                    let _ = child.wait();
                    result.timed_out = true;
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                Err(_) => {
                    let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
        // También cierra los pipes heredados por nietos cuando el líder termina.
        let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
        if let Some((bytes, cut)) = out.and_then(|h| h.join().ok()) {
            result.stdout = bytes;
            result.truncated |= cut;
        }
        if let Some((bytes, cut)) = err.and_then(|h| h.join().ok()) {
            result.stderr = bytes;
            result.truncated |= cut;
        }
        Ok(result)
    })
}

/// Lanza sin esperar (`xdg-open`, `nautilus`); un hilo recoge al hijo para que no
/// quede zombi. Usa un grupo de procesos separado; no crea una sesión POSIX.
pub fn spawn_detached(program: &str, args: &[OsString]) -> Result<(), ProcError> {
    use std::os::unix::process::CommandExt;
    let mut cmd = command(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = cmd
        .spawn()
        .map_err(|e| ProcError::Spawn(format!("{program}: {e}")))?;
    std::thread::Builder::new()
        .name("comandos-reap".into())
        .spawn(move || {
            let _ = child.wait();
        })
        .map(|_| ())
        .map_err(|e| ProcError::Spawn(e.to_string()))
}
