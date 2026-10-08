#[path = "support/python.rs"]
mod python;
use comandos_runtime::{capabilities::Paths, extension_launch};
use serde_json::{Value, json};
#[test]
fn inventories_match_python_serialized() {
    let root = std::env::temp_dir().join(format!(
        "comandos-extension-inventory-{}",
        std::process::id()
    ));
    let write = |p: &str, s: &str| {
        let p = root.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    };
    write(
        ".agents/skills/shared/SKILL.md",
        "---\nname: shared\n---\nhello",
    );
    write(
        ".agents/.skill-lock.json",
        r#"{"skills":{"shared":{"sourceType":"github","source":"demo/repo"}}}"#,
    );
    write(
        ".config/comandos/extensions/catalog.json",
        r#"{"servers":{"shared":{"command":"never-run"},"disabled":{"enabled":false}}}"#,
    );
    write(
        ".codex/config.toml",
        "[mcp_servers.native]\ncommand='never-run'\n[mcp_servers.'dotted.name']\nenabled=true\n",
    );
    write(
        ".claude/settings.json",
        r#"{"skillOverrides":{"shared":"off"}}"#,
    );
    write(".grok/config.toml", "disabled_mcp_servers=['shared']\n");
    write(
        ".config/opencode/opencode.json",
        r#"{"permission":{"skill":"deny"}}"#,
    );
    write(".gemini/config/mcp_config.json", "{}");
    write(
        ".codex/plugins/cache/market/demo/default/.codex-plugin/plugin.json",
        r#"{"name":"demo","skills":"./skills","mcpServers":{"plugin":{"command":"never-run"}}}"#,
    );
    write(
        ".codex/plugins/cache/market/demo/default/skills/plugin/SKILL.md",
        "---\nname: plugin\n---\nhello",
    );
    write(".claude/plugins/installed_plugins.json", &json!({"plugins":{"demo@market":[{"scope":"user","installPath":root.join("claude-plugin")}]}}).to_string());
    write(
        "claude-plugin/.claude-plugin/plugin.json",
        r#"{"name":"demo","skills":"./skills","mcpServers":{"plug":{"command":"never-run"}}}"#,
    );
    write(
        "claude-plugin/skills/plugin/SKILL.md",
        "---\nname: plugin\n---\nhello",
    );
    write(
        ".claude/settings.json",
        r#"{"skillOverrides":{"shared":"off"},"enabledPlugins":{"demo@market":true}}"#,
    );
    let shared_spec = json!({"command":"never-run"});
    let name_hash = comandos_extensions::python_json::digest(&json!("shared")).unwrap();
    write(&format!(".local/state/comandos/extensions/sizes/{name_hash}.json"), &json!({"tokens":42,"tokenizer":"cl100k_base","basis":"tool-definitions","configuration":comandos_extensions::python_json::digest(&shared_spec).unwrap(),"content":"synthetic","measuredAt":comandos_extensions::metadata::now().floor() as i64}).to_string());
    let registry = json!({"harnesses":{"codex":{"defaultHome":root.join(".codex")},"claude":{"defaultHome":root.join(".claude")},"grok":{"defaultHome":root.join(".grok")},"opencode":{},"agy":{}}});
    let paths = Paths::new(&root, &root);
    let script = r#"import sys,json,pathlib
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'));sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'bin'))
import extension_launch as e
r=json.loads(sys.argv[2]); cwd=sys.argv[3]
print(json.dumps([e._internal_inventory(r,sys.argv[4],'main',cwd)[1],e.inventory(r,sys.argv[4],'main',cwd)],ensure_ascii=False,separators=(',',':')))"#;
    for h in ["codex", "claude", "grok", "opencode", "agy"] {
        let oracle = python::run_python(
            script,
            &[registry.to_string().as_ref(), root.as_os_str(), h.as_ref()],
            &root,
        )
        .unwrap();
        let actual = json!([
            extension_launch::internal_inventory(&registry, h, "main", &root, &paths)
                .unwrap()
                .1,
            extension_launch::inventory(&registry, h, "main", &root, &paths).unwrap()
        ]);
        let expected: Value = serde_json::from_str(&oracle).unwrap();
        assert_eq!(actual.to_string(), expected.to_string(), "{h}");
    }
    assert!(extension_launch::inventory(&registry, "shell", "main", &root, &paths).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn incomplete_and_unreadable_catalog_contracts() {
    let root = std::env::temp_dir().join(format!(
        "comandos-extension-incomplete-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join(".codex/skills/demo")).unwrap();
    std::fs::write(
        root.join(".codex/skills/demo/SKILL.md"),
        "---\nname: demo\n---\n",
    )
    .unwrap();
    std::fs::write(root.join(".codex/config.toml"), "bad=[").unwrap();
    let registry = json!({"harnesses":{"codex":{"defaultHome":root.join(".codex")}}});
    let paths = Paths::new(&root, &root);
    let script = r#"import sys,json,pathlib
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'));sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'bin'))
import extension_launch as e
r=json.loads(sys.argv[2]);cwd=sys.argv[3]
try: result=e.inventory(r,'codex','main',cwd)
except ValueError as err: result={'error':str(err)}
print(json.dumps(result,ensure_ascii=False))"#;
    for broken_catalog in [false, true] {
        if broken_catalog {
            std::fs::create_dir_all(root.join(".config/comandos/extensions")).unwrap();
            std::fs::write(root.join(".config/comandos/extensions/catalog.json"), "bad").unwrap();
        }
        let expected: Value = serde_json::from_str(
            &python::run_python(
                script,
                &[registry.to_string().as_ref(), root.as_os_str()],
                &root,
            )
            .unwrap(),
        )
        .unwrap();
        let actual = match extension_launch::inventory(&registry, "codex", "main", &root, &paths) {
            Ok(v) => v,
            Err(extension_launch::LaunchError::Value(s)) => json!({"error":s}),
            Err(e) => panic!("{e:?}"),
        };
        assert_eq!(actual.to_string(), expected.to_string());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn synchronous_offline_counter_preserves_verified_fixture_and_corrupt_fallback() {
    let cache = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.migration-build/public-token-cache");
    let fixture: Value = serde_json::from_str(include_str!(
        "../../comandos-extensions/tests/fixtures/metadata.json"
    ))
    .unwrap();
    let rows = fixture["texts"].as_array().unwrap();
    let texts: Vec<String> = rows
        .iter()
        .map(|r| r["text"].as_str().unwrap().to_owned())
        .collect();
    let expected: Vec<Option<u64>> = rows.iter().map(|r| r["tokens"].as_u64()).collect();
    assert_eq!(
        comandos_extensions::tokenizer::offline_counts_at(&cache, &texts),
        Some(expected)
    );
    let root =
        std::env::temp_dir().join(format!("comandos-offline-corrupt-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join(comandos_extensions::tokenizer::ENCODING_FILE),
        "invalid",
    )
    .unwrap();
    assert_eq!(
        comandos_extensions::tokenizer::offline_counts_at(&root, &texts),
        None
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn inventory_retains_only_bounded_metadata_counts_across_calls() {
    let root = std::env::temp_dir().join(format!(
        "comandos-inventory-count-cache-{}",
        std::process::id()
    ));
    let skill = root.join(".agents/skills/demo/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(&skill, "hello").unwrap();
    let cache = root.join(".cache/comandos/tiktoken");
    std::fs::create_dir_all(&cache).unwrap();
    let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.migration-build/public-token-cache")
        .join(comandos_extensions::tokenizer::ENCODING_FILE);
    let encoding = cache.join(comandos_extensions::tokenizer::ENCODING_FILE);
    std::fs::copy(source, &encoding).unwrap();
    let paths = Paths::new(&root, &root);
    let registry = json!({"harnesses":{"codex":{"defaultHome":root.join(".codex")}}});
    let a = extension_launch::inventory(&registry, "codex", "main", &root, &paths).unwrap();
    assert_eq!(a["skills"][0]["size"]["tokens"], 1);
    std::fs::remove_file(encoding).unwrap();
    let b = extension_launch::inventory(&registry, "codex", "main", &root, &paths).unwrap();
    assert_eq!(a.to_string(), b.to_string());
    std::fs::write(skill, "hello new text").unwrap();
    let c = extension_launch::inventory(&registry, "codex", "main", &root, &paths).unwrap();
    assert!(c["skills"][0]["size"]["tokens"].is_null());
    std::fs::remove_dir_all(root).unwrap();
}
