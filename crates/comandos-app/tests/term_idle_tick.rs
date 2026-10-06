#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[allow(dead_code)]
#[path = "support/t18_native.rs"]
mod native;
use serde_json::json;
#[test]
fn real_tick_is_idle_without_damage_but_read_and_sync_flush_still_paint() {
    let source = include_str!("../src/term/view.rs");
    let methods = native::body(source, "fn tick(self: &Rc<Self>)")
        + &native::body(source, "fn read(self: &Rc<Self>, condition: IOCondition)");
    let probe =
        include_str!("support/term_idle_tick.rs.txt").replace("// ACTUAL_METHODS", &methods);
    let result = native::execute_source(&probe, &json!({}));
    eprintln!("idle tick receipt: {result}");
    assert_eq!(result["idle_paints"], 0);
    assert_eq!(result["bytes_paints"], 1);
    assert_eq!(result["full_read_pending"], true);
    assert_eq!(result["full_read_paints"], 1);
    assert_eq!(result["sync_early_paints"], 0);
    assert_eq!(result["sync_deadline_paints"], 1);
    assert_eq!(result["sync_after_idle_paints"], 1);
    assert_eq!(result["explicit_sync_paints"], 1);
    assert_eq!(result["blink_paints"], 1);
    assert_eq!(result["unfocused_paints"], 0);
    assert_eq!(result["unmapped_paints"], 0);
    assert_eq!(result["queued_before_map"], true);
    assert_eq!(result["remapped_paints"], 1);
}
