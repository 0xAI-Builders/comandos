use comandos_runtime::{capabilities::Paths, extension_launch as launch};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};

const BINDING: &str = "/private/tmux.sock|123|456|$7|%8|901";
const ENV: &str = "COMANDOS_MCP_SESSION_BINDING";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let f =
            Self(std::env::temp_dir().join(comandos_runtime::fresh_id("session-binding").unwrap()));
        f.write(
            ".local/share/comandos/bin/comandos",
            b"\x7fELFprivate-test-fixture",
        );
        let native = f.0.join(".local/share/comandos/bin/comandos");
        fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir_all(f.0.join(".local/bin")).unwrap();
        symlink(native, launch::helper_for_home(&f.0)).unwrap();
        f.write(".config/comandos/extensions/catalog.json", br#"{"servers":{"managed":{"command":"never-run"},"synthetic":{"url":"https://example.invalid/mcp"}}}"#);
        f.write(".codex/config.toml", b"[mcp_servers.managed]\ncommand='old-private'\n[mcp_servers.native]\ncommand='never-run'\n[mcp_servers.alias]\ncommand='cc-extensions'\nargs=['serve','managed']\n[mcp_servers.alias.env]\nKEEP='fixture-value'\n");
        f.write(".claude/settings.json", b"{}");
        f.write(".claude.json", br#"{"mcpServers":{"managed":{"command":"old-private"},"native":{"command":"never-run"},"alias":{"command":"cc-extensions","args":["serve","managed"],"env":{"KEEP":"fixture-value"}}}}"#);
        f
    }
    fn write(&self, path: &str, bytes: &[u8]) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fn prepare(&self, harness: &str) -> Value {
        self.prepare_selection(harness, &json!({}))
    }
    fn prepare_selection(&self, harness: &str, selection: &Value) -> Value {
        let registry = json!({"harnesses":{"codex":{"defaultHome":self.0.join(".codex")},"claude":{"defaultHome":self.0.join(".claude")}}});
        let bundle = launch::prepare_launch_with_binding(
            &registry,
            harness,
            "main",
            &self.0,
            selection,
            &self.0.join("runtime"),
            "binding-test",
            &Paths::new(&self.0, &self.0),
            Some(BINDING),
        )
        .unwrap();
        serde_json::from_slice(&fs::read(bundle["manifest"].as_str().unwrap()).unwrap()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn codex_binds_each_managed_proxy_in_its_own_environment() {
    let f = Fixture::new();
    let before = fs::read(f.0.join(".codex/config.toml")).unwrap();
    let manifest = f.prepare("codex");
    assert!(
        manifest["env"].get(ENV).is_none(),
        "not a global Codex environment override"
    );
    let mut envs = serde_json::Map::new();
    for arg in manifest["args"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .filter(|a| a.starts_with("mcp_servers."))
    {
        let data = launch::parse_toml(arg).unwrap().unwrap();
        for (name, spec) in data["mcp_servers"].as_object().unwrap() {
            if let Some(env) = spec.get("env") {
                envs.insert(name.clone(), env.clone());
            }
        }
    }
    for name in ["managed", "synthetic", "alias"] {
        assert_eq!(
            envs.get(name).and_then(|env| env.get(ENV)),
            Some(&json!(BINDING)),
            "proxy {name}"
        );
    }
    assert!(!envs.contains_key("native"));
    assert_eq!(before, fs::read(f.0.join(".codex/config.toml")).unwrap());
}

#[test]
fn bound_off_connectors_stay_configured_while_unmanaged_off_is_disabled() {
    let f = Fixture::new();
    let desired = json!({"mcps":{"managed":false,"synthetic":false,"alias":false,"native":false}});
    comandos_store::extension_gate::save(&f.0, BINDING, &desired["mcps"]).unwrap();
    let manifest = f.prepare_selection("codex", &desired);
    let args: Vec<_> = manifest["args"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for name in ["managed", "synthetic", "alias"] {
        assert!(
            args.contains(&format!("mcp_servers.{name}.enabled=true").as_str()),
            "bound proxy {name} must stay connected"
        );
        assert!(!args.contains(&format!("mcp_servers.{name}.enabled=false").as_str()));
    }
    assert!(args.contains(&"mcp_servers.native.enabled=false"));
    assert_eq!(manifest["bundle"]["selection"]["mcps"], desired["mcps"]);

    let manifest = f.prepare_selection("claude", &desired);
    let args = manifest["args"].as_array().unwrap();
    let index = args.iter().position(|a| a == "--mcp-config").unwrap();
    let config: Value =
        serde_json::from_slice(&fs::read(args[index + 1].as_str().unwrap()).unwrap()).unwrap();
    for name in ["managed", "synthetic", "alias"] {
        assert_eq!(
            config["mcpServers"][name]["env"][ENV], BINDING,
            "bound proxy {name}"
        );
    }
    assert!(config["mcpServers"].get("native").is_none());
    assert!(!comandos_store::extension_gate::enabled(&f.0, BINDING, "managed").unwrap());
}

#[test]
fn initial_bound_launch_publishes_exclusion_before_any_proxy_can_connect() {
    let f = Fixture::new();
    assert!(
        comandos_store::extension_gate::selection(&f.0, BINDING)
            .unwrap()
            .is_none()
    );
    let desired = json!({"mcps":{"managed":false}});
    f.prepare_selection("codex", &desired);
    assert!(!comandos_store::extension_gate::enabled(&f.0, BINDING, "managed").unwrap());
    // A later launch carrying an old selection must not undo a live user decision.
    comandos_store::extension_gate::save(&f.0, BINDING, &json!({"managed":true})).unwrap();
    f.prepare_selection("claude", &desired);
    assert!(comandos_store::extension_gate::enabled(&f.0, BINDING, "managed").unwrap());
}

#[test]
fn claude_binds_managed_proxies_and_preserves_unrelated_environment() {
    let f = Fixture::new();
    let before = fs::read(f.0.join(".claude.json")).unwrap();
    let manifest = f.prepare("claude");
    assert!(manifest["env"].get(ENV).is_none());
    let args = manifest["args"].as_array().unwrap();
    let index = args.iter().position(|a| a == "--mcp-config").unwrap();
    let config: Value =
        serde_json::from_slice(&fs::read(args[index + 1].as_str().unwrap()).unwrap()).unwrap();
    let servers = &config["mcpServers"];
    for name in ["managed", "synthetic", "alias"] {
        assert_eq!(servers[name]["env"][ENV], BINDING, "proxy {name}");
    }
    assert_eq!(servers["alias"]["env"]["KEEP"], "fixture-value");
    assert!(servers["native"].get("env").is_none());
    assert_eq!(before, fs::read(f.0.join(".claude.json")).unwrap());
}
