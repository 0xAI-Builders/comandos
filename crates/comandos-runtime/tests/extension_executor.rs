use comandos_runtime::extension_launch::{self as launch, executor};
use serde_json::json;
use std::{collections::HashMap, ffi::OsString};
#[test]
fn resolution_keeps_home_and_applies_environment_prefix_without_execution() {
    let data = json!({"home":"/private/home","bundle":{"harness":"codex"},"args":["--resume","two words"],"env":{"CODEX_HOME":"/private/account"}});
    let env = HashMap::from([
        (OsString::from("HOME"), OsString::from("/private/home")),
        ("DROP".into(), "secret-canary".into()),
    ]);
    let argv = ["env", "-u", "DROP", "ASSIGN=a b", "codex", "--yolo"].map(str::to_owned);
    let got = executor::resolve_command(&data, &argv, &env).unwrap();
    assert_eq!(got.argv, ["codex", "--yolo", "--resume", "two words"]);
    assert_eq!(
        got.environment.get(&OsString::from("HOME")),
        Some(&OsString::from("/private/home"))
    );
    assert!(!got.environment.contains_key(&OsString::from("DROP")));
    assert_eq!(
        got.environment.get(&OsString::from("ASSIGN")),
        Some(&OsString::from("a b"))
    );
    assert_eq!(
        launch::helper_for_home(std::path::Path::new("/private/home")),
        std::path::Path::new("/private/home/.local/bin/cc-extension-session")
    );
    assert!(
        executor::resolve_command(&data, &["HOME=/elsewhere".into(), "codex".into()], &env)
            .is_err()
    );
}

#[test]
fn opencode_merge_keeps_unmanaged_settings_and_selected_rules_win() {
    use std::os::unix::ffi::OsStringExt;
    let data = json!({"home":"/private/home","bundle":{"harness":"opencode"},"args":[],"env":{"OPENCODE_CONFIG_CONTENT":r#"{"permission":{"skill":{"demo":"deny"}},"mcp":{"new":{"enabled":false}}}"#}});
    let bytes = OsString::from_vec(vec![0xff, b'x']);
    let env = HashMap::from([
        ("HOME".into(), "/private/home".into()),
        (
            "OPENCODE_CONFIG_CONTENT".into(),
            r#"{"permission":"allow","theme":"dark","mcp":{"old":{"command":["do-not-execute"]}}}"#
                .into(),
        ),
        ("UNMANAGED_BYTES".into(), bytes.clone()),
    ]);
    let got = executor::resolve_command(&data, &["opencode".into()], &env).unwrap();
    let content: serde_json::Value = serde_json::from_str(
        got.environment
            .get(&OsString::from("OPENCODE_CONFIG_CONTENT"))
            .unwrap()
            .to_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        content,
        json!({"permission":{"*":"allow","skill":{"demo":"deny"}},"theme":"dark","mcp":{"old":{"command":["do-not-execute"]},"new":{"enabled":false}}})
    );
    assert_eq!(
        got.environment.get(&OsString::from("UNMANAGED_BYTES")),
        Some(&bytes)
    );
}
#[test]
fn codex_previous_rules_precede_selected_rules_even_when_count_is_zero() {
    let env = HashMap::from([("HOME".into(), "/private/home".into())]);
    for count in [0, 1, 20] {
        let rules = r#"skills.config=[{path="base",enabled=true},{path="selected",enabled=false}]"#;
        let data = json!({"home":"/private/home","bundle":{"harness":"codex"},"args":["-c",rules],"env":{},"codexSelectedSkillCount":count});
        let got = executor::resolve_command(
            &data,
            &[
                "codex".into(),
                "--config=skills.config=[{path=\"previous\",enabled=true}]".into(),
            ],
            &env,
        )
        .unwrap();
        let parsed = launch::parse_toml(got.argv.last().unwrap())
            .unwrap()
            .unwrap();
        let paths: Vec<_> = parsed["skills"]["config"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            match count {
                0 => vec!["base", "selected", "previous"],
                1 => vec!["base", "previous", "selected"],
                _ => vec!["previous", "base", "selected"],
            }
        );
    }
}
#[test]
fn missing_or_script_helper_declines_before_private_prepare_effects() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let home = std::env::temp_dir().join(comandos_runtime::fresh_id("native-admission").unwrap());
    fs::create_dir_all(&home).unwrap();
    let paths = comandos_runtime::capabilities::Paths::new(&home, &home);
    let runtime = home.join("must-not-exist");
    let registry = json!({"harnesses":{"codex":{"defaultHome":home.join(".codex")}}});
    assert!(
        launch::prepare_launch(
            &registry,
            "codex",
            "main",
            &home,
            &json!({}),
            &runtime,
            "admission-test",
            &paths
        )
        .is_err()
    );
    assert!(!runtime.exists());
    let native = home.join(".local/share/comandos/bin/comandos");
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    fs::write(&native, b"#!/bin/sh\necho secret-canary\n").unwrap();
    fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
    let alias = launch::helper_for_home(&home);
    fs::create_dir_all(alias.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&native, &alias).unwrap();
    assert!(launch::require_helper(&home).is_err());
    assert!(
        launch::prepare_launch(
            &registry,
            "codex",
            "main",
            &home,
            &json!({}),
            &runtime,
            "admission-test",
            &paths
        )
        .is_err()
    );
    assert!(!runtime.exists());
    fs::remove_dir_all(home).unwrap();
}
