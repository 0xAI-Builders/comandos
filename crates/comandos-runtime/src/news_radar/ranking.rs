use super::*;
use serde_json::json;
use sha1::{Digest, Sha1};
use std::collections::{BTreeSet, HashMap};
const AI_RE: &str = "\\b(a\\.?i\\.?|ai|llms?|gpt[\\w.\\-]*|chatgpt|claude|anthropic|openai|gemini|deepmind|deepseek|qwen|llama|mistral|grok|xai|copilot|codex|cursor|agents?|agentic|models?|transformers?|diffusion|hugging ?face|ollama|mcp|rag|inference|fine-?tun\\w*|neural|sora|veo|midjourney|nvidia|reasoning|benchmark|tokens?|embedding\\w*|vllm|llama\\.cpp|gguf|lora|multimodal|chatbot\\w*)\\b";
const NOISE_RE: &str = "\\b(customers?|case study|partners?(hip)?|scales?|helping|helps|how .{0,30} uses|ebook|webinar|policy|economic|education|students|teachers|grants?|hiring|careers|events?|summit|podcast|newsletter|recap|community spotlight|year in review)\\b";
const PERSONAL_RE: &str = "\\b(portfolio|portafolio|resume|résumé|curriculum|cv|about me|sobre m[ií]|my (personal )?(site|website|homepage|blog)|software (developer|engineer)|full[- ]?stack developer)\\b";
const PRERELEASE_RE: &str = "(alpha|beta|rc\\d*|nightly|preview|canary|dev)\\b";
const RELEASE_RE: &str = "\\b(meet|new|our next|next[- ]gen\\w*|launch\\w*|releas\\w*|introduc\\w*|announc\\w*|now available|open[- ]?sourc\\w*|weights|lanza\\w*|presenta\\w*|v\\d+(\\.\\d+)*|\\d+\\.\\d+)\\b";
const _TOKEN_RE: &str = "[a-z0-9]+(?:\\.[0-9]+)*";
pub(crate) fn ai(s: &str) -> bool {
    re(&format!("(?i){AI_RE}")).is_match(s)
}
pub(crate) fn prerelease(s: &str) -> bool {
    re(&format!("(?i){PRERELEASE_RE}")).is_match(s)
}
fn matches(pattern: &str, s: &str) -> bool {
    re(&format!("(?i){pattern}")).is_match(s)
}
pub fn tokens(title: &str) -> BTreeSet<String> {
    let lower = title.to_lowercase();
    re(_TOKEN_RE)
        .find_iter(&lower)
        .map(|m| m.as_str().to_owned())
        .filter(|w| {
            (w.len() >= 3 || w.bytes().any(|c| c.is_ascii_digit()))
                && !_STOP.split_whitespace().any(|s| s == w)
        })
        .collect()
}
pub fn similar(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let shared: Vec<_> = a.intersection(b).collect();
    if shared.len() as f64 / a.union(b).count() as f64 >= 0.5 {
        return true;
    }
    shared.iter().any(|w| w.bytes().any(|c| c.is_ascii_digit()))
        && shared.iter().any(|w| {
            !w.bytes().any(|c| c.is_ascii_digit())
                && !_GENERIC.split_whitespace().any(|s| s == w.as_str())
        })
}
pub fn cluster(items: &[Value]) -> Vec<Vec<Value>> {
    let mut order: Vec<_> = (0..items.len()).collect();
    order.sort_by(|&a, &b| {
        items[b]["official"]
            .as_bool()
            .cmp(&items[a]["official"].as_bool())
            .then_with(|| num(&items[b]["heat"]).total_cmp(&num(&items[a]["heat"])))
    });
    let mut groups: Vec<Vec<Value>> = vec![];
    let mut seeds = vec![];
    let mut urls = HashMap::new();
    for k in order {
        let it = &items[k];
        let toks = tokens(text(&it["title"]));
        let keys: BTreeSet<_> = [text(&it["url"]), text(&it["discussion"])]
            .iter()
            .filter_map(|u| canonical(u))
            .filter(|u| !u.contains("news.ycombinator.com") && !u.contains("reddit.com"))
            .collect();
        let home = keys
            .iter()
            .find_map(|u| urls.get(u).copied())
            .or_else(|| seeds.iter().position(|s| similar(&toks, s)))
            .unwrap_or_else(|| {
                seeds.push(toks);
                groups.push(vec![]);
                groups.len() - 1
            });
        groups[home].push(it.clone());
        for u in keys {
            urls.entry(u).or_insert(home);
        }
    }
    groups
}
pub fn age_factor(hours: Option<f64>) -> f64 {
    let Some(h) = hours else { return 0.7 };
    for (limit, factor) in [(12.0, 1.0), (24.0, 0.85), (36.0, 0.6), (72.0, 0.3)] {
        if h < limit {
            return factor;
        }
    }
    0.08
}
pub fn personal(item: &Value) -> bool {
    matches(PERSONAL_RE, text(&item["title"]))
        || (matches!(text(&item["family"]), "reddit" | "hn" | "lobsters")
            && url::Url::parse(text(&item["url"]))
                .is_ok_and(|u| u.path().is_empty() || u.path() == "/"))
}
pub fn primary(group: &[Value]) -> Option<&Value> {
    let key = |i: &Value| {
        if i["officialUrl"] == true {
            return 0;
        }
        match text(&i["family"]) {
            "oficial" => 0,
            "x" => 1,
            "hf-org" => 2,
            "release" => 3,
            "prensa" => 5,
            _ => 4,
        }
    };
    group.iter().min_by(|a, b| {
        key(a)
            .cmp(&key(b))
            .then_with(|| num(&b["heat"]).total_cmp(&num(&a["heat"])))
    })
}
pub fn score(group: &[Value], now: i64) -> f64 {
    if group.is_empty() {
        return 0.0;
    }
    let official: Vec<_> = group.iter().filter(|i| i["official"] == true).collect();
    let families: BTreeSet<_> = group.iter().map(|i| text(&i["family"])).collect();
    let heat: f64 = group.iter().map(|i| num(&i["heat"])).sum();
    let launch = group
        .iter()
        .any(|i| i["release"] == true || matches(RELEASE_RE, text(&i["title"])));
    let mut base = 0.0;
    if official
        .iter()
        .any(|i| matches!(text(&i["family"]), "oficial" | "x") || i["officialUrl"] == true)
    {
        base += 22.0 + if launch { 18.0 } else { 0.0 };
        if !launch
            && official
                .iter()
                .all(|i| matches(NOISE_RE, text(&i["title"])))
        {
            base -= 16.0
        }
    } else if official.iter().any(|i| i["family"] == "hf-org") {
        base += 26.0
    } else if official
        .iter()
        .any(|i| i["family"] == "release" && i["release"] == true)
    {
        base += 24.0
    } else if !official.is_empty() {
        base += 6.0
    }
    if launch {
        base += 6.0
    }
    let breadth = 8.0 * (families.len() as f64 - 1.0);
    if official.is_empty() && !launch && group.iter().all(personal) {
        base -= 0.6 * (heat + breadth)
    }
    let lead = primary(group)
        .and_then(|i| i["publishedAt"].as_i64())
        .filter(|n| *n != 0)
        .or_else(|| group.iter().filter_map(|i| i["publishedAt"].as_i64()).max());
    let value = (base + heat + breadth) * age_factor(lead.map(|t| (now - t) as f64 / 3600.0));
    comandos_store::news::py_round(value, 2).unwrap_or(0.0)
}
pub fn rank(
    items: &[Value],
    now: i64,
    limit: usize,
    seen: &[String],
    min_score: f64,
) -> Vec<Value> {
    let seen: BTreeSet<_> = seen.iter().filter_map(|s| canonical(s)).collect();
    let mut ranked = vec![];
    for group in cluster(items) {
        if group.iter().any(|i| {
            [text(&i["url"]), text(&i["discussion"])]
                .iter()
                .filter_map(|u| canonical(u))
                .any(|u| seen.contains(&u))
        }) {
            continue;
        }
        let s = score(&group, now);
        if s < min_score {
            continue;
        }
        let Some(p) = primary(&group) else { continue };
        let lab = if p["official"] == true {
            text(&p["lab"])
        } else {
            ""
        };
        let Some(url) = canonical(text(&p["url"])) else {
            continue;
        };
        ranked.push(json!({"key":format!("radar:{}",&format!("{:x}",Sha1::digest(url.as_bytes()))[..16]),"score":s,"kind":if lab.is_empty(){"hot"}else{"oficial"},"lab":if lab.is_empty(){"Comunidad"}else{lab},"title":p["title"],"items":group}));
    }
    ranked.sort_by(|a, b| num(&b["score"]).total_cmp(&num(&a["score"])));
    ranked.truncate(limit);
    ranked
}
fn comma(v: &Value) -> String {
    let s = v.as_i64().unwrap_or(0).to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',')
        }
        out.push(c)
    }
    out
}
fn scalar(v: &Value) -> String {
    if v.is_null() {
        "None".into()
    } else if let Some(s) = v.as_str() {
        s.into()
    } else {
        v.to_string()
    }
}
pub fn heat_label(i: &Value) -> String {
    let s = &i["signals"];
    let zero = Value::from(0);
    let v = |k: &str| s.get(k).unwrap_or(&zero);
    match text(&i["family"]) {
        "hn" | "lobsters" => format!(
            "{} pts · {} comentarios",
            scalar(v("points")),
            scalar(v("comments"))
        ),
        "reddit" => format!(
            "#{} en hot de r/{}",
            scalar(&s["hotRank"]),
            scalar(&s["subreddit"])
        ),
        "github" => format!(
            "{} ★ hoy · #{} en trending",
            comma(v("starsToday")),
            scalar(&s["trendingRank"])
        ),
        "hf" | "hf-org" => format!(
            "{}{} ♥ · {} descargas",
            if num(&s["trendingRank"]) != 0.0 {
                format!("#{} trending · ", scalar(&s["trendingRank"]))
            } else {
                String::new()
            },
            comma(v("likes")),
            comma(v("downloads"))
        ),
        "papers" => format!("{} votos en papers del día", scalar(v("upvotes"))),
        "x" => format!("{} ♥ · {} reposts", comma(v("likes")), comma(v("reposts"))),
        _ => {
            if i["official"] == true {
                "oficial".into()
            } else {
                String::new()
            }
        }
    }
}

const _STOP: &str = "the a an and or of to in on for with by from at is are was be as it its this that these those\nnew now how why what when your you our we they their into about over more than not just after vs via can\nwill has have had using use used out get got make made up one two first all also only like\nel la los las un una y o de del en con por para que se su sus es son al lo como más ya nuevo nueva\nshow hn ask";

const _GENERIC: &str = "announces announced announcing releases released release launch launches launched model models\navailable update updates version versions introducing today open source weights preview official users\nsays said report reports ships shipped adds added support supports better faster cheaper";
