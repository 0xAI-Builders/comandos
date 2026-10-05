//! Lectura acotada de una conversación verificada por el llamador.
//! Solo devuelve contadores; nunca devuelve argumentos ni contenido de mensajes.
use comandos_core::json::{truthy, workspace_loads};
use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Map, Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};
type Signature = (u64, u64, i64, i64);
fn signature(path: &Path) -> Option<Signature> {
    let m = path.metadata().ok()?;
    Some((m.ino(), m.len(), m.mtime(), m.mtime_nsec()))
}
#[derive(PartialEq)]
struct CacheKey {
    harness: String,
    conversation: String,
    path: PathBuf,
    signature: Signature,
    wal: Option<Signature>,
    rows: Vec<(usize, String, String, String)>,
    limit: usize,
}
static CACHE: OnceLock<Mutex<VecDeque<(CacheKey, Value)>>> = OnceLock::new();
fn lines(mut raw: &[u8]) -> impl Iterator<Item = &[u8]> {
    std::iter::from_fn(move || {
        if raw.is_empty() {
            return None;
        }
        let end = raw
            .iter()
            .position(|b| *b == b'\r' || *b == b'\n')
            .unwrap_or(raw.len());
        let line = &raw[..end];
        let skip = if raw.get(end) == Some(&b'\r') && raw.get(end + 1) == Some(&b'\n') {
            2
        } else {
            1
        };
        raw = raw.get(end.saturating_add(skip)..).unwrap_or_default();
        Some(line)
    })
}
fn first<'a>(values: &[&'a Value]) -> &'a Value {
    values
        .iter()
        .copied()
        .find(|v| truthy(v))
        .unwrap_or(&Value::Null)
}
fn arguments(value: &Value) -> Value {
    if value.is_object() {
        return value.clone();
    }
    if let Some(s) = value.as_str().filter(|s| s.chars().count() < 262144)
        && let Ok(v) = workspace_loads(s)
        && v.is_object()
    {
        return v;
    }
    json!({})
}
fn ident(value: &Value, fallback: impl FnOnce() -> String) -> String {
    if truthy(value) {
        crate::acp_client::py_str(value)
    } else {
        fallback()
    }
}
struct Observation<'a> {
    harness: &'a str,
    rows: [&'a [Value]; 2],
    counts: HashMap<(usize, String), u64>,
    seen: HashSet<(usize, String, String)>,
    complete: bool,
    turns: u64,
}
impl Observation<'_> {
    fn call(&mut self, name: &Value, args: &Value, ident: String, server: &Value) {
        let name = name.as_str().unwrap_or("");
        if !truthy(server) && name.is_empty() {
            self.complete = false;
            return;
        }
        let args = arguments(args);
        let (kind, target) = if truthy(server) {
            (0, server.as_str().unwrap_or("").to_owned())
        } else if let Some(rest) = name.strip_prefix("mcp__") {
            let Some((server, _)) = rest.split_once("__") else {
                self.complete = false;
                return;
            };
            (0, server.into())
        } else if ["Skill", "skill"].contains(&name) {
            (
                1,
                first(&[&args["skill"], &args["name"]])
                    .as_str()
                    .unwrap_or("")
                    .into(),
            )
        } else if name == "call_mcp_tool" {
            (0, args["ServerName"].as_str().unwrap_or("").into())
        } else if ["view_file", "read_file"].contains(&name) {
            let filename = first(&[&args["AbsolutePath"], &args["path"], &args["file_path"]]);
            let matches: Vec<_> = self.rows[1]
                .iter()
                .filter(|r| truthy(filename) && *filename == r["path"])
                .collect();
            if matches.len() != 1 {
                return;
            }
            (1, matches[0]["name"].as_str().unwrap_or("").into())
        } else if self.harness == "opencode" {
            let matches: Vec<_> = self.rows[0]
                .iter()
                .filter_map(|r| r["name"].as_str())
                .filter(|s| {
                    let prefix: String = s
                        .chars()
                        .map(|c| {
                            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                                c
                            } else {
                                '_'
                            }
                        })
                        .collect();
                    name.starts_with(&(prefix + "_"))
                })
                .collect();
            if matches.len() > 1 {
                self.complete = false;
                return;
            }
            let Some(target) = matches.first() else {
                return;
            };
            (0, (*target).into())
        } else {
            return;
        };
        if target.is_empty()
            || self.rows[kind]
                .iter()
                .filter(|r| r["name"].as_str() == Some(&target))
                .count()
                > 1
        {
            self.complete = false;
            return;
        }
        if self.seen.insert((kind, target.clone(), ident)) {
            *self.counts.entry((kind, target)).or_default() += 1;
        }
    }
    // False means a Codex session_meta explicitly identified a different conversation.
    fn record(&mut self, r: &Value, index: usize, conversation: &str) -> Result<bool, ()> {
        if !r.is_object() {
            self.complete = false;
            return Ok(true);
        }
        if truthy(&r["sessionId"]) && r["sessionId"] != conversation {
            return Ok(true);
        }
        match self.harness {
            "claude" => {
                if r["type"] == "user" {
                    self.turns += 1;
                }
                if r["type"] != "assistant" {
                    return Ok(true);
                }
                let message = &r["message"];
                if truthy(message) && !message.is_object() {
                    return Err(());
                }
                let content = &message["content"];
                if truthy(content) && (content.is_number() || content.is_boolean()) {
                    return Err(());
                }
                for block in content.as_array().map(Vec::as_slice).unwrap_or(&[]) {
                    if block.is_object() && block["type"] == "tool_use" {
                        self.call(
                            &block["name"],
                            &block["input"],
                            ident(&block["id"], || format!("line-{index}")),
                            &Value::Null,
                        );
                    }
                }
            }
            "codex" => {
                let payload = &r["payload"];
                if truthy(payload) && !payload.is_object() {
                    return Err(());
                }
                if r["type"] == "session_meta" && payload["id"] != conversation {
                    return Ok(false);
                }
                if r["type"] == "response_item"
                    && ["function_call", "custom_tool_call"]
                        .contains(&payload["type"].as_str().unwrap_or(""))
                {
                    self.call(
                        &payload["name"],
                        &payload["arguments"],
                        ident(&payload["call_id"], || format!("line-{index}")),
                        &Value::Null,
                    );
                }
                if payload["type"] == "user_message" {
                    self.turns += 1;
                }
            }
            "agy" => {
                let calls = &r["tool_calls"];
                if truthy(calls) && (calls.is_number() || calls.is_boolean()) {
                    return Err(());
                }
                for (offset, item) in calls
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                    .iter()
                    .enumerate()
                {
                    if item.is_object() {
                        self.call(
                            &item["name"],
                            &item["args"],
                            ident(&item["id"], || {
                                format!(
                                    "{}-{offset}",
                                    r.get("step_index")
                                        .map(crate::acp_client::py_str)
                                        .unwrap_or_else(|| index.to_string())
                                )
                            }),
                            &Value::Null,
                        );
                    }
                }
                if r["source"] == "USER" {
                    self.turns += 1;
                }
            }
            "grok" => {
                let payload = first(&[&r["data"], &r["event"], r]);
                if !payload.is_object() {
                    return Ok(true);
                }
                let typ = first(&[&r["type"], &payload["type"]]);
                let id = ident(&payload["tool_call_id"], || format!("line-{index}"));
                if typ == "mcp_tool_call_completed" {
                    self.call(
                        &payload["tool_name"],
                        &Value::Null,
                        id,
                        first(&[&payload["server_name"], &payload["server"]]),
                    );
                } else if typ == "tool_call" || typ == "tool_call_completed" {
                    self.call(
                        first(&[&payload["name"], &payload["tool_name"]]),
                        &payload["arguments"],
                        id,
                        &Value::Null,
                    );
                }
                if typ == "user_message" {
                    self.turns += 1;
                }
            }
            _ => {}
        }
        Ok(true)
    }
    fn sqlite(
        &mut self,
        path: &Path,
        conversation: &str,
        limit: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(Duration::from_millis(500))?;
        let mut stmt =
            db.prepare("SELECT id,substr(data,1,?) FROM part WHERE session_id=? LIMIT 10001")?;
        let records = stmt.query_map(
            params![
                limit.saturating_add(1).min(i64::MAX as usize) as i64,
                conversation
            ],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )?;
        let mut consumed = 0usize;
        for (index, row) in records.enumerate() {
            let (id, raw) = row?;
            consumed = consumed.saturating_add(raw.len());
            if index >= 10000 || consumed > limit {
                self.complete = false;
                break;
            }
            self.turns = 1;
            let r = workspace_loads(&raw)?;
            if !r.is_object() {
                return Err("invalid part".into());
            }
            if r["type"] == "tool" {
                let state = &r["state"];
                if truthy(state) && !state.is_object() {
                    return Err("invalid tool state".into());
                }
                if !["running", "completed", "error"]
                    .contains(&state["status"].as_str().unwrap_or(""))
                {
                    self.complete = false;
                    continue;
                }
                self.call(
                    &r["tool"],
                    &state["input"],
                    ident(&r["callID"], || id),
                    &Value::Null,
                );
            }
        }
        Ok(())
    }
}

pub fn conversation_usage(
    harness: &str,
    conversation: &str,
    source: Option<&Path>,
    inventory: &Value,
    max_bytes: usize,
) -> Value {
    let rows = [
        inventory["mcps"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        inventory["skills"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    ];
    let counts: Map<String, Value> = ["mcps", "skills"]
        .into_iter()
        .zip(rows)
        .map(|(kind, rows)| {
            (
                kind.into(),
                Value::Object(
                    rows.iter()
                        .filter_map(|r| r["id"].as_str().map(|id| (id.into(), Value::Null)))
                        .collect(),
                ),
            )
        })
        .collect();
    let mut result = json!({"counts":counts,"complete":false,"turns":0,"source":"conversation","tokens":null,"note":"Sin historial completo; la ausencia de llamadas no demuestra falta de uso."});
    let Some(path) = source.filter(|p| !p.as_os_str().is_empty()) else {
        return result;
    };
    if conversation.is_empty() || !["claude", "codex", "grok", "opencode", "agy"].contains(&harness)
    {
        return result;
    }
    let Some(sig) = signature(path) else {
        return result;
    };
    let mut wal_path = path.as_os_str().to_owned();
    wal_path.push("-wal");
    let key = CacheKey {
        harness: harness.into(),
        conversation: conversation.into(),
        path: path.into(),
        signature: sig,
        wal: signature(Path::new(&wal_path)),
        rows: rows
            .iter()
            .enumerate()
            .flat_map(|(kind, rows)| {
                rows.iter().map(move |r| {
                    (
                        kind,
                        r["id"].as_str().unwrap_or("").into(),
                        r["name"].as_str().unwrap_or("").into(),
                        r["path"].as_str().unwrap_or("").into(),
                    )
                })
            })
            .collect(),
        limit: max_bytes,
    };
    let cache = CACHE.get_or_init(Mutex::default);
    {
        let mut cache = cache.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(index) = cache.iter().position(|(k, _)| *k == key)
            && let Some(entry) = cache.remove(index)
        {
            let value = entry.1.clone();
            cache.push_back(entry);
            return value;
        }
    }
    let mut obs = Observation {
        harness,
        rows,
        counts: HashMap::new(),
        seen: HashSet::new(),
        complete: ["claude", "opencode"].contains(&harness),
        turns: 0,
    };
    if harness == "opencode" {
        if obs.sqlite(path, conversation, max_bytes).is_err() {
            obs.complete = false;
        }
    } else {
        let read = || -> std::io::Result<Vec<u8>> {
            let mut f = File::open(path)?;
            let truncated = sig.1 > max_bytes as u64;
            if truncated {
                f.seek(SeekFrom::End(-(max_bytes.min(i64::MAX as usize) as i64)))?;
            }
            let mut raw = Vec::new();
            f.take(max_bytes.saturating_add(1) as u64)
                .read_to_end(&mut raw)?;
            if truncated {
                raw = raw
                    .iter()
                    .position(|b| *b == b'\n')
                    .map(|i| raw[i + 1..].to_vec())
                    .unwrap_or_default();
            }
            Ok(raw)
        };
        match read() {
            Err(_) => obs.complete = false,
            Ok(mut raw) => {
                if sig.1 > max_bytes as u64 || raw.len() > max_bytes {
                    obs.complete = false;
                }
                raw.truncate(max_bytes);
                for (index, line) in lines(&raw).enumerate() {
                    let r = std::str::from_utf8(line)
                        .ok()
                        .and_then(|s| workspace_loads(s).ok());
                    let Some(r) = r else {
                        obs.complete = false;
                        continue;
                    };
                    match obs.record(&r, index, conversation) {
                        Ok(false) => return result,
                        Err(()) => {
                            obs.complete = false;
                            break;
                        }
                        Ok(true) => {}
                    }
                }
            }
        }
    }
    obs.complete &= obs.turns > 0;
    for (kind, group) in rows.iter().enumerate() {
        for row in *group {
            if let Some(id) = row["id"].as_str() {
                result["counts"][["mcps", "skills"][kind]][id] = obs
                    .counts
                    .get(&(kind, row["name"].as_str().unwrap_or("").into()))
                    .copied()
                    .or(if obs.complete { Some(0) } else { None })
                    .map(Value::from)
                    .unwrap_or(Value::Null);
            }
        }
    }
    result["complete"] = obs.complete.into();
    result["turns"] = obs.turns.into();
    if obs.complete {
        result["note"] = "Llamadas registradas en esta conversación.".into();
    }
    let mut cache = cache.lock().unwrap_or_else(|p| p.into_inner());
    cache.retain(|(k, _)| *k != key);
    cache.push_back((key, result.clone()));
    while cache.len() > 32 {
        cache.pop_front();
    }
    result
}
