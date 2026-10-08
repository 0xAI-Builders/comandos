//! Bounded JSONL to one owned stdio proxy. Never starts or stops a daemon.
use super::{
    Result,
    release::{self, Client},
};
use nix::{
    fcntl::{FcntlArg, OFlag, fcntl},
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Write},
    os::{fd::AsFd, unix::process::CommandExt},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
const LIMIT: usize = 16_000_000;
pub struct Control {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    buffer: Vec<u8>,
    ident: u64,
    timeout: Duration,
    cancel: Arc<AtomicBool>,
}
fn nonblocking(fd: impl AsFd) -> Result<()> {
    let flags = fcntl(&fd, FcntlArg::F_GETFL).map_err(|e| e.to_string())?;
    fcntl(
        &fd,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
impl Drop for Control {
    fn drop(&mut self) {
        drop(self.input.take());
        let Ok(id) = i32::try_from(self.child.id()) else {
            let _ = self.child.kill();
            let _ = self.child.wait();
            return;
        };
        let pid = Pid::from_raw(id);
        let _ = killpg(pid, Signal::SIGTERM);
        let end = Instant::now() + Duration::from_millis(100);
        while Instant::now() < end
            && matches!(
                comandos_runtime::procs::child_exited_unreaped(&self.child),
                Ok(false)
            )
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        // Keep the leader unreaped until the last group signal reserves its identity.
        let _ = killpg(pid, Signal::SIGKILL);
        let _ = self.child.wait();
    }
}
impl Control {
    pub fn open(plan: &Value, timeout: Duration, cancel: Arc<AtomicBool>) -> Result<Self> {
        release::validate(plan)?;
        let home = Path::new(release::string(plan, "home")?);
        let sock = home.join("app-server-control/app-server-control.sock");
        if !sock.exists() {
            return Err(
                "La conversación está bloqueada y no hay control del servidor existente".into(),
            );
        }
        if cancel.load(Ordering::SeqCst) {
            return Err("mantenimiento cancelado".into());
        }
        let mut child = Command::new(release::string(plan, "binary")?)
            .args(["app-server", "proxy", "--sock"])
            .arg(sock)
            .env("CODEX_HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|e| e.to_string())?;
        // The child is owned immediately so every subsequent failure reaps it.
        let input = child.stdin.take().ok_or("control sin stdin")?;
        let output = child.stdout.take().ok_or("control sin stdout")?;
        let mut c = Self {
            child,
            input: Some(input),
            output,
            buffer: vec![],
            ident: 0,
            timeout,
            cancel,
        };
        nonblocking(c.input.as_ref().ok_or("control cerrado")?)?;
        nonblocking(&c.output)?;
        c.call("initialize",json!({"clientInfo":{"name":"comandos_full_access","version":"1"},"capabilities":{"experimentalApi":true}}))?;
        c.send(&json!({"method":"initialized"}), Instant::now() + timeout)?;
        Ok(c)
    }
    fn alive(&self) -> bool {
        comandos_runtime::procs::child_exited_unreaped(&self.child).is_ok_and(|exited| !exited)
    }
    fn guard(&self, end: Instant, method: &str) -> Result<()> {
        if self.cancel.load(Ordering::SeqCst) {
            Err("mantenimiento cancelado".into())
        } else if Instant::now() >= end {
            Err(format!(
                "Codex no respondió a {method} en {} segundos",
                self.timeout.as_secs_f64()
            ))
        } else {
            Ok(())
        }
    }
    fn send(&mut self, value: &Value, end: Instant) -> Result<()> {
        let mut bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        if bytes.len() > LIMIT {
            return Err("petición de mantenimiento excede el límite".into());
        }
        let mut sent = 0;
        while sent < bytes.len() {
            self.guard(end, "escritura")?;
            match self
                .input
                .as_mut()
                .ok_or("control cerrado")?
                .write(&bytes[sent..])
            {
                Ok(0) => return Err("stdin control cerrado".into()),
                Ok(n) => sent += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    if !self.alive() {
                        return Err("control de Codex cerrado durante escritura".into());
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(())
    }
}
impl Client for Control {
    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.ident = self.ident.checked_add(1).ok_or("IDs control agotados")?;
        let id = self.ident;
        let end = Instant::now() + self.timeout;
        self.send(&json!({"id":id,"method":method,"params":params}), end)?;
        loop {
            self.guard(end, method)?;
            if let Some(n) = self.buffer.iter().position(|b| *b == b'\n') {
                let line = self.buffer.drain(..=n).collect::<Vec<_>>();
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let message: Value = serde_json::from_slice(&line)
                    .map_err(|e| format!("respuesta de mantenimiento inválida: {e}"))?;
                if !message.is_object() {
                    return Err("respuesta de mantenimiento no es objeto".into());
                }
                if message.get("method").is_some() {
                    if let Some(id) = message.get("id") {
                        self.send(&json!({"id":id,"error":{"code":-32601,"message":"This maintenance client cannot approve or execute requests"}}),end)?;
                    }
                    continue;
                }
                if message["id"] != id {
                    continue;
                }
                if let Some(error) = message.get("error") {
                    return Err(format!(
                        "{method}: {}",
                        error["message"]
                            .as_str()
                            .unwrap_or("Codex rechazó la operación")
                    ));
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or("control no devolvió result".into());
            }
            let mut bytes = [0; 65536];
            match self.output.read(&mut bytes) {
                Ok(0) => return Err(format!("El control de Codex se cerró durante {method}")),
                Ok(n) => {
                    if self.buffer.len().saturating_add(n) > LIMIT {
                        return Err(
                            "La respuesta de mantenimiento de Codex excede el límite".into()
                        );
                    }
                    self.buffer.extend_from_slice(&bytes[..n]);
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}
// Reads user-requested plan files without allowing a FIFO to block the CLI.
pub(super) fn read_plan(path: &Path) -> Result<Value> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    if !path.is_absolute() {
        return Err("--plan requiere ruta absoluta".into());
    }
    let m = path.symlink_metadata().map_err(|e| e.to_string())?;
    if !m.is_file() || m.len() > 4 * 1024 * 1024 {
        return Err("plan no regular o demasiado grande".into());
    }
    let f = fs::OpenOptions::new()
        .read(true)
        .custom_flags((OFlag::O_NONBLOCK | OFlag::O_NOCTTY).bits())
        .open(path)
        .map_err(|e| e.to_string())?;
    let o = f.metadata().map_err(|e| e.to_string())?;
    if !o.is_file() || (m.dev(), m.ino()) != (o.dev(), o.ino()) {
        return Err("plan cambió al abrir".into());
    }
    let mut raw = vec![];
    f.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    if raw.len() > 4 * 1024 * 1024 {
        return Err("plan excede límite".into());
    }
    comandos_core::json::parse_unique_value(std::str::from_utf8(&raw).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
