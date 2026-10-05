// crates/comandos-core/tests/web_assets.rs
use comandos_core::web_assets::{MANIFEST_FILE, Manifest, out_dir_from};
use std::path::Path;

#[test]
fn manifest_round_trips_through_json() {
    let mut m = Manifest::default();
    m.files.insert(
        "comandos_web_bg.wasm".into(),
        "a1b2c3d4e5f6/comandos_web_bg.wasm".into(),
    );
    m.files
        .insert("comandos_web_boot.js".into(), "a1b2c3d4e5f6/boot.js".into());
    let text = serde_json::to_string(&m).unwrap();
    assert_eq!(
        text,
        r#"{"files":{"comandos_web_bg.wasm":"a1b2c3d4e5f6/comandos_web_bg.wasm","comandos_web_boot.js":"a1b2c3d4e5f6/boot.js"}}"#
    );
    let back: Manifest = serde_json::from_str(&text).unwrap();
    assert_eq!(back, m);
    assert_eq!(
        back.path("comandos_web_boot.js"),
        Some("a1b2c3d4e5f6/boot.js")
    );
    assert_eq!(back.path("nada.js"), None);
    assert_eq!(MANIFEST_FILE, "manifest.json");
}

#[test]
fn out_dir_follows_cargo_target_dir_rules() {
    let ws = Path::new("/ws");
    let cwd = Path::new("/cwd");
    // Sin CARGO_TARGET_DIR: <workspace>/target/web.
    assert_eq!(out_dir_from(None, cwd, ws), Path::new("/ws/target/web"));
    // Vacío cuenta como ausente.
    assert_eq!(
        out_dir_from(Some(Path::new("")), cwd, ws),
        Path::new("/ws/target/web")
    );
    // Absoluto: tal cual.
    assert_eq!(
        out_dir_from(Some(Path::new("/x/t")), cwd, ws),
        Path::new("/x/t/web")
    );
    // Relativo: respecto al directorio desde el que se invoca cargo.
    assert_eq!(
        out_dir_from(Some(Path::new(".build/t")), cwd, ws),
        Path::new("/cwd/.build/t/web")
    );
}
