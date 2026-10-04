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
    assert!(matches!(
        resolve("comandos", &v(&["--version"])),
        Command::Version
    ));
    assert!(
        matches!(resolve("comandos", &v(&["frobnicate"])), Command::Unknown(s) if s == "frobnicate")
    );
}
