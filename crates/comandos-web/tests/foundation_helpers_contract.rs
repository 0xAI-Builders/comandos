#[allow(dead_code)]
#[path = "../src/components/foundation_helpers.rs"]
mod helpers;
#[test]
fn markdown_and_paths_never_promote_agent_urls_to_links() {
    assert_eq!(
        helpers::md_html("**bold** <script>x</script> [safe](javascript:bad)", false),
        "<b>bold</b> &lt;script&gt;x&lt;/script&gt; <u>safe</u><br>"
    );
    assert!(
        helpers::md_html("/tmp/example/file.rs:12", false)
            .contains("data-path=\"/tmp/example/file.rs\"")
    );
}
#[test]
fn formatting_and_selection_keep_original_contract() {
    assert_eq!(helpers::fmt_money(0.0001), "$0.0001");
    assert_eq!(helpers::fmt_tokens(1234000.0), "1.2M tok");
    assert_eq!(
        helpers::short_path("/home/jesus/codebase/Project"),
        "Project"
    );
    assert_eq!(helpers::ago_txt(100.0, 3761.0, false), "hace 1 h 1 min");
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn mount_helpers_fixture() -> Result<(), wasm_bindgen::JsValue> {
    helpers::mount()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn attach_helpers_fixture() -> Result<(), wasm_bindgen::JsValue> {
    helpers::attach()
}
#[test]
fn recorded_original_formatting_contract() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("foundation_helpers_golden.json")).expect("golden JSON");
    for case in cases.as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let arg = &case["args"][0];
        let input = arg.as_str().unwrap_or_default();
        let n = helpers::num(arg);
        let actual = match name {
            "mdHtml" => helpers::md_html(input, false),
            "mdInline" => helpers::md_inline(input),
            "mdStrip" => helpers::md_strip(input),
            "shortPath" => helpers::short_path(input),
            "agoTxt" => helpers::ago_txt(n, 3761.0, false),
            "fmtMoney" => helpers::fmt_money(n),
            "fmtTokens" => helpers::fmt_tokens(n),
            "confidenceLabel" => helpers::confidence(input).to_string(),
            "shortModel" => helpers::short_model(input),
            _ => panic!("unknown golden function"),
        };
        assert_eq!(
            actual,
            case["expected"].as_str().expect("result"),
            "{name}({arg})"
        );
    }
}
#[test]
fn selection_respects_clicked_pane_timestamp_and_operable_filter() {
    use serde_json::json;
    let items = json!([{"session":"s","pane":"%1","alive":true},{"session":"s","pane":"%2","alive":true},{"session":"local","alive":true},{"session":"hidden","alive":true,"operable":false}]);
    let active = json!({"session":"s","pane":"%2","ts":3});
    assert_eq!(
        helpers::row_key(&helpers::pick_sel(&items, "s|%1", 4000.0, &active)),
        "s|%1"
    );
    assert_eq!(
        helpers::row_key(&helpers::pick_sel(&items, "s|%1", 2000.0, &active)),
        "s|%2"
    );
    assert_eq!(
        helpers::row_key(&helpers::pick_sel(&items, "", 0.0, &json!({}))),
        "local"
    );
    assert!(helpers::pick_sel(&items, "hidden", 0.0, &json!({})).is_null());
    assert!(helpers::pick_sel(&items, "", 0.0, &json!({"session":"s","pane":"missing"})).is_null());
}

#[test]
fn new_sessions_preserve_the_users_codex_default() {
    let defaults = helpers::ns_defaults();
    assert_eq!(defaults["harness"], "codex");
    assert_eq!(defaults["motor"], "codex");
    assert_eq!(defaults["routeId"], "codex:codex");
    assert_eq!(defaults["model"], "gpt-6.1-sol");
}
