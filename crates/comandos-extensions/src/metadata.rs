//! Recorded provenance and reference sizes; no tokenizer tables live here.
use crate::python_json;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, File, OpenOptions},
    future::Future,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const MAX_BYTES: usize = 2_000_000;
pub const TOKENIZER: &str = "cl100k_base";
const MAX_CACHE: usize = 1024;
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
pub fn unknown_size(basis: &str) -> Value {
    json!({"tokens":null,"tokenizer":TOKENIZER,"basis":basis})
}
fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= MAX_BYTES).then_some(bytes)
}
fn read_json(path: &Path) -> Value {
    read_bounded(path)
        .and_then(|bytes| comandos_core::json::parse_slice(&bytes).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}
fn read_size(path: &Path) -> Value {
    crate::config::read_bytes(path)
        .ok()
        .flatten()
        .filter(|b| b.len() <= MAX_BYTES)
        .and_then(|b| comandos_core::json::parse_slice(&b).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn resolve(path: &Path) -> Option<PathBuf> {
    fn resolve_missing(path: &Path, depth: usize) -> Option<PathBuf> {
        if depth > 64 {
            return None;
        }
        match path.canonicalize() {
            Ok(path) => return Some(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
        let parent = path.parent()?;
        if let Ok(target) = fs::read_link(path) {
            return resolve_missing(
                &if target.is_absolute() {
                    target
                } else {
                    parent.join(target)
                },
                depth + 1,
            );
        }
        Some(resolve_missing(parent, depth + 1)?.join(path.file_name()?))
    }
    resolve_missing(path, 0)
}
fn valid_github_source(source: &str) -> bool {
    static SOURCE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    // Python 3.11's re module uses Unicode 14; newer letters must not gain
    // provenance merely because the native regex dependency was upgraded.
    SOURCE
        .get_or_init(|| {
            regex::Regex::new(
                r"\A[[\p{L}\p{N}_.-]&&\p{Age:14.0}]+/[[\p{L}\p{N}_.-]&&\p{Age:14.0}]+\z",
            )
            .expect("constant provenance pattern")
        })
        .is_match(source)
}
pub fn skill_origin(row: &Value, home: &Path) -> Value {
    if let Some(plugin) = row["plugin"].as_str().filter(|s| !s.is_empty()) {
        return json!({"id":format!("plugin:{plugin}"),"label":plugin,"kind":"plugin"});
    }
    let recorded = (|| {
        let path = resolve(Path::new(row["path"].as_str()?))?;
        let root = resolve(&home.join(".agents/skills"))?;
        let rel = path.strip_prefix(root).ok()?;
        let parts: Vec<_> = rel.components().collect();
        if parts.len() != 2 || rel.file_name()? != "SKILL.md" {
            return None;
        }
        let lock = read_json(&home.join(".agents/.skill-lock.json"));
        let record = lock.get("skills")?.get(parts[0].as_os_str().to_str()?)?;
        let source = record["source"].as_str()?;
        if record["sourceType"] != "github" || !valid_github_source(source) {
            return None;
        }
        Some(json!({"id":format!("github:{source}"),"label":source,"kind":"repository"}))
    })();
    recorded
        .unwrap_or_else(|| json!({"id":"unknown","label":"Origen no registrado","kind":"unknown"}))
}
fn size_path(home: &Path, name: &str) -> Option<PathBuf> {
    Some(
        home.join(".local/state/comandos/extensions/sizes")
            .join(format!("{}.json", python_json::digest(&json!(name)).ok()?)),
    )
}
fn integer(value: &Value, nonnegative: bool) -> bool {
    value.as_number().is_some_and(|number| {
        let s = number.to_string();
        let digits = if nonnegative {
            s.as_str()
        } else {
            s.strip_prefix('-').unwrap_or(&s)
        };
        !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
    })
}
pub fn mcp_size(home: &Path, name: &str, spec: &Value, at: f64) -> Value {
    let unknown = || unknown_size("tool-definitions");
    let Some(path) = size_path(home, name) else {
        return unknown();
    };
    let value = read_size(&path);
    let measured = &value["measuredAt"];
    let age = at - measured.as_f64().unwrap_or(f64::NAN);
    if value["basis"] != "tool-definitions"
        || value["tokenizer"] != TOKENIZER
        || !integer(measured, false)
        || !integer(&value["tokens"], true)
        || !(0.0..86400.0).contains(&age)
        || python_json::digest(spec).ok().as_deref() != value["configuration"].as_str()
    {
        return unknown();
    }
    json!({"tokens":value["tokens"],"tokenizer":value["tokenizer"],"basis":value["basis"],"measuredAt":measured})
}
pub(crate) fn private_dir(path: &Path) -> std::io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}
pub(crate) fn lock_file(path: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}
pub(crate) async fn acquire_lock(file: &File, deadline: tokio::time::Instant) -> bool {
    loop {
        match file.try_lock() {
            Ok(()) => return true,
            Err(fs::TryLockError::WouldBlock) => {}
            Err(_) => return false,
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}
static TEMP: AtomicU64 = AtomicU64::new(0);
fn write_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Missing directory"))?;
    let tmp = dir.join(format!(
        ".size-{}-{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let mut f = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)?;
    let result = (|| {
        f.write_all(value.to_string().as_bytes())?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    let _ = fs::remove_file(tmp);
    result
}
/// Fixed-clock convenience for deterministic fixtures.
pub async fn record_mcp_size<F, Fut>(
    home: &Path,
    name: &str,
    spec: &Value,
    tools: &[Value],
    at: f64,
    counter: &F,
) where
    F: Fn(Vec<String>) -> Fut,
    Fut: Future<Output = Vec<Option<u64>>>,
{
    record_mcp_size_with_clock(home, name, spec, tools, || at, counter).await;
}
/// Read wall time after lock acquisition and again after counting.
pub async fn record_mcp_size_with_clock<F, Fut, C>(
    home: &Path,
    name: &str,
    spec: &Value,
    tools: &[Value],
    clock: C,
    counter: &F,
) where
    F: Fn(Vec<String>) -> Fut,
    Fut: Future<Output = Vec<Option<u64>>>,
    C: Fn() -> f64,
{
    if tools.len() > 10000 {
        return;
    }
    let mut definitions = Vec::with_capacity(tools.len());
    for tool in tools {
        if tool["name"].as_str().is_none() {
            return;
        }
        let mut definition = serde_json::Map::new();
        for key in ["name", "description", "inputSchema", "outputSchema"] {
            if let Some(value) = tool.get(key) {
                definition.insert(key.into(), value.clone());
            }
        }
        definitions.push(Value::Object(definition));
    }
    definitions.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let definitions = Value::Array(definitions);
    let Ok(text) = python_json::dumps(&definitions, false, true) else {
        return;
    };
    if text.len() > MAX_BYTES {
        return;
    }
    let (Ok(configuration), Ok(content), Some(path)) = (
        python_json::digest(spec),
        python_json::digest(&definitions),
        size_path(home, name),
    ) else {
        return;
    };
    let Some(dir) = path.parent() else { return };
    // La admisión posee un flock bloqueante: fuera del hilo async para que
    // una medida que espera el mismo guardia no bloquee el contador en curso.
    let caller_home = home.to_path_buf();
    let Ok(Ok(access)) = tokio::task::spawn_blocking(move || {
        comandos_store::domains::caller::CallerAccess::open(&caller_home, "extensions")
    })
    .await
    else {
        return;
    };
    let lock_path = if access.mode() == comandos_store::unified::Mode::Sealed {
        // Mantener la exclusión por recurso sin recrear el árbol legado.
        // La ruta de la conexión también respeta una base de datos reubicada.
        let (Some(db_path), Some(resource)) = (
            access.db().and_then(|db| db.path()),
            path.file_stem().and_then(|name| name.to_str()),
        ) else {
            return;
        };
        let mut name = std::ffi::OsString::from(db_path);
        name.push(format!(".extensions-size-{resource}.lock"));
        std::path::PathBuf::from(name)
    } else {
        if private_dir(dir).is_err() {
            return;
        }
        path.with_extension("json.lock")
    };
    let Ok(lock) = lock_file(&lock_path) else {
        return;
    };
    if !acquire_lock(&lock, tokio::time::Instant::now() + Duration::from_secs(8)).await {
        return;
    }
    let Some((_, doc_name)) = crate::config::state_document(&path) else {
        return;
    };
    let old = access
        .read_document(&doc_name, &path)
        .ok()
        .flatten()
        .filter(|b| b.len() <= MAX_BYTES)
        .and_then(|b| comandos_core::json::parse_slice(&b).ok())
        .unwrap_or_else(|| json!({}));
    // Bajo el flock original, la medida idéntica y vigente evita volver a contar.
    let age = clock() - old["measuredAt"].as_f64().unwrap_or(f64::NAN);
    if old["content"] == content
        && old["configuration"] == configuration
        && old["basis"] == "tool-definitions"
        && old["tokenizer"] == TOKENIZER
        && integer(&old["tokens"], true)
        && integer(&old["measuredAt"], false)
        && (0.0..86400.0).contains(&age)
    {
        return;
    }
    let counts = counter(vec![text]).await;
    let [Some(count)] = counts.as_slice() else {
        return;
    };
    let data = json!({"tokens":count,"tokenizer":TOKENIZER,"basis":"tool-definitions","configuration":configuration,"content":content,"measuredAt":clock().floor() as i64});
    let bytes = data.to_string().into_bytes();
    let _ = access.write(
        || Ok(write_atomic(&path, &data)?),
        |db, origin| {
            comandos_store::unified::doc_put(
                db,
                &doc_name,
                "extensions",
                &bytes,
                origin,
                (clock() * 1000.0) as i64,
            )
            .map(|_| ())
        },
    );
    drop(lock);
}
/// Best-effort complete lists retain at most 64 KiB of cursor contents and
/// 1024 cursor entries, including the expected continuation and cycle history.
/// Exceeding either limit discards the measurement, without affecting MCP frames.
#[derive(Default)]
pub struct ToolListCapture {
    expected: Option<String>,
    rows: Option<Vec<Value>>,
    bytes: usize,
    cursor_bytes: usize,
    seen: HashSet<String>,
}
const MAX_CURSOR_BYTES: usize = 65_536;
const MAX_CURSOR_ENTRIES: usize = 1024;
impl ToolListCapture {
    pub fn invalidate(&mut self) {
        self.rows = None;
        self.expected = None;
        self.seen.clear();
        self.bytes = 0;
        self.cursor_bytes = 0;
    }
    pub fn add(
        &mut self,
        cursor: Option<&str>,
        next: Option<&str>,
        tools: &[Value],
    ) -> Option<Vec<Value>> {
        if cursor.is_none() {
            self.invalidate();
            self.rows = Some(Vec::new());
        } else if self.rows.is_none()
            || cursor != self.expected.as_deref()
            || self.seen.contains(cursor?)
        {
            self.invalidate();
            return None;
        }
        let next = next.filter(|s| !s.is_empty());
        if next.is_some_and(|s| self.seen.contains(s) || Some(s) == cursor) {
            self.invalidate();
            return None;
        }
        // A consumed cursor moves from expected to seen, so it is counted once.
        // Check the next allocation and collection overhead before retaining it.
        let cursor_bytes = self.cursor_bytes.saturating_add(next.map_or(0, str::len));
        let cursor_entries =
            self.seen.len() + usize::from(cursor.is_some()) + usize::from(next.is_some());
        if cursor_bytes > MAX_CURSOR_BYTES || cursor_entries > MAX_CURSOR_ENTRIES {
            self.invalidate();
            return None;
        }
        let Ok(bytes) = python_json::default_len(&Value::Array(tools.to_vec())) else {
            self.invalidate();
            return None;
        };
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > MAX_BYTES || self.rows.as_ref()?.len().saturating_add(tools.len()) > 10000 {
            self.invalidate();
            return None;
        }
        self.rows.as_mut()?.extend_from_slice(tools);
        if let Some(consumed) = self.expected.take() {
            self.seen.insert(consumed);
        }
        self.cursor_bytes = cursor_bytes;
        self.expected = next.map(str::to_owned);
        if next.is_some() {
            None
        } else {
            let rows = self.rows.take();
            self.invalidate();
            rows
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Identity {
    mtime: i64,
    nanos: i64,
    size: u64,
    inode: u64,
}
struct FileEntry {
    identity: Identity,
    digest: Option<String>,
}
#[derive(Default)]
pub struct SkillMetadataCache {
    files: HashMap<PathBuf, FileEntry>,
    file_order: VecDeque<PathBuf>,
    counts: HashMap<String, (Option<u64>, f64)>,
    count_order: VecDeque<String>,
}
impl SkillMetadataCache {
    pub fn cache_lengths(&self) -> (usize, usize) {
        (self.files.len(), self.counts.len())
    }
    /// `at` is monotonic seconds, supplied by the caller, for failed-count retries.
    pub async fn measure<F, Fut>(
        &mut self,
        rows: &[Value],
        home: &Path,
        at: f64,
        counter: &F,
    ) -> Value
    where
        F: Fn(Vec<String>) -> Fut,
        Fut: Future<Output = Vec<Option<u64>>>,
    {
        let mut result = serde_json::Map::new();
        let mut pending = Vec::new();
        let mut texts = Vec::new();
        let mut chars = 0usize;
        for row in rows {
            let Some(id) = row["id"].as_str() else {
                continue;
            };
            let mut item =
                json!({"origin":skill_origin(row,home),"size":unknown_size("skill-file")});
            let loaded = (|| {
                if comandos_core::json::truthy(&row["group"]) {
                    return None;
                }
                let path = Path::new(row["path"].as_str()?).canonicalize().ok()?;
                let stat = fs::metadata(&path).ok()?;
                let identity = Identity {
                    mtime: stat.mtime(),
                    nanos: stat.mtime_nsec(),
                    size: stat.len(),
                    inode: stat.ino(),
                };
                if let Some(entry) = self.files.get(&path).filter(|e| e.identity == identity) {
                    match &entry.digest {
                        None => return None,
                        Some(digest) => {
                            if let Some((count, when)) = self
                                .counts
                                .get(digest)
                                .filter(|(count, when)| count.is_some() || at - when < 30.0)
                            {
                                let _ = when;
                                return Some((digest.clone(), *count, None));
                            }
                        }
                    }
                }
                let text = read_bounded(&path)
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .map(|text| text.replace("\r\n", "\n").replace('\r', "\n"));
                let digest = text
                    .as_ref()
                    .map(|t| format!("{:x}", Sha256::digest(t.as_bytes())));
                if !self.files.contains_key(&path) {
                    while self.files.len() >= MAX_CACHE {
                        if let Some(old) = self.file_order.pop_front() {
                            self.files.remove(&old);
                        }
                    }
                    self.file_order.push_back(path.clone());
                }
                self.files.insert(
                    path,
                    FileEntry {
                        identity,
                        digest: digest.clone(),
                    },
                );
                let digest = digest?;
                if let Some((count, _)) = self
                    .counts
                    .get(&digest)
                    .filter(|(count, when)| count.is_some() || at - when < 30.0)
                {
                    Some((digest, *count, None))
                } else {
                    Some((digest, None, text))
                }
            })();
            if let Some((digest, count, text)) = loaded {
                item["size"]["tokens"] = json!(count);
                if let Some(text) = text {
                    let len = text.chars().count();
                    if chars.saturating_add(len) <= 8_000_000 {
                        chars += len;
                        pending.push((id.to_owned(), digest));
                        texts.push(text);
                    }
                }
            }
            result.insert(id.to_owned(), item);
        }
        if !texts.is_empty() {
            let counts = counter(texts).await;
            if counts.len() == pending.len() {
                for ((id, digest), count) in pending.into_iter().zip(counts) {
                    result.get_mut(&id).unwrap()["size"]["tokens"] = json!(count);
                    if !self.counts.contains_key(&digest) {
                        while self.counts.len() >= MAX_CACHE {
                            if let Some(old) = self.count_order.pop_front() {
                                self.counts.remove(&old);
                            }
                        }
                        self.count_order.push_back(digest.clone());
                    }
                    self.counts.insert(digest, (count, at));
                }
            }
        }
        Value::Object(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_collision_does_not_delete_existing_temporary_file() {
        let dir = std::env::temp_dir().join(format!("metadata-atomic-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        TEMP.store(0, Ordering::Relaxed);
        let temp = dir.join(format!(".size-{}-0", std::process::id()));
        fs::write(&temp, "keep").unwrap();
        assert!(write_atomic(&dir.join("size.json"), &json!({})).is_err());
        assert_eq!(fs::read_to_string(temp).unwrap(), "keep");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn all_unicode_scalars_match_python_311_provenance_words() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/python_word_ranges.json"))
                .unwrap();
        assert_eq!(fixture["unicode"], "14.0.0");
        let ranges: Vec<(u32, u32)> = fixture["ranges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r[0].as_u64().unwrap() as u32, r[1].as_u64().unwrap() as u32))
            .collect();
        let mut index = 0;
        let mut source = String::with_capacity(6);
        for scalar in 0..0x110000 {
            let Some(ch) = char::from_u32(scalar) else {
                continue;
            };
            while index < ranges.len() && ranges[index].1 < scalar {
                index += 1;
            }
            let word = index < ranges.len() && ranges[index].0 <= scalar;
            source.clear();
            source.push_str("o/");
            source.push(ch);
            assert_eq!(
                valid_github_source(&source),
                word || ch == '.' || ch == '-',
                "U+{scalar:04X}"
            );
        }
    }
}
