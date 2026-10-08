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
        atomic::{AtomicBool, AtomicI64, Ordering},
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

type Pending = Arc<Mutex<BTreeMap<i64, oneshot::Sender<Result<Value, WorkerError>>>>>;

pub struct Worker {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    root: Option<(i32, u64)>,
    owned: Mutex<BTreeMap<i32, u64>>,
    closing: Mutex<()>,
    closed: AtomicBool,
    pending: Pending,
    counter: AtomicI64,
    profile: PathBuf,
    stderr: Arc<Mutex<VecDeque<String>>>,
    tasks: std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
    last_used: Mutex<Instant>,
}

impl Worker {
    pub async fn start(
        cfg: &BrokerConfig,
        profile: PathBuf,
        notify: mpsc::Sender<Value>,
    ) -> Result<Arc<Self>, WorkerError> {
        let worker = Arc::new(Self::spawn(cfg, profile, notify)?);
        let mut cleanup = StartCleanup(Some(worker.clone()), cfg.stop_grace);
        if let Err(error) = worker.initialize(cfg).await {
            if worker.clone().close(cfg.stop_grace).await.is_ok() {
                cleanup.0 = None;
            }
            return Err(error);
        }
        cleanup.0 = None;
        Ok(worker)
    }

    pub fn spawn(
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
        let root = child.id().and_then(|pid| {
            crate::proc_scan::process_identity(Path::new("/proc"), pid as i32)
                .map(|(generation, _)| (pid as i32, generation))
        });
        Ok(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            root,
            owned: Mutex::new(BTreeMap::new()),
            closing: Mutex::new(()),
            closed: AtomicBool::new(false),
            pending,
            counter: AtomicI64::new(0),
            profile,
            stderr: stderr_ring,
            tasks: std::sync::Mutex::new(vec![pump, drain]),
            last_used: Mutex::new(Instant::now()),
        })
    }

    pub fn own_task(&self, task: tokio::task::JoinHandle<()>) {
        self.tasks.lock().unwrap().push(task);
    }

    pub async fn initialize(&self, cfg: &BrokerConfig) -> Result<(), WorkerError> {
        let init = json!({
            "protocolVersion": cfg.catalog.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-11-25"),
            "capabilities": {},
            "clientInfo": {"name":"comandos-browser","version":"1"}
        });
        let _response =
            match tokio::time::timeout(Duration::from_secs(20), self.request("initialize", init))
                .await
            {
                Ok(Ok(response)) if response.get("error").is_none() => response,
                _ => {
                    return Err(WorkerError::Io("Worker initialization failed".to_owned()));
                }
            };
        self.send_value(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await?;
        Ok(())
    }

    async fn send_value(&self, value: &Value) -> Result<(), WorkerError> {
        let bytes = encode_message(value).map_err(WorkerError::Json)?;
        let mut stdin = self.stdin.lock().await;
        tokio::time::timeout(Duration::from_secs(10), async {
            stdin.write_all(&bytes).await?;
            stdin.flush().await
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
        let _closing = self.closing.lock().await;
        if self.closed.load(Ordering::SeqCst) {
            return Ok(());
        }
        let proc_root = Path::new("/proc");
        let root_pid = self
            .root
            .filter(|(pid, generation)| {
                crate::proc_scan::process_identity(proc_root, *pid)
                    .is_some_and(|(current, _)| current == *generation)
            })
            .map(|(pid, _)| pid)
            .unwrap_or(-1);
        let mut owned = self.owned.lock().await;
        owned.extend(owned_processes(proc_root, root_pid, &self.profile));
        {
            let mut child = self.child.lock().await;
            if let Some(pid) = child.id() {
                let _ = nix::sys::signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
            }
            if tokio::time::timeout(Duration::from_secs_f64(stop_grace.max(0.0)), child.wait())
                .await
                .is_err()
            {
                let _ = child.start_kill();
                child
                    .wait()
                    .await
                    .map_err(|e| CloseError::Io(e.to_string()))?;
            }
        }
        // The profile scan still works after reaping the root, and previous generations
        // remain owned across failed closes and retries.
        let root_pid = self
            .root
            .filter(|(pid, generation)| {
                crate::proc_scan::process_identity(proc_root, *pid)
                    .is_some_and(|(current, _)| current == *generation)
            })
            .map(|(pid, _)| pid)
            .unwrap_or(-1);
        owned.extend(owned_processes(proc_root, root_pid, &self.profile));
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
        for task in self.tasks.lock().unwrap().drain(..) {
            task.abort();
        }
        let _ = std::fs::remove_dir_all(&self.profile);
        // Late Arc owners must never rescan a profile after ownership is released.
        self.closed.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn pid(&self) -> Option<u32> {
        self.root.map(|(pid, _)| pid as u32)
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

// Direct users retain cleanup ownership even when startup is cancelled.
struct StartCleanup(Option<Arc<Worker>>, f64);
impl Drop for StartCleanup {
    fn drop(&mut self) {
        if let Some(worker) = self.0.take() {
            let grace = self.1;
            tokio::spawn(async move {
                while worker.clone().close(grace).await.is_err() {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            });
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        for task in self.tasks.get_mut().unwrap().drain(..) {
            task.abort();
        }
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let proc_root = Path::new("/proc");
        let root_pid = self
            .root
            .filter(|(pid, generation)| {
                crate::proc_scan::process_identity(proc_root, *pid)
                    .is_some_and(|(current, _)| current == *generation)
            })
            .map(|(pid, _)| pid)
            .unwrap_or(-1);
        let owned = self.owned.get_mut();
        owned.extend(owned_processes(proc_root, root_pid, &self.profile));
        signal_owned(proc_root, owned, Signal::SIGKILL);
    }
}
