//! El inventario nunca transforma ni oculta estado desconocido.
use std::{fs, path::PathBuf, process::Command, time::SystemTime};

struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "comandos-inventory-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(p.join(".claude/hooks/state")).unwrap();
        Self(p)
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn inventory_reports_domains_hashes_and_unknowns_without_writes() {
    let home = Home::new();
    let hooks = home.0.join(".claude/hooks");
    for (file, text) in [
        ("app-tabs.json", "{\"a\":\"ñ\"}"),
        ("state/pane.json", "{}"),
        ("providers.env", "SECRET=x"),
        ("usage.db", "legacy"),
        ("new-state.json", "unclassified"),
    ] {
        fs::write(hooks.join(file), text).unwrap();
    }
    std::os::unix::fs::symlink("missing.py", hooks.join("md2tg.py")).unwrap();
    let before = fs::metadata(hooks.join("app-tabs.json"))
        .unwrap()
        .modified()
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args(["state", "inventory", "--json", "--home"])
        .arg(&home.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let files = value["files"].as_array().unwrap();
    let row = |path: &str| files.iter().find(|r| r["source"] == path).unwrap();
    assert_eq!(row("H/app-tabs.json")["domain"], "tabs");
    assert_eq!(row("H/state/pane.json")["domain"], "session-status");
    assert_eq!(
        row("H/providers.env")["classification"],
        "se-queda-como-archivo"
    );
    assert_eq!(row("H/usage.db")["classification"], "resto");
    assert_eq!(row("H/md2tg.py")["classification"], "resto");
    assert_eq!(row("H/new-state.json")["classification"], "sin-dominio");
    assert_eq!(row("H/app-tabs.json")["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(row("H/md2tg.py")["symlink"], true);
    assert!(
        !home
            .0
            .join(".local/share/comandos/comandos.sqlite3")
            .exists()
    );
    assert_eq!(
        fs::metadata(hooks.join("app-tabs.json"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
    assert_eq!(fs::read(hooks.join("providers.env")).unwrap(), b"SECRET=x");
}
