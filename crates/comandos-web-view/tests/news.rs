use comandos_web_view::news::{
    blocks_html, inline_text, model_name, note_command, status_label, story_kicker,
};
use serde_json::json;
#[test]
fn captured_sources_escape_text_and_only_name_local_media() {
    assert_eq!(inline_text("<x> `code`"), "&lt;x&gt; <code>code</code>");
    let html = blocks_html(
        &json!([{ "type":"li", "text":"a" }, { "type":"li", "text":"b" }, { "type":"img", "media":"https://evil/p.png" }, { "type":"img", "media":"0123456789abcdef0123456789abcdef.png", "alt":"<x>" }]),
    );
    assert!(html.starts_with("<ul><li>a</li><li>b</li></ul>"));
    assert!(!html.contains("src="));
    assert!(html.contains("data-media=\"0123456789abcdef0123456789abcdef.png\""));
    assert!(html.contains("alt=\"&lt;x&gt;\""));
}
#[test]
fn live_reader_labels_and_explicit_note_command() {
    assert_eq!(status_label("not_published"), "No se generó");
    assert_eq!(
        model_name("opencode:opencode/longcat-2.5-preview-free"),
        "longcat-2.5-preview-free"
    );
    assert_eq!(
        story_kicker(&json!({"category":"hot","meta":{"lab":"Comunidad"}})),
        ("Hot · comunidad".into(), true)
    );
    assert_eq!(note_command(" /NoTa a\nb "), Some("a\nb".into()));
    assert_eq!(note_command("nota a"), None);
}
