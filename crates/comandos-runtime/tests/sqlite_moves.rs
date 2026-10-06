use comandos_runtime::session_operations::{config_history, open_journal};
use comandos_store::{
    migrate::{move_db, spec_for},
    unified,
};
use std::{fs, path::PathBuf};
struct Home(PathBuf);
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn operations_opener_and_readonly_history_route_without_source_shm_or_home_locks() {
    let home = Home(std::env::temp_dir().join(format!(
            "comandos-s4-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    fs::create_dir(&home.0).unwrap();
    let spec = spec_for(&home.0, "db-operations").unwrap();
    let target = home.0.join(".local/share/comandos/comandos.sqlite3");
    let old = open_journal(&spec.legacy).unwrap();
    old.execute_batch(
        "INSERT INTO session_operations VALUES('op','pane','hash','{}','confirmed',1,'{}','{}',1)",
    )
    .unwrap();
    drop(old);
    move_db(
        &spec,
        &target,
        &home.0.join(".local/share/comandos/backups/test"),
        4000,
    )
    .unwrap();
    let new = open_journal(&spec.legacy).unwrap();
    assert_eq!(new.path(), target.to_str());
    drop(new);
    let before = fs::read(&spec.legacy).unwrap();
    let new_before = fs::read(&target).unwrap();
    assert!(
        config_history(&spec.legacy, "/project", "pane").unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(fs::read(&spec.legacy).unwrap(), before);
    assert_eq!(fs::read(&target).unwrap(), new_before);
    assert!(!PathBuf::from(format!("{}-shm", spec.legacy.display())).exists());
    assert!(!PathBuf::from(format!("{}-shm", target.display())).exists());

    let new = unified::open_unified(&target).unwrap();
    assert_eq!(
        new.query_row("SELECT count(*) FROM session_operations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
