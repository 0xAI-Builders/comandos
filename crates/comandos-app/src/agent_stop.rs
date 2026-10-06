//! Ctrl+C nunca mata al CLI: copia, entrega ^C o arma limpieza tras su salida.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtrlCAction {
    Copy,
    SendInterrupt,
    Cleanup,
}
pub fn ctrl_c_action(
    has_selection: bool,
    copy_context: bool,
    now: f64,
    last: f64,
    window: f64,
) -> CtrlCAction {
    if has_selection {
        CtrlCAction::Copy
    } else if !copy_context && last.is_finite() && (0.0..=window).contains(&(now - last)) {
        CtrlCAction::Cleanup
    } else {
        CtrlCAction::SendInterrupt
    }
}
use crate::config::RunMode;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
pub const DOUBLE_TAP_SECONDS: f64 = 1.;
pub const EXIT_WAIT_SECONDS: f64 = 5.;
pub const GRACE_SECONDS: f64 = 1.;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRecord {
    pub pid: u32,
    pub parent: u32,
    pub start_time: u64,
    pub argv: Vec<String>,
    pub group: u32,
    pub session: u32,
    pub foreground: i32,
    pub comm: String,
}
impl ProcessRecord {
    pub fn same_identity(&self, other: &Self) -> bool {
        self.pid == other.pid
            && self.start_time == other.start_time
            && self.parent == other.parent
            && self.group == other.group
    }
}
pub trait ProcessReader {
    fn snapshot(&self) -> Result<Vec<ProcessRecord>, String>;
    fn read(&self, pid: u32) -> Option<ProcessRecord>;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Interrupt,
    Terminate,
}
/// La implementación debe fijar la identidad antes de revalidarla y entregarla.
/// El adaptador Linux conserva un pidfd, nunca vuelve a señalar mediante el PID.
pub trait SignalSink {
    type Handle;
    fn pin(
        &self,
        record: &ProcessRecord,
        reader: &dyn ProcessReader,
    ) -> Result<Self::Handle, String>;
    fn send(
        &self,
        handle: &Self::Handle,
        record: &ProcessRecord,
        signal: Signal,
        reader: &dyn ProcessReader,
    ) -> Result<(), String>;
}
pub trait Clock {
    fn now(&self) -> f64;
    fn sleep(&self, duration: Duration);
}
pub struct SystemClock(Instant);
impl Default for SystemClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}
impl Clock for SystemClock {
    fn now(&self) -> f64 {
        self.0.elapsed().as_secs_f64()
    }
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration)
    }
}
pub fn is_agent_name(name: &str) -> bool {
    let name = name.rsplit('/').next().unwrap_or("");
    matches!(name, "grok" | "codex") || name.starts_with("grok-") || name.starts_with("codex-")
}
fn is_agent(record: &ProcessRecord) -> bool {
    is_agent_name(&record.comm)
        || record.argv.first().is_some_and(|a| is_agent_name(a))
        || (record
            .argv
            .first()
            .is_some_and(|a| matches!(a.rsplit('/').next(), Some("node" | "bun" | "deno")))
            && record.argv.get(1).is_some_and(|a| is_agent_name(a)))
}
pub fn parse_client_state(output: &str, tty: &str) -> Value {
    for line in output.lines() {
        let parts: Vec<_> = line.split('|').collect();
        if let [terminal, table, prefix, mode, pid] = parts.as_slice()
            && *terminal == tty
        {
            let pane_pid = (!pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
                .then(|| pid.parse::<u32>().ok())
                .flatten();
            return json!({"key_table":table,"prefix":*prefix=="1","in_mode":*mode=="1","pane_pid":pane_pid});
        }
    }
    Value::Null
}
pub fn client_blocks_stop(state: &Value) -> bool {
    !comandos_core::json::truthy(state)
        || !state
            .get("pane_pid")
            .is_some_and(comandos_core::json::truthy)
        || state.get("prefix").is_some_and(comandos_core::json::truthy)
        || state
            .get("in_mode")
            .is_some_and(comandos_core::json::truthy)
        || state
            .get("key_table")
            .is_some_and(|table| !matches!(table.as_str(), Some("root" | "")))
}
pub fn descendant_pids(root: u32, records: &[ProcessRecord]) -> Vec<u32> {
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for row in records {
        children.entry(row.parent).or_default().push(row.pid);
    }
    let mut stack = children.get(&root).cloned().unwrap_or_default();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    while let Some(pid) = stack.pop() {
        if pid != root && seen.insert(pid) {
            out.push(pid);
            stack.extend(children.get(&pid).into_iter().flatten().copied());
        }
    }
    out
}
#[derive(Debug, Clone, Default)]
pub struct StopPlan {
    pane: Option<ProcessRecord>,
    leader: Option<ProcessRecord>,
    targets: Vec<ProcessRecord>,
}
impl StopPlan {
    pub fn build(root: u32, records: &[ProcessRecord], client_state: &Value) -> Self {
        if client_blocks_stop(client_state)
            || client_state.get("pane_pid").and_then(Value::as_u64) != Some(u64::from(root))
        {
            return Self::default();
        }
        let procs: BTreeMap<_, _> = records.iter().map(|r| (r.pid, r)).collect();
        if procs.len() != records.len() {
            return Self::default();
        }
        let Some(pane) = procs.get(&root).copied() else {
            return Self::default();
        };
        let Ok(foreground) = u32::try_from(pane.foreground) else {
            return Self::default();
        };
        if foreground == 0 {
            return Self::default();
        }
        let Some(leader) = procs
            .get(&foreground)
            .copied()
            .filter(|r| r.pid == r.group && is_agent(r))
        else {
            return Self::default();
        };
        if leader.pid != root && !descendant_pids(root, records).contains(&leader.pid) {
            return Self::default();
        }
        let descendants: BTreeSet<_> = descendant_pids(leader.pid, records).into_iter().collect();
        let targets = procs
            .values()
            .filter(|r| descendants.contains(&r.pid) && r.group != leader.group)
            .map(|r| (*r).clone())
            .collect();
        Self {
            pane: Some(pane.clone()),
            leader: Some(leader.clone()),
            targets,
        }
    }
    pub fn targets(&self) -> &[ProcessRecord] {
        &self.targets
    }
    // Un pane cuyo proceso ES el líder puede desaparecer al salir; otro pane debe conservar su identidad.
    fn scope_unchanged(&self, reader: &dyn ProcessReader) -> bool {
        let (Some(pane), Some(leader)) = (&self.pane, &self.leader) else {
            return false;
        };
        match reader.read(pane.pid) {
            Some(now) => pane.same_identity(&now),
            None => pane.pid == leader.pid,
        }
    }
    pub fn execute(
        &self,
        reader: &dyn ProcessReader,
        sink: &impl SignalSink,
        clock: &impl Clock,
        mode: RunMode,
        cancelled: &impl Cancellation,
    ) -> Result<Vec<(u32, Signal)>, String> {
        if mode == RunMode::Shadow || cancelled.is_cancelled() || self.targets.is_empty() {
            return Ok(vec![]);
        }
        let (Some(_pane), Some(leader)) = (&self.pane, &self.leader) else {
            return Ok(vec![]);
        };
        let deadline = clock.now() + EXIT_WAIT_SECONDS;
        loop {
            if cancelled.is_cancelled() || !self.scope_unchanged(reader) {
                return Ok(vec![]);
            }
            if reader
                .read(leader.pid)
                .is_none_or(|now| now.start_time != leader.start_time)
            {
                break;
            }
            if clock.now() >= deadline {
                return Ok(vec![]);
            }
            clock.sleep(Duration::from_millis(100));
        }
        let mut pinned = Vec::new();
        for record in &self.targets {
            if cancelled.is_cancelled() || !self.scope_unchanged(reader) {
                return Ok(vec![]);
            }
            if reader
                .read(record.pid)
                .is_some_and(|now| record.same_identity(&now))
                && let Ok(handle) = sink.pin(record, reader)
            {
                pinned.push((record, handle));
            }
        }
        let mut sent = Vec::new();
        let mut survivors = Vec::new();
        for (record, handle) in pinned {
            if cancelled.is_cancelled() || !self.scope_unchanged(reader) {
                return Ok(sent);
            }
            if reader
                .read(record.pid)
                .is_some_and(|now| record.same_identity(&now))
                && sink
                    .send(&handle, record, Signal::Interrupt, reader)
                    .is_ok()
            {
                sent.push((record.pid, Signal::Interrupt));
                survivors.push((record, handle));
            }
        }
        if survivors.is_empty() {
            return Ok(sent);
        }
        let deadline = clock.now() + GRACE_SECONDS;
        while clock.now() < deadline {
            if cancelled.is_cancelled() || !self.scope_unchanged(reader) {
                return Ok(sent);
            }
            // Último tramo acotado al deadline; evita añadir 100 ms por redondeo del reloj.
            let remaining = (deadline - clock.now()).clamp(1e-9, 0.1);
            clock.sleep(Duration::from_secs_f64(remaining));
        }
        for (record, handle) in survivors {
            if cancelled.is_cancelled() || !self.scope_unchanged(reader) {
                break;
            }
            if reader
                .read(record.pid)
                .is_some_and(|now| record.same_identity(&now))
                && sink
                    .send(&handle, record, Signal::Terminate, reader)
                    .is_ok()
            {
                sent.push((record.pid, Signal::Terminate));
            }
        }
        Ok(sent)
    }
}
/// Directorio de proceso inyectable: los tests usan archivos propios, nunca /proc vivo.
pub struct ProcReader {
    root: PathBuf,
}
impl ProcReader {
    pub fn linux() -> Self {
        Self {
            root: "/proc".into(),
        }
    }
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }
}
pub fn parse_stat(pid: u32, raw: &[u8], argv: Vec<String>, comm: String) -> Option<ProcessRecord> {
    let end = raw.iter().rposition(|b| *b == b')')?;
    let fields: Vec<_> = std::str::from_utf8(raw.get(end + 2..)?)
        .ok()?
        .split_whitespace()
        .collect();
    Some(ProcessRecord {
        pid,
        parent: fields.get(1)?.parse().ok()?,
        group: fields.get(2)?.parse().ok()?,
        session: fields.get(3)?.parse().ok()?,
        foreground: fields.get(5)?.parse().ok()?,
        start_time: fields.get(19)?.parse().ok()?,
        argv,
        comm,
    })
}
impl ProcessReader for ProcReader {
    fn snapshot(&self) -> Result<Vec<ProcessRecord>, String> {
        let dirs = std::fs::read_dir(&self.root).map_err(|e| e.to_string())?;
        Ok(dirs
            .filter_map(Result::ok)
            .filter_map(|d| d.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(|pid| self.read(pid))
            .collect())
    }
    fn read(&self, pid: u32) -> Option<ProcessRecord> {
        let path = self.root.join(pid.to_string());
        let raw = std::fs::read(path.join("stat")).ok()?;
        let status = std::fs::read_to_string(path.join("status")).ok()?;
        let comm = status
            .lines()
            .find_map(|line| line.strip_prefix("Name:"))
            .unwrap_or("")
            .trim()
            .into();
        let argv = std::fs::read(path.join("cmdline"))
            .unwrap_or_default()
            .split(|b| *b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        parse_stat(pid, &raw, argv, comm)
    }
}
pub struct PidfdSignals;
pub struct PinnedProcess {
    fd: std::os::fd::OwnedFd,
    record: ProcessRecord,
}
impl SignalSink for PidfdSignals {
    type Handle = PinnedProcess;
    fn pin(
        &self,
        record: &ProcessRecord,
        reader: &dyn ProcessReader,
    ) -> Result<Self::Handle, String> {
        let pid = i32::try_from(record.pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
            .ok_or("Invalid PID")?;
        let handle = rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty())
            .map_err(|e| format!("pidfd refused: {e}"))?;
        if reader
            .read(record.pid)
            .is_none_or(|now| !record.same_identity(&now))
        {
            return Err("Process identity changed before pin".into());
        }
        Ok(PinnedProcess {
            fd: handle,
            record: record.clone(),
        })
    }
    fn send(
        &self,
        handle: &Self::Handle,
        record: &ProcessRecord,
        signal: Signal,
        reader: &dyn ProcessReader,
    ) -> Result<(), String> {
        if !handle.record.same_identity(record)
            || reader
                .read(record.pid)
                .is_none_or(|now| !record.same_identity(&now))
        {
            return Err("Process identity changed before signal".into());
        }
        rustix::process::pidfd_send_signal(
            &handle.fd,
            match signal {
                Signal::Interrupt => rustix::process::Signal::INT,
                Signal::Terminate => rustix::process::Signal::TERM,
            },
        )
        .map_err(|e| e.to_string())
    }
}
/// Prueba separada del cierre: socket + sesión/pane/identidad del backend + respuesta explícita.
#[derive(Debug, Clone)]
pub struct PaneCloseIntent {
    socket: PathBuf,
    preview: crate::tab_actions::PaneClose,
    pub title: String,
}
impl PaneCloseIntent {
    pub fn prepare(
        socket: &Path,
        session: &str,
        pane: &str,
        post: impl FnMut(&Value) -> Result<Value, String>,
    ) -> Result<Self, String> {
        if !socket.is_absolute() {
            return Err("Falta socket explícito".into());
        }
        let preview = crate::tab_actions::PaneClose::prepare(session, pane, post)?;
        Ok(Self {
            socket: socket.into(),
            title: preview.title.clone(),
            preview,
        })
    }
    pub fn finish(
        &self,
        socket: &Path,
        confirmed: bool,
        cancelled: bool,
        mode: RunMode,
        post: impl FnOnce(&Value) -> Result<Value, String>,
    ) -> Result<Option<Value>, String> {
        if !confirmed || cancelled || mode == RunMode::Shadow {
            return Ok(None);
        }
        if socket != self.socket {
            return Err("Cambió el socket; el panel no se cerró".into());
        }
        self.preview.finish(true, false, post)
    }
}
/// Jobs llama el servicio inyectado; tests nunca instancian el lector Linux vivo.
pub trait CleanupService: Send + Sync {
    fn cleanup(
        &self,
        root: u32,
        state: &Value,
        mode: RunMode,
        cancelled: &StopCancellation,
    ) -> Result<Vec<(u32, Signal)>, String>;
}
pub struct NativeCleanup;
pub trait Cancellation {
    fn is_cancelled(&self) -> bool;
}
impl Cancellation for AtomicBool {
    fn is_cancelled(&self) -> bool {
        self.load(Ordering::Acquire)
    }
}
pub struct StopCancellation {
    pub app: std::sync::Arc<AtomicBool>,
    pub terminal: std::sync::Arc<AtomicBool>,
}
impl Cancellation for StopCancellation {
    fn is_cancelled(&self) -> bool {
        self.app.load(Ordering::Acquire) || self.terminal.load(Ordering::Acquire)
    }
}
impl CleanupService for NativeCleanup {
    fn cleanup(
        &self,
        root: u32,
        state: &Value,
        mode: RunMode,
        cancelled: &StopCancellation,
    ) -> Result<Vec<(u32, Signal)>, String> {
        if mode == RunMode::Shadow || cancelled.is_cancelled() {
            return Ok(vec![]);
        }
        let reader = ProcReader::linux();
        let snapshot = reader.snapshot()?;
        StopPlan::build(root, &snapshot, state).execute(
            &reader,
            &PidfdSignals,
            &SystemClock::default(),
            mode,
            cancelled,
        )
    }
}
