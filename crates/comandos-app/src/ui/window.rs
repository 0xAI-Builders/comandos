use crate::config::AppConfig;
use gtk::prelude::*;

pub fn create(application: &gtk::Application, cfg: &AppConfig) -> gtk::ApplicationWindow {
    let window = gtk::ApplicationWindow::new(application);
    window.set_title(cfg.title());
    window.set_default_size(1280, 820);
    window.set_position(gtk::WindowPosition::Center);
    window.set_widget_name(cfg.wm_class());
    window.set_app_paintable(true);
    if let Some(screen) = gdk::Screen::default()
        && let Some(visual) = screen.rgba_visual()
    {
        window.set_visual(Some(&visual));
    }
    window
}
