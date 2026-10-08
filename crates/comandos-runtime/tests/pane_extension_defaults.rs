use comandos_runtime::pane_extensions::canonical_selection;
use serde_json::json;

#[test]
fn new_catalog_entries_inherit_defaults_without_resetting_session_exclusions() {
    let inventory = json!({
        "mcps":[
            {"id":"new","enabled":true,"toggleable":true},
            {"id":"excluded","enabled":true,"toggleable":true},
            {"id":"global-off","enabled":false,"toggleable":false},
            {"id":"unknown","enabled":null,"toggleable":false}
        ],
        "skills":[{"id":"shared:new","enabled":true,"toggleable":true}]
    });
    let desired = json!({"mcps":{"excluded":false,"global-off":true,"removed":true},"skills":{}});
    assert_eq!(
        canonical_selection(&inventory, &desired),
        json!({
            "mcps":{"new":true,"excluded":false,"global-off":false},
            "skills":{"shared:new":true}
        })
    );
}

#[test]
fn global_off_then_on_preserves_only_the_previous_pane_preference() {
    use comandos_runtime::pane_extensions::gate_selection;
    let off = json!({"mcps":[{"id":"mail","enabled":false,"toggleable":false},{"id":"other","enabled":true,"toggleable":true}],"skills":[]});
    let desired = json!({"mcps":{"mail":false,"other":false},"skills":{}});
    let inherited = gate_selection(&off, &desired, &json!({}));
    assert_eq!(inherited, json!({"other":false}));
    let excluded = gate_selection(&off, &desired, &json!({"mail":false}));
    assert_eq!(excluded, json!({"mail":false,"other":false}));
    let on = json!({"mcps":[{"id":"mail","enabled":true,"toggleable":true},{"id":"other","enabled":true,"toggleable":true}],"skills":[]});
    assert_eq!(
        canonical_selection(&on, &json!({"mcps":inherited}))["mcps"]["mail"],
        true
    );
    assert_eq!(
        canonical_selection(&on, &json!({"mcps":excluded}))["mcps"]["mail"],
        false
    );
}

#[test]
fn failed_gate_publication_rolls_back_the_draft_and_stale_writes_never_publish() {
    use comandos_runtime::pane_extensions::{ExtensionStore, Fault};
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let store = ExtensionStore::new(&conn, &|| 1000.0).unwrap();
    let defaults = json!({"mcps":{"mail":true},"skills":{}});
    let draft = store
        .state("pane", "conversation", "codex", &defaults)
        .unwrap();
    let key = draft["key"].as_str().unwrap();
    let changed = json!({"mcps":{"mail":false},"skills":{}});
    assert!(
        store
            .save_with(key, &json!(0), &changed, || Err(Fault::Persistence(
                "disk full".into()
            )))
            .is_err()
    );
    assert_eq!(
        store.require_revision(key, &json!(0)).unwrap()["desired"],
        defaults
    );
    store
        .save_with(key, &json!(0), &changed, || Ok(()))
        .unwrap();
    let mut called = false;
    assert!(
        store
            .save_with(key, &json!(0), &defaults, || {
                called = true;
                Ok(())
            })
            .is_err()
    );
    assert!(!called);
}

#[test]
fn shared_catalog_defaults_override_stale_cli_defaults_but_keep_pane_exclusions() {
    use comandos_runtime::{capabilities::Paths, extension_launch};
    let home = std::env::temp_dir().join(comandos_runtime::fresh_id("catalog-defaults").unwrap());
    let write = |relative: &str, bytes: &str| {
        let path = home.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    };
    write(
        ".codex/config.toml",
        "[mcp_servers.managed]\ncommand='old-private-server'\nenabled=false\n[mcp_servers.global_off]\ncommand='old-private-server'\nenabled=true\n",
    );
    write(
        ".claude/settings.json",
        r#"{"disabledMcpjsonServers":["managed"]}"#,
    );
    write(
        ".claude.json",
        r#"{"mcpServers":{"managed":{"command":"old-private-server"},"global_off":{"command":"old-private-server"}}}"#,
    );
    write(
        ".config/comandos/extensions/catalog.json",
        r#"{"servers":{"managed":{"command":"shared-server"},"global_off":{"command":"shared-server","enabled":false}}}"#,
    );
    let registry = json!({"harnesses":{"codex":{"defaultHome":home.join(".codex")},"claude":{"defaultHome":home.join(".claude")}}});
    for harness in ["codex", "claude"] {
        let (_, inv) = extension_launch::internal_inventory(
            &registry,
            harness,
            "main",
            &home,
            &Paths::new(&home, &home),
        )
        .unwrap();
        let managed = inv["mcps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "managed")
            .unwrap();
        assert_eq!(managed["enabled"], true, "{harness}");
        assert_eq!(
            managed["synthetic"], true,
            "route through the catalog launcher: {harness}"
        );
        assert_eq!(
            extension_launch::normalize(&inv, &json!({})).unwrap()["mcps"],
            json!({"managed":true,"global_off":false})
        );
        let selected =
            extension_launch::normalize(&inv, &json!({"mcps":{"managed":false}})).unwrap();
        assert_eq!(selected["mcps"]["managed"], false);
        assert!(extension_launch::normalize(&inv, &json!({"mcps":{"global_off":true}})).is_err());
    }
    std::fs::remove_dir_all(home).unwrap();
}
