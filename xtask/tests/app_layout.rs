#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use serde_json::{Value, json};
use xtask::app_layout::compare_layout;

fn fixture() -> Value {
    json!({
        "schema": 1,
        "environment": {"theme":"oscuro","font":{"family":"Ubuntu Sans Mono","size_px":13},"dpr":1,"viewport":{"width":1280,"height":800}},
        "capture": {"captured_at_unix_ms":123,"process_pid":12,"window_handle":"private-1"},
        "tabs": [{"key":"a","label":"Trabajo","selected":true},{"key":"b","label":"Terminal","selected":false}],
        "workspace": {"type":"split","id":"s1","axis":"x","ratio":0.55,"first":{"type":"tab","tabId":"a"},"second":{"type":"tab","tabId":"b"}},
        "widgets": [{"role":"terminal","tab_key":"a","geometry":{"x":0,"y":40,"width":600,"height":760}}]
    })
}

#[test]
fn a_missing_tab_is_not_an_ignorable_layout_difference() {
    let reference = json!({"tabs":[{"key":"fixture","selected":true}]});
    let candidate = json!({"tabs":[]});
    assert!(
        !compare_layout(&reference, &candidate)
            .expect("valid documents")
            .matches()
    );
}

#[test]
fn declared_capture_metadata_and_one_css_pixel_rounding_are_ignored() {
    let reference = fixture();
    let mut candidate = reference.clone();
    candidate["capture"] =
        json!({"captured_at_unix_ms":456,"process_pid":99,"window_handle":"private-2"});
    candidate["widgets"][0]["geometry"]["x"] = json!(1);
    candidate["widgets"][0]["geometry"]["width"] = json!(599);
    assert!(compare_layout(&reference, &candidate).unwrap().matches());
}

#[test]
fn large_widget_dimensions_do_not_lose_integer_precision() {
    let reference = json!({"widgets":[{"geometry":{"width":18014398509481984_u64}}]});
    let candidate = json!({"widgets":[{"geometry":{"width":18014398509481986_u64}}]});
    assert!(!compare_layout(&reference, &candidate).unwrap().matches());
}

#[test]
fn semantic_order_labels_selection_identity_and_split_ratio_remain_exact() {
    let reference = fixture();
    let alternatives = [
        ("label", json!("Otro")),
        ("selected", json!(false)),
        ("key", json!("different")),
    ];
    for (key, value) in alternatives {
        let mut candidate = reference.clone();
        candidate["tabs"][0][key] = value;
        assert!(
            !compare_layout(&reference, &candidate).unwrap().matches(),
            "{key}"
        );
    }
    let mut candidate = reference.clone();
    candidate["tabs"].as_array_mut().unwrap().reverse();
    assert!(!compare_layout(&reference, &candidate).unwrap().matches());
    let mut candidate = reference.clone();
    candidate["workspace"]["ratio"] = json!(0.56);
    assert!(!compare_layout(&reference, &candidate).unwrap().matches());
    candidate = reference.clone();
    candidate["workspace"]["id"] = json!("s2");
    assert!(!compare_layout(&reference, &candidate).unwrap().matches());
}

#[test]
fn viewport_theme_font_dpr_and_larger_geometry_differences_are_rejected() {
    let reference = fixture();
    for (pointer, value) in [
        ("/environment/theme", json!("claro")),
        ("/environment/font/family", json!("Inter")),
        ("/environment/font/size_px", json!(14)),
        ("/environment/dpr", json!(2)),
        ("/environment/viewport/width", json!(1281)),
        ("/widgets/0/geometry/x", json!(1.01)),
    ] {
        let mut candidate = reference.clone();
        *candidate.pointer_mut(pointer).unwrap() = value;
        assert!(
            !compare_layout(&reference, &candidate).unwrap().matches(),
            "{pointer}"
        );
    }
}

#[test]
fn undeclared_timestamps_and_large_semantic_numbers_are_never_dropped_or_rounded() {
    let reference = json!({"tabs":[{"key":"a","timestamp":1,"id":9007199254740992_u64}]});
    let candidate = json!({"tabs":[{"key":"a","timestamp":2,"id":9007199254740993_u64}]});
    let diff = compare_layout(&reference, &candidate).unwrap();
    assert!(!diff.matches());
    assert_eq!(diff.differences.len(), 2);
}

#[test]
fn extra_content_and_invalid_roots_fail_closed() {
    let reference = fixture();
    let mut candidate = reference.clone();
    candidate["widgets"][0]["notice"] = json!("new content");
    assert!(!compare_layout(&reference, &candidate).unwrap().matches());
    assert!(compare_layout(&json!([]), &reference).is_err());
    assert!(compare_layout(&reference, &json!(null)).is_err());
}

#[test]
fn non_finite_json_numbers_are_rejected_even_in_declared_capture_metadata() {
    let invalid: Value =
        serde_json::from_str(r#"{"tabs":[],"capture":{"captured_at_unix_ms":1e999}}"#).unwrap();
    assert!(compare_layout(&invalid, &invalid).is_err());
}

#[test]
fn geometry_tolerance_never_reaches_nested_models_or_semantic_children() {
    for (reference, candidate) in [
        (
            json!({"widgets":[{"role":"canvas","model":{"geometry":{"width":600}}}]}),
            json!({"widgets":[{"role":"canvas","model":{"geometry":{"width":601}}}]}),
        ),
        (
            json!({"workspace":{"children":[{"geometry":{"width":600}}]}}),
            json!({"workspace":{"children":[{"geometry":{"width":601}}]}}),
        ),
        (
            json!({"widgets":{"0":{"geometry":{"width":600}}}}),
            json!({"widgets":{"0":{"geometry":{"width":601}}}}),
        ),
    ] {
        assert!(!compare_layout(&reference, &candidate).unwrap().matches());
    }
    let reference = json!({"children":[{"children":[{"geometry":{"width":600}}]}]});
    let candidate = json!({"children":[{"children":[{"geometry":{"width":601}}]}]});
    assert!(compare_layout(&reference, &candidate).unwrap().matches());
}

#[test]
fn cli_returns_zero_for_match_one_for_difference_two_for_invalid_input() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "comandos-layout-cli-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let reference = root.join("reference layout.json");
    let candidate = root.join("candidate layout.json");
    fs::write(&reference, serde_json::to_vec(&fixture()).unwrap()).unwrap();
    fs::write(&candidate, serde_json::to_vec(&fixture()).unwrap()).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("app-layout")
            .arg("--reference")
            .arg(&reference)
            .arg("--candidate")
            .arg(&candidate)
            .output()
            .unwrap()
    };
    let matched = run();
    assert_eq!(matched.status.code(), Some(0));
    assert!(
        serde_json::from_slice::<Value>(&matched.stdout).unwrap()["differences"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fs::write(&candidate, b"{\"tabs\":[]}").unwrap();
    assert_eq!(run().status.code(), Some(1));
    fs::write(&candidate, b"invalid JSON").unwrap();
    assert_eq!(run().status.code(), Some(2));
    fs::remove_dir_all(root).unwrap();
}
