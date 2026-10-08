//! MainContext clipboard port. Each terminal owns its requests and selection state.
use crate::config::RunMode;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Clipboard,
    Primary,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardError {
    Shadow,
    Cancelled,
    DestinationChanged,
    Tmux(String),
}
impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ClipboardError {}
pub trait TmuxIo {
    fn mode(&self) -> RunMode;
    fn socket(&self) -> &std::path::Path;
    fn read(&self, args: &[&str]) -> Result<crate::tmux::TmuxOut, crate::tmux::TmuxError>;
    fn mutate(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> Result<crate::tmux::TmuxOut, crate::tmux::TmuxError>;
}
impl TmuxIo for crate::tmux::TmuxCtl {
    fn mode(&self) -> RunMode {
        self.mode()
    }
    fn socket(&self) -> &std::path::Path {
        self.socket_path()
    }
    fn read(&self, args: &[&str]) -> Result<crate::tmux::TmuxOut, crate::tmux::TmuxError> {
        self.read(args)
    }
    fn mutate(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> Result<crate::tmux::TmuxOut, crate::tmux::TmuxError> {
        match stdin {
            Some(s) => self.mutate_with_stdin(args, s),
            None => self.mutate(args),
        }
    }
}
pub trait ClipboardPort {
    fn set_text(&self, target: Target, text: &str) -> bool;
    fn request_text(&self, target: Target, done: Box<dyn FnOnce(Option<String>)>);
}
pub struct GtkClipboard;
impl ClipboardPort for GtkClipboard {
    fn set_text(&self, target: Target, text: &str) -> bool {
        if !gtk::is_initialized_main_thread() {
            return false;
        }
        let clipboard = gtk::Clipboard::get(&atom(target));
        clipboard.set_text(text);
        clipboard.store();
        true
    }
    fn request_text(&self, target: Target, done: Box<dyn FnOnce(Option<String>)>) {
        if !gtk::is_initialized_main_thread() {
            done(None);
            return;
        }
        gtk::Clipboard::get(&atom(target))
            .request_text(move |_, text| done(text.map(str::to_string)));
    }
}
fn atom(target: Target) -> gdk::Atom {
    match target {
        Target::Clipboard => gdk::SELECTION_CLIPBOARD,
        Target::Primary => gdk::SELECTION_PRIMARY,
    }
}
pub struct Clipboard {
    mode: RunMode,
    port: Rc<dyn ClipboardPort>,
    cancelled: Arc<AtomicBool>,
    valid: RefCell<Rc<dyn Fn() -> bool>>,
    epoch: Cell<u64>,
    last_copy: Cell<Option<f64>>,
    released: Cell<bool>,
}
impl Clipboard {
    pub fn new(mode: RunMode, port: Rc<dyn ClipboardPort>, cancelled: Arc<AtomicBool>) -> Rc<Self> {
        Rc::new(Self {
            mode,
            port,
            cancelled,
            valid: RefCell::new(Rc::new(|| true)),
            epoch: Cell::new(0),
            last_copy: Cell::new(None),
            released: Cell::new(false),
        })
    }
    pub fn set_valid(&self, valid: Rc<dyn Fn() -> bool>) {
        *self.valid.borrow_mut() = valid;
        self.invalidate();
    }
    pub fn permitted(&self) -> bool {
        let valid = self.valid.borrow().clone();
        self.mode != RunMode::Shadow && !self.cancelled.load(Ordering::Acquire) && valid()
    }
    pub fn invalidate(&self) {
        self.epoch.set(self.epoch.get().wrapping_add(1));
    }
    pub fn copy(&self, target: Target, text: &str) -> bool {
        self.permitted() && !text.is_empty() && self.port.set_text(target, text)
    }
    pub fn copy_selection(&self, text: &str, now: f64) -> bool {
        if self.copy(Target::Clipboard, text) {
            self.last_copy.set(Some(now));
            true
        } else {
            false
        }
    }
    pub fn recent_selection(&self, now: f64) -> bool {
        self.permitted()
            && self
                .last_copy
                .get()
                .is_some_and(|t| now >= t && now - t < 15.)
    }
    pub fn release_selection(&self) {
        self.released.set(true);
    }
    pub fn take_selection_release(&self) -> bool {
        self.released.replace(false)
    }
    pub fn osc52(&self, target: Target, text: &str) {
        if self.permitted() && text.len() <= comandos_term::engine::MAX_CLIPBOARD_BYTES {
            self.port.set_text(target, text);
        }
    }
    pub fn request(self: &Rc<Self>, target: Target, done: Box<dyn FnOnce(String)>) {
        if !self.permitted() {
            return;
        }
        self.invalidate();
        let epoch = self.epoch.get();
        let weak = Rc::downgrade(self);
        self.port.request_text(
            target,
            Box::new(move |text| {
                if let Some(owner) = weak
                    .upgrade()
                    .filter(|o| o.permitted() && o.epoch.get() == epoch)
                    && let Some(text) = text
                        .filter(|s| !s.is_empty() && s.len() <= crate::clipboard_bridge::MAX_BYTES)
                {
                    drop(owner);
                    done(text);
                }
            }),
        );
    }
}
pub struct Replay<T> {
    pending: bool,
    keys: std::collections::VecDeque<T>,
}
impl<T> Default for Replay<T> {
    fn default() -> Self {
        Self {
            pending: false,
            keys: std::collections::VecDeque::new(),
        }
    }
}
impl<T> Replay<T> {
    pub fn pending(&self) -> bool {
        self.pending
    }
    pub fn begin(&mut self, key: T) -> Result<(), ClipboardError> {
        self.pending = true;
        self.push(key)
    }
    pub fn push(&mut self, key: T) -> Result<(), ClipboardError> {
        if self.keys.len() >= 64 {
            return Err(ClipboardError::Tmux("Cola de teclado llena".into()));
        }
        self.keys.push_back(key);
        Ok(())
    }
    pub fn finish(&mut self, copy_context: bool) -> Vec<(T, bool)> {
        self.pending = false;
        self.keys
            .drain(..)
            .enumerate()
            .map(|(i, k)| (k, i == 0 && copy_context))
            .collect()
    }
    pub fn cancel(&mut self) {
        self.pending = false;
        self.keys.clear();
    }
}
