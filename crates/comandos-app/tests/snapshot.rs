#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::snapshot::{carry_pane_keys, carry_resume_ids};
use serde_json::json;

#[test]
fn snapshot_ids_survive_pane_remap() {
    let previous = json!({"windows":[{"panes":[{"id":"%1","pid":10,"start":20,"command":"claude","agent":"claude","resume_id":"rid","key":"pane-old"}]}]});
    let captured = json!({"windows":[{"panes":[{"id":"%2","pid":11,"start":21,"command":"claude","agent":"claude","resume_id":"rid"}]}]});
    let out = carry_pane_keys(captured, &previous);
    assert_eq!(out["windows"][0]["panes"][0]["key"], "pane-old");
}

#[test]
fn resume_id_carries_only_same_process() {
    let previous = json!({"windows":[{"panes":[{"id":"%1","pid":10,"start":20,"command":"claude","agent":"claude","resume_id":"rid"}]}]});
    let captured = json!({"windows":[{"panes":[{"id":"%1","pid":10,"start":20,"command":"claude","agent":"claude"}]}]});
    let out = carry_resume_ids(captured, &previous);
    assert_eq!(out["windows"][0]["panes"][0]["resume_id"], "rid");
}
