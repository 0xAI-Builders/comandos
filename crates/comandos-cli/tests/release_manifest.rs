use comandos_cli::install::{
    manifest::{STATE_PROTOCOL, release_protocol},
    release::{WebSource, stage_release},
};
use std::{fs, os::unix::fs::PermissionsExt};
#[test]
fn staged_release_has_protocol_manifest_0644_and_reuses_it() {
    let root = std::env::temp_dir().join(format!("cmd-manifest-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let exe = root.join("fixture-bin");
    fs::write(&exe, b"S2-private-binary").unwrap();
    let r = stage_release(&root, &exe, &WebSource::None).unwrap();
    let dir = r.path.parent().unwrap();
    assert_eq!(release_protocol(dir), STATE_PROTOCOL);
    assert_eq!(
        fs::metadata(dir.join("manifest.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
    let again = stage_release(&root, &exe, &WebSource::None).unwrap();
    assert_eq!(r.id, again.id);
    fs::write(dir.join("manifest.json"), b"invalid").unwrap();
    assert!(stage_release(&root, &exe, &WebSource::None).is_err());
    fs::remove_dir_all(&root).unwrap();
}
#[test]
fn manifest_is_committed_before_binary_rename() {
    use sha2::{Digest, Sha256};
    let root = std::env::temp_dir().join(format!("cmd-manifest-order-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let bytes = b"private-order-fixture";
    let exe = root.join("source");
    fs::write(&exe, bytes).unwrap();
    let id: String = Sha256::digest(bytes)
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect();
    let dir = root.join(".local/share/comandos/releases").join(id);
    fs::create_dir_all(dir.join(format!("comandos.tmp.{}", std::process::id()))).unwrap();
    assert!(stage_release(&root, &exe, &WebSource::None).is_err());
    assert_eq!(release_protocol(&dir), STATE_PROTOCOL);
    assert!(!dir.join("comandos").exists());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn staging_never_labels_an_old_existing_binary_as_protocol_two() {
    use sha2::{Digest, Sha256};
    let root = std::env::temp_dir().join(format!("cmd-manifest-old-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let bytes = b"existing-old-fixture";
    fs::create_dir_all(&root).unwrap();
    let exe = root.join("source");
    fs::write(&exe, bytes).unwrap();
    let id: String = Sha256::digest(bytes)
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect();
    let dir = root.join(".local/share/comandos/releases").join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("comandos"), bytes).unwrap();
    assert!(stage_release(&root, &exe, &WebSource::None).is_err());
    assert!(!dir.join("manifest.json").exists());
    assert_eq!(fs::read(dir.join("comandos")).unwrap(), bytes);
    fs::remove_dir_all(root).unwrap();
}
