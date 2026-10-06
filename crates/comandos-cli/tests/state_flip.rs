use comandos_store::{migrate, unified};
use std::{fs, path::PathBuf, process::Command};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "cli-state-flip-{}-{}",
            std::process::id(),
            migrate::journal::new_id(0).unwrap()
        ));
        fs::create_dir_all(home.join(".claude/hooks")).unwrap();
        fs::create_dir(home.join("proc")).unwrap();
        fs::write(home.join(".claude/hooks/snippets.json"), b"[]\n").unwrap();
        let now = migrate::journal::now_ms().unwrap();
        let db = unified::unified_path(&home);
        migrate::migrate(&migrate::MigrateOptions {
            home: home.clone(),
            db: db.clone(),
            dry_run: false,
            resume: false,
            domains: Some(vec!["ui-docs".into()]),
            now_ms: now - 90_000_000,
        })
        .unwrap();
        let c = unified::open_unified(&db).unwrap();
        for t in [now - 89_000_000, now - 40_000_000, now - 1] {
            let r = migrate::verify(&home, &c, "ui-docs").unwrap();
            migrate::journal::record_verify(&c, &r, t).unwrap();
        }
        Self(home)
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_comandos"))
            .arg("state")
            .args(args)
            .arg("--home")
            .arg(&self.0)
            .arg("--proc-root")
            .arg(self.0.join("proc"))
            .arg("--repo")
            .arg(&self.0)
            .env_remove("COMANDOS_DB")
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn flip_cli_requires_capable_writers_and_demotes_without_loss() {
    let f = Fixture::new();
    let p = f.0.join("proc/123");
    fs::create_dir(&p).unwrap();
    std::os::unix::fs::symlink("/fixture/comandos", p.join("exe")).unwrap();
    fs::write(p.join("cmdline"), b"comandos\0dash\0").unwrap();
    let out = f.run(&["flip", "ui-docs", "unified"]);
    assert!(!out.status.success());
    fs::remove_dir_all(p).unwrap();
    let out = f.run(&["flip", "ui-docs", "unified"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = f.run(&["demote", "--all"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read(f.0.join(".claude/hooks/snippets.json")).unwrap(),
        b"[]\n"
    );
}
#[test]
fn rollback_requires_explicit_loss_flag() {
    let f = Fixture::new();
    let out = f.run(&["rollback", "anything"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("perder-desde-el-respaldo"));
}
#[test]
fn rollback_release_demotes_unified_domains_and_refuses_sealed() {
    let f = Fixture::new();
    let out = f.run(&["flip", "ui-docs", "unified"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let releases = f.0.join(".local/share/comandos/releases");
    let bin = f.0.join(".local/share/comandos/bin");
    fs::create_dir_all(&bin).unwrap();
    for id in ["aaaaaaaaaaaa", "bbbbbbbbbbbb"] {
        fs::create_dir_all(releases.join(id)).unwrap();
        fs::write(releases.join(id).join("comandos"), b"fixture").unwrap();
    }
    fs::write(releases.join("previous"), b"aaaaaaaaaaaa\n").unwrap();
    std::os::unix::fs::symlink(releases.join("bbbbbbbbbbbb/comandos"), bin.join("comandos"))
        .unwrap();
    comandos_cli::install::release::rollback_release(&f.0).unwrap();
    let c = unified::open_unified(&unified::unified_path(&f.0)).unwrap();
    assert_eq!(
        unified::mode_of(Some(&c), "ui-docs").unwrap(),
        unified::Mode::Mirror
    );
    unified::set_mode(&c, "ui-docs", unified::Mode::Sealed, "fixture", 1).unwrap();
    let before = fs::read_link(bin.join("comandos")).unwrap();
    assert!(comandos_cli::install::release::rollback_release(&f.0).is_err());
    assert_eq!(fs::read_link(bin.join("comandos")).unwrap(), before);
}
#[test]
fn full_private_state_drill_preserves_source_and_exercises_sqlite() {
    let f = Fixture::new();
    let h = f.0.join("source-home");
    let hooks = h.join(".claude/hooks");
    fs::create_dir_all(hooks.join("state")).unwrap();
    fs::create_dir(hooks.join("native-processes")).unwrap();
    for (name, body) in [
        ("snippets.json", "[]\n"),
        ("state/pane.json", "{}\n"),
        ("native-processes/1.json", "{}\n"),
        ("events.jsonl", "{}\n"),
        ("app-focus.json", "{}\n"),
        (
            "app-sessions-v2.json",
            "{\"version\":2,\"saved_at\":1,\"sessions\":{}}\n",
        ),
    ] {
        fs::write(hooks.join(name), body).unwrap();
    }
    let db = h.join(".claude/hooks/operator/actions.sqlite");
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    let c = rusqlite_for_fixture(&db);
    drop(c);
    let original = fs::read(hooks.join("snippets.json")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args(["state", "drill", "--source-home"])
        .arg(&h)
        .env_remove("COMANDOS_DB")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("db-operator"));
    assert!(text.contains("Rollback de release protocolo 0: verificado"));
    assert_eq!(fs::read(hooks.join("snippets.json")).unwrap(), original);
    assert!(!hooks.join("snippets.json.lock").exists());
}
fn rusqlite_for_fixture(path: &std::path::Path) -> rusqlite::Connection {
    let c = rusqlite::Connection::open(path).unwrap();
    c.execute_batch("CREATE TABLE actions(id TEXT PRIMARY KEY,tool TEXT,status TEXT,detail TEXT,created REAL,updated REAL);INSERT INTO actions VALUES('original','browser','confirmed',X'00FF',1.25,NULL)").unwrap();
    c
}
