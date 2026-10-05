//! `MotorResults` (dueño de `~/.claude/hooks/motor-results.json`) contra
//! `motor_result_set`/`motor_stage` del Python (`bin/cc-dash` 3614–3672):
//! mismo archivo, byte a byte, tras la misma secuencia y el mismo reloj.
mod support;

use comandos_server::dash::native::ops::results::MotorResults;
use serde_json::{Map, Value, json};
use std::sync::Arc;
use support::{TestHome, ops::clone_home, oracle};

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
    let Some(_) = oracle::run_dash(
        &twin,
        "dash.time.time = lambda: 1000.0\n\
         dash.motor_result_set('nuevo|%1', False, 'detalle ñ', operationId='op-1', vacío=None)",
    ) else {
        return;
    };
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
    let Some(_) = oracle::run_dash(
        &twin,
        "dash.time.time = lambda: 1234.5\n\
         long = 'é' * 250\n\
         dash.motor_stage('a|%1', long, 'c' * 50, operationId='x')\n\
         dash.motor_result_set('c|%3', True, long, rolledBack=False)\n\
         dash.motor_stage('d|%4', 'aplicando')",
    ) else {
        return;
    };
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
