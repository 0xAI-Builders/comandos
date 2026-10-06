#![allow(clippy::unwrap_used)]
use comandos_app_mac::{
    app::{Action, App},
    tabs_ops::TabsOps,
};
use comandos_desktop::Lang;
use serde_json::json;
fn ops() -> TabsOps {
    TabsOps::new(App::new("noche", Lang::Es))
}
#[test]
fn save_before_restore_finishes_keeps_unrestored_labels_and_order() {
    let mut o = ops();
    let ticket = o.ticket();
    o.begin_restore(vec![
        ("old".into(), json!("Old")),
        ("remaining".into(), json!("Rem")),
    ]);
    assert!(o.finish_boot(&ticket, "owned").is_empty());
    o.add_tab("old", "New", "old", false).unwrap();
    assert_eq!(o.save_value(), json!({"old":"New","remaining":"Rem"}));
    assert_eq!(
        o.save_value()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["old", "remaining"]
    );
}
#[test]
fn actions_during_restore_wait_and_cancelled_original_alias_never_replays() {
    let mut o = ops();
    let ticket = o.ticket();
    o.begin_restore(vec![
        ("ssh-host".into(), json!("Host")),
        ("project:2".into(), json!("P")),
    ]);
    o.finish_boot(&ticket, "owned");
    assert!(
        o.queue(Action::Open {
            session: "queued".into(),
            win: "claude".into(),
            label: None
        })
        .unwrap()
        .is_empty()
    );
    o.record_alias("ssh-host", "ssh-renamed");
    assert_eq!(o.current_session("ssh-host"), "ssh-renamed");
    assert!(o.cancel_restore("ssh-renamed"));
    assert!(o.restore_cancelled("ssh-host") && o.restore_cancelled("ssh-renamed"));
    assert_eq!(o.save_value(), json!({"project:2":"P"}));
    assert_eq!(o.finish_restore(&ticket).len(), 1);
    assert!(!o.restore_cancelled("ssh-host"));
}
#[test]
fn close_hub_is_noop_and_other_close_selects_last_tab() {
    let mut o = ops();
    let t = o.ticket();
    o.finish_boot(&t, "owned");
    o.add_tab("__hub__", "local", "local", true).unwrap();
    o.add_tab("first", "first", "first", false).unwrap();
    o.add_tab("last", "last", "last", false).unwrap();
    o.select_tab("first");
    let hub = o.tabs()[0].instance;
    assert!(o.close_model("__hub__", hub).is_none());
    let first = o.tabs()[1].instance;
    assert!(o.close_model("first", first + 100).is_none());
    assert!(o.close_model("first", first).is_some());
    assert_eq!(o.active_key(), Some("last"));
}
#[test]
fn reopened_key_cancels_old_session_operation_and_close_never_targets_replacement() {
    let mut o = ops();
    let old = o.start_operation("same");
    assert!(old.current());
    let fresh = o.start_operation("same");
    assert!(!old.current() && fresh.current());
    o.finish_operation("same", &old);
    assert!(fresh.current());
    o.cancel_operation("same");
    assert!(!fresh.current());
    let new = o.start_operation("same");
    assert!(new.current());
    o.shutdown();
    assert!(!new.current());
}

#[test]
fn closed_sheet_completion_cannot_finish_a_new_sheet_for_reopened_key() {
    let mut dialogs = comandos_app_mac::dialogs::DialogOwner::default();
    let old = comandos_app_mac::dialogs::TabScope {
        key: "own".into(),
        instance: 1,
    };
    let stale = dialogs.begin(old.clone()).unwrap();
    assert!(dialogs.cancel(&old));
    let new = comandos_app_mac::dialogs::TabScope {
        key: "own".into(),
        instance: 2,
    };
    let current = dialogs.begin(new).unwrap();
    assert!(!dialogs.finish(stale));
    assert!(dialogs.finish(current));
}

#[test]
fn unexpected_restore_error_unblocks_actions_without_discarding_saved_labels() {
    let mut ops = comandos_app_mac::tabs_ops::TabsOps::new(comandos_app_mac::app::App::new(
        "noche",
        comandos_desktop::Lang::En,
    ));
    let ticket = ops.ticket();
    ops.begin_restore(vec![("not-yet".into(), serde_json::json!("Keep"))]);
    ops.finish_boot(&ticket, "own");
    ops.queue(comandos_app_mac::app::Action::OpenBackground {
        session: "queued".into(),
        label: None,
    })
    .unwrap();
    let drained = ops.fail_restore(&ticket);
    assert_eq!(drained.len(), 1);
    assert_eq!(ops.save_value(), serde_json::json!({"not-yet":"Keep"}));
    assert!(ops.ready());
}
