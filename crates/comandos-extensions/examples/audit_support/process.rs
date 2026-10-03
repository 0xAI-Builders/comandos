//! Bounded development children. No shell evaluation, one child at a time.
use serde_json::json;
use std::os::unix::process::CommandExt;
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
pub const INPUT_CAP: usize = 8 * 1024 * 1024;
pub const OUTPUT_CAP: usize = 1024 * 1024;
static CHILD_OWNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[derive(Default)]
pub(super) struct OutputQuota {
    pub(super) used: usize,
    pub(super) observed: usize,
    pub(super) truncated: bool,
}
impl OutputQuota {
    pub(super) fn reserve(&mut self, bytes: usize) -> usize {
        self.observed = self.observed.saturating_add(bytes);
        let accepted = bytes.min(OUTPUT_CAP - self.used);
        self.used += accepted;
        self.truncated |= accepted < bytes;
        accepted
    }
}
fn reader<T: Read + Send + 'static>(
    mut stream: T,
    tx: mpsc::Sender<(bool, Vec<u8>, Option<String>)>,
    stdout: bool,
    quota: std::sync::Arc<std::sync::Mutex<OutputQuota>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut data = Vec::new();
        let mut buffer = [0u8; 4096];
        let error = loop {
            let n = match stream.read(&mut buffer) {
                Ok(0) => break None,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => break Some(format!("pipe read failure: {e}")),
            };
            let accepted = match quota.lock() {
                Ok(mut q) => q.reserve(n),
                Err(_) => break Some("output quota poisoned".into()),
            };
            // Reserve the combined stdout/stderr bytes before growing either Vec.
            data.extend_from_slice(&buffer[..accepted]);
            if accepted < n {
                break Some("output overflow".into());
            }
        };
        let _ = tx.send((stdout, data, error));
    })
}
pub fn run(
    program: &str,
    args: &[String],
    input: &[u8],
    deadline: Duration,
    evidence: &std::path::Path,
) -> Result<Vec<u8>, String> {
    run_inner(program, args, input, deadline, evidence, |_| Ok(()))
}
pub(super) fn run_inner(
    program: &str,
    args: &[String],
    input: &[u8],
    deadline: Duration,
    evidence: &std::path::Path,
    after_spawn: impl FnOnce(u32) -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    use std::io::Write;
    if input.len() > INPUT_CAP {
        return Err("input overflow".into());
    }
    // Also serialize fault tests when a caller forgets --test-threads=1.
    let _owner = CHILD_OWNER.lock().map_err(|_| "child owner poisoned")?;
    let mut cmd = Command::new("/usr/bin/prlimit");
    cmd.args([
        "--as=402653184",
        "--cpu=2:3",
        "--",
        "/usr/bin/time",
        "--format=\nRESOURCE cpu_user=%U cpu_system=%S rss_kib=%M status=%x",
        program,
    ])
    .args(args)
    .process_group(0)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let now = Instant::now();
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    let quota = std::sync::Arc::new(std::sync::Mutex::new(OutputQuota::default()));
    let out = reader(
        child.stdout.take().unwrap(),
        tx.clone(),
        true,
        quota.clone(),
    );
    let err = reader(child.stderr.take().unwrap(), tx, false, quota.clone());
    let bytes = input.to_vec();
    let mut stdin = child.stdin.take().unwrap();
    let writer = thread::spawn(move || stdin.write_all(&bytes));
    let mut stdout = None;
    let mut stderr = None;
    let mut failure = after_spawn(pid).err();
    let status = loop {
        while let Ok((is_out, data, error)) = rx.try_recv() {
            if error.is_some() {
                failure = error;
            }
            if data.len() > OUTPUT_CAP {
                failure = Some("output overflow".to_string());
            }
            if is_out {
                stdout = Some(data);
            } else {
                stderr = Some(data);
            }
        }
        if now.elapsed() > deadline {
            failure = Some("timeout".to_string());
        }
        if failure.is_some() {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
            break match child.wait() {
                Ok(s) => Some(s),
                Err(e) => {
                    failure = Some(format!("reap failure: {e}"));
                    None
                }
            };
        }
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(e) => {
                failure = Some(format!("reap failure: {e}"));
                break None;
            }
        }
        thread::sleep(Duration::from_millis(5));
    };
    // Stop surviving group members and close inherited pipes.
    let _ = nix::sys::signal::killpg(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGKILL,
    );
    let write_result = writer.join().map_err(|_| "stdin writer panicked")?;
    if out.join().is_err() {
        failure = Some("stdout reader panicked".into());
    }
    if err.join().is_err() {
        failure = Some("stderr reader panicked".into());
    }
    for (is_out, data, error) in rx.try_iter() {
        if error.is_some() {
            failure = error;
        }
        if is_out {
            stdout = Some(data);
        } else {
            stderr = Some(data);
        }
    }
    let stdout = stdout.unwrap_or_default();
    let stderr = stderr.unwrap_or_default();
    let quota = quota.lock().map_err(|_| "output quota poisoned")?;
    if quota.truncated || stdout.len() + stderr.len() > OUTPUT_CAP {
        failure = Some("output overflow".to_string());
    }
    if failure.is_none()
        && status
            .as_ref()
            .is_some_and(std::process::ExitStatus::success)
        && let Err(e) = write_result
    {
        failure = Some(format!("stdin write failure: {e}"));
    }
    std::fs::write(evidence.with_extension("stdout"), &stdout).map_err(|e| e.to_string())?;
    std::fs::write(evidence.with_extension("stderr"), &stderr).map_err(|e| e.to_string())?;
    std::fs::write(evidence.with_extension("json"),serde_json::to_vec_pretty(&json!({"program":program,"args":args,"input_bytes":input.len(),"stdout_bytes":stdout.len(),"stderr_bytes":stderr.len(),"combined_retained_bytes":quota.used,"combined_observed_bytes":quota.observed,"output_truncated":quota.truncated,"status":status.as_ref().map(ToString::to_string),"failure":failure,"elapsed_ms":now.elapsed().as_millis(),"reaped":status.is_some(),"caps":{"as":402653184,"cpu_soft":2,"cpu_hard":3,"deadline_ms":deadline.as_millis(),"output_combined":OUTPUT_CAP,"input":INPUT_CAP}})).unwrap()).map_err(|e|e.to_string())?;
    if let Some(e) = failure {
        return Err(e);
    }
    let status = status.ok_or("missing child status")?;
    if !status.success() {
        return Err(format!("child exit {status}"));
    }
    Ok(stdout)
}
