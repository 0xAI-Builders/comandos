use comandos_runtime::{
    fresh_id, now_ms, open_state, parse_process_stat, process_start_time, state_path,
};
use std::collections::HashSet;
use std::path::Path;

#[test]
fn path_precedence_is_explicit_override_xdg_then_home() {
    let p = |s| Some(Path::new(s));
    assert_eq!(
        state_path(p("/explicit"), p("/override"), p("/xdg"), p("/home/test")).unwrap(),
        Path::new("/explicit")
    );
    assert_eq!(
        state_path(None, p("/override"), p("/xdg"), p("/home/test")).unwrap(),
        Path::new("/override")
    );
    assert_eq!(
        state_path(None, None, p("/xdg"), p("/home/test")).unwrap(),
        Path::new("/xdg/comandos/app-state.sqlite3")
    );
    assert_eq!(
        state_path(None, None, None, p("/home/test")).unwrap(),
        Path::new("/home/test/.local/state/comandos/app-state.sqlite3")
    );
    assert!(state_path(None, None, None, None).is_err());
}

#[test]
fn kernel_identity_handles_spaces_and_parentheses_in_comm() {
    let stat = format!(
        "42 (agent (worker)) S {} 98765 0 0",
        vec!["0"; 18].join(" ")
    );
    assert_eq!(parse_process_stat(&stat), Some(98765));
    assert_eq!(parse_process_stat("missing fields"), None);
    assert_eq!(process_start_time("../../"), None);
    assert_eq!(process_start_time("9999999999"), None);
    #[cfg(target_os = "linux")]
    assert!(process_start_time(&std::process::id().to_string()).is_some());
}

#[test]
fn native_reception_ids_are_uuid4_hex_and_unique_without_a_clock_counter() {
    let mut seen = HashSet::new();
    for _ in 0..1000 {
        let id = fresh_id("event").unwrap();
        let hex = id.strip_prefix("event-").expect("event prefix");
        assert_eq!(hex.len(), 32);
        assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(&hex[12..13], "4");
        assert!("89ab".contains(&hex[16..17]));
        assert!(seen.insert(id));
    }
    assert!(now_ms().unwrap() > 1_000_000_000_000);
}

#[test]
fn native_open_initializes_an_independent_database() {
    let path = std::env::temp_dir().join(format!("native-open-{}.sqlite3", std::process::id()));
    let conn = open_state(&path, 1500).unwrap();
    assert_eq!(comandos_store::state::schema_version(&conn).unwrap(), 11);
    conn.close().unwrap();
    std::fs::remove_file(path).unwrap();
}
