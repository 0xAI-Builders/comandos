#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[allow(dead_code)]
#[path = "support/t18_native.rs"]
mod native;

#[test]
fn local_dashboard_receives_auth_before_loading_without_external_or_shadow_access() {
    let constructor = native::body(include_str!("../src/ui/webview.rs"), "fn create_for_page(");
    let setup = &constructor[constructor.find('{').unwrap() + 1
        ..constructor.find("webview.set_hexpand(true)").unwrap()];
    let probe = include_str!("support/webview_context.rs.txt").replace("// CONSTRUCTOR", setup);
    let receipt = native::execute_source(&probe, &serde_json::json!({}));
    assert_eq!(receipt["authenticated_views"], 3);
    assert_eq!(receipt["shadow_scripts"], 0);
    assert_eq!(receipt["missing_token_scripts"], 0);
    let script = &receipt["script"];
    assert_eq!(script["frames"], "TopFrame");
    assert_eq!(script["time"], "Start");
    assert_eq!(
        script["allow"],
        serde_json::json!(["http://127.0.0.1/*"]),
        "WebKit UserContentURLPattern rejects a port in the host"
    );
    let source = script["source"].as_str().unwrap();
    let source = serde_json::to_string(source).unwrap();
    let node = format!(
        "const assert=require('node:assert/strict'),vm=require('node:vm');const script={source};for(const origin of ['http://127.0.0.1:7337','https://outside.invalid','http://127.0.0.1:73370']){{let calls=[];vm.runInNewContext(script,{{location:{{origin}},localStorage:{{setItem:(...a)=>calls.push(a)}}}});assert.deepEqual(calls,origin==='http://127.0.0.1:7337'?[['cc_token','private-\\\"fixture\\\\token']]:[]);}}"
    );
    let result = std::process::Command::new("node")
        .args(["-e", &node])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
