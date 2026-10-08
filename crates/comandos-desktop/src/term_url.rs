//! HTTP terminal URLs preserve the original query order and urlencode spelling.
pub struct TermUrl;
impl TermUrl {
    pub fn build(dash: &str, session: Option<&str>, theme: &str, auth: &str) -> String {
        let mut url = format!(
            "{}/term.html?theme={}&auth={}",
            dash.trim_end_matches('/'),
            quote_plus(theme),
            quote_plus(auth)
        );
        if let Some(session) = session.filter(|s| !s.is_empty()) {
            url.push_str("&arg=");
            url.push_str(&quote_plus(session));
        }
        url
    }
}
fn quote_plus(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'~' => {
                out.push(char::from(b))
            }
            b' ' => out.push('+'),
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}
