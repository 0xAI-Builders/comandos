use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

fn temp_home() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home =
        std::env::temp_dir().join(format!("comandos-install-{}-{nanos}", std::process::id()));
    fs::create_dir_all(home.join(".local/bin")).unwrap();
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
    home
}

fn install(home: &Path, args: &[&str]) -> ExitStatus {
    Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args(["install", "--home", home.to_str().unwrap()])
        .args(args)
        // Herméticas: sin `web/` de ningún entorno.
        .env_remove("COMANDOS_WEB_SOURCE")
        .status()
        .unwrap()
}

fn staged(home: &Path) -> PathBuf {
    home.join(".local/share/comandos/bin/comandos")
}

fn record(home: &Path, name: &str) -> String {
    fs::read_to_string(home.join(format!(".local/share/comandos/rollback/{name}.target"))).unwrap()
}

#[test]
fn link_and_rollback_round_trip() {
    let home = temp_home();
    fs::write(home.join("old-target"), "#!/bin/sh\necho old\n").unwrap();
    symlink(
        home.join("old-target"),
        home.join(".local/bin/cc-extensions"),
    )
    .unwrap();
    assert!(install(&home, &["--stage"]).success());
    assert!(staged(&home).exists());
    assert!(install(&home, &["--link", "cc-extensions"]).success());
    assert_eq!(
        fs::read_link(home.join(".local/bin/cc-extensions")).unwrap(),
        staged(&home)
    );
    assert_eq!(
        record(&home, "cc-extensions").trim(),
        format!("LINK:{}", home.join("old-target").display())
    );
    assert!(install(&home, &["--rollback", "cc-extensions"]).success());
    assert_eq!(
        fs::read_link(home.join(".local/bin/cc-extensions")).unwrap(),
        home.join("old-target")
    );
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn staged_binary_is_executable() {
    let home = temp_home();
    assert!(install(&home, &["--stage"]).success());
    assert_ne!(
        fs::metadata(staged(&home)).unwrap().permissions().mode() & 0o111,
        0
    );
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn regular_file_is_backed_up_and_restored() {
    let home = temp_home();
    let hook = home.join(".claude/hooks/cc-notify.sh");
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::write(&hook, b"#!/bin/sh\necho real\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(install(&home, &["--stage"]).success());
    assert!(install(&home, &["--link", "cc-notify.sh"]).success());
    // Los hooks viven en ~/.claude/hooks y el original queda respaldado.
    assert_eq!(fs::read_link(&hook).unwrap(), staged(&home));
    let orig = home.join(".local/share/comandos/rollback/cc-notify.sh.orig");
    assert!(orig.exists());
    assert_eq!(
        record(&home, "cc-notify.sh").trim(),
        format!("FILE:{}", orig.display())
    );
    assert!(install(&home, &["--rollback", "cc-notify.sh"]).success());
    assert_eq!(fs::read(&hook).unwrap(), b"#!/bin/sh\necho real\n");
    assert_eq!(
        fs::metadata(&hook).unwrap().permissions().mode() & 0o777,
        0o750
    );
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn absent_path_rollback_removes_symlink() {
    let home = temp_home();
    assert!(install(&home, &["--stage"]).success());
    assert!(install(&home, &["--link", "cc-new"]).success());
    assert_eq!(record(&home, "cc-new").trim(), "ABSENT");
    assert!(install(&home, &["--rollback", "cc-new"]).success());
    assert!(fs::symlink_metadata(home.join(".local/bin/cc-new")).is_err());
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn second_link_keeps_original_record() {
    let home = temp_home();
    fs::write(home.join("old-target"), "x").unwrap();
    symlink(home.join("old-target"), home.join(".local/bin/cc-x")).unwrap();
    assert!(install(&home, &["--stage"]).success());
    assert!(install(&home, &["--link", "cc-x"]).success());
    let first = record(&home, "cc-x");
    assert!(install(&home, &["--link", "cc-x"]).success());
    assert_eq!(record(&home, "cc-x"), first);
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn invalid_name_is_rejected() {
    let home = temp_home();
    assert!(install(&home, &["--stage"]).success());
    assert!(!install(&home, &["--link", "a/b"]).success());
    assert!(!install(&home, &["--link", ".."]).success());
    assert!(!install(&home, &["--stage", "--link", "x"]).success());
    assert!(!home.join(".local/share/comandos/rollback/a").exists());
    fs::remove_dir_all(&home).unwrap();
}

mod app {
    use super::*;
    use std::{collections::BTreeMap, os::unix::fs::MetadataExt, process::Output};

    struct Fixture {
        root: PathBuf,
        home: PathBuf,
        candidate: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = temp_home();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            let home = root.join("home with ' quote");
            for dir in [
                "home with ' quote",
                "config",
                "data",
                "state",
                "cache",
                "runtime",
                "tmp",
                "path",
            ] {
                let p = root.join(dir);
                fs::create_dir_all(&p).unwrap();
                fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
            }
            fs::create_dir_all(home.join(".local/bin")).unwrap();
            let candidate = root.join("app candidate's binary");
            fs::write(&candidate, b"#!/bin/sh\nexit 91\n").unwrap();
            fs::set_permissions(&candidate, fs::Permissions::from_mode(0o750)).unwrap();
            Self {
                root,
                home,
                candidate,
            }
        }

        fn run(&self, args: &[&str]) -> Output {
            Command::new(env!("CARGO_BIN_EXE_comandos"))
                .args(["install", "--home"])
                .arg(&self.home)
                .args(args)
                .env("HOME", &self.home)
                .env("XDG_CONFIG_HOME", self.root.join("config"))
                .env("XDG_DATA_HOME", self.root.join("data"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env("XDG_CACHE_HOME", self.root.join("cache"))
                .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
                .env("PATH", self.root.join("path"))
                .env("TMPDIR", self.root.join("tmp"))
                .env("TMP", self.root.join("tmp"))
                .env("TEMP", self.root.join("tmp"))
                .env_remove("COMANDOS_WEB_SOURCE")
                .output()
                .unwrap()
        }

        fn stage(&self) -> Output {
            self.run(&["--stage-app", self.candidate.to_str().unwrap()])
        }

        fn link(&self) -> PathBuf {
            self.home.join(".local/bin/cc-app")
        }
        fn pointer(&self) -> PathBuf {
            self.home.join(".local/share/comandos/bin/comandos-app")
        }
        fn artifact(&self) -> PathBuf {
            fs::canonicalize(self.pointer()).unwrap()
        }
        fn original(&self) -> PathBuf {
            let old = PathBuf::from("../../old app's binary");
            symlink(&old, self.link()).unwrap();
            old
        }
        fn snapshot(&self) -> BTreeMap<PathBuf, (u64, u32, i64, i64, Vec<u8>)> {
            fn walk(
                root: &Path,
                p: &Path,
                out: &mut BTreeMap<PathBuf, (u64, u32, i64, i64, Vec<u8>)>,
            ) {
                let m = p.symlink_metadata().unwrap();
                let bytes = if m.file_type().is_symlink() {
                    use std::os::unix::ffi::OsStrExt;
                    fs::read_link(p).unwrap().as_os_str().as_bytes().to_vec()
                } else if m.is_file() {
                    fs::read(p).unwrap()
                } else {
                    Vec::new()
                };
                out.insert(
                    p.strip_prefix(root).unwrap().to_path_buf(),
                    (m.ino(), m.mode(), m.mtime(), m.mtime_nsec(), bytes),
                );
                if m.is_dir() {
                    for e in fs::read_dir(p).unwrap() {
                        walk(root, &e.unwrap().path(), out);
                    }
                }
            }
            let mut out = BTreeMap::new();
            walk(&self.root, &self.root, &mut out);
            out
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    fn success(o: &Output) {
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }

    #[test]
    fn cc_app_dry_run_keeps_the_existing_link() {
        let f = Fixture::new();
        let old = f.original();
        success(&f.stage());
        let before = f.snapshot();
        let o = f.run(&["--link", "cc-app", "--dry-run"]);
        success(&o);
        assert_eq!(fs::read_link(f.link()).unwrap(), old);
        assert_eq!(
            f.snapshot(),
            before,
            "dry-run writes nothing, including metadata"
        );
        let stdout = String::from_utf8(o.stdout).unwrap();
        assert!(stdout.contains(f.artifact().to_str().unwrap()));
        assert!(
            stdout.contains(
                f.home
                    .join(".local/share/comandos/rollback/cc-app.target")
                    .to_str()
                    .unwrap()
            )
        );
        assert!(stdout.contains("rollback"));
    }

    #[test]
    fn app_release_has_content_manifest_and_cannot_link_cli() {
        let f = Fixture::new();
        assert_eq!(f.run(&["--link", "cc-app"]).status.code(), Some(1));
        success(&f.run(&["--stage"]));
        assert_eq!(f.run(&["--link", "cc-app"]).status.code(), Some(1));
        success(&f.stage());
        let artifact = f.artifact();
        assert_eq!(artifact.file_name().unwrap(), "comandos-app");
        assert_eq!(
            fs::read(&artifact).unwrap(),
            fs::read(&f.candidate).unwrap()
        );
        assert_eq!(
            artifact.metadata().unwrap().permissions().mode() & 0o777,
            0o755
        );
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(artifact.parent().unwrap().join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["artifact"], "comandos-app");
        use sha2::{Digest, Sha256};
        assert_eq!(
            manifest["sha256"],
            format!("{:x}", Sha256::digest(fs::read(&f.candidate).unwrap()))
        );
        success(&f.run(&["--link", "cc-app"]));
        assert_eq!(fs::read_link(f.link()).unwrap(), artifact);
    }

    #[test]
    fn staging_next_app_does_not_cut_over_and_link_keeps_first_rollback() {
        let f = Fixture::new();
        let old = f.original();
        success(&f.stage());
        success(&f.run(&["--link", "cc-app"]));
        let first = fs::read_link(f.link()).unwrap();
        let original_record = record(&f.home, "cc-app");
        fs::write(&f.candidate, b"#!/bin/sh\nexit 92\n").unwrap();
        success(&f.stage());
        assert_eq!(fs::read_link(f.link()).unwrap(), first);
        success(&f.run(&["--link", "cc-app"]));
        assert_ne!(fs::read_link(f.link()).unwrap(), first);
        assert_eq!(record(&f.home, "cc-app"), original_record);
        let before = f.snapshot();
        let o = f.run(&["--rollback", "cc-app", "--dry-run"]);
        success(&o);
        assert!(String::from_utf8_lossy(&o.stdout).contains(&old.display().to_string()));
        assert_eq!(f.snapshot(), before);
        success(&f.run(&["--rollback", "cc-app"]));
        assert_eq!(fs::read_link(f.link()).unwrap(), old);
    }

    #[test]
    fn regular_original_survives_updates_and_rollback_restores_inode_mode_bytes() {
        let f = Fixture::new();
        fs::write(f.link(), b"original app\n").unwrap();
        fs::set_permissions(f.link(), fs::Permissions::from_mode(0o751)).unwrap();
        let ino = f.link().metadata().unwrap().ino();
        success(&f.stage());
        success(&f.run(&["--link", "cc-app"]));
        success(&f.run(&["--link", "cc-app"]));
        success(&f.run(&["--rollback", "cc-app"]));
        assert_eq!(fs::read(f.link()).unwrap(), b"original app\n");
        assert_eq!(f.link().metadata().unwrap().ino(), ino);
        assert_eq!(f.link().metadata().unwrap().mode() & 0o777, 0o751);
    }

    #[test]
    fn app_dry_stage_and_failed_candidate_are_read_only() {
        let f = Fixture::new();
        let before = f.snapshot();
        let o = f.run(&["--stage-app", f.candidate.to_str().unwrap(), "--dry-run"]);
        success(&o);
        assert!(String::from_utf8_lossy(&o.stdout).contains("comandos-app"));
        assert_eq!(f.snapshot(), before);
        assert_eq!(f.run(&["--stage-app", "relative"]).status.code(), Some(2));
        fs::set_permissions(&f.candidate, fs::Permissions::from_mode(0o600)).unwrap();
        let before = f.snapshot();
        assert_eq!(f.stage().status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
        fs::remove_file(&f.candidate).unwrap();
        symlink("missing", &f.candidate).unwrap();
        let before = f.snapshot();
        assert_eq!(f.stage().status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
    }

    #[test]
    fn malformed_tampered_or_symlinked_app_release_never_links_or_changes_original() {
        let f = Fixture::new();
        let old = f.original();
        success(&f.stage());
        let artifact = f.artifact();
        let manifest = artifact.parent().unwrap().join("manifest.json");
        let good = fs::read(&manifest).unwrap();
        for malformed in [
            b"{bad".as_slice(),
            b"{\"state_protocol\":2}",
            b"{\"state_protocol\":2,\"artifact\":\"comandos\",\"sha256\":\"bad\"}",
        ] {
            fs::write(&manifest, malformed).unwrap();
            let before = f.snapshot();
            assert_eq!(f.run(&["--link", "cc-app"]).status.code(), Some(1));
            assert_eq!(
                f.run(&["--link", "cc-app", "--dry-run"]).status.code(),
                Some(1)
            );
            assert_eq!(f.snapshot(), before);
        }
        fs::write(&manifest, &good).unwrap();
        fs::write(&artifact, b"tampered").unwrap();
        let before = f.snapshot();
        assert_eq!(f.run(&["--link", "cc-app"]).status.code(), Some(1));
        assert_eq!(f.stage().status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
        fs::remove_file(&artifact).unwrap();
        symlink(&f.candidate, &artifact).unwrap();
        let before = f.snapshot();
        assert_eq!(f.run(&["--link", "cc-app"]).status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
        assert_eq!(fs::read_link(f.link()).unwrap(), old);
    }

    #[test]
    fn app_release_symlink_parent_and_redirected_pointer_are_rejected() {
        let f = Fixture::new();
        let outside = f.root.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::create_dir_all(f.home.join(".local/share/comandos")).unwrap();
        symlink(&outside, f.home.join(".local/share/comandos/releases")).unwrap();
        let before = f.snapshot();
        assert_eq!(f.stage().status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
        fs::remove_file(f.home.join(".local/share/comandos/releases")).unwrap();
        success(&f.stage());
        fs::remove_file(f.pointer()).unwrap();
        symlink(&f.candidate, f.pointer()).unwrap();
        let before = f.snapshot();
        assert_eq!(f.run(&["--link", "cc-app"]).status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
    }

    #[test]
    fn malformed_rollback_and_existing_backup_fail_without_overwrite() {
        let f = Fixture::new();
        success(&f.stage());
        let rollback = f.home.join(".local/share/comandos/rollback");
        fs::create_dir(&rollback).unwrap();
        fs::write(rollback.join("cc-app.target"), b"BAD\n").unwrap();
        let before = f.snapshot();
        assert_eq!(
            f.run(&["--rollback", "cc-app", "--dry-run"]).status.code(),
            Some(1)
        );
        assert_eq!(f.snapshot(), before);
        fs::write(f.link(), b"original").unwrap();
        fs::write(rollback.join("cc-app.orig"), b"existing backup").unwrap();
        let before = f.snapshot();
        assert_eq!(f.run(&["--link", "cc-app"]).status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
    }

    #[test]
    fn cli_stage_never_prunes_complete_app_release() {
        let f = Fixture::new();
        success(&f.stage());
        let artifact = f.artifact();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        fs::File::open(artifact.parent().unwrap())
            .unwrap()
            .set_modified(old)
            .unwrap();
        success(&f.run(&["--stage"]));
        assert!(artifact.is_file());
        success(&f.run(&["--link", "cc-app"]));
    }

    #[test]
    fn app_restage_checks_own_release_and_unknown_manifest_version() {
        let f = Fixture::new();
        success(&f.stage());
        let artifact = f.artifact();
        success(&f.run(&["--stage-app", artifact.to_str().unwrap()]));
        let manifest = artifact.parent().unwrap().join("manifest.json");
        let mut v: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        v["artifact_version"] = 99.into();
        fs::write(&manifest, serde_json::to_vec(&v).unwrap()).unwrap();
        let before = f.snapshot();
        assert_eq!(
            f.run(&["--stage-app", artifact.to_str().unwrap()])
                .status
                .code(),
            Some(1)
        );
        assert_eq!(f.stage().status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
    }

    #[test]
    fn absent_app_rollback_and_cli_dry_runs_are_read_only() {
        let f = Fixture::new();
        let before = f.snapshot();
        success(&f.run(&["--stage", "--dry-run"]));
        assert_eq!(f.snapshot(), before);
        assert_eq!(
            f.run(&["--rollback", "cc-app", "--dry-run"]).status.code(),
            Some(1)
        );
        assert_eq!(f.snapshot(), before);
        success(&f.stage());
        success(&f.run(&["--link", "cc-app"]));
        let before = f.snapshot();
        success(&f.run(&["--rollback", "cc-app", "--dry-run"]));
        assert_eq!(f.snapshot(), before);
        success(&f.run(&["--rollback", "cc-app"]));
        assert!(f.link().symlink_metadata().is_err());
    }

    #[test]
    fn app_link_does_not_follow_rollback_symlinks_or_replace_external_changes() {
        let f = Fixture::new();
        f.original();
        success(&f.stage());
        success(&f.run(&["--link", "cc-app"]));
        let original_record = record(&f.home, "cc-app");
        fs::remove_file(f.link()).unwrap();
        symlink("external replacement", f.link()).unwrap();
        let before = f.snapshot();
        assert_eq!(f.run(&["--link", "cc-app"]).status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
        assert_eq!(record(&f.home, "cc-app"), original_record);
        assert_eq!(f.run(&["--rollback", "cc-app"]).status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
        let rec = f.home.join(".local/share/comandos/rollback/cc-app.target");
        fs::remove_file(&rec).unwrap();
        symlink(&f.candidate, rec).unwrap();
        let before = f.snapshot();
        assert_eq!(f.run(&["--link", "cc-app"]).status.code(), Some(1));
        assert_eq!(f.run(&["--rollback", "cc-app"]).status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
    }

    #[test]
    fn app_candidate_symlink_and_directory_link_are_rejected_before_writes() {
        let f = Fixture::new();
        let real = f.root.join("real candidate");
        fs::rename(&f.candidate, &real).unwrap();
        symlink(&real, &f.candidate).unwrap();
        let before = f.snapshot();
        assert_eq!(f.stage().status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
        fs::remove_file(&f.candidate).unwrap();
        fs::rename(&real, &f.candidate).unwrap();
        success(&f.stage());
        fs::create_dir(f.link()).unwrap();
        let before = f.snapshot();
        assert_eq!(
            f.run(&["--link", "cc-app", "--dry-run"]).status.code(),
            Some(1)
        );
        assert_eq!(f.run(&["--link", "cc-app"]).status.code(), Some(1));
        assert_eq!(f.snapshot(), before);
    }

    #[test]
    fn concurrent_app_links_preserve_one_regular_original() {
        let f = Fixture::new();
        fs::write(f.link(), b"original concurrent app").unwrap();
        let inode = f.link().metadata().unwrap().ino();
        success(&f.stage());
        let outputs = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..8 {
                handles.push(scope.spawn(|| f.run(&["--link", "cc-app"])));
            }
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        for output in outputs {
            success(&output);
        }
        success(&f.run(&["--rollback", "cc-app"]));
        assert_eq!(fs::read(f.link()).unwrap(), b"original concurrent app");
        assert_eq!(f.link().metadata().unwrap().ino(), inode);
    }
}
