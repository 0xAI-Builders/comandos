//! Captura explícita sobre el estado y los widgets realmente poseídos por App.
use super::App;
use crate::layout_dump::{self, Decision, Diagnostic};
use gtk::prelude::*;
use serde_json::{Value, json};
use std::{
    rc::Rc,
    sync::atomic::Ordering,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use webkit2gtk::WebViewExt;

impl App {
    fn diagnostic_issues(&self) -> Vec<String> {
        let mut issues = Vec::new();
        for (blocked, reason) in [
            (!self.restore.borrow().ready(), "restore_running"),
            (!self.startup_valid.get(), "startup_invalid"),
            (
                self.focus_restoring.get() && !self.focus_ready.get(),
                "focus_restore_pending",
            ),
            (!self.window.is_mapped(), "window_unmapped"),
            (self.webview.is_loading(), "dashboard_loading"),
            (
                self.cfg.dash_url().is_some() && !self.prefs_received.get(),
                "preferences_unavailable",
            ),
            (
                self.cfg.dash_url().is_some() && !self.state_received.get(),
                "session_state_unavailable",
            ),
            (
                self.cfg.dash_url().is_some() && !self.marks_received.get(),
                "marks_unavailable",
            ),
            (
                self.cfg.dash_url().is_some() && self.workspace_doc.borrow().is_null(),
                "workspace_unavailable",
            ),
            (self.posting.get(), "workspace_post_pending"),
            (
                self.pending_workspace.borrow().is_some(),
                "workspace_apply_pending",
            ),
            (self.dragging.get(), "drag_active"),
            (
                self.workspace.resize_pending() || self.resize_queue.borrow().pending(),
                "resize_pending",
            ),
            (self.quick_busy.get(), "quick_terminal_pending"),
        ] {
            if blocked {
                issues.push(reason.into());
            }
        }
        for (key, term) in self.terms.borrow().iter() {
            if !term.diagnostic_ready() {
                issues.push(format!("terminal_not_ready:{key}"));
            }
        }
        for record in self.registry.borrow().records() {
            if record.kind == crate::tabs::TabKind::Web
                && let Some(leaf) = self.workspace.leaf(&record.key)
                && contains_webview(&leaf)
            {
                issues.push(format!("web_terminal_content_unavailable:{}", record.key));
            }
        }
        if let Some(observation) = self.dashboard_observation.borrow().as_ref() {
            let observation = observation.borrow();
            if !observation.ready() {
                issues.push(observation.error.as_ref().map_or_else(
                    || "dashboard_not_loaded".into(),
                    |error| format!("dashboard_load_failed:{error}"),
                ));
            }
        }
        issues
    }
    fn semantic_layout(&self) -> Result<Value, String> {
        let window: &gtk::Window = self.window.upcast_ref();
        let selected = if self.workspace.widget().is_visible() {
            self.workspace.focused()
        } else {
            self.strip.borrow().selected_key()
        };
        let theme = self.applied_theme.borrow();
        let theme = theme.as_ref().ok_or("applied theme unavailable")?;
        let font = layout_dump::measured_font(
            window.upcast_ref(),
            &window.style_context().font(gtk::StateFlags::NORMAL),
        )?;
        let mut widgets = Vec::new();
        for (id, role, widget) in [
            ("window", "window", window.upcast_ref::<gtk::Widget>()),
            ("main-pane", "main-pane", self.paned.upcast_ref()),
            ("dashboard", "dashboard", self.webview.upcast_ref()),
            (
                "tabstrip",
                "tabstrip",
                self.tab_layout.widget().upcast_ref(),
            ),
            (
                "workspace",
                "workspace",
                self.workspace.widget().upcast_ref(),
            ),
            ("fallback-notebook", "notebook", self.notebook.upcast_ref()),
            ("toolbar", "toolbar", self.toolbar.upcast_ref()),
            ("status", "status", self.status.upcast_ref()),
            (
                "drag-overlay",
                "drag-overlay",
                self.drag_layer.widget().upcast_ref(),
            ),
        ] {
            widgets.push(layout_dump::owned_widget(window, id, role, widget, None));
        }
        if let Some(content) = window.child() {
            widgets.push(layout_dump::owned_widget(
                window,
                "content-root",
                "content-root",
                &content,
                None,
            ));
            if let Some(header) = content
                .downcast_ref::<gtk::Container>()
                .and_then(|content| content.children().first().cloned())
            {
                widgets.push(layout_dump::owned_widget(
                    window, "header", "header", &header, None,
                ));
                observed_children(window, "header", &header, &mut widgets);
            }
        }
        observed_children(window, "toolbar", self.toolbar.upcast_ref(), &mut widgets);
        let strip_entries = self.tab_layout.entries();
        let strip_selected = strip_entries
            .iter()
            .find(|(_, widget)| widget.style_context().has_class("cur"))
            .map(|(key, _)| key.clone());
        for (key, widget) in &strip_entries {
            let label = self
                .labels
                .borrow()
                .get(key)
                .map(|tab| tab.text.text().to_string())
                .or_else(|| {
                    self.group_labels
                        .borrow()
                        .get(key)
                        .map(|tab| tab.text.text().to_string())
                });
            widgets.push(layout_dump::owned_widget(
                window,
                &format!("strip:{key}"),
                "strip-tab",
                widget,
                label.as_deref(),
            ));
            observed_children(window, &format!("strip:{key}"), widget, &mut widgets);
        }
        let registry = self.registry.borrow();
        let mut tabs = Vec::new();
        let mut terminals = Vec::new();
        for key in registry.ordered_keys() {
            let record = registry
                .records()
                .find(|record| record.key == key)
                .ok_or("registry key unavailable")?;
            let labels = self.labels.borrow();
            let tab = labels.get(&key).ok_or("tab label unavailable")?;
            let label = tab.text.text().to_string();
            tabs.push(json!({"key":key,"label":label,"kind":match record.kind {crate::tabs::TabKind::Local=>"local",crate::tabs::TabKind::Session=>"session",crate::tabs::TabKind::Web=>"web"},"favorite":record.favorite,"selected":selected.as_deref()==Some(&key),"state":tab.diagnostic_state(),"attached":self.terms.borrow().contains_key(&key)}));
            widgets.push(layout_dump::owned_widget(
                window,
                &format!("tab-text:{key}"),
                "tab-text",
                tab.text.upcast_ref(),
                Some(&label),
            ));
            if let Some(leaf) = self.workspace.leaf(&key) {
                widgets.push(layout_dump::owned_widget(
                    window,
                    &format!("leaf:{key}"),
                    "leaf",
                    &leaf,
                    None,
                ));
                observed_children(window, &format!("leaf:{key}"), &leaf, &mut widgets);
            }
            if let Some(term) = self.terms.borrow().get(&key) {
                let mut snapshot = term.diagnostic_snapshot()?;
                if let Some(object) = snapshot.as_object_mut() {
                    object.insert("key".into(), json!(key));
                }
                terminals.push(snapshot);
                widgets.push(layout_dump::owned_widget(
                    window,
                    &format!("terminal:{key}"),
                    "terminal",
                    term.widget().upcast_ref(),
                    None,
                ));
            }
        }
        for (id, identity, widget) in self.workspace.diagnostic_splits() {
            let mut measured = layout_dump::owned_widget(window, &id, "split", &widget, None);
            if let Some(object) = measured.as_object_mut() {
                object.insert("split".into(), identity);
            }
            widgets.push(measured);
        }
        let doc = self.workspace_doc.borrow();
        let active_group = self
            .workspace
            .widget()
            .current_page()
            .and_then(|page| self.workspace.widget().nth_page(Some(page)))
            .and_then(|page| self.workspace.group_of_page(&page));
        layout_dump::normalize(
            json!({"viewport":{"width":window.allocated_width(),"height":window.allocated_height(),"dpr":window.scale_factor()},"window":{"title":window.title().map(|v|v.to_string())},"dashboard":{"uri":self.webview.uri().map(|v|v.to_string()),"title":self.webview.title().map(|v|v.to_string()),"loading":self.webview.is_loading()},"theme":{"name":theme.values.get("name"),"tokens":theme.values,"ansi":theme.ansi,"button_style":*self.applied_button_style.borrow()},"font":font,"tabs":tabs,"strip":{"order":strip_entries.iter().map(|(key,_)|key).collect::<Vec<_>>(),"selected":strip_selected,"layout":if self.tab_layout.rows(){"rows"}else{"single"}},"workspace":{"available":!doc.is_null(),"revision":(!doc.is_null()).then_some(self.revision.get()),"groups":doc.get("groups").cloned().unwrap_or_else(||json!([])),"focused_tab":self.workspace.focused(),"selected_tab":selected,"active_group":active_group},"widgets":widgets,"terminals":terminals}),
        )
    }
    pub(super) fn install_layout_diagnostic(self: &Rc<Self>) -> Option<glib::SourceId> {
        if !layout_dump::enabled(std::env::var("COMANDOS_APP_LAYOUT_DUMP").ok().as_deref()) {
            return None;
        }
        let path = self.cfg.layout_dump_path();
        if let Err(error) = self.guard.remove_if_exists(&path) {
            self.status
                .set_text(&format!("Layout diagnostic: {error:?}"));
            eprintln!("layout diagnostic failed removing stale capture: {error:?}");
            return None;
        }
        let timeout = std::env::var("COMANDOS_APP_LAYOUT_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| (100..=60_000).contains(value))
            .unwrap_or(layout_dump::DEFAULT_TIMEOUT_MS);
        let epoch = Instant::now();
        let mut diagnostic = Diagnostic::new(0, timeout);
        let weak = Rc::downgrade(self);
        Some(glib::timeout_add_local(
            Duration::from_millis(100),
            move || {
                let Some(app) = weak
                    .upgrade()
                    .filter(|app| !app.closed.load(Ordering::Acquire))
                else {
                    return glib::ControlFlow::Break;
                };
                if diagnostic.done() {
                    return glib::ControlFlow::Continue;
                }
                let mut issues = app.diagnostic_issues();
                let snapshot = if issues.is_empty() {
                    match app.semantic_layout() {
                        Ok(value) => Some(value),
                        Err(error) => {
                            issues.push(error);
                            None
                        }
                    }
                } else {
                    None
                };
                let now = u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
                let decision = diagnostic.observe(now, &issues, snapshot.as_ref());
                let mut value = match decision {
                    Decision::Pending => return glib::ControlFlow::Continue,
                    Decision::Ready(value) => value,
                    Decision::Failed(issues) => {
                        json!({"schema":layout_dump::SCHEMA,"readiness":{"status":"failed","issues":issues}})
                    }
                };
                if let Some(object) = value.as_object_mut() {
                    object.insert("capture".into(), json!({"captured_at_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).ok().and_then(|time|u64::try_from(time.as_millis()).ok()),"process_pid":std::process::id(),"window_handle":app.window.window().map(|window|format!("{:p}",window.as_ptr()))}));
                }
                // Diagnostic-only local atomic IO: no queued worker can indefinitely
                // postpone publication behind a polling/process job.
                if let Err(error) = layout_dump::publish(&app.guard, &path, &value) {
                    eprintln!("layout diagnostic write failed: {error}");
                }
                glib::ControlFlow::Continue
            },
        ))
    }
}

fn observed_children(
    window: &gtk::Window,
    parent_id: &str,
    parent: &gtk::Widget,
    widgets: &mut Vec<Value>,
) {
    if let Some(container) = parent.downcast_ref::<gtk::Container>() {
        for (index, child) in container.children().into_iter().enumerate() {
            let id = format!("{parent_id}:{index}");
            widgets.push(layout_dump::owned_widget(
                window,
                &id,
                "chrome-content",
                &child,
                None,
            ));
            observed_children(window, &id, &child, widgets);
        }
    }
}

fn contains_webview(widget: &gtk::Widget) -> bool {
    widget.is::<webkit2gtk::WebView>()
        || widget
            .downcast_ref::<gtk::Container>()
            .is_some_and(|parent| parent.children().iter().any(contains_webview))
}
