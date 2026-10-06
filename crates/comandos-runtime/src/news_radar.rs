//! News radar: injected HTTP, all eight collectors, ranking and local captures.
mod capture;
mod collectors;
mod network;
mod ranking;
pub use capture::*;
pub use collectors::*;
pub use network::*;
pub use ranking::*;
use serde_json::Value;
pub(crate) fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
pub(crate) fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}
pub(crate) fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
/// Built-in expressions fail closed instead of panicking in the worker.
pub(crate) struct Pattern(Option<regex::Regex>);
pub(crate) fn re(pattern: &str) -> Pattern {
    Pattern(regex::Regex::new(pattern).ok())
}
impl Pattern {
    pub(crate) fn is_match(&self, s: &str) -> bool {
        self.0.as_ref().is_some_and(|r| r.is_match(s))
    }
    pub(crate) fn captures<'h>(&self, s: &'h str) -> Option<regex::Captures<'h>> {
        self.0.as_ref().and_then(|r| r.captures(s))
    }
    pub(crate) fn find<'h>(&self, s: &'h str) -> Option<regex::Match<'h>> {
        self.0.as_ref().and_then(|r| r.find(s))
    }
    pub(crate) fn captures_iter<'r, 'h: 'r>(
        &'r self,
        s: &'h str,
    ) -> impl Iterator<Item = regex::Captures<'h>> + 'r {
        self.0.iter().flat_map(move |r| r.captures_iter(s))
    }
    pub(crate) fn find_iter<'r, 'h: 'r>(
        &'r self,
        s: &'h str,
    ) -> impl Iterator<Item = regex::Match<'h>> + 'r {
        self.0.iter().flat_map(move |r| r.find_iter(s))
    }
    pub(crate) fn replace_all<'h, R: regex::Replacer>(
        &self,
        s: &'h str,
        r: R,
    ) -> std::borrow::Cow<'h, str> {
        match &self.0 {
            Some(re) => re.replace_all(s, r),
            None => std::borrow::Cow::Borrowed(s),
        }
    }
}
pub fn timestamp(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_f64() {
        return Some((if n > 1e11 { n / 1000.0 } else { n }) as i64);
    }
    let s = v.as_str()?.trim();
    if (9..=13).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_digit()) {
        return timestamp(&s.parse::<i64>().ok()?.into());
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .or_else(|_| chrono::DateTime::parse_from_rfc2822(s))
        .map(|v| v.timestamp())
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|v| v.and_utc().timestamp())
        })
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|d| d.and_utc().timestamp())
        })
}
pub fn strip_html(s: &str, limit: usize) -> String {
    let unescaped = html_escape::decode_html_entities(s);
    let replaced = re(r"<[^>]+>").replace_all(&unescaped, " ");
    let unescaped = html_escape::decode_html_entities(&replaced);
    clip(
        &unescaped.split_whitespace().collect::<Vec<_>>().join(" "),
        limit,
    )
}
pub(crate) fn network_normalize(url: &str) -> Option<String> {
    network::normalized(url, false)
}
