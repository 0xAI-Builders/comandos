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
    let previous = gtk::Button::with_label("‹");
    let next = gtk::Button::with_label("›");
    previous.set_widget_name("tab-cycle-prev");
    next.set_widget_name("tab-cycle-next");
    for (button, host) in [
        (&previous, layout.navigation()),
        (&next, layout.navigation()),
    ] {
        button.style_context().add_class("tab-cycle");
        host.pack_start(button, false, false, 0);
    }
    for label in ["Terminal", "+", "Sort", "Rows"] {
        let action = gtk::Button::with_label(label);
        layout.actions().pack_start(&action, false, false, 0);
    }
    overlay.add(layout.widget());
    window.add(&overlay);
    window.set_default_size(600, 300);
    let destroyed = Rc::new(Cell::new(false));
    window.connect_destroy({
        let destroyed = destroyed.clone();
        move |_| destroyed.set(true)
    });
    let labels = (0..13)
        .map(|i| {
            comandos_app::ui::tab_label::TabLabel::new(
                &format!("Session {i} with a complete descriptive name"),
                false,
                true,
            )
        })
        .collect::<Vec<_>>();
    layout.set_rows(false);
    layout.set_items(
        (0..13)
            .map(|i| (format!("s{i}"), labels[i].widget()))
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
        .filter_map(|w| w.downcast::<gtk::Box>().ok())
        .flat_map(|row| row.children())
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
    assert!(
        previous.is_visible() && next.is_visible(),
        "arrows stay visible with all session cards"
    );
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
    assert_eq!(
        previous.parent(),
        next.parent(),
        "arrows share their own row"
    );
    let (nav_x, nav_y) = layout
        .navigation()
        .translate_coordinates(layout.widget(), 0, 0)
        .unwrap();
    let (actions_x, actions_bottom) = layout
        .actions()
        .translate_coordinates(layout.widget(), 0, layout.actions().allocated_height())
        .unwrap();
    let cards_right = layout
        .entries()
        .iter()
        .map(|(_, w)| {
            w.translate_coordinates(layout.widget(), w.allocated_width(), 0)
                .unwrap()
                .0
        })
        .max()
        .unwrap();
    let cards_bottom = layout
        .entries()
        .iter()
        .map(|(_, w)| {
            w.translate_coordinates(layout.widget(), 0, w.allocated_height())
                .unwrap()
                .1
        })
        .max()
        .unwrap();
    assert!(
        nav_x >= actions_x && nav_x >= cards_right,
        "arrows belong in the right control column"
    );
    assert!(
        nav_y >= actions_bottom,
        "arrows sit below the action buttons"
    );
    assert!(
        nav_y < cards_bottom,
        "arrows must not add a row below all cards"
    );
    layout.set_rows(false);
    let frame = glib::MainLoop::new(None, false);
    glib::timeout_add_local_once(Duration::from_millis(80), {
        let frame = frame.clone();
        move || frame.quit()
    });
    frame.run();
    assert_ne!(
        previous.parent(),
        next.parent(),
        "compact arrows sit on opposite sides"
    );
    assert!(!layout.navigation().is_visible());
    assert!(
        layout.widget().children()[0].allocated_height() <= 50,
        "compact header stays one short row: {}",
        layout.widget().children()[0].allocated_height()
    );
    for label in &labels {
        assert_eq!(label.text.ellipsize(), pango::EllipsizeMode::None);
        assert!(
            !label.text.layout().unwrap().is_ellipsized(),
            "compact names remain complete"
        );
    }
    assert_eq!(
        layout.navigation_buttons().len(),
        2,
        "boundary controls remain accessible after reparenting"
    );
    window.close();
}
