//! Pending native Terminal installation contract. Files and callbacks are private;
//! neither the platform service manager nor an installed application is invoked.
use comandos_cli::install::{
    darwin, full,
    plan::{self, Action},
    platform::Platform,
};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const ORIGINAL_UNIT: &str = include_str!("../../../systemd/cc-dash.service");
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "install-native-term-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn unit(&self) -> PathBuf {
        self.0.join(".config/systemd/user/cc-dash.service")
    }
    fn unit_action(&self) -> Action {
        plan::plan(&self.0, Platform::LinuxNative, &self.0.join("release"))
            .into_iter()
            .find(|a| matches!(a, Action::WriteUnit { path, .. } if path == &self.unit()))
            .expect("dashboard service must be part of Linux installation")
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn put(path: &Path, bytes: &[u8], mode: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}
#[test]
fn linux_generated_dashboard_service_explicitly_selects_native_terminal() {
    let home = Home::new();
    let Action::WriteUnit {
        bytes, original, ..
    } = home.unit_action()
    else {
        unreachable!()
    };
    assert_eq!(
        original,
        ORIGINAL_UNIT.as_bytes(),
        "immutable legacy ownership marker"
    );
    let expected = ORIGINAL_UNIT.replace(
        "%h/.local/bin/cc-dash",
        "%h/.local/share/comandos/bin/comandos dash --term native",
    );
    assert_eq!(String::from_utf8(bytes).unwrap(), expected);
    assert_eq!(
        fs::read_dir(&home.0).unwrap().count(),
        0,
        "planning is read-only"
    );
}
#[test]
fn darwin_generated_program_arguments_explicitly_select_native_terminal() {
    let home = Home::new();
    let plist = darwin::agent_plist(&home.0).unwrap();
    let array = plist
        .split("<key>ProgramArguments</key><array>")
        .nth(1)
        .unwrap()
        .split("</array>")
        .next()
        .unwrap();
    let arguments: Vec<_> = array
        .split("<string>")
        .skip(1)
        .map(|value| value.split("</string>").next().unwrap())
        .collect();
    assert_eq!(
        arguments,
        [
            format!("{}/.local/share/comandos/bin/comandos", home.0.display()),
            "dash".into(),
            "--term".into(),
            "native".into(),
            "--no-open".into(),
        ]
    );
    assert_eq!(
        fs::read_dir(&home.0).unwrap().count(),
        0,
        "rendering is read-only"
    );
}
#[test]
fn previous_generated_rust_dashboard_unit_upgrades_to_native_terminal() {
    let home = Home::new();
    let previous = ORIGINAL_UNIT.replace(
        "%h/.local/bin/cc-dash",
        "%h/.local/share/comandos/bin/comandos dash",
    );
    put(&home.unit(), previous.as_bytes(), 0o644);
    plan::apply_with(&[home.unit_action()], false, &mut |_| {
        panic!("service action")
    })
    .unwrap();
    let expected = ORIGINAL_UNIT.replace(
        "%h/.local/bin/cc-dash",
        "%h/.local/share/comandos/bin/comandos dash --term native",
    );
    assert_eq!(
        fs::read_to_string(home.unit()).unwrap(),
        expected,
        "known previous generated unit is owned, not a customized unit"
    );
}

#[test]
fn linux_generated_unit_is_idempotent_without_restarting_services() {
    let home = Home::new();
    let mut reloads = 0;
    let actions = [
        home.unit_action(),
        Action::Systemctl {
            home: home.0.clone(),
            args: vec!["--user".into(), "daemon-reload".into()],
        },
    ];
    plan::apply_with(&actions, false, &mut |action| {
        let Action::Systemctl {
            home: action_home,
            args,
        } = action
        else {
            panic!("unexpected tool")
        };
        assert_eq!(action_home, &home.0);
        assert_eq!(args, &["--user", "daemon-reload"]);
        reloads += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(reloads, 1);
    let installed = fs::read(home.unit()).unwrap();
    let metadata = home.unit().metadata().unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o644);
    let report =
        plan::apply_with(&actions, false, &mut |_| panic!("idempotent tool call")).unwrap();
    assert_eq!(report, ["nada que hacer"]);
    assert_eq!(fs::read(home.unit()).unwrap(), installed);
    assert_eq!(home.unit().metadata().unwrap().ino(), metadata.ino());
}
#[test]
fn linux_custom_regular_and_symlink_units_preserve_exact_bytes_modes_and_inodes() {
    for linked in [false, true] {
        let home = Home::new();
        let target = if linked {
            home.0.join("custom.service")
        } else {
            home.unit()
        };
        let custom = b"[Service]\nExecStart=/private/custom-dashboard --term ttyd\n# retain user configuration\n";
        put(&target, custom, 0o640);
        if linked {
            fs::create_dir_all(home.unit().parent().unwrap()).unwrap();
            symlink(&target, home.unit()).unwrap();
        }
        let inode = home.unit().symlink_metadata().unwrap().ino();
        for _ in 0..2 {
            let report = plan::apply_with(&[home.unit_action()], false, &mut |_| {
                panic!("custom unit tool call")
            })
            .unwrap();
            assert!(
                report
                    .iter()
                    .any(|r| r.starts_with("preserved customized unit "))
            );
            assert_eq!(fs::read(&target).unwrap(), custom);
            assert_eq!(
                target.metadata().unwrap().permissions().mode() & 0o777,
                0o640
            );
            assert_eq!(home.unit().symlink_metadata().unwrap().ino(), inode);
            if linked {
                assert_eq!(fs::read_link(home.unit()).unwrap(), target);
            }
        }
    }
}
#[test]
fn linux_generated_unit_late_failure_restores_original_bytes_and_mode() {
    let home = Home::new();
    put(&home.unit(), ORIGINAL_UNIT.as_bytes(), 0o640);
    let actions = [home.unit_action(), Action::AgentsSetup(home.0.clone())];
    let error = plan::apply_with(&actions, false, &mut |action| {
        assert!(matches!(action, Action::AgentsSetup(_)));
        Err("private later installation failure".into())
    })
    .unwrap_err();
    assert!(error.contains("private later installation failure"));
    assert!(!error.contains("retained recovery journal"), "{error}");
    assert_eq!(fs::read(home.unit()).unwrap(), ORIGINAL_UNIT.as_bytes());
    assert_eq!(
        home.unit().metadata().unwrap().permissions().mode() & 0o777,
        0o640
    );
}
#[test]
fn linux_generated_unit_rollback_refuses_a_foreign_edit_and_keeps_its_journal() {
    let home = Home::new();
    put(&home.unit(), ORIGINAL_UNIT.as_bytes(), 0o640);
    let custom = b"[Service]\nExecStart=/private/foreign-dashboard\n";
    let error = plan::apply_with(
        &[home.unit_action(), Action::AgentsSetup(home.0.clone())],
        false,
        &mut |action| {
            assert!(matches!(action, Action::AgentsSetup(_)));
            put(&home.unit(), custom, 0o600);
            Err("private failure after foreign edit".into())
        },
    )
    .unwrap_err();
    assert!(error.contains("retained recovery journal"), "{error}");
    assert_eq!(fs::read(home.unit()).unwrap(), custom);
    assert_eq!(
        home.unit().metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
    let journal = fs::read_dir(home.0.join(".local/share/comandos/install-journals"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.join("manifest.json").is_file())
        .unwrap();
    let args = [
        "--home".into(),
        home.0.to_str().unwrap().into(),
        "--recover-install".into(),
        journal.to_str().unwrap().into(),
    ];
    assert!(full::run_with(&args, &mut |_| panic!("recovery external tool")).is_err());
    assert_eq!(fs::read(home.unit()).unwrap(), custom);
    assert!(journal.join("manifest.json").is_file());
}
