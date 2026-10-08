#![allow(clippy::unwrap_used, clippy::expect_used)]
use comandos_app_mac::app::{App, WindowSpec, parse_args};
use comandos_app_mac::tmux::argv_for;
use comandos_desktop::{Lang, parse_bridge};
use serde_json::json;
#[test]
fn sandbox_arguments_always_choose_explicit_private_tmux() {
    let cfg = parse_args(&[
        "--sandbox".into(),
        "/tmp/owned-m3-sandbox".into(),
        "--dash-url".into(),
        "http://127.0.0.1:7337".into(),
    ])
    .unwrap();
    assert_eq!(
        argv_for(&cfg.mode, &["list-sessions"]),
        vec![
            "tmux",
            "-S",
            "/tmp/owned-m3-sandbox/tmux-1000/default",
            "list-sessions"
        ]
    );
}
#[test]
fn live_defaults_match_the_original_without_executing_user_tmux() {
    let cfg = parse_args(&[]).unwrap();
    assert_eq!(cfg.dash_url, "http://127.0.0.1:4777");
    assert_eq!(
        argv_for(&cfg.mode, &["list-sessions"]),
        vec!["tmux", "list-sessions"]
    );
}
#[test]
fn invalid_configuration_never_constructs_a_runner() {
    for args in [
        vec!["--unknown"],
        vec!["--sandbox"],
        vec!["--dash-url", "http://example.org:7337"],
        vec!["--dash-url", "file:///owned"],
        vec!["--dump-dom"],
    ] {
        assert!(parse_args(&args.into_iter().map(str::to_string).collect::<Vec<_>>()).is_err());
    }
}
#[test]
fn window_geometry_matches_the_original_and_keeps_strip_above_terminal() {
    let w = WindowSpec::default();
    assert_eq!(w.size, (1280., 800.));
    assert_eq!(w.minimum, (760., 480.));
    assert_eq!(w.left_fraction, 0.52);
    assert_eq!(w.strip_height, 30.);
    assert_eq!(w.title, "ComandOS");
    assert!(w.dark_aqua);
}
#[test]
fn terminal_actions_wait_for_authenticated_boot_and_close_cancels_queue() {
    let mut a = App::new("noche", Lang::Es);
    let first = a.ticket();
    a.receive_bridge(parse_bridge(&json!({"session":"owned"})).unwrap())
        .unwrap();
    assert_eq!(a.pending_count(), 1);
    assert!(a.tabs().is_empty());
    a.shutdown();
    assert!(!first.current());
    assert!(a.finish_boot(&first, "private token").is_empty());
    assert_eq!(a.pending_count(), 0);
}
#[test]
fn replacement_boot_discards_stale_token_and_drains_only_current_owner() {
    let mut a = App::new("noche", Lang::Es);
    let old = a.ticket();
    a.receive_bridge(parse_bridge(&json!({"session":"owned"})).unwrap())
        .unwrap();
    let current = a.restart_boot();
    assert!(a.finish_boot(&old, "old token").is_empty());
    assert!(!a.ready());
    assert_eq!(a.finish_boot(&current, "private token").len(), 1);
    assert!(a.ready());
}
#[test]
fn valid_theme_updates_existing_and_future_tabs_without_invalid_override() {
    let mut a = App::new("noche", Lang::En);
    let ticket = a.ticket();
    a.finish_boot(&ticket, "private token");
    a.add_tab("a", "A", "a", false).unwrap();
    a.add_tab("b", "B", "b", false).unwrap();
    a.receive_bridge(parse_bridge(&json!({"theme":"neon"})).unwrap())
        .unwrap();
    assert_eq!(a.theme(), "neon");
    a.receive_bridge(parse_bridge(&json!({"theme":"unknown"})).unwrap())
        .unwrap();
    assert_eq!(a.theme(), "neon");
    assert!(a.theme_script().contains("window.postMessage"));
    assert!(a.theme_script().contains("neon"));
}
#[test]
fn rename_updates_only_the_captured_tab_and_keeps_active_selection() {
    let mut a = App::new("noche", Lang::En);
    let ticket = a.ticket();
    a.finish_boot(&ticket, "private token");
    a.add_tab("a", "A", "a", false).unwrap();
    a.add_tab("b", "B", "b", false).unwrap();
    a.select_tab("b");
    a.receive_bridge(
        parse_bridge(&json!({"type":"rename","session":"a","label":"  X  "})).unwrap(),
    )
    .unwrap();
    assert_eq!(a.tabs()[0].label, "X");
    assert_eq!(a.active_key(), Some("b"));
}
#[test]
fn tab_lazy_creation_and_shutdown_invalidate_per_instance_callbacks() {
    let mut a = App::new("noche", Lang::En);
    let ticket = a.ticket();
    a.finish_boot(&ticket, "private token");
    a.add_tab("a", "A", "a", false).unwrap();
    a.add_tab("b", "B", "b", false).unwrap();
    assert_eq!(a.loaded_count(), 0);
    a.select_tab("a");
    assert_eq!(a.loaded_count(), 1);
    a.select_tab("b");
    assert_eq!(a.loaded_count(), 2);
    a.select_tab("a");
    assert_eq!(a.loaded_count(), 2);
    a.shutdown();
    assert!(!ticket.current());
}
#[test]
fn a_delayed_initial_preference_never_overwrites_a_newer_theme_message() {
    let mut a = App::new("noche", Lang::Es);
    a.receive_bridge(parse_bridge(&json!({"theme":"neon"})).unwrap())
        .unwrap();
    a.initial_theme("dia");
    assert_eq!(a.theme(), "neon");
}
#[test]
fn state_cache_is_bounded_by_tabs_and_does_not_require_loaded_views() {
    let mut a = App::new("noche", Lang::Es);
    let t = a.ticket();
    a.finish_boot(&t, "owned");
    a.add_tab("a", "A", "a", false).unwrap();
    a.add_tab("hub", "local", "local", true).unwrap();
    a.update_states(&json!([{"session":"unknown","status":"working"},{"session":"a","status":"working"},{"session":"a","status":"done"},{"session":"hub","status":"dead"}]));
    assert_eq!(a.tabs()[0].dot_color, "#2EE59D");
    assert_eq!(a.tabs()[1].dot_color, comandos_desktop::DOT_IDLE);
    assert_eq!(a.loaded_count(), 0);
    assert_eq!(a.tabs().len(), 2);
}
#[test]
fn polling_theme_and_rename_never_steal_dashboard_focus() {
    let mut a = App::new("noche", Lang::Es);
    let t = a.ticket();
    a.finish_boot(&t, "owned");
    a.add_tab("a", "A", "a", false).unwrap();
    a.select_tab("a");
    let instance = a.tabs()[0].instance;
    assert_eq!(a.take_focus_request(), Some(instance));
    a.update_states(&json!([]));
    a.receive_bridge(parse_bridge(&json!({"theme":"neon"})).unwrap())
        .unwrap();
    a.receive_bridge(parse_bridge(&json!({"type":"rename","session":"a","label":"X"})).unwrap())
        .unwrap();
    assert_eq!(a.take_focus_request(), None);
    a.select_tab("a");
    assert_eq!(a.take_focus_request(), Some(instance));
}
#[test]
fn a_destroyed_instance_cannot_mint_or_rearm_valid_tickets() {
    let mut a = App::new("noche", Lang::Es);
    a.shutdown();
    assert!(!a.ticket().current());
    assert!(!a.restart_boot().current());
    assert!(!a.ready());
}
#[test]
fn boot_ticket_from_another_instance_is_refused_even_at_same_generation() {
    let mut a = App::new("noche", Lang::Es);
    let b = App::new("noche", Lang::Es);
    a.finish_boot(&b.ticket(), "wrong owner's token");
    assert!(!a.ready());
    assert!(a.token().is_empty());
}
