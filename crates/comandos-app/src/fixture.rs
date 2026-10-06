//! Estado exportable T12: identidades sintéticas; sandbox restaura únicamente shells.
use serde_json::{Value, json};
use std::path::Path;
pub fn mixed(home: &Path) -> Value {
    let cwd = home.display().to_string();
    let body = "120x32,0,0{59x32,0,0,1,60x32,60,0,2}";
    let layout = format!(
        "{:04x},{body}",
        comandos_core::workspace::snapshot::layout_checksum(body)
    );
    let shell = "120x32,0,0,3";
    let shell_layout = format!(
        "{:04x},{shell}",
        comandos_core::workspace::snapshot::layout_checksum(shell)
    );
    json!({"tabs":{"local":"Local","term-fixture-mixed":"Mixto · 日本","term-fixture-shell":"Shell","ssh-fixture":"SSH sintético","xterm-fixture":"xterm","missing-fixture":"Sesión ausente"},
    "snapshot":{"version":2,"sessions":{"term-fixture-mixed":{"windows":[{"index":0,"name":"claude","width":120,"height":32,"active":true,"zoomed":false,"layout":layout,"panes":[{"id":"%1","cwd":cwd,"active":true,"agent":"claude","resume_id":"12345678-1234-1234-1234-123456789abc","key":"fixture-claude"},{"id":"%2","cwd":cwd,"active":false,"agent":"codex","resume_id":"87654321-4321-4321-4321-cba987654321","key":"fixture-codex"}]}]},"term-fixture-shell":{"windows":[{"index":0,"name":"claude","width":120,"height":32,"active":true,"zoomed":false,"layout":shell_layout,"panes":[{"id":"%3","cwd":cwd,"active":true,"agent":""}]}]}}},
    "workspace":{"schema":1,"revision":7,"tabs":{"local":{"session":"local"},"term-fixture-mixed":{"session":"term-fixture-mixed","paneKeys":["fixture-claude","fixture-codex"]},"term-fixture-shell":{"session":"term-fixture-shell"},"missing-fixture":{"session":"missing-fixture"},"ssh-fixture":{"session":"ssh-fixture"},"xterm-fixture":{"session":"xterm-fixture"}},"groups":[{"id":"local","tree":{"type":"tab","tabId":"local"}},{"id":"split-fixture","tree":{"type":"split","axis":"x","ratio":0.55,"first":{"type":"tab","tabId":"term-fixture-mixed"},"second":{"type":"split","axis":"y","ratio":0.4,"first":{"type":"tab","tabId":"term-fixture-shell"},"second":{"type":"tab","tabId":"missing-fixture"}}}},{"id":"ssh","tree":{"type":"tab","tabId":"ssh-fixture"}},{"id":"web","tree":{"type":"tab","tabId":"xterm-fixture"}}]},
    "prefs":{"theme":"bruno","tabs_layout":"rows","favorites":["term-fixture-mixed"],"button_style":"sutil"},
    "state":[{"session":"term-fixture-mixed","status":"working","agent":"claude","label":"Mixto · 日本"},{"session":"term-fixture-shell","status":"waiting","label":"Shell"},{"session":"ssh-fixture","status":"done","label":"SSH sintético"}],
    "marks":{"marks":[{"scope":"session","key":"term-fixture-mixed","mark":"awaiting_reply","revision":2},{"scope":"session","key":"term-fixture-shell","mark":"frozen","revision":1},{"scope":"session","key":"ssh-fixture","mark":"resolved","revision":1}]}})
}
pub fn export(root: &Path) -> Result<(), String> {
    if !root.is_absolute() {
        return Err("La raíz debe ser absoluta".into());
    }
    if root.exists() {
        return Err("La raíz ya existe".into());
    }
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(root)
        .map_err(|e| e.to_string())?;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(root.join("run"))
        .map_err(|e| e.to_string())?;
    let args = vec![
        "--mode".into(),
        "sandbox".into(),
        "--tmux-socket".into(),
        "fixture".into(),
    ];
    let external_home = root
        .parent()
        .ok_or("Falta directorio padre")?
        .join("comandos-fixture-env-home");
    let config = crate::config::parse_args(&args, false, &|key| match key {
        "HOME" => Some(external_home.display().to_string()),
        "XDG_RUNTIME_DIR" => Some(root.join("run").display().to_string()),
        _ => None,
    })
    .map_err(|e| format!("{e:?}"))?;
    let guard = crate::guard::WriteGuard::from_config(&config, "");
    let sandbox = config.sandbox_root().ok_or("Falta raíz privada")?;
    let home = sandbox.join("home");
    let hooks = config.hooks_dir().to_path_buf();
    guard
        .create_dir_all(&home, 0o700)
        .map_err(|e| format!("{e:?}"))?;
    guard
        .create_dir_all(&hooks, 0o700)
        .map_err(|e| format!("{e:?}"))?;
    let fixture = mixed(&home);
    for (name, key) in [
        ("app-tabs.json", "tabs"),
        ("app-sessions-v2.json", "snapshot"),
        ("app-layout.json", "workspace"),
    ] {
        guard
            .write_atomic(
                &hooks.join(name),
                &serde_json::to_vec_pretty(fixture.get(key).ok_or("Falta estado")?)
                    .map_err(|e| e.to_string())?,
                "fixture",
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    guard
        .write_atomic(
            &sandbox.join("dashboard-fixture.json"),
            &serde_json::to_vec_pretty(&fixture).map_err(|e| e.to_string())?,
            "fixture",
        )
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}
