//! GTK terminal over the shared engine and a private nonblocking PTY.
use super::{
    engine::*,
    keys::{drag_button, key_action, mouse_button},
    paint::{CellGeom, FrameCache, LineKind, PaintOp},
    pty::{DrainOutcome, PtyDrain, PtyError, PtySession},
    schedule::PaintSchedule,
    settle::{RESPAWN_MS, SETTLE_CAP_MS, SETTLE_QUIET_MS, Settle, initial_size},
};
use crate::config::RunMode;
use glib::{ControlFlow, IOCondition, SourceId};
use gtk::prelude::*;
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

pub type TextCallback = Rc<dyn Fn(&str)>;
pub type ExitCallback = Rc<dyn Fn(i32)>;
pub type ScrollCallback = Rc<dyn Fn(&str, i32, u16, u16)>;
pub struct TermOptions {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub session: Option<String>,
    pub palette: Palette,
    pub scrollback: usize,
    pub preferences: Value,
    pub mode: RunMode,
    pub environment: Vec<(String, String)>,
    pub clear_env: bool,
    pub tmux_size: Option<(u16, u16)>,
    pub before_spawn: Option<Rc<dyn Fn() -> Result<(), String>>>,
    pub on_title: Option<TextCallback>,
    pub on_exit: Option<ExitCallback>,
    pub on_bell: Option<Rc<dyn Fn()>>,
    pub on_link: Option<TextCallback>,
    pub on_ssh_scroll: Option<ScrollCallback>,
}
#[derive(Debug)]
pub enum TermViewError {
    GtkUnavailable,
    UnsafeEnvironment,
    Pty(PtyError),
}
struct Model {
    engine: TermEngine,
    frame: FrameCache,
    pty: Option<PtySession>,
    drain: PtyDrain,
    geom: CellGeom,
    font: pango::FontDescription,
    font_scale: f64,
    opacity: f64,
    padding: f64,
    preferences: Value,
    settle: Settle,
    respawn_at: Option<u64>,
    schedule: PaintSchedule,
    focused: bool,
    blink_visible: bool,
    selection: Option<Selection>,
    pressed: Option<Point>,
    last_motion: Option<(u16, u16, gdk::ModifierType)>,
    ime: Ime,
    preedit: String,
    cursor_shape: CursorShape,
    wheel_delta: f64,
    ssh_delta: f64,
    ssh_point: (u16, u16),
    ssh_at: Option<u64>,
}
struct Inner {
    area: gtk::DrawingArea,
    im: gtk::IMMulticontext,
    options: TermOptions,
    epoch: Instant,
    model: RefCell<Model>,
    read_source: RefCell<Option<SourceId>>,
    write_source: RefCell<Option<SourceId>>,
    timer: RefCell<Option<SourceId>>,
    closed: Cell<bool>,
}
pub struct TermView {
    inner: Rc<Inner>,
}
impl TermView {
    pub fn new(options: TermOptions) -> Result<Self, TermViewError> {
        if !gtk::is_initialized_main_thread() {
            return Err(TermViewError::GtkUnavailable);
        }
        if options.mode == RunMode::Sandbox && !options.clear_env {
            return Err(TermViewError::UnsafeEnvironment);
        }
        let area = gtk::DrawingArea::new();
        area.set_can_focus(true);
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.add_events(
            gdk::EventMask::KEY_PRESS_MASK
                | gdk::EventMask::KEY_RELEASE_MASK
                | gdk::EventMask::BUTTON_PRESS_MASK
                | gdk::EventMask::BUTTON_RELEASE_MASK
                | gdk::EventMask::POINTER_MOTION_MASK
                | gdk::EventMask::SCROLL_MASK
                | gdk::EventMask::SMOOTH_SCROLL_MASK
                | gdk::EventMask::FOCUS_CHANGE_MASK,
        );
        let epoch = Instant::now();
        let preferences = options.preferences.clone();
        let inner = Rc::new(Inner {
            area,
            im: gtk::IMMulticontext::new(),
            epoch,
            model: RefCell::new(Model {
                engine: TermEngine::new(
                    80,
                    24,
                    options.scrollback,
                    options.palette.clone(),
                    true,
                    epoch,
                ),
                frame: FrameCache::default(),
                pty: None,
                drain: PtyDrain::default(),
                geom: CellGeom {
                    cell_w: 8.,
                    cell_h: 19.,
                    origin_x: 12.,
                    origin_y: 10.,
                    dpr: 1.,
                    font_size: 13.,
                },
                font: pango::FontDescription::from_string("monospace 13"),
                font_scale: 1.,
                opacity: 1.,
                padding: 10.,
                preferences,
                settle: Settle::new(SETTLE_QUIET_MS, SETTLE_CAP_MS, 0),
                respawn_at: None,
                schedule: PaintSchedule::default(),
                focused: false,
                blink_visible: true,
                selection: None,
                pressed: None,
                last_motion: None,
                ime: Ime::default(),
                preedit: String::new(),
                cursor_shape: CursorShape::Block,
                wheel_delta: 0.,
                ssh_delta: 0.,
                ssh_point: (0, 0),
                ssh_at: None,
            }),
            options,
            read_source: RefCell::new(None),
            write_source: RefCell::new(None),
            timer: RefCell::new(None),
            closed: Cell::new(false),
        });
        inner.configure_font();
        connect_events(&inner);
        inner.arm();
        // Standalone terminals start immediately; session clients wait for size.
        if inner.options.session.is_none() {
            inner.spawn().map_err(TermViewError::Pty)?;
        }
        Ok(Self { inner })
    }
    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.inner.area
    }
    pub fn feed(&self, bytes: &[u8]) -> Result<(), PtyError> {
        self.inner.send(bytes)
    }
    pub fn paste(&self, text: &str) -> Result<(), PtyError> {
        let bytes = encode_paste(text, &self.inner.model.borrow().engine.modes());
        self.feed(&bytes)
    }
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyError> {
        self.inner.resize(cols, rows)
    }
    pub fn session(&self) -> Option<&str> {
        self.inner.options.session.as_deref()
    }
    pub fn child_tty(&self) -> Option<String> {
        self.inner
            .model
            .borrow()
            .pty
            .as_ref()
            .and_then(PtySession::child_tty)
    }
    pub fn selection_text(&self) -> Option<String> {
        self.inner.selection_text()
    }
    pub fn set_font_scale(&self, scale: f64) {
        if scale.is_finite() {
            self.inner.model.borrow_mut().font_scale = scale.clamp(0.5, 3.);
            self.inner.configure_font();
            self.inner.allocate();
        }
    }
    pub fn set_preferences(&self, prefs: &Value) {
        self.inner.model.borrow_mut().preferences = prefs.clone();
        self.inner.configure_font();
        self.inner.allocate();
    }
    pub fn set_palette(&self, palette: Palette) {
        self.inner.model.borrow_mut().engine.set_palette(palette);
        self.inner.area.queue_draw();
    }
}
impl Inner {
    fn ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
    fn configure_font(&self) {
        let mut m = self.model.borrow_mut();
        let family = m
            .preferences
            .get("font_family")
            .and_then(Value::as_str)
            .unwrap_or("Ubuntu Sans Mono");
        let size = m
            .preferences
            .get("font_size")
            .and_then(Value::as_f64)
            .unwrap_or(13.)
            .clamp(6., 72.)
            * m.font_scale;
        let mut font = pango::FontDescription::new();
        font.set_family(&format!(
            "{family},JetBrainsMono Nerd Font,JetBrains Mono,DejaVu Sans Mono,Monospace"
        ));
        font.set_size((size * f64::from(pango::SCALE)).round() as i32);
        let layout = self.area.create_pango_layout(Some("M"));
        layout.set_font_description(Some(&font));
        let (width, height) = layout.pixel_size();
        let line_scale = 1.2; // cc-app TERM_LINE_SCALE, not a preference.
        m.font = font;
        m.geom.cell_w = f64::from(width.max(1));
        m.geom.cell_h = (f64::from(height.max(1)) * line_scale).ceil();
        m.geom.font_size = size * 96. / 72.;
        m.geom.dpr = f64::from(self.area.scale_factor().max(1));
        m.padding = m
            .preferences
            .get("terminal_padding")
            .and_then(Value::as_f64)
            .unwrap_or(8.)
            .clamp(0., 40.);
        m.geom.origin_x = m.padding + 2.;
        m.geom.origin_y = m.padding + 14.;
        m.opacity = m
            .preferences
            .get("terminal_opacity")
            .and_then(Value::as_f64)
            .unwrap_or(100.)
            .clamp(30., 100.)
            / 100.;
        m.cursor_shape = match m.preferences.get("cursor_shape").and_then(Value::as_str) {
            Some("ibeam") => CursorShape::Beam,
            Some("underline") => CursorShape::Underline,
            _ => CursorShape::Block,
        };
        self.area.queue_draw();
    }
    fn dimensions(&self) -> (u16, u16) {
        let m = self.model.borrow();
        initial_size(
            (self.area.allocated_width(), self.area.allocated_height()),
            m.geom.cell_w,
            m.geom.cell_h,
            (
                m.geom.origin_x + (m.padding - 4.).max(0.),
                m.geom.origin_y + (m.padding - 2.).max(0.),
            ),
            self.options.tmux_size,
        )
    }
    fn allocate(self: &Rc<Self>) {
        self.model.borrow_mut().settle.on_alloc(self.ms());
        if self.model.borrow().pty.is_some() {
            let (c, r) = self.dimensions();
            let _ = self.resize(c, r);
        }
        self.arm();
    }
    fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyError> {
        let mut m = self.model.borrow_mut();
        if let Some(p) = &m.pty {
            p.resize(cols, rows)?;
        }
        let px = (
            m.geom.cell_w.round().clamp(1., f64::from(u16::MAX)) as u16,
            m.geom.cell_h.round().clamp(1., f64::from(u16::MAX)) as u16,
        );
        m.engine.resize(cols, rows, px);
        self.area.queue_draw();
        Ok(())
    }
    fn spawn(self: &Rc<Self>) -> Result<(), PtyError> {
        if let Some(before) = &self.options.before_spawn {
            before().map_err(PtyError::Spawn)?;
        }
        let (cols, rows) = self.dimensions();
        let p = PtySession::spawn_with_env(
            &self.options.argv,
            cols,
            rows,
            &self.options.cwd,
            &self.options.environment,
            self.options.clear_env,
        )?;
        let fd = p.raw_fd();
        {
            let mut model = self.model.borrow_mut();
            model.pty = Some(p);
            model.drain = PtyDrain::default();
        }
        self.resize(cols, rows)?;
        if let Some(source) = self.read_source.borrow_mut().take() {
            source.remove();
        }
        let weak = Rc::downgrade(self);
        *self.read_source.borrow_mut() = Some(glib::source::unix_fd_add_local(
            fd,
            IOCondition::IN | IOCondition::HUP | IOCondition::ERR,
            move |_, condition| {
                let Some(inner) = weak.upgrade() else {
                    return ControlFlow::Break;
                };
                if inner.read(condition) {
                    ControlFlow::Continue
                } else {
                    inner.read_source.borrow_mut().take();
                    ControlFlow::Break
                }
            },
        ));
        Ok(())
    }
    fn send(self: &Rc<Self>, bytes: &[u8]) -> Result<(), PtyError> {
        let mut m = self.model.borrow_mut();
        let Some(p) = m.pty.as_mut() else {
            return Err(PtyError::Io("PTY not attached".into()));
        };
        p.write(bytes)?;
        drop(m);
        self.watch_write();
        Ok(())
    }
    fn send_event(self: &Rc<Self>, bytes: &[u8]) {
        if let Err(error) = self.send(bytes) {
            let message = format!("terminal input: {error:?}");
            if let Some(callback) = &self.options.on_title {
                callback(&message);
            } else {
                eprintln!("{message}");
            }
        }
    }
    fn watch_write(self: &Rc<Self>) {
        if self.write_source.borrow().is_some() {
            return;
        }
        let fd = self
            .model
            .borrow()
            .pty
            .as_ref()
            .filter(|p| p.has_pending())
            .map(PtySession::raw_fd);
        let Some(fd) = fd else {
            return;
        };
        let weak = Rc::downgrade(self);
        *self.write_source.borrow_mut() = Some(glib::source::unix_fd_add_local(
            fd,
            IOCondition::OUT | IOCondition::HUP | IOCondition::ERR,
            move |_, condition| {
                let Some(inner) = weak.upgrade() else {
                    return ControlFlow::Break;
                };
                let keep = if condition.intersects(IOCondition::HUP | IOCondition::ERR) {
                    false
                } else {
                    inner
                        .model
                        .borrow_mut()
                        .pty
                        .as_mut()
                        .is_some_and(|p| matches!(p.flush_pending(), Ok(false)))
                };
                if keep {
                    ControlFlow::Continue
                } else {
                    inner.write_source.borrow_mut().take();
                    ControlFlow::Break
                }
            },
        ));
    }
    fn read(self: &Rc<Self>, condition: IOCondition) -> bool {
        let mut m = self.model.borrow_mut();
        let outcome = {
            let Model {
                pty, engine, drain, ..
            } = &mut *m;
            pty.as_mut().map_or(DrainOutcome::Closed, |pty| {
                drain.read_into(pty, |bytes| engine.feed(bytes, Instant::now()))
            })
        };
        let closed = outcome == DrainOutcome::Closed
            || (outcome == DrainOutcome::WouldBlock
                && condition.intersects(IOCondition::HUP | IOCondition::ERR));
        if closed {
            m.drain.close();
        }
        let drained = m.engine.drain();
        let mut lines = Vec::new();
        m.engine.take_dirty(&mut lines);
        m.schedule.damage(self.ms(), &lines);
        drop(m);
        if !drained.replies.is_empty() {
            self.send_event(&drained.replies);
        }
        if let Some(title) = drained.title
            && let Some(callback) = &self.options.on_title
        {
            callback(&title);
        }
        if drained.bell
            && let Some(callback) = &self.options.on_bell
        {
            callback();
        }
        if let Some(text) = drained.clipboard {
            let target = if drained.clipboard_target == Some(ClipboardTarget::Selection) {
                gdk::SELECTION_PRIMARY
            } else {
                gdk::SELECTION_CLIPBOARD
            };
            gtk::Clipboard::get(&target).set_text(&text);
        }
        self.arm();
        !closed
    }
    fn arm(self: &Rc<Self>) {
        if self.closed.get() {
            return;
        }
        if let Some(id) = self.timer.borrow_mut().take() {
            id.remove();
        }
        let now = self.ms();
        let m = self.model.borrow();
        // fd readiness drives IO; the low-rate fallback observes exit status.
        let mut delay = if m.drain.has_exited() && m.drain.finished().is_none() {
            1
        } else {
            100
        };
        if let Some(paint) = m.schedule.next_delay_ms(now) {
            delay = delay.min(paint);
        }
        if m.pty.is_none() {
            let at = m.respawn_at.or_else(|| m.settle.next_check_ms(now));
            if let Some(at) = at {
                delay = delay.min(at.saturating_sub(now));
            }
        }
        if let Some(at) = m.ssh_at {
            delay = delay.min(at.saturating_sub(now));
        }
        if let Some(at) = m.engine.next_deadline() {
            let ms = u64::try_from(at.saturating_duration_since(self.epoch).as_millis())
                .unwrap_or(u64::MAX);
            delay = delay.min(ms.saturating_sub(now));
        }
        drop(m);
        let weak = Rc::downgrade(self);
        *self.timer.borrow_mut() = Some(glib::timeout_add_local(
            Duration::from_millis(delay.max(1)),
            move || {
                if let Some(inner) = weak.upgrade() {
                    inner.timer.borrow_mut().take();
                    if !inner.closed.get() {
                        inner.tick();
                        inner.arm();
                    }
                }
                ControlFlow::Break
            },
        ));
    }
    fn tick(self: &Rc<Self>) {
        let now = self.ms();
        let mut m = self.model.borrow_mut();
        let spawn = m.pty.is_none()
            && (m.respawn_at.is_some_and(|at| now >= at)
                || (m.respawn_at.is_none() && m.settle.due(now)));
        if spawn {
            m.settle.fired();
            m.respawn_at = None;
            drop(m);
            if let Err(error) = self.spawn() {
                if let Some(callback) = &self.options.on_title {
                    callback(&format!("terminal: {error:?}"));
                }
                self.model.borrow_mut().respawn_at = Some(now.saturating_add(RESPAWN_MS));
            }
            m = self.model.borrow_mut();
        }
        {
            let Model { pty, drain, .. } = &mut *m;
            if let Some(pty) = pty.as_mut() {
                drain.observe_exit(pty);
            }
        }
        if m.drain.has_exited() && m.drain.finished().is_none() {
            drop(m);
            if !self.read(IOCondition::empty()) {
                if let Some(source) = self.read_source.borrow_mut().take() {
                    source.remove();
                }
            }
            m = self.model.borrow_mut();
        }
        let exit = m.drain.finished();
        if let Some(code) = exit {
            m.pty.take();
            m.drain = PtyDrain::default();
            m.respawn_at = Some(now.saturating_add(RESPAWN_MS));
            drop(m);
            if let Some(source) = self.read_source.borrow_mut().take() {
                source.remove();
            }
            if let Some(source) = self.write_source.borrow_mut().take() {
                source.remove();
            }
            if let Some(callback) = &self.options.on_exit {
                callback(code);
            }
            m = self.model.borrow_mut();
        }
        m.engine.tick(Instant::now());
        let mut lines = Vec::new();
        m.engine.take_dirty(&mut lines);
        m.schedule.damage(now, &lines);
        let blink = m
            .preferences
            .get("cursor_blink")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let synchronized = m.engine.next_deadline().is_some();
        m.schedule.set_synchronized(synchronized);
        if blink && m.schedule.blink_due(now) {
            m.blink_visible = !m.blink_visible;
            m.schedule.blinked(now);
            self.area.queue_draw();
        }
        if m.schedule.next_delay_ms(now) == Some(0) {
            self.area.queue_draw();
        }
        let ssh = if m.ssh_at.is_some_and(|at| now >= at) {
            m.ssh_at = None;
            let delta = m.ssh_delta;
            m.ssh_delta = 0.;
            let mut n = delta.round_ties_even() as i32;
            if n == 0 && delta.abs() >= 0.25 {
                n = if delta > 0. { 1 } else { -1 };
            }
            Some((n, m.ssh_point))
        } else {
            None
        };
        drop(m);
        if let Some((delta, (col, row))) = ssh
            && delta != 0
            && let (Some(callback), Some(session)) =
                (&self.options.on_ssh_scroll, self.options.session.as_deref())
        {
            callback(session, delta, col, row);
        }
    }
    fn selection_text(&self) -> Option<String> {
        let m = self.model.borrow();
        m.selection
            .map(|s| selected_text(m.engine.engine(), &s))
            .filter(|text| !text.is_empty())
    }
    fn point(&self, x: f64, y: f64) -> (u16, u16) {
        let m = self.model.borrow();
        let (cols, rows) = m.engine.size();
        let col = ((x - m.geom.origin_x).max(0.) / m.geom.cell_w).floor() as u16;
        let row = ((y - m.geom.origin_y).max(0.) / m.geom.cell_h).floor() as u16;
        (
            col.min(cols.saturating_sub(1)),
            row.min(rows.saturating_sub(1)),
        )
    }
    fn absolute(&self, col: u16, row: u16) -> Point {
        let offset =
            i32::try_from(self.model.borrow().engine.engine().display_offset()).unwrap_or(i32::MAX);
        (i32::from(row).saturating_sub(offset), col)
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.closed.set(true);
        for source in [
            &mut self.read_source,
            &mut self.write_source,
            &mut self.timer,
        ] {
            if let Some(id) = source.get_mut().take() {
                id.remove();
            }
        }
        self.model.get_mut().pty.take();
    }
}

fn modifiers(state: gdk::ModifierType) -> (bool, bool, bool) {
    (
        state.contains(gdk::ModifierType::CONTROL_MASK),
        state.contains(gdk::ModifierType::MOD1_MASK),
        state.contains(gdk::ModifierType::SHIFT_MASK),
    )
}
fn connect_events(inner: &Rc<Inner>) {
    let weak = Rc::downgrade(inner);
    inner.area.connect_destroy(move |_| {
        if let Some(inner) = weak.upgrade() {
            inner.closed.set(true);
            for source in [&inner.read_source, &inner.write_source, &inner.timer] {
                if let Some(id) = source.borrow_mut().take() {
                    id.remove();
                }
            }
            inner.model.borrow_mut().pty.take();
        }
    });
    let weak = Rc::downgrade(inner);
    inner.im.connect_preedit_end(move |_| {
        if let Some(inner) = weak.upgrade() {
            let mut m = inner.model.borrow_mut();
            m.ime.end("");
            m.preedit.clear();
        }
    });
    let targets = gtk::TargetList::new(&[]);
    targets.add_uri_targets(0);
    targets.add_text_targets(1);
    inner
        .area
        .drag_dest_set(gtk::DestDefaults::ALL, &[], gdk::DragAction::COPY);
    inner.area.drag_dest_set_target_list(Some(&targets));
    let weak = Rc::downgrade(inner);
    inner
        .area
        .connect_drag_data_received(move |_, context, _, _, data, _, time| {
            let mut success = false;
            if let Some(inner) = weak.upgrade() {
                let uris = data.uris();
                let text = if uris.is_empty() {
                    data.text().map(|s| s.to_string())
                } else {
                    Some(
                        uris.iter()
                            .map(|uri| {
                                let raw = if uri.starts_with("file:") {
                                    glib::filename_from_uri(uri)
                                        .ok()
                                        .map(|(p, _)| p.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| uri.to_string())
                                } else {
                                    uri.to_string()
                                };
                                format!("'{}'", raw.replace('\'', "'\\''"))
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                            + " ",
                    )
                };
                if let Some(text) = text {
                    let bytes = encode_paste(&text, &inner.model.borrow().engine.modes());
                    success = inner.send(&bytes).is_ok();
                }
            }
            context.drag_finish(success, false, time);
        });

    let weak = Rc::downgrade(inner);
    inner.area.connect_realize(move |area| {
        if let Some(inner) = weak.upgrade() {
            inner.im.set_client_window(area.window().as_ref());
            inner.configure_font();
        }
    });
    let weak = Rc::downgrade(inner);
    inner.area.connect_unrealize(move |_| {
        if let Some(inner) = weak.upgrade() {
            inner.im.set_client_window(None);
        }
    });
    let weak = Rc::downgrade(inner);
    inner.area.connect_size_allocate(move |_, _| {
        if let Some(inner) = weak.upgrade() {
            inner.allocate();
        }
    });
    let weak = Rc::downgrade(inner);
    inner.area.connect_draw(move |_, context| {
        if let Some(inner) = weak.upgrade() {
            inner.draw(context);
        }
        glib::Propagation::Stop
    });
    let weak = Rc::downgrade(inner);
    inner.im.connect_preedit_start(move |_| {
        if let Some(inner) = weak.upgrade() {
            inner.model.borrow_mut().ime.start();
        }
    });
    let weak = Rc::downgrade(inner);
    inner.im.connect_preedit_changed(move |im| {
        if let Some(inner) = weak.upgrade() {
            inner.model.borrow_mut().preedit = im.preedit_string().0.to_string();
            inner.area.queue_draw();
        }
    });
    let weak = Rc::downgrade(inner);
    inner.im.connect_commit(move |_, text| {
        if let Some(inner) = weak.upgrade() {
            let bytes = {
                let mut m = inner.model.borrow_mut();
                m.preedit.clear();
                m.ime.end(text)
            };
            if let Some(bytes) = bytes {
                inner.send_event(&bytes);
            }
            inner.area.queue_draw();
        }
    });
    let weak = Rc::downgrade(inner);
    inner.area.connect_key_press_event(move |_, event| {
        let Some(inner) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        if inner.im.filter_keypress(event) {
            return glib::Propagation::Stop;
        }
        let mut m = inner.model.borrow_mut();
        if m.ime.keydown_ignored(0) {
            return glib::Propagation::Stop;
        }
        let action = key_action(event.keyval(), event.state(), &m.engine.modes());
        m.ime.forget_commit();
        drop(m);
        match action {
            KeyAction::Send(bytes) => {
                inner.send_event(&bytes);
                glib::Propagation::Stop
            }
            KeyAction::ScrollPage(direction) => {
                let mut m = inner.model.borrow_mut();
                let rows = i32::from(m.engine.size().1);
                m.engine.scroll(-direction * rows);
                inner.area.queue_draw();
                glib::Propagation::Stop
            }
            KeyAction::None => glib::Propagation::Proceed,
        }
    });
    let weak = Rc::downgrade(inner);
    inner.area.connect_key_release_event(move |_, event| {
        weak.upgrade().map_or(glib::Propagation::Proceed, |inner| {
            if inner.im.filter_keypress(event) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        })
    });
    for on in [true, false] {
        let weak = Rc::downgrade(inner);
        let callback = move |_: &gtk::DrawingArea, _: &gdk::EventFocus| {
            if let Some(inner) = weak.upgrade() {
                if on {
                    inner.im.focus_in();
                } else {
                    inner.im.focus_out();
                }
                let mut m = inner.model.borrow_mut();
                m.focused = on;
                m.blink_visible = true;
                m.schedule.set_focused(on, inner.ms());
                let bytes = focus(on, &m.engine.modes());
                drop(m);
                if let Some(bytes) = bytes {
                    inner.send_event(bytes);
                }
                inner.area.queue_draw();
            }
            glib::Propagation::Proceed
        };
        if on {
            inner.area.connect_focus_in_event(callback);
        } else {
            inner.area.connect_focus_out_event(callback);
        }
    }
    let weak = Rc::downgrade(inner);
    inner.area.connect_button_press_event(move |area, event| {
        let Some(inner) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        area.grab_focus();
        let (x, y) = event.position();
        let (col, row) = inner.point(x, y);
        let mods = modifiers(event.state());
        if event.button() == 2
            && (mods.2 || inner.model.borrow().engine.modes().mouse == MouseMode::Off)
        {
            let weak = Rc::downgrade(&inner);
            gtk::Clipboard::get(&gdk::SELECTION_PRIMARY).request_text(move |_, text| {
                if let (Some(inner), Some(text)) = (weak.upgrade(), text) {
                    let bytes = encode_paste(text, &inner.model.borrow().engine.modes());
                    inner.send_event(&bytes);
                }
            });
            return glib::Propagation::Stop;
        }
        let button = mouse_button(event.button());
        let bytes = encode_mouse(
            button,
            MouseKind::Press,
            col,
            row,
            mods,
            &inner.model.borrow().engine.modes(),
        );
        if let Some(bytes) = bytes.filter(|_| !mods.2) {
            inner.send_event(&bytes);
            return glib::Propagation::Stop;
        }
        if event.button() == 1 {
            let point = inner.absolute(col, row);
            let mut m = inner.model.borrow_mut();
            let mode = match event.event_type() {
                gdk::EventType::DoubleButtonPress => SelectMode::Word,
                gdk::EventType::TripleButtonPress => SelectMode::Line,
                _ => {
                    if mods.1 {
                        SelectMode::Block
                    } else {
                        SelectMode::Simple
                    }
                }
            };
            m.pressed = Some(point);
            m.selection = if mode == SelectMode::Simple {
                None
            } else {
                Some(Selection {
                    anchor: point,
                    head: point,
                    mode,
                })
            };
            inner.area.queue_draw();
        }
        glib::Propagation::Stop
    });
    let weak = Rc::downgrade(inner);
    inner.area.connect_motion_notify_event(move |area, event| {
        let Some(inner) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let (x, y) = event.position();
        let (col, row) = inner.point(x, y);
        let point = inner.absolute(col, row);
        let state = event.state();
        let mut m = inner.model.borrow_mut();
        if m.last_motion == Some((col, row, state)) {
            return glib::Propagation::Stop;
        }
        m.last_motion = Some((col, row, state));
        let button = drag_button(state);
        let bytes = encode_mouse(
            button,
            MouseKind::Move,
            col,
            row,
            modifiers(state),
            &m.engine.modes(),
        );
        if let Some(bytes) = bytes.filter(|_| !state.contains(gdk::ModifierType::SHIFT_MASK)) {
            drop(m);
            inner.send_event(&bytes);
            return glib::Propagation::Stop;
        }
        if state.contains(gdk::ModifierType::BUTTON1_MASK) {
            if m.selection.is_none()
                && let Some(anchor) = m.pressed
            {
                m.selection = Some(Selection {
                    anchor,
                    head: point,
                    mode: SelectMode::Simple,
                });
            }
            if let Some(selection) = m.selection.as_mut() {
                selection.head = point;
            }
            area.queue_draw();
        }
        let linked = link_at(&m, point, row, col).is_some();
        drop(m);
        if let Some(window) = area.window() {
            window.set_cursor(
                gdk::Cursor::from_name(&window.display(), if linked { "pointer" } else { "text" })
                    .as_ref(),
            );
        }
        glib::Propagation::Stop
    });
    let weak = Rc::downgrade(inner);
    inner.area.connect_button_release_event(move |_, event| {
        let Some(inner) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let (x, y) = event.position();
        let (col, row) = inner.point(x, y);
        let point = inner.absolute(col, row);
        let mods = modifiers(event.state());
        let mut m = inner.model.borrow_mut();
        let bytes = encode_mouse(
            mouse_button(event.button()),
            MouseKind::Release,
            col,
            row,
            mods,
            &m.engine.modes(),
        );
        if let Some(bytes) = bytes.filter(|_| !mods.2) {
            drop(m);
            inner.send_event(&bytes);
            return glib::Propagation::Stop;
        }
        let clicked = m.pressed.take() == Some(point) && m.selection.is_none();
        let link = if clicked {
            link_at(&m, point, row, col)
        } else {
            None
        };
        drop(m);
        if let Some(link) = link {
            inner.model.borrow_mut().selection = None;
            if let Some(callback) = &inner.options.on_link {
                callback(&link);
            }
        } else if let Some(text) = inner.selection_text() {
            gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD).set_text(&text);
            gtk::Clipboard::get(&gdk::SELECTION_PRIMARY).set_text(&text);
        }
        inner.area.queue_draw();
        glib::Propagation::Stop
    });
    let weak = Rc::downgrade(inner);
    inner.area.connect_scroll_event(move |_, event| {
        let Some(inner) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let (x, y) = event.position();
        let (col, row) = inner.point(x, y);
        let amount = match event.direction() {
            gdk::ScrollDirection::Up => -3.,
            gdk::ScrollDirection::Down => 3.,
            gdk::ScrollDirection::Smooth => event
                .scroll_deltas()
                .map_or(0., |(_, dy)| (dy * 2.).clamp(-8., 8.)),
            _ => 0.,
        };
        let mut m = inner.model.borrow_mut();
        let ssh = inner
            .options
            .session
            .as_deref()
            .is_some_and(|s| s.starts_with("ssh-") || s.starts_with("sshtab-"));
        if ssh && m.selection.is_none() && inner.options.on_ssh_scroll.is_some() {
            m.ssh_delta = (m.ssh_delta + amount).clamp(-24., 24.);
            m.ssh_point = (col, row);
            if m.ssh_at.is_none() {
                m.ssh_at = Some(inner.ms().saturating_add(45));
            }
            return glib::Propagation::Stop;
        }
        m.wheel_delta += amount;
        let lines = m.wheel_delta.trunc() as i32;
        m.wheel_delta -= f64::from(lines);
        match wheel(lines, col, row, modifiers(event.state()), &m.engine.modes()) {
            WheelAction::Mouse(bytes) | WheelAction::Arrows(bytes) => {
                drop(m);
                inner.send_event(&bytes);
            }
            WheelAction::Scroll(lines) => {
                m.engine.scroll(-lines);
                inner.area.queue_draw();
            }
        }
        glib::Propagation::Stop
    });
}
fn color(context: &cairo::Context, rgb: [u8; 3]) {
    context.set_source_rgb(
        f64::from(rgb[0]) / 255.,
        f64::from(rgb[1]) / 255.,
        f64::from(rgb[2]) / 255.,
    );
}
impl Inner {
    fn draw(&self, context: &cairo::Context) {
        let mut m = self.model.borrow_mut();
        let palette = m.engine.palette();
        context.set_source_rgba(
            f64::from(palette.bg[0]) / 255.,
            f64::from(palette.bg[1]) / 255.,
            f64::from(palette.bg[2]) / 255.,
            m.opacity,
        );
        context.set_operator(cairo::Operator::Source);
        let _ = context.paint();
        context.set_operator(cairo::Operator::Over);
        let layout = self.area.create_pango_layout(None);
        let (_, rows) = m.engine.size();
        {
            let Model {
                engine,
                frame,
                geom,
                ..
            } = &mut *m;
            frame.refresh(engine, geom);
        }
        for op in m.frame.ops() {
            paint_op(context, &layout, &m.font, &m.geom, op);
        }
        if let Some(selection) = m.selection
            && let Some((start, end)) = selection_bounds(m.engine.engine(), &selection)
        {
            let offset = i32::try_from(m.engine.engine().display_offset()).unwrap_or(i32::MAX);
            color(context, m.engine.palette().selection);
            for row in 0..rows {
                let absolute = i32::from(row).saturating_sub(offset);
                if absolute < start.0 || absolute > end.0 {
                    continue;
                }
                let (cols, _) = m.engine.size();
                let (left, right) = if selection.mode == SelectMode::Block {
                    (start.1, end.1)
                } else {
                    (
                        if absolute == start.0 { start.1 } else { 0 },
                        if absolute == end.0 {
                            end.1
                        } else {
                            cols.saturating_sub(1)
                        },
                    )
                };
                context.rectangle(
                    m.geom.origin_x + f64::from(left) * m.geom.cell_w,
                    m.geom.origin_y + f64::from(row) * m.geom.cell_h,
                    f64::from(right.saturating_sub(left).saturating_add(1)) * m.geom.cell_w,
                    m.geom.cell_h,
                );
                context.set_source_rgba(
                    f64::from(m.engine.palette().selection[0]) / 255.,
                    f64::from(m.engine.palette().selection[1]) / 255.,
                    f64::from(m.engine.palette().selection[2]) / 255.,
                    0.35,
                );
                let _ = context.fill();
            }
        }
        let cursor = m.frame.cursor();
        let blink = m
            .preferences
            .get("cursor_blink")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if cursor.visible && (!blink || !m.focused || m.blink_visible) {
            let x = m.geom.origin_x + f64::from(cursor.col) * m.geom.cell_w;
            let y = m.geom.origin_y + cursor.line as f64 * m.geom.cell_h;
            let width = m.geom.cell_w * if cursor.wide { 2. } else { 1. };
            color(context, m.engine.palette().cursor);
            let shape = if cursor.shape == CursorShape::Block {
                m.cursor_shape
            } else {
                cursor.shape
            };
            if !m.focused {
                context.rectangle(x + 0.5, y + 0.5, width - 1., m.geom.cell_h - 1.);
                context.set_line_width(1.);
                let _ = context.stroke();
            } else {
                match shape {
                    CursorShape::Beam => context.rectangle(x, y, 2., m.geom.cell_h),
                    CursorShape::Underline => {
                        context.rectangle(x, y + m.geom.cell_h - 2., width, 2.)
                    }
                    CursorShape::Block => context.rectangle(x, y, width, m.geom.cell_h),
                    CursorShape::HollowBlock => {
                        context.rectangle(x + 0.5, y + 0.5, width - 1., m.geom.cell_h - 1.);
                        context.set_line_width(1.);
                        let _ = context.stroke();
                    }
                    CursorShape::Hidden => {}
                }
                let _ = context.fill();
                if shape == CursorShape::Block {
                    let _ = context.save();
                    context.rectangle(x, y, width, m.geom.cell_h);
                    context.clip();
                    for op in m.frame.ops() {
                        let mut op = op.clone();
                        match &mut op {
                            PaintOp::Text {
                                y: row_y,
                                col,
                                cells,
                                rgb,
                                ..
                            } if *row_y == y
                                && *col <= cursor.col
                                && cursor.col < col.saturating_add(*cells) =>
                            {
                                *rgb = m.engine.palette().cursor_accent;
                                paint_op(context, &layout, &m.font, &m.geom, &op);
                            }
                            PaintOp::Glyph {
                                x: cell_x,
                                y: row_y,
                                rgb,
                                ..
                            } if *row_y == y && *cell_x == x => {
                                *rgb = m.engine.palette().cursor_accent;
                                paint_op(context, &layout, &m.font, &m.geom, &op);
                            }
                            _ => {}
                        }
                    }
                    let _ = context.restore();
                }
            }
        }
        if !m.preedit.is_empty() {
            layout.set_font_description(Some(&m.font));
            layout.set_text(&m.preedit);
            color(context, m.engine.palette().fg);
            context.move_to(
                m.geom.origin_x + f64::from(cursor.col) * m.geom.cell_w,
                m.geom.origin_y + cursor.line as f64 * m.geom.cell_h,
            );
            pangocairo::functions::show_layout(context, &layout);
        }
        m.schedule.painted(self.ms());
    }
}
fn paint_op(
    context: &cairo::Context,
    layout: &pango::Layout,
    font: &pango::FontDescription,
    g: &CellGeom,
    op: &PaintOp,
) {
    match op {
        PaintOp::Rect { x, y, w, h, rgb } => {
            color(context, *rgb);
            context.rectangle(*x, *y, *w, *h);
            let _ = context.fill();
        }
        PaintOp::Text {
            x,
            y,
            text,
            cells,
            bold,
            italic,
            rgb,
            ..
        } => {
            let mut font = font.clone();
            font.set_weight(if *bold {
                pango::Weight::Bold
            } else {
                pango::Weight::Normal
            });
            font.set_style(if *italic {
                pango::Style::Italic
            } else {
                pango::Style::Normal
            });
            layout.set_font_description(Some(&font));
            color(context, *rgb);
            if *cells <= 2 && text.chars().count() != usize::from(*cells) {
                layout.set_text(text);
                context.move_to(*x, *y);
                pangocairo::functions::show_layout(context, layout);
            } else {
                for (index, character) in text.chars().enumerate() {
                    layout.set_text(&character.to_string());
                    context.move_to(*x + index as f64 * g.cell_w, *y);
                    pangocairo::functions::show_layout(context, layout);
                }
            }
        }
        PaintOp::Line { x, y, w, rgb, kind } => {
            color(context, *rgb);
            let y = match kind {
                LineKind::Strike => *y + g.cell_h * 0.5,
                _ => *y + g.cell_h - 2.,
            };
            context.set_line_width(1.);
            let dash = match kind {
                LineKind::Under(Underline::Dotted) => vec![1., 2.],
                LineKind::Under(Underline::Dashed) => vec![4., 3.],
                _ => Vec::new(),
            };
            context.set_dash(&dash, 0.);
            context.move_to(*x, y);
            if *kind == LineKind::Under(Underline::Curly) {
                let mut at = *x;
                let step = (g.cell_w / 2.).max(2.);
                let mut sign = 1.;
                while at < *x + *w {
                    let end = (at + step).min(*x + *w);
                    context.curve_to(
                        at + (end - at) / 3.,
                        y + sign * 2.,
                        at + 2. * (end - at) / 3.,
                        y + sign * 2.,
                        end,
                        y,
                    );
                    at = end;
                    sign = -sign;
                }
            } else {
                context.line_to(*x + *w, y);
            }
            let _ = context.stroke();
            if *kind == LineKind::Under(Underline::Double) {
                context.move_to(*x, y - 2.);
                context.line_to(*x + *w, y - 2.);
                let _ = context.stroke();
            }
            context.set_dash(&[], 0.);
        }
        PaintOp::Glyph { x, y, ops, rgb } => {
            let _ = context.save();
            context.translate(*x, *y);
            context.scale(1. / g.dpr, 1. / g.dpr);
            color(context, *rgb);
            for op in ops {
                match op {
                    DrawOp::FillRect { x, y, w, h } => {
                        context.rectangle(*x, *y, *w, *h);
                        let _ = context.fill();
                    }
                    DrawOp::FillPattern { mask } => {
                        for y in 0..(g.cell_h * g.dpr).ceil() as usize {
                            for x in 0..(g.cell_w * g.dpr).ceil() as usize {
                                let on = mask
                                    .get(y % mask.len().max(1))
                                    .and_then(|row| row.get(x % row.len().max(1)))
                                    .copied()
                                    .unwrap_or(0);
                                if on != 0 {
                                    context.rectangle(x as f64, y as f64, 1., 1.);
                                }
                            }
                        }
                        let _ = context.fill();
                    }
                    DrawOp::ClipCell => {
                        context.rectangle(0., 0., g.cell_w * g.dpr, g.cell_h * g.dpr);
                        context.clip();
                    }
                    DrawOp::BeginPath => context.new_path(),
                    DrawOp::MoveTo { x, y } => context.move_to(*x, *y),
                    DrawOp::LineTo { x, y } => context.line_to(*x, *y),
                    DrawOp::CurveTo {
                        x1,
                        y1,
                        x2,
                        y2,
                        x,
                        y,
                    } => context.curve_to(*x1, *y1, *x2, *y2, *x, *y),
                    DrawOp::Stroke { line_width } => {
                        context.set_line_width(*line_width);
                        let _ = context.stroke();
                    }
                    DrawOp::Fill => {
                        let _ = context.fill();
                    }
                }
            }
            let _ = context.restore();
        }
    }
}

fn link_at(m: &Model, point: Point, row: u16, col: u16) -> Option<String> {
    if let Some(url) = find_urls(m.engine.engine(), point.0)
        .into_iter()
        .find(|url| url.start <= point && point <= url.end)
    {
        return Some(url.url);
    }
    let mut text = String::new();
    let mut rendered = RowRender::default();
    let (cols, rows) = m.engine.size();
    for line in 0..usize::from(rows) {
        m.engine.render_line(line, &mut rendered);
        let mut column = 0;
        for run in &rendered.runs {
            while column < run.col {
                text.push(' ');
                column += 1;
            }
            text.push_str(&run.text);
            column = run.col.saturating_add(run.cells);
        }
        while column < cols {
            text.push(' ');
            column += 1;
        }
        text.push('\n');
    }
    super::links::url_from_wrapped_text(&text, "", Some(usize::from(row)), Some(usize::from(col)))
}

#[cfg(test)]
mod native_paint_tests {
    use super::*;
    #[test]
    fn glyph_device_pixels_convert_to_widget_pixels_once() {
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 24, 24).unwrap();
        let context = cairo::Context::new(&surface).unwrap();
        let layout = pangocairo::functions::create_layout(&context);
        let font = pango::FontDescription::from_string("monospace 13");
        let geom = CellGeom {
            cell_w: 8.,
            cell_h: 10.,
            origin_x: 0.,
            origin_y: 0.,
            dpr: 2.,
            font_size: 13.,
        };
        let op = PaintOp::Glyph {
            x: 0.,
            y: 0.,
            rgb: [255, 0, 0],
            ops: vec![DrawOp::FillRect {
                x: 0.,
                y: 0.,
                w: 16.,
                h: 20.,
            }],
        };
        paint_op(&context, &layout, &font, &geom, &op);
        drop(context);
        drop(layout);
        surface.flush();
        let stride = usize::try_from(surface.stride()).unwrap();
        let data = surface.data().unwrap();
        let alpha = |x: usize, y: usize| data[y * stride + x * 4 + 3];
        assert_eq!(alpha(7, 9), 255);
        assert_eq!(alpha(8, 9), 0);
        assert_eq!(alpha(7, 10), 0);
    }
    #[test]
    fn curly_underline_has_a_different_raster_from_single() {
        let raster = |kind| {
            let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 40, 24).unwrap();
            let context = cairo::Context::new(&surface).unwrap();
            let layout = pangocairo::functions::create_layout(&context);
            let geom = CellGeom {
                cell_w: 8.,
                cell_h: 19.,
                origin_x: 0.,
                origin_y: 0.,
                dpr: 1.,
                font_size: 13.,
            };
            paint_op(
                &context,
                &layout,
                &pango::FontDescription::from_string("monospace 13"),
                &geom,
                &PaintOp::Line {
                    x: 0.,
                    y: 0.,
                    w: 32.,
                    rgb: [255; 3],
                    kind,
                },
            );
            drop(context);
            drop(layout);
            surface.flush();
            surface.data().unwrap().to_vec()
        };
        assert_ne!(
            raster(LineKind::Under(Underline::Single)),
            raster(LineKind::Under(Underline::Curly))
        );
    }
}
