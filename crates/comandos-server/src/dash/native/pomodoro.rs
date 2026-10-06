//! F. GET /pomodoro (8321, `pomodoro_payload` 6528): bloque y progreso de
//! app-state, cola de `H/focus-queue.jsonl`, ajustes de la base de uso y la
//! ruta de sonido de los avisos. POST /pomodoro vive en `push.rs` (corte
//! `services`, P3) y despierta el planificador de `background::pomodoro`.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    files::{self, Strict},
    light::read_reply,
};
use crate::HandlerError;
use comandos_core::{focus::policy_v1, json::truthy, notifications::sound_device};
use comandos_store::{focus, notifications as nd, pomodoro::PomodoroStore, usage};
use serde_json::{Map, Value, json};
use std::path::Path;

pub const ROUTES: &[Entry] = &[Entry {
    verb: Verb::Get,
    key: Key::Raw("/pomodoro"),
    route: NativeRoute::Pomodoro,
}];

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// Lo que sale del worker de app-state: snapshot, progreso y ruta de sonido.
type StateParts = (Value, Value, (bool, Value));

pub async fn answer(native: &Native) -> Answer {
    // Con el carril de uso apagado la respuesta acabaría reenviada: se declina
    // antes de calcular nada en el worker de app-state (cc-app sondea cada 2 s).
    if !native.usage.enabled() {
        return Err(Fault::Decline);
    }
    // La cola no tiene efectos: se lee primero para declinar antes de escribir.
    let queue = focus_queue_domain(
        &native.options().home,
        &native.options().hooks.join("focus-queue.jsonl"),
    )?;
    let clock = native.options().clock.clone();
    let (state, progress, sound) = native
        .with_state(move |b| -> Result<StateParts, Fault> {
            let policy = policy_v1();
            // `pomodoro_store()`: la política se activa una vez (idempotente).
            // Un INSERT que falla no escribe nada, y las lecturas no escriben:
            // sus fallos se reenvían y el Python responde lo que corresponda.
            if !b.pomodoro_policy {
                focus::ensure_policy(&b.conn, &policy, clock()).map_err(|_| Fault::Decline)?;
                b.pomodoro_policy = true;
            }
            let new_id = String::new;
            let state = PomodoroStore::new(&b.conn, &*clock, &new_id)
                .snapshot()
                .map_err(|_| Fault::Decline)?;
            let server_now = state["serverNowMs"].as_i64().ok_or_else(failure)?;
            let progress =
                focus::ledger_progress(&b.conn, &policy, server_now).map_err(|_| Fault::Decline)?;
            // `pomodoro_sound_route` (6537): `except Exception: False, None`.
            let sound_now = clock();
            let sound = match (nd::load_prefs(&b.conn), nd::clients(&b.conn, sound_now)) {
                (Ok(prefs), Ok(clients)) => {
                    let enabled = prefs["modes"].get("focus") == Some(&json!("sound"))
                        && !truthy(&prefs["muted"]);
                    (enabled, sound_device(&clients, sound_now))
                }
                _ => (false, Value::Null),
            };
            Ok((state, progress, sound))
        })
        .await??;
    // `read_focus_settings(USAGE_DB)`: `init_db` + `select key,value`. Un fallo
    // de la base de uso se reenvía (el Python responde lo que corresponda).
    let rows = native
        .usage
        .with(|u| usage::focus_settings_rows(&u.conn))
        .await?
        .map_err(|_| Fault::Decline)?;
    let settings = settings(rows)?;
    let Value::Object(mut out) = state else {
        return Err(failure());
    };
    out.insert("queue".into(), Value::Array(queue));
    out.insert("settings".into(), Value::Object(settings));
    out.insert("progress".into(), progress);
    let (enabled, device) = sound;
    out.insert(
        "sound".into(),
        json!({
            "enabled": enabled,
            "device": if enabled { device } else { Value::Null },
            "desktopDevice": native.options().desktop_device,
        }),
    );
    read_reply(&Value::Object(out))
}

/// `output[key] = json.loads(value)` con `except Exception: pass`. Un valor no
/// textual (BLOB: `json.loads(bytes)` sí decodifica) o incierto declina.
pub(crate) fn settings(rows: Vec<(String, Option<String>)>) -> Result<Map<String, Value>, Fault> {
    let mut out = Map::new();
    for (key, value) in rows {
        match value.as_deref().map(files::loads_strict) {
            Some(Strict::Value(v)) => {
                out.insert(key, v);
            }
            Some(Strict::Missing | Strict::Unreadable) => {}
            Some(Strict::Unsure) | None => return Err(Fault::Decline),
        }
    }
    Ok(out)
}

fn focus_queue_domain(home: &Path, path: &Path) -> Result<Vec<Value>, Fault> {
    comandos_store::unified::with_readonly_access(home, "logs", |mode, db| {
        if !matches!(
            mode,
            comandos_store::unified::Mode::Unified | comandos_store::unified::Mode::Sealed
        ) {
            return Ok(focus_queue(path));
        }
        let Some(db) = db else {
            return Ok(Err(Fault::Decline));
        };
        let lines = comandos_store::unified::log_tail(
            db,
            comandos_store::unified::LogName::FocusQueue,
            i64::MAX as usize,
        )?;
        let mut body = Vec::new();
        for line in lines {
            body.extend(line);
            body.push(b'\n');
        }
        Ok(focus_queue_bytes(body))
    })
    .map_err(|_| Fault::Decline)?
}
/// `focus_queue` (135): líneas JSON válidas, las últimas 50. Modo texto: bytes
/// no UTF-8 lanzan fuera del `try` interno (500 en el Python) → se declina.
fn focus_queue(path: &Path) -> Result<Vec<Value>, Fault> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        // `except OSError: pass`.
        Err(_) => return Ok(Vec::new()),
    };
    focus_queue_bytes(bytes)
}
fn focus_queue_bytes(bytes: Vec<u8>) -> Result<Vec<Value>, Fault> {
    let text = String::from_utf8(bytes).map_err(|_| Fault::Decline)?;
    // Saltos universales del modo texto: `\r\n` y `\r` terminan línea.
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = Vec::new();
    for line in text.split_inclusive('\n') {
        match files::loads_strict(line) {
            Strict::Value(v) => out.push(v),
            Strict::Missing | Strict::Unreadable => {}
            Strict::Unsure => return Err(Fault::Decline),
        }
    }
    let start = out.len().saturating_sub(50);
    Ok(out.split_off(start))
}
