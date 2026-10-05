#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
mod support;
use comandos_app::config::RunMode;
use comandos_app::guard::GuardError;
use support::tmux::TestTmux;

#[test]
fn shadow_guard_refuses_hooks_writes() {
    let Some(f) = TestTmux::for_mode(RunMode::Shadow) else {
        return;
    };
    let target = f.config.hooks_dir().join("app-tabs.json");
    assert!(matches!(
        f.guard.write_atomic(&target, b"[]", "app-tabs."),
        Err(GuardError::Shadow(_))
    ));
    assert!(matches!(
        f.guard.remove_file(&target),
        Err(GuardError::Shadow(_))
    ));
    assert!(!target.exists());
    // Lo único que la sombra escribe: su candado y su volcado de diseño.
    assert!(
        f.guard
            .write_atomic(&f.config.layout_dump_path(), b"{}", "layout.")
            .is_ok()
    );
}

#[test]
fn guard_writes_atomically_inside_roots() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    let path = f.config.hooks_dir().join("sub/app-tabs.json");
    f.guard
        .create_dir_all(path.parent().unwrap(), 0o700)
        .unwrap();
    f.guard
        .write_atomic(&path, "[\"ñandú\"]".as_bytes(), "app-tabs.")
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[\"ñandú\"]");
    let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "ningún temporal queda atrás");
}

#[test]
fn guard_refuses_dotdot_and_symlink_escape() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    let hooks = f.config.hooks_dir().to_path_buf();
    std::fs::create_dir_all(&hooks).unwrap();
    let outside = f.config.home().join("fuera");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, hooks.join("enlace")).unwrap();
    assert!(matches!(
        f.guard
            .write_atomic(&hooks.join("../../../home/x.json"), b"1", "x."),
        Err(GuardError::Escape(_) | GuardError::Outside(_))
    ));
    assert!(matches!(
        f.guard
            .write_atomic(&hooks.join("enlace/x.json"), b"1", "x."),
        Err(GuardError::Escape(_))
    ));
    assert!(!outside.join("x.json").exists());
    assert!(matches!(
        f.guard.write_atomic(
            &f.config.home().join(".claude/hooks/app-tabs.json"),
            b"1",
            "x."
        ),
        Err(GuardError::Outside(_))
    ));
}

#[test]
fn guard_rechecks_at_write_time() {
    // TOCTOU: la ruta era válida al construir la guarda; después alguien cambia
    // un directorio intermedio por un enlace que sale. La escritura lo detecta.
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    let dir = f.config.hooks_dir().join("cambia");
    f.guard.create_dir_all(&dir, 0o700).unwrap();
    f.guard
        .write_atomic(&dir.join("a.json"), b"1", "a.")
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let outside = f.config.home().join("robo");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, &dir).unwrap();
    assert!(matches!(
        f.guard.write_atomic(&dir.join("a.json"), b"2", "a."),
        Err(GuardError::Escape(_))
    ));
    assert!(!outside.join("a.json").exists());
}

#[test]
fn temporary_prefix_cannot_escape_the_parent() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let path = f.config.hooks_dir().join("app-tabs.json");
    for prefix in ["../evil", "/tmp/evil", "x/y", ".", "..", ""] {
        assert!(matches!(
            f.guard.write_atomic(&path, b"x", prefix),
            Err(GuardError::Escape(_))
        ));
    }
}

#[test]
fn live_guard_has_a_closed_file_list() {
    let f = TestTmux::for_mode(RunMode::Live).unwrap();
    let allowed = f.config.hooks_dir().join("app-tabs.json");
    f.guard.write_atomic(&allowed, b"[]", "tabs.").unwrap();
    for path in [
        f.config.hooks_dir().join("unrelated"),
        f.config.runtime_dir().join("other-service.sock"),
    ] {
        assert!(matches!(
            f.guard.write_atomic(&path, b"x", "x."),
            Err(GuardError::Outside(_))
        ));
    }
}

#[test]
fn final_symlink_is_never_followed() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let outside = f.config.home().join("outside");
    std::fs::write(&outside, "original").unwrap();
    let path = f.config.hooks_dir().join("app-tabs.json");
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(f.guard.append_if_exists(&path, b"x").is_err());
    assert!(f.guard.open_lock(&path).is_err());
    f.guard
        .write_atomic(&path, b"replacement", "tabs.")
        .unwrap();
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "original");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "replacement");
}
