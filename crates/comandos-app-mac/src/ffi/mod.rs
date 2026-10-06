//! D1 boundary: retained Objective-C objects stay on the AppKit thread.
#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
mod delegate;
mod webview;
mod window;
use crate::{
    app::{Action, App, AppConfig, RunMode},
    jobs::{Jobs, ResultData, SystemBackend, Task},
};
use block2::RcBlock;
use delegate::Delegate;
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::NSApplication;
use objc2_foundation::{MainThreadMarker, NSOperationQueue};
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use window::Views;
thread_local! {static OWNER:RefCell<Option<(u64,Weak<RefCell<Native>>)>>=const {RefCell::new(None)};}
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
pub(super) struct Native {
    cfg: AppConfig,
    model: App,
    views: Option<Views>,
    jobs: Option<Jobs>,
    delegate: Option<Retained<Delegate>>,
    mtm: MainThreadMarker,
    closed: bool,
    id: u64,
    dump_started: bool,
}
pub fn run(cfg: AppConfig) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("AppKit requiere el hilo principal")?;
    prepare(&cfg)?;
    let backend = Arc::new(SystemBackend::new(&cfg)?);
    let id = NEXT_OWNER.fetch_add(1, Ordering::Relaxed);
    let native = Rc::new(RefCell::new(Native {
        cfg,
        model: App::new(
            "noche",
            comandos_desktop::ui_lang(None, &std::env::var("LANG").unwrap_or_default()),
        ),
        views: None,
        jobs: None,
        delegate: None,
        mtm,
        closed: false,
        id,
        dump_started: false,
    }));
    OWNER.with(|slot| *slot.borrow_mut() = Some((id, Rc::downgrade(&native))));
    let delegate = Delegate::new(mtm, Rc::downgrade(&native));
    let jobs = Jobs::new(backend, Arc::new(move || post_drain(id)))?;
    {
        let mut owner = native.borrow_mut();
        owner.jobs = Some(jobs);
        owner.views = Some(Views::new(&owner.cfg, &delegate, owner.model.lang, mtm)?);
        owner.delegate = Some(delegate.clone());
    }
    let app = NSApplication::sharedApplication(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    if let Ok(mut owner) = native.try_borrow_mut() {
        owner.close();
    }
    app.setDelegate(None);
    OWNER.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}
fn prepare(cfg: &AppConfig) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    if let RunMode::Sandbox { tmux_socket, hooks } = &cfg.mode {
        let root = hooks.parent().ok_or("sandbox sin directorio")?;
        let metadata = std::fs::symlink_metadata(root).map_err(|e| format!("sandbox: {e}"))?;
        if !metadata.is_dir()
            || metadata.uid() != nix::unistd::geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
        {
            return Err("sandbox requiere directorio propio 0700 sin symlink".into());
        }
        for dir in [
            hooks.as_path(),
            tmux_socket.parent().ok_or("socket sin directorio")?,
        ] {
            if !dir.exists() {
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .create(dir)
                    .map_err(|e| e.to_string())?;
            }
            let m = std::fs::symlink_metadata(dir).map_err(|e| e.to_string())?;
            if !m.is_dir() || m.uid() != metadata.uid() || m.mode() & 0o077 != 0 {
                return Err("directorio sandbox inseguro".into());
            }
        }
    }
    Ok(())
}
fn post_drain(id: u64) {
    let block = RcBlock::new(move || {
        if MainThreadMarker::new().is_none() {
            return;
        }
        let owner = OWNER.with(|slot| {
            slot.borrow()
                .as_ref()
                .filter(|(current, _)| *current == id)
                .and_then(|(_, weak)| weak.upgrade())
        });
        if let Some(owner) = owner
            && let Ok(mut owner) = owner.try_borrow_mut()
        {
            owner.drain();
        }
    });
    // SEGURIDAD: Foundation copies the block; it captures only a Send integer.
    // The queue invokes it on the main thread, where the weak owner is resolved.
    unsafe {
        NSOperationQueue::mainQueue().addOperationWithBlock(&block);
    }
}
impl Native {
    pub(super) fn dump_dashboard(&mut self) {
        if self.closed || self.dump_started {
            return;
        }
        let Some(path) = self.cfg.dump_dom.clone() else {
            return;
        };
        let Some(views) = &self.views else {
            return;
        };
        // SEGURIDAD: The retained dashboard is queried only on the owning main thread.
        let at_origin = unsafe {
            views
                .dash
                .URL()
                .and_then(|url| url.absoluteString())
                .is_some_and(|url| {
                    let text = url.to_string();
                    text == self.cfg.dash_url
                        || text
                            .strip_prefix(&self.cfg.dash_url)
                            .is_some_and(|s| s.starts_with('/'))
                })
        };
        if !at_origin {
            return;
        }
        self.dump_started = true;
        let id = self.id;
        let ticket = self.model.ticket();
        let callback = RcBlock::new(
            move |result: *mut objc2::runtime::AnyObject, error: *mut objc2_foundation::NSError| {
                if MainThreadMarker::new().is_none() || !ticket.current() || !error.is_null() {
                    return;
                }
                // SEGURIDAD: evaluateJavaScript supplies a valid optional Objective-C result
                // during this callback. No pointer escapes it; NSString contents are copied.
                let Some(object) = (unsafe { result.as_ref() }) else {
                    return;
                };
                let Some(text) = object.downcast_ref::<objc2_foundation::NSString>() else {
                    return;
                };
                if text.len() > 8 << 20 {
                    return;
                }
                let html = text.to_string();
                let owner = OWNER.with(|slot| {
                    slot.borrow()
                        .as_ref()
                        .filter(|(current, _)| *current == id)
                        .and_then(|(_, owner)| owner.upgrade())
                });
                if let Some(owner) = owner
                    && let Ok(owner) = owner.try_borrow()
                    && !owner.closed
                    && ticket.current()
                {
                    owner.submit(Task::Dump {
                        path: path.clone(),
                        html,
                    });
                }
            },
        );
        // SEGURIDAD: WebKit copies the completion block, which captures only Send data,
        // and returns the DOM asynchronously. The fixed expression includes no input.
        unsafe {
            views.dash.evaluateJavaScript_completionHandler(
                &objc2_foundation::NSString::from_str("document.documentElement.outerHTML"),
                Some(&callback),
            );
        }
    }
    pub(super) fn launch(&mut self) {
        if self.closed {
            return;
        }
        if let Some(views) = &self.views {
            views.show(self.mtm);
        }
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        let hooks = match &self.cfg.mode {
            RunMode::Sandbox { hooks, .. } => hooks.clone(),
            RunMode::Live => home.join(".claude/hooks"),
        };
        self.submit(Task::Preferences {
            env_lang: std::env::var("LANG").unwrap_or_default(),
        });
        self.submit(Task::Boot { hooks, home });
    }
    fn submit(&self, task: Task) {
        if self.closed {
            return;
        }
        if let Some(jobs) = &self.jobs
            && let Err(error) = jobs.submit(self.model.ticket(), task)
        {
            eprintln!("ComandOS: {error}");
        }
    }
    pub(super) fn action(&mut self, action: Action) {
        if self.closed {
            return;
        }
        let pending = match self.model.queue(action) {
            Ok(actions) => actions,
            Err(error) => {
                eprintln!("ComandOS: {error}");
                return;
            }
        };
        for action in pending {
            self.dispatch(action);
        }
    }
    fn dispatch(&self, action: Action) {
        match action {
            Action::Open { ref session, .. } => {
                let existing = self.model.tabs().iter().any(|tab| tab.key == *session);
                self.submit(Task::Open { action, existing });
            }
            Action::NewLocal => {
                let home = std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default();
                let active = self
                    .model
                    .tabs()
                    .iter()
                    .find(|tab| Some(tab.key.as_str()) == self.model.active_key() && !tab.is_hub)
                    .map(|tab| tab.key.clone());
                let epoch = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                self.submit(Task::NewLocal {
                    home,
                    active,
                    epoch,
                    pid: std::process::id(),
                });
            }
        }
    }
    pub(super) fn receive(&mut self, body: serde_json::Value) {
        let Some(message) = comandos_desktop::parse_bridge(&body) else {
            return;
        };
        match self.model.receive_bridge(message) {
            Ok(actions) => {
                for action in actions {
                    self.dispatch(action);
                }
                self.sync();
            }
            Err(error) => eprintln!("mensaje invalido del tablero: {error}"),
        }
    }
    fn drain(&mut self) {
        if self.closed {
            return;
        }
        let deliveries = self.jobs.as_ref().map(Jobs::drain).unwrap_or_default();
        for delivery in deliveries {
            if !delivery.ticket.current() {
                continue;
            }
            match delivery.result {
                Ok(ResultData::Preferences { theme, lang }) => {
                    self.model.lang = lang;
                    self.model.initial_theme(&theme);
                }
                Ok(ResultData::Boot { token, hub }) => {
                    let actions = self.model.finish_boot(&delivery.ticket, &token);
                    self.add_opened(hub);
                    for action in actions {
                        self.dispatch(action);
                    }
                }
                Ok(ResultData::Opened(opened)) => self.add_opened(opened),
                Ok(ResultData::Dumped) => {}
                Ok(ResultData::States(value)) => self.model.update_states(&value),
                Err(error) => eprintln!("ComandOS: {error}"),
            }
        }
        self.sync();
    }
    fn add_opened(&mut self, opened: crate::jobs::Opened) {
        if opened.raise
            && let Some(views) = &self.views
        {
            views.show(self.mtm);
        }
        let key = if opened.is_hub {
            "__hub__"
        } else {
            &opened.session
        };
        if let Err(error) = self
            .model
            .add_tab(key, &opened.label, &opened.session, opened.is_hub)
        {
            eprintln!("ComandOS: {error}");
            return;
        }
        self.model.select_tab(key);
    }
    pub(super) fn select(&mut self, instance: u64) {
        let key = self
            .model
            .tabs()
            .iter()
            .find(|t| t.instance == instance)
            .map(|t| t.key.clone());
        if let Some(key) = key {
            self.model.select_tab(&key);
            self.sync();
        }
    }
    fn sync(&mut self) {
        if self.closed {
            return;
        }
        let focus = self.model.take_focus_request();
        if let (Some(views), Some(delegate)) = (&mut self.views, &self.delegate)
            && let Err(error) = views.sync(&self.model, focus, &self.cfg, delegate, self.mtm)
        {
            eprintln!("ComandOS: {error}");
        }
    }
    pub(super) fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.model.shutdown();
        if let Some(jobs) = &mut self.jobs {
            jobs.close();
        }
        if let Some(views) = &mut self.views {
            views.close();
        }
        self.jobs.take();
    }
}
