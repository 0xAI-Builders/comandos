use comandos_cli::dispatch::{Command, resolve};

fn v(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn dispatch_by_argv0_and_explicit() {
    assert!(
        matches!(resolve("/home/x/.local/bin/cc-extensions", &v(&["serve", "mobbin"])), Command::Ext(a) if a == v(&["serve", "mobbin"]))
    );
    assert!(
        matches!(resolve("comandos", &v(&["ext", "serve", "mobbin"])), Command::Ext(a) if a == v(&["serve", "mobbin"]))
    );
    assert!(
        matches!(resolve("/home/x/.claude/hooks/cc-notify.sh", &v(&[])), Command::Hook(a) if a == v(&["claude"]))
    );
    assert!(
        matches!(resolve("cc-usage-tool.sh", &v(&[])), Command::Hook(a) if a == v(&["claude-usage"]))
    );
    assert!(
        matches!(resolve("codex-notify.sh", &v(&["{}"])), Command::Hook(a) if a == v(&["codex", "{}"]))
    );
    for (alias, harness) in [
        ("codex-hooks.sh", "codex-hooks"),
        ("gemini-hooks.sh", "gemini"),
        ("agy-hooks.sh", "agy"),
        ("/home/x/ComandOS/adapters/agy-statusline.py", "agy-status"),
        ("grok-hooks.py", "grok"),
        ("cc-status.sh", "claude-status"),
    ] {
        assert!(
            matches!(resolve(alias, &v(&["x"])), Command::Hook(a) if a == v(&[harness, "x"])),
            "{alias}"
        );
    }
    assert!(matches!(
        resolve("comandos", &v(&["--version"])),
        Command::Version
    ));
    assert!(
        matches!(resolve("comandos", &v(&["frobnicate"])), Command::Unknown(s) if s == "frobnicate")
    );
}

#[test]
fn dispatch_install() {
    assert!(
        matches!(resolve("comandos", &v(&["install", "--stage"])), Command::Install(a) if a == v(&["--stage"]))
    );
}

#[test]
fn dispatch_dash_and_cc_dash_alias() {
    assert!(
        matches!(resolve("comandos", &v(&["dash", "4777", "--no-open"])), Command::Dash(a) if a == v(&["4777", "--no-open"]))
    );
    assert!(
        matches!(resolve("/home/x/.local/bin/cc-dash", &v(&["--no-open"])), Command::Dash(a) if a == v(&["--no-open"]))
    );
}

#[test]
fn dispatch_browser_aliases_and_explicit() {
    assert!(
        matches!(resolve("comandos", &v(&["browser", "remote"])), Command::Browser(a) if a == v(&["remote"]))
    );
    assert!(
        matches!(resolve("/home/x/.local/bin/cc-browser-remote", &v(&[])), Command::Browser(a) if a == v(&["remote"]))
    );
    assert!(
        matches!(resolve("cc-browser-expose", &v(&["status"])), Command::Browser(a) if a == v(&["expose", "status"]))
    );
    assert!(
        matches!(resolve("cc-browser-npx-guard", &v(&["chrome-devtools-mcp"])), Command::Browser(a) if a == v(&["npx-guard", "chrome-devtools-mcp"]))
    );
}

#[test]
fn dispatch_winstart_alias_and_explicit() {
    for argv0 in ["comandos", "/private/bin/cc-winstart"] {
        let args = if argv0 == "comandos" {
            v(&["winstart", "--uninstall"])
        } else {
            v(&["--uninstall"])
        };
        assert_eq!(
            format!("{:?}", resolve(argv0, &args)),
            "Winstart([\"--uninstall\"])"
        );
    }
}
#[test]
fn dispatch_agents_alias_and_explicit_preserve_arguments() {
    for argv0 in ["comandos", "/private/bin/cc-agents"] {
        let args = if argv0 == "comandos" {
            v(&["agents", "setup", "ignored"])
        } else {
            v(&["setup", "ignored"])
        };
        assert_eq!(
            resolve(argv0, &args),
            Command::Agents(v(&["setup", "ignored"]))
        );
    }
}

#[test]
fn dispatch_acp_alias_and_explicit_preserve_values() {
    for (name, args) in [
        (
            "comandos",
            v(&["acp", "--model", "two words", "--resume", "id"]),
        ),
        (
            "/private/bin/cc-acp",
            v(&["--model", "two words", "--resume", "id"]),
        ),
    ] {
        assert_eq!(
            resolve(name, &args),
            Command::Acp(v(&["--model", "two words", "--resume", "id"]))
        );
    }
}
