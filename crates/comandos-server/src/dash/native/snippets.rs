//! F. Snippets (`bin/cc-dash`: `snippet_validate` 5196, `read_snippets` 5264,
//! GET 8740, POST 9287–9332). `H/snippets.json` se reescribe bajo el `flock`
//! de `<archivo>.lock` que comparte con el Python; si está tomado, se declina.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    files::{DomainDocument, FileLock, Strict},
    light::{data, error, read_reply},
    py, reply,
};
use crate::{HandlerError, Request};
use comandos_core::json::truthy;
use http::StatusCode;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetsRoute {
    List,
    Create,
    Update,
    Delete,
}

const fn entry(verb: Verb, path: &'static str, route: SnippetsRoute) -> Entry {
    Entry {
        verb,
        key: Key::Raw(path),
        route: NativeRoute::Snippets(route),
    }
}

pub const ROUTES: &[Entry] = &[
    entry(Verb::Get, "/snippets", SnippetsRoute::List),
    entry(Verb::Post, "/snippets", SnippetsRoute::Create),
    entry(Verb::Post, "/snippets/update", SnippetsRoute::Update),
    entry(Verb::Post, "/snippets/delete", SnippetsRoute::Delete),
];

pub async fn answer(native: &Native, route: SnippetsRoute, request: &Request) -> Answer {
    let path = DomainDocument::new(
        &native.options().home,
        &native.options().hooks,
        "snippets.json",
    )
    .map_err(|_| failure())?;
    let now = (native.options().clock)().div_euclid(1000);
    match route {
        SnippetsRoute::List => {
            let rows = tokio::task::spawn_blocking(move || read_snippets(&path))
                .await
                .map_err(|_| failure())??;
            read_reply(&Value::Array(rows))
        }
        SnippetsRoute::Create => create(path, data(request)?, now).await,
        SnippetsRoute::Update => update(path, data(request)?, now).await,
        SnippetsRoute::Delete => delete(path, data(request)?).await,
    }
}

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// `SNIPPET_ID_RE = ^[a-f0-9]{16}\Z`.
fn snippet_id(id: &str) -> bool {
    id.len() == 16
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `str(x.get(k, ""))`: escalares seguros; flotantes y contenedores declinan.
fn text_of(value: Option<&Value>) -> Result<String, Fault> {
    match value {
        None => Ok(String::new()),
        Some(v) => py::str_scalar(v).ok_or(Fault::Decline),
    }
}

/// `str(data.get("id", ""))` antes de `SNIPPET_ID_RE.match`: el `str()` de un
/// flotante, una lista o un objeto nunca casa (lleva `.`, `+`, `i`, `n`, `[` o
/// `{`), así que esos ids reciben el 400 `id invalido` del Python, sin declinar.
fn id_of(value: Option<&Value>) -> String {
    value.and_then(py::str_scalar).unwrap_or_default()
}

/// `snippet_validate` (5196), con `tags` tal cual lo recibe.
fn validate(name: &str, body: &str, tags: &Value) -> Option<&'static str> {
    if py::strip(name).is_empty() {
        return Some("name vacio");
    }
    if name.chars().count() > 80 {
        return Some("name demasiado largo (max 80)");
    }
    if py::strip(body).is_empty() {
        return Some("body vacio");
    }
    if body.chars().count() > 20000 {
        return Some("body demasiado largo (max 20000)");
    }
    let Value::Array(tags) = tags else {
        return Some("tags debe ser lista");
    };
    if tags.len() > 10 {
        return Some("demasiados tags (max 10)");
    }
    for tag in tags {
        match tag {
            Value::String(t) if !py::strip(t).is_empty() => {
                if t.chars().count() > 32 {
                    return Some("tag demasiado largo (max 32)");
                }
            }
            _ => return Some("tag vacio"),
        }
    }
    None
}

/// `data.get("tags") or []`.
fn tags_of(value: Option<&Value>) -> Value {
    match value {
        Some(v) if truthy(v) => v.clone(),
        _ => Value::Array(Vec::new()),
    }
}

/// `read_snippets` (5264).
fn read_snippets(path: &DomainDocument) -> Result<Vec<Value>, Fault> {
    snippets_from(path.strict())
}
fn snippets_from(read: Strict) -> Result<Vec<Value>, Fault> {
    let raw = match read {
        Strict::Value(Value::Array(raw)) => raw,
        Strict::Unsure => return Err(Fault::Decline),
        _ => return Ok(Vec::new()),
    };
    let mut out = Vec::new();
    for item in raw {
        let Value::Object(it) = item else { continue };
        // Un id que `str()` no puede casar salta la fila, sin declinar.
        let id = id_of(it.get("id"));
        if !snippet_id(&id) {
            continue;
        }
        let name = text_of(it.get("name"))?;
        let body = text_of(it.get("body"))?;
        let raw_tags = tags_of(it.get("tags"));
        // `tags if isinstance(tags, list) else []` solo para validar.
        let checked = if raw_tags.is_array() {
            raw_tags.clone()
        } else {
            Value::Array(Vec::new())
        };
        if validate(&name, &body, &checked).is_some() {
            continue;
        }
        // Una fila válida con `tags` verdadera que no es lista: el Python
        // iteraría la cadena o las claves (o lanzaría TypeError). Se declina.
        if !raw_tags.is_array() {
            return Err(Fault::Decline);
        }
        let tags = raw_tags;
        let updated = match it.get("updated_at") {
            Some(v) if truthy(v) => py::int_of(v).map_err(|c| match c {
                py::Conversion::Exotic => Fault::Decline,
                _ => failure(),
            })?,
            _ => 0,
        };
        let mut row = Map::new();
        row.insert("id".into(), Value::String(id));
        row.insert("name".into(), Value::String(name));
        row.insert("body".into(), Value::String(body));
        row.insert("tags".into(), tags);
        row.insert("updated_at".into(), json!(updated));
        out.push(Value::Object(row));
    }
    Ok(out)
}

/// `secrets.token_hex(8)`.
fn token_hex8() -> Result<String, Fault> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|_| failure())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `{"id","name":strip,"body","tags":[strip…],"updated_at"}`.
fn item(id: String, name: &str, body: String, tags: &Value, now: i64) -> Value {
    let tags: Vec<Value> = tags
        .as_array()
        .map(|t| {
            t.iter()
                .filter_map(Value::as_str)
                .map(|s| Value::String(py::strip(s).to_owned()))
                .collect()
        })
        .unwrap_or_default();
    let mut row = Map::new();
    row.insert("id".into(), Value::String(id));
    row.insert("name".into(), Value::String(py::strip(name).to_owned()));
    row.insert("body".into(), Value::String(body));
    row.insert("tags".into(), Value::Array(tags));
    row.insert("updated_at".into(), json!(now));
    Value::Object(row)
}

/// `with file_lock(SNIPPETS_FILE): items = read_snippets(); …; write_snippets(items)`.
/// Mirror/Unified wait for the existing file writer before reading; Legacy
/// retains its compatibility fallback when that lock is already owned.
async fn locked<T: Send + 'static>(
    path: DomainDocument,
    mutate: impl FnOnce(&mut Vec<Value>) -> (bool, T) + Send + 'static,
) -> Result<T, Fault> {
    let source = path.clone();
    let access = tokio::task::spawn_blocking(move || source.access())
        .await
        .map_err(|_| failure())?
        .map_err(|_| failure())?;
    let lock = match access.mode() {
        comandos_store::unified::Mode::Sealed => None,
        comandos_store::unified::Mode::Legacy => Some(
            FileLock::try_acquire(&path.file)
                .map_err(|_| failure())?
                .ok_or(Fault::Decline)?,
        ),
        _ => Some(
            FileLock::acquire_timeout(&path.file, std::time::Duration::from_secs(3))
                .await
                .map_err(|_| Fault::Decline)?,
        ),
    };
    tokio::task::spawn_blocking(move || {
        let _lock = lock;
        access
            .with_write_transaction(|| {
                let mut items = snippets_from(path.strict_under(&access))?;
                let (write, out) = mutate(&mut items);
                if write {
                    let bytes = comandos_core::json::response_dumps(&Value::Array(items))
                        .map_err(|_| failure())?;
                    path.write_under(&access, bytes.as_bytes(), 0)
                        .map_err(|_| failure())?;
                }
                Ok(out)
            })
            .map_err(|_| failure())?
    })
    .await
    .map_err(|_| failure())?
}

async fn create(path: DomainDocument, data: &Map<String, Value>, now: i64) -> Answer {
    let name = text_of(data.get("name"))?;
    let body = text_of(data.get("body"))?;
    let tags = tags_of(data.get("tags"));
    if let Some(message) = validate(&name, &body, &tags) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    let new = item(token_hex8()?, &name, body, &tags, now);
    let stored = new.clone();
    locked(path, move |items| {
        items.insert(0, stored);
        (true, ())
    })
    .await?;
    reply(StatusCode::OK, &json!({"item": new}))
}

async fn update(path: DomainDocument, data: &Map<String, Value>, now: i64) -> Answer {
    let id = id_of(data.get("id"));
    if !snippet_id(&id) {
        return error(StatusCode::BAD_REQUEST, "id invalido");
    }
    let name = text_of(data.get("name"))?;
    let body = text_of(data.get("body"))?;
    let tags = tags_of(data.get("tags"));
    if let Some(message) = validate(&name, &body, &tags) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    let updated = locked(path, move |items| {
        let mut updated = None;
        for it in items.iter_mut() {
            if it.get("id").and_then(Value::as_str) == Some(id.as_str()) {
                let new = item(id.clone(), &name, body.clone(), &tags, now);
                *it = new.clone();
                updated = Some(new);
            }
        }
        (updated.is_some(), updated)
    })
    .await?;
    match updated {
        None => error(StatusCode::NOT_FOUND, "snippet no encontrado"),
        Some(new) => reply(StatusCode::OK, &json!({"item": new})),
    }
}

async fn delete(path: DomainDocument, data: &Map<String, Value>) -> Answer {
    let id = id_of(data.get("id"));
    if !snippet_id(&id) {
        return error(StatusCode::BAD_REQUEST, "id invalido");
    }
    let removed = locked(path, move |items| {
        let before = items.len();
        items.retain(|it| it.get("id").and_then(Value::as_str) != Some(id.as_str()));
        let removed = items.len() != before;
        (removed, removed)
    })
    .await?;
    if removed {
        reply(StatusCode::OK, &json!({"ok": true}))
    } else {
        error(StatusCode::NOT_FOUND, "snippet no encontrado")
    }
}
