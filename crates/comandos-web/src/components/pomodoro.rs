use serde_json::Value;
pub const MIN: f64 = 60000.;
pub fn elapsed(block: &Value, now: f64) -> f64 {
    if block.is_null() {
        return 0.;
    }
    let n = |k| block.get(k).and_then(Value::as_f64).unwrap_or(0.);
    let delta = if block.get("status").and_then(Value::as_str) == Some("running") {
        (now - n("resumedAtMs")).max(0.)
    } else {
        0.
    };
    n("targetMs").min(n("activeMs") + delta)
}
pub fn remaining(block: &Value, now: f64) -> f64 {
    if block.is_null() {
        0.
    } else {
        (block.get("targetMs").and_then(Value::as_f64).unwrap_or(0.) - elapsed(block, now)).max(0.)
    }
}
pub fn fmt(ms: f64) -> String {
    let s = (ms.max(0.) / 1000.).ceil() as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}
pub fn delta(block: &Value, now: f64, minutes: f64) -> f64 {
    (elapsed(block, now) + (minutes + 0.5).floor() * MIN).clamp(MIN, 180. * MIN)
        - block.get("targetMs").and_then(Value::as_f64).unwrap_or(0.)
}
pub fn hourglass(now: f64, flip: Option<f64>) -> f64 {
    if let Some(flip) = flip
        && now >= flip
        && now - flip < 660.
    {
        return 21. + ((now - flip) / 110.).floor();
    }
    let t = now.rem_euclid(8010.);
    if t < 7350. {
        (t / 350.).floor()
    } else {
        21. + ((t - 7350.) / 110.).floor()
    }
}
#[cfg(target_arch = "wasm32")]
#[path = "pomodoro_web.rs"]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::{attach, mount};
#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn attach() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
