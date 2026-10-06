use comandos_cli::{
    dispatch::{Command, resolve},
    pane_model,
};
use comandos_store::unified::{self, Mode, Origin};
use std::{
    fs,
    os::unix::fs::symlink,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "native-pane-model-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join(".claude/hooks")).unwrap();
        Self(path)
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn pane_border_matches_frozen_original_output_and_exact_pane_identity() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/pane-model-reference.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        assert_eq!(
            pane_model::render(
                case["body"].as_str().unwrap().as_bytes(),
                case["pane"].as_str().unwrap()
            ),
            case["expected"].as_str().unwrap().as_bytes()
        );
    }
    assert!(pane_model::render(b"%1 claude secret\n", "%.*").is_empty());
    assert!(pane_model::render(b"%1 claude secret\n", "%1\n%1").is_empty());
}

#[test]
fn reader_uses_quota_authority_without_rewriting_legacy_source() {
    for mode in [Mode::Legacy, Mode::Mirror, Mode::Unified, Mode::Sealed] {
        let home = Home::new();
        let path = home.0.join(".claude/hooks/pane-models.txt");
        let old = b"%1 claude Legacy\n";
        fs::write(&path, old).unwrap();
        let db = unified::open_unified(&unified::unified_path(&home.0)).unwrap();
        unified::doc_put(
            &db,
            "hooks/pane-models.txt",
            "quota-docs",
            b"%1 codex Native\n",
            Origin::Import,
            1,
        )
        .unwrap();
        unified::set_mode(&db, "quota-docs", mode, "fixture", 2).unwrap();
        let expected = if matches!(mode, Mode::Unified | Mode::Sealed) {
            "#[fg=colour43,bold]▸ codex Native#[default]".as_bytes()
        } else {
            "#[fg=colour141,bold]▸ claude Legacy#[default]".as_bytes()
        };
        assert_eq!(pane_model::read(&home.0, "%1").unwrap(), expected);
        assert_eq!(fs::read(&path).unwrap(), old);
        if mode == Mode::Sealed {
            fs::remove_file(&path).unwrap();
            assert_eq!(pane_model::read(&home.0, "%1").unwrap(), expected);
            assert!(!path.exists());
        }
    }
}

#[test]
fn native_alias_and_explicit_cli_preserve_bytes_and_missing_output() {
    let home = Home::new();
    let alias = home.0.join("cc-pane-model");
    symlink(env!("CARGO_BIN_EXE_comandos"), &alias).unwrap();
    let pane = vec!["%1".into()];
    assert_eq!(
        resolve(alias.to_str().unwrap(), &pane),
        Command::PaneModel(pane.clone())
    );
    fs::write(
        home.0.join(".claude/hooks/pane-models.txt"),
        "%1 claude ñ😀\n",
    )
    .unwrap();
    for (binary, args) in [
        (alias.as_path(), vec!["%1"]),
        (
            std::path::Path::new(env!("CARGO_BIN_EXE_comandos")),
            vec!["pane-model", "%1"],
        ),
    ] {
        let result = std::process::Command::new(binary)
            .args(args)
            .env("HOME", &home.0)
            .output()
            .unwrap();
        assert!(result.status.success());
        assert_eq!(
            result.stdout,
            "#[fg=colour141,bold]▸ claude ñ😀#[default]".as_bytes()
        );
    }
    fs::remove_file(home.0.join(".claude/hooks/pane-models.txt")).unwrap();
    let missing = std::process::Command::new(alias)
        .arg("%1")
        .env("HOME", &home.0)
        .output()
        .unwrap();
    assert!(missing.status.success());
    assert!(missing.stdout.is_empty());
    assert!(!home.0.join(".local/share/comandos").exists());
}
