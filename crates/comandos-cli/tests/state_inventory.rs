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
#[test]
fn unified_database_and_guards_are_control_metadata_even_with_override() {
    let home = Home::new();
    let hooks = home.0.join(".claude/hooks");
    let db = hooks.join("prefs.json");
    for path in [
        &db,
        &PathBuf::from(format!("{}.sealed-ui-docs", db.display())),
        &PathBuf::from(format!("{}.domain-modes.lock", db.display())),
        &PathBuf::from(format!("{}-wal", db.display())),
    ] {
        fs::write(path, b"control").unwrap();
    }
    let outside = home.0.join("outside/overridden.sqlite3");
    fs::create_dir_all(outside.parent().unwrap()).unwrap();
    fs::write(&outside, b"private-db").unwrap();
    fs::write(format!("{}.sealed-tabs", outside.display()), b"control").unwrap();
    for configured in [&db, &outside] {
        let output = Command::new(env!("CARGO_BIN_EXE_comandos"))
            .args(["state", "inventory", "--json", "--home"])
            .arg(&home.0)
            .env("COMANDOS_DB", configured)
            .output()
            .unwrap();
        assert!(output.status.success());
        let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let files = v["files"].as_array().unwrap();
        for file in files.iter().filter(|r| {
            r["path"]
                .as_str()
                .unwrap()
                .starts_with(configured.to_str().unwrap())
        }) {
            assert_eq!(file["classification"], "metadatos-control");
            assert!(file["domain"].is_null());
        }
        assert!(
            files
                .iter()
                .any(|r| r["path"] == configured.to_str().unwrap())
        );
    }
}

#[test]
fn equivalent_override_spellings_exclude_destination_once() {
    let home = Home::new();
    let hooks = home.0.join(".claude/hooks");
    let db = hooks.join("prefs.json");
    let controls = [
        db.clone(),
        PathBuf::from(format!("{}.sealed-ui-docs", db.display())),
        PathBuf::from(format!("{}-wal", db.display())),
    ];
    for path in &controls {
        fs::write(path, b"private-control").unwrap();
    }
    std::os::unix::fs::symlink(&hooks, home.0.join("hooks-alias")).unwrap();
    for configured in [
        hooks.join("../hooks/prefs.json"),
        PathBuf::from(".claude/hooks/../hooks/prefs.json"),
        home.0.join("hooks-alias/prefs.json"),
        home.0.join("hooks-alias/../hooks/prefs.json"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_comandos"))
            .args(["state", "inventory", "--json", "--home"])
            .arg(&home.0)
            .current_dir(&home.0)
            .env("COMANDOS_DB", configured)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let rows = value["files"].as_array().unwrap();
        for control in &controls {
            let equivalent: Vec<_> = rows
                .iter()
                .filter(|r| fs::canonicalize(r["path"].as_str().unwrap()).unwrap() == *control)
                .collect();
            assert_eq!(equivalent.len(), 1, "duplicate rows: {equivalent:?}");
            assert_eq!(equivalent[0]["classification"], "metadatos-control");
            assert!(equivalent[0]["domain"].is_null());
        }
    }
}

#[test]
fn missing_destination_and_sidecars_use_equivalent_parent_identity() {
    use comandos_store::domains::catalog::is_unified_control_file;
    let home = Home::new();
    let hooks = home.0.join(".claude/hooks");
    std::os::unix::fs::symlink(&hooks, home.0.join("hooks-alias")).unwrap();
    for configured in [
        hooks.join("../hooks/prefs.json"),
        home.0.join("hooks-alias/prefs.json"),
        home.0.join("hooks-alias/missing/../prefs.json"),
    ] {
        assert!(is_unified_control_file(
            &hooks.join("prefs.json"),
            &configured
        ));
        assert!(is_unified_control_file(
            &hooks.join("prefs.json.sealed-ui-docs"),
            &configured
        ));
        assert!(!is_unified_control_file(
            &hooks.join("snippets.json"),
            &configured
        ));
    }
    assert!(!hooks.join("prefs.json").exists());
}
