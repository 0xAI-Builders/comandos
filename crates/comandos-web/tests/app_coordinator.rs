use comandos_web::components::app_coordinator;
#[test]
fn viewport_split_uses_device_orientation_and_exact_boundaries() {
    assert!(!app_coordinator::should_split_layout(747.0, 600.0, true));
    assert!(app_coordinator::should_split_layout(748.0, 600.0, true));
    assert!(!app_coordinator::should_split_layout(900.0, 1000.0, true));
    assert!(!app_coordinator::should_split_layout(899.0, 600.0, false));
    assert!(app_coordinator::should_split_layout(900.0, 1200.0, false));
    assert_eq!(
        app_coordinator::visible_height(&[800.0, f64::NAN, 570.4, 0.0]),
        570.0
    );
    assert_eq!(app_coordinator::visible_height(&[]), 0.0);
    assert_eq!(app_coordinator::split_bounds(600.0), (300.0, 300.0));
    assert_eq!(app_coordinator::split_bounds(1200.0), (300.0, 760.0));
}
#[test]
fn pane_identity_and_model_labels_match_source() {
    assert_eq!(app_coordinator::row_key("s", "%2"), "s|%2");
    assert_eq!(app_coordinator::row_key("s", ""), "s");
    assert_eq!(
        app_coordinator::tab_model_short("claude-sonnet-20250929[1m]"),
        "sonnet"
    );
    assert_eq!(app_coordinator::tab_model_short("claude-opus-5"), "opus");
    assert_eq!(app_coordinator::tab_model_short("x-2025bad1"), "x-2025bad1");
    assert_eq!(
        app_coordinator::tab_model_short("x[abc\nlast"),
        "x[abc\nlast"
    );
    assert_eq!(
        app_coordinator::tab_model_short("x[abc\nlast[1m]"),
        "x[abc\nlast"
    );
    assert_eq!(
        app_coordinator::tab_model_short("claude-x-claude-5"),
        "x-claude"
    );
    #[cfg(not(target_arch = "wasm32"))]
    for mount in [
        app_coordinator::mount,
        app_coordinator::mount_app,
        app_coordinator::mount_identity,
        app_coordinator::mount_sidebar,
        app_coordinator::mount_render,
    ] {
        assert!(mount().is_ok());
    }
}
