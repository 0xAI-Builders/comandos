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

/// Ejecuta `f` en otro hilo y falla si no termina en `limit` (detecta cuelgues).
fn within<T: Send + 'static>(
    limit: std::time::Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(limit)
        .expect("se colgó: no terminó dentro del plazo")
}

#[test]
fn a_silent_broker_times_out_and_poisons_the_client() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls2 = calls.clone();
    let outcome = within(std::time::Duration::from_secs(5), move || {
        // El broker contesta new_page y luego enmudece.
        let mut client = support::fake_broker_with(calls2, false, |name, _| match name {
            "new_page" => Some(support::text(
                "## Pages\n1: about:blank\n2: about:blank [selected]",
            )),
            _ => None,
        });
        client.set_timeouts(
            std::time::Duration::from_millis(200),
            std::time::Duration::from_millis(400),
        );
        let started = std::time::Instant::now();
        let mut page = client.open_page("about:blank").unwrap();
        let err = page.eval("() => 1").unwrap_err();
        let elapsed = started.elapsed();
        // Soltar la página no debe intentar close_page ni colgarse.
        drop(page);
        let again = client.call("list_pages", json!({})).unwrap_err();
        (err, elapsed, again, client.is_poisoned())
    });
    let (err, elapsed, again, poisoned) = outcome;
    assert!(err.contains("sin respuesta"), "{err}");
    assert!(elapsed < std::time::Duration::from_secs(2), "{elapsed:?}");
    assert!(poisoned);
    assert!(again.contains("inutilizable"), "{again}");
    let names: Vec<String> = calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|c| c["name"].as_str().map(str::to_string))
        .collect();
    assert_eq!(names, ["new_page", "evaluate_script"]);
}

#[test]
fn screenshots_get_the_longer_timeout() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let outcome = within(std::time::Duration::from_secs(5), move || {
        let mut client = support::fake_broker_with(calls, false, |name, _| match name {
            "new_page" => Some(support::text("## Pages\n2: about:blank [selected]")),
            _ => None,
        });
        client.set_timeouts(
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(900),
        );
        let mut page = client.open_page("about:blank").unwrap();
        let started = std::time::Instant::now();
        let err = page.screenshot_png(true).unwrap_err();
        (err, started.elapsed())
    });
    let (err, elapsed) = outcome;
    assert!(
        err.contains("take_screenshot") || err.contains("sin respuesta"),
        "{err}"
    );
    assert!(
        elapsed >= std::time::Duration::from_millis(800),
        "{elapsed:?}"
    );
}

#[test]
fn server_pings_are_answered_and_other_requests_refused() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut client = support::fake_broker_with(calls.clone(), true, |name, _| match name {
        "new_page" => Some(support::text("## Pages\n2: about:blank [selected]")),
        _ => Some(support::text("ok")),
    });
    client.call("list_pages", json!({})).unwrap();
    let calls = calls.lock().unwrap();
    let responses: Vec<&Value> = calls.iter().filter_map(|c| c.get("response")).collect();
    let ping = responses.iter().find(|r| r["id"] == "srv-1").unwrap();
    assert_eq!(ping["result"], json!({}));
    let other = responses.iter().find(|r| r["id"] == "srv-x1").unwrap();
    assert_eq!(other["error"]["code"], -32601);
}

#[test]
fn keepalive_is_added_to_ssh_brokers() {
    use xtask::mcp::{broker_argv_from_script, with_keepalive};
    let script = "#!/bin/sh\n# Stream MCP JSONL directly to the remote browser broker.\nexec ssh -T -o BatchMode=yes -o ExitOnForwardFailure=yes -o ConnectTimeout=8 -W 127.0.0.1:19441 macmini\n";
    let argv = broker_argv_from_script(script).unwrap();
    assert_eq!(argv[0], "ssh");
    assert_eq!(argv.last().unwrap(), "macmini");
    let argv = with_keepalive(argv);
    let joined = argv.join(" ");
    assert!(
        joined.starts_with("ssh -o ServerAliveInterval="),
        "{joined}"
    );
    assert!(joined.contains("-o ServerAliveCountMax="), "{joined}");
    assert!(joined.ends_with("-W 127.0.0.1:19441 macmini"), "{joined}");
    // Scripts con algo más que un exec ssh simple no se reinterpretan.
    assert_eq!(
        broker_argv_from_script("#!/bin/sh\nexec ssh \"$@\" macmini\n"),
        None
    );
    assert_eq!(
        broker_argv_from_script("#!/bin/sh\nexport X=1\nexec ssh -W h:1 m\n"),
        None
    );
    // Un broker que no es ssh queda igual.
    assert_eq!(
        with_keepalive(vec!["/tmp/fake".into()]),
        vec!["/tmp/fake".to_string()]
    );
}
