use crate::{
    config::BrokerConfig,
    session::{
        BoxFuture, ERR_BUSY, ERR_EXPIRED, ERR_TIMEOUT, ERR_UNAVAILABLE, ERR_UNKNOWN_TOOL, Registry,
        SessionId, ToolBackend, tool_error,
    },
    worker::{CloseError, Worker, remote_error_text},
};
use serde_json::{Value, json};
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
}

pub struct Pool {
    cfg: BrokerConfig,
    registry: Arc<Registry>,
    tools: BTreeSet<String>,
    state: Mutex<State>,
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
            state: Mutex::new(State {
                slots: BTreeMap::new(),
                stuck: Vec::new(),
                starting: 0,
                closing: 0,
                waiters: 0,
            }),
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
        let mut registered_waiter = false;
        loop {
            let profile = {
                let mut state = self.state.lock().await;
                if let Some(worker) = state
                    .slots
                    .get(session)
                    .and_then(|slot| slot.worker.as_ref().cloned())
                {
                    if registered_waiter {
                        state.waiters = state.waiters.saturating_sub(1);
                    }
                    return Some(worker);
                }
                if self.total_workers_locked(&state) < self.cfg.max_workers {
                    state.starting += 1;
                    let profile = self.cfg.state_dir.join(format!("session-{}", session.0));
                    Some(profile)
                } else {
                    if self.cfg.queue_timeout <= 0.0 {
                        return None;
                    }
                    if !registered_waiter {
                        if state.waiters >= 16 {
                            return None;
                        }
                        state.waiters += 1;
                        registered_waiter = true;
                    }
                    None
                }
            };
            if let Some(profile) = profile {
                let (tx, mut rx) = mpsc::channel(16);
                let registry = self.registry.clone();
                let notify_session = session.clone();
                tokio::spawn(async move {
                    while let Some(value) = rx.recv().await {
                        registry.notify(&notify_session, value).await;
                    }
                });
                let worker = Worker::start(&self.cfg, profile, tx).await.map(Arc::new);
                match worker {
                    Ok(worker) => {
                        let mut close_started = false;
                        {
                            let mut state = self.state.lock().await;
                            state.starting = state.starting.saturating_sub(1);
                            if registered_waiter {
                                state.waiters = state.waiters.saturating_sub(1);
                            }
                            if let Some(slot) = state.slots.get_mut(session) {
                                slot.worker = Some(worker.clone());
                                slot.expired = false;
                                slot.last_used = Instant::now();
                            } else {
                                state.closing += 1;
                                close_started = true;
                            }
                        }
                        if close_started {
                            let _ = self.close_worker(worker).await;
                            self.finish_closing(None).await;
                            return None;
                        }
                        self.registry.set_worker(session, worker.pid(), true).await;
                        return Some(worker);
                    }
                    Err(_) => {
                        let mut state = self.state.lock().await;
                        state.starting = state.starting.saturating_sub(1);
                        if registered_waiter {
                            state.waiters = state.waiters.saturating_sub(1);
                        }
                        drop(state);
                        self.notify_capacity.notify_waiters();
                        return None;
                    }
                }
            }
            let Some(deadline) = deadline else {
                if registered_waiter {
                    let mut state = self.state.lock().await;
                    state.waiters = state.waiters.saturating_sub(1);
                }
                return None;
            };
            let now = Instant::now();
            if now >= deadline {
                if registered_waiter {
                    let mut state = self.state.lock().await;
                    state.waiters = state.waiters.saturating_sub(1);
                }
                return None;
            }
            let wait = self.notify_capacity.notified();
            if tokio::time::timeout(deadline.saturating_duration_since(now), wait)
                .await
                .is_err()
            {
                if registered_waiter {
                    let mut state = self.state.lock().await;
                    state.waiters = state.waiters.saturating_sub(1);
                }
                return None;
            }
        }
    }

    async fn finish_closing(&self, stuck: Option<Arc<Worker>>) {
        {
            let mut state = self.state.lock().await;
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
            let mut state = self.state.lock().await;
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
        let result = self.close_worker(worker.clone()).await;
        if result.is_err() {
            self.finish_closing(Some(worker)).await;
        } else {
            self.finish_closing(None).await;
        }
        self.registry.set_worker(session, None, false).await;
        result
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
        let now = Instant::now();
        let sessions = {
            let state = self.state.lock().await;
            state
                .slots
                .iter()
                .filter(|(_, slot)| {
                    slot.worker.is_some()
                        && !slot.busy
                        && now.duration_since(slot.last_used).as_secs_f64() > self.cfg.idle_seconds
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        for session in sessions {
            if let Err(e) = self.release_worker(&session, true).await {
                eprintln!("comandos-broker-mac: no se pudo cerrar trabajador inactivo: {e:?}");
            }
        }
    }

    pub async fn retry_stuck(self: &Arc<Self>) {
        let stuck = {
            let mut state = self.state.lock().await;
            let stuck = std::mem::take(&mut state.stuck);
            state.closing += stuck.len();
            stuck
        };
        for worker in stuck {
            if let Err(e) = self.close_worker(worker.clone()).await {
                eprintln!("comandos-broker-mac: trabajador atascado sigue vivo: {e:?}");
                self.finish_closing(Some(worker)).await;
            } else {
                self.finish_closing(None).await;
            }
        }
    }

    pub async fn status(&self) -> Value {
        let snapshots = self.registry.snapshots().await;
        let state = self.state.lock().await;
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
        let _ = self.release_worker(&session, false).await;
        self.state.lock().await.slots.remove(&session);
        self.notify_capacity.notify_waiters();
    }

    pub async fn close_all(self: &Arc<Self>) {
        let sessions = self
            .state
            .lock()
            .await
            .slots
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for session in sessions {
            let _ = self.release_worker(&session, false).await;
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
                let mut state = pool.state.lock().await;
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
                    .await
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
                let mut state = pool.state.lock().await;
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
