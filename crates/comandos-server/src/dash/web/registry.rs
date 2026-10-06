use super::file_stamp::FileStamp;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Script,
    Region {
        marker_start: String,
        marker_end: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub source: String,
    pub sha256: String,
    pub exports: Vec<String>,
    pub deps: Vec<String>,
    pub kind: Kind,
}

impl Entry {
    pub fn script(id: &str, source: &str, sha256: &str, exports: &[&str], deps: &[&str]) -> Self {
        Self {
            id: id.to_string(),
            source: source.to_string(),
            sha256: sha256.to_string(),
            exports: exports.iter().map(|s| s.to_string()).collect(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            kind: Kind::Script,
        }
    }

    pub fn region(
        id: &str,
        source: &str,
        marker_start: &str,
        marker_end: &str,
        sha256: &str,
        exports: &[&str],
        deps: &[&str],
    ) -> Self {
        Self {
            id: id.to_string(),
            source: source.to_string(),
            sha256: sha256.to_string(),
            exports: exports.iter().map(|s| s.to_string()).collect(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            kind: Kind::Region {
                marker_start: marker_start.to_string(),
                marker_end: marker_end.to_string(),
            },
        }
    }

    pub fn cut(&self, html: &str) -> String {
        match &self.kind {
            Kind::Script => cut_script(html, &self.source),
            Kind::Region {
                marker_start,
                marker_end,
            } => cut_region(html, marker_start, marker_end).unwrap_or_else(|| html.to_string()),
        }
    }

    pub fn hash_in_page(&self, html: &str) -> Option<String> {
        match &self.kind {
            Kind::Script => None,
            Kind::Region {
                marker_start,
                marker_end,
            } => region_text(html, marker_start, marker_end).map(|s| sha256_hex(s.as_bytes())),
        }
    }
}

fn cut_script(html: &str, source: &str) -> String {
    let name = source.rsplit('/').next().unwrap_or(source);
    let relative = source.strip_prefix("dash/").unwrap_or(source);
    let mut search_from = 0;
    while let Some(open_rel) = html[search_from..].find("<script") {
        let open = search_from + open_rel;
        if let Some(comment) = html[search_from..].find("<!--").map(|at| search_from + at)
            && comment < open
        {
            let Some(end) = html[comment + 4..].find("-->") else {
                break;
            };
            search_from = comment + 4 + end + 3;
            continue;
        }
        if html
            .as_bytes()
            .get(open + 7)
            .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'>')
        {
            search_from = open + 7;
            continue;
        }
        let Some(close_rel) = html[open..].find("</script>") else {
            break;
        };
        let close = open + close_rel + "</script>".len();
        let tag = &html[open..close];
        if script_src(tag).is_some_and(|src| {
            let path = src
                .split(['?', '#'])
                .next()
                .unwrap_or(src)
                .trim_start_matches('/');
            path == name || path == relative
        }) {
            let end = close + html[close..].strip_prefix('\n').map_or(0, |_| 1);
            let mut out = String::with_capacity(html.len().saturating_sub(end - open));
            out.push_str(&html[..open]);
            out.push_str(&html[end..]);
            return out;
        }
        search_from = close;
    }
    html.to_string()
}

/// Solo atributos del tag de apertura: nunca buscar el nombre en el cuerpo JS.
fn script_src(tag: &str) -> Option<&str> {
    let bytes = tag.as_bytes();
    let mut i = 7;
    while i < bytes.len() {
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if bytes.get(i).is_none_or(|b| *b == b'>') {
            return None;
        }
        let start = i;
        while bytes
            .get(i)
            .is_some_and(|b| !b.is_ascii_whitespace() && !matches!(b, b'=' | b'>'))
        {
            i += 1;
        }
        let key = tag.get(start..i)?;
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if bytes.get(i) != Some(&b'=') {
            continue;
        }
        i += 1;
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let quote = bytes.get(i).copied().filter(|b| matches!(b, b'\'' | b'"'));
        if quote.is_some() {
            i += 1;
        }
        let value_start = i;
        while bytes.get(i).is_some_and(|b| {
            quote.map_or_else(|| !b.is_ascii_whitespace() && *b != b'>', |q| *b != q)
        }) {
            i += 1;
        }
        let value = tag.get(value_start..i)?;
        if key.eq_ignore_ascii_case("src") {
            return Some(value);
        }
        if quote.is_some() {
            i += 1;
        }
    }
    None
}

fn region_bounds(html: &str, start: &str, end: &str) -> Option<(usize, usize)> {
    let a = html.find(start)?;
    let b = html[a + start.len()..].find(end)? + a + start.len();
    Some((a, b))
}

fn region_text<'a>(html: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let (a, b) = region_bounds(html, start, end)?;
    html.get(a..b)
}

fn cut_region(html: &str, start: &str, end: &str) -> Option<String> {
    let (a, b) = region_bounds(html, start, end)?;
    let mut out = String::with_capacity(html.len().saturating_sub(b - a));
    out.push_str(&html[..a]);
    out.push_str(&html[b..]);
    Some(out)
}

#[derive(Debug, Clone)]
pub struct Resolved {
    entries: Vec<Entry>,
    repo: Option<PathBuf>,
    sources: BTreeMap<String, String>,
    hashes: Arc<Mutex<BTreeMap<String, (FileStamp, String)>>>,
}

impl Resolved {
    pub fn from_repo(entries: Vec<Entry>, repo: &Path) -> Self {
        Self {
            entries,
            repo: Some(repo.to_path_buf()),
            sources: BTreeMap::new(),
            hashes: Arc::default(),
        }
    }

    pub fn from_components_dir(repo: Option<PathBuf>, dir: &Path) -> Self {
        let mut entries = Vec::new();
        if let Ok(read) = fs::read_dir(dir) {
            let mut files: Vec<PathBuf> = read.filter_map(Result::ok).map(|e| e.path()).collect();
            files.sort();
            for path in files
                .into_iter()
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
            {
                if let Ok(text) = fs::read_to_string(&path)
                    && let Ok(value) = serde_json::from_str::<Value>(&text)
                    && let Some(entry) = entry_from_json(&value)
                {
                    entries.push(entry);
                }
            }
        }
        Self {
            entries,
            repo,
            sources: BTreeMap::new(),
            hashes: Arc::default(),
        }
    }

    pub fn in_memory(entries: Vec<Entry>, sources: &[(&str, &str)]) -> Self {
        Self {
            entries,
            repo: None,
            sources: sources
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            hashes: Arc::default(),
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn source_hash(&self, entry: &Entry) -> Option<String> {
        if let Some(text) = self.sources.get(&entry.source) {
            return Some(sha256_hex(text.as_bytes()));
        }
        let repo = self.repo.as_ref()?;
        let path = repo.join(&entry.source);
        let mut hashes = self.hashes.lock().unwrap_or_else(|e| e.into_inner());
        let Some(stamp) = FileStamp::read(&path) else {
            hashes.remove(&entry.source);
            return None;
        };
        if let Some((cached_stamp, hash)) = hashes.get(&entry.source)
            && *cached_stamp == stamp
        {
            return Some(hash.clone());
        }
        let text = fs::read(&path).ok()?;
        if FileStamp::read(&path).as_ref() != Some(&stamp) {
            hashes.remove(&entry.source);
            return None;
        }
        let hash = sha256_hex(&text);
        hashes.insert(entry.source.clone(), (stamp, hash.clone()));
        Some(hash)
    }
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn entry_from_json(value: &Value) -> Option<Entry> {
    let id = value.get("id")?.as_str()?;
    let source = value.get("source")?.as_str()?;
    let sha = value.get("sha256")?.as_str()?;
    let exports = strings(value, "exports");
    let deps = strings(value, "deps");
    match value.get("kind")?.as_str()? {
        "script" => Some(Entry {
            id: id.into(),
            source: source.into(),
            sha256: sha.into(),
            exports,
            deps,
            kind: Kind::Script,
        }),
        "region" => Some(Entry {
            id: id.into(),
            source: source.into(),
            sha256: sha.into(),
            exports,
            deps,
            kind: Kind::Region {
                marker_start: value.get("marker_start")?.as_str()?.into(),
                marker_end: value.get("marker_end")?.as_str()?.into(),
            },
        }),
        _ => None,
    }
}
