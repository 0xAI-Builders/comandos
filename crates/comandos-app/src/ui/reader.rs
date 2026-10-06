//! Reader visibility never changes the sidebar or the selected workspace leaf.
use serde_json::{Value, json};
pub fn reader_layout_state(open: bool, terminal: bool) -> Value {
    json!({"terminals":terminal||!open,"reader":open})
}
pub fn reader_uri(base: &str, id: &str, version: &str) -> Result<String, String> {
    crate::config::loopback_only(base)?;
    let valid = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}@\d{2}:\d{2}$").is_ok_and(|r| r.is_match(id));
    Ok(format!(
        "{}/?panel=news&v={}{}",
        base.trim_end_matches('/'),
        super::webview::encode_query(version),
        if valid {
            format!("&news={}", super::webview::encode_query(id))
        } else {
            String::new()
        }
    ))
}
