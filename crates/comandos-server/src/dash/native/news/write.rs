//! Escrituras sin agente de los Resúmenes (plan 2f-4, Tarea 2): las ramas
//! POST `/news/saved`, `/news/notes` y `/news/chat/note` de `news_post` (906)
//! de `bin/cc-dash`, sobre `set_saved`, `add_note`, `update_note`,
//! `delete_note` y `toggle_chat_note` de `news_reading` (portadas en
//! `comandos_store::news`).
//!
//! Cada petición corre entera en su propia tarea (si el cliente se va, la
//! escritura termina igual, como el hilo del Python) y su trabajo de SQLite va
//! en el worker de app-state. `LookupError` → 404 con `str(exc).strip("'")`,
//! `ValueError` → 400, `RuntimeError` → 409; lo dudoso declina solo antes de
//! abrir la transacción de escritura; lo que falla después es el 500 que daría
//! la excepción sin capturar del Python.
use super::{
    NewsRoute,
    read::{news_int, trace_decline},
};
use crate::{
    HandlerError, Request,
    dash::native::{
        Answer, Fault, Native,
        light::{data, error},
        reply,
    },
};
use comandos_core::json::truthy;
use comandos_store::news::{self, NewsError};
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::sync::Arc;

pub async fn answer(native: &Arc<Native>, route: NewsRoute, request: &Request) -> Answer {
    let d = data(request)?.clone();
    let task_native = Arc::clone(native);
    let job = tokio::spawn(async move { post(&task_native, route, &d).await });
    job.await.map_err(|_| failure())?
}

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// `_news_int(value)` sobre un valor del cuerpo: `int(str(value))` entre 1 y
/// 2**53 − 1. Un `float`, `bool`, `None`, lista u objeto nunca dan un `str`
/// que `int()` acepte.
fn news_int_value(value: Option<&Value>) -> Result<Option<i64>, Fault> {
    match value {
        Some(Value::String(s)) => news_int(s),
        // Con `arbitrary_precision`, `as_i64` solo acepta el texto de un entero.
        Some(Value::Number(n)) => Ok(n.as_i64().filter(|n| 0 < *n && *n < (1 << 53))),
        _ => Ok(None),
    }
}

/// Lo que se hará en el worker, ya validado fuera de él.
enum Job {
    Saved(Option<i64>, bool),
    Add(Option<i64>, Value),
    Update(Option<i64>, Value),
    Delete(Option<i64>),
    Toggle(Option<i64>),
}

/// `data.get(clave)` (o `None`).
fn field(d: &Map<String, Value>, key: &str) -> Value {
    d.get(key).cloned().unwrap_or(Value::Null)
}

async fn post(native: &Arc<Native>, route: NewsRoute, d: &Map<String, Value>) -> Answer {
    let job = match route {
        NewsRoute::SavedPost => Job::Saved(
            news_int_value(d.get("storyId"))?,
            d.get("saved").is_some_and(truthy),
        ),
        NewsRoute::NotesPost => match d.get("action").and_then(Value::as_str) {
            Some("add") => Job::Add(news_int_value(d.get("storyId"))?, field(d, "text")),
            Some("update") => Job::Update(news_int_value(d.get("noteId"))?, field(d, "text")),
            Some("delete") => Job::Delete(news_int_value(d.get("noteId"))?),
            _ => return error(StatusCode::BAD_REQUEST, "Acción de nota desconocida"),
        },
        NewsRoute::ChatNotePost => Job::Toggle(news_int_value(d.get("chatId"))?),
        // Las rutas GET no llegan aquí.
        _ => return Err(Fault::Decline),
    };
    let now = (native.options().clock)();
    let done = native
        .with_state(move |backend| run(&backend.conn, job, now))
        .await?;
    respond(done)
}

/// La rama de `news_post` en el worker: `(estado, cuerpo)`.
fn run(conn: &rusqlite::Connection, job: Job, now: i64) -> Result<(StatusCode, Value), NewsError> {
    let ok = |value: Value| Ok((StatusCode::OK, value));
    match job {
        // `set_saved(...) if story else None`; `None` → «Noticia no encontrada».
        Job::Saved(story, on) => match story.map(|s| news::set_saved(conn, s, on, now)) {
            Some(got) => match got? {
                Some(saved) => ok(json!({"saved": saved})),
                None => Ok((
                    StatusCode::NOT_FOUND,
                    json!({"error": "Noticia no encontrada"}),
                )),
            },
            None => Ok((
                StatusCode::NOT_FOUND,
                json!({"error": "Noticia no encontrada"}),
            )),
        },
        Job::Add(story, text) => ok(json!({"note": news::add_note(conn, story, &text, now)?})),
        Job::Update(note, text) => ok(json!({"note": news::update_note(conn, note, &text, now)?})),
        Job::Delete(note) => ok(json!({"deleted": news::delete_note(conn, note)?})),
        Job::Toggle(chat) => ok(news::toggle_chat_note(conn, chat, now)?),
    }
}

fn respond(done: Result<(StatusCode, Value), NewsError>) -> Answer {
    match done {
        Ok((status, value)) => reply(status, &value),
        // `str(exc).strip("'")`: el `str()` de un `KeyError` lleva comillas.
        Err(NewsError::Lookup(message)) => error(StatusCode::NOT_FOUND, message.trim_matches('\'')),
        Err(NewsError::Value(message)) => error(StatusCode::BAD_REQUEST, &message),
        Err(NewsError::Runtime(message)) => error(StatusCode::CONFLICT, &message),
        Err(NewsError::Fault(news::Fault::Raise(_))) => Err(failure()),
        Err(NewsError::Fault(fault)) => {
            trace_decline(&fault);
            Err(Fault::Decline)
        }
        Err(NewsError::AfterWrite(fault)) => {
            eprintln!("comandos dash news: fallo tras escribir ({fault}); 500");
            Err(failure())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_ids_follow_news_int() {
        let v = |text: &str| serde_json::from_str::<Value>(text).unwrap();
        let got = |text: &str| news_int_value(Some(&v(text))).ok();
        assert_eq!(got("10"), Some(Some(10)));
        assert_eq!(got("\" 10 \""), Some(Some(10)));
        assert_eq!(got("10.0"), Some(None));
        assert_eq!(got("1e1"), Some(None));
        assert_eq!(got("-0"), Some(None));
        assert_eq!(got("true"), Some(None));
        assert_eq!(got("null"), Some(None));
        assert_eq!(got("[10]"), Some(None));
        assert_eq!(got("9007199254740991"), Some(Some(9007199254740991)));
        assert_eq!(got("9007199254740992"), Some(None));
        assert_eq!(got("123456789012345678901234567890"), Some(None));
        assert_eq!(news_int_value(None).ok(), Some(None));
        assert!(news_int_value(Some(&v("\"\\u0663\""))).is_err());
    }
}
