//! Corte `services` (plan 2f-3, Tarea 5): Web Push inerte y POST `/pomodoro`
//! (`PushRoute::PomodoroPost`, P3 del preflight; la GET sigue en la base).
//!
//! - GET `/push/key`, POST y DELETE `/push/subscription`, POST `/push/test`
//!   (8391, 9051, 8632, 9053): `web_push.available()` es hoy
//!   `(False, …pywebpush…)` en esta máquina (D10), así que las cuatro
//!   responden 503 sin leer nada. El cuerpo del DELETE ya lo admitió el
//!   transporte (`delete_body`, P49).
//! - POST `/pomodoro` (`pomodoro_post` 6674): ajustes por el carril de uso,
//!   `ack` (borra `focus-queue.jsonl`) u orden del temporizador por el worker
//!   de app-state; tras la orden, siempre `wake()` del planificador del frente
//!   (el `finally: _POMODORO_WAKE.set()`).
//!
//! Efectos en vivo: las transacciones de Pomodoro son las del Python
//! (`requestId` + `expectedRevision` del store portado); los ajustes, un
//! `insert … on conflict` por clave en una transacción; `ack`, un `unlink`.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    background::pomodoro::{schedule_sounds, token_hex, wake, with_store},
    delete_body,
    light::data,
    pomodoro::settings as loaded_settings,
    py::{Conversion, int_of, str_scalar},
    reply,
};
use crate::{HandlerError, Request};
use comandos_core::json::{response_dumps, truthy};
use comandos_store::{pomodoro, usage};
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushRoute {
    Key,
    SubscriptionPost,
    SubscriptionDelete,
    Test,
    PomodoroPost,
}

pub const ROUTES: &[Entry] = &[
    // `urllib.parse.urlsplit(self.path).path == "/push/key"`.
    Entry {
        verb: Verb::Get,
        key: Key::Path("/push/key"),
        route: NativeRoute::Push(PushRoute::Key),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/push/subscription"),
        route: NativeRoute::Push(PushRoute::SubscriptionPost),
    },
    Entry {
        verb: Verb::Delete,
        key: Key::Raw("/push/subscription"),
        route: NativeRoute::Push(PushRoute::SubscriptionDelete),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/push/test"),
        route: NativeRoute::Push(PushRoute::Test),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/pomodoro"),
        route: NativeRoute::Push(PushRoute::PomodoroPost),
    },
];

/// `web_push.available()[1]` sin `pywebpush` (D10).
pub const UNAVAILABLE: &str =
    "Push no disponible: falta pywebpush (ModuleNotFoundError); ver requirements-push.txt";

/// `POMODORO_STYLES` (6462).
pub const POMODORO_STYLES: [&str; 6] = [
    "alchemy", "arcade", "shikashi", "soul", "garden", "crystals",
];

/// Claves que `set_focus_settings` guarda (cc_usage 2322).
const FOCUS_SETTINGS: [&str; 7] = [
    "focusMinutes",
    "shortBreakMinutes",
    "longBreakMinutes",
    "cycles",
    "autoBreak",
    "dailyGoalMinutes",
    "style",
];

pub async fn answer(native: &Arc<Native>, route: PushRoute, request: &Request) -> Answer {
    let unavailable = json!({"ok": false, "error": UNAVAILABLE});
    match route {
        PushRoute::Key => reply(
            StatusCode::SERVICE_UNAVAILABLE,
            &json!({"available": false, "error": UNAVAILABLE}),
        ),
        PushRoute::SubscriptionPost | PushRoute::Test => {
            reply(StatusCode::SERVICE_UNAVAILABLE, &unavailable)
        }
        PushRoute::SubscriptionDelete => {
            delete_body(request)?;
            reply(StatusCode::SERVICE_UNAVAILABLE, &unavailable)
        }
        PushRoute::PomodoroPost => pomodoro_post(native, request).await,
    }
}

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// La petición entera en su propia tarea: si el cliente se va a mitad, la
/// orden y el `wake()` posterior terminan igual (el hilo del Python también).
async fn pomodoro_post(native: &Arc<Native>, request: &Request) -> Answer {
    let d = data(request)?.clone();
    let task_native = Arc::clone(native);
    let job = tokio::spawn(async move { post(&task_native, &d).await });
    job.await.map_err(|_| failure())?
}

async fn post(native: &Arc<Native>, d: &Map<String, Value>) -> Answer {
    if let Some(settings) = d.get("settings").filter(|v| !v.is_null()) {
        let Value::Object(settings) = settings else {
            return reply(
                StatusCode::BAD_REQUEST,
                &json!({"ok": false, "error": "settings inválido"}),
            );
        };
        // D3: un estilo global para toda la app (seis estilos aprobados).
        if let Some(style) = settings.get("style")
            && !POMODORO_STYLES.iter().any(|known| style == *known)
        {
            return reply(
                StatusCode::BAD_REQUEST,
                &json!({"ok": false, "error": "Estilo de Pomodoro desconocido"}),
            );
        }
        let saved = set_focus_settings(native, settings).await?;
        return reply(StatusCode::OK, &json!({"ok": true, "settings": saved}));
    }
    if d.get("ack").is_some_and(truthy) {
        // `os.remove(FOCUS_QUEUE)` con `except OSError: pass`.
        let queue = native.options().hooks.join("focus-queue.jsonl");
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(queue)).await;
        return reply(StatusCode::OK, &json!({"ok": true}));
    }
    let command = if d.contains_key("action") {
        Value::Object(d.clone())
    } else {
        legacy_command(d)?
    };
    let answer = command_reply(native, command).await;
    // `finally: _POMODORO_WAKE.set()`.
    wake();
    answer
}

/// `pomodoro_store().command(command)`; `PomodoroError` → `(status, payload)`.
/// Cualquier otro error es una excepción sin capturar del Python (500).
async fn command_reply(native: &Arc<Native>, command: Value) -> Answer {
    let clock = native.options().clock.clone();
    let result = native
        .with_state(move |b| with_store(b, &clock, |store| store.command(&command)))
        .await?;
    match result {
        Ok((value, sounds)) => {
            schedule_sounds(native, sounds);
            reply(StatusCode::OK, &value)
        }
        Err(pomodoro::Error::Domain(error)) => {
            let status = StatusCode::from_u16(error.status).map_err(|_| failure())?;
            reply(status, &error.payload())
        }
        Err(pomodoro::Error::Persistence(_)) => Err(failure()),
    }
}

/// `_pomodoro_legacy_command` (6658): el cuerpo anterior a 1.0
/// (`{mins, mode}` / `{stop}`) del operador. Declina antes de efectos lo que
/// no se puede reproducir con certeza (un `mins` exótico, un `mode` que no es
/// escalar).
fn legacy_command(d: &Map<String, Value>) -> Result<Value, Fault> {
    let rid = format!("legacy-{}", token_hex(16).map_err(|_| failure())?);
    if d.get("stop").is_some_and(truthy) {
        return Ok(json!({"requestId": rid, "expectedRevision": null, "action": "cancel"}));
    }
    // `int(data.get("mins") or 25)` con `except (TypeError, ValueError): 0`.
    let mins = match d.get("mins").filter(|v| truthy(v)) {
        None => 25,
        Some(raw) => match int_of(raw) {
            Ok(mins) => mins,
            Err(Conversion::Value(_) | Conversion::Type) => 0,
            // `OverflowError` no se captura: 500 en el Python.
            Err(Conversion::Overflow) => return Err(failure()),
            Err(Conversion::Exotic) => return Err(Fault::Decline),
        },
    };
    let target = mins.checked_mul(60_000).ok_or(Fault::Decline)?;
    // `str(data.get("mode") or "focus")`.
    let mode = match d.get("mode").filter(|v| truthy(v)) {
        None => "focus".to_owned(),
        Some(mode) => str_scalar(mode).ok_or(Fault::Decline)?,
    };
    let field = |key: &str| d.get(key).cloned().unwrap_or(Value::Null);
    Ok(json!({
        "requestId": rid,
        "expectedRevision": null,
        "action": "start",
        "mode": mode,
        "targetMs": target,
        "project": field("project"),
        "sessionKey": field("session"),
        "paneKey": field("pane"),
        "cycleIndex": field("cycleIndex"),
        "cycleTotal": field("cycleTotal"),
    }))
}

/// Lo que devuelve el trabajo de ajustes en el carril de uso.
enum Saved {
    Rows(Vec<(String, Option<String>)>),
    /// Una fila no se puede decodificar con certeza: se deshizo la escritura.
    Unsure,
}

/// `cc_usage.set_focus_settings(USAGE_DB, settings)`: `init_db`, un upsert por
/// clave permitida (`json.dumps(value)`) y las filas en el orden de la tabla,
/// en una transacción. Si alguna fila no se puede leer con certeza la
/// transacción se deshace y se declina (sin efectos); un error de SQLite es
/// la excepción del Python (500).
async fn set_focus_settings(
    native: &Native,
    settings: &Map<String, Value>,
) -> Result<Map<String, Value>, Fault> {
    let mut pairs = Vec::new();
    for (key, value) in settings {
        if FOCUS_SETTINGS.contains(&key.as_str()) {
            // Un valor que el codificador portado no escribe: aún no hubo efectos.
            let text = response_dumps(value).map_err(|_| Fault::Decline)?;
            pairs.push((key.clone(), text));
        }
    }
    let saved = native
        .usage
        .with(move |u| -> rusqlite::Result<Saved> {
            usage::ensure_schema(&u.conn).map_err(store_sql)?;
            let tx = rusqlite::Transaction::new_unchecked(&u.conn, rusqlite::TransactionBehavior::Immediate)?;
            comandos_store::migrate::move_db::admit_write(&u.conn).map_err(store_sql)?;
            for (key, text) in &pairs {
                tx.execute(
                    "insert into focus_settings(key,value) values(?,?) on conflict(key) do update set value=excluded.value",
                    rusqlite::params![key, text],
                )?;
            }
            let rows = usage::focus_settings_rows(&tx).map_err(store_sql)?;
            if loaded_settings(rows.clone()).is_err() {
                tx.rollback()?;
                return Ok(Saved::Unsure);
            }
            tx.commit()?;
            Ok(Saved::Rows(rows))
        })
        .await?
        .map_err(|_| failure())?;
    match saved {
        Saved::Rows(rows) => loaded_settings(rows),
        Saved::Unsure => Err(Fault::Decline),
    }
}

/// Un error del store de uso como error de SQLite (todos terminan en 500).
fn store_sql(error: comandos_store::Error) -> rusqlite::Error {
    match error {
        comandos_store::Error::Sql(error) => error,
        other => rusqlite::Error::ToSqlConversionFailure(other.to_string().into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_command_translates_like_python() {
        let d = |v: Value| v.as_object().cloned().unwrap();
        let cancel = legacy_command(&d(json!({"stop": 1}))).ok().unwrap();
        assert_eq!(cancel["action"], "cancel");
        assert!(cancel["requestId"].as_str().unwrap().starts_with("legacy-"));
        assert_eq!(cancel["requestId"].as_str().unwrap().len(), 39);
        let start = legacy_command(&d(json!({"mins": "30", "session": "s"})))
            .ok()
            .unwrap();
        assert_eq!(start["targetMs"], 1_800_000);
        assert_eq!(start["mode"], "focus");
        assert_eq!(start["sessionKey"], "s");
        assert_eq!(start["project"], Value::Null);
        // `or 25`, `ValueError` → 0, `TypeError` → 0, `int(True)` = 1.
        let target =
            |v: Value| legacy_command(&d(json!({"mins": v}))).ok().unwrap()["targetMs"].clone();
        assert_eq!(target(json!(0)), 1_500_000);
        assert_eq!(target(json!("x")), 0);
        assert_eq!(target(json!([1])), 0);
        assert_eq!(target(json!(true)), 60_000);
        assert_eq!(target(json!(2.9)), 120_000);
        let mode = legacy_command(&d(json!({"mode": 5}))).ok().unwrap();
        assert_eq!(mode["mode"], "5");
        assert!(matches!(
            legacy_command(&d(json!({"mode": {"a": 1}}))),
            Err(Fault::Decline)
        ));
    }
}
