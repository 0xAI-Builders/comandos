//! Differential proof against the original Value-based resize queue.
#[path = "../src/pane_chrome/resize.rs"]
mod resize;
use resize::ResizeRequest;
use serde_json::{Value, json};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;
fn original(pane: &str, vertical: bool, size: f64) -> Value {
    json!({"pane":pane,"axis":if vertical{"x"}else{"y"},"size":size})
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn queue_equality_and_payload_bytes_match_original_values() {
    let mut cases = Vec::new();
    for pane in ["", "%1", r#"pane<&""#, "😀", "private-pane"] {
        for vertical in [false, true] {
            for size in [
                0.,
                -0.,
                2.,
                2.5,
                -2.,
                1e-7,
                1e21,
                f64::MAX,
                f64::MIN_POSITIVE,
                f64::from_bits(1),
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NAN,
            ] {
                let request = ResizeRequest::new(pane.into(), vertical, size);
                let previous = original(pane, vertical, size);
                let mut body = json!({"session":"private-session","action":"resize"});
                for key in ["pane", "axis", "size"] {
                    body[key] = previous[key].clone();
                }
                assert_eq!(
                    request.body("private-session").to_string(),
                    body.to_string()
                );
                assert!(request == request.clone());
                cases.push((request, previous));
            }
        }
    }
    for (a, original_a) in &cases {
        for (b, original_b) in &cases {
            assert_eq!(a == b, original_a == original_b);
            assert_eq!(Some(a) == Some(b), Some(original_a) == Some(original_b));
        }
    }
}
