//! Python desktop wrapped URL/local-path rules; native OSC8 stays in engine/select.
use regex::Regex;
use std::sync::OnceLock;
const WRAP: &str = r"[ \t]*\r?\n[ \t]*";
const URL: &str = r#"https?://(?:[^\s'"<>\]\)]+|[ \t]*\r?\n[ \t]*[^\s'"<>\]\)]+)+"#;
fn wrap() -> Option<&'static Regex> {
    static VALUE: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(WRAP)).as_ref().ok()
}
fn patterns() -> Option<&'static [Regex; 2]> {
    static VALUE: OnceLock<Result<[Regex; 2], regex::Error>> = OnceLock::new();
    VALUE
        .get_or_init(|| {
            Ok([
                Regex::new(URL)?,
                Regex::new(r"(?:~|/)(?:[\w.+@%,=~/-]+|[ \t]*\r?\n[ \t]*[\w.+@%,=~/-]+)+(?::\d+)?")?,
            ])
        })
        .as_ref()
        .ok()
}
pub fn normalize_wrapped_url_text(text: &str) -> String {
    let (Some(patterns), Some(wrap)) = (patterns(), wrap()) else {
        return text.to_owned();
    };
    let Some(url) = patterns.first() else {
        return text.to_owned();
    };
    url.replace_all(text, |caps: &regex::Captures<'_>| {
        wrap.replace_all(caps.get(0).map_or("", |m| m.as_str()), "")
            .into_owned()
    })
    .into_owned()
}
pub fn url_from_wrapped_text(
    text: &str,
    fragment: &str,
    row: Option<usize>,
    col: Option<usize>,
) -> Option<String> {
    let (Some(patterns), Some(wrap)) = (patterns(), wrap()) else {
        return None;
    };
    let point = row.zip(col).and_then(|(row, col)| {
        let mut offset = 0;
        for (index, line) in text.split_inclusive('\n').enumerate() {
            if index == row {
                return Some(offset + line.char_indices().nth(col).map_or(line.len(), |(i, _)| i));
            }
            offset += line.len();
        }
        None
    });
    let frag = normalize_wrapped_url_text(fragment).trim().to_owned();
    let mut candidates = Vec::new();
    for pattern in patterns {
        for m in pattern.find_iter(text) {
            let joined = wrap.replace_all(m.as_str(), "").into_owned();
            let body = joined
                .rsplit_once(':')
                .filter(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                .map_or(joined.as_str(), |(body, _)| body);
            if !joined.starts_with("http") && body.starts_with('/') && body.matches('/').count() < 2
            {
                continue;
            }
            candidates.push((m.start(), m.end(), joined));
        }
    }
    let candidates: Vec<_> = candidates
        .iter()
        .enumerate()
        .filter(|(index, m)| {
            !candidates
                .iter()
                .enumerate()
                .any(|(other, o)| other != *index && o.0 <= m.0 && m.1 <= o.1)
        })
        .map(|(_, m)| m)
        .collect();
    for candidate in &candidates {
        if (!frag.is_empty() && (candidate.2.contains(&frag) || frag.contains(&candidate.2)))
            || point.is_some_and(|p| candidate.0 <= p && p < candidate.1)
        {
            return Some(candidate.2.clone());
        }
    }
    if frag.is_empty() && point.is_none() && candidates.len() == 1 {
        return candidates.first().map(|c| c.2.clone());
    }
    (!frag.is_empty()).then_some(frag)
}
