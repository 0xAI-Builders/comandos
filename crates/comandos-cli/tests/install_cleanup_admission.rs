use comandos_cli::install::{cleanup, retirement};
use comandos_runtime::retirement::{Manifest, Options};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::symlink,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    options: Options,
}
impl Fixture {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cleanup-admission-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        for sub in [
            "repo/lib",
            "repo/crates/example/tests",
            "home/.local/bin",
            "proc",
            "transient",
        ] {
            fs::create_dir_all(root.join(sub)).unwrap();
        }
        fs::write(root.join("repo/lib/retired.py"), "def retired(): pass\n").unwrap();
        fs::write(
            root.join("repo/crates/example/tests/native.rs"),
            "#[test]\nfn replacement_works() {}\n",
        )
        .unwrap();
        let options = Options {
            repo: root.join("repo"),
            repo_live: root.join("repo"),
            home: root.join("home"),
            proc: root.join("proc"),
            transient: root.join("transient"),
            crontab: String::new(),
            commit: "fixture".into(),
            tracked: Some(vec![
                "lib/retired.py".into(),
                "crates/example/tests/native.rs".into(),
            ]),
        };
        symlink(
            options.repo.join("lib/retired.py"),
            options.home.join(".local/bin/old"),
        )
        .unwrap();
        Self { root, options }
    }
    fn manifest(&self, status: &str) -> Manifest {
        Manifest::from_value(&json!({"version":1,"rows":[{"path":"lib/retired.py","rust":"native replacement","phase":"fixture","status":status,"verified_by":["example::native::replacement_works"],"verification":{"commit":"fixture","passed":true}}]})).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn pending_and_new_live_consumers_never_move_legacy_paths() {
    let fixture = Fixture::new();
    let manifest = fixture.manifest("retired");
    let pending = retirement::select(&fixture.options, &fixture.manifest("pending")).unwrap();
    assert!(pending.paths.is_empty());
    assert_eq!(pending.denied, 1);
    let selected = retirement::select(&fixture.options, &manifest).unwrap();
    assert_eq!(selected.paths, vec![PathBuf::from(".local/bin/old")]);
    assert_eq!(selected.denied, 0);
    assert!(
        cleanup::stage_admitted(&fixture.options.home, &selected.paths, true, &mut || Ok(()))
            .unwrap()
            .is_none()
    );
    assert!(!fixture.options.home.join(".local/share/comandos").exists());
    // A process that appeared after selection must block the locked admission.
    fs::create_dir(fixture.options.proc.join("42")).unwrap();
    fs::write(
        fixture.options.proc.join("42/cmdline"),
        format!(
            "python3\0{}\0",
            fixture.options.repo.join("lib/retired.py").display()
        ),
    )
    .unwrap();
    let result =
        cleanup::stage_admitted(&fixture.options.home, &selected.paths, false, &mut || {
            let fresh = retirement::select(&fixture.options, &manifest).unwrap();
            if fresh.denied != 0 {
                Err("new live consumer".into())
            } else {
                Ok(())
            }
        });
    assert!(result.is_err());
    assert!(fixture.options.home.join(".local/bin/old").is_symlink());
    assert!(
        !fixture
            .options
            .home
            .join(".local/share/comandos/backups")
            .exists()
    );
}

#[test]
fn admitted_legacy_links_archive_and_restore_without_touching_foreign_files() {
    let fixture = Fixture::new();
    let manifest = fixture.manifest("retired");
    fs::write(fixture.options.home.join("foreign"), "user data").unwrap();
    let original = fs::read_link(fixture.options.home.join(".local/bin/old")).unwrap();
    let selected = retirement::select(&fixture.options, &manifest).unwrap();
    let backup =
        cleanup::stage_admitted(&fixture.options.home, &selected.paths, false, &mut || {
            let fresh = retirement::select(&fixture.options, &manifest).unwrap();
            if fresh.denied != 0 || fresh.paths != selected.paths {
                Err("admission changed".into())
            } else {
                Ok(())
            }
        })
        .unwrap()
        .unwrap();
    assert!(!fixture.options.home.join(".local/bin/old").is_symlink());
    assert_eq!(
        fs::read_to_string(fixture.options.home.join("foreign")).unwrap(),
        "user data"
    );
    cleanup::restore(&fixture.options.home, &backup, false).unwrap();
    assert_eq!(
        fs::read_link(fixture.options.home.join(".local/bin/old")).unwrap(),
        original
    );
}

#[test]
fn pending_cleanup_cli_cannot_install_or_create_backups_even_in_real_mode() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.options.repo.join("docs/verification")).unwrap();
    fs::write(fixture.options.repo.join("docs/verification/retirement.json"), serde_json::to_vec(&json!({"version":1,"rows":[{"path":"lib/retired.py","status":"pending","verified_by":[]}]})).unwrap()).unwrap();
    for extra in [vec![], vec!["--dry-run"]] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_comandos"))
            .args([
                "install",
                "--home",
                fixture.options.home.to_str().unwrap(),
                "--cleanup-legacy",
                "--cleanup-repo",
                fixture.options.repo.to_str().unwrap(),
            ])
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains("no artifact has completed retirement")
        );
        assert!(fixture.options.home.join(".local/bin/old").is_symlink());
        assert!(!fixture.options.home.join(".local/share/comandos").exists());
    }
    let invalid = std::process::Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args([
            "install",
            "--home",
            fixture.options.home.to_str().unwrap(),
            "--cleanup-legacy",
            "--cleanup-repo",
            fixture.options.repo.to_str().unwrap(),
            "--restore-legacy",
            "unrelated",
        ])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(2));
}
