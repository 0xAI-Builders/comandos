use comandos_store::domains::catalog::{TargetKind, catalog, file_classification, source};

#[test]
fn sqlite_sidecars_stay_with_their_domain_and_nearby_names_do_not_match() {
    for path in [
        "H/comandos-usage.sqlite",
        "H/comandos-usage.sqlite-wal",
        "H/comandos-usage.sqlite-shm",
    ] {
        let spec = source(path).expect("uso");
        assert_eq!(spec.domain, "db-usage");
        assert_eq!(spec.kind, TargetKind::Sqlite);
    }
    assert!(source("H/comandos-usage.sqlite.backup").is_none());
    assert!(source("H/stateful/pane.json").is_none());
    assert!(source("H/state/pane.json.backup").is_none());
}

#[test]
fn shared_tabs_commands_and_snapshots_have_distinct_owners() {
    for (path, domain) in [
        ("H/app-tabs.json", "tabs"),
        ("H/app-tab-active.json", "tabs"),
        ("H/app-tab-open.json", "app-commands"),
        ("H/app-tab-close.json", "app-commands"),
        ("H/app-tab-back.json", "app-commands"),
        ("H/app-sessions-v2.json.bak", "layout"),
        ("H/app-sessions-v2.json.history/123.json", "layout"),
        ("H/native-processes/pane.json", "processes"),
        ("STATE/extensions/snapshot.json", "extensions"),
    ] {
        assert_eq!(source(path).expect(path).domain, domain);
        assert_eq!(
            catalog().iter().filter(|s| s.matches(path)).count(),
            1,
            "dueño único: {path}"
        );
    }
    assert_eq!(file_classification("H/unknown-state.json"), "sin-dominio");
    assert_eq!(
        file_classification("H/providers.env"),
        "se-queda-como-archivo"
    );
    assert_eq!(file_classification("H/md2tg.py"), "resto");
}
