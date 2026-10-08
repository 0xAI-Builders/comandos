#[path = "support/python.rs"]
mod python;
use comandos_runtime::{
    capabilities::{self, Paths},
    session_profiles,
};
use serde_json::{Value, json};
#[test]
fn empty_inventory_and_launch_contract() {
    let root =
        std::env::temp_dir().join(format!("comandos-profile-runtime-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let paths = Paths::new(&root, &root);
    let registry = json!({"harnesses":{"shell":{},"codex":{"defaultHome":root.join(".codex"),"accountsRoot":root.join("accounts"),"authFile":"auth.json","accountEnv":"CODEX_HOME","capabilities":{"accounts":true}}}});
    let script = r#"import sys,json,pathlib
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'));sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'bin'))
import session_profiles as p,capabilities as c
r=json.loads(sys.argv[2]);cwd=sys.argv[3]
print(json.dumps([c.session_capabilities(r,'shell','main',cwd),p.inventory(r,'shell','main',cwd),p.launch_draft({'id':'p','harness':'codex'}),p.launch_args({'id':'p','harness':'codex'},r,cwd,cwd,dry_run=True)],ensure_ascii=False))"#;
    let text = registry.to_string();
    let oracle = python::run_python(script, &[text.as_ref(), root.as_os_str()], &root).unwrap();
    let actual = json!([
        capabilities::session_capabilities(&registry, "shell", "main", &root, &paths).unwrap(),
        session_profiles::inventory(&registry, "shell", "main", &root, &paths).unwrap(),
        session_profiles::launch_draft(&json!({"id":"p","harness":"codex"})).unwrap(),
        session_profiles::launch_args(
            &json!({"id":"p","harness":"codex"}),
            &registry,
            &root,
            &root,
            true,
            &paths
        )
        .unwrap()
    ]);
    assert_eq!(actual, serde_json::from_str::<Value>(&oracle).unwrap());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn all_harness_inventories_plugins_errors_and_launch_match_python() {
    let root = std::env::temp_dir().join(format!("comandos-profile-all-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let project = root.join("project");
    std::fs::create_dir_all(project.join(".git")).unwrap();
    let write = |p: &str, s: &str| {
        let p = root.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    };
    write(
        ".codex/config.toml",
        "[mcp_servers.chrome-bg]\ndescription=' Custom   description '\nenabled=true\n[skills]\ninclude_instructions=false\n[[skills.config]]\nname='sample'\nenabled=false\n[plugins.'demo@market']\nenabled=true\n",
    );
    write(
        ".codex/skills/sample/SKILL.md",
        "---\nname: sample\ndescription: |\n  Multi line\n  description\n---\nbody",
    );
    write(
        ".codex/plugins/cache/market/demo/default/.codex-plugin/plugin.json",
        r#"{"name":"demo","skills":"./skills","mcpServers":{"plug":{"command":"never-run"}}}"#,
    );
    write(
        ".codex/plugins/cache/market/demo/default/skills/s/SKILL.md",
        "---\nname: plugskill\n---\n",
    );
    write(
        "project/.codex/config.toml",
        "[mcp_servers.chrome-bg]\ncommand='never-run'\n[skills]\nconfig=[]\n",
    );
    write(
        ".claude.json",
        r#"{"mcpServers":{"user":{"command":"never-run"}},"projects":{}}"#,
    );
    write(
        ".claude/settings.json",
        r#"{"enabledPlugins":{},"disabledMcpjsonServers":["project"]}"#,
    );
    write(
        ".claude/skills/sample/SKILL.md",
        "---\nname: duplicate\ndescription: description\ndisable-model-invocation: true\n---\n",
    );
    write(
        "project/.claude/skills/sample/SKILL.md",
        "---\nname: duplicate\n---\n",
    );
    write(
        "project/.mcp.json",
        r#"{"mcpServers":{"project":{"command":"never-run"}}}"#,
    );
    write(
        ".grok/config.toml",
        "[compat.claude]\nmcps=false\nskills=false\n[compat.cursor]\nmcps=false\nskills=false\n[mcp_servers.grok]\nenabled=true\n",
    );
    write(
        ".config/opencode/opencode.jsonc",
        "{/*comment*/\"mcp\":{\"oc\":{\"enabled\":false,},},}",
    );
    write(
        ".gemini/settings.json",
        r#"{"mcpServers":{"gem":{"command":"never-run"}},"mcp":{"excluded":["gem"]}}"#,
    );
    write(
        ".gemini/extensions/demo/gemini-extension.json",
        r#"{"name":"demo","mcpServers":{"ext":{"command":"never-run"}}}"#,
    );
    write(
        ".gemini/extensions/extension-enablement.json",
        r#"{"demo":{"overrides":["!*"]}}"#,
    );
    write(
        ".gemini/config/mcp_config.json",
        r#"{"mcpServers":{"agy":{"enabled":true}}}"#,
    );
    let mut harnesses = serde_json::Map::new();
    for h in ["codex", "claude", "grok"] {
        harnesses.insert(h.into(),json!({"defaultHome":root.join(format!(".{h}")),"accountsRoot":root.join(format!("accounts/{h}")),"authFile":"auth.json","accountEnv":"ENV","capabilities":{"accounts":true}}));
    }
    for h in ["opencode", "gemini", "agy", "acp", "shell", "custom"] {
        harnesses.insert(h.into(), json!({}));
    }
    let registry = json!({"harnesses":harnesses});
    let text = registry.to_string();
    let paths = Paths::new(&root, &project);
    let script = r#"import sys,json,pathlib
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'));sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'bin'))
import session_profiles as p,capabilities as c
r=json.loads(sys.argv[2]);cwd=sys.argv[3];h=sys.argv[4]
print(json.dumps([c.session_capabilities(r,h,'main',cwd),p.inventory(r,h,'main',cwd)],ensure_ascii=False))"#;
    for h in [
        "codex", "claude", "grok", "opencode", "gemini", "agy", "acp", "shell", "custom",
    ] {
        let oracle = python::run_python(
            script,
            &[text.as_ref(), project.as_os_str(), h.as_ref()],
            &root,
        )
        .unwrap();
        let actual = json!([
            capabilities::session_capabilities(&registry, h, "main", &project, &paths).unwrap(),
            session_profiles::inventory(&registry, h, "main", &project, &paths).unwrap()
        ]);
        assert_eq!(
            actual,
            serde_json::from_str::<Value>(&oracle).unwrap(),
            "{h}"
        );
    }
    let inv = session_profiles::inventory(&registry, "codex", "main", &project, &paths).unwrap();
    let id = inv["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "sample")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let profiles = json!([{"id":"p","harness":"codex","skills":{id:true},"mcps":{"chrome-bg":false}},{"id":"p","harness":"claude","mcps":{"user":false}},{"id":"p","harness":"codex","skills":{"missing":false}},{"id":"p","harness":"grok","skills":{"s":true}}]);
    let ptext = profiles.to_string();
    let script = r#"import sys,json,pathlib
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'));sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'bin'))
import session_profiles as p
r=json.loads(sys.argv[2]);cwd=sys.argv[3];out=[]
for profile in json.loads(sys.argv[4]):
 try:out.append({'ok':p.launch_args(profile,r,cwd,cwd,dry_run=True)})
 except ValueError as e:out.append({'error':str(e)})
print(json.dumps(out,ensure_ascii=False))"#;
    let oracle = python::run_python(
        script,
        &[text.as_ref(), project.as_os_str(), ptext.as_ref()],
        &root,
    )
    .unwrap();
    let actual: Vec<_> = profiles
        .as_array()
        .unwrap()
        .iter()
        .map(
            |p| match session_profiles::launch_args(p, &registry, &project, &root, true, &paths) {
                Ok(v) => json!({"ok":v}),
                Err(e) => json!({"error":e.to_string()}),
            },
        )
        .collect();
    assert_eq!(
        json!(actual),
        serde_json::from_str::<Value>(&oracle).unwrap()
    );
    write(".codex/config.toml", "invalid=");
    let oracle = python::run_python(
        script,
        &[text.as_ref(), project.as_os_str(), ptext.as_ref()],
        &root,
    )
    .unwrap();
    let actual: Vec<_> = profiles
        .as_array()
        .unwrap()
        .iter()
        .map(
            |p| match session_profiles::launch_args(p, &registry, &project, &root, true, &paths) {
                Ok(v) => json!({"ok":v}),
                Err(e) => json!({"error":e.to_string()}),
            },
        )
        .collect();
    assert_eq!(
        json!(actual),
        serde_json::from_str::<Value>(&oracle).unwrap()
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn configuration_reader_matches_python_jsonc_toml_and_bounds() {
    let root = std::env::temp_dir().join(format!("comandos-profile-reader-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let cases = [
        (
            "x.jsonc",
            "{\"url\":\"https://example/a//b\", /*x*/ \"mcp\":{},}",
        ),
        ("x.json", "[]"),
        (
            "x.toml",
            "irrelevant='secret'\n[skills]\npaths=['a']\n[[skills.config]]\npath='foo'\nenabled=true\nextra='x'\n[mcp_servers.s]\ncommand='never-run'\ndescription='hello'\nenabled=false\n",
        ),
        ("x.toml", "[mcp_servers.s]\ndescription=1979-05-27\n"),
        (
            "x.toml",
            "irrelevant=999999999999999999999999999999\n[mcp_servers.s]\nenabled=0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF\ndescription=1e400\n",
        ),
        (
            "x.toml",
            "[mcp_servers.s]\nenabled=1__0000000000000000000000\n",
        ),
        (
            "x.toml",
            "[mcp_servers.s]\ndescription=1979-05-27T07:32:00.123456789Z\n",
        ),
        ("x.toml", "[mcp_servers.s]\ndescription=1979-02-30\n"),
        ("x.toml", "[mcp_servers.s]\ndescription=0000-01-01\n"),
    ];
    let script = r#"import sys,json,pathlib
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'));import capabilities as c
errors=[];v=c.read_config(sys.argv[2],errors,'test');print(json.dumps([v,errors],ensure_ascii=False,default=str))"#;
    for (name, text) in cases {
        let p = root.join(name);
        std::fs::write(&p, text).unwrap();
        let oracle = python::run_python(script, &[p.as_os_str()], &root).unwrap();
        let mut errors = vec![];
        let v = capabilities::read_config(&p, &mut errors, "test");
        assert_eq!(
            Value::Array(vec![v, Value::Array(errors)]),
            comandos_core::json::workspace_loads(&oracle).unwrap(),
            "{text}"
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn plugin_scopes_versions_missing_files_and_path_guards_match_python() {
    let root =
        std::env::temp_dir().join(format!("comandos-profile-plugins-{}", std::process::id()));
    let project = root.join("project");
    std::fs::create_dir_all(project.join(".git")).unwrap();
    let write = |p: &str, s: &str| {
        let p = root.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    };
    write(
        ".codex/config.toml",
        "[plugins.'demo@market']\nenabled=true\n[plugins.'missing@market']\nenabled=true\n[plugins.'bad!@market']\nenabled=false\n",
    );
    for version in ["2", "10", "999999999999999999999999999999999999"] {
        write(&format!(".codex/plugins/cache/market/demo/{version}/.codex-plugin/plugin.json"),&json!({"name":format!("v{version}"),"skills":["../../../../../../outside"],"mcpServers":["../../../../../../outside.json"]}).to_string());
    }
    write(
        ".claude/settings.json",
        r#"{"enabledPlugins":{"demo@market":true}}"#,
    );
    write(
        "claude-plugin/.claude-plugin/plugin.json",
        r#"{"name":"claude-demo","skills":"skills","mcpServers":{"mcpServers":{"srv":{"enabled":true,"description":"plugin description"}}}}"#,
    );
    write("claude-plugin/skills/s/SKILL.md", "---\nname: demo\n---\n");
    write(".claude/plugins/installed_plugins.json",&json!({"plugins":{"demo@market":[{"scope":"user","installPath":root.join("claude-plugin")},{"scope":"project","projectPath":"/other","installPath":root.join("wrong-plugin")}]}}).to_string());
    write(
        ".grok/config.toml",
        &format!(
            "[compat.claude]\nmcps=false\nskills=false\n[compat.cursor]\nmcps=false\nskills=false\n[plugins]\npaths=[{}]\ndisabled=['grok-demo']\n",
            serde_json::to_string(&root.join("grok-plugin")).unwrap()
        ),
    );
    write(
        "grok-plugin/.grok-plugin/plugin.json",
        r#"{"name":"grok-demo","mcpServers":{"g":{"command":"never-run"}}}"#,
    );
    write("grok-plugin/skills/s/SKILL.md", "---\nname: demo\n---\n");
    let mut harnesses = serde_json::Map::new();
    for h in ["codex", "claude", "grok"] {
        harnesses.insert(h.into(),json!({"defaultHome":root.join(format!(".{h}")),"accountsRoot":root.join(format!("accounts/{h}")),"authFile":"auth.json","accountEnv":"ENV","capabilities":{"accounts":true}}));
    }
    let registry = json!({"harnesses":harnesses});
    let text = registry.to_string();
    let paths = Paths::new(&root, &project);
    let script = r#"import sys,json,pathlib
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'));sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'bin'))
import session_profiles as p,capabilities as c
r=json.loads(sys.argv[2]);cwd=sys.argv[3];h=sys.argv[4]
print(json.dumps([c.session_capabilities(r,h,'main',cwd),p.inventory(r,h,'main',cwd)],ensure_ascii=False))"#;
    for h in ["codex", "claude", "grok"] {
        let oracle = python::run_python(
            script,
            &[text.as_ref(), project.as_os_str(), h.as_ref()],
            &root,
        )
        .unwrap();
        let actual = json!([
            capabilities::session_capabilities(&registry, h, "main", &project, &paths).unwrap(),
            session_profiles::inventory(&registry, h, "main", &project, &paths).unwrap()
        ]);
        assert_eq!(
            actual,
            serde_json::from_str::<Value>(&oracle).unwrap(),
            "{h}"
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn yaml_implicit_scalars_and_quoted_names_match_python() {
    let root = std::env::temp_dir().join(format!("comandos-profile-yaml-{}", std::process::id()));
    let project = root.join("project");
    std::fs::create_dir_all(project.join(".git")).unwrap();
    let home = root.join(".claude");
    std::fs::create_dir_all(&home).unwrap();
    for (name, front) in [
        (
            "flow",
            "{name: no, description: yes, disable-model-invocation: ON}",
        ),
        ("alias", "value: &v no\nname: *v\ndescription: *v"),
        (
            "merge",
            "defaults: &d {name: merged, description: text}\n<<: *d\nname: override",
        ),
        (
            "integer",
            "name: 0123\ndescription: 1:20\ndisable-model-invocation: 0x1",
        ),
        (
            "huge",
            "name: huge\nunknown: 99999999999999999999999999999999999999999999999999999999",
        ),
        (
            "bool",
            "name: no\ndescription: yes\ndisable-model-invocation: ON",
        ),
        ("quoted", "name: 'no'\ndescription: 'yes'"),
        ("date", "name: 2026-10-05\ndescription: 2026-10-05"),
        ("duplicate", "name: first\nname: last"),
        (
            "folded",
            "name: folded\ndescription: >\n  first line\n  second line",
        ),
    ] {
        let p = home.join("skills").join(name).join("SKILL.md");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, format!("---\n{front}\n---\n")).unwrap();
    }
    let cr = home.join("skills/cr/SKILL.md");
    std::fs::create_dir_all(cr.parent().unwrap()).unwrap();
    std::fs::write(cr, "---\rname: customcr\rdescription: X\r---\r").unwrap();
    let boundary = home.join("skills/boundary/SKILL.md");
    std::fs::create_dir_all(boundary.parent().unwrap()).unwrap();
    std::fs::write(
        boundary,
        format!(
            "---\r\nname: customboundary\r\n{}description: X\r\n---\r\n",
            "\r\n".repeat(8000)
        ),
    )
    .unwrap();
    let registry = json!({"harnesses":{"claude":{"defaultHome":home,"accountsRoot":root.join("accounts"),"authFile":"auth.json","accountEnv":"ENV","capabilities":{"accounts":true}}}});
    let text = registry.to_string();
    let paths = Paths::new(&root, &project);
    let script = r#"import sys,json,pathlib
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'));sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'bin'))
import session_profiles as p
print(json.dumps(p.inventory(json.loads(sys.argv[2]),'claude','main',sys.argv[3]),ensure_ascii=False))"#;
    let oracle = python::run_python(script, &[text.as_ref(), project.as_os_str()], &root).unwrap();
    assert_eq!(
        session_profiles::inventory(&registry, "claude", "main", &project, &paths).unwrap(),
        serde_json::from_str::<Value>(&oracle).unwrap()
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn claude_private_launch_preserves_shared_files_and_rejects_unsafe_selection() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("comandos-profile-launch-{}", std::process::id()));
    let project = root.join("project");
    std::fs::create_dir_all(project.join(".git")).unwrap();
    let home = root.join(".claude");
    std::fs::create_dir_all(&home).unwrap();
    let config = root.join(".claude.json");
    let original =
        r#"{"mcpServers":{"keep":{"command":"never-run"},"off":{"command":"never-run"}}}"#;
    std::fs::write(&config, original).unwrap();
    let registry = json!({"harnesses":{"claude":{"defaultHome":home,"accountsRoot":root.join("accounts"),"authFile":"auth.json","accountEnv":"ENV","capabilities":{"accounts":true}}}});
    let paths = Paths::new(&root, &project);
    let profile = json!({"id":"p","harness":"claude","mcps":{"off":false}});
    let runtime = root.join("launches");
    let dry = session_profiles::launch_args(&profile, &registry, &project, &runtime, true, &paths)
        .unwrap();
    assert_eq!(
        dry,
        [
            "--strict-mcp-config",
            "--mcp-config",
            "<private-launch-config>"
        ]
    );
    assert!(!runtime.exists());
    let args =
        session_profiles::launch_args(&profile, &registry, &project, &runtime, false, &paths)
            .unwrap();
    let private = std::path::Path::new(&args[2]);
    assert!(private.starts_with(&runtime));
    assert_eq!(
        std::fs::metadata(private).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&runtime).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(private).unwrap()).unwrap(),
        json!({"mcpServers":{"keep":{"command":"never-run"}}})
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    for spec in [
        json!({"command":"never-run ${SECRET}"}),
        json!({"command":"never-run","cwd":"relative"}),
    ] {
        std::fs::write(
            &config,
            json!({"mcpServers":{"keep":spec,"off":{"command":"never-run"}}}).to_string(),
        )
        .unwrap();
        assert_eq!(
            session_profiles::launch_args(&profile, &registry, &project, &runtime, true, &paths)
                .unwrap_err()
                .to_string(),
            "MCP con expansión o cwd relativo: selección aislada no soportada"
        );
    }
    assert_eq!(std::fs::read_dir(&runtime).unwrap().count(), 1);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn bounded_regular_configuration_reads_and_utf8_paths() {
    use std::os::unix::ffi::OsStrExt;
    let root = std::env::temp_dir().join(format!("comandos-profile-bounds-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut errors = vec![];
    let fifo = root.join("fifo.json");
    nix::unistd::mkfifo(
        &fifo,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    assert_eq!(
        capabilities::read_config(&fifo, &mut errors, "fifo"),
        json!({})
    );
    assert_eq!(
        errors,
        json!([{"source":"fifo","code":"configuration_unreadable"}])
            .as_array()
            .unwrap()
            .clone()
    );
    let big = root.join("big.json");
    std::fs::write(&big, vec![b' '; 2 * 1024 * 1024 + 1]).unwrap();
    assert_eq!(
        capabilities::read_config(&big, &mut errors, "big"),
        json!({})
    );
    assert_eq!(errors.len(), 2);
    let bad = root.join(std::ffi::OsStr::from_bytes(b"invalid-\xff"));
    std::fs::create_dir_all(&bad).unwrap();
    let paths = Paths::new(&root, &root);
    assert!(matches!(
        capabilities::configuration(
            &json!({"harnesses":{"shell":{}}}),
            "shell",
            "main",
            &bad,
            &paths
        ),
        Err(capabilities::Fault::Uncertain(_))
    ));
    std::fs::remove_dir_all(root).unwrap();
}
