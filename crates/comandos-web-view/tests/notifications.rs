use comandos_web_view::notifications::{groups, normalize, render_strip};
use serde_json::json;

#[test]
fn complete_notice_text_is_escaped_and_reachable() {
    let text = "á<&🍅".repeat(1250);
    let html = render_strip(
        &json!({"notices":[{"eventId":"long","category":"info","excerpt":text}],"loaded":true}),
    );
    assert!(html.contains("<summary data-nt-focus=\"full:long\">Ver TODO</summary>"));
    assert!(html.contains(&"á&lt;&amp;🍅".repeat(1250)));
    assert!(!html.contains("á<&"));
}

#[test]
fn grouped_pending_counts_survive_read_and_filter() {
    let notices = vec![
        json!({"eventId":"pending","project":"A","read":true,"sequence":1}),
        json!({"eventId":"fresh","project":"A","read":false,"sequence":2}),
        json!({"eventId":"news","category":"news","sequence":3}),
    ];
    let pending = vec![json!("pending")];
    let found = groups(notices, "pending", &pending);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["pendingCount"], 1);
    assert_eq!(found[0]["unread"], 0);
}

#[test]
fn preferences_preserve_unknown_server_fields() {
    let normalized = normalize(
        &json!({"volume":9,"modes":{"attention":"off","future":"visual"},"serverRevision":7}),
    );
    assert_eq!(normalized["volume"], 1.0);
    assert_eq!(normalized["serverRevision"], 7);
    assert_eq!(normalized["modes"]["future"], "visual");
    assert_eq!(normalized["modes"]["error"], "sound");
}
