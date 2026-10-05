use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
mod support;

use support::real_tools;
use xtask::mcp::{
    Client, EmulationMode, ToolInfo, check_schema, eval_value, image_from_result, request_line,
    selected_page_id,
};

#[test]
fn request_is_one_json_line() {
    let line = request_line(
        7,
        "tools/call",
        json!({"name": "list_pages", "arguments": {}}),
    );
    assert!(line.ends_with('\n') && !line[..line.len() - 1].contains('\n'));
    let v: Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], 7);
}

#[test]
fn screenshot_image_is_extracted() {
    let result = json!({"content": [
        {"type": "text", "text": "Took a screenshot"},
        {"type": "image", "mimeType": "image/png", "data": "iVBORw0KGgo="}]});
    assert_eq!(
        image_from_result(&result).unwrap(),
        vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
    );
    assert!(image_from_result(&json!({"content": [{"type": "text", "text": "x"}]})).is_err());
}

#[test]
fn selected_page_comes_from_the_pages_listing() {
    let text = "## Pages\n1: about:blank\n2: data:text/html,<h1 id=t>Hola</h1> [selected]";
    assert_eq!(selected_page_id(text), Some(2));
    assert_eq!(selected_page_id("## Pages\n1: about:blank"), None);
}

#[test]
fn eval_result_is_parsed_from_the_json_block() {
    let r = json!({"content": [{"type": "text",
        "text": "Script ran on page and returned:\n```json\n[980,2,{\"x\":8}]\n```"}]});
    assert_eq!(eval_value(&r).unwrap(), json!([980, 2, {"x": 8}]));
    let s = json!({"content": [{"type": "text",
        "text": "Script ran on page and returned:\n```json\n\"a\\\"b\"\n```"}]});
    assert_eq!(eval_value(&s).unwrap(), json!("a\"b"));
}

#[test]
fn real_chrome_bg_schema_is_accepted_with_viewport_emulation() {
    let schema = check_schema(&real_tools()).unwrap();
    assert!(schema.viewport_emulation);
}

#[test]
fn old_devtools_schema_without_page_id_fails_clearly() {
    // MCP de Chrome DevTools 1.9.0: take_screenshot sin pageId (preflight S14).
    let mut tools = real_tools();
    for t in &mut tools {
        if t.name == "take_screenshot" {
            t.schema = json!({"type": "object", "properties": {"format": {}, "fullPage": {}}});
        }
    }
    let err = check_schema(&tools).unwrap_err();
    assert!(
        err.contains("take_screenshot") && err.contains("pageId"),
        "{err}"
    );
    let without_new_page: Vec<ToolInfo> = real_tools()
        .into_iter()
        .filter(|t| t.name != "new_page")
        .collect();
    assert!(
        check_schema(&without_new_page)
            .unwrap_err()
            .contains("new_page")
    );
}

#[test]
fn emulate_without_viewport_falls_back_to_resize_and_reports_it() {
    let mut tools = real_tools();
    for t in &mut tools {
        if t.name == "emulate" {
            t.schema =
                json!({"type": "object", "properties": {"pageId": {}}, "required": ["pageId"]});
        }
    }
    assert!(!check_schema(&tools).unwrap().viewport_emulation);
}

fn fake_server(calls: Arc<Mutex<Vec<Value>>>) -> Client {
    let png_1x1 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    support::fake_broker(calls, move |name, _args| match name {
        "new_page" => support::text("## Pages\n1: about:blank\n2: about:blank [selected]"),
        "evaluate_script" => support::eval_reply(&json!({"ready": true})),
        "take_screenshot" => json!({"content": [
            {"type": "text", "text": "Took a screenshot"},
            {"type": "image", "mimeType": "image/png", "data": png_1x1}]}),
        "navigate_page" => json!({"content": [{"type": "text", "text": "boom"}], "isError": true}),
        _ => support::text("ok"),
    })
}

#[test]
fn client_drives_a_page_by_page_id_over_the_fake_broker() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut client = fake_server(calls.clone());
    assert!(client.tools().iter().any(|t| t.name == "emulate"));
    {
        let mut page = client.open_page("about:blank").unwrap();
        assert_eq!(page.id(), 2);
        assert_eq!(
            page.emulate(390, 900, 2, true).unwrap(),
            EmulationMode::Viewport
        );
        assert_eq!(page.eval("() => 1").unwrap(), json!({"ready": true}));
        let png = page.screenshot_png(true).unwrap();
        assert_eq!(xtask::png_diff::decode(&png).unwrap().width, 1);
        let err = page.navigate("http://127.0.0.1:7300/").unwrap_err();
        assert!(err.contains("navigate_page"), "{err}");
        page.close().unwrap();
    }
    let calls = calls.lock().unwrap();
    let names: Vec<&str> = calls.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "new_page",
            "emulate",
            "evaluate_script",
            "take_screenshot",
            "navigate_page",
            "close_page"
        ]
    );
    assert_eq!(
        calls[1]["arguments"],
        json!({"pageId": 2, "viewport": "390x900x2,mobile,touch"})
    );
    for c in &calls[1..] {
        assert_eq!(c["arguments"]["pageId"], 2, "{c}");
    }
    assert_eq!(calls[4]["arguments"]["type"], "url");
}

#[test]
fn unknown_tool_is_refused_before_calling_the_broker() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut client = fake_server(calls.clone());
    assert!(
        client
            .call("lighthouse_audit", json!({}))
            .unwrap_err()
            .contains("lighthouse_audit")
    );
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn dropping_an_open_page_closes_it() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut client = fake_server(calls.clone());
    drop(client.open_page("about:blank").unwrap());
    let calls = calls.lock().unwrap();
    assert_eq!(calls.last().unwrap()["name"], "close_page");
}
