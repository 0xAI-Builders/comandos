use super::*;
use crate::{news_radar as radar, news_watch};
use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
/// Article reader preserves Python's truncation rule (radar fetch rejects oversize).
pub fn read_article(url: &str, net: &radar::Network) -> (bool, String, Option<String>) {
    let Some(mut current) = normalize_url(url) else {
        return (false, String::new(), Some("URL no válida".into()));
    };
    let readable = radar::re(r"^(text/(html|plain)|application/xhtml)");
    let spaces = radar::re(r"[ \t]+");
    let newline = radar::re(r"\s+\n");
    for _ in 0..8 {
        let parsed = match url::Url::parse(&current) {
            Ok(p) => p,
            Err(_) => return (false, String::new(), Some("URL no válida".into())),
        };
        if !(net.resolver)(parsed.host_str().unwrap_or("")) {
            return (false, String::new(), Some("host no público".into()));
        }
        let request = radar::Request {
            accept: "text/html,text/plain".into(),
            max_bytes: 400000,
            timeout: Duration::from_secs(10),
            headers: [("User-Agent".into(), "ComandOS-news/1.0".into())]
                .into_iter()
                .collect(),
        };
        let response = match net.transport.get(&current, &request) {
            Ok(r) => r,
            Err(e) => return (false, String::new(), Some(clip(&e, 120))),
        };
        if matches!(response.status, 301 | 302 | 303 | 307 | 308)
            && response.location.as_ref().is_some_and(|s| !s.is_empty())
        {
            if let Some(location) = response
                .location
                .and_then(|s| parsed.join(&s).ok())
                .and_then(|u| normalize_url(u.as_str()))
            {
                current = location;
                continue;
            }
            return (false, String::new(), Some("redirección no válida".into()));
        }
        if response.status >= 300 {
            return (
                false,
                String::new(),
                Some(format!("HTTP {}", response.status)),
            );
        }
        let kind = response.content_type;
        if !readable.is_match(&kind) {
            return (
                false,
                String::new(),
                Some(format!("tipo no legible: {}", radar::clip(&kind, 40))),
            );
        }
        let bytes = &response.body[..response.body.len().min(400000)];
        let mut text = radar::decode(bytes, &kind);
        if kind.contains("html") {
            text = extract_text(&text)
        }
        let text = spaces.replace_all(&text, " ").into_owned();
        let text = newline.replace_all(&text, "\n").trim().to_owned();
        if text.is_empty() {
            return (false, text, Some("página vacía".into()));
        }
        return (true, radar::clip(&text, 6000), None);
    }
    (
        false,
        String::new(),
        Some("demasiadas redirecciones".into()),
    )
}
fn extract_text(page: &str) -> String {
    use html5ever::tokenizer::*;
    use std::cell::RefCell;
    struct State {
        skip: usize,
        parts: Vec<String>,
        pending: String,
    }
    impl State {
        fn flush(&mut self) {
            if self.skip == 0 && !self.pending.trim().is_empty() {
                self.parts.push(self.pending.trim().into())
            }
            self.pending.clear();
        }
    }
    struct TextSink(RefCell<State>);
    impl TokenSink for TextSink {
        type Handle = ();
        fn process_token(&self, t: Token, _: u64) -> TokenSinkResult<()> {
            let mut st = self.0.borrow_mut();
            match t {
                Token::TagToken(t) => {
                    st.flush();
                    if [
                        "script", "style", "noscript", "svg", "nav", "footer", "header", "form",
                    ]
                    .contains(&t.name.as_ref())
                    {
                        if t.kind == TagKind::StartTag {
                            if !t.self_closing {
                                st.skip += 1;
                            }
                        } else {
                            st.skip = st.skip.saturating_sub(1)
                        }
                    }
                    if t.kind == TagKind::StartTag
                        && !t.self_closing
                        && matches!(t.name.as_ref(), "script" | "style")
                    {
                        return TokenSinkResult::RawData(
                            html5ever::tokenizer::states::RawKind::Rawtext,
                        );
                    }
                }
                Token::CharacterTokens(t) => st.pending.push_str(&t),
                Token::CommentToken(_) | Token::EOFToken => st.flush(),
                _ => {}
            }
            TokenSinkResult::Continue
        }
    }
    let tok = Tokenizer::new(
        TextSink(RefCell::new(State {
            skip: 0,
            parts: vec![],
            pending: String::new(),
        })),
        Default::default(),
    );
    let input = BufferQueue::default();
    input.push_back(page.into());
    let _ = tok.feed(&input);
    tok.end();
    let mut st = tok.sink.0.into_inner();
    st.flush();
    st.parts.join("\n")
}

/// Bound work to at most N workers and preserve input order.
fn parallel<T: Sync, R: Send, F: Fn(&T) -> R + Sync>(jobs: &[T], workers: usize, f: F) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..jobs.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..workers.min(jobs.len()) {
            let f = &f;
            let next = &next;
            let results = &results;
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    let r = f(job);
                    results.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(r);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_else(|e| e.into_inner())
        .into_iter()
        .flatten()
        .collect()
}
pub fn make_fetcher(
    collect: Arc<dyn Fn(i64) -> Result<Value> + Send + Sync>,
    net: radar::Network,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
) -> Arc<Fetcher> {
    Arc::new(move |_policy, limit| {
        let ts = now();
        let result = collect(ts)?;
        let mut items = result["items"].as_array().cloned().unwrap_or_default();
        items.sort_by(|a, b| {
            b["at"]
                .as_f64()
                .unwrap_or(0.0)
                .total_cmp(&a["at"].as_f64().unwrap_or(0.0))
        });
        let mut seen = HashSet::new();
        let mut queues: Vec<(String, VecDeque<Value>)> = vec![];
        for i in items {
            let Some(url) = normalize_url(s(&i["url"])) else {
                continue;
            };
            if !seen.insert(url) {
                continue;
            }
            let source = s(&i["source"]);
            if let Some((_, q)) = queues.iter_mut().find(|(s, _)| s == source) {
                q.push_back(i)
            } else {
                queues.push((source.into(), VecDeque::from([i])))
            }
        }
        let mut chosen = vec![];
        while chosen.len() < limit {
            let before = chosen.len();
            for (_, q) in &mut queues {
                if chosen.len() == limit {
                    break;
                }
                if let Some(i) = q.pop_front() {
                    chosen.push(i)
                }
            }
            if chosen.len() == before {
                break;
            }
        }
        let items = parallel(&chosen, 8, |i| {
            let (ok, text, error) = read_article(s(&i["url"]), &net);
            json!({"url":i["url"],"title":i["title"],"kind":i["kind"],"source":i["source"],"publishedAt":i["publishedAt"],"discoveredAt":ts*1000,"meta":i.get("meta").cloned().unwrap_or(json!({})),"fetchStatus":if ok{"ok"}else{"failed"},"fetchError":error,"text":text})
        });
        Ok(json!({"items":items,"failures":result["failures"]}))
    })
}
/// Explicit adapter for the already ported news_watch sources.
pub fn collect_watch(sources: &news_watch::Sources<'_>, now: i64) -> Value {
    let (items, failures) = sources.collect(now);
    json!({"items":items,"failures":failures.into_iter().map(|f|json!({"source":f.source,"error":f.error})).collect::<Vec<_>>()})
}
pub fn media_dir(xdg: Option<&Path>, home: &Path) -> PathBuf {
    xdg.map(Path::to_owned)
        .unwrap_or_else(|| home.join(".local/state"))
        .join("comandos/news-media")
}
pub type RadarCollect = dyn Fn(i64) -> Result<Value> + Send + Sync;
pub type RecentUrls = dyn Fn() -> Result<Vec<String>> + Send + Sync;
pub fn make_radar_fetcher(
    collect: Arc<RadarCollect>,
    net: radar::Network,
    media: PathBuf,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    recent: Arc<RecentUrls>,
    max_per_story: usize,
) -> Arc<Fetcher> {
    Arc::new(move |p, limit| {
        let ts = now();
        let found = collect(ts)?;
        let seen = recent().unwrap_or_default();
        let ranked = radar::rank(
            found["items"].as_array().map(Vec::as_slice).unwrap_or(&[]),
            ts,
            n(p, "maxStories", 6).max(0) as usize,
            &seen,
            10.0,
        );
        let mut jobs = vec![];
        for (index, g) in ranked.iter().enumerate() {
            let mut items = g["items"].as_array().cloned().unwrap_or_default();
            let primary = radar::primary(&items).cloned();
            items.sort_by(|a, b| {
                (!Some(a).eq(&primary.as_ref()))
                    .cmp(&(!Some(b).eq(&primary.as_ref())))
                    .then_with(|| b["official"].as_bool().cmp(&a["official"].as_bool()))
                    .then_with(|| {
                        b["heat"]
                            .as_f64()
                            .unwrap_or(0.0)
                            .total_cmp(&a["heat"].as_f64().unwrap_or(0.0))
                    })
            });
            let mut articles = vec![];
            let mut talks = vec![];
            let mut seen = HashSet::new();
            for i in items {
                let Some(url) = normalize_url(s(&i["url"])) else {
                    continue;
                };
                let disc = normalize_url(s(&i["discussion"]));
                if disc.as_deref() != Some(&url) && seen.insert(url.clone()) {
                    articles.push((i.clone(), "article", s(&i["url"]).to_owned()))
                }
                if let Some(disc) = disc
                    && matches!(s(&i["family"]), "hn" | "reddit" | "lobsters" | "x")
                    && seen.insert(disc.clone())
                {
                    let original = s(&i["discussion"]).to_owned();
                    talks.push((i, "discussion", original))
                }
            }
            let picked = articles
                .iter()
                .take(3)
                .chain(talks.iter().take(2))
                .chain(articles.iter().skip(3))
                .chain(talks.iter().skip(2));
            for (i, role, url) in picked.take(max_per_story) {
                if jobs.len() >= limit {
                    break;
                }
                jobs.push((g.clone(), index + 1, i.clone(), *role, url.clone()));
            }
        }
        let items = parallel(&jobs, 6, |(g, index, i, role, url)| {
            let (ok, cap, error) = if *role == "discussion" {
                radar::discussion_capture(i, &net)
            } else {
                let fallback = if s(&i["summary"]).is_empty() {
                    vec![]
                } else {
                    vec![json!({"type":"p","text":i["summary"]})]
                };
                radar::capture_page(url, &media, &net, 6, &fallback)
            };
            let text = if ok {
                radar::blocks_text(
                    cap["blocks"].as_array().map(Vec::as_slice).unwrap_or(&[]),
                    14000,
                )
            } else {
                String::new()
            };
            let origin = if *role == "discussion" || i["official"] == true {
                s(&i["origin"]).into()
            } else {
                url::Url::parse(url)
                    .ok()
                    .and_then(|u| {
                        u.host_str()
                            .map(|h| h.trim_start_matches("www.").to_owned())
                    })
                    .unwrap_or_default()
            };
            let mut signals = i["signals"].as_object().cloned().unwrap_or_default();
            signals.remove("selftext");
            json!({"url":url,"title":cap["title"].as_str().filter(|s|!s.is_empty()).unwrap_or(s(&i["title"])),"kind":g["kind"],"source":origin,"publishedAt":i.get("publishedAt").filter(|v|!v.is_null()).unwrap_or(&cap["publishedAt"]),"discoveredAt":ts*1000,"announcementKey":g["key"],"meta":{"role":role,"official":i["official"]==true&&*role=="article","lab":i["lab"],"family":i["family"],"heat":radar::heat_label(i),"signals":signals,"groupKind":g["kind"],"groupLab":g["lab"],"groupRank":index,"groupScore":g["score"]},"fetchStatus":if ok&&!text.is_empty(){"ok"}else{"failed"},"fetchError":if ok&&!text.is_empty(){None}else{error},"text":text,"capture":if ok{cap}else{Value::Null}})
        });
        Ok(json!({"items":items,"failures":found["failures"]}))
    })
}
