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
    format!(
        "<a href=\"{}\"{} target=\"_blank\" rel=\"noopener noreferrer nofollow\">",
        esc(url),
        if title.is_empty() {
            String::new()
        } else {
            format!(" title=\"{}\"", esc(title))
        }
    )
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
        let href = if link.as_str().contains("://") {
            link.as_str().to_owned()
        } else {
            format!("http://{}", link.as_str())
        };
        if !http_url(&href) {
            continue;
        }
        out.push_str(&esc(text.get(at..link.start()).unwrap_or_default()));
        out.push_str(&anchor(&href, ""));
        out.push_str(&esc(link.as_str()));
        out.push_str("</a>");
        at = link.end();
    }
    out.push_str(&esc(text.get(at..).unwrap_or_default()));
    out
}
/// Live createRenderer profile: raw HTML is text; no remote image elements.
pub fn render(text: &str, _profile: Profile) -> String {
    let mut events = Vec::new();
    let mut links: Vec<(String, String)> = Vec::new();
    let mut code = 0usize;
    let mut skip = 0usize;
    for (event, range) in
        Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH)
            .into_offset_iter()
    {
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
                let image = text.get(range.start..).is_some_and(|s| s.starts_with("!"));
                if http_url(&dest_url) {
                    if image {
                        events.push(Event::Text("!".into()));
                    }
                    events.push(Event::Html(anchor(&dest_url, &title).into()));
                    links.push((dest_url.to_string(), String::new()));
                } else {
                    events.push(Event::Text(
                        text.get(range).unwrap_or_default().to_owned().into(),
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
                if let Some((_, label)) = links.last_mut() {
                    label.push_str(&t);
                }
                if code > 0 || !links.is_empty() {
                    events.push(Event::Text(t));
                } else {
                    events.push(Event::Html(linkified(&t).into()));
                }
            }
            Event::Code(t) => {
                if let Some((_, label)) = links.last_mut() {
                    label.push_str(&t);
                }
                events.push(Event::Code(t));
            }
            Event::Html(t) => {
                events.push(Event::Start(Tag::Paragraph));
                events.push(Event::Text(
                    t.to_string().trim_end_matches('\n').to_owned().into(),
                ));
                events.push(Event::End(TagEnd::Paragraph));
            }
            Event::InlineHtml(t) => events.push(Event::Text(t)),
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
    let attrs = ["href", "target", "rel", "class", "title", "start"]
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
    if text.len() > MAX_INPUT_BYTES {
        return Reply::json(
            StatusCode::PAYLOAD_TOO_LARGE,
            &serde_json::json!({"error":"texto demasiado largo"}),
        );
    }
    Reply::json(
        StatusCode::OK,
        &serde_json::json!({"html":render(text,Profile::News)}),
    )
}
