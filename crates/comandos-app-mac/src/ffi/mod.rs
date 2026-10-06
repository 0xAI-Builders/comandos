//! D1 boundary: retained Objective-C objects stay on the AppKit thread.
#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
mod alert;
mod delegate;
mod menu;
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
    model: crate::tabs_ops::TabsOps,
    menus: Option<menu::Menus>,
    alert: Option<alert::AlertHandle>,
    dialogs: crate::dialogs::DialogOwner,
    ipc: Option<crate::ipc::Ipc>,
    ipc_timer: Option<Retained<objc2_foundation::NSTimer>>,
    views: Option<Views>,
    jobs: Option<Jobs>,
    delegate: Option<Retained<Delegate>>,
    mtm: MainThreadMarker,
    closed: bool,
    id: u64,
    dump_started: bool,
    next_local: u64,
}
pub fn run(cfg: AppConfig) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("AppKit requiere el hilo principal")?;
    prepare(&cfg)?;
    let backend = Arc::new(SystemBackend::new(&cfg)?);
    let id = NEXT_OWNER.fetch_add(1, Ordering::Relaxed);
    let native = Rc::new(RefCell::new(Native {
        cfg,
        model: crate::tabs_ops::TabsOps::new(App::new(
            "noche",
            comandos_desktop::ui_lang(None, &std::env::var("LANG").unwrap_or_default()),
        )),
        menus: None,
        alert: None,
        dialogs: Default::default(),
        ipc: None,
        ipc_timer: None,
        views: None,
        jobs: None,
        delegate: None,
        mtm,
        closed: false,
        id,
        dump_started: false,
        next_local: 0,
    }));
    OWNER.with(|slot| *slot.borrow_mut() = Some((id, Rc::downgrade(&native))));
    let delegate = Delegate::new(mtm, Rc::downgrade(&native));
    let jobs = Jobs::new(backend, Arc::new(move || post_drain(id)))?;
    {
        let mut owner = native.borrow_mut();
        owner.jobs = Some(jobs);
        owner.views = Some(Views::new(&owner.cfg, &delegate, owner.model.lang, mtm)?);
        owner.menus = Some(menu::Menus::install(&delegate, owner.model.lang, mtm));
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
        let home = self.home();
        let hooks = match &self.cfg.mode {
            RunMode::Sandbox { hooks, .. } => hooks.clone(),
            RunMode::Live => home.join(".claude/hooks"),
        };
        self.ipc = Some(crate::ipc::Ipc::new(hooks.clone()));
        if let Some(delegate) = &self.delegate {
            // SEGURIDAD: Owned timer targets the retained main-thread delegate; close invalidates it.
            self.ipc_timer = Some(unsafe {
                objc2_foundation::NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(0.5,delegate,objc2::sel!(checkIPC:),None,true)
            });
        }
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
    fn dispatch(&mut self, action: Action) {
        match &action {
            Action::Open { session, .. } | Action::OpenBackground { session, .. } => {
                let key = session.clone();
                let existing = self.model.tabs().iter().any(|tab| tab.key == key);
                if existing && matches!(action, Action::OpenBackground { .. }) {
                    return;
                }
                self.scoped(&key, Task::Open { action, existing });
            }
            Action::NewLocal => {
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
                let Some(next) = self.next_local.checked_add(1) else {
                    return;
                };
                self.next_local = next;
                self.scoped(
                    &format!("\0new-local-{next}"),
                    Task::NewLocal {
                        home: self.home(),
                        active,
                        epoch,
                        pid: std::process::id(),
                    },
                );
            }
        }
    }
    fn home(&self) -> std::path::PathBuf {
        match &self.cfg.mode {
            RunMode::Sandbox { hooks, .. } => {
                hooks.parent().unwrap_or(std::path::Path::new("/")).into()
            }
            RunMode::Live => std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_default(),
        }
    }
    fn scoped(&mut self, key: &str, work: Task) -> bool {
        if self.closed {
            return false;
        }
        let operation = self.model.start_operation(key);
        let result = self
            .jobs
            .as_ref()
            .ok_or_else(|| "worker unavailable".to_string())
            .and_then(|jobs| {
                jobs.submit(
                    self.model.ticket(),
                    Task::Scoped {
                        key: key.into(),
                        operation: operation.clone(),
                        work: Box::new(work),
                    },
                )
            });
        if let Err(error) = result {
            self.model.cancel_operation(key);
            eprintln!("ComandOS: {error}");
            false
        } else {
            true
        }
    }
    fn save(&mut self) {
        let value = self.model.save_value();
        self.scoped("\0save", Task::Save { value });
    }
    pub(super) fn receive(&mut self, body: serde_json::Value) {
        let Some(message) = comandos_desktop::parse_bridge(&body) else {
            return;
        };
        let rename = matches!(message, comandos_desktop::BridgeMessage::Rename { .. });
        match self.model.receive_bridge(message) {
            Ok(actions) => {
                for action in actions {
                    self.dispatch(action);
                }
                if rename {
                    self.save();
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
            self.result(&delivery.ticket, delivery.result);
        }
        self.sync();
    }
    fn result(&mut self, ticket: &crate::app::Ticket, result: Result<ResultData, String>) {
        if !self.model.owns_ticket(ticket) {
            return;
        }
        match result {
            Ok(ResultData::Scoped {
                key,
                operation,
                result,
            }) => {
                if operation.ticket_for(ticket).is_none() {
                    return;
                }
                self.model.finish_operation(&key, &operation);
                if key == "\0restore" && result.is_err() {
                    self.fail_restore(ticket);
                }
                self.result(ticket, *result);
            }
            Ok(ResultData::Preferences { theme, lang }) => {
                self.model.lang = lang;
                self.model.initial_theme(&theme);
            }
            Ok(ResultData::Boot {
                token,
                hub,
                saved,
                metadata,
            }) => {
                self.model.begin_restore(saved.clone());
                self.model.finish_boot(ticket, &token);
                self.add_opened(hub);
                if !self.scoped(
                    "\0restore",
                    Task::Restore {
                        saved,
                        metadata,
                        home: self.home(),
                        state: self.model.restore.clone(),
                    },
                ) {
                    self.fail_restore(ticket);
                }
            }
            Ok(ResultData::Restored(tabs)) => {
                for tab in tabs {
                    if !self.model.restore_cancelled(&tab.original)
                        && !self.model.restore_cancelled(&tab.opened.session)
                        && !self
                            .model
                            .tabs()
                            .iter()
                            .any(|t| t.key == tab.opened.session)
                    {
                        self.add_opened(tab.opened);
                    }
                }
                self.finish_restore(ticket);
            }
            Ok(ResultData::Opened(opened)) => {
                let new = !self
                    .model
                    .tabs()
                    .iter()
                    .any(|t| t.session == opened.session);
                self.add_opened(opened);
                if new {
                    self.save();
                }
            }
            Ok(ResultData::States(value)) => self.model.update_states(&value),
            Ok(
                ResultData::Dumped | ResultData::Saved | ResultData::Closed | ResultData::Renamed,
            ) => {}
            Err(error) => eprintln!("ComandOS: {error}"),
        }
    }
    fn fail_restore(&mut self, ticket: &crate::app::Ticket) {
        let pending = self.model.fail_restore(ticket);
        self.save();
        for action in pending {
            self.dispatch(action);
        }
    }
    fn finish_restore(&mut self, ticket: &crate::app::Ticket) {
        let pending = self.model.finish_restore(ticket);
        self.save();
        for action in pending {
            self.dispatch(action);
        }
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
        if opened.metadata.is_some() {
            self.model.set_metadata(key, opened.metadata);
        }
        if opened.select {
            self.model.select_tab(key);
        }
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
        if let (Some(menus), Some(delegate)) = (&mut self.menus, &self.delegate) {
            menus.refresh(delegate, self.model.lang, self.mtm);
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
        if let Some(timer) = self.ipc_timer.take() {
            timer.invalidate();
        }
        self.ipc.take();
        self.dialogs.shutdown();
        if let (Some(alert), Some(views)) = (self.alert.take(), &self.views) {
            alert.cancel(&views.window, self.mtm);
        }
        if let Some(menus) = &mut self.menus {
            menus.detach(self.mtm);
        }
        self.menus.take();
        if let Some(jobs) = &mut self.jobs {
            jobs.close();
        }
        if let Some(views) = &mut self.views {
            views.close();
        }
        self.jobs.take();
    }
}

impl Native {
    pub(super) fn menu_action(&mut self, action: crate::strip::MenuAction) {
        use crate::strip::MenuAction;
        match action {
            MenuAction::Quit => {} // Main menu's Quit targets NSApplication directly.
            MenuAction::Reload => {
                if let Some(views) = &self.views {
                    views.reload();
                }
            }
            MenuAction::Toggle => {
                self.model.term_visible = !self.model.term_visible;
                if let Some(views) = &self.views {
                    views.toggle(self.model.term_visible);
                }
            }
            MenuAction::ZoomIn | MenuAction::ZoomOut | MenuAction::ZoomReset => {
                if let Some(views) = &self.views {
                    views.zoom(
                        self.model
                            .active_key()
                            .and_then(|key| self.model.tabs().iter().find(|t| t.key == key))
                            .map(|t| t.instance),
                        match action {
                            MenuAction::ZoomIn => 1.1,
                            MenuAction::ZoomOut => 1. / 1.1,
                            _ => 0.,
                        },
                    );
                }
            }
            MenuAction::NewLocal => self.action(Action::NewLocal),
            MenuAction::Close => {
                if let Some(tab) = self
                    .model
                    .tabs()
                    .iter()
                    .find(|t| Some(t.key.as_str()) == self.model.active_key())
                {
                    self.prompt(
                        crate::dialogs::TabScope {
                            key: tab.key.clone(),
                            instance: tab.instance,
                        },
                        false,
                    );
                }
            }
            MenuAction::Next | MenuAction::Previous => {
                self.model
                    .cycle_tab(if action == MenuAction::Next { 1 } else { -1 });
                self.sync();
            }
        }
    }
    pub(super) fn prompt(&mut self, scope: crate::dialogs::TabScope, rename: bool) {
        if self.closed || self.alert.is_some() {
            return;
        }
        let Some(tab) =
            self.model.tabs().iter().find(|t| {
                t.key == scope.key && t.instance == scope.instance && (!t.is_hub || rename)
            })
        else {
            return;
        };
        let Some(views) = &self.views else {
            return;
        };
        let Some(dialog_id) = self.dialogs.begin(scope.clone()) else {
            return;
        };
        let id = self.id;
        let ticket = self.model.ticket();
        let complete = move |outcome: crate::dialogs::DialogOutcome| {
            let ticket = ticket.clone();
            let block = RcBlock::new(move || {
                if MainThreadMarker::new().is_none() || !ticket.current() {
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
                    && owner.model.owns_ticket(&ticket)
                {
                    owner.dialog(dialog_id, outcome.clone());
                }
            });
            // SEGURIDAD: AppKit completion copies a Send-data block onto main. It
            // resolves only this weak owner after any sheet-ending borrow has ended.
            unsafe {
                NSOperationQueue::mainQueue().addOperationWithBlock(&block);
            }
        };
        self.alert = Some(if rename {
            alert::prompt_rename(
                &views.window,
                scope,
                &tab.label,
                self.model.lang,
                self.mtm,
                complete,
            )
        } else {
            alert::prompt_close(
                &views.window,
                scope,
                &tab.label,
                self.model.lang,
                self.mtm,
                complete,
            )
        });
    }
    fn dialog(&mut self, id: crate::dialogs::DialogId, outcome: crate::dialogs::DialogOutcome) {
        if !self.dialogs.finish(id) {
            return;
        }
        self.alert.take();
        match outcome {
            crate::dialogs::DialogOutcome::Close {
                scope,
                confirmed: true,
            } => self.close_tab(&scope.key, scope.instance),
            crate::dialogs::DialogOutcome::Rename {
                scope,
                label: Some(label),
            } if self.model.rename(&scope.key, scope.instance, &label) => {
                self.scoped(
                    &scope.key,
                    Task::Rename {
                        session: scope.key.clone(),
                        label,
                    },
                );
                self.save();
                self.sync();
            }
            _ => {}
        }
    }
    fn close_tab(&mut self, key: &str, instance: u64) {
        if let Some(tab) = self.model.close_model(key, instance) {
            if self.dialogs.cancel(&crate::dialogs::TabScope {
                key: key.into(),
                instance,
            }) && let (Some(alert), Some(views)) = (self.alert.take(), &self.views)
            {
                alert.cancel(&views.window, self.mtm);
            }
            self.scoped(key, Task::ArchiveClose { tab });
            self.save();
            self.sync();
        }
    }
    pub(super) fn check_ipc(&mut self) {
        if self.closed {
            return;
        }
        let events = self
            .ipc
            .as_mut()
            .map(crate::ipc::Ipc::poll)
            .unwrap_or_default();
        for (kind, body) in events {
            let Some(session) = body
                .get("session")
                .and_then(serde_json::Value::as_str)
                .filter(|s| comandos_desktop::validation::valid_session(s))
            else {
                continue;
            };
            let label = body
                .get("label")
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            match kind {
                crate::ipc::Kind::Focus => {
                    if ["local", "hub", "control"].contains(&session) {
                        continue;
                    }
                    let win = body
                        .get("win")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("claude");
                    if !comandos_desktop::validation::valid_window(win) {
                        continue;
                    }
                    self.action(Action::Open {
                        session: session.into(),
                        win: win.into(),
                        label,
                    });
                    if let Some(views) = &self.views {
                        views.show(self.mtm);
                    }
                }
                crate::ipc::Kind::Open => {
                    if session != "__hub__" {
                        self.action(Action::OpenBackground {
                            session: session.into(),
                            label,
                        });
                    }
                }
                crate::ipc::Kind::Close => {
                    let queued = self.model.cancel_pending(session);
                    let restoring = self.model.cancel_restore(session);
                    let current = self.model.current_session(session);
                    self.model.cancel_operation(session);
                    if current != session {
                        self.model.cancel_operation(&current);
                    }
                    let scope = self
                        .model
                        .tabs()
                        .iter()
                        .find(|t| t.key == session || t.key == current)
                        .map(|t| (t.key.clone(), t.instance));
                    if let Some((key, instance)) = scope {
                        self.close_tab(&key, instance);
                    } else if restoring || queued {
                        if restoring {
                            self.save();
                        }
                        self.scoped(
                            session,
                            Task::RemoveMetadata {
                                session: session.into(),
                            },
                        );
                    }
                }
            }
        }
    }
}
