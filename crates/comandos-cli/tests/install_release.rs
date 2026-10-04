// crates/comandos-cli/tests/install_release.rs
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn comandos() -> &'static str {
    env!("CARGO_BIN_EXE_comandos")
}
fn home(tag: &str) -> std::path::PathBuf {
    let h = std::env::temp_dir().join(format!("cmd-rel-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&h);
    fs::create_dir_all(&h).unwrap();
    h
}
fn run(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(comandos())
        .arg("install")
        .arg("--home")
        .arg(home)
        .args(args)
        .output()
        .unwrap()
}
fn sha12(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(fs::read(path).unwrap());
    format!("{:x}", h.finalize())[..12].to_string()
}

#[test]
fn stage_installs_a_release_and_links_bin_comandos_to_it() {
    let h = home("stage");
    assert!(run(&h, &["--stage"]).status.success());
    let bin = h.join(".local/share/comandos/bin/comandos");
    let target = fs::read_link(&bin).unwrap();
    let id = sha12(Path::new(comandos()));
    assert_eq!(target, Path::new("../releases").join(&id).join("comandos"));
    let real = h
        .join(".local/share/comandos/releases")
        .join(&id)
        .join("comandos");
    assert!(real.metadata().unwrap().permissions().mode() & 0o111 != 0);
    // Idempotente: el mismo binario no crea otra release.
    assert!(run(&h, &["--stage"]).status.success());
    assert_eq!(
        fs::read_dir(h.join(".local/share/comandos/releases"))
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_type().unwrap().is_dir())
            .count(),
        1
    );
}

#[test]
fn a_fase_1_regular_file_becomes_the_previous_release_and_rollback_restores_it() {
    let h = home("legacy");
    let bin = h.join(".local/share/comandos/bin/comandos");
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    fs::write(&bin, b"#!/bin/sh\necho viejo\n").unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    let old_id = sha12(&bin);
    assert!(run(&h, &["--stage"]).status.success());
    assert!(bin.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(
        fs::read_to_string(h.join(".local/share/comandos/releases/previous"))
            .unwrap()
            .trim(),
        old_id
    );
    let out = run(&h, &["--rollback-release"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_link(&bin).unwrap(),
        Path::new("../releases").join(&old_id).join("comandos")
    );
    assert_eq!(
        fs::read_to_string(h.join(".local/share/comandos/releases/previous"))
            .unwrap()
            .trim(),
        sha12(Path::new(comandos()))
    );
}

#[test]
fn releases_lists_current_first_and_prunes_to_five() {
    let h = home("prune");
    let rel = h.join(".local/share/comandos/releases");
    for i in 0..7 {
        let d = rel.join(format!("{i:012x}"));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("comandos"), [i as u8]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    assert!(run(&h, &["--stage"]).status.success());
    let dirs = fs::read_dir(&rel)
        .unwrap()
        .filter(|e| e.as_ref().unwrap().file_type().unwrap().is_dir())
        .count();
    assert_eq!(dirs, 5, "actual + 4 más recientes");
    let out = run(&h, &["--releases"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.lines()
            .next()
            .unwrap()
            .starts_with(&format!("* {}", sha12(Path::new(comandos()))))
    );
}

#[test]
fn rollback_without_previous_fails_cleanly() {
    let h = home("noprev");
    assert!(run(&h, &["--stage"]).status.success());
    let out = run(&h, &["--rollback-release"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no hay release anterior"));
}
