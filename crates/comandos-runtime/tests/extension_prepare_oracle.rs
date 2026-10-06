#[path = "support/python.rs"]
mod python;
use comandos_runtime::{capabilities::Paths, extension_launch as e};
use serde_json::{Value, json};
fn normalize(v: &mut Value, private: &str) {
    match v {
        Value::String(s) => *s = s.replace(private, "PRIVATE"),
        Value::Array(a) => {
            for v in a {
                normalize(v, private)
            }
        }
        Value::Object(o) => {
            let old = std::mem::take(o);
            for (k, mut v) in old {
                normalize(&mut v, private);
                o.insert(k.replace(private, "PRIVATE"), v);
            }
        }
        _ => {}
    }
}
#[test]
fn native_launch_artifacts_match_python_and_preserve_shared_files() {
    let root =
        std::env::temp_dir().join(format!("comandos-extension-prepare-{}", std::process::id()));
    let write = |p: &str, s: &str| {
        let p = root.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    };
    write(
        ".agents/skills/demo/SKILL.md",
        "---\nname: demo\n---\nhello",
    );
    write(
        ".codex/config.toml",
        "[mcp_servers.native]\ncommand='never-run'\n[[skills.config]]\nname='other'\nenabled=false\n",
    );
    write(".claude/settings.json", "{}");
    write(
        ".claude.json",
        r#"{"mcpServers":{"native":{"command":"never-run"}}}"#,
    );
    write(
        ".config/opencode/opencode.json",
        r#"{"mcp":{"native":{"type":"local","command":["never-run"]}}}"#,
    );
    write(
        ".config/comandos/extensions/catalog.json",
        r#"{"servers":{"shared":{"command":"never-run"},"disabled":{"enabled":false}}}"#,
    );
    let r = json!({"harnesses":{"codex":{"defaultHome":root.join(".codex")},"claude":{"defaultHome":root.join(".claude")},"opencode":{}}});
    let paths = Paths::new(&root, &root);
    let runtime = root.join("runtime");
    let script = r#"import sys,json,pathlib
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'));sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'bin'))
import extension_launch as e
b=e.prepare_launch(json.loads(sys.argv[2]),sys.argv[3],'main',sys.argv[4],json.loads(sys.argv[5]),sys.argv[6],'test-op')
p=pathlib.Path(b['manifest']);data=json.loads(p.read_text());print(json.dumps([b,data],ensure_ascii=False))"#;
    for h in ["codex", "claude", "opencode"] {
        let before = std::fs::read(root.join(".codex/config.toml")).unwrap();
        let inv = e::inventory(&r, h, "main", &root, &paths).unwrap();
        let skills: serde_json::Map<String, Value> = inv["skills"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["toggleable"] == true)
            .map(|r| (r["id"].as_str().unwrap().to_owned(), json!(false)))
            .collect();
        let selection = json!({"mcps":{"native":false,"shared":true},"skills":skills});
        let oracle: Value = serde_json::from_str(
            &python::run_python(
                script,
                &[
                    r.to_string().as_ref(),
                    h.as_ref(),
                    root.as_os_str(),
                    selection.to_string().as_ref(),
                    runtime.as_os_str(),
                ],
                &root,
            )
            .unwrap(),
        )
        .unwrap();
        let b = e::prepare_launch(
            &r, h, "main", &root, &selection, &runtime, "test-op", &paths,
        )
        .unwrap();
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(b["manifest"].as_str().unwrap()).unwrap())
                .unwrap();
        let mut actual = json!([b, manifest]);
        let mut expected = oracle;
        let ap = std::path::Path::new(actual[0]["manifest"].as_str().unwrap())
            .parent()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let bp = std::path::Path::new(expected[0]["manifest"].as_str().unwrap())
            .parent()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        normalize(&mut actual, &ap);
        normalize(&mut expected, &bp);
        assert_eq!(actual.to_string(), expected.to_string(), "{h}");
        assert_eq!(
            before,
            std::fs::read(root.join(".codex/config.toml")).unwrap()
        );
    }
    assert!(
        e::prepare_launch(
            &r,
            "codex",
            "main",
            &root,
            &json!({}),
            &runtime,
            "../bad",
            &paths
        )
        .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn namespace_materialization_matches_python_without_shared_configuration_writes() {
    let root =
        std::env::temp_dir().join(format!("comandos-extension-mounts-{}", std::process::id()));
    let write = |p: &str, s: &str| {
        let p = root.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    };
    write(
        ".grok/config.toml",
        "[mcp_servers.native]\ncommand='never-run'\n[skills]\ndisabled=['old']\n",
    );
    write(
        ".gemini/config/mcp_config.json",
        r#"{"mcpServers":{"native":{"command":"never-run"}}}"#,
    );
    write(".gemini/config/skills.json", r#"{"exclude":["old"]}"#);
    write(
        ".agents/skills/demo/SKILL.md",
        "---\nname: demo\n---\nsynthetic",
    );
    let registry = json!({"harnesses":{"grok":{"defaultHome":root.join(".grok")},"agy":{}}});
    let paths = Paths::new(&root, &root);
    let script = r#"import sys,json,pathlib
sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'lib'));sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'bin'))
import extension_launch as e
try:e.prepare_launch(json.loads(sys.argv[2]),sys.argv[3],'main',sys.argv[4],json.loads(sys.argv[5]),sys.argv[6],'mount-test');error='ok'
except ValueError as err:error=str(err)
private=next(pathlib.Path(sys.argv[6]).glob('extensions-*'))
files={p.name:p.read_text() for p in sorted(private.iterdir()) if p.is_file() and p.name!='manifest.json'}
print(json.dumps([error,files],ensure_ascii=False))"#;
    for h in ["grok", "agy"] {
        let config = root.join(if h == "grok" {
            ".grok/config.toml"
        } else {
            ".gemini/config/mcp_config.json"
        });
        let before = std::fs::read(&config).unwrap();
        let inv = e::inventory(&registry, h, "main", &root, &paths).unwrap();
        let skills: serde_json::Map<String, Value> = inv["skills"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["toggleable"] == true)
            .map(|r| (r["id"].as_str().unwrap().into(), json!(false)))
            .collect();
        let selection = json!({"mcps":{"native":false},"skills":skills});
        let runtime = root.join(format!("runtime-{h}"));
        let expected: Value = serde_json::from_str(
            &python::run_python(
                script,
                &[
                    registry.to_string().as_ref(),
                    h.as_ref(),
                    root.as_os_str(),
                    selection.to_string().as_ref(),
                    runtime.as_os_str(),
                ],
                &root,
            )
            .unwrap(),
        )
        .unwrap();
        let native_runtime = root.join(format!("native-{h}"));
        let error = match e::prepare_launch(
            &registry,
            h,
            "main",
            &root,
            &selection,
            &native_runtime,
            "mount-test",
            &paths,
        ) {
            Ok(_) => "ok".to_owned(),
            Err(e::LaunchError::Value(s)) => s,
            Err(e) => panic!("{e:?}"),
        };
        let private = std::fs::read_dir(&native_runtime)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("extensions-")
            })
            .unwrap();
        let mut files = serde_json::Map::new();
        let mut entries: Vec<_> = std::fs::read_dir(&private)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_file() && p.file_name().unwrap() != "manifest.json" {
                files.insert(
                    p.file_name().unwrap().to_str().unwrap().into(),
                    json!(std::fs::read_to_string(&p).unwrap()),
                );
            }
        }
        assert_eq!(
            json!([error, files]).to_string(),
            expected.to_string(),
            "{h}"
        );
        assert_eq!(before, std::fs::read(&config).unwrap());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn private_runtime_root_symlink_is_rejected_without_chmod_of_its_target() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = std::env::temp_dir().join(format!(
        "comandos-extension-root-link-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::set_permissions(root.join("target"), std::fs::Permissions::from_mode(0o755)).unwrap();
    symlink(root.join("target"), root.join("runtime")).unwrap();
    let registry = json!({"harnesses":{"codex":{"defaultHome":root.join(".codex")}}});
    let paths = Paths::new(&root, &root);
    assert_eq!(
        e::prepare_launch(
            &registry,
            "codex",
            "main",
            &root,
            &json!({}),
            &root.join("runtime"),
            "root-link-test",
            &paths
        ),
        Err(e::LaunchError::Value("Directorio privado inválido.".into()))
    );
    assert_eq!(
        std::fs::metadata(root.join("target"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(std::fs::read_dir(root.join("target")).unwrap().count(), 0);
    std::fs::remove_dir_all(root).unwrap();
}
