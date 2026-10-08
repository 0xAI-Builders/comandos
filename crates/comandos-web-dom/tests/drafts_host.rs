use comandos_web_dom::drafts::{AnchorMemory, DraftMemory, line_at};

#[test]
fn restore_writes_the_saved_text_into_the_composer_and_sends_nothing_to_the_pty() {
    let mut d = DraftMemory::new("tab:alpha");
    let mut composer = String::new();
    let state =
        r#"{"drafts":{"tab:alpha":{"text":"git commit -m \"wip","selStart":3,"selEnd":3}}}"#;
    let r = d.restore(state, &composer);
    assert_eq!(r.state, "restored");
    composer = r.text.unwrap_or_default();
    assert_eq!(composer, "git commit -m \"wip");
    assert_eq!(r.selection, Some((3, 3)));
    assert!(!d.can_send_to_terminal());
}

#[test]
fn changes_save_as_debounced_patches_the_last_one_wins_empty_removes() {
    let mut d = DraftMemory::new("k");
    d.changed("h", Some(1), Some(1));
    d.changed("ho", Some(2), Some(2));
    d.changed("hola", Some(4), Some(4));
    assert_eq!(
        d.flush_json(7).as_deref(),
        Some(r#"{"draftsPatch":{"k":{"text":"hola","selStart":4,"selEnd":4,"updatedAt":7}}}"#)
    );
    d.changed("", Some(0), Some(0));
    assert_eq!(
        d.flush_json(8).as_deref(),
        Some(r#"{"draftsPatch":{"k":null}}"#)
    );
}

#[test]
fn reading_anchor_is_found_by_its_line_text_or_reported_missing() {
    let history = "uno\ndos\ntres\ncuatro\n";
    let a = AnchorMemory::new("k");
    assert_eq!(
        a.find_json(r#"{"readingAnchors":{"k":{"text":"tres"}}}"#, history)
            .state,
        "found"
    );
    assert_eq!(
        a.find_json(r#"{"readingAnchors":{"k":{"text":"borrado"}}}"#, history)
            .state,
        "missing"
    );
    assert_eq!(a.find_json("{}", history).state, "none");
}

#[test]
fn line_at_matches_the_original_top_line_rule() {
    assert_eq!(line_at("uno\ndos\ntres\n", 5), "dos");
    assert_eq!(line_at("uno\ndos", 0), "uno");
}

#[test]
fn line_at_uses_javascript_utf16_offsets_after_emoji() {
    assert_eq!(line_at("🙂uno\ndos\ntres", 7), "dos");
}
