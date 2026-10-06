//! Must-answer confirmation dialog: only explicit Yes/No produces a result.
use gtk::prelude::*;

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
    let dialog = gtk::Dialog::with_buttons(
        Some(primary),
        parent,
        gtk::DialogFlags::MODAL,
        &[
            (no_label, gtk::ResponseType::No),
            (yes_label, gtk::ResponseType::Yes),
        ],
    );
    dialog.set_deletable(false);
    dialog.set_resizable(false);
    let area = dialog.content_area();
    let label = gtk::Label::new(Some(secondary));
    label.set_wrap(true);
    area.pack_start(&label, true, true, 12);
    dialog.show_all();
    loop {
        match dialog.run() {
            gtk::ResponseType::Yes => {
                dialog.close();
                return ConfirmAnswer::Yes;
            }
            gtk::ResponseType::No => {
                dialog.close();
                return ConfirmAnswer::No;
            }
            _ => {}
        }
    }
}
