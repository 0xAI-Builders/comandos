//! Modelos y controles de la cabecera de la ventana propia.
use gtk::prelude::*;
pub const HEADER_ACTIONS: [&str; 6] = [
    "quickTerminal",
    "newSession",
    "sortMenu",
    "notices",
    "chains",
    "analytics",
];
pub fn badge_text(count: u64) -> Option<String> {
    (count > 0).then(|| count.to_string())
}
pub fn tween(elapsed_us: f64, from: f64, to: f64, ms: f64) -> f64 {
    let t = (elapsed_us / (ms * 1000.)).clamp(0., 1.);
    from + (to - from) * (1. - (1. - t).powi(3))
}
pub struct Header {
    pub bar: gtk::HeaderBar,
    brand: gtk::Label,
    controls: Vec<(gtk::Button, &'static str)>,
    pub badge: gtk::Label,
    pub actions: Vec<(gtk::Button, &'static str)>,
    pub hourglass: gtk::Image,
    pub notice_wrap: gtk::Overlay,
}
impl Header {
    pub fn new(window: &gtk::ApplicationWindow, english: bool) -> Self {
        let bar = gtk::HeaderBar::new();
        bar.set_show_close_button(false);
        bar.style_context().add_class("cc-headerbar");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let brand = gtk::Label::new(None);
        row.pack_start(&brand, false, false, 0);
        let name = gtk::Label::new(Some("COMANDOS"));
        name.style_context().add_class("cc-brand");
        row.pack_start(&name, false, false, 0);
        bar.pack_start(&row);
        let mut controls = Vec::new();
        for (icon, label) in [
            ("close", if english { "Close" } else { "Cerrar" }),
            ("maximize", if english { "Maximize" } else { "Maximizar" }),
            ("minimize", if english { "Minimize" } else { "Minimizar" }),
        ] {
            let b = super::icons::button(icon, 16, label);
            b.style_context().add_class("cc-winctl");
            if icon == "close" {
                b.style_context().add_class("cc-close-btn");
            }
            let weak = window.downgrade();
            b.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() {
                    match icon {
                        "close" => w.close(),
                        "maximize" => {
                            if w.is_maximized() {
                                w.unmaximize()
                            } else {
                                w.maximize()
                            }
                        }
                        _ => w.iconify(),
                    }
                }
            });
            bar.pack_end(&b);
            controls.push((b, icon));
        }
        let badge = gtk::Label::new(None);
        badge.set_no_show_all(true);
        badge.style_context().add_class("cc-badge");
        let mut actions = Vec::new();
        for (icon, key, label) in [
            (
                "settings",
                "settings",
                if english { "Settings" } else { "Ajustes" },
            ),
            (
                "bell",
                "notif",
                if english {
                    "Notifications"
                } else {
                    "Notificaciones"
                },
            ),
            (
                "sparkles",
                "news",
                if english { "Summaries" } else { "Resúmenes" },
            ),
            ("timer", "pomo", "Pomodoro"),
        ] {
            let b = super::icons::button(icon, 16, label);
            b.style_context().add_class("cc-key");
            b.style_context().add_class(&format!("cc-key-{key}"));
            actions.push((b, key));
        }
        let hourglass = gtk::Image::new();
        if let Some((button, _)) = actions.iter().find(|(_, k)| *k == "pomo") {
            button.set_image(Some(&hourglass));
        }
        let notice_wrap = gtk::Overlay::new();
        if let Some((button, _)) = actions.iter().find(|(_, k)| *k == "notif") {
            notice_wrap.add(button);
        }
        badge.set_halign(gtk::Align::End);
        badge.set_valign(gtk::Align::Start);
        notice_wrap.add_overlay(&badge);
        notice_wrap.set_overlay_pass_through(&badge, true);
        Self {
            bar,
            brand,
            controls,
            badge,
            actions,
            hourglass,
            notice_wrap,
        }
    }
    pub fn paint(&self, theme: &crate::theme::ThemeTokens) {
        let brand = theme
            .values
            .get("brand")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("#8B7CF6");
        self.brand.set_markup(&format!(
            r#"<span size="10500" foreground="{}">●</span>"#,
            glib::markup_escape_text(brand)
        ));
        let dim = theme
            .values
            .get("dim")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("#9AA6BF");
        for (b, name) in &self.controls {
            b.set_image(Some(&super::icons::image(name, 16, dim)));
        }
        for (b, key) in self.actions.iter().filter(|(_, k)| *k != "pomo") {
            let name = match *key {
                "notif" => "bell",
                "news" => "sparkles",
                "pomo" => "timer",
                _ => "settings",
            };
            b.set_image(Some(&super::icons::image(name, 16, dim)));
        }
    }
    pub fn set_badge(&self, count: u64) {
        if let Some(text) = badge_text(count) {
            self.badge.set_text(&text);
            self.badge.show();
        } else {
            self.badge.hide();
        }
    }
}
pub fn install(app: &std::rc::Rc<super::app::App>) {
    app.install_header();
}

pub fn animations_enabled() -> bool {
    gtk::Settings::default()
        .map(|s| s.is_gtk_enable_animations())
        .unwrap_or(true)
}

/// El reloj empieza con la primera entrega de FrameClock, como el original.
pub struct Tween {
    start: Option<f64>,
    from: f64,
    to: f64,
    ms: f64,
}
impl Tween {
    pub fn new(from: f64, to: f64, ms: f64) -> Self {
        Self {
            start: None,
            from,
            to,
            ms,
        }
    }
    pub fn step(&mut self, clock_us: f64) -> (f64, bool) {
        let start = *self.start.get_or_insert(clock_us);
        (
            tween(clock_us - start, self.from, self.to, self.ms),
            clock_us - start >= self.ms * 1000.,
        )
    }
}
/// GTK retira este callback al destruir su widget; el llamador puede cancelarlo antes.
pub fn animate(
    widget: &impl IsA<gtk::Widget>,
    apply: impl Fn(f64) + 'static,
    from: f64,
    to: f64,
    ms: f64,
    done: Option<Box<dyn FnOnce()>>,
) -> gtk::TickCallbackId {
    let clock = std::cell::RefCell::new(Tween::new(from, to, ms));
    let done = std::cell::RefCell::new(done);
    widget.add_tick_callback(move |_, frame| {
        let (value, finished) = clock.borrow_mut().step(frame.frame_time() as f64);
        apply(value);
        if finished {
            if let Some(done) = done.borrow_mut().take() {
                done();
            }
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    })
}
