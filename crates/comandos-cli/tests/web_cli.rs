use std::{fs, path::PathBuf, process::Command};

struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "web-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(p.join(".claude/hooks")).unwrap();
        Self(p)
    }
    fn selection(&self) -> PathBuf {
        self.0.join(".claude/hooks/comandos-web.json")
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn comandos(home: &Home, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_comandos"))
        .env("HOME", &home.0)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn web_set_shadow_writes_selection_atomically() {
    let home = Home::new();
    let out = comandos(&home, &["web", "set", "quick-terminal", "shadow"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_to_string(home.selection()).unwrap(),
        "{\"on\":[],\"shadow\":[\"quick-terminal\"]}"
    );
}

#[test]
fn web_set_unknown_component_exits_2() {
    let home = Home::new();
    let out = comandos(&home, &["web", "set", "nada", "on"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("componente desconocido: nada"));
}

#[test]
fn web_status_falls_back_to_selection_file_when_front_is_down() {
    let home = Home::new();
    fs::write(
        home.selection(),
        "{\"on\":[\"quick-terminal\"],\"shadow\":[]}",
    )
    .unwrap();
    let out = comandos(&home, &["web", "status"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "{\"on\":[\"quick-terminal\"],\"shadow\":[]}"
    );
}
