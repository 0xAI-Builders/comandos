//! Platform admission is exercised in owned children with private missing/poisoned HOME.
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "codex-platform-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for dir in [
            "home", "config", "data", "cache", "state", "run", "tmp", "bin", "proc",
        ] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(dir))
                .unwrap();
        }
        for name in ["codex", "tmux", "stty"] {
            let tool = root.join("bin").join(name);
            fs::write(
                &tool,
                format!(
                    "#!/bin/sh\nprintf called > {}\nexit 55\n",
                    root.join("tool-called").display()
                ),
            )
            .unwrap();
            fs::set_permissions(tool, fs::Permissions::from_mode(0o700)).unwrap();
        }
        nix::unistd::mkfifo(
            &root.join("plan-fifo"),
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )
        .unwrap();
        Self(root)
    }
    fn run(&self, platform: &str, args: &[String], expected: i32) -> Output {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "codex::portability_tests::platform_case",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", self.0.join("home"))
            .env("PATH", self.0.join("bin"))
            .env("C5_TEST_PLATFORM", platform)
            .env("C5_TEST_ARGS", serde_json::to_string(args).unwrap())
            .env("C5_TEST_EXPECTED", expected.to_string());
        for (key, dir) in [
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_RUNTIME_DIR", "run"),
            ("TMPDIR", "tmp"),
            ("TMP", "tmp"),
            ("TEMP", "tmp"),
        ] {
            command.env(key, self.0.join(dir));
        }
        command.output().unwrap()
    }
    fn cases(&self) -> Vec<Vec<String>> {
        let mut cases = vec![];
        for option in [
            None,
            Some("--apply"),
            Some("--dry-run"),
            Some("--install-only"),
            Some("--retry-failed"),
        ] {
            let mut args = vec![
                "full-access".into(),
                "--proc-root".into(),
                self.0.join("proc").to_str().unwrap().into(),
            ];
            if let Some(option) = option {
                args.push(option.into());
            }
            cases.push(args);
        }
        cases.push(vec![
            "full-access".into(),
            "--retry-report".into(),
            self.0.join("plan-fifo").to_str().unwrap().into(),
        ]);
        for dry in [false, true] {
            let mut args = vec![
                "thread-release".into(),
                "--plan".into(),
                self.0.join("plan-fifo").to_str().unwrap().into(),
            ];
            if dry {
                args.push("--dry-run".into());
            }
            cases.push(args);
        }
        cases
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn tree(root: &Path) -> BTreeMap<PathBuf, (u32, u64, Vec<u8>)> {
    let mut out = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        let metadata = path.symlink_metadata().unwrap();
        let body = if metadata.is_file() {
            fs::read(&path).unwrap()
        } else {
            vec![]
        };
        out.insert(path.clone(), (metadata.mode(), metadata.ino(), body));
        if metadata.is_dir() {
            out.extend(tree(&path));
        }
    }
    out
}
#[test]
fn platform_case() {
    let Ok(platform) = std::env::var("C5_TEST_PLATFORM") else {
        return;
    };
    let args: Vec<String> = serde_json::from_str(&std::env::var("C5_TEST_ARGS").unwrap()).unwrap();
    let expected: i32 = std::env::var("C5_TEST_EXPECTED").unwrap().parse().unwrap();
    assert_eq!(super::main_for_platform(&args, &platform), expected);
}
#[test]
fn darwin_maintenance_rejects_before_missing_home_plan_or_tool_lookup() {
    let fixture = Fixture::new();
    fs::remove_dir(fixture.0.join("home")).unwrap();
    let before = tree(&fixture.0);
    for args in fixture.cases() {
        let result = fixture.run("macos", &args, 1);
        assert!(result.status.success(), "{:?}", result);
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("requieren Linux (/proc y pidfd)"),
            "{:?}",
            result
        );
        assert_eq!(tree(&fixture.0), before);
    }
}
#[test]
fn darwin_maintenance_rejects_before_future_authority_and_keeps_all_bytes() {
    let fixture = Fixture::new();
    let db = comandos_store::unified::open_unified(&comandos_store::unified::unified_path(
        &fixture.0.join("home"),
    ))
    .unwrap();
    db.pragma_update(None, "user_version", 999999).unwrap();
    drop(db);
    let before = tree(&fixture.0);
    for args in fixture.cases() {
        let result = fixture.run("macos", &args, 1);
        assert!(result.status.success(), "{:?}", result);
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("requieren Linux (/proc y pidfd)"),
            "{:?}",
            result
        );
        assert_eq!(tree(&fixture.0), before);
    }
}
#[test]
fn darwin_admits_help_and_literal_policy_without_home() {
    let fixture = Fixture::new();
    fs::remove_dir(fixture.0.join("home")).unwrap();
    let before = tree(&fixture.0);
    for name in [
        "full-access",
        "thread-release",
        "yolo-install",
        "yolo-policy",
    ] {
        let result = fixture.run("macos", &[name.into(), "--help".into()], 0);
        assert!(result.status.success(), "{:?}", result);
        assert!(result.stderr.is_empty());
    }
    let result = fixture.run(
        "macos",
        &[
            "yolo-policy".into(),
            "--".into(),
            "--yolo".into(),
            "resume".into(),
            "literal 雪 '& $()".into(),
        ],
        0,
    );
    assert!(result.status.success(), "{:?}", result);
    assert!(result.stderr.is_empty());
    assert!(String::from_utf8_lossy(&result.stdout).contains("literal 雪 '& $()"));
    assert_eq!(tree(&fixture.0), before);
}

#[cfg(not(target_os = "linux"))]
#[test]
fn native_adapter_rejects_before_proc_argument_validation() {
    let result = super::runtime::Local::new(
        std::env::temp_dir(),
        PathBuf::from("relative-proc"),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    assert!(result.err().unwrap().contains("requieren Linux"));
}
