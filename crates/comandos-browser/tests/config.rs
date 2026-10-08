use comandos_browser::config::BrokerConfig;
use serde_json::json;

#[test]
fn defaults_clamping_and_catalog_names_match_broker() {
    let base = std::env::temp_dir();
    let config = |max| json!({"command":["node","mcp.js"],"state_dir":base,"catalog":{"tools":[{"name":"navigate_page"},{"name":"navigate_page"}]},"max_workers":max});
    for (raw, want) in [(0, 1), (1, 1), (2, 2), (5, 2), (-2, 1)] {
        let c = BrokerConfig::from_value(config(raw), &base).unwrap();
        assert_eq!((c.port, c.max_workers), (19441, want));
        assert_eq!(
            (
                c.idle_seconds,
                c.tool_timeout,
                c.queue_timeout,
                c.stop_grace
            ),
            (300.0, 120.0, 0.0, 7.0)
        );
        assert_eq!(c.tools.len(), 1);
        assert!(c.tools.contains("navigate_page"));
        assert!(c.python_status.is_none());
    }
}

#[test]
fn load_resolves_catalog_relative_to_config_without_creating_state() {
    let dir = std::env::temp_dir().join(format!("comandos-browser-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("catalog.json"),
        r#"{"tools":[{"name":"take_snapshot"}]}"#,
    )
    .unwrap();
    let state = dir.join("not-created");
    let value = json!({"command":["node"],"env":{"A":"b"},"state_dir":state,"catalog":"catalog.json","port":19442,"python_status":"old/status.json"});
    std::fs::write(dir.join("config.json"), value.to_string()).unwrap();
    let c = BrokerConfig::load(&dir.join("config.json")).unwrap();
    assert_eq!(c.port, 19442);
    assert!(c.tools.contains("take_snapshot"));
    assert_eq!(c.env, vec![("A".into(), "b".into())]);
    assert!(!state.exists());
    assert_eq!(c.python_status, Some(dir.join("old/status.json")));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn malformed_process_and_timeout_config_fails_before_any_spawn() {
    let base = std::env::temp_dir();
    for (key, bad) in [
        ("command", json!([])),
        ("command", json!(["x", 3])),
        ("env", json!({"A":3})),
        ("catalog", json!({"tools":[{}]})),
        ("port", json!(65536)),
        ("state_dir", json!(null)),
        ("tool_timeout", json!("oops")),
    ] {
        let mut value = json!({"command":["x"],"state_dir":base,"catalog":{"tools":[]}});
        value[key] = bad;
        assert!(BrokerConfig::from_value(value, &base).is_err(), "{key}");
    }
}
