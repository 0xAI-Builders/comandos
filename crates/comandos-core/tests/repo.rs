//! Regla pura del checkout: sin disco.
use comandos_core::repo::repo_root_from;
use std::path::{Path, PathBuf};

#[test]
fn override_wins_unless_empty() {
    let target = Path::new("/checkout/dash/index.html");
    assert_eq!(
        repo_root_from(Some(target), Some("/otro")),
        Some(PathBuf::from("/otro"))
    );
    assert_eq!(
        repo_root_from(Some(target), Some("")),
        Some(PathBuf::from("/checkout"))
    );
    assert_eq!(
        repo_root_from(Some(target), None),
        Some(PathBuf::from("/checkout"))
    );
    assert_eq!(repo_root_from(None, Some("/otro")), Some("/otro".into()));
}

#[test]
fn without_target_or_depth_there_is_no_checkout() {
    assert_eq!(repo_root_from(None, None), None);
    assert_eq!(repo_root_from(None, Some("")), None);
    assert_eq!(repo_root_from(Some(Path::new("/index.html")), None), None);
}
