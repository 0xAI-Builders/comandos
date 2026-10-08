#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[allow(dead_code, clippy::disallowed_methods)]
#[path = "support/tmux.rs"]
mod tmux;
#[path = "support/wizard_pty_actor.rs"]
mod wizard_pty_actor;
use comandos_app::{config::RunMode, ui::extensions::wizard_target_when};
use std::os::unix::fs::DirBuilderExt;

#[test]
fn wizard_resolves_real_active_and_explicit_panes_with_literal_cwd() {
    let f = tmux::TestTmux::for_mode(RunMode::Sandbox)
        .expect("private tmux required for wizard regression");
    let root = f.config.sandbox_root().unwrap();
    let first_cwd = root.join("first path | 雪");
    let second_cwd = root.join("second path | literal '");
    for p in [&first_cwd, &second_cwd] {
        std::fs::DirBuilder::new().mode(0o700).create(p).unwrap();
    }
    let first = f.tmux.raw(&[
        "new-session",
        "-d",
        "-s",
        "local",
        "-c",
        first_cwd.to_str().unwrap(),
        "-P",
        "-F",
        "#{pane_id}",
        "/bin/sh",
    ]);
    assert!(first.status.success(), "{first:?}");
    let first = String::from_utf8(first.stdout).unwrap().trim().to_string();
    let second = f.tmux.raw(&[
        "split-window",
        "-h",
        "-t",
        "=local:",
        "-c",
        second_cwd.to_str().unwrap(),
        "-P",
        "-F",
        "#{pane_id}",
        "/bin/sh",
    ]);
    assert!(second.status.success(), "{second:?}");
    let second = String::from_utf8(second.stdout).unwrap().trim().to_string();
    assert_eq!(
        wizard_target_when(&f.ctl, "local", "", || true).unwrap(),
        ("local".into(), second, second_cwd.to_str().unwrap().into())
    );
    assert_eq!(
        wizard_target_when(&f.ctl, "local", &first, || true).unwrap(),
        ("local".into(), first, first_cwd.to_str().unwrap().into())
    );
    assert!(f.ctl.read(&["has-session", "-t", "=local"]).unwrap().ok());
}

#[test]
fn sandbox_attach_preserves_unicode_without_an_inherited_locale() {
    let f = tmux::TestTmux::for_mode(RunMode::Sandbox)
        .expect("private tmux required for UTF-8 regression");
    f.tmux.new_session("local", 80, 24);
    let attach = f.ctl.attach_argv("local");
    let data = wizard_pty_actor::attach_and_read(&attach[2], f.config.home(), || {
        let sent = f.tmux.raw(&[
            "send-keys",
            "-t",
            "=local:",
            "printf '\\351\\233\\252\\n'",
            "Enter",
        ]);
        assert!(sent.status.success(), "{sent:?}");
    });
    assert!(
        data.windows("雪".len()).any(|w| w == "雪".as_bytes()),
        "{data:?}"
    );
}
