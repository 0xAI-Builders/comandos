use super::*;
use serde_json::json;
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};
const OFFICIAL_FEEDS: &[(&str, &str)] = &[
    ("OpenAI", "https://openai.com/news/rss.xml"),
    (
        "Google",
        "https://blog.google/innovation-and-ai/technology/ai/rss/",
    ),
    ("Google DeepMind", "https://deepmind.google/blog/rss.xml"),
    ("Hugging Face", "https://huggingface.co/blog/feed.xml"),
    ("Qwen", "https://qwenlm.github.io/blog/index.xml"),
];
const OFFICIAL_RELEASES: &[(&str, &str)] = &[
    (
        "Anthropic · Claude Code",
        "https://github.com/anthropics/claude-code/releases.atom",
    ),
    (
        "OpenAI · Codex",
        "https://github.com/openai/codex/releases.atom",
    ),
    (
        "Google · Gemini CLI",
        "https://github.com/google-gemini/gemini-cli/releases.atom",
    ),
];
const HF_LABS: &[(&str, &str)] = &[
    ("deepseek-ai", "DeepSeek"),
    ("Qwen", "Qwen"),
    ("mistralai", "Mistral"),
    ("meta-llama", "Meta"),
    ("google", "Google"),
    ("openai", "OpenAI"),
    ("moonshotai", "Moonshot"),
    ("zai-org", "Z.ai"),
    ("MiniMaxAI", "MiniMax"),
    ("nvidia", "NVIDIA"),
    ("microsoft", "Microsoft"),
    ("ibm-granite", "IBM"),
];
const PRESS_FEEDS: &[(&str, &str)] = &[
    (
        "TechCrunch",
        "https://techcrunch.com/category/artificial-intelligence/feed/",
    ),
    (
        "The Verge",
        "https://www.theverge.com/rss/ai-artificial-intelligence/index.xml",
    ),
    (
        "Simon Willison",
        "https://simonwillison.net/atom/everything/",
    ),
];
const SUBREDDITS: &[&str] = &[
    "LocalLLaMA",
    "ClaudeAI",
    "singularity",
    "OpenAI",
    "ChatGPTCoding",
    "artificial",
];
const HN_QUERIES: &[&str] = &[
    "AI",
    "LLM",
    "OpenAI",
    "Anthropic",
    "Claude",
    "Gemini",
    "GPT",
    "model",
    "agent",
    "DeepSeek",
];
const X_ACCOUNTS: &[&str] = &[
    "AnthropicAI",
    "claudeai",
    "OpenAI",
    "OpenAIDevs",
    "GoogleDeepMind",
    "GeminiApp",
    "xai",
    "MistralAI",
    "deepseek_ai",
    "Alibaba_Qwen",
    "huggingface",
    "AIatMeta",
];
const OFFICIAL_DOMAINS: &[(&str, &str)] = &[
    ("(^|\\.)anthropic\\.com$|^claude\\.(ai|com)$", "Anthropic"),
    ("(^|\\.)openai\\.com$", "OpenAI"),
    ("^deepmind\\.google$", "Google DeepMind"),
    ("^blog\\.google$", "Google"),
    ("^developers\\.googleblog\\.com$", "Google"),
    ("(^|\\.)x\\.ai$", "xAI"),
    ("^mistral\\.ai$", "Mistral"),
    ("^ai\\.meta\\.com$", "Meta"),
    ("^qwenlm\\.github\\.io$|^qwen\\.ai$", "Qwen"),
    ("(^|\\.)deepseek\\.com$", "DeepSeek"),
    ("^blogs\\.nvidia\\.com$", "NVIDIA"),
    ("^huggingface\\.co$", "Hugging Face"),
];
fn item(family: &str, origin: &str, title: &str, url: &str, published: Value) -> Value {
    json!({"family":family,"origin":origin,"title":strip_html(title,300),"url":url,"publishedAt":published,"official":false,"lab":null,"discussion":null,"signals":{},"summary":"","heat":0.0,"release":false})
}
fn fail(errors: &mut Vec<Value>, name: &str, e: String) {
    errors.push(json!({"source":name,"error":clip(&e,200)}))
}
fn feed(net: &Network, url: &str) -> Result<Vec<Value>, String> {
    parse_feed(&net.text(
        url,
        "application/rss+xml,application/atom+xml,application/xml,*/*",
    )?)
}
pub fn anthropic_news(page: &str) -> Vec<Value> {
    let page = page.replace("\\\"", "\"");
    let pattern = re(
        r#"(?s)"publishedOn":"([^"]+)","slug":\{"_type":"slug","current":"([a-z0-9-]+)"\}(.{0,1500}?)"title":"([^"]{4,240})""#,
    );
    let summary = re(r#""summary":"([^"]{0,600})""#);
    let mut seen = HashSet::new();
    let mut out = vec![];
    for c in pattern.captures_iter(&page) {
        if !seen.insert(c[2].to_owned()) {
            continue;
        }
        let mut i = item(
            "oficial",
            "Anthropic",
            &html_escape::decode_html_entities(&c[4]),
            &format!("https://www.anthropic.com/news/{}", &c[2]),
            json!(timestamp(&c[1].to_owned().into())),
        );
        i["official"] = true.into();
        i["lab"] = "Anthropic".into();
        i["summary"] = summary
            .captures(&c[3])
            .map(|m| html_escape::decode_html_entities(&m[1]).into_owned())
            .unwrap_or_default()
            .into();
        out.push(i);
        if out.len() >= 12 {
            break;
        }
    }
    out
}
pub fn collect_official(now: i64, net: &Network, errors: &mut Vec<Value>) -> Vec<Value> {
    let mut out = vec![];
    for &(lab, url) in OFFICIAL_FEEDS.iter().chain(OFFICIAL_RELEASES) {
        match feed(net, url) {
            Err(e) => fail(errors, lab, e),
            Ok(rows) => {
                let release = OFFICIAL_RELEASES.contains(&(lab, url));
                for e in rows.iter().take(if release { 10 } else { 12 }) {
                    if release && prerelease(text(&e["title"])) {
                        continue;
                    }
                    let title = if release {
                        format!(
                            "{} {}",
                            lab.split(" · ").last().unwrap_or(lab),
                            text(&e["title"])
                        )
                    } else {
                        text(&e["title"]).into()
                    };
                    let mut i = item(
                        if release { "release" } else { "oficial" },
                        lab,
                        &title,
                        text(&e["url"]),
                        e["publishedAt"].clone(),
                    );
                    i["official"] = true.into();
                    i["lab"] = lab.split(" · ").next().unwrap_or(lab).into();
                    i["summary"] = e["summary"].clone();
                    if release {
                        let p = re(r"\d+\.\d+(?:\.\d+)?");
                        i["release"] = p
                            .find(text(&e["title"]))
                            .is_some_and(|v| re(r"^\d+\.\d+(?:\.0)?$").is_match(v.as_str()))
                            .into()
                    }
                    out.push(i)
                }
            }
        }
    }
    match net.text("https://www.anthropic.com/news", "text/html") {
        Ok(s) => out.extend(anthropic_news(&s)),
        Err(e) => fail(errors, "Anthropic", e),
    }
    for &(author, lab) in HF_LABS {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("author", author)
            .append_pair("sort", "createdAt")
            .append_pair("direction", "-1")
            .append_pair("limit", "6")
            .finish();
        match net.json(&format!("https://huggingface.co/api/models?{query}")) {
            Err(e) => fail(errors, &format!("Hugging Face · {lab}"), e),
            Ok(rows) => {
                for m in rows.as_array().into_iter().flatten() {
                    let Some(created) =
                        timestamp(&m["createdAt"]).filter(|t| *t != 0 && now - *t <= 72 * 3600)
                    else {
                        continue;
                    };
                    let mut i = item(
                        "hf-org",
                        &format!("Hugging Face · {lab}"),
                        text(&m["id"]),
                        &format!("https://huggingface.co/{}", text(&m["id"])),
                        created.into(),
                    );
                    i["official"] = true.into();
                    i["lab"] = lab.into();
                    i["signals"] = json!({"likes":num(&m["likes"]) as i64,"downloads":num(&m["downloads"]) as i64});
                    i["release"] = true.into();
                    out.push(i)
                }
            }
        }
    }
    out
}
pub fn collect_hn(now: i64, net: &Network, errors: &mut Vec<Value>) -> Vec<Value> {
    let mut out = vec![];
    let mut seen = HashSet::new();
    for q in HN_QUERIES {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("query", q)
            .append_pair("tags", "story")
            .append_pair("hitsPerPage", "40")
            .append_pair(
                "numericFilters",
                &format!("created_at_i>{},points>40", now - 36 * 3600),
            )
            .finish();
        let data = match net.json(&format!("https://hn.algolia.com/api/v1/search?{query}")) {
            Ok(d) => d,
            Err(e) => {
                fail(errors, "Hacker News", e);
                break;
            }
        };
        for h in data["hits"].as_array().into_iter().flatten() {
            if !seen.insert(h["objectID"].to_string()) || !ai(text(&h["title"])) {
                continue;
            }
            let points = num(&h["points"]);
            let comments = num(&h["num_comments"]);
            let id = h["objectID"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| h["objectID"].to_string());
            let disc = format!("https://news.ycombinator.com/item?id={id}");
            let target = h["url"].as_str().filter(|s| !s.is_empty()).unwrap_or(&disc);
            let mut i = item(
                "hn",
                "Hacker News",
                text(&h["title"]),
                target,
                h["created_at_i"].clone(),
            );
            i["discussion"] = disc.into();
            i["signals"] =
                json!({"points":points as i64,"comments":comments as i64,"hnId":h["objectID"]});
            i["heat"] =
                (12.0 * (1.0 + points / 50.0).log2() + 4.0 * (1.0 + comments / 40.0).log2()).into();
            out.push(i)
        }
    }
    out
}
pub fn reddit_get(url: &str, net: &Network, sleep: &dyn Fn(Duration)) -> Result<String, String> {
    for attempt in 0..2 {
        match net.text(url, "application/atom+xml,*/*") {
            Ok(s) => return Ok(s),
            Err(e) if e == "HTTP 429" && attempt == 0 => sleep(Duration::from_secs(5)),
            Err(e) => return Err(e),
        }
    }
    Err("Reddit no devolvió respuesta tras dos intentos".into())
}
pub fn collect_reddit_with(
    now: i64,
    net: &Network,
    errors: &mut Vec<Value>,
    sleep: &dyn Fn(Duration),
    clock: &dyn Fn() -> f64,
) -> Vec<Value> {
    let mut out = vec![];
    let start = clock();
    let mut limited = 0;
    for (n, sub) in SUBREDDITS.iter().enumerate() {
        let source = format!("r/{sub}");
        if clock() - start > 45.0 || limited >= 2 {
            fail(
                errors,
                &source,
                "omitido: Reddit limitó las peticiones".into(),
            );
            continue;
        }
        if n > 0 {
            sleep(Duration::from_millis(2500))
        }
        let entries = match reddit_get(
            &format!("https://www.reddit.com/r/{sub}/hot/.rss?limit=25"),
            net,
            sleep,
        )
        .and_then(|x| parse_feed(&x))
        {
            Ok(e) => e,
            Err(e) => {
                limited += usize::from(e.contains("429"));
                fail(errors, &source, e);
                continue;
            }
        };
        for (rank, e) in entries.iter().take(25).enumerate() {
            if e["publishedAt"]
                .as_i64()
                .is_some_and(|p| now - p > 48 * 3600)
            {
                continue;
            }
            let body = html_escape::decode_html_entities(text(&e["html"]));
            let links = re(r#"href="([^"]+)""#);
            let target=links.captures_iter(&body).map(|c|html_escape::decode_html_entities(&c[1]).into_owned()).find(|u|u.starts_with("http")&&!re(r"//(www\.|old\.)?reddit\.com|//redd\.it|//i\.redd\.it|//v\.redd\.it|//preview\.redd").is_match(u));
            if matches!(*sub, "ChatGPT" | "artificial" | "singularity") && !ai(text(&e["title"])) {
                continue;
            }
            let mut i = item(
                "reddit",
                &source,
                text(&e["title"]),
                target.as_deref().unwrap_or(text(&e["url"])),
                e["publishedAt"].clone(),
            );
            i["discussion"] = e["url"].clone();
            i["signals"] = json!({"hotRank":rank+1,"subreddit":sub,"selftext":strip_html(&re(r"(?s)submitted by.*").replace_all(&body,""),2000)});
            i["heat"] = (9.0 * (1.0 - rank as f64 / 25.0)
                + if matches!(*sub, "LocalLLaMA" | "ClaudeAI" | "singularity") {
                    3.0
                } else {
                    0.0
                })
            .into();
            out.push(i)
        }
    }
    out
}
pub fn collect_reddit(now: i64, net: &Network, errors: &mut Vec<Value>) -> Vec<Value> {
    let start = Instant::now();
    collect_reddit_with(now, net, errors, &std::thread::sleep, &|| {
        start.elapsed().as_secs_f64()
    })
}
pub fn collect_github_trending(_now: i64, net: &Network, errors: &mut Vec<Value>) -> Vec<Value> {
    let page = match net.text("https://github.com/trending?since=daily", "text/html") {
        Ok(p) => p,
        Err(e) => {
            fail(errors, "GitHub trending", e);
            return vec![];
        }
    };
    let mut out = vec![];
    for (rank, b) in re(r#"(?s)<article class="Box-row">(.*?)</article>"#)
        .captures_iter(&page)
        .enumerate()
    {
        let Some(repo) = re(r#"<h2[^>]*>\s*<a[^>]*href="/([^"]+)""#).captures(&b[1]) else {
            continue;
        };
        let name = repo[1].trim();
        let desc = re(r#"(?s)<p class="col-9[^"]*">(.*?)</p>"#)
            .captures(&b[1])
            .map(|c| strip_html(&c[1], 400))
            .unwrap_or_default();
        let today = re(r"([\d,]+)\s+stars? today")
            .captures(&b[1])
            .and_then(|c| c[1].replace(',', "").parse::<i64>().ok())
            .unwrap_or(0);
        if !ai(&format!("{name} {desc}")) {
            continue;
        }
        let mut i = item(
            "github",
            "GitHub trending",
            &if desc.is_empty() {
                name.into()
            } else {
                format!("{name}: {desc}")
            },
            &format!("https://github.com/{name}"),
            Value::Null,
        );
        i["signals"] = json!({"starsToday":today,"trendingRank":rank+1});
        i["heat"] = (10.0 * (1.0 + today as f64 / 150.0).log2()).into();
        i["summary"] = desc.into();
        out.push(i)
    }
    out
}
pub fn collect_hf(now: i64, net: &Network, errors: &mut Vec<Value>) -> Vec<Value> {
    let mut out = vec![];
    match net.json("https://huggingface.co/api/models?sort=trendingScore&direction=-1&limit=20") {
        Err(e) => fail(errors, "Hugging Face trending", e),
        Ok(rows) => {
            for (rank, m) in rows.as_array().into_iter().flatten().enumerate() {
                let created = timestamp(&m["createdAt"]);
                if created.is_some_and(|c| now - c > 21 * 24 * 3600) {
                    continue;
                }
                let mut i = item(
                    "hf",
                    "Hugging Face trending",
                    text(&m["id"]),
                    &format!("https://huggingface.co/{}", text(&m["id"])),
                    json!(created),
                );
                i["signals"] = json!({"trendingRank":rank+1,"likes":num(&m["likes"]) as i64,"downloads":num(&m["downloads"]) as i64});
                i["heat"] = (9.0 * (1.0 - rank as f64 / 20.0)).into();
                i["release"] = true.into();
                out.push(i)
            }
        }
    }
    match net.json("https://huggingface.co/api/daily_papers?limit=15") {
        Err(e) => fail(errors, "Hugging Face papers", e),
        Ok(rows) => {
            for p in rows.as_array().into_iter().flatten() {
                let paper = &p["paper"];
                let up = num(&paper["upvotes"]);
                if up < 15.0 {
                    continue;
                }
                let mut i = item(
                    "papers",
                    "Hugging Face papers",
                    paper["title"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(text(&p["title"])),
                    &format!("https://huggingface.co/papers/{}", text(&paper["id"])),
                    json!(timestamp(&p["publishedAt"])),
                );
                i["signals"] = json!({"upvotes":up as i64});
                i["heat"] = (5.0 * (1.0 + up / 15.0).log2()).into();
                i["summary"] = strip_html(text(&paper["summary"]), 800).into();
                out.push(i)
            }
        }
    }
    out
}
pub fn collect_lobsters(now: i64, net: &Network, errors: &mut Vec<Value>) -> Vec<Value> {
    let rows = match net.json("https://lobste.rs/t/ai.json") {
        Ok(r) => r,
        Err(e) => {
            fail(errors, "Lobsters", e);
            return vec![];
        }
    };
    let mut out = vec![];
    for s in rows.as_array().into_iter().flatten() {
        let created = timestamp(&s["created_at"]);
        if created.is_some_and(|c| now - c > 48 * 3600) {
            continue;
        }
        let score = num(&s["score"]);
        let mut i = item(
            "lobsters",
            "Lobsters",
            text(&s["title"]),
            s["url"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(text(&s["comments_url"])),
            json!(created),
        );
        i["discussion"] = s["comments_url"].clone();
        i["signals"] = json!({"points":score as i64,"comments":num(&s["comment_count"]) as i64});
        i["heat"] = (5.0 * (1.0 + score / 10.0).log2()).into();
        out.push(i)
    }
    out
}
pub fn collect_press(now: i64, net: &Network, errors: &mut Vec<Value>) -> Vec<Value> {
    let mut out = vec![];
    for &(name, url) in PRESS_FEEDS {
        match feed(net, url) {
            Err(e) => fail(errors, name, e),
            Ok(rows) => {
                for e in rows.iter().take(15) {
                    if e["publishedAt"]
                        .as_i64()
                        .is_some_and(|p| now - p > 48 * 3600)
                        || name == "Simon Willison"
                            && !ai(&format!(
                                "{} {}",
                                text(&e["title"]),
                                clip(text(&e["summary"]), 300)
                            ))
                    {
                        continue;
                    }
                    let mut i = item(
                        "prensa",
                        name,
                        text(&e["title"]),
                        text(&e["url"]),
                        e["publishedAt"].clone(),
                    );
                    i["summary"] = e["summary"].clone();
                    i["heat"] = 4.0.into();
                    out.push(i)
                }
            }
        }
    }
    out
}
pub fn collect_x(
    _now: i64,
    net: &Network,
    errors: &mut Vec<Value>,
    token: Option<&str>,
) -> Vec<Value> {
    let Some(token) = token.filter(|s| !s.is_empty()) else {
        return vec![];
    };
    let query = format!(
        "({}) -is:retweet -is:reply",
        X_ACCOUNTS
            .iter()
            .map(|a| format!("from:{a}"))
            .collect::<Vec<_>>()
            .join(" OR ")
    );
    let q = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("query", &query)
        .append_pair("max_results", "50")
        .append_pair(
            "tweet.fields",
            "created_at,public_metrics,entities,author_id",
        )
        .append_pair("expansions", "author_id")
        .append_pair("user.fields", "username,name")
        .finish();
    let mut request = Request {
        accept: "application/json".into(),
        ..Request::default()
    };
    request
        .headers
        .insert("Authorization".into(), format!("Bearer {token}"));
    let data = match net
        .fetch(
            &format!("https://api.x.com/2/tweets/search/recent?{q}"),
            &request,
        )
        .and_then(|f| serde_json::from_slice::<Value>(&f.body).map_err(|e| e.to_string()))
    {
        Ok(d) => d,
        Err(e) => {
            fail(errors, "X", e);
            return vec![];
        }
    };
    let mut out = vec![];
    for t in data["data"].as_array().into_iter().flatten() {
        let user = data["includes"]["users"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|u| u["id"] == t["author_id"]);
        let username = user.and_then(|u| u["username"].as_str()).unwrap_or("i");
        let metrics = &t["public_metrics"];
        let likes = num(&metrics["like_count"]);
        let reposts = num(&metrics["retweet_count"]);
        let link = t["entities"]["urls"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|u| u["expanded_url"].as_str())
            .find(|u| !u.is_empty() && !u.contains("x.com"));
        let post = format!("https://x.com/{username}/status/{}", text(&t["id"]));
        let mut i = item(
            "x",
            &format!(
                "X · @{}",
                user.and_then(|u| u["username"].as_str()).unwrap_or("?")
            ),
            &clip(text(&t["text"]), 280),
            link.unwrap_or(&post),
            json!(timestamp(&t["created_at"])),
        );
        i["official"] = true.into();
        i["lab"] = user.map(|u| u["name"].clone()).unwrap_or(Value::Null);
        i["discussion"] = post.into();
        i["signals"] = json!({"likes":likes as i64,"reposts":reposts as i64});
        i["heat"] =
            (8.0 * (1.0 + likes / 500.0).log2() + 4.0 * (1.0 + reposts / 100.0).log2()).into();
        out.push(i)
    }
    out
}
pub fn official_lab(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let host = u.host_str()?.trim_start_matches("www.");
    if host == "huggingface.co" && !u.path().starts_with("/blog") {
        return None;
    }
    OFFICIAL_DOMAINS
        .iter()
        .find(|(p, _)| re(p).is_match(host))
        .map(|(_, s)| (*s).into())
}
pub type Collector = fn(i64, &Network, &mut Vec<Value>) -> Vec<Value>;
pub const COLLECTORS: &[Collector] = &[
    collect_official,
    collect_hn,
    collect_reddit,
    collect_github_trending,
    collect_hf,
    collect_lobsters,
    collect_press,
];
pub fn collect_with(
    now: i64,
    net: &Network,
    collectors: &[Collector],
    x_token: Option<&str>,
) -> Value {
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = collectors
            .iter()
            .map(|f| {
                scope.spawn(move || {
                    let mut errors = vec![];
                    let items = f(now, net, &mut errors);
                    (items, errors)
                })
            })
            .collect();
        let mut xerrors = vec![];
        let x = collect_x(now, net, &mut xerrors, x_token);
        let mut got: Vec<_> = handles
            .into_iter()
            .map(|h| {
                h.join().unwrap_or_else(|_| {
                    (vec![], vec![json!({"source":"recolector","error":"panic"})])
                })
            })
            .collect();
        got.push((x, xerrors));
        got
    });
    let mut items = vec![];
    let mut failures = vec![];
    for (got, errors) in results {
        items.extend(
            got.into_iter()
                .filter(|i| !text(&i["title"]).is_empty() && canonical(text(&i["url"])).is_some()),
        );
        failures.extend(errors)
    }
    for i in &mut items {
        if i["official"] != true
            && let Some(lab) = official_lab(text(&i["url"]))
        {
            i["official"] = true.into();
            i["lab"] = lab.into();
            i["officialUrl"] = true.into()
        }
    }
    json!({"items":items,"failures":failures})
}
pub fn collect(now: i64, net: &Network, x_token: Option<&str>) -> Value {
    collect_with(now, net, COLLECTORS, x_token)
}
