#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
#[test]
fn mixed_fixture_exports_valid_geometry_only_into_a_new_private_root() {
    use comandos_core::workspace::snapshot::{Snapshot, check_snapshot};
    let root = std::env::temp_dir().join(format!("comandos-fixture-{}", std::process::id()));
    assert!(!root.exists());
    comandos_app::fixture::export(&root).unwrap();
    let value = comandos_app::fixture::mixed(&root.join("run/comandos-app-sbx/home"));
    assert_eq!(check_snapshot(&value["snapshot"]), Snapshot::Valid);
    assert!(comandos_core::workspace::validate_document(&value["workspace"]).is_ok());
    assert!(comandos_app::fixture::export(&root).is_err());
    assert!(
        root.join("run/comandos-app-sbx/hooks/app-sessions-v2.json")
            .is_file()
    );
    assert!(
        value["snapshot"]["sessions"]["term-fixture-mixed"]["windows"][0]["panes"][0]["cwd"]
            .as_str()
            .unwrap()
            .starts_with(root.to_str().unwrap())
    );
    std::fs::remove_dir_all(root).unwrap();
}
