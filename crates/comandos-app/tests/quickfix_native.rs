//! Explicit offscreen GTK fixture: no user window, input or tmux session is touched.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]
use comandos_app::ui::{
    confirm::{ConfirmAnswer, confirm_must_answer},
    tabstrip::TabStripLayout,
};
use gtk::prelude::*;
use std::{cell::Cell, rc::Rc, time::Duration};
#[allow(dead_code)]
#[path = "../src/jobs.rs"]
mod jobs;

fn button(widget: &gtk::Widget, label: &str) -> Option<gtk::Button> {
    if let Some(b) = widget.downcast_ref::<gtk::Button>() {
        if b.label().as_deref() == Some(label) {
            return Some(b.clone());
        }
    }
    widget
        .downcast_ref::<gtk::Container>()
        .and_then(|c| c.children().iter().find_map(|child| button(child, label)))
}

#[test]
#[ignore = "explicit offscreen GTK test using the existing display; never launches a browser or display server"]
fn modal_completions_leave_parent_alive_and_polling_does_not_reset_scroll() {
    gtk::init().unwrap();
    let window = gtk::OffscreenWindow::new();
    let overlay = gtk::Overlay::new();
    let strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let layout = TabStripLayout::new(&strip);
    overlay.add(layout.widget());
    window.add(&overlay);
    window.set_default_size(600, 300);
    let destroyed = Rc::new(Cell::new(false));
    window.connect_destroy({
        let destroyed = destroyed.clone();
        move |_| destroyed.set(true)
    });
    layout.set_items(
        (0..13)
            .map(|i| {
                let b = gtk::Button::with_label(&format!("session {i}"));
                b.set_size_request(160, 32);
                (format!("s{i}"), b.upcast())
            })
            .collect(),
    );
    window.show_all();
    let context = glib::MainContext::default();
    while context.pending() {
        context.iteration(false);
    }
    layout.set_current("s0");
    let scroll = layout
        .widget()
        .children()
        .into_iter()
        .find_map(|w| w.downcast::<gtk::ScrolledWindow>().ok())
        .unwrap();
    layout.scroll_by(280.);
    let offset = scroll.hadjustment().value();
    assert!(offset > 0.);
    for _ in 0..200 {
        layout.set_current("s0");
    }
    assert_eq!(scroll.hadjustment().value(), offset);
    for (label, expected) in [
        ("Cancelar", ConfirmAnswer::No),
        ("Cerrar", ConfirmAnswer::Yes),
    ] {
        let outer = glib::MainLoop::new(None, false);
        let result = Rc::new(Cell::new(None));
        let parent = window.clone();
        let click_parent = window.clone();
        let tx = jobs::to_main({
            let result = result.clone();
            let outer = outer.clone();
            move |()| {
                glib::idle_add_local_once(move || {
                    button(click_parent.upcast_ref(), label)
                        .expect("confirmation button")
                        .clicked();
                });
                result.set(Some(confirm_must_answer(
                    Some(parent.upcast_ref()),
                    "Cerrar split",
                    "Fixture",
                    "Cerrar",
                    "Cancelar",
                )));
                outer.quit();
            }
        });
        let timeout = glib::timeout_add_local_once(Duration::from_secs(3), {
            let outer = outer.clone();
            move || outer.quit()
        });
        tx.send_blocking(()).unwrap();
        outer.run();
        timeout.remove();
        assert_eq!(result.get(), Some(expected));
        assert!(
            !destroyed.get(),
            "closing a split confirmation must keep the application window alive"
        );
        assert_eq!(
            overlay.children().len(),
            1,
            "confirmation must release only its own widgets"
        );
    }
    layout.set_rows(true);
    // GTK may need a frame after changing the number of rows.
    let frame = glib::MainLoop::new(None, false);
    glib::timeout_add_local_once(Duration::from_millis(80), {
        let frame = frame.clone();
        move || frame.quit()
    });
    frame.run();
    let widths = layout
        .entries()
        .iter()
        .map(|(_, w)| w.allocated_width())
        .collect::<Vec<_>>();
    assert!(widths.iter().all(|w| *w > 1));
    assert!(
        widths.iter().max().unwrap() - widths.iter().min().unwrap() <= 1,
        "cards, including the last row, must have uniform widths: {widths:?}"
    );
    window.close();
}
