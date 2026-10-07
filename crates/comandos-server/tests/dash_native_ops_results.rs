//! `MotorResults` (dueño de `~/.claude/hooks/motor-results.json`) contra
//! `motor_result_set`/`motor_stage` del Python (`bin/cc-dash` 3614–3672):
//! mismo archivo, byte a byte, tras la misma secuencia y el mismo reloj.
mod support;

use comandos_server::dash::native::ops::results::MotorResults;
use serde_json::{Map, Value, json};
use std::sync::Arc;
use support::{TestHome, ops::clone_home};

fn read(home: &TestHome) -> Vec<u8> {
    std::fs::read(home.hooks().join("motor-results.json")).unwrap()
}

fn fixed(ts: f64) -> Arc<dyn Fn() -> f64 + Send + Sync> {
    Arc::new(move || ts)
}

#[test]
fn motor_results_trim_matches_python() {
    let home = TestHome::new("motor-trim");
    let mut seed = Map::new();
    for i in 0..301 {
        // Orden de inserción distinto del de `ts`: el recorte ordena por `ts`
        // y el resto conserva su orden.
        let ts = f64::from((i * 37) % 301);
        seed.insert(
            format!("s{i}|%{i}"),
            json!({"ok": true, "detail": "x", "ts": ts}),
        );
    }
    home.write("motor-results.json", &Value::Object(seed).to_string());
    let twin = clone_home(&home, "motor-trim-py");
    let results = MotorResults::load_with(&home.hooks().join("motor-results.json"), fixed(1000.0));
    results
        .set_with_ts(
            "nuevo|%1",
            false,
            "detalle ñ",
            &[("operationId", json!("op-1")), ("vacío", Value::Null)],
            1000.0,
        )
        .unwrap();
    support::http_golden::dash_files(
        &twin,
        "server-motor-results",
        &["motor-results.json"],
        "dash.time.time = lambda: 1000.0\n\
         dash.motor_result_set('nuevo|%1', False, 'detalle ñ', operationId='op-1', vacío=None)",
        &Default::default(),
    );
    assert_eq!(read(&home), read(&twin));
    assert_eq!(results.all().len(), 200);
}

#[test]
fn motor_stage_and_long_detail_match_python() {
    let home = TestHome::new("motor-stage");
    home.write(
        "motor-results.json",
        r#"{"a|%1": {"stage": "viejo", "ts": 5}, "b|%2": 7}"#,
    );
    let twin = clone_home(&home, "motor-stage-py");
    let results = MotorResults::load_with(&home.hooks().join("motor-results.json"), fixed(1234.5));
    let long = "é".repeat(250);
    results
        .stage(
            "a|%1",
            &long,
            &"c".repeat(50),
            &[("operationId", json!("x"))],
        )
        .unwrap();
    results
        .set("c|%3", true, &long, &[("rolledBack", json!(false))])
        .unwrap();
    results.stage("d|%4", "aplicando", "", &[]).unwrap();
    support::http_golden::dash_files(
        &twin,
        "server-motor-results",
        &["motor-results.json"],
        "dash.time.time = lambda: 1234.5\n\
         long = 'é' * 250\n\
         dash.motor_stage('a|%1', long, 'c' * 50, operationId='x')\n\
         dash.motor_result_set('c|%3', True, long, rolledBack=False)\n\
         dash.motor_stage('d|%4', 'aplicando')",
        &Default::default(),
    );
    assert_eq!(read(&home), read(&twin));
}

#[test]
fn unreadable_file_starts_empty_and_uncertain_file_is_never_rewritten() {
    let home = TestHome::new("motor-load");
    home.write("motor-results.json", "{roto");
    let results = MotorResults::load_with(&home.hooks().join("motor-results.json"), fixed(1.0));
    assert!(results.certain());
    assert!(results.all().is_empty());
    // Bytes que no son UTF-8: el Python cargaría según su codificación.
    std::fs::write(home.hooks().join("motor-results.json"), b"{\"a\xff\": 1}").unwrap();
    let results = MotorResults::load_with(&home.hooks().join("motor-results.json"), fixed(1.0));
    assert!(!results.certain());
    assert!(results.set("k|%1", true, "ok", &[]).is_err());
    assert_eq!(read(&home), b"{\"a\xff\": 1}");
}

fn on_disk(home: &TestHome) -> Map<String, Value> {
    serde_json::from_slice(&read(home)).unwrap()
}

/// Ronda 1 (I2): el Python heredado reescribe el archivo desde su memoria
/// (sin las claves de Rust); Rust sigue sirviendo las suyas y las devuelve al
/// archivo en su siguiente escritura.
#[test]
fn legacy_overwrite_keeps_rust_entries_visible() {
    let home = TestHome::new("motor-legacy-overwrite");
    let path = home.hooks().join("motor-results.json");
    let results = MotorResults::load_with(&path, fixed(1.0));
    results
        .set_with_ts("rust|%1", true, "mío", &[], 100.0)
        .unwrap();
    assert!(on_disk(&home).contains_key("rust|%1"));
    support::http_golden::dash_files(
        &home,
        "server-motor-results",
        &["motor-results.json"],
        "dash.MOTOR_RESULT.clear()\n\
         dash.time.time = lambda: 200.0\n\
         dash.motor_result_set('legacy|%2', True, 'py')",
        &Default::default(),
    );
    let disk = on_disk(&home);
    assert!(!disk.contains_key("rust|%1"), "el Python pisó el archivo");
    let all = results.all();
    assert_eq!(all["rust|%1"]["detail"], json!("mío"));
    assert_eq!(all["legacy|%2"]["detail"], json!("py"));
    assert_eq!(results.get("rust|%1").unwrap()["ts"], json!(100.0));
    results
        .set_with_ts("rust|%3", false, "otro", &[], 300.0)
        .unwrap();
    let disk = on_disk(&home);
    for key in ["rust|%1", "legacy|%2", "rust|%3"] {
        assert!(disk.contains_key(key), "falta {key} en {disk:?}");
    }
}

/// Ronda 1 (I2): cada escritura de Rust relee el archivo y conserva lo que
/// escribió el Python; con la misma clave gana el `ts` más reciente.
#[test]
fn rust_writes_keep_legacy_entries_and_newer_ts_wins() {
    let home = TestHome::new("motor-legacy-merge");
    let path = home.hooks().join("motor-results.json");
    let results = MotorResults::load_with(&path, fixed(1.0));
    results
        .set_with_ts("shared|%1", true, "rust viejo", &[], 100.0)
        .unwrap();
    support::http_golden::dash_files(
        &home,
        "server-motor-results",
        &["motor-results.json"],
        "dash.time.time = lambda: 50.0\n\
         dash.motor_result_set('legacy|%1', True, 'py')\n\
         dash.time.time = lambda: 300.0\n\
         dash.motor_result_set('shared|%1', False, 'py nuevo')",
        &Default::default(),
    );
    // El Python cargó el archivo al importar: conserva la clave de Rust.
    assert_eq!(
        results.get("shared|%1").unwrap()["detail"],
        json!("py nuevo")
    );
    results
        .set_with_ts("rust|%2", true, "mío", &[], 400.0)
        .unwrap();
    let disk = on_disk(&home);
    assert_eq!(disk["legacy|%1"]["detail"], json!("py"));
    assert_eq!(disk["shared|%1"]["detail"], json!("py nuevo"));
    assert_eq!(disk["rust|%2"]["detail"], json!("mío"));
    results
        .set_with_ts("shared|%1", true, "rust nuevo", &[], 500.0)
        .unwrap();
    assert_eq!(on_disk(&home)["shared|%1"]["detail"], json!("rust nuevo"));
    assert_eq!(results.all()["legacy|%1"]["ts"], json!(50.0));
}

#[test]
fn late_rust_write_preserves_newer_legacy_result_for_same_operation() {
    let home = TestHome::new("motor-older-write");
    let path = home.hooks().join("motor-results.json");
    let results = MotorResults::load_with(&path, fixed(1.0));
    home.write(
        "motor-results.json",
        r#"{"shared|%1":{"ok":true,"detail":"legacy nuevo","ts":300.0}}"#,
    );
    results
        .set_with_ts("shared|%1", false, "rust atrasado", &[], 200.0)
        .unwrap();
    assert_eq!(on_disk(&home)["shared|%1"]["detail"], json!("legacy nuevo"));
    assert_eq!(results.get("shared|%1").unwrap()["ts"], json!(300.0));
}

/// Ronda 1 (I2): la mezcla de disco y memoria se acota con el recorte de O3
/// (más de 300 → las 200 de `ts` más reciente).
#[test]
fn merged_view_is_trimmed_like_python() {
    let home = TestHome::new("motor-merge-trim");
    let path = home.hooks().join("motor-results.json");
    let results = MotorResults::load_with(&path, fixed(1.0));
    results
        .set_with_ts("rust|%0", true, "mío", &[], 10_000.0)
        .unwrap();
    let mut seed = Map::new();
    for i in 0..300 {
        seed.insert(
            format!("py{i}|%{i}"),
            json!({"ok": true, "ts": f64::from(i)}),
        );
    }
    home.write("motor-results.json", &Value::Object(seed).to_string());
    let all = results.all();
    assert_eq!(all.len(), 200);
    assert!(all.contains_key("rust|%0"));
    assert!(!all.contains_key("py100|%100"));
    assert!(all.contains_key("py101|%101"));
}

#[test]
fn late_rust_write_keeps_newer_own_entry_after_legacy_overwrite() {
    let home = TestHome::new("motor-own-newer");
    let results = MotorResults::load_with(&home.hooks().join("motor-results.json"), fixed(1.0));
    results
        .set_with_ts("audit|%0", true, "nuevo", &[], 300.0)
        .unwrap();
    home.write("motor-results.json", "{}");
    results
        .set_with_ts("audit|%0", false, "atrasado", &[], 200.0)
        .unwrap();
    assert_eq!(results.get("audit|%0").unwrap()["ts"], json!(300.0));
    assert_eq!(on_disk(&home)["audit|%0"]["detail"], json!("nuevo"));
    results
        .set_with_ts("audit|%0", true, "igual", &[], 300.0)
        .unwrap();
    assert_eq!(results.get("audit|%0").unwrap()["detail"], json!("igual"));
}
