//! Shadow is a captured audit view: no tmux client, PTY or server-size feedback.
use crate::{
    config::RunMode,
    tmux::{TmuxCtl, TmuxError},
};
use std::{
    collections::BTreeSet,
    os::unix::fs::MetadataExt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub const REFRESH_MS: u64 = 500;
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_CELLS: usize = 200_000;
const MAX_BYTES: usize = 4 << 20;
const MAX_PANES: usize = 64;
const FORMAT: &str = "#{pid}|#{session_id}|#{window_id}|#{window_width}|#{window_height}|#{window_layout}|#{pane_id}|#{pane_left}|#{pane_top}|#{pane_width}|#{pane_height}|#{cursor_x}|#{cursor_y}|#{cursor_flag}|#{pane_active}";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pane {
    id: String,
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    cursor_x: u16,
    cursor_y: u16,
    cursor: bool,
    active: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Layout {
    identity: String,
    cols: u16,
    rows: u16,
    panes: Vec<Pane>,
}
impl Layout {
    fn geometry(&self) -> Self {
        let mut layout = self.clone();
        for p in &mut layout.panes {
            p.cursor_x = 0;
            p.cursor_y = 0;
            p.cursor = false;
        }
        layout
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowFrame {
    pub cols: u16,
    pub rows: u16,
    pub bytes: Vec<u8>,
}
fn bad(message: &str) -> TmuxError {
    TmuxError::BadArgs(message.into())
}
fn parse(raw: &str) -> Result<Layout, TmuxError> {
    let mut layout: Option<Layout> = None;
    let mut ids = BTreeSet::new();
    for line in raw.lines() {
        let mut fields = line.split('|');
        let mut field = || fields.next().ok_or_else(|| bad("incomplete shadow layout"));
        let pid = field()?;
        let session = field()?;
        let window = field()?;
        if pid.is_empty()
            || !pid.bytes().all(|b| b.is_ascii_digit())
            || !id(session, '$')
            || !id(window, '@')
        {
            return Err(bad("invalid shadow identity"));
        }
        let cols = number(field()?)?;
        let rows = number(field()?)?;
        let geometry = field()?;
        if cols < 2
            || rows == 0
            || usize::from(cols) * usize::from(rows) > MAX_CELLS
            || geometry.is_empty()
        {
            return Err(bad("shadow geometry exceeds limit"));
        }
        let identity = format!("{pid}|{session}|{window}|{geometry}");
        let pane = field()?.to_string();
        if !id(&pane, '%') || !ids.insert(pane.clone()) {
            return Err(bad("invalid shadow pane"));
        }
        let left = number(field()?)?;
        let top = number(field()?)?;
        let width = number(field()?)?;
        let height = number(field()?)?;
        let cursor_x = number(field()?)?;
        let cursor_y = number(field()?)?;
        let cursor = boolean(field()?)?;
        let active = boolean(field()?)?;
        if fields.next().is_some()
            || width == 0
            || height == 0
            || u32::from(left) + u32::from(width) > u32::from(cols)
            || u32::from(top) + u32::from(height) > u32::from(rows)
            || cursor_x >= width
            || cursor_y >= height
        {
            return Err(bad("invalid shadow pane geometry"));
        }
        let entry = layout.get_or_insert_with(|| Layout {
            identity: identity.clone(),
            cols,
            rows,
            panes: Vec::new(),
        });
        if entry.identity != identity
            || entry.cols != cols
            || entry.rows != rows
            || entry.panes.len() >= MAX_PANES
        {
            return Err(bad("inconsistent shadow window"));
        }
        entry.panes.push(Pane {
            id: pane,
            left,
            top,
            width,
            height,
            cursor_x,
            cursor_y,
            cursor,
            active,
        });
    }
    layout.ok_or_else(|| bad("empty shadow layout"))
}
fn id(raw: &str, prefix: char) -> bool {
    raw.strip_prefix(prefix)
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}
fn number(raw: &str) -> Result<u16, TmuxError> {
    raw.parse().map_err(|_| bad("invalid shadow dimension"))
}
fn boolean(raw: &str) -> Result<bool, TmuxError> {
    match raw {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(bad("invalid shadow boolean")),
    }
}
fn socket(ctl: &TmuxCtl) -> Result<(u64, u64), TmuxError> {
    let meta = std::fs::metadata(ctl.socket_path()).map_err(|e| TmuxError::Spawn(e.to_string()))?;
    use std::os::unix::fs::FileTypeExt;
    if !meta.file_type().is_socket() {
        return Err(bad("shadow source is not a socket"));
    }
    Ok((meta.dev(), meta.ino()))
}
fn command(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).into()).collect()
}

/// One discovery and one grouped capture process per window, never one job per pane.
/// The entire capture shares a deadline and cooperative cancellation boundary.
pub fn capture_when(
    ctl: &TmuxCtl,
    session: &str,
    allowed: &dyn Fn() -> bool,
) -> Result<ShadowFrame, TmuxError> {
    capture_with(ctl, session, allowed, &|cmds, timeout, valid| {
        ctl.read_batch_when(cmds, timeout, valid)
    })
}
/// Injectable read boundary for fixture/oracle verification; production uses capture_when.
#[allow(clippy::type_complexity)]
pub fn capture_with(
    ctl: &TmuxCtl,
    session: &str,
    allowed: &dyn Fn() -> bool,
    read: &dyn Fn(
        &[Vec<String>],
        Duration,
        &dyn Fn() -> bool,
    ) -> Result<crate::tmux::TmuxOut, TmuxError>,
) -> Result<ShadowFrame, TmuxError> {
    if ctl.mode() != RunMode::Shadow || !crate::tab_actions::valid_session(session) || !allowed() {
        return Err(bad("shadow capture unavailable"));
    }
    let stamp = socket(ctl)?;
    let deadline = Instant::now() + CAPTURE_TIMEOUT;
    let valid = || allowed() && Instant::now() < deadline && socket(ctl).is_ok_and(|s| s == stamp);
    let target = format!("={session}:");
    let meta = command(&["list-panes", "-t", &target, "-F", FORMAT]);
    let before = read(
        std::slice::from_ref(&meta),
        deadline.saturating_duration_since(Instant::now()),
        &valid,
    )?;
    if !before.ok() || !valid() {
        return Err(bad("shadow discovery failed or cancelled"));
    }
    let layout = parse(&before.stdout)?;
    let mut commands: Vec<_> = layout
        .panes
        .iter()
        .map(|p| command(&["capture-pane", "-p", "-e", "-N", "-t", &p.id]))
        .collect();
    commands.push(meta);
    let out = read(
        &commands,
        deadline.saturating_duration_since(Instant::now()),
        &valid,
    )?;
    if !out.ok() || !valid() || out.stdout.len() > MAX_BYTES {
        return Err(bad("shadow capture failed or exceeded limit"));
    }
    let mut lines = out.stdout.split_inclusive('\n');
    let mut screens = Vec::new();
    for p in &layout.panes {
        let mut screen = Vec::new();
        for _ in 0..p.height {
            screen.push(lines.next().ok_or_else(|| bad("partial shadow pane"))?);
        }
        screens.push(screen);
    }
    let after = lines.collect::<String>();
    if parse(&after)?.geometry() != layout.geometry() || !valid() {
        return Err(bad("shadow window changed during capture"));
    }
    let mut bytes = b"\x1b[?1049l\x1b[0m\x1b[2J\x1b[H\x1b[?7l\x1b[?25l".to_vec();
    for (p, screen) in layout.panes.iter().zip(screens) {
        bytes.extend_from_slice(b"\x1b[0m");
        for (row, line) in screen.iter().enumerate() {
            bytes.extend_from_slice(
                format!("\x1b[{};{}H", usize::from(p.top) + row + 1, p.left + 1).as_bytes(),
            );
            bytes.extend_from_slice(sgr_text(line.trim_end_matches(['\r', '\n'])).as_bytes());
        }
        // Borders are local cells, never tmux resize/select commands.
        let bottom = p.top + p.height;
        if bottom < layout.rows {
            bytes.extend_from_slice(
                format!(
                    "\x1b[0m\x1b[{};{}H{}",
                    bottom + 1,
                    p.left + 1,
                    "─".repeat(usize::from(p.width))
                )
                .as_bytes(),
            );
        }
        let right = p.left + p.width;
        if right < layout.cols {
            for row in p.top..bottom {
                bytes.extend_from_slice(
                    format!("\x1b[0m\x1b[{};{}H│", row + 1, right + 1).as_bytes(),
                );
            }
        }
    }
    if let Some(p) = layout.panes.iter().find(|p| p.active) {
        bytes.extend_from_slice(
            format!(
                "\x1b[0m\x1b[{};{}H{}",
                p.top + p.cursor_y + 1,
                p.left + p.cursor_x + 1,
                if p.cursor { "\x1b[?25h" } else { "" }
            )
            .as_bytes(),
        );
    }
    if bytes.len() > MAX_BYTES || !valid() {
        return Err(bad("shadow frame cancelled or exceeded limit"));
    }
    Ok(ShadowFrame {
        cols: layout.cols,
        rows: layout.rows,
        bytes,
    })
}
/// Capture attributes are SGR; never replay OSC, DCS, replies or mouse/focus modes.
fn sgr_text(line: &str) -> String {
    let mut result = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.next() != Some('[') {
                continue;
            }
            let mut seq = String::from("\x1b[");
            while let Some(&n) = chars.peek() {
                if n.is_ascii_digit() || matches!(n, ';' | ':') {
                    seq.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            if chars.next() == Some('m') {
                seq.push('m');
                result.push_str(&seq);
            }
        } else if !c.is_control() {
            result.push(c);
        }
    }
    result
}

/// One idle worker and bounded request/result slots; visibility cancels its process.
pub struct ShadowReader {
    request: Option<SyncSender<u64>>,
    result: Receiver<(u64, Result<ShadowFrame, TmuxError>)>,
    closed: Arc<AtomicBool>,
    visible: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
    pending: bool,
}
impl ShadowReader {
    pub fn new(ctl: TmuxCtl, session: String) -> Result<Self, TmuxError> {
        if ctl.mode() != RunMode::Shadow || !crate::tab_actions::valid_session(&session) {
            return Err(bad("invalid shadow reader"));
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let (done, result) = mpsc::sync_channel(1);
        let closed = Arc::new(AtomicBool::new(false));
        let visible = Arc::new(AtomicBool::new(false));
        let generation = Arc::new(AtomicU64::new(0));
        let cancel = closed.clone();
        let mapped = visible.clone();
        let epoch = generation.clone();
        let worker = std::thread::Builder::new()
            .name("app-shadow-capture".into())
            .spawn(move || {
                while let Ok(ticket) = rx.recv() {
                    if cancel.load(Ordering::Acquire) {
                        break;
                    }
                    let allowed = || {
                        !cancel.load(Ordering::Acquire)
                            && mapped.load(Ordering::Acquire)
                            && epoch.load(Ordering::Acquire) == ticket
                    };
                    let frame = capture_when(&ctl, &session, &allowed);
                    let _ = done.try_send((ticket, frame));
                }
            })
            .map_err(|e| TmuxError::Spawn(e.to_string()))?;
        Ok(Self {
            request: Some(tx),
            result,
            closed,
            visible,
            generation,
            worker: Some(worker),
            pending: false,
        })
    }
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible.swap(visible, Ordering::AcqRel) != visible {
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
    }
    pub fn request(&mut self) -> bool {
        if self.pending
            || !self.visible.load(Ordering::Acquire)
            || self.closed.load(Ordering::Acquire)
        {
            return false;
        }
        let sent = self
            .request
            .as_ref()
            .is_some_and(|tx| tx.try_send(self.generation.load(Ordering::Acquire)).is_ok());
        self.pending = sent;
        sent
    }
    pub fn take(&mut self) -> Option<Result<ShadowFrame, TmuxError>> {
        let (ticket, result) = self.result.try_recv().ok()?;
        self.pending = false;
        (self.visible.load(Ordering::Acquire) && self.generation.load(Ordering::Acquire) == ticket)
            .then_some(result)
    }
}
impl Drop for ShadowReader {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        self.visible.store(false, Ordering::Release);
        self.request.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
