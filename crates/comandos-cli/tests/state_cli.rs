use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "comandos-migrate-cli-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(home.join(".claude/hooks/state")).unwrap();
        fs::write(
            home.join(".claude/hooks/snippets.json"),
            "[ {\"body\": \"ñ\"} ]\n".as_bytes(),
        )
        .unwrap();
        fs::write(
            home.join(".claude/hooks/state/p--s--1.json"),
            b"{\"status\":\"done\"}",
        )
        .unwrap();
        Self(home)
    }
    fn command(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_comandos"))
            .arg("state")
            .args(args)
            .args(["--home"])
            .arg(&self.0)
            .env("HOME", &self.0)
            .env(
                "COMANDOS_DB",
                self.0.join(".local/share/comandos/comandos.sqlite3"),
            )
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn dry_run_ignores_tmpdir_inside_source_home() {
    let h = Fixture::new();
    let temp = h.0.join("tmpdir");
    fs::create_dir(&temp).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args(["state", "migrate", "--dry-run", "--home"])
        .arg(&h.0)
        .env("HOME", &h.0)
        .env(
            "COMANDOS_DB",
            h.0.join(".local/share/comandos/comandos.sqlite3"),
        )
        .env("TMPDIR", &temp)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(fs::read_dir(temp).unwrap().count(), 0);
    assert!(!h.0.join(".local").exists());
}

#[test]
fn dry_run_has_report_without_creating_database_or_backups() {
    let h = Fixture::new();
    let out = h.command(&["migrate", "--dry-run", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(report["backup_dir"].is_null());
    assert_eq!(report["steps"].as_array().unwrap().len(), 2);
    assert!(!h.0.join(".local").exists());
    assert!(!h.0.join(".claude/hooks/snippets.json.lock").exists());
}

#[test]
fn migrate_verify_backups_and_injected_status_roundtrip() {
    let h = Fixture::new();
    let out = h.command(&["migrate", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        PathBuf::from(report["backup_dir"].as_str().unwrap())
            .join("manifest.json")
            .exists()
    );
    assert!(
        h.command(&["verify", "--domain", "ui-docs", "--json"])
            .status
            .success()
    );
    let backups = h.command(&["backups", "--json"]);
    assert!(backups.status.success());
    let listed: serde_json::Value = serde_json::from_slice(&backups.stdout).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    let proc = h.0.join("proc-fixture");
    let repo = h.0.join("repo-fixture");
    fs::create_dir_all(&proc).unwrap();
    fs::create_dir_all(&repo).unwrap();
    let out = h.command(&[
        "status",
        "--json",
        "--proc-root",
        proc.to_str().unwrap(),
        "--repo",
        repo.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        status["domains"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["domain"] == "ui-docs"
                && row["mode"] == "mirror"
                && !row["last_verify"].is_null())
    );
    fs::write(h.0.join(".claude/hooks/snippets.json"), b"changed").unwrap();
    let different = h.command(&["verify", "--domain", "ui-docs"]);
    assert!(!different.status.success());
    assert!(String::from_utf8_lossy(&different.stdout).contains("hooks/snippets.json"));
}
