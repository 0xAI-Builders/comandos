use crate::{
    config::BrokerConfig,
    session::{
        BoxFuture, ERR_BUSY, ERR_EXPIRED, ERR_TIMEOUT, ERR_UNAVAILABLE, ERR_UNKNOWN_TOOL, Registry,
        SessionId, ToolBackend, tool_error,
    },
    worker::{CloseError, Worker, remote_error_text},
};
use serde_json::{Value, json};
use std::sync::Mutex as StateMutex;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};
use tokio::sync::{Mutex, Notify, mpsc};

type CloseHook = Arc<dyn Fn() -> BoxFuture<'static, Result<(), CloseError>> + Send + Sync>;

struct Slot {
    worker: Option<Arc<Worker>>,
    expired: bool,
    busy: bool,
    last_used: Instant,
}

struct State {
    slots: BTreeMap<SessionId, Slot>,
    stuck: Vec<Arc<Worker>>,
    starting: usize,
    closing: usize,
    waiters: usize,
    shutting_down: bool,
}

pub struct Pool {
    cfg: BrokerConfig,
    registry: Arc<Registry>,
    tools: BTreeSet<String>,
    state: StateMutex<State>,
    lifecycle: StateMutex<Vec<tokio::task::JoinHandle<()>>>,
    shutdown: Notify,
    notify_capacity: Notify,
    close_hook: Mutex<Option<CloseHook>>,
    fail_next_close: AtomicBool,
}

impl Pool {
    pub fn new(cfg: BrokerConfig, registry: Arc<Registry>) -> Arc<Self> {
        Arc::new(Self {
            tools: cfg.tools.clone(),
            cfg,
            registry,
            state: StateMutex::new(State {
                slots: BTreeMap::new(),
                stuck: Vec::new(),
                starting: 0,
                closing: 0,
                waiters: 0,
                shutting_down: false,
            }),
            lifecycle: StateMutex::new(Vec::new()),
            shutdown: Notify::new(),
            notify_capacity: Notify::new(),
            close_hook: Mutex::new(None),
            fail_next_close: AtomicBool::new(false),
        })
    }

    pub async fn with_close_hook(self: &Arc<Self>, hook: CloseHook) -> Arc<Self> {
        *self.close_hook.lock().await = Some(hook);
        self.clone()
    }

    pub fn fail_next_close(&self) {
        self.fail_next_close.store(true, Ordering::SeqCst);
    }

    async fn acquire(self: &Arc<Self>, session: &SessionId) -> Option<Arc<Worker>> {
        let deadline = (self.cfg.queue_timeout > 0.0)
            .then(|| Instant::now() + Duration::from_secs_f64(self.cfg.queue_timeout));
        let mut waiter = Waiter {
            pool: self.clone(),
            registered: false,
        };
        loop {
            // Register before checking capacity so a release cannot be missed.
            let capacity = self.notify_capacity.notified();
            tokio::pin!(capacity);
            capacity.as_mut().enable();
            let profile = {
                let mut state = self.state.lock().unwrap();
                if state.shutting_down || !state.slots.contains_key(session) {
                    return None;
                }
                if let Some(worker) = state
                    .slots
                    .get(session)
                    .and_then(|slot| slot.worker.clone())
                {
                    return Some(worker);
                }
                if self.total_workers_locked(&state) < self.cfg.max_workers {
                    state.starting += 1;
                    Some(self.cfg.state_dir.join(format!("session-{}", session.0)))
                } else {
                    deadline?;
                    if !waiter.registered {
                        if state.waiters >= 16 {
                            return None;
                        }
                        state.waiters += 1;
                        waiter.registered = true;
                    }
                    None
                }
            };
            if let Some(profile) = profile {
                let mut reservation = Starting {
                    pool: self.clone(),
                    worker: None,
                    active: true,
                };
                let (tx, mut rx) = mpsc::channel(16);
                let worker = Arc::new(Worker::spawn(&self.cfg, profile, tx).ok()?);
                reservation.worker = Some(worker.clone());
                let registry = self.registry.clone();
                let notify_session = session.clone();
                let notification_task = tokio::spawn(async move {
                    while let Some(value) = rx.recv().await {
                        registry.notify(&notify_session, value).await;
                    }
                });
                worker.own_task(notification_task);
                let shutdown = self.shutdown.notified();
                tokio::pin!(shutdown);
                shutdown.as_mut().enable();
                if self.state.lock().unwrap().shutting_down {
                    return None;
                }
                let initialized = tokio::select! {
                    result = worker.initialize(&self.cfg) => result.is_ok(),
                    _ = &mut shutdown => false,
                };
                if !initialized {
                    return None;
                }
                {
                    let mut state = self.state.lock().unwrap();
                    if let Some(slot) = state.slots.get_mut(session) {
                        slot.worker = Some(worker.clone());
                        slot.expired = false;
                        slot.last_used = Instant::now();
                    } else {
                        return None;
                    }
                    state.starting -= 1;
                    reservation.active = false;
                }
                self.registry.set_worker(session, worker.pid(), true).await;
                return Some(worker);
            }
            let deadline = deadline?;
            if tokio::time::timeout(
                deadline.saturating_duration_since(Instant::now()),
                &mut capacity,
            )
            .await
            .is_err()
            {
                return None;
            }
        }
    }

    fn track(&self, task: tokio::task::JoinHandle<()>) {
        let mut tasks = self.lifecycle.lock().unwrap();
        tasks.retain(|task| !task.is_finished());
        tasks.push(task);
    }

    fn owned_close(
        self: &Arc<Self>,
        worker: Arc<Worker>,
    ) -> tokio::sync::oneshot::Receiver<Result<(), CloseError>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let pool = self.clone();
        let task = tokio::spawn(async move {
            let result = pool.close_worker(worker.clone()).await;
            pool.finish_closing(if result.is_err() { Some(worker) } else { None })
                .await;
            let _ = tx.send(result);
        });
        self.track(task);
        rx
    }

    async fn finish_closing(&self, stuck: Option<Arc<Worker>>) {
        {
            let mut state = self.state.lock().unwrap();
            state.closing = state.closing.saturating_sub(1);
            if let Some(worker) = stuck {
                state.stuck.push(worker);
            }
        }
        self.notify_capacity.notify_waiters();
    }

    async fn release_worker(
        self: &Arc<Self>,
        session: &SessionId,
        expired: bool,
    ) -> Result<(), CloseError> {
        let worker = {
            let mut state = self.state.lock().unwrap();
            let Some(slot) = state.slots.get_mut(session) else {
                return Ok(());
            };
            slot.expired |= expired;
            slot.busy = false;
            let worker = slot.worker.take();
            if worker.is_some() {
                state.closing += 1;
            }
            worker
        };
        let Some(worker) = worker else {
            return Ok(());
        };
        let result = self.owned_close(worker);
        self.registry.set_worker(session, None, false).await;
        result
            .await
            .unwrap_or_else(|_| Err(CloseError::Io("cleanup task failed".into())))
    }

    async fn close_worker(&self, worker: Arc<Worker>) -> Result<(), CloseError> {
        if self.fail_next_close.swap(false, Ordering::SeqCst) {
            return Err(CloseError::DescendantsAlive);
        }
        if let Some(hook) = self.close_hook.lock().await.clone() {
            hook().await?;
        }
        worker.close(self.cfg.stop_grace).await
    }

    pub async fn reap_idle(self: &Arc<Self>) {
        let sessions = self
            .state
            .lock()
            .unwrap()
            .slots
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for session in sessions {
            let worker = {
                let mut state = self.state.lock().unwrap();
                let Some(slot) = state.slots.get_mut(&session) else {
                    continue;
                };
                if slot.busy || slot.last_used.elapsed().as_secs_f64() <= self.cfg.idle_seconds {
                    continue;
                }
                let Some(worker) = slot.worker.take() else {
                    continue;
                };
                slot.expired = true;
                state.closing += 1;
                worker
            };
            let close = self.owned_close(worker);
            self.registry.set_worker(&session, None, false).await;
            if let Ok(Err(e)) = close.await {
                eprintln!("comandos-broker-mac: no se pudo cerrar trabajador inactivo: {e:?}");
            }
        }
    }

    pub async fn retry_stuck(self: &Arc<Self>) {
        let closes = {
            let mut state = self.state.lock().unwrap();
            let stuck = std::mem::take(&mut state.stuck);
            state.closing += stuck.len();
            // Transfer every worker into its cleanup task before any cancellation point.
            stuck
                .into_iter()
                .map(|worker| self.owned_close(worker))
                .collect::<Vec<_>>()
        };
        for close in closes {
            if let Ok(Err(e)) = close.await {
                eprintln!("comandos-broker-mac: trabajador atascado sigue vivo: {e:?}");
            }
        }
    }

    pub async fn status(&self) -> Value {
        let snapshots = self.registry.snapshots().await;
        let state = self.state.lock().unwrap();
        let sessions = snapshots
            .into_iter()
            .map(|s| {
                json!({"id":s.id.0,"client":s.client_name,"workerPid":s.worker_pid,"busy":s.busy})
            })
            .collect::<Vec<_>>();
        json!({
            "connections": sessions.len(),
            "workers": self.total_workers_locked(&state),
            "maxWorkers": self.cfg.max_workers,
            "queued": state.waiters,
            "sessions": sessions,
            "stuck": state.stuck.len()
        })
    }

    fn total_workers_locked(&self, state: &State) -> usize {
        state
            .slots
            .values()
            .filter(|slot| slot.worker.is_some())
            .count()
            + state.stuck.len()
            + state.starting
            + state.closing
            + self.fresh_python_workers()
    }

    fn fresh_python_workers(&self) -> usize {
        let Some(path) = &self.cfg.python_status else {
            return 0;
        };
        let Ok(meta) = std::fs::metadata(path) else {
            return 0;
        };
        let Ok(modified) = meta.modified() else {
            return 0;
        };
        if SystemTime::now()
            .duration_since(modified)
            .map(|d| d > Duration::from_secs(15))
            .unwrap_or(true)
        {
            return 0;
        }
        std::fs::read(path)
            .ok()
            .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
            .and_then(|v| v.get("workers").and_then(Value::as_u64).map(|n| n as usize))
            .unwrap_or(0)
    }

    pub async fn disconnect(self: &Arc<Self>, session: SessionId) {
        let close = {
            let mut state = self.state.lock().unwrap();
            let worker = state.slots.remove(&session).and_then(|slot| slot.worker);
            worker.map(|worker| {
                state.closing += 1;
                self.owned_close(worker)
            })
        };
        self.notify_capacity.notify_waiters();
        self.registry.set_worker(&session, None, false).await;
        if let Some(close) = close {
            let _ = close.await;
        }
    }

    pub async fn close_all(self: &Arc<Self>) {
        {
            self.state.lock().unwrap().shutting_down = true;
        }
        self.shutdown.notify_waiters();
        self.notify_capacity.notify_waiters();
        let sessions = self
            .state
            .lock()
            .unwrap()
            .slots
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for session in sessions {
            self.disconnect(session).await;
        }
        loop {
            let tasks = std::mem::take(&mut *self.lifecycle.lock().unwrap());
            for task in tasks {
                let _ = task.await;
            }
            if self.state.lock().unwrap().starting == 0 && self.lifecycle.lock().unwrap().is_empty()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        self.retry_stuck().await;
    }

    pub fn state_dir(&self) -> PathBuf {
        self.cfg.state_dir.clone()
    }
}

impl ToolBackend for Arc<Pool> {
    fn call(&self, session: SessionId, params: Value) -> BoxFuture<'static, Value> {
        let pool = self.clone();
        Box::pin(async move {
            let name = params.get("name").and_then(Value::as_str);
            if !name.is_some_and(|name| pool.tools.contains(name)) {
                return tool_error(ERR_UNKNOWN_TOOL);
            }
            {
                let mut state = pool.state.lock().unwrap();
                let slot = state.slots.entry(session.clone()).or_insert(Slot {
                    worker: None,
                    expired: false,
                    busy: false,
                    last_used: Instant::now(),
                });
                if slot.expired {
                    slot.expired = false;
                    return tool_error(ERR_EXPIRED);
                }
                slot.busy = true;
            }
            pool.registry.set_worker(&session, None, true).await;
            let Some(worker) = pool.acquire(&session).await else {
                pool.state
                    .lock()
                    .unwrap()
                    .slots
                    .entry(session.clone())
                    .and_modify(|slot| slot.busy = false);
                pool.registry.set_worker(&session, None, false).await;
                return tool_error(ERR_BUSY);
            };
            pool.registry.set_worker(&session, worker.pid(), true).await;
            let out = tokio::time::timeout(
                Duration::from_secs_f64(pool.cfg.tool_timeout),
                worker.request("tools/call", params),
            )
            .await;
            let value = match out {
                Ok(Ok(response)) => response
                    .get("result")
                    .cloned()
                    .unwrap_or_else(|| tool_error(&remote_error_text(&response))),
                Ok(Err(_)) => {
                    let _ = pool.release_worker(&session, true).await;
                    tool_error(ERR_UNAVAILABLE)
                }
                Err(_) => {
                    let _ = pool.release_worker(&session, true).await;
                    tool_error(ERR_TIMEOUT)
                }
            };
            let keep_worker = {
                let mut state = pool.state.lock().unwrap();
                if let Some(slot) = state.slots.get_mut(&session) {
                    slot.busy = false;
                    slot.last_used = Instant::now();
                    slot.worker
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &worker))
                } else {
                    false
                }
            };
            if keep_worker {
                pool.registry
                    .set_worker(&session, worker.pid(), false)
                    .await;
            } else {
                pool.registry.set_worker(&session, None, false).await;
            }
            value
        })
    }

    fn release(&self, session: SessionId) -> BoxFuture<'static, ()> {
        let pool = self.clone();
        Box::pin(async move {
            pool.disconnect(session).await;
        })
    }
}

struct Waiter {
    pool: Arc<Pool>,
    registered: bool,
}
impl Drop for Waiter {
    fn drop(&mut self) {
        if self.registered {
            self.pool.state.lock().unwrap().waiters -= 1;
        }
    }
}
struct Starting {
    pool: Arc<Pool>,
    worker: Option<Arc<Worker>>,
    active: bool,
}
impl Drop for Starting {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut state = self.pool.state.lock().unwrap();
        state.starting -= 1;
        if let Some(worker) = self.worker.take() {
            state.closing += 1;
            self.pool.owned_close(worker);
        }
        self.pool.notify_capacity.notify_waiters();
    }
}
