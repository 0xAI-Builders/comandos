use comandos_web::components::chain_builder::{SaveGate, valid_command};

#[test]
fn pending_save_survives_replacement_without_closing_or_running_it() {
    let mut gate = SaveGate::default();
    gate.open();
    let ticket = gate.begin();
    assert!(ticket.is_some());
    gate.close();
    gate.open();
    assert!(gate.begin().is_none());
    assert!(!gate.finish(ticket.unwrap_or_default()));
    let next = gate.begin();
    assert!(next.is_some());
    assert!(gate.finish(next.unwrap_or_default()));
    gate.close();
    assert!(gate.begin().is_none());
}

#[test]
fn command_validation_matches_javascript_whitespace_and_control_policy() {
    for text in ["", " \u{feff}\u{a0}", "a\n", "a\0", "a\u{7f}"] {
        assert!(!valid_command(text), "{text:?}");
    }
    for text in ["echo hi  ", "\u{85}", "echo 🦀", "\u{feff}echo hi"] {
        assert!(valid_command(text), "{text:?}");
    }
}
