use comandos_oracle::{Mode, try_oracle_at_with_mode};
use serde_json::json;
use std::{fs, path::PathBuf};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "golden-oracle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn record_replay_and_check_never_skip_expected_results() {
    let f = Fixture::new();
    let input = json!({"case":1});
    assert_eq!(
        try_oracle_at_with_mode(&f.0, "fixture", &input, Mode::Record, || Ok(
            b"known\n".to_vec()
        ))
        .unwrap(),
        b"known\n"
    );
    assert_eq!(
        try_oracle_at_with_mode(&f.0, "fixture", &input, Mode::Replay, || panic!(
            "replay launched interpreter"
        ))
        .unwrap(),
        b"known\n"
    );
    assert!(
        try_oracle_at_with_mode(&f.0, "fixture", &input, Mode::Check, || Ok(
            b"drift".to_vec()
        ))
        .unwrap_err()
        .contains("deriva")
    );
}
#[test]
fn missing_golden_fails_without_running_closure() {
    let f = Fixture::new();
    let error = try_oracle_at_with_mode(&f.0, "missing", &json!({}), Mode::Replay, || {
        panic!("missing golden launched interpreter")
    })
    .unwrap_err();
    assert!(error.contains("COMANDOS_ORACLE=record"));
}
#[test]
fn traversal_is_rejected() {
    let f = Fixture::new();
    assert!(
        try_oracle_at_with_mode(&f.0, "../escape", &json!({}), Mode::Record, || Ok(vec![]))
            .is_err()
    );
}
#[test]
fn path_normalization_can_be_rehydrated() {
    let roots = [("<HOME>", std::path::Path::new("/tmp/private-home"))];
    let input = b"{\"path\":\"/tmp/private-home/a\"}";
    let normalized = comandos_oracle::normalize(input, &roots);
    assert_eq!(normalized, b"{\"path\":\"<HOME>/a\"}");
    assert_eq!(comandos_oracle::restore(&normalized, &roots), input);
}
#[test]
fn snapshots_replay_files_symlinks_and_sqlite_without_corrupting_path_lengths() {
    let f = Fixture::new();
    let home = f.0.join("home");
    fs::create_dir(&home).unwrap();
    fs::write(
        home.join("state.json"),
        format!("{{\"path\":\"{}/file\"}}", home.display()),
    )
    .unwrap();
    std::os::unix::fs::symlink("state.json", home.join("link")).unwrap();
    let c = rusqlite::Connection::open(home.join("state.sqlite")).unwrap();
    c.execute_batch(
        "CREATE TABLE records(id INTEGER PRIMARY KEY,value TEXT); CREATE INDEX before_next_table ON records(value); CREATE TABLE later(value TEXT); CREATE TABLE a_child(parent_id INTEGER REFERENCES records(id)); PRAGMA user_version=11",
    )
    .unwrap();
    c.execute(
        "INSERT INTO records VALUES(1,?1)",
        [home.join("file").to_string_lossy().as_ref()],
    )
    .unwrap();
    c.execute("INSERT INTO a_child VALUES(1)", []).unwrap();
    drop(c);
    fs::create_dir(home.join("empty")).unwrap();
    let tree = comandos_oracle::snapshot_tree(&home, &[("<HOME>", &home)]).unwrap();
    fs::write(home.join("state.json"), b"changed").unwrap();
    let replay = f.0.join("a-longer-replay-home");
    fs::create_dir(&replay).unwrap();
    comandos_oracle::restore_tree(&replay, &tree, &[("<HOME>", &replay)]).unwrap();
    assert_eq!(
        comandos_oracle::snapshot_tree(&replay, &[("<HOME>", &replay)]).unwrap(),
        tree
    );
    assert!(
        fs::read_to_string(replay.join("state.json"))
            .unwrap()
            .contains(replay.to_str().unwrap())
    );
    assert!(replay.join("empty").is_dir());
    let c = rusqlite::Connection::open(replay.join("state.sqlite")).unwrap();
    assert_eq!(
        c.query_row("SELECT value FROM records", [], |r| r.get::<_, String>(0))
            .unwrap(),
        replay.join("file").to_string_lossy()
    );
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        11
    );
    assert_eq!(
        fs::read_link(replay.join("link")).unwrap(),
        std::path::Path::new("state.json")
    );
}
#[test]
fn replay_applies_only_oracle_changes_and_keeps_unchanged_sqlite_inode() {
    use std::os::unix::fs::MetadataExt;
    let f = Fixture::new();
    let golden = f.0.join("golden");
    let home = f.0.join("record-home");
    let replay = f.0.join("longer-replay-home");
    for h in [&home, &replay] {
        fs::create_dir(h).unwrap();
        let c = rusqlite::Connection::open(h.join("native.sqlite")).unwrap();
        c.execute_batch("CREATE TABLE x(value TEXT);INSERT INTO x VALUES('native')")
            .unwrap();
        fs::write(h.join("deleted"), "old").unwrap();
    }
    comandos_oracle::text_with_tree_at_with_mode(
        &golden,
        "delta",
        &json!({}),
        &home,
        &[("<HOME>", &home)],
        Mode::Record,
        || {
            fs::remove_file(home.join("deleted")).unwrap();
            fs::write(home.join("created"), home.to_string_lossy().as_bytes()).unwrap();
            Ok(home.display().to_string())
        },
    )
    .unwrap();
    let inode = fs::metadata(replay.join("native.sqlite")).unwrap().ino();
    let result = comandos_oracle::text_with_tree_at_with_mode(
        &golden,
        "delta",
        &json!({}),
        &replay,
        &[("<HOME>", &replay)],
        Mode::Replay,
        || panic!("Python must never execute"),
    )
    .unwrap();
    assert_eq!(result, replay.display().to_string());
    assert_eq!(fs::read_to_string(replay.join("created")).unwrap(), result);
    assert!(!replay.join("deleted").exists());
    assert_eq!(
        fs::metadata(replay.join("native.sqlite")).unwrap().ino(),
        inode
    );
}
