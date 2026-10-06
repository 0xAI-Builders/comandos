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

#[test]
fn sqlite_move_dry_run_and_inverse_cli_roundtrip() {
    let h = Fixture::new();
    let source = h.0.join(".claude/hooks/operator/actions.sqlite");
    let operator = comandos_store::operator::open_operator_db_at(&source).unwrap();
    operator
        .conn
        .execute("INSERT INTO actions(id,detail) VALUES('original','ñ')", [])
        .unwrap();
    drop(operator);
    let before = fs::read(&source).unwrap();
    let out = h.command(&["move", "db-operator", "--dry-run"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let estimate: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(estimate["copy_ms"].as_u64().unwrap() > 0);
    assert_eq!(fs::read(&source).unwrap(), before);
    assert!(!h.0.join(".local").exists());
    let out = h.command(&["move", "db-operator"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let moved = comandos_store::operator::open_operator_db_at(&source).unwrap();
    assert_eq!(moved.table, "operator_actions");
    moved
        .conn
        .execute("INSERT INTO operator_actions(id) VALUES('after')", [])
        .unwrap();
    drop(moved);
    let out = h.command(&["demote", "db-operator"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let legacy = comandos_store::operator::open_operator_db_at(&source).unwrap();
    assert_eq!(legacy.table, "actions");
    assert_eq!(
        legacy
            .conn
            .query_row("SELECT count(*) FROM actions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert!(!h.command(&["move", "db-unknown"]).status.success());
    assert!(
        !h.command(&["demote", "db-operator", "--dry-run"])
            .status
            .success()
    );
}
#[test]
fn migrate_dry_run_reports_usage_move_estimate_without_creating_home_state() {
    let h = Fixture::new();
    let path = h.0.join(".claude/hooks/comandos-usage.sqlite");
    let c = comandos_store::usage::open_usage_db_at(&path).unwrap();
    comandos_store::usage::ensure_schema(&c).unwrap();
    drop(c);
    let before = fs::read(&path).unwrap();
    let out = h.command(&["migrate", "--dry-run"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(report["usage_move_estimate_ms"].as_u64().unwrap() > 0);
    assert_eq!(fs::read(path).unwrap(), before);
    assert!(!h.0.join(".local").exists());
}
