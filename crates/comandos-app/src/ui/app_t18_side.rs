use super::*;
use ui::app_commands::{self as commands, CommandError};
use webkit2gtk::WebViewExt;
impl App {
    pub(super) fn install_auxiliary_views(self: &Rc<Self>) {
        for name in [
            "T18.reader",
            "T18.sidebar_term",
            "T18.left_panel",
            "T18.chain_modal",
            "T18.web_modal",
            "T18.web_tab",
            "T18.chains",
            "T18.analytics",
            "T18.mosaic",
            "T18.mosaic_zoom",
        ] {
            let weak = Rc::downgrade(self);
            self.install_handler(
                name,
                Rc::new(move |args| {
                    let app = weak
                        .upgrade()
                        .filter(|a| !a.closed.load(Ordering::Acquire))
                        .ok_or_else(|| CommandError::Refused("application closed".into()))?;
                    match name {
                        "T18.reader" => app.reader_action(args),
                        "T18.sidebar_term" => {
                            app.side_from_web(args);
                            Ok(())
                        }
                        "T18.left_panel" => {
                            let action = args.get("action").and_then(Value::as_str);
                            let hidden = match action {
                                Some("toggle") => !app.t18.state.borrow().left_hidden,
                                Some("hide") => true,
                                Some("show") => false,
                                None => args.get("hidden").is_some_and(comandos_core::json::truthy),
                                _ => {
                                    return Err(CommandError::Invalid(
                                        "invalid left panel action".into(),
                                    ));
                                }
                            };
                            app.left_panel_set(hidden);
                            Ok(())
                        }
                        "T18.chain_modal" => app.chain_message(args),
                        "T18.web_tab" => app.open_web_tab(
                            commands::string_arg(args, "url", "")?,
                            args.get("label")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                        ),
                        "T18.web_modal" => app.open_web_modal_label(
                            commands::string_arg(args, "url", "")?,
                            ui::modals::ModalKind::Url,
                            args.get("label")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                        ),
                        "T18.chains" => app.open_internal_modal(ui::modals::ModalKind::Chains),
                        "T18.analytics" => {
                            app.open_internal_modal(ui::modals::ModalKind::Analytics)
                        }
                        "T18.mosaic" => {
                            let state = commands::string_arg(args, "state", "toggle")?;
                            match state {
                                "on" => app.mosaic_open(),
                                "off" => app.mosaic_close(),
                                "toggle" => {
                                    let on =
                                        app.t18.mosaic.borrow().on || app.t18.mosaic_loading.get();
                                    if on {
                                        app.mosaic_close()
                                    } else {
                                        app.mosaic_open()
                                    }
                                }
                                _ => {
                                    return Err(CommandError::Invalid(
                                        "invalid mosaic state".into(),
                                    ));
                                }
                            };
                            Ok(())
                        }
                        _ => {
                            let session = commands::session_arg(args, "")?;
                            app.mosaic_zoom(session);
                            Ok(())
                        }
                    }
                }),
            );
        }
        for (button, amount) in [(&self.t18.previous, -150.0), (&self.t18.next, 150.0)] {
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(app) = weak.upgrade() {
                    app.side_scroll(amount);
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.t18.toggle.connect_clicked(move |_| {
            if let Some(app) = weak.upgrade() {
                app.side_action("toggle", "");
            }
        });
        if let Some(plus) = self
            .t18
            .head
            .children()
            .into_iter()
            .find(|w| w.widget_name() == "side-plus")
            .and_then(|w| w.downcast::<gtk::Button>().ok())
        {
            let weak = Rc::downgrade(self);
            plus.connect_clicked(move |_| {
                if let Some(app) = weak.upgrade() {
                    app.side_action("new", "");
                }
            });
        }
        let adjustment = self.t18.tabs_scroll.hadjustment();
        let weak = Rc::downgrade(self);
        adjustment.connect_changed(move |_| {
            if let Some(app) = weak.upgrade() {
                app.side_sync_arrows();
            }
        });
        let weak = Rc::downgrade(self);
        adjustment.connect_value_changed(move |_| {
            if let Some(app) = weak.upgrade() {
                app.side_sync_arrows();
            }
        });
        self.t18
            .tabs_scroll
            .add_events(gdk::EventMask::SCROLL_MASK | gdk::EventMask::SMOOTH_SCROLL_MASK);
        let weak = Rc::downgrade(self);
        self.t18.tabs_scroll.connect_scroll_event(move |_, event| {
            if let Some(app) = weak.upgrade() {
                let (x, y) = event.delta();
                let discrete = match event.direction() {
                    gdk::ScrollDirection::Up | gdk::ScrollDirection::Left => Some(-1.),
                    gdk::ScrollDirection::Down | gdk::ScrollDirection::Right => Some(1.),
                    _ => None,
                };
                let delta = ui::side::wheel_delta(x, y, discrete);
                app.side_scroll(delta);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        let weak = Rc::downgrade(self);
        self.t18.head.connect_size_allocate(move |_, _| {
            if let Some(app) = weak.upgrade() {
                app.queue_side_layout();
            }
        });
        let weak = Rc::downgrade(self);
        self.t18.head_event.connect_button_press_event(move |_, e| {
            let Some(app) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let pinned = app.side_pin_position().is_some();
            if e.button() == 1
                && e.event_type() == gdk::EventType::ButtonPress
                && !app.t18.state.borrow().collapsed
                && !pinned
            {
                app.t18.head_event.style_context().add_class("dragging");
                app.t18
                    .drag
                    .set(Some((e.root().1, app.t18.side.position())));
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        let weak = Rc::downgrade(self);
        self.t18
            .head_event
            .connect_motion_notify_event(move |_, e| {
                if let Some(app) = weak.upgrade()
                    && let Some(start) = app.t18.drag.get()
                {
                    let position = ui::side::drag_position(
                        start,
                        e.root().1,
                        app.t18.side.min_position(),
                        app.t18.side.max_position(),
                    );
                    app.t18.side.set_position(position);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
        let weak = Rc::downgrade(self);
        self.t18
            .head_event
            .connect_button_release_event(move |_, _e| {
                if let Some(app) = weak.upgrade()
                    && app.t18.drag.take().is_some()
                {
                    app.t18.head_event.style_context().remove_class("dragging");
                    app.save_side_share();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
        let weak = Rc::downgrade(self);
        self.t18.side.connect_position_notify(move |_| {
            if let Some(app) = weak.upgrade() {
                if app.t18.state.borrow().collapsed || app.side_pin_position().is_some() {
                    app.queue_side_layout();
                } else {
                    app.queue_side_share();
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.t18.side.connect_size_allocate(move |_, _| {
            if let Some(app) = weak.upgrade() {
                app.queue_side_layout();
            }
        });
        let weak = Rc::downgrade(self);
        self.paned.connect_size_allocate(move |_, _| {
            if let Some(app) = weak.upgrade()
                && app.t18.state.borrow().left_hidden
            {
                app.t18.side.hide();
            }
        });
        self.install_aux_signals();
        self.t18.head_event.connect_realize(|widget| {
            if let Some(window) = widget.window() {
                window.set_cursor(gdk::Cursor::from_name(&widget.display(), "row-resize").as_ref());
            }
        });
        self.load_aux_layout();
    }
    fn load_aux_layout(self: &Rc<Self>) {
        let state = self.state.clone();
        let ticket = self.t18.scope.ticket();
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || state.read_ui_document("app-layout.json"),
            move |result| {
                if let Some(app) = weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire))
                    && ticket.current()
                {
                    match result {
                        Ok(layout) => match ui::side::SideState::from_json(&layout) {
                            Ok(state) => {
                                *app.t18.layout_doc.borrow_mut() = layout;
                                *app.t18.state.borrow_mut() = state;
                                let hidden = app.t18.state.borrow().left_hidden;
                                app.t18.side.set_visible(!hidden);
                                app.paint_left_button(hidden);
                            }
                            Err(error) => app.status.set_text(&error.to_string()),
                        },
                        Err(error) => app.status.set_text(&format!("Auxiliary layout: {error:?}")),
                    }
                }
            },
        );
    }
    fn side_action(&self, action: &str, id: &str) {
        self.quiet_js(&format!(
            "window.sidebarTermAction&&sidebarTermAction({},{})",
            json!(action),
            json!(id)
        ));
    }
    fn side_scroll(&self, amount: f64) {
        let a = self.t18.tabs_scroll.hadjustment();
        a.set_value(ui::side::scroll_position(
            a.value(),
            amount,
            a.lower(),
            a.upper(),
            a.page_size(),
        ));
        self.side_sync_arrows();
    }
    fn side_sync_arrows(&self) {
        let a = self.t18.tabs_scroll.hadjustment();
        let (visible, previous, next) = ui::side::arrow_state(
            self.t18.head.allocated_width(),
            a.value(),
            a.lower(),
            a.upper(),
            a.page_size(),
        );
        self.t18.previous.set_visible(visible);
        self.t18.next.set_visible(visible);
        self.t18.previous.set_sensitive(previous);
        self.t18.next.set_sensitive(next);
    }
    fn side_pin_position(&self) -> Option<i32> {
        self.t18.state.borrow().pin_position(
            self.t18.host.is_visible(),
            self.webview.zoom_level(),
            self.t18.side.min_position(),
            self.t18.side.max_position(),
        )
    }
    fn queue_side_layout(self: &Rc<Self>) {
        if self.closed.load(Ordering::Acquire) || self.t18.queued.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(self);
        let ticket = self.t18.side_scope.ticket();
        *self.t18.queued.borrow_mut() = Some(glib::idle_add_local_once(move || {
            if let Some(app) = weak.upgrade() {
                app.t18.queued.borrow_mut().take();
                if ticket.current() {
                    app.side_keep_layout();
                }
            }
        }));
    }
    fn side_keep_layout(&self) {
        if self.t18.state.borrow().left_hidden {
            self.t18.side.hide();
            return;
        }
        let state = self.t18.state.borrow().clone();
        let visible = !state.sheet_open;
        self.t18.host.set_visible(visible);
        let current = self.t18.side_current.borrow().clone();
        let overlay = current.as_deref().and_then(|key| {
            self.t18
                .clients
                .borrow()
                .get(key)
                .map(|c| c.overlay.clone())
        });
        if let Some(overlay) = overlay {
            overlay.widget.set_visible(!state.collapsed && visible);
        }
        if state.collapsed && visible {
            let position = self.t18.side.max_position();
            if self.t18.side.position() != position {
                self.t18.side.set_position(position);
            }
        } else if let Some(position) = state.share_position(
            self.t18.side.allocated_height(),
            visible,
            self.side_pin_position(),
        ) && self.t18.side.position() != position
        {
            self.t18.side.set_position(position);
        }
        self.side_sync_arrows();
    }
    fn queue_side_share(self: &Rc<Self>) {
        if let Some(old) = self.t18.share_save.borrow_mut().take() {
            old.remove();
        }
        let weak = Rc::downgrade(self);
        let ticket = self.t18.scope.ticket();
        *self.t18.share_save.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_millis(600),
            move || {
                if let Some(app) = weak.upgrade() {
                    app.t18.share_save.borrow_mut().take();
                    if ticket.current() && !app.closed.load(Ordering::Acquire) {
                        app.save_side_share();
                    }
                }
            },
        ));
    }
    fn save_side_share(self: &Rc<Self>) {
        let pinned = self.side_pin_position().is_some();
        if self.t18.state.borrow_mut().save_share(
            self.t18.side.allocated_height(),
            self.t18.side.position(),
            self.t18.host.is_visible(),
            pinned,
        ) {
            self.queue_ui_save("app-layout.json", 0);
        }
    }
    fn side_render(self: &Rc<Self>) {
        for old in self.t18.tabs.children() {
            self.t18.tabs.remove(&old);
        }
        let state = self.t18.state.borrow().clone();
        let has_tabs = !state.tabs.is_empty();
        let mut selected = None;
        for tab in state.tabs {
            let key = tab
                .get("id")
                .unwrap_or(&Value::Null)
                .as_str()
                .unwrap_or_default()
                .to_string();
            let ev = gtk::EventBox::new();
            ev.set_valign(gtk::Align::Center);
            ev.set_margin_start(3);
            ev.set_margin_end(3);
            let css = ev.style_context();
            css.add_class("side-tab");
            if tab.get("on").unwrap_or(&Value::Null) == true && !state.hidden {
                css.add_class("cur");
            }
            if tab.get("sel").unwrap_or(&Value::Null) == true {
                css.add_class("sel");
            }
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            ev.add(&row);
            let name = super::app_t18::side_button(
                "",
                &["side-name"],
                tab.get("title")
                    .unwrap_or(&Value::Null)
                    .as_str()
                    .unwrap_or_default(),
            );
            let inner = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            let dot = gtk::Label::new(Some("●"));
            dot.style_context().add_class("side-dot");
            inner.pack_start(&dot, false, false, 0);
            inner.pack_start(
                &gtk::Label::new(tab.get("label").unwrap_or(&Value::Null).as_str()),
                false,
                false,
                0,
            );
            name.add(&inner);
            let weak = Rc::downgrade(self);
            let id = key.clone();
            name.connect_clicked(move |_| {
                if let Some(app) = weak.upgrade() {
                    app.side_action("focus", &id);
                }
            });
            row.pack_start(&name, false, false, 0);
            let closing = tab.get("closing").unwrap_or(&Value::Null) == true;
            let close = super::app_t18::side_button(
                if closing { "¿Cerrar?" } else { "✕" },
                if closing {
                    &["side-x", "armed"]
                } else {
                    &["side-x"]
                },
                if closing {
                    "Otro clic la cierra"
                } else {
                    "Cerrar esta terminal"
                },
            );
            let weak = Rc::downgrade(self);
            let id = key.clone();
            close.connect_clicked(move |_| {
                if let Some(app) = weak.upgrade() {
                    app.side_action("close", &id);
                }
            });
            row.pack_start(&close, false, false, 0);
            self.t18.tabs.pack_start(&ev, false, false, 0);
            if tab.get("on").unwrap_or(&Value::Null) == true {
                selected = Some((key, ev));
            }
        }
        self.t18.tabs.show_all();
        self.t18.toggle.set_visible(has_tabs);
        let dim = self
            .applied_theme
            .borrow()
            .as_ref()
            .and_then(|t| t.values.get("dim"))
            .and_then(Value::as_str)
            .unwrap_or("#AAAAAA")
            .to_string();
        let image = ui::icons::image(
            if state.hidden {
                "chevron-up"
            } else {
                "chevron-down"
            },
            18,
            &dim,
        );
        self.t18.toggle.set_image(Some(&image));
        image.show();
        let ctx = self.t18.head_event.style_context();
        if state.hidden && has_tabs {
            ctx.add_class("closed");
        } else {
            ctx.remove_class("closed");
        }
        self.t18.toggle.set_tooltip_text(Some(if state.hidden {
            "Mostrar terminal"
        } else {
            "Ocultar terminal"
        }));
        if let Some((key, widget)) = selected
            && *self.t18.shown.borrow() != key
        {
            *self.t18.shown.borrow_mut() = key;
            let weak = Rc::downgrade(self);
            let w = widget.downgrade();
            let ticket = self.t18.side_scope.ticket();
            if let Some(old) = self.t18.reveal.borrow_mut().take() {
                old.remove();
            }
            *self.t18.reveal.borrow_mut() = Some(glib::idle_add_local_once(move || {
                if let Some(app) = weak.upgrade() {
                    app.t18.reveal.borrow_mut().take();
                    if let Some(w) = w.upgrade()
                        && ticket.current()
                    {
                        let a = w.allocation();
                        let adj = app.t18.tabs_scroll.hadjustment();
                        if a.width() > 1 {
                            if f64::from(a.x()) < adj.value() {
                                adj.set_value(f64::from(a.x()));
                            } else if f64::from(a.x() + a.width()) > adj.value() + adj.page_size() {
                                adj.set_value(f64::from(a.x() + a.width()) - adj.page_size());
                            }
                        }
                    }
                }
            }));
        }
        self.queue_side_layout();
    }
    fn side_from_web(self: &Rc<Self>, data: &Value) {
        let before_state = self.t18.state.borrow().clone();
        let before = (before_state.session.clone(), before_state.hidden);
        self.t18.state.borrow_mut().adopt_web(data);
        if *self.t18.state.borrow() == before_state {
            return;
        }
        let changed = {
            let state = self.t18.state.borrow();
            (state.session.clone(), state.hidden) != before
        };
        if changed {
            self.t18.side_scope.advance();
        }
        self.side_render();
        self.side_show_current();
    }
    fn side_show_current(self: &Rc<Self>) {
        let state = self.t18.state.borrow().clone();
        let session = state.session;
        if state.hidden || session.is_empty() {
            self.t18.state.borrow_mut().collapsed = true;
            self.queue_side_layout();
            return;
        }
        let keep = self
            .t18
            .state
            .borrow()
            .tabs
            .iter()
            .filter_map(|t| t.get("id").and_then(Value::as_str).map(str::to_string))
            .collect::<BTreeSet<_>>();
        let old = self
            .t18
            .clients
            .borrow()
            .keys()
            .filter(|key| {
                key.starts_with("side:") && !keep.contains(key.trim_start_matches("side:"))
            })
            .cloned()
            .collect::<Vec<_>>();
        for key in old {
            if key != format!("side:{session}") {
                self.close_aux(&key);
            }
        }
        let id = format!("side:{session}");
        let cached = self
            .t18
            .clients
            .borrow()
            .get(&id)
            .map(|client| client.overlay.widget.clone());
        if let Some(widget) = cached
            && self.t18.side_current.borrow().as_deref() == Some(id.as_str())
        {
            self.t18.state.borrow_mut().collapsed = false;
            self.show_side_client(&id, &widget);
            return;
        }
        self.t18.state.borrow_mut().collapsed = true;
        self.queue_side_layout();
        if self.t18.side_loading.replace(true) {
            return;
        }
        let tmux = self.tmux.clone();
        let worker_session = session.clone();
        let ticket = self.t18.side_scope.ticket();
        let worker = ticket.clone();
        let weak = Rc::downgrade(self);
        let closed = self.closed.clone();
        self.jobs.spawn(
            move || {
                if !worker.current() || closed.load(Ordering::Acquire) {
                    return false;
                }
                tmux.read(&["has-session", "-t", &format!("={worker_session}")])
                    .is_ok_and(|o| o.ok())
                    && worker.current()
                    && !closed.load(Ordering::Acquire)
            },
            move |exists| {
                let Some(app) = weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire)) else {
                    return;
                };
                app.t18.side_loading.set(false);
                if !ticket.current() {
                    app.side_show_current();
                    return;
                }
                if exists
                    && app.t18.state.borrow().session == session
                    && !app.t18.state.borrow().hidden
                {
                    let cached = app
                        .t18
                        .clients
                        .borrow()
                        .get(&id)
                        .map(|client| client.overlay.clone());
                    let target = if let Some(overlay) = cached {
                        Ok(overlay)
                    } else {
                        app.aux_terminal(&session, &id, "side", 10000)
                    };
                    app.t18.state.borrow_mut().collapsed = false;
                    match target {
                        Ok(overlay) => {
                            app.show_side_client(&id, &overlay.widget);
                            app.pane_geo_soon(&overlay);
                        }
                        Err(error) => app.status.set_text(&error),
                    }
                } else {
                    app.t18.state.borrow_mut().collapsed = true;
                    app.queue_side_layout();
                }
            },
        );
    }
    fn show_side_client(self: &Rc<Self>, id: &str, widget: &gtk::Overlay) {
        let previous = self.t18.side_current.borrow().clone();
        if let Some(previous) = previous
            && previous != id
            && let Some(old) = self.t18.clients.borrow().get(&previous)
        {
            old.overlay.widget.hide();
        }
        if widget.parent().is_none() {
            self.t18.host.pack_start(widget, true, true, 0);
        }
        *self.t18.side_current.borrow_mut() = Some(id.into());
        self.t18.host.show();
        widget.show_all();
        self.queue_side_layout();
    }
    pub(super) fn queue_ui_save(self: &Rc<Self>, name: &'static str, delay: u64) {
        if !self.writable() {
            return;
        }
        let (slot, busy) = if name == "app-layout.json" {
            (&self.t18.layout_save, &self.t18.layout_busy)
        } else {
            (&self.t18.pane_save, &self.t18.pane_busy)
        };
        if let Some(old) = slot.borrow_mut().take() {
            old.remove();
        }
        if busy.get() {
            return;
        }
        let weak = Rc::downgrade(self);
        *slot.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_millis(delay),
            move || {
                if let Some(app) = weak.upgrade() {
                    app.flush_ui_save(name);
                }
            },
        ));
    }
    fn flush_ui_save(self: &Rc<Self>, name: &'static str) {
        let layout = name == "app-layout.json";
        let (slot, busy, scope) = if layout {
            (
                &self.t18.layout_save,
                &self.t18.layout_busy,
                &self.t18.layout_scope,
            )
        } else {
            (
                &self.t18.pane_save,
                &self.t18.pane_busy,
                &self.t18.pane_scope,
            )
        };
        slot.borrow_mut().take();
        if !self.writable() || busy.replace(true) {
            return;
        }
        let old = if layout {
            self.t18.layout_doc.borrow().clone()
        } else {
            self.t18.pane_doc.borrow().clone()
        };
        let next = if layout {
            self.t18.state.borrow().to_json()
        } else {
            let mut d = if old.is_object() {
                old.clone()
            } else {
                json!({})
            };
            if let Some(map) = d.as_object_mut() {
                map.insert(
                    "position".into(),
                    json!(self.pane_saved.get().unwrap_or(self.paned.position())),
                );
            }
            d
        };
        let worker_next = next.clone();
        let state = self.state.clone();
        let ticket = scope.ticket();
        let worker = ticket.clone();
        let closed = self.closed.clone();
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || {
                state.write_ui_document_when(name, &old, &worker_next, || {
                    worker.current() && !closed.load(Ordering::Acquire)
                })
            },
            move |result| {
                if let Some(app) = weak.upgrade().filter(|a| !a.closed.load(Ordering::Acquire))
                    && ticket.current()
                {
                    let busy = if layout {
                        &app.t18.layout_busy
                    } else {
                        &app.t18.pane_busy
                    };
                    busy.set(false);
                    match result {
                        Ok(true) => {
                            if layout {
                                *app.t18.layout_doc.borrow_mut() = next.clone();
                                if app.t18.state.borrow().to_json() != next {
                                    app.queue_ui_save(name, 600);
                                }
                            } else {
                                *app.t18.pane_doc.borrow_mut() = next.clone();
                                if next.get("position").unwrap_or(&Value::Null)
                                    != &json!(app.pane_saved.get().unwrap_or(app.paned.position()))
                                {
                                    app.queue_ui_save(name, 250);
                                }
                            }
                        }
                        Ok(false) => app
                            .status
                            .set_text("Layout changed concurrently; save refused"),
                        Err(error) => app.status.set_text(&format!("Layout save: {error:?}")),
                    }
                }
            },
        );
    }
}

impl App {
    fn install_aux_signals(self: &Rc<Self>) {
        let relay = match ui::signals::SignalRelay::new() {
            Ok(relay) => relay,
            Err(error) => {
                self.status.set_text(&format!("Desktop signals: {error}"));
                return;
            }
        };
        let fd = relay.fd();
        *self.t18.relay.borrow_mut() = Some(relay);
        let weak = Rc::downgrade(self);
        let ticket = self.t18.scope.ticket();
        self.t18.sources.borrow_mut().push(glib::source::unix_fd_add_local(fd,glib::IOCondition::IN,move|_,_|{
            let Some(app)=weak.upgrade().filter(|a|!a.closed.load(Ordering::Acquire))else{return glib::ControlFlow::Break;};
            if !ticket.current(){return glib::ControlFlow::Break;}let actions=app.t18.relay.borrow_mut().as_mut().map(|r|r.drain()).unwrap_or_default();
            for action in actions{match action {
                ui::signals::Action::Mosaic=>{let active=app.t18.mosaic.borrow().on||app.t18.mosaic_loading.get();if active{app.mosaic_close();}else{app.mosaic_open();}},
                ui::signals::Action::NoticesDebug=>app.quiet_js("document.getElementById('btn-notif').click();setTimeout(()=>console.log('NFDBG cards=',document.querySelectorAll('#notif-panel .nf2').length,'panel=',getComputedStyle(document.getElementById('notif-panel')||document.body).display,'badge=',document.getElementById('notif-badge').textContent,'webBtns=',document.querySelectorAll('.nf2-web').length),2500)"),
                ui::signals::Action::JavascriptDebug=>app.debug_js_file(),
            }}glib::ControlFlow::Continue
        }));
    }
    fn debug_js_file(self: &Rc<Self>) {
        if !self.writable() || self.t18.debug_loading.replace(true) {
            return;
        }
        let path = self.cfg.runtime_dir().join("comandos-dbg.js");
        let ticket = self.t18.scope.ticket();
        let weak = Rc::downgrade(self);
        self.jobs.spawn(
            move || ui::signals::debug_script(&path, nix::unistd::geteuid().as_raw()),
            move |result| {
                if let Some(app) = weak.upgrade()
                    && {
                        app.t18.debug_loading.set(false);
                        app.writable()
                    }
                    && ticket.current()
                {
                    match result {
                        Ok(code) => {
                            println!(
                                "NFGTK paned_pos={} wv_alloc={}x{} zoom={}",
                                app.paned.position(),
                                app.webview.allocated_width(),
                                app.webview.allocated_height(),
                                app.webview.zoom_level()
                            );
                            app.quiet_js(&code);
                        }
                        Err(error) => app.status.set_text(&format!("Debug script: {error}")),
                    }
                }
            },
        );
    }
}
