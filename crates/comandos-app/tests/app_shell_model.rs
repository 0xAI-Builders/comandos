#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
use comandos_app::ui::webview::dashboard_uri;

#[test]
fn disconnected_dashboard_has_no_live_url() {
    assert!(dashboard_uri(None, "fixture-v1").is_none());
    let uri = dashboard_uri(Some("http://127.0.0.1:7311"), "fixture-v1").expect("fixture");
    assert!(uri.contains("app=1") && uri.contains("anwin=1") && uri.contains("fixture-v1"));
}

#[test]
fn web_terminal_uri_encodes_auth_and_keeps_absolute_asset_location() {
    let uri = comandos_app::ui::webview::terminal_uri(
        std::path::Path::new("/private/assets/term.html"),
        "term-one",
        "tok en&secret",
        "日本",
    );
    assert!(uri.starts_with(
        "file:///private/assets/term.html?auth=tok%20en%26secret&arg=term-one&theme="
    ));
}

#[test]
fn display_lock_contends_and_activation_targets_only_its_mode_class() {
    use std::os::unix::fs::DirBuilderExt;
    // Locking and injected activation need no display connection.
    let root = std::env::temp_dir().join(format!("comandos-instance-test-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root.join("home"))
        .unwrap();
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root.join("run"))
        .unwrap();
    let lookup = |key: &str| match key {
        "HOME" => Some(root.join("home").display().to_string()),
        "XDG_RUNTIME_DIR" => Some(root.join("run").display().to_string()),
        _ => None,
    };
    let cfg = comandos_app::config::parse_args(
        &[
            "--mode".into(),
            "sandbox".into(),
            "--tmux-socket".into(),
            "test-instance".into(),
        ],
        false,
        &lookup,
    )
    .unwrap();
    let guard = comandos_app::guard::WriteGuard::from_config(&cfg, ":fixture");
    let lock = comandos_app::ui::window::instance_lock(&cfg, &guard, ":fixture").unwrap();
    assert!(matches!(
        comandos_app::ui::window::instance_lock(&cfg, &guard, ":fixture"),
        Err(comandos_app::ui::window::InstanceError::Contended)
    ));
    assert!(comandos_app::ui::window::instance_lock(&cfg, &guard, ":other").is_ok());
    let called = std::cell::Cell::new(false);
    assert!(comandos_app::ui::window::activate_existing(&cfg, &|spec| {
        called.set(true);
        assert_eq!(spec.program, "wmctrl");
        assert_eq!(
            spec.args,
            vec![
                std::ffi::OsString::from("-x"),
                "-a".into(),
                "sandbox-app-rs.sandbox-app-rs".into()
            ]
        );
        Ok(comandos_app::proc::ProcOutput {
            code: Some(0),
            ..Default::default()
        })
    }));
    assert!(called.get());
    drop(lock);
    std::fs::remove_dir_all(root).unwrap();
}
