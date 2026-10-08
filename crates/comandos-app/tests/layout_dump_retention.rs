#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[allow(dead_code)]
#[path = "support/t18_native.rs"]
mod native;
use serde_json::json;

#[test]
fn actual_diagnostic_releases_candidate_after_ready_and_timeout() {
    let source = include_str!("../src/layout_dump.rs");
    let methods = native::body(source, "pub struct Diagnostic {")
        + &native::body(source, "impl Diagnostic {");
    let probe =
        include_str!("support/layout_dump_retention.rs.txt").replace("// ACTUAL_METHODS", &methods);
    let result = native::execute_source(&probe, &json!({}));
    eprintln!("diagnostic receipt: {result}");
    assert_eq!(result["ready_payload"], true);
    assert_eq!(result["ready_candidate"], false);
    assert_eq!(result["timeout_candidate"], false);
    assert_eq!(result["timeout_issues"], json!(["layout_not_quiet"]));
    assert_eq!(result["done_reopens"], false);
    assert_eq!(result["issues_candidate"], false);
}
