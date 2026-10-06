#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::reader::reader_layout_state;
#[path = "support/t18_oracle.rs"]
mod oracle;
#[test]
fn reader_with_terminal_keeps_both_regions() {
    for (open, terminal, terms) in [
        (false, false, true),
        (false, true, true),
        (true, false, false),
        (true, true, true),
    ] {
        assert_eq!(
            reader_layout_state(open, terminal),
            serde_json::json!({"terminals":terms,"reader":open})
        );
    }
}
#[test]
fn owned_modals_keep_original_sizes_and_must_answer_close_policy() {
    use comandos_app::ui::modals::{ModalKind, close_allowed, modal_size};
    for (width, height) in [
        (300, 300),
        (760, 570),
        (1280, 720),
        (1280, 1200),
        (4000, 3000),
    ] {
        for (kind, name) in [
            (ModalKind::Url, "url"),
            (ModalKind::Chains, "chains"),
            (ModalKind::Analytics, "analytics"),
        ] {
            let (w, h) = modal_size(kind, width, height);
            assert_eq!(
                serde_json::json!([w, h]),
                oracle::original(
                    serde_json::json!({"op":"modal-size","kind":name,"width":width,"height":height})
                )
            );
        }
    }
    for dismissable in [false, true] {
        for (escape, backdrop) in [(false, true), (true, false), (false, false)] {
            let original = oracle::original(
                serde_json::json!({"op":"modal-close","dismissable":dismissable,"escape":escape,"backdrop":backdrop}),
            );
            assert_eq!(
                original["closed"],
                close_allowed(dismissable, escape, backdrop)
            );
            if backdrop {
                assert_eq!(original["consumed"], true);
            }
        }
    }
}
