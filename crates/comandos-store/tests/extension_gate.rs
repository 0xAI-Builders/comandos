use comandos_store::extension_gate;
use serde_json::json;
use std::{fs, path::PathBuf};

struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "comandos-gate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn defaults_on_and_saved_exclusions_are_scoped_to_exact_pane_identity() {
    let home = Home::new();
    let a = "/tmp/tmux/private|10|100|$0|%0|11";
    let b = "/tmp/tmux/private|10|100|$0|%1|12";
    assert!(extension_gate::enabled(&home.0, a, "mail").unwrap());
    extension_gate::save(&home.0, a, &json!({"mail":false,"search":true})).unwrap();
    assert!(!extension_gate::enabled(&home.0, a, "mail").unwrap());
    assert!(extension_gate::enabled(&home.0, b, "mail").unwrap());
    assert!(extension_gate::enabled(&home.0, a, "new-server").unwrap());
    assert!(extension_gate::enabled(&home.0, "/tmp/tmux/private|10|200|$0|%0|11", "mail").unwrap());
    extension_gate::save(&home.0, a, &json!({"mail":true})).unwrap();
    assert!(extension_gate::enabled(&home.0, a, "mail").unwrap());
}

#[test]
fn invalid_selection_and_corrupt_policy_do_not_enable_a_denied_server() {
    let home = Home::new();
    let identity = "socket|1|2|$0|%0|3";
    extension_gate::save(&home.0, identity, &json!({"mail":false})).unwrap();
    assert!(extension_gate::save(&home.0, identity, &json!({"mail":"false"})).is_err());
    assert!(!extension_gate::enabled(&home.0, identity, "mail").unwrap());
    let file = fs::read_dir(home.0.join(".config/comandos/extensions/session-gates"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(file, "{").unwrap();
    assert!(extension_gate::enabled(&home.0, identity, "mail").is_err());
}

#[cfg(unix)]
#[test]
fn policy_is_private_and_refuses_a_symlink_directory() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let home = Home::new();
    let identity = "socket|1|2|$0|%0|3";
    extension_gate::save(&home.0, identity, &json!({"mail":false})).unwrap();
    let dir = home.0.join(".config/comandos/extensions/session-gates");
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let file = fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::remove_dir_all(&dir).unwrap();
    let other = home.0.join("outside");
    fs::create_dir(&other).unwrap();
    symlink(&other, &dir).unwrap();
    assert!(extension_gate::save(&home.0, identity, &json!({"mail":false})).is_err());
    assert!(extension_gate::enabled(&home.0, identity, "mail").is_err());
    assert!(fs::read_dir(other).unwrap().next().is_none());
}

#[test]
fn initialization_never_replaces_an_existing_exclusion_or_newer_selection() {
    let home = Home::new();
    let identity = "socket|1|2|$0|%0|3";
    extension_gate::initialize(&home.0, identity, &json!({"mail":false})).unwrap();
    assert!(!extension_gate::enabled(&home.0, identity, "mail").unwrap());
    extension_gate::initialize(&home.0, identity, &json!({"mail":true})).unwrap();
    assert!(!extension_gate::enabled(&home.0, identity, "mail").unwrap());
    extension_gate::save(&home.0, identity, &json!({"mail":true})).unwrap();
    extension_gate::initialize(&home.0, identity, &json!({"mail":false})).unwrap();
    assert!(extension_gate::enabled(&home.0, identity, "mail").unwrap());
    assert_eq!(
        fs::read_dir(home.0.join(".config/comandos/extensions/session-gates"))
            .unwrap()
            .count(),
        1
    );
}
