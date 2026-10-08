//! Confirmación embebida: fondo y Escape no deciden por el usuario.
use gtk::prelude::*;
use std::{cell::Cell, rc::Rc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAnswer {
    Yes,
    No,
}

pub fn confirm_must_answer(
    parent: Option<&gtk::Window>,
    primary: &str,
    secondary: &str,
    yes_label: &str,
    no_label: &str,
) -> ConfirmAnswer {
    let Some(parent) = parent else {
        return ConfirmAnswer::No;
    };
    let Some(child) = parent.child() else {
        return ConfirmAnswer::No;
    };
    let overlay = if let Ok(overlay) = child.clone().downcast::<gtk::Overlay>() {
        overlay
    } else {
        parent.remove(&child);
        let overlay = gtk::Overlay::new();
        overlay.add(&child);
        parent.add(&overlay);
        overlay.show();
        overlay
    };
    let backdrop = gtk::EventBox::new();
    backdrop.set_hexpand(true);
    backdrop.set_vexpand(true);
    backdrop.style_context().add_class("cc-modal-backdrop");
    backdrop.connect_button_press_event(|_, _| glib::Propagation::Stop);
    let panel = gtk::Frame::new(None);
    panel.set_size_request(480, -1);
    panel.set_halign(gtk::Align::Center);
    panel.set_valign(gtk::Align::Center);
    panel.style_context().add_class("cc-snip-dialog");
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    panel.add(&root);
    let body = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    body.set_margin_top(20);
    body.set_margin_bottom(16);
    body.set_margin_start(22);
    body.set_margin_end(22);
    body.pack_start(
        &gtk::Image::from_icon_name(Some("dialog-question-symbolic"), gtk::IconSize::Dialog),
        false,
        false,
        0,
    );
    let text = gtk::Box::new(gtk::Orientation::Vertical, 6);
    for (value, class) in [(primary, "primary"), (secondary, "dim-label")] {
        let label = gtk::Label::new(Some(value));
        label.set_xalign(0.);
        label.set_wrap(true);
        label.style_context().add_class(class);
        text.pack_start(&label, false, false, 0);
    }
    body.pack_start(&text, true, true, 0);
    root.pack_start(&body, true, true, 0);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_margin_top(6);
    actions.set_margin_bottom(14);
    actions.set_margin_start(16);
    actions.set_margin_end(16);
    actions.set_homogeneous(true);
    let no = gtk::Button::with_label(no_label);
    let yes = gtk::Button::with_label(yes_label);
    actions.pack_start(&no, true, true, 0);
    actions.pack_start(&yes, true, true, 0);
    root.pack_start(&actions, false, false, 0);
    let answer = Rc::new(Cell::new(ConfirmAnswer::No));
    let finished = Rc::new(Cell::new(false));
    let loop_ = glib::MainLoop::new(None, false);
    let finish: Rc<dyn Fn(ConfirmAnswer)> = Rc::new({
        let answer = answer.clone();
        let finished = finished.clone();
        let loop_ = loop_.clone();
        move |value| {
            if !finished.replace(true) {
                answer.set(value);
                loop_.quit();
            }
        }
    });
    for (button, value) in [(&no, ConfirmAnswer::No), (&yes, ConfirmAnswer::Yes)] {
        let finish = finish.clone();
        button.connect_clicked(move |_| finish(value));
    }
    let key_handler = parent.connect_key_press_event({
        let finish = finish.clone();
        move |_, e| {
            match e.keyval() {
                gdk::keys::constants::y
                | gdk::keys::constants::Y
                | gdk::keys::constants::Return
                | gdk::keys::constants::KP_Enter => finish(ConfirmAnswer::Yes),
                gdk::keys::constants::n | gdk::keys::constants::N => finish(ConfirmAnswer::No),
                _ => {}
            }
            glib::Propagation::Stop
        }
    });
    let destroy_handler = parent.connect_destroy({
        let finish = finish.clone();
        move |_| finish(ConfirmAnswer::No)
    });
    overlay.add_overlay(&backdrop);
    overlay.add_overlay(&panel);
    backdrop.show_all();
    panel.show_all();
    yes.grab_focus();
    loop_.run();
    parent.disconnect(key_handler);
    parent.disconnect(destroy_handler);
    overlay.remove(&panel);
    overlay.remove(&backdrop);
    answer.get()
}
