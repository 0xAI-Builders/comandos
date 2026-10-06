#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[allow(dead_code)]
#[path = "support/t17_native.rs"]
mod native;
use serde_json::json;

fn expression(source: &str, arm: &str) -> String {
    let start = source.find(arm).unwrap();
    let expression = source[start..].split_once("=>").unwrap().1.trim_start();
    let mut depth = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in expression.bytes().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'(' | b'{' | b'[' => depth += 1,
                b')' | b'}' | b']' => depth -= 1,
                b',' if depth == 0 => return expression[..index].trim().to_string(),
                _ => {}
            }
        }
    }
    panic!("unterminated dispatch arm")
}
fn handler() -> String {
    let source = include_str!("../src/ui/app_t17_shelf.rs");
    let start = source
        .find("_ => {\n                            let session")
        .unwrap();
    let brace = start + source[start..].find('{').unwrap();
    let mut depth = 0;
    for (i, byte) in source.as_bytes().iter().enumerate().skip(brace) {
        if *byte == b'{' {
            depth += 1;
        }
        if *byte == b'}' {
            depth -= 1;
            if depth == 0 {
                return source[brace..=i].into();
            }
        }
    }
    panic!("unterminated actual wizard handler")
}

#[test]
fn actual_menu_and_keyboard_dispatch_reach_the_real_handler_with_optional_pane() {
    let menu = expression(
        include_str!("../src/ui/app_t16.rs"),
        "MenuAction::StartAI =>",
    );
    let keyboard = expression(
        include_str!("../src/ui/app_t15.rs"),
        "KeyAction::StartAI =>",
    );
    let source = include_str!("support/t17_wizard_dispatch.rs.txt")
        .replace("// MENU_EXPRESSION", &menu)
        .replace("// KEYBOARD_EXPRESSION", &keyboard)
        .replace("// ACTUAL_HANDLER", &handler());
    let result = native::execute_source(&source, &json!({}));
    assert_eq!(result["menu"], json!(["fixture", "%2"]));
    assert_eq!(result["keyboard"], json!(["fixture", ""]));
    assert_eq!(result["null"], json!(["", ""]));
    assert_eq!(result["invalid"], 3);
    assert_eq!(result["closed"], 0);
}

#[test]
fn actual_wizard_worker_pins_identity_socket_and_resolved_pane_across_lookup() {
    let source = include_str!("../src/ui/app_t17_shelf.rs");
    let start = source
        .find("move || -> Result<(String, String, String), String> {")
        .unwrap();
    let end = start + source[start..].find("\n            move |result|").unwrap();
    let worker = source[start..end].trim().trim_end_matches(',');
    let source =
        include_str!("support/t17_wizard_worker.rs.txt").replace("// ACTUAL_WORKER", worker);
    let result = native::execute_source(&source, &json!({}));
    assert_eq!(result["explicit"], json!(["fixture", "%2", "cwd2"]));
    assert_eq!(result["omitted"], json!(["fixture", "%9", "cwd9"]));
    assert_eq!(result["active_changed"], json!(["fixture", "%9", "cwd9"]));
    for name in [
        "recycled",
        "socket_changed",
        "cancelled",
        "closed",
        "generation_changed",
    ] {
        assert!(result[name].is_null(), "{name}: {result}");
    }
    assert_eq!(result["closed_reads"], 0);
}

#[test]
fn actual_wizard_completion_cannot_publish_after_close_replacement_or_navigation() {
    let source = include_str!("../src/ui/app_t17_shelf.rs");
    let method = &source[source.find("pub(super) fn start_ai_in_pane").unwrap()..];
    let start = method.find("move |result| {").unwrap();
    let end = start + method[start..].find("\n        );").unwrap();
    let callback = method[start..end].trim().trim_end_matches(',');
    let source = include_str!("support/t17_wizard_completion.rs.txt")
        .replace("// ACTUAL_COMPLETION", callback);
    let result = native::execute_source(&source, &json!({}));
    assert_eq!(result["current"], 1);
    for name in [
        "closed",
        "destroyed",
        "cancelled",
        "replaced",
        "navigated",
        "superseded",
    ] {
        assert_eq!(result[name], 0, "{name}: {result}");
    }
}
