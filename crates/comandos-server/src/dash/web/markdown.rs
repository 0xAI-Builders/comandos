//! Pure news Markdown rendering. No Native startup, store or agent call.
use crate::{HandlerError, Reply, Request};
use http::StatusCode;
use linkify::{LinkFinder, LinkKind};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};
use std::collections::{HashMap, HashSet};

pub const MAX_INPUT_BYTES: usize = 256 * 1024;
#[derive(Debug, Clone, Copy)]
pub enum Profile {
    News,
}
fn esc(s: &str) -> String {
    comandos_web_view::escape::text(s)
}
fn http_url(s: &str) -> bool {
    let s = s.trim().to_ascii_lowercase();
    s.starts_with("http://") || s.starts_with("https://")
}
fn host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|s| s.trim_start_matches("www.").to_lowercase())
        })
        .unwrap_or_default()
}
fn anchor(url: &str, title: &str) -> String {
    let url = idna_href(url);
    format!(
        "<a href=\"{}\"{} target=\"_blank\" rel=\"noopener noreferrer nofollow\">",
        esc(&url),
        if title.is_empty() {
            String::new()
        } else {
            format!(" title=\"{}\"", esc(title))
        }
    )
}
// markdown-it normalizes an international hostname without rewriting the
// spelling of the scheme, port, userinfo or path.
fn idna_href(href: &str) -> String {
    let Some((scheme, rest)) = href.split_once("://") else {
        return href.into();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = host_port.split(':').next().unwrap_or(host_port);
    if host.is_ascii() {
        return href.into();
    }
    // The original normalizer's protocol whitelist is case-sensitive. Its
    // uppercase HTTP spelling percent-encodes Unicode rather than applying
    // IDNA; keep the same bytes while the sanitizer still enforces HTTP(S).
    let ascii = if !matches!(scheme, "http" | "https") {
        host.as_bytes()
            .iter()
            .map(|byte| {
                if byte.is_ascii() {
                    char::from(*byte).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect::<String>()
    } else {
        let Some(ascii) = url::Url::parse(href)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
        else {
            return href.into();
        };
        ascii
    };
    let start = scheme.len() + 3 + authority.len() - host_port.len();
    format!(
        "{}{}{}",
        href.get(..start).unwrap_or_default(),
        ascii,
        href.get(start + host.len()..).unwrap_or_default()
    )
}

struct HtmlText {
    source: String,
    marker: String,
}
impl HtmlText {
    fn restore(&self, value: &str) -> String {
        if self.marker.is_empty() {
            value.into()
        } else {
            value.replace(&self.marker, "<")
        }
    }
}

// pulldown has no html:false option. Mask raw HTML's opening punctuation before
// parsing so normal Markdown paragraphs, soft breaks and formatting still run.
// The mask is absent from the input, unbordered and entirely punctuation. Thus
// it cannot overlap literal input or alter emphasis flanking. Restore it in
// text and code before escaping; it never reaches HTML or the JSON boundary.
fn html_as_text(text: &str, options: Options) -> HtmlText {
    let mut positions = HashSet::new();
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        if matches!(event, Event::Html(_) | Event::InlineHtml(_)) {
            for (offset, ch) in text.get(range.clone()).unwrap_or_default().char_indices() {
                if ch != '<' {
                    continue;
                }
                let at = range.start + offset;
                let tail = text.get(at + 1..).unwrap_or_default();
                let angle_http = tail
                    .find('>')
                    .and_then(|end| tail.get(..end))
                    .is_some_and(|url| http_url(url) && !url.chars().any(char::is_whitespace));
                if !angle_http {
                    positions.insert(at);
                }
            }
        }
    }
    if positions.is_empty() {
        return HtmlText {
            source: text.into(),
            marker: String::new(),
        };
    }
    const START: char = '⁅';
    const PUNCT: &[char] = &[
        '⁆', '⁇', '⁈', '⁉', '⁊', '⁋', '⁌', '⁍', '⁎', '⁏', '⁐', '⁑', '⁒', '⁓', '⁔', '⁕', '⁖', '⁗',
        '⁘', '⁙', '⁚', '⁛', '⁜', '⁝', '⁞',
    ];
    let marker = if !text.contains(START) {
        START.to_string()
    } else {
        // There are more candidates than input positions. Every candidate
        // starts with START and its remaining symbols cannot be START.
        let mut width = 5;
        let mut candidates = PUNCT.len().pow(4);
        while candidates <= text.len() {
            width += 1;
            candidates = candidates.saturating_mul(PUNCT.len());
        }
        let chars = text.chars().collect::<Vec<_>>();
        let used = chars
            .windows(width)
            .filter_map(|window| {
                if window.first() != Some(&START) {
                    return None;
                }
                window.iter().skip(1).try_fold(0usize, |n, ch| {
                    PUNCT
                        .iter()
                        .position(|p| p == ch)
                        .map(|p| n * PUNCT.len() + p)
                })
            })
            .collect::<HashSet<_>>();
        let mut number = (0..candidates)
            .find(|n| !used.contains(n))
            .unwrap_or_default();
        let mut suffix = Vec::new();
        for _ in 1..width {
            suffix.push(PUNCT.get(number % PUNCT.len()).copied().unwrap_or('⁆'));
            number /= PUNCT.len();
        }
        std::iter::once(START)
            .chain(suffix.into_iter().rev())
            .collect()
    };
    let mut source = String::with_capacity(text.len());
    for (at, ch) in text.char_indices() {
        if positions.contains(&at) {
            source.push_str(&marker);
        } else {
            source.push(ch);
        }
    }
    HtmlText { source, marker }
}
fn tail(text: &str, url: &str) -> String {
    let Ok(pattern) =
        regex::Regex::new(r"(?i)^(?:https?://)?((?:[a-z0-9-]+\.)+[a-z]{2,})(?:[/:?#]\S*)?$")
    else {
        return String::new();
    };
    let label = pattern
        .captures(text.trim())
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim_start_matches("www.").to_lowercase());
    let host = host(url);
    if label.is_some_and(|l| l != host) {
        format!(" <span class=\"nr-host\">({})</span>", esc(&host))
    } else {
        String::new()
    }
}
fn linkified(text: &str) -> String {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]).url_must_have_scheme(true);
    let mut out = String::new();
    let mut at = 0;
    for link in finder.links(text) {
        // fuzzyEmail=false: a domain within an e-mail is not a link either.
        if text.get(..link.start()).is_some_and(|s| s.ends_with('@')) {
            continue;
        }
        let visible = link
            .as_str()
            .split(['«', '»', '“', '”', '‘', '’', '…'])
            .next()
            .unwrap_or_default();
        let href = if visible.contains("://") {
            visible.replace('\\', "%5C")
        } else {
            format!("http://{visible}")
        };
        if !http_url(&href) {
            continue;
        }
        out.push_str(&esc(text.get(at..link.start()).unwrap_or_default()));
        out.push_str(&anchor(&href, ""));
        out.push_str(&esc(visible));
        out.push_str("</a>");
        at = link.start() + visible.len();
    }
    out.push_str(&esc(text.get(at..).unwrap_or_default()));
    out
}
// Markdown-it detects a bare URL before processing punctuation escapes.
// Pulldown removes those backslashes and emits separate Text spans. Recover
// bytes only for raw autolinks whose boundaries map to the decoded text run;
// ordinary Markdown escapes, entities, explicit links and code stay decoded.
fn restore_autolink_escapes(
    source: &str,
    spans: &[(std::ops::Range<usize>, String)],
    decoded: &str,
) -> String {
    let Some(first) = spans.first() else {
        return decoded.into();
    };
    let Some(last) = spans.last() else {
        return decoded.into();
    };
    let Some(raw) = source.get(first.0.start..last.0.end) else {
        return decoded.into();
    };
    let offset = |at: usize| {
        let mut decoded_at = 0;
        for (range, value) in spans {
            if at == range.start {
                return Some(decoded_at);
            }
            if at == range.end {
                return Some(decoded_at + value.len());
            }
            if range.contains(&at) && source.get(range.clone()) == Some(value.as_str()) {
                return Some(decoded_at + at - range.start);
            }
            decoded_at += value.len();
        }
        None
    };
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]).url_must_have_scheme(true);
    let mut out = String::new();
    let mut at = 0;
    for link in finder.links(raw) {
        if !link.as_str().contains('\\') || !http_url(link.as_str()) {
            continue;
        }
        if raw.get(..link.start()).is_some_and(|s| s.ends_with('@')) {
            continue;
        }
        let (Some(start), Some(end)) = (
            offset(first.0.start + link.start()),
            offset(first.0.start + link.end()),
        ) else {
            continue;
        };
        if start < at {
            continue;
        }
        let Some(prefix) = decoded.get(at..start) else {
            continue;
        };
        if decoded.get(start..end).is_none() {
            continue;
        }
        out.push_str(prefix);
        out.push_str(link.as_str());
        at = end;
    }
    out.push_str(decoded.get(at..).unwrap_or_default());
    out
}
/// Live createRenderer profile: raw HTML is text; no remote image elements.
pub fn render(text: &str, _profile: Profile) -> String {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let prepared = html_as_text(text, options);
    let source = &prepared.source;
    let mut events = Vec::new();
    let mut links: Vec<(String, String)> = Vec::new();
    let mut code = 0usize;
    let mut skip = 0usize;
    let mut strikes = Vec::new();
    // A single tilde is literal in markdown-it. Convert these boundaries
    // before Text coalescing, so a URL's closing tilde remains part of the
    // same text run and receives the original autolink treatment.
    let parsed = Parser::new_ext(source, options)
        .into_offset_iter()
        .map(|(event, mut range)| {
            let event = match event {
                Event::Start(Tag::Strikethrough) => {
                    let double = source
                        .get(range.start..)
                        .is_some_and(|s| s.starts_with("~~"));
                    strikes.push(double);
                    if double {
                        Event::Start(Tag::Strikethrough)
                    } else {
                        range.end = range.start + 1;
                        Event::Text("~".into())
                    }
                }
                Event::End(TagEnd::Strikethrough) => {
                    if strikes.pop().unwrap_or(false) {
                        Event::End(TagEnd::Strikethrough)
                    } else {
                        range.start = range.end.saturating_sub(1);
                        Event::Text("~".into())
                    }
                }
                event => event,
            };
            (event, range)
        })
        .collect::<Vec<_>>();
    let mut parser = parsed.into_iter().peekable();
    while let Some((event, range)) = parser.next() {
        if skip > 0 {
            match event {
                Event::Start(_) => skip += 1,
                Event::End(_) => skip -= 1,
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                code += 1;
                events.push(Event::Start(Tag::CodeBlock(kind)));
            }
            Event::End(TagEnd::CodeBlock) => {
                code = code.saturating_sub(1);
                events.push(Event::End(TagEnd::CodeBlock));
            }
            Event::Start(Tag::Link {
                dest_url, title, ..
            })
            | Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                let image = source
                    .get(range.start..)
                    .is_some_and(|s| s.starts_with("!"));
                if http_url(&dest_url) {
                    if image {
                        events.push(Event::Text("!".into()));
                    }
                    events.push(Event::Html(anchor(&dest_url, &title).into()));
                    links.push((dest_url.to_string(), String::new()));
                } else {
                    events.push(Event::Text(
                        prepared
                            .restore(source.get(range).unwrap_or_default())
                            .into(),
                    ));
                    skip = 1;
                }
            }
            Event::End(TagEnd::Link) | Event::End(TagEnd::Image) => {
                if let Some((url, label)) = links.pop() {
                    events.push(Event::Html(format!("</a>{}", tail(&label, &url)).into()));
                }
            }
            Event::Text(t) => {
                // Literal punctuation and entities can split one visible URL
                // into adjacent Text events. Linkify the complete text run.
                let mut spans = vec![(range, t.to_string())];
                let mut t = t.to_string();
                while matches!(parser.peek(), Some((Event::Text(_), _))) {
                    if let Some((Event::Text(next), next_range)) = parser.next() {
                        t.push_str(&next);
                        spans.push((next_range, next.to_string()));
                    }
                }
                let t = if code == 0 && links.is_empty() {
                    restore_autolink_escapes(source, &spans, &t)
                } else {
                    t
                };
                let t = prepared.restore(&t);
                if let Some((_, label)) = links.last_mut() {
                    label.push_str(&t);
                }
                if code > 0 || !links.is_empty() {
                    events.push(Event::Text(t.into()));
                } else {
                    events.push(Event::Html(linkified(&t).into()));
                }
            }
            Event::Code(t) => {
                let t = prepared.restore(&t);
                if let Some((_, label)) = links.last_mut() {
                    label.push_str(&t);
                }
                events.push(Event::Code(t.into()));
            }
            Event::Html(t) => {
                events.push(Event::Start(Tag::Paragraph));
                events.push(Event::Html(
                    linkified(&prepared.restore(t.trim_end_matches('\n'))).into(),
                ));
                events.push(Event::End(TagEnd::Paragraph));
            }
            Event::InlineHtml(t) => {
                events.push(Event::Html(linkified(&prepared.restore(&t)).into()))
            }
            Event::Start(Tag::Strikethrough) => events.push(Event::Html("<s>".into())),
            Event::End(TagEnd::Strikethrough) => events.push(Event::Html("</s>".into())),
            Event::Start(Tag::Table(_)) => {
                events.push(Event::Html("<div class=\"nr-table-wrap\"><table>".into()))
            }
            Event::End(TagEnd::Table) => events.push(Event::Html("</table></div>".into())),
            other => events.push(other),
        }
    }
    let mut raw = String::new();
    html::push_html(&mut raw, events.into_iter());
    let tags: HashSet<_> = [
        "p",
        "br",
        "strong",
        "em",
        "del",
        "s",
        "a",
        "ul",
        "ol",
        "li",
        "blockquote",
        "code",
        "pre",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hr",
        "table",
        "thead",
        "tbody",
        "tr",
        "th",
        "td",
        "div",
        "span",
    ]
    .into_iter()
    .collect();
    // The live DOMPurify URI policy removes numeric `start` values. Preserve
    // that visible list numbering; only target/rel are explicitly URI-safe.
    let attrs = ["href", "target", "rel", "class", "title"]
        .into_iter()
        .collect();
    ammonia::Builder::default()
        .tags(tags)
        .tag_attributes(HashMap::new())
        .generic_attributes(attrs)
        .url_schemes(["http", "https"].into_iter().collect())
        .url_relative(ammonia::UrlRelative::Deny)
        .link_rel(None)
        .clean(&raw)
        .to_string()
}
/// Scoped browser-string decoder; other POST domains retain their existing JSON policy.
pub fn parse_request(raw: &[u8]) -> Option<serde_json::Value> {
    let raw = std::str::from_utf8(raw).ok()?;
    serde_json::from_str(&comandos_web_view::utf16::json_to_unicode(raw)).ok()
}
pub fn handle(request: &Request) -> Result<Reply, HandlerError> {
    if request.body.len() > MAX_INPUT_BYTES {
        return Reply::json(
            StatusCode::PAYLOAD_TOO_LARGE,
            &serde_json::json!({"error":"texto demasiado largo"}),
        );
    }
    let Some(body) = request.data.as_ref() else {
        return Reply::json(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({"error":"JSON invalido"}),
        );
    };
    if body.get("profile").and_then(serde_json::Value::as_str) != Some("news") {
        return Reply::json(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({"error":"perfil desconocido"}),
        );
    }
    let Some(text) = body.get("text").and_then(serde_json::Value::as_str) else {
        return Reply::json(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({"error":"text debe ser texto"}),
        );
    };
    let text_bytes = char::decode_utf16(comandos_web_view::utf16::decode(text))
        .map(|c| c.map(|c| c.len_utf8()).unwrap_or(3))
        .sum::<usize>();
    if text_bytes > MAX_INPUT_BYTES {
        return Reply::json(
            StatusCode::PAYLOAD_TOO_LARGE,
            &serde_json::json!({"error":"texto demasiado largo"}),
        );
    }
    let output = serde_json::json!({"html":render(text,Profile::News)}).to_string();
    Ok(Reply::bytes(
        StatusCode::OK,
        "application/json",
        comandos_web_view::utf16::json_to_javascript(&output).into_bytes(),
    ))
}
