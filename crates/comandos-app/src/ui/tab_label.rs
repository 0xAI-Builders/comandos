//! Renameable tab label with favorite and close affordances.
use gtk::prelude::*;

pub fn tab_label(label: &str, favorite: bool) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.style_context().add_class("strip-tab-label");
    let fav = gtk::Button::with_label(if favorite { "★" } else { "☆" });
    fav.style_context().add_class("tabfav");
    let name = gtk::Label::new(Some(label));
    name.style_context().add_class("tab-name");
    name.set_ellipsize(pango::EllipsizeMode::End);
    let close = gtk::Button::with_label("×");
    close.style_context().add_class("tabclose");
    row.pack_start(&fav, false, false, 0);
    row.pack_start(&name, false, false, 0);
    row.pack_start(&close, false, false, 0);
    row
}
