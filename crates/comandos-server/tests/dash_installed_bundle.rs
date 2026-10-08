//! Runtime configuration from a private installed executable, outside the checkout.
//! This resolves paths only: no listener, worker, tmux or service is started.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "installed-web-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        for path in [
            "home/.claude/hooks/dash",
            "cwd",
            "runtime",
            "releases/private-id/web",
        ] {
            fs::create_dir_all(root.join(path)).unwrap();
            fs::set_permissions(root.join(path), fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(
            root.join("home/.claude/hooks/dash-token"),
            b"private-config-token",
        )
        .unwrap();
        fs::write(
            root.join("releases/private-id/web/manifest.json"),
            b"{\"files\":{}}",
        )
        .unwrap();
        // The actual test executable calls the same from_env entry used by dash.
        // Relocation makes current_exe observe the installed release layout.
        fs::copy(
            std::env::current_exe().unwrap(),
            root.join("releases/private-id/comandos"),
        )
        .unwrap();
        Self(root)
    }
    fn probe(&self, target: Option<&Path>) -> PathBuf {
        let mut command = Command::new(self.0.join("releases/private-id/comandos"));
        command
            .args([
                "--exact",
                "installed_configuration_worker",
                "--ignored",
                "--nocapture",
            ])
            .current_dir(self.0.join("cwd"))
            .env_clear()
            .env("HOME", self.0.join("home"))
            .env("XDG_RUNTIME_DIR", self.0.join("runtime"))
            .env(
                "COMANDOS_INSTALLED_CONFIGURATION_OUTPUT",
                self.0.join("resolved-web-dir"),
            );
        if let Some(target) = target {
            command.env("CARGO_TARGET_DIR", target);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        PathBuf::from(fs::read_to_string(self.0.join("resolved-web-dir")).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
#[ignore = "private relocated executable subprocess only"]
fn installed_configuration_worker() {
    let output =
        std::env::var_os("COMANDOS_INSTALLED_CONFIGURATION_OUTPUT").expect("owned fixture output");
    let cfg =
        comandos_server::dash::from_env(&["--term".into(), "native".into(), "--no-open".into()])
            .unwrap();
    assert_eq!(cfg.term, comandos_server::dash::term::TermMode::Native);
    fs::write(output, cfg.web_dir.to_str().unwrap()).unwrap();
}
#[test]
fn installed_executable_discovers_its_sibling_web_bundle_outside_checkout() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.probe(None),
        fixture.0.join("releases/private-id/web")
    );
}
#[test]
fn empty_cargo_target_dir_still_discovers_installed_bundle() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.probe(Some(Path::new(""))),
        fixture.0.join("releases/private-id/web")
    );
}
#[test]
fn explicit_absolute_cargo_target_dir_overrides_installed_bundle() {
    let fixture = Fixture::new();
    let target = fixture.0.join("explicit-build");
    assert_eq!(fixture.probe(Some(&target)), target.join("web"));
}
#[test]
fn explicit_relative_cargo_target_dir_remains_relative_to_current_directory() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.probe(Some(Path::new("explicit-build"))),
        fixture.0.join("cwd/explicit-build/web")
    );
}
#[test]
fn missing_installed_bundle_preserves_developer_workspace_fallback() {
    let fixture = Fixture::new();
    fs::remove_dir_all(fixture.0.join("releases/private-id/web")).unwrap();
    assert_eq!(fixture.probe(None), fixture.0.join("cwd/target/web"));
}
