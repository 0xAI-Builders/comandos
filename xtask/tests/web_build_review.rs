use std::{fs, path::PathBuf};
use xtask::web_build::{swap_into_place, validate_output};

fn owned(root: &std::path::Path, name: &str) -> PathBuf {
    let out = root.join(name);
    fs::create_dir_all(out.join("0123456789ab")).unwrap();
    fs::write(out.join("0123456789ab/module.js"), "owned fixture").unwrap();
    let mut manifest = xtask::web_build::Manifest::default();
    manifest
        .files
        .insert("module.js".into(), "0123456789ab/module.js".into());
    xtask::web_build::assemble(
        &root.join("absent"),
        &out,
        xtask::web_build::Manifest::default(),
        manifest,
    )
    .unwrap();
    assert!(validate_output(&out).is_ok());
    out
}

#[test]
fn an_ownership_receipt_covers_exact_unchanged_regular_files() {
    let root = scratch("owned");
    let out = owned(&root, "out");
    let file = out.join("0123456789ab/module.js");
    fs::write(&file, "modified later").unwrap();
    assert!(validate_output(&out).is_err());
    fs::write(&file, "owned fixture").unwrap();
    assert!(validate_output(&out).is_ok());
    fs::write(out.join("0123456789ab/valuable.txt"), "foreign data").unwrap();
    let next = owned(&root, "next");
    assert!(swap_into_place(&next, &out, &root.join("trash")).is_err());
    assert_eq!(
        fs::read_to_string(out.join("0123456789ab/valuable.txt")).unwrap(),
        "foreign data"
    );
    assert!(validate_output(&next).is_ok());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn valid_staging_cannot_authorize_deleting_preexisting_trash() {
    let root = scratch("owned-trash");
    let out = owned(&root, "out");
    let next = owned(&root, "next");
    let trash = root.join("trash");
    fs::create_dir(&trash).unwrap();
    fs::write(trash.join("valuable.txt"), "foreign trash").unwrap();
    assert!(swap_into_place(&next, &out, &trash).is_err());
    assert_eq!(
        fs::read_to_string(trash.join("valuable.txt")).unwrap(),
        "foreign trash"
    );
    assert!(validate_output(&out).is_ok());
    assert!(validate_output(&next).is_ok());
    fs::remove_dir_all(root).unwrap();
}

fn scratch(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("web-output-review-{}-{name}", std::process::id()));
    fs::create_dir(&root).unwrap();
    root
}

#[test]
fn a_hexadecimal_child_does_not_authorize_deleting_foreign_data() {
    let root = scratch("hex-only");
    let out = root.join("foreign");
    fs::create_dir_all(out.join("0123456789ab")).unwrap();
    let canary = out.join("0123456789ab/valuable.txt");
    fs::write(&canary, "foreign data").unwrap();
    assert!(validate_output(&out).is_err());
    assert_eq!(fs::read_to_string(&canary).unwrap(), "foreign data");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn swap_rechecks_ownership_and_never_removes_foreign_output_or_trash() {
    let root = scratch("swap");
    let out = root.join("foreign");
    fs::create_dir_all(out.join("0123456789ab")).unwrap();
    let canary = out.join("0123456789ab/valuable.txt");
    fs::write(&canary, "foreign data").unwrap();
    let next = root.join("next");
    fs::create_dir(&next).unwrap();
    fs::write(next.join("manifest.json"), "{\"files\":{}}").unwrap();
    let trash = root.join("trash");
    fs::create_dir(&trash).unwrap();
    fs::write(trash.join("valuable.txt"), "foreign trash").unwrap();
    assert!(swap_into_place(&next, &out, &trash).is_err());
    assert_eq!(fs::read_to_string(canary).unwrap(), "foreign data");
    assert_eq!(
        fs::read_to_string(trash.join("valuable.txt")).unwrap(),
        "foreign trash"
    );
    assert!(next.join("manifest.json").exists());
    fs::remove_dir_all(root).unwrap();
}
