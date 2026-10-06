use super::*;
use html5ever::tokenizer::{BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer};
use serde_json::{Map, json};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    fs::OpenOptions,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
};
const SKIP: &[&str] = &[
    "script", "style", "noscript", "svg", "nav", "footer", "header", "form", "aside", "button",
    "iframe", "template", "select", "dialog", "canvas",
];
fn kind(tag: &str) -> Option<&'static str> {
    match tag {
        "p" | "td" | "dd" => Some("p"),
        "h1" | "h2" | "h3" | "h4" | "dt" => Some("h"),
        "li" => Some("li"),
        "blockquote" => Some("quote"),
        "pre" => Some("code"),
        "figcaption" => Some("caption"),
        _ => None,
    }
}
struct Blocks {
    base: String,
    scope: Option<String>,
    inside: usize,
    skip: usize,
    inline: usize,
    open: Option<(String, String)>,
    blocks: Vec<Value>,
    meta: Map<String, Value>,
    text_chars: usize,
}
impl Blocks {
    // 120 output blocks plus one possible leading title. Discard boilerplate
    // and adjacent duplicates while streaming so a 2.5 MB page with hundreds
    // of thousands of tiny paragraphs cannot amplify into hundreds of MB.
    fn append(&mut self, block: Value) {
        self.text_chars += text(&block["text"]).chars().count();
        if boilerplate(&block) {
            return;
        }
        let key = if block["type"] == "img" {
            text(&block["src"])
        } else {
            text(&block["text"])
        };
        if let Some(last) = self.blocks.last() {
            let last_key = if last["type"] == "img" {
                text(&last["src"])
            } else {
                text(&last["text"])
            };
            // The first heading may disappear after metadata has been read;
            // preserve an equal following paragraph/image for that case.
            let possible_title =
                self.blocks.len() == 1 && last["type"] == "h" && block["type"] != "h";
            if key == last_key && !possible_title {
                return;
            }
        }
        if self.blocks.len() < 121 {
            self.blocks.push(block);
        }
    }

    fn close(&mut self) {
        if let Some((kind, text)) = self.open.take() {
            let t = if kind == "code" {
                text.trim_matches('\n').to_owned()
            } else {
                text.split_whitespace().collect::<Vec<_>>().join(" ")
            }
            .replace("``", "");
            if t.chars().count() > 1 {
                self.append(json!({"type":kind,"text":clip(&t,4000)}));
            }
        }
    }
    fn tag(&mut self, tag: &str, a: &Map<String, Value>, end: bool) {
        if end {
            if self.scope.as_deref() == Some(tag) {
                self.inside = self.inside.saturating_sub(1)
            }
            if SKIP.contains(&tag) {
                self.skip = self.skip.saturating_sub(1);
                return;
            }
            if tag == "code"
                && self.inline > 0
                && let Some((_, v)) = &mut self.open
            {
                v.push('`');
                self.inline -= 1;
            }
            if kind(tag).is_some() {
                self.close()
            }
            return;
        }
        if tag == "meta" {
            let key = a
                .get("property")
                .or_else(|| a.get("name"))
                .map(text)
                .unwrap_or("")
                .to_lowercase();
            if [
                "og:title",
                "og:image",
                "article:published_time",
                "author",
                "og:site_name",
                "description",
                "og:description",
                "twitter:image",
            ]
            .contains(&key.as_str())
                && a.get("content").is_some_and(|v| !text(v).is_empty())
            {
                self.meta.entry(key).or_insert_with(|| a["content"].clone());
            }
            return;
        }
        if tag == "html" && a.contains_key("lang") {
            self.meta
                .insert("lang".into(), clip(text(&a["lang"]), 8).into());
        }
        if tag == "time" && a.contains_key("datetime") {
            self.meta
                .entry("time")
                .or_insert_with(|| a["datetime"].clone());
        }
        if self.scope.as_deref() == Some(tag) {
            self.inside += 1
        }
        if SKIP.contains(&tag) {
            self.skip += 1;
            return;
        }
        if self.skip > 0 || self.inside == 0 {
            return;
        }
        if tag == "img" {
            let mut src = a
                .get("src")
                .or_else(|| a.get("data-src"))
                .map(text)
                .unwrap_or("");
            if src.is_empty() || src.starts_with("data:") {
                src = a
                    .get("srcset")
                    .map(text)
                    .unwrap_or("")
                    .split(',')
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
            };
            if !src.is_empty() && !src.starts_with("data:") {
                self.close();
                self.append(json!({"type":"img","src":join(&self.base,src),"alt":clip(a.get("alt").map(text).unwrap_or(""),200)}));
            }
            return;
        }
        if let Some((kind, t)) = &mut self.open {
            if tag == "br" {
                t.push('\n')
            }
            if tag == "code" && kind != "code" {
                self.inline += 1;
                t.push('`')
            }
        }
        if let Some(k) = kind(tag) {
            self.close();
            self.open = Some((k.into(), String::new()))
        }
    }
}
struct Sink(RefCell<Blocks>);
impl TokenSink for Sink {
    type Handle = ();
    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut b = self.0.borrow_mut();
        match token {
            Token::TagToken(tag) => {
                let attrs = tag
                    .attrs
                    .iter()
                    .map(|a| (a.name.local.to_string(), Value::String(a.value.to_string())))
                    .collect();
                b.tag(tag.name.as_ref(), &attrs, tag.kind == TagKind::EndTag);
                if tag.self_closing {
                    b.tag(tag.name.as_ref(), &Map::new(), true)
                }
                if tag.kind == TagKind::StartTag
                    && !tag.self_closing
                    && matches!(tag.name.as_ref(), "script" | "style")
                {
                    return TokenSinkResult::RawData(
                        html5ever::tokenizer::states::RawKind::Rawtext,
                    );
                }
            }
            Token::CharacterTokens(t) => {
                if b.skip == 0
                    && b.inside > 0
                    && let Some((_, s)) = &mut b.open
                {
                    s.push_str(&t)
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}
pub(crate) fn join(base: &str, path: &str) -> String {
    url::Url::parse(base)
        .and_then(|u| u.join(path))
        .map(|u| u.to_string())
        .unwrap_or_else(|_| path.into())
}
fn parse(page: &str, base: &str, scope: Option<&str>) -> (Vec<Value>, Map<String, Value>, usize) {
    let sink = Sink(RefCell::new(Blocks {
        base: base.into(),
        scope: scope.map(str::to_owned),
        inside: usize::from(scope.is_none()),
        skip: 0,
        inline: 0,
        open: None,
        blocks: vec![],
        meta: Map::new(),
        text_chars: 0,
    }));
    let tokenizer = Tokenizer::new(sink, Default::default());
    let input = BufferQueue::default();
    input.push_back(page.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let mut b = tokenizer.sink.0.into_inner();
    b.close();
    (b.blocks, b.meta, b.text_chars)
}
fn boilerplate(b: &Value) -> bool {
    use std::sync::LazyLock;
    static BOILER: LazyLock<Pattern> = LazyLock::new(|| {
        re(
            r"(?i)^(share|subscribe|sign up|log in|cookie|accept|related|read more|advertisement|follow us|copy link|table of contents|skip to)",
        )
    });
    static SHARE: LazyLock<Pattern> = LazyLock::new(|| {
        re(
            r"(?i)^(x(\.com)?|twitter|facebook|linkedin|e-?mail|mail|threads|bluesky|reddit|whatsapp|copy( link)?|print|share( this)?|hacker news|youtube|instagram|rss)$",
        )
    });
    let t = text(&b["text"]);
    b["type"] != "img"
        && ((BOILER.is_match(t) && t.chars().count() < 80) || SHARE.is_match(t.trim()))
}
pub fn page_blocks(page: &str, base: &str) -> (Vec<Value>, Map<String, Value>) {
    let scope = if re(r"(?i)<article[\s>]").is_match(page) {
        Some("article")
    } else if re(r"(?i)<main[\s>]").is_match(page) {
        Some("main")
    } else {
        None
    };
    let (mut blocks, mut meta, count) = parse(page, base, scope);
    if scope.is_some() && count < 400 {
        let parsed = parse(page, base, None);
        blocks = parsed.0;
        meta = parsed.1;
    }
    let mut out = vec![];
    let mut last = String::new();
    let mut chars = 0;
    for b in blocks {
        let t = text(&b["text"]);
        if b["type"] != "img" {
            if boilerplate(&b) {
                continue;
            }
            if b["type"] == "h"
                && t == meta.get("og:title").map(text).unwrap_or("").trim()
                && out.is_empty()
            {
                continue;
            }
        }
        let key = if t.is_empty() { text(&b["src"]) } else { t };
        if key == last {
            continue;
        }
        last = key.into();
        chars += t.chars().count();
        out.push(b);
        if out.len() >= 120 || chars > 30000 {
            break;
        }
    }
    (out, meta)
}
pub fn parse_feed(xml: &str) -> Result<Vec<Value>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let mut out = vec![];
    for node in doc
        .descendants()
        .filter(|n| n.is_element() && matches!(n.tag_name().name(), "item" | "entry"))
    {
        let mut e = json!({"title":"","url":"","publishedAt":null,"summary":"","html":""});
        for c in node.children().filter(|n| n.is_element()) {
            let t = c.text().unwrap_or("");
            match c.tag_name().name() {
                "title" => e["title"] = strip_html(t, 300).into(),
                "link" => {
                    let href = c.attribute("href").unwrap_or(t.trim());
                    if !href.is_empty()
                        && (text(&e["url"]).is_empty()
                            || matches!(c.attribute("rel"), None | Some("alternate")))
                    {
                        e["url"] = href.into()
                    }
                }
                "pubDate" | "published" | "updated" | "date" if e["publishedAt"].is_null() => {
                    e["publishedAt"] = json!(timestamp(&t.into()))
                }
                "description" | "summary" if text(&e["summary"]).is_empty() => {
                    if text(&e["html"]).is_empty() {
                        e["html"] = t.into()
                    }
                    e["summary"] = strip_html(t, 1200).into()
                }
                "encoded" | "content" => {
                    if !t.is_empty() {
                        e["html"] = t.into()
                    }
                    if text(&e["summary"]).is_empty() {
                        e["summary"] = strip_html(t, 1200).into()
                    }
                }
                _ => {}
            }
        }
        if !text(&e["title"]).is_empty() && !text(&e["url"]).is_empty() {
            out.push(e)
        }
    }
    Ok(out)
}
pub fn save_image(url: &str, media: &Path, net: &Network) -> Result<Option<String>, String> {
    let Ok(f) = net.fetch(
        url,
        &Request {
            accept: "image/avif,image/webp,image/png,image/jpeg,image/gif".into(),
            max_bytes: 4_000_000,
            timeout: std::time::Duration::from_secs(12),
            ..Request::default()
        },
    ) else {
        return Ok(None);
    };
    let kind = f
        .content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let ext = match kind.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        _ => return Ok(None),
    };
    if f.body.len() < 1500 {
        return Ok(None);
    }
    let name = format!(
        "{}.{}",
        &format!("{:x}", Sha256::digest(&f.body))[..32],
        ext
    );
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(media)
        .map_err(|e| e.to_string())?;
    let path = media.join(&name);
    if !path.exists() {
        let tmp = media.join(format!(
            ".{}.part",
            crate::fresh_id("news-image").map_err(|e| e.to_string())?
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            file.write_all(&f.body)?;
            std::fs::rename(&tmp, &path)
        })();
        let _ = std::fs::remove_file(tmp);
        result.map_err(|e: std::io::Error| e.to_string())?;
    }
    Ok(Some(name))
}
pub fn blocks_text(blocks: &[Value], limit: usize) -> String {
    let lines: Vec<String> = blocks
        .iter()
        .filter_map(|b| {
            let t = text(&b["type"]);
            if t == "img" {
                return (!text(&b["alt"]).is_empty())
                    .then(|| format!("[imagen: {}]", text(&b["alt"])));
            }
            let prefix = match t {
                "h" => "## ",
                "li" => "- ",
                "quote" => "> ",
                "caption" => "(pie) ",
                _ => "",
            };
            Some(format!("{prefix}{}", text(&b["text"])))
        })
        .collect();
    clip(&lines.join("\n"), limit)
}
pub fn capture_page(
    url: &str,
    media: &Path,
    net: &Network,
    max_images: usize,
    fallback: &[Value],
) -> (bool, Value, Option<String>) {
    let got = net.fetch(url, &Request::default());
    let error = got.as_ref().err().cloned();
    let final_url = got.as_ref().map(|f| f.final_url.as_str()).unwrap_or(url);
    let mut cap = json!({"finalUrl":final_url,"title":null,"byline":null,"lang":null,"publishedAt":null,"blocks":[],"partial":false});
    let mut blocks;
    if let Ok(f) = &got
        && re(r"^(text/(html|plain)|application/xhtml)").is_match(if f.content_type.is_empty() {
            "text/html"
        } else {
            &f.content_type
        })
    {
        let page = decode(&f.body, &f.content_type);
        let meta;
        if f.content_type.contains("html") || f.content_type.is_empty() {
            (blocks, meta) = page_blocks(&page, final_url)
        } else {
            blocks = page
                .split("\n\n")
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .take(120)
                .map(|p| json!({"type":"p","text":p}))
                .collect();
            meta = Map::new()
        };
        let title = strip_html(meta.get("og:title").map(text).unwrap_or(""), 300);
        cap["title"] = if title.is_empty() {
            Value::Null
        } else {
            title.into()
        };
        cap["lang"] = meta.get("lang").cloned().unwrap_or(Value::Null);
        let by = strip_html(
            meta.get("author")
                .or_else(|| meta.get("og:site_name"))
                .map(text)
                .unwrap_or(""),
            120,
        );
        if !by.is_empty() {
            cap["byline"] = by.into()
        }
        cap["publishedAt"] = json!(timestamp(
            meta.get("article:published_time")
                .or_else(|| meta.get("time"))
                .unwrap_or(&Value::Null)
        ));
        if let Some(hero) = meta.get("og:image").or_else(|| meta.get("twitter:image"))
            && !blocks.iter().take(4).any(|b| b["type"] == "img")
        {
            blocks.insert(
                0,
                json!({"type":"img","src":join(final_url,text(hero)),"alt":text(&cap["title"])}),
            )
        }
        if blocks
            .iter()
            .map(|b| text(&b["text"]).chars().count())
            .sum::<usize>()
            < 300
            && !fallback.is_empty()
        {
            blocks = blocks
                .into_iter()
                .filter(|b| b["type"] == "img")
                .take(1)
                .chain(fallback.iter().cloned())
                .collect();
            cap["partial"] = true.into()
        }
    } else if !fallback.is_empty() {
        blocks = fallback.to_vec();
        cap["partial"] = true.into()
    } else {
        return (
            false,
            cap,
            Some(error.unwrap_or_else(|| {
                format!(
                    "tipo no legible: {}",
                    got.as_ref()
                        .map(|f| clip(&f.content_type, 40))
                        .unwrap_or_default()
                )
            })),
        );
    }
    let mut kept = vec![];
    let mut images = 0;
    for b in blocks {
        if b["type"] == "img" {
            if images >= max_images || canonical(text(&b["src"])).is_none() {
                continue;
            }
            if let Ok(Some(name)) = save_image(text(&b["src"]), media, net) {
                images += 1;
                kept.push(json!({"type":"img","media":name,"alt":text(&b["alt"])}))
            }
        } else {
            kept.push(b)
        }
    }
    let ok = kept.iter().any(|b| b["type"] != "img");
    cap["blocks"] = json!(kept);
    (
        ok,
        cap,
        if ok {
            None
        } else {
            Some(error.unwrap_or("página sin texto legible".into()))
        },
    )
}
pub fn discussion_capture(item: &Value, net: &Network) -> (bool, Value, Option<String>) {
    let mut blocks = vec![];
    let result: Result<(), String> = (|| {
        match text(&item["family"]) {
            "hn" if !item["signals"]["hnId"].is_null() => {
                let id = item["signals"]["hnId"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| item["signals"]["hnId"].to_string());
                let d = net.json(&format!("https://hn.algolia.com/api/v1/items/{id}"))?;
                if !text(&d["text"]).is_empty() {
                    blocks.push(json!({"type":"p","text":strip_html(text(&d["text"]),3000)}))
                }
                for c in d["children"].as_array().into_iter().flatten().take(10) {
                    let t = strip_html(text(&c["text"]), 1200);
                    if !t.is_empty() {
                        blocks.push(json!({"type":"quote","text":format!("{}: {t}",c["author"].as_str().filter(|s|!s.is_empty()).unwrap_or("anónimo"))}))
                    }
                }
            }
            "reddit" => {
                if !text(&item["signals"]["selftext"]).is_empty() {
                    blocks.push(json!({"type":"p","text":item["signals"]["selftext"]}))
                }
                let xml = reddit_get(
                    &format!(
                        "{}/.rss?limit=12",
                        text(&item["discussion"]).trim_end_matches('/')
                    ),
                    net,
                    &std::thread::sleep,
                )?;
                for e in parse_feed(&xml)?.iter().skip(1).take(10) {
                    let t = strip_html(
                        &html_escape::decode_html_entities(text(&e["html"]))
                            .replace("<!-- SC_OFF -->", "")
                            .replace("<!-- SC_ON -->", ""),
                        1200,
                    );
                    let author = text(&e["title"])
                        .split_once(" on ")
                        .map(|(a, _)| a.trim_start_matches("/u/"))
                        .unwrap_or("comentario");
                    if !t.is_empty() {
                        blocks.push(json!({"type":"quote","text":format!("{author}: {t}")}))
                    }
                }
            }
            _ => {
                if !text(&item["summary"]).is_empty() {
                    blocks.push(json!({"type":"p","text":item["summary"]}))
                }
            }
        }
        Ok(())
    })();
    let ok = result.is_ok() && !blocks.is_empty();
    let cap = json!({"finalUrl":item.get("discussion").filter(|v|!v.is_null()).unwrap_or(&item["url"]),"title":item["title"],"byline":item["origin"],"lang":if result.is_ok(){json!("en")}else{Value::Null},"publishedAt":item["publishedAt"],"blocks":blocks,"partial":result.is_err()});
    (
        ok,
        cap,
        result
            .err()
            .map(|e| clip(&e, 120))
            .or_else(|| (!ok).then(|| "hilo sin texto".into())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_candidates_are_bounded_even_for_hostile_html() {
        let page = format!(
            "<article>{}{}</article>",
            "<p>Subscribe</p>".repeat(20000),
            (0..20000)
                .map(|i| format!("<p>Paragraph {i}</p>"))
                .collect::<String>()
        );
        let (candidates, _, chars) = parse(&page, "https://example.test/", Some("article"));
        assert_eq!(candidates.len(), 121);
        assert!(chars > 400);
        let (blocks, _) = page_blocks(&page, "https://example.test/");
        assert_eq!(blocks.len(), 120);
        assert_eq!(blocks[0]["text"], "Paragraph 0");
    }
}
