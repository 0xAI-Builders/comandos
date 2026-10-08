//! Lectura de los Resúmenes (plan 2f-4, Tarea 1): ramas GET de `_do_GET`
//! (8398–8415) con `news_editions_payload` (794), `news_edition_payload`
//! (812), `news_get` (870) y `news_media` (894) de `bin/cc-dash`.
//!
//! Todo es lectura: las consultas de app-state van en un solo trabajo del
//! worker de la base (`comandos_store::news`, mismas sentencias que el
//! Python); los archivos (`news-watch.json`, imágenes) en el pool de bloqueo.
//! Una excepción del Python que se sabe con certeza es `500`; lo dudoso
//! declina antes de responder.
//!
//! Imágenes (`/news/media/<nombre>`): el nombre tiene que casar
//! `^[0-9a-f]{32}\.(png|jpg|webp|gif|avif)$` (sin `/` ni `..`), y además la
//! ruta real (`canonicalize`) tiene que quedar bajo la ruta real de
//! `media_dir()`: un enlace simbólico que salga de la carpeta es «Imagen no
//! encontrada». Un archivo de hasta 1 MiB se lee de una vez; uno mayor se
//! envía por trozos de 64 KiB con `Content-Length` fijo, sin cargarlo entero.
//! `Range` se ignora y `HEAD` no llega aquí (va a `send_head`), como en el
//! Python.
use super::NewsRoute;
use crate::{
    HandlerError, Reply, ReplyBody, Request,
    dash::native::{
        Answer, Fault, Native, NativeOptions,
        files::Strict,
        light::{error, read_reply},
        py::{int as py_int, str_scalar, take_chars},
    },
    dash::router::path_of,
};
use bytes::Bytes;
use comandos_core::{
    dashboard_access::{query_pairs, request_target_parts},
    json::truthy,
    text::NumError,
};
use comandos_store::news;
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{self, Read},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::mpsc;

/// Hasta aquí una imagen se lee entera (como el `fh.read()` del Python).
const SMALL_MEDIA: u64 = 1 << 20;
/// Trozo de lectura de las imágenes grandes.
const MEDIA_CHUNK: u64 = 64 * 1024;
/// Trozos en vuelo hacia el transporte.
const MEDIA_FRAMES: usize = 4;

pub async fn answer(native: &Arc<Native>, route: NewsRoute, request: &Request) -> Answer {
    match route {
        NewsRoute::Latest => latest(native).await,
        NewsRoute::Editions => editions(native).await,
        NewsRoute::Edition => edition(native, request).await,
        NewsRoute::Media => media(native, request).await,
        NewsRoute::Source | NewsRoute::Chat | NewsRoute::Notes | NewsRoute::Saved => {
            news_get(native, route, request).await
        }
        // Las escrituras van por `write` y `agents`.
        NewsRoute::SavedPost
        | NewsRoute::NotesPost
        | NewsRoute::ChatNotePost
        | NewsRoute::ChatPost
        | NewsRoute::TranslatePost => Err(Fault::Decline),
    }
}

// ---------------------------------------------------------------- apoyo

/// Lo que devuelve un trabajo de la base: `(estado, cuerpo)`.
type Payload = Result<(StatusCode, Value), news::Fault>;

/// Un trabajo de lectura en el worker de la base y su respuesta.
async fn on_state<F>(native: &Arc<Native>, job: F) -> Answer
where
    F: FnOnce(&rusqlite::Connection) -> Payload + Send + 'static,
{
    let done = native.with_state(move |backend| job(&backend.conn)).await?;
    respond(done)
}

fn respond(done: Payload) -> Answer {
    match done {
        Ok((StatusCode::OK, value)) => read_reply(&value),
        Ok((status, value)) => Reply::json(status, &value).map_err(|_| Fault::Decline),
        // El Python lanzaría sin capturar: el 500 genérico del tablero.
        Err(news::Fault::Raise(_)) => Err(Fault::Error(HandlerError::Failure)),
        Err(fault @ (news::Fault::Unsure(_) | news::Fault::Sql(_))) => {
            trace_decline(&fault);
            Err(Fault::Decline)
        }
    }
}

/// Qué clases de declinación ya se anotaron en stderr (una vez por clase en
/// la vida del proceso, no una por petición).
pub(super) struct TraceOnce([AtomicBool; 3]);

impl TraceOnce {
    pub(super) const fn new() -> TraceOnce {
        TraceOnce([
            AtomicBool::new(false),
            AtomicBool::new(false),
            AtomicBool::new(false),
        ])
    }

    /// `true` solo la primera vez que llega esta clase (`0` duda, `1` SQLite,
    /// `2` cadena de agentes dudosa).
    pub(super) fn first(&self, kind: usize) -> bool {
        self.0
            .get(kind)
            .is_some_and(|flag| !flag.swap(true, Ordering::AcqRel))
    }
}

pub(super) static TRACED: TraceOnce = TraceOnce::new();

/// Una línea en stderr la primera vez que se reenvía una lectura por duda o
/// por un fallo de SQLite: un worker roto no pasa en silencio, y una petición
/// repetida no llena el registro. Solo el tipo de fallo (el texto de SQLite o
/// la duda), nunca datos de la petición.
pub(super) fn trace_decline(fault: &news::Fault) {
    let kind = usize::from(matches!(fault, news::Fault::Sql(_)));
    if TRACED.first(kind) {
        eprintln!("comandos dash news: se reenvía al heredado ({fault}); no se repite");
    }
}

fn not_found(message: &str) -> Payload {
    Ok((StatusCode::NOT_FOUND, json!({"error": message})))
}

/// `urllib.parse.parse_qs(urlsplit(self.path).query)`: pares sin vacíos.
/// Declina si el `urlsplit` portado ve otra ruta o si la decodificación
/// produjo U+FFFD (como `query::Query::parse`).
fn query(target: &str) -> Result<Vec<(String, String)>, Fault> {
    let (path, raw) = request_target_parts(target).ok_or(Fault::Decline)?;
    if path != path_of(target) {
        return Err(Fault::Decline);
    }
    let pairs = query_pairs(&raw, false);
    if pairs
        .iter()
        .any(|(k, v)| k.contains('\u{fffd}') || v.contains('\u{fffd}'))
    {
        return Err(Fault::Decline);
    }
    Ok(pairs)
}

/// `(query.get(nombre) or [""])[0]`.
fn first<'a>(pairs: &'a [(String, String)], name: &str) -> &'a str {
    pairs
        .iter()
        .find(|(k, _)| k == name)
        .map_or("", |(_, v)| v.as_str())
}

/// `_news_int(value)`: `int(str(value))` entre 1 y 2**53 − 1, si no `None`.
/// Un texto no ASCII (dígitos o blancos Unicode que `int()` acepta) declina.
pub fn news_int(text: &str) -> Result<Option<i64>, Fault> {
    match py_int(text) {
        Ok(n) => Ok((0 < n && n < (1 << 53)).then_some(n)),
        Err(NumError::Invalid) => Ok(None),
        // ASCII válido que no cabe en `i64`: fuera de rango igual.
        Err(NumError::Exotic) if text.is_ascii() => Ok(None),
        Err(NumError::Exotic) => Err(Fault::Decline),
    }
}

// ---------------------------------------------------------------- GET /news/latest

/// `_read_json_quiet(HOOKS/news-watch.json) or {}`.
async fn latest(native: &Arc<Native>) -> Answer {
    let opts = native.options().clone();
    let read = tokio::task::spawn_blocking(move || {
        super::super::files::DomainDocument::new(&opts.home, &opts.hooks, "news-watch.json")
            .map_or(Strict::Unsure, |d| d.strict())
    })
    .await
    .map_err(|_| Fault::Decline)?;
    let value = match read {
        Strict::Missing | Strict::Unreadable => json!({}),
        Strict::Unsure => return Err(Fault::Decline),
        Strict::Value(v) if truthy(&v) => v,
        Strict::Value(_) => json!({}),
    };
    read_reply(&value)
}

// ---------------------------------------------------------------- GET /news/editions

/// `env.get(clave)` verdadero en el entorno que vería el Python: el de los
/// hijos si está fijado (pruebas confinadas), si no el del proceso.
fn env_check(env: Option<Vec<(OsString, OsString)>>) -> impl Fn(&str) -> bool {
    move |key: &str| match &env {
        Some(vars) => vars
            .iter()
            .any(|(k, v)| k.as_os_str() == key && !v.is_empty()),
        None => std::env::var_os(key).is_some_and(|v| !v.is_empty()),
    }
}

/// `f"{st.get('agent') or st.get('kind')}:{st.get('model') or 'predeterminado'}"`
/// por paso de `summarizer` (`steps` si es cadena, si no el propio objeto).
fn chain(config: Option<&Map<String, Value>>) -> Result<Value, news::Fault> {
    let empty = Map::new();
    let summarizer = config
        .and_then(|c| c.get("summarizer"))
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let steps: Vec<&Value> = if summarizer.get("kind").and_then(Value::as_str) == Some("chain") {
        match summarizer.get("steps") {
            Some(Value::Array(items)) => items.iter().collect(),
            // Un `dict` o un `str` se recorren (claves, letras): nada es `dict`.
            Some(Value::Object(_) | Value::String(_)) => Vec::new(),
            // `for st in None/5/True` → `TypeError`.
            _ => return Err(news::Fault::Raise("TypeError".into())),
        }
    } else if summarizer.is_empty() {
        Vec::new()
    } else {
        // `[summarizer]`: un solo paso, el objeto mismo.
        return Ok(json!([step_label(summarizer)?]));
    };
    let mut out = Vec::new();
    for step in steps {
        if let Some(step) = step.as_object() {
            out.push(Value::String(step_label(step)?));
        }
    }
    Ok(Value::Array(out))
}

fn step_label(step: &Map<String, Value>) -> Result<String, news::Fault> {
    let pick = |key: &str| step.get(key).filter(|v| truthy(v));
    let who = pick("agent")
        .or_else(|| step.get("kind"))
        .unwrap_or(&Value::Null);
    let model = pick("model")
        .cloned()
        .unwrap_or_else(|| json!("predeterminado"));
    // `str()` de un `float`, lista u objeto: no se reproduce con certeza.
    let text = |v: &Value| str_scalar(v).ok_or_else(|| news::Fault::Unsure("str()".into()));
    Ok(format!("{}:{}", text(who)?, text(&model)?))
}

/// Tope de `news-editions.json` (1 MiB): más grande es «sin configurar» (el
/// Python lo leería entero).
const MAX_CONFIG: u64 = 1024 * 1024;

/// `load_config(HOOKS/news-editions.json)` sin bloquear: se abre con
/// `O_NONBLOCK` y solo un archivo regular de hasta 1 MiB se lee. Un FIFO, un
/// dispositivo, un directorio o un archivo mayor cuentan como `OSError`
/// (`None`); el Python se quedaría colgado en el FIFO (desviación aceptada:
/// el frente no se cuelga).
pub(super) fn read_config(path: &Path) -> news::Result<Option<Map<String, Value>>> {
    let Ok(file) = OpenOptions::new()
        .read(true)
        .custom_flags(nix::fcntl::OFlag::O_NONBLOCK.bits())
        .open(path)
    else {
        return Ok(None);
    };
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= MAX_CONFIG)
    {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    // Crece mientras se lee: tampoco se pasa del tope.
    if (&file)
        .take(MAX_CONFIG + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_CONFIG
    {
        return Ok(None);
    }
    news::config_from_bytes(&bytes)
}

pub(super) fn read_config_domain(opts: &NativeOptions) -> news::Result<Option<Map<String, Value>>> {
    comandos_store::domains::caller::read(&opts.home, "news-docs", |mode, db| {
        if matches!(
            mode,
            comandos_store::unified::Mode::Unified | comandos_store::unified::Mode::Sealed
        ) {
            let Some(db) = db else {
                return Ok(Ok(None));
            };
            let bytes =
                comandos_store::unified::doc_get(db, "hooks/news-editions.json")?.map(|r| r.body);
            Ok(match bytes {
                Some(bytes) if bytes.len() as u64 <= MAX_CONFIG => news::config_from_bytes(&bytes),
                _ => Ok(None),
            })
        } else {
            Ok(read_config(&opts.hooks.join("news-editions.json")))
        }
    })
    .map_err(|_| news::Fault::Unsure("domain read".into()))?
}

async fn editions(native: &Arc<Native>) -> Answer {
    let opts: &NativeOptions = native.options();
    let read_opts = opts.clone();
    let env = env_check(opts.child_env.clone());
    let now_ms = (opts.clock)();
    // La configuración se lee antes, en el pool de bloqueo: nunca en el
    // worker de la base (un FIFO lo dejaría parado para todas las rutas).
    let config = tokio::task::spawn_blocking(move || read_config_domain(&read_opts))
        .await
        .map_err(|_| Fault::Decline)?;
    let config = match config {
        Ok(config) => config,
        Err(fault) => return respond(Err(fault)),
    };
    on_state(native, move |conn| {
        let mut out = match news::config_status(config.as_ref(), &env) {
            Value::Object(status) => status,
            _ => Map::new(),
        };
        let policy = news::policy_from_config(config.as_ref())?;
        let latest = news::latest_readable(conn)?;
        let editions = news::list_editions(conn, 30, Some(now_ms))?;
        let chain = chain(config.as_ref())?;
        out.insert(
            "latest".into(),
            latest
                .and_then(|l| l.get("id").cloned())
                .unwrap_or(Value::Null),
        );
        out.insert("editions".into(), Value::Array(editions));
        out.insert(
            "next".into(),
            news::next_scheduled(conn, now_ms)?.unwrap_or(Value::Null),
        );
        out.insert("chain".into(), chain);
        let mut shown = Map::new();
        for key in ["timezone", "slots", "maxSources", "budgetUsd"] {
            shown.insert(key.into(), policy.get(key).cloned().unwrap_or(Value::Null));
        }
        out.insert("policy".into(), Value::Object(shown));
        Ok((StatusCode::OK, Value::Object(out)))
    })
    .await
}

// ---------------------------------------------------------------- GET /news/edition

/// `_NEWS_EDITION_ID_RE.match(id)`: `^\d{4}-\d{2}-\d{2}@\d{2}:\d{2}$` con el
/// `$` del Python (también antes de un `\n` final). `\d` admite dígitos
/// Unicode: un dígito no ASCII declina.
fn edition_id_matches(id: &str) -> Result<bool, news::Fault> {
    if id.chars().any(|c| !c.is_ascii() && c.is_numeric()) {
        return Err(news::Fault::Unsure("dígito Unicode en la edición".into()));
    }
    let b = id.strip_suffix('\n').unwrap_or(id).as_bytes();
    let digit = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    let at = |i: usize, c: u8| b.get(i) == Some(&c);
    Ok(b.len() == 16
        && (0..4).all(digit)
        && at(4, b'-')
        && (5..7).all(digit)
        && at(7, b'-')
        && (8..10).all(digit)
        && at(10, b'@')
        && (11..13).all(digit)
        && at(13, b':')
        && (14..16).all(digit))
}

async fn edition(native: &Arc<Native>, request: &Request) -> Answer {
    let pairs = query(&request.target)?;
    let ids: Vec<String> = pairs
        .into_iter()
        .filter(|(k, _)| k == "id")
        .map(|(_, v)| v)
        .collect();
    let [id] = ids.as_slice() else {
        return error(StatusCode::BAD_REQUEST, "Falta la edición");
    };
    let id = id.clone();
    on_state(native, move |conn| edition_payload(conn, id)).await
}

/// `news_edition_payload(edition_id)`.
fn edition_payload(conn: &rusqlite::Connection, edition_id: String) -> Payload {
    let edition_id = if edition_id == "latest" {
        let Some(latest) = news::latest_readable(conn)? else {
            return not_found("Aún no hay ediciones publicadas.");
        };
        match latest.get("id") {
            Some(Value::String(id)) => id.clone(),
            // `re.match` sobre algo que no es `str`: incierto.
            _ => return Err(news::Fault::Unsure("id de edición".into())),
        }
    } else {
        edition_id
    };
    if !edition_id_matches(&edition_id)? {
        return Ok((
            StatusCode::BAD_REQUEST,
            json!({"error": "Edición inválida"}),
        ));
    }
    let Some(mut found) = news::get_edition(conn, &edition_id)? else {
        return not_found("Edición no encontrada");
    };
    let ids: Vec<i64> = found
        .get("stories")
        .and_then(Value::as_array)
        .map(|stories| {
            stories
                .iter()
                .filter_map(|st| st.get("id").and_then(Value::as_i64))
                .collect()
        })
        .unwrap_or_default();
    let counts = news::story_counts(conn, &ids)?;
    if let Some(stories) = found.get_mut("stories").and_then(Value::as_array_mut) {
        for story in stories {
            let count = story
                .get("id")
                .and_then(Value::as_i64)
                .and_then(|id| counts.get(&id).cloned())
                .unwrap_or_else(|| json!({"notes": 0, "chat": 0, "saved": false}));
            if let Some(obj) = story.as_object_mut() {
                obj.insert("counts".into(), count);
            }
        }
    }
    Ok((StatusCode::OK, found))
}

// ---------------------------------------------------------------- GET /news/{source,chat,notes,saved}

async fn news_get(native: &Arc<Native>, route: NewsRoute, request: &Request) -> Answer {
    let pairs = query(&request.target)?;
    match route {
        NewsRoute::Source => {
            let sid = news_int(first(&pairs, "id"))?;
            on_state(native, move |conn| {
                let src = match sid {
                    Some(sid) => news::get_source(conn, sid)?.map(|s| (sid, s)),
                    None => None,
                };
                let Some((sid, mut src)) = src else {
                    return not_found("Fuente no encontrada");
                };
                let translation = news::translation(conn, sid, "es")?.unwrap_or(Value::Null);
                if let Some(obj) = src.as_object_mut() {
                    obj.insert("translation".into(), translation);
                }
                Ok((StatusCode::OK, src))
            })
            .await
        }
        NewsRoute::Chat => {
            let story = news_int(first(&pairs, "story"))?;
            on_state(native, move |conn| {
                let Some(story) = story else {
                    return not_found("Noticia no encontrada");
                };
                if news::story_row(conn, story)?.is_none() {
                    return not_found("Noticia no encontrada");
                }
                let messages = news::chat_history(conn, story)?;
                Ok((StatusCode::OK, json!({"messages": messages})))
            })
            .await
        }
        NewsRoute::Notes => {
            let story = news_int(first(&pairs, "story"))?;
            // `[:200]` corta por puntos de código.
            let q = take_chars(first(&pairs, "q"), 200);
            on_state(native, move |conn| {
                Ok((StatusCode::OK, news::list_notes(conn, story, &q, 500)?))
            })
            .await
        }
        _ => {
            on_state(native, |conn| {
                let stories = news::saved_stories(conn)?;
                Ok((StatusCode::OK, json!({"stories": stories})))
            })
            .await
        }
    }
}

// ---------------------------------------------------------------- GET /news/media/<nombre>

/// `_NEWS_MEDIA_TYPES[ext]` si `name` casa `_NEWS_MEDIA_RE`.
fn media_type(name: &str) -> Option<&'static str> {
    let (stem, ext) = name.split_once('.')?;
    if stem.len() != 32 || !stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    match ext {
        "png" => Some("image/png"),
        "jpg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}

/// `news_editions.media_dir()`: `$XDG_STATE_HOME` (o `~/.local/state`) +
/// `comandos/news-media`, con la instantánea del entorno de las opciones.
pub fn media_dir(opts: &NativeOptions) -> PathBuf {
    opts.xdg_state_home
        .clone()
        .unwrap_or_else(|| opts.home.join(".local/state"))
        .join("comandos/news-media")
}

enum Media {
    /// Cualquier fallo: «Imagen no encontrada».
    Missing,
    Small(Vec<u8>),
    Large(File, u64),
}

/// Abre la imagen confinada a `dir` (ruta real bajo la ruta real de la
/// carpeta), sin bloquear en un FIFO, y la lee si es pequeña.
fn open_media(dir: &Path, name: &str) -> Media {
    let (Ok(root), Ok(real)) = (
        std::fs::canonicalize(dir),
        std::fs::canonicalize(dir.join(name)),
    ) else {
        return Media::Missing;
    };
    if real == root || !real.starts_with(&root) {
        return Media::Missing;
    }
    let Ok(file) = open_confined(&real) else {
        return Media::Missing;
    };
    let Ok(meta) = file.metadata() else {
        return Media::Missing;
    };
    if !meta.is_file() || meta.len() == 0 {
        return Media::Missing;
    }
    if meta.len() > SMALL_MEDIA {
        return Media::Large(file, meta.len());
    }
    let mut body = Vec::new();
    match (&file).take(SMALL_MEDIA).read_to_end(&mut body) {
        // `if body`: un archivo vacío también es 404.
        Ok(_) if !body.is_empty() => Media::Small(body),
        _ => Media::Missing,
    }
}

/// Abre la ruta ya resuelta sin bloquear en un FIFO y sin seguir un enlace
/// final: si entre el `canonicalize` y el `open` alguien cambió el archivo
/// por un enlace simbólico, la apertura falla (`ELOOP`) en vez de salir de la
/// carpeta.
fn open_confined(real: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags((nix::fcntl::OFlag::O_NONBLOCK | nix::fcntl::OFlag::O_NOFOLLOW).bits())
        .open(real)
}

async fn media(native: &Arc<Native>, request: &Request) -> Answer {
    let path = path_of(&request.target);
    let name = path.strip_prefix("/news/media/").unwrap_or_default();
    let Some(kind) = media_type(name) else {
        return error(StatusCode::NOT_FOUND, "Imagen no encontrada");
    };
    let dir = media_dir(native.options());
    let owned = name.to_owned();
    let opened = tokio::task::spawn_blocking(move || open_media(&dir, &owned))
        .await
        .map_err(|_| Fault::Decline)?;
    match opened {
        Media::Missing => error(StatusCode::NOT_FOUND, "Imagen no encontrada"),
        Media::Small(body) => Ok(Reply::bytes(StatusCode::OK, kind, body)),
        Media::Large(file, length) => {
            let (tx, receiver) = mpsc::channel(MEDIA_FRAMES);
            native
                .tasks()
                .spawn(pump_media(file, length, tx))
                .map_err(|_| Fault::Decline)?;
            let mut reply = Reply::bytes(StatusCode::OK, kind, Bytes::new());
            reply.body = ReplyBody::SizedStream { length, receiver };
            Ok(reply)
        }
    }
}

/// Envía `length` bytes del archivo por trozos; cada lectura es un salto
/// corto al pool de bloqueo (ninguno espera al cliente). Si el archivo se
/// acorta, el transporte corta la conexión por la longitud incumplida.
async fn pump_media(file: File, length: u64, tx: mpsc::Sender<io::Result<Bytes>>) {
    let mut file = Some(file);
    let mut left = length;
    while left > 0 {
        let Some(current) = file.take() else {
            return;
        };
        let want = left.min(MEDIA_CHUNK);
        let read = tokio::task::spawn_blocking(move || {
            let mut chunk = Vec::new();
            let got = (&current).take(want).read_to_end(&mut chunk);
            (current, got.map(|_| chunk))
        })
        .await;
        match read {
            Ok((current, Ok(chunk))) if !chunk.is_empty() => {
                left = left.saturating_sub(chunk.len() as u64);
                file = Some(current);
                if tx.send(Ok(Bytes::from(chunk))).await.is_err() {
                    return;
                }
            }
            _ => {
                let _ = tx.send(Err(io::Error::other("imagen acortada"))).await;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_names_follow_the_python_regex() {
        let hex = "0123456789abcdef0123456789abcdef";
        assert_eq!(media_type(&format!("{hex}.png")), Some("image/png"));
        assert_eq!(media_type(&format!("{hex}.jpg")), Some("image/jpeg"));
        assert_eq!(media_type(&format!("{hex}.avif")), Some("image/avif"));
        assert_eq!(media_type(&format!("{hex}.jpeg")), None);
        assert_eq!(media_type(&format!("{hex}.PNG")), None);
        assert_eq!(media_type(&format!("{}.png", hex.to_uppercase())), None);
        assert_eq!(media_type(&format!("../{hex}.png")), None);
        assert_eq!(media_type(&format!("{hex}.png/x")), None);
        assert_eq!(media_type(&format!("{hex}.png\n")), None);
        assert_eq!(media_type("nada.png"), None);
        assert_eq!(media_type(""), None);
    }

    /// Revisión de la Tarea 1: entre el `canonicalize` y el `open`, el último
    /// componente pudo cambiarse por un enlace; `O_NOFOLLOW` lo rechaza.
    #[test]
    fn media_open_refuses_a_final_symlink() {
        let dir = std::env::temp_dir().join(format!("laneB-nofollow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("real.png");
        std::fs::write(&real, b"x").unwrap();
        let link = dir.join("enlace.png");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(open_confined(&real).is_ok());
        let refused = open_confined(&link);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(refused.is_err(), "se siguió el enlace final");
    }

    #[test]
    fn edition_ids_follow_the_python_regex() {
        assert!(edition_id_matches("2026-10-03@09:00").unwrap());
        assert!(edition_id_matches("2026-10-03@09:00\n").unwrap());
        assert!(!edition_id_matches("2026-10-03@09:00\n\n").unwrap());
        assert!(!edition_id_matches("2026-10-3@09:00").unwrap());
        assert!(!edition_id_matches("x").unwrap());
        assert!(edition_id_matches("2026-10-0\u{663}@09:00").is_err());
    }

    #[test]
    fn trace_once_per_kind() {
        let traced = TraceOnce::new();
        assert!(traced.first(0));
        assert!(!traced.first(0));
        assert!(traced.first(1));
        assert!(!traced.first(1));
        assert!(traced.first(2));
        assert!(!traced.first(2));
        assert!(!traced.first(3));
    }

    #[test]
    fn news_int_matches_python_int() {
        assert_eq!(news_int("5").ok(), Some(Some(5)));
        assert_eq!(news_int(" +5 ").ok(), Some(Some(5)));
        assert_eq!(news_int("1_0").ok(), Some(Some(10)));
        assert_eq!(news_int("0").ok(), Some(None));
        assert_eq!(news_int("-3").ok(), Some(None));
        assert_eq!(
            news_int("9007199254740991").ok(),
            Some(Some(9007199254740991))
        );
        assert_eq!(news_int("9007199254740992").ok(), Some(None));
        assert_eq!(
            news_int("99999999999999999999999999999999999999999").ok(),
            Some(None)
        );
        assert_eq!(news_int("x").ok(), Some(None));
        assert_eq!(news_int("").ok(), Some(None));
        assert!(news_int("\u{663}").is_err());
    }
}
