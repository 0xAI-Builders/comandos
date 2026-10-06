use crate::{
    config::BrokerConfig,
    proc_scan::{owned_processes, signal_owned},
    session::ERR_REMOTE_PREFIX,
    wire::{Line, encode_message, read_line},
};
use comandos_core::json::parse_slice;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, mpsc, oneshot},
};

#[derive(Debug)]
pub enum WorkerError {
    Io(String),
    Json(String),
    Disconnected,
    ResponseError(String),
}

#[derive(Debug)]
pub enum CloseError {
    Io(String),
    DescendantsAlive,
}

struct Inner {
    child: Child,
    stdin: ChildStdin,
}

type Pending = Arc<Mutex<BTreeMap<i64, oneshot::Sender<Result<Value, WorkerError>>>>>;

pub struct Worker {
    inner: Arc<Mutex<Inner>>,
    pending: Pending,
    counter: AtomicI64,
    profile: PathBuf,
    stderr: Arc<Mutex<VecDeque<String>>>,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    last_used: Mutex<Instant>,
}

impl Worker {
    pub async fn start(
        cfg: &BrokerConfig,
        profile: PathBuf,
        notify: mpsc::Sender<Value>,
    ) -> Result<Self, WorkerError> {
        std::fs::create_dir_all(&profile).map_err(|e| WorkerError::Io(e.to_string()))?;
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| WorkerError::Io(e.to_string()))?;
        let Some((program, args)) = cfg.command.split_first() else {
            return Err(WorkerError::Io("command vacío".to_owned()));
        };
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.arg(format!("--user-data-dir={}", profile.display()));
        cmd.envs(std::env::vars());
        for (key, value) in &cfg.env {
            cmd.env(key, value);
        }
        cmd.env("CHROME_DEVTOOLS_MCP_NO_UPDATE_CHECKS", "1");
        cmd.env("CHROME_DEVTOOLS_MCP_NO_USAGE_STATISTICS", "1");
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| WorkerError::Io(e.to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| WorkerError::Io("stdout no disponible".to_owned()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| WorkerError::Io("stderr no disponible".to_owned()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| WorkerError::Io("stdin no disponible".to_owned()))?;
        let pending: Pending = Arc::new(Mutex::new(BTreeMap::new()));
        let stderr_ring = Arc::new(Mutex::new(VecDeque::with_capacity(8)));
        let pump_pending = pending.clone();
        let pump_notify = notify;
        let pump = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();
            while let Ok(Line::Message(raw)) = read_line(&mut reader, &mut buf).await {
                let Ok(message) = parse_slice(&raw) else {
                    continue;
                };
                if let Some(id) = message.get("id").and_then(Value::as_i64) {
                    if let Some(tx) = pump_pending.lock().await.remove(&id) {
                        let _ = tx.send(Ok(message));
                    }
                } else if message
                    .get("method")
                    .and_then(Value::as_str)
                    .is_some_and(|method| method.starts_with("notifications/"))
                {
                    let _ = pump_notify.try_send(message);
                }
            }
            for (_, tx) in std::mem::take(&mut *pump_pending.lock().await) {
                let _ = tx.send(Err(WorkerError::Disconnected));
            }
        });
        let err_ring = stderr_ring.clone();
        let drain = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            drain_stderr(&mut reader, err_ring).await;
        });
        let worker = Self {
            inner: Arc::new(Mutex::new(Inner { child, stdin })),
            pending,
            counter: AtomicI64::new(0),
            profile,
            stderr: stderr_ring,
            tasks: Mutex::new(vec![pump, drain]),
            last_used: Mutex::new(Instant::now()),
        };
        let init = json!({
            "protocolVersion": cfg.catalog.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-11-25"),
            "capabilities": {},
            "clientInfo": {"name":"comandos-browser","version":"1"}
        });
        let _response =
            match tokio::time::timeout(Duration::from_secs(20), worker.request("initialize", init))
                .await
            {
                Ok(Ok(response)) if response.get("error").is_none() => response,
                _ => {
                    let _ = Arc::new(worker).close(cfg.stop_grace).await;
                    return Err(WorkerError::Io("Worker initialization failed".to_owned()));
                }
            };
        if let Err(error) = worker
            .send_value(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await
        {
            let _ = Arc::new(worker).close(cfg.stop_grace).await;
            return Err(error);
        }
        Ok(worker)
    }

    async fn send_value(&self, value: &Value) -> Result<(), WorkerError> {
        let bytes = encode_message(value).map_err(WorkerError::Json)?;
        let mut inner = self.inner.lock().await;
        tokio::time::timeout(Duration::from_secs(10), async {
            inner.stdin.write_all(&bytes).await?;
            inner.stdin.flush().await
        })
        .await
        .map_err(|_| WorkerError::Io("stdin write timed out".to_owned()))?
        .map_err(|e| WorkerError::Io(e.to_string()))
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, WorkerError> {
        let id = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        if let Err(error) = self
            .send_value(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await
        {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }
        let response = rx.await.map_err(|_| WorkerError::Disconnected)??;
        *self.last_used.lock().await = Instant::now();
        Ok(response)
    }

    pub async fn close(self: Arc<Self>, stop_grace: f64) -> Result<(), CloseError> {
        let pid = self.pid().map(|pid| pid as i32);
        let mut owned = pid
            .map(|pid| owned_processes(Path::new("/proc"), pid, &self.profile))
            .unwrap_or_default();
        {
            let mut inner = self.inner.lock().await;
            if let Some(pid) = inner.child.id() {
                let _ = nix::sys::signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
            }
            if tokio::time::timeout(
                Duration::from_secs_f64(stop_grace.max(0.0)),
                inner.child.wait(),
            )
            .await
            .is_err()
            {
                let _ = inner.child.start_kill();
                let _ = inner.child.wait().await;
            }
        }
        if let Some(pid) = pid {
            owned.extend(owned_processes(Path::new("/proc"), pid, &self.profile));
        }
        signal_owned(Path::new("/proc"), &owned, Signal::SIGTERM);
        tokio::time::sleep(Duration::from_millis(50)).await;
        signal_owned(Path::new("/proc"), &owned, Signal::SIGKILL);
        for _ in 0..40 {
            let remaining = remaining_live(Path::new("/proc"), &owned);
            if remaining.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if !remaining_live(Path::new("/proc"), &owned).is_empty() {
            return Err(CloseError::DescendantsAlive);
        }
        for task in self.tasks.lock().await.drain(..) {
            task.abort();
        }
        let _ = std::fs::remove_dir_all(&self.profile);
        Ok(())
    }

    pub fn pid(&self) -> Option<u32> {
        self.inner
            .try_lock()
            .ok()
            .and_then(|inner| inner.child.id())
    }

    pub async fn last_used(&self) -> Instant {
        *self.last_used.lock().await
    }

    pub async fn stderr_tail(&self) -> Vec<String> {
        self.stderr.lock().await.iter().cloned().collect()
    }
}

fn remaining_live(proc_root: &Path, owned: &BTreeMap<i32, u64>) -> BTreeMap<i32, u64> {
    owned
        .iter()
        .filter_map(|(pid, generation)| {
            let identity = crate::proc_scan::process_identity(proc_root, *pid)?;
            if identity.0 != *generation {
                return None;
            }
            let raw = std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
            let state = raw
                .get(raw.rfind(')')? + 2..)?
                .split_whitespace()
                .next()
                .unwrap_or("");
            (state != "Z").then_some((*pid, *generation))
        })
        .collect()
}

async fn drain_stderr<R: AsyncBufRead + Unpin>(reader: &mut R, ring: Arc<Mutex<VecDeque<String>>>) {
    let mut line = Vec::with_capacity(2048);
    while let Ok(available) = reader.fill_buf().await {
        if available.is_empty() {
            if !line.is_empty() {
                push_stderr(&ring, &line).await;
            }
            break;
        }
        let newline = available.iter().position(|&byte| byte == b'\n');
        let count = newline.map_or(available.len(), |index| index + 1);
        for &byte in &available[..count] {
            if line.len() < 2048 {
                line.push(byte);
            }
        }
        reader.consume(count);
        if newline.is_some() {
            push_stderr(&ring, &line).await;
            line.clear();
        }
    }
}

async fn push_stderr(ring: &Arc<Mutex<VecDeque<String>>>, chunk: &[u8]) {
    let mut ring = ring.lock().await;
    if ring.len() == 8 {
        ring.pop_front();
    }
    ring.push_back(String::from_utf8_lossy(chunk).into_owned());
}

pub fn remote_error_text(value: &Value) -> String {
    format!(
        "{ERR_REMOTE_PREFIX}{}",
        value
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("unknown error")
    )
}
