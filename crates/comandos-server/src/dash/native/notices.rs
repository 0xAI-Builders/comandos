//! A. Notificaciones (`bin/cc-dash`: `/notifs/count`, `/notices/watch`,
//! `/notices`, el bloque POST de avisos; `lib/notification_delivery.py`).
//! La base, en el worker; tmux, fuera de él.
use super::{
    Answer, Entry, Fault, Key, Native, NativeRoute, Verb,
    light::{data, error},
    py::{self, NumError},
    query::Query,
    reply,
    tmux::Tmux,
};
use crate::{HandlerError, Reply, Request};
use comandos_core::{
    json::truthy,
    notifications::{LiveCheck, live_pending},
};
use comandos_store::{Error as StoreError, latest_sequence, notifications as nd};
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticesRoute {
    List,
    Prefs,
    Watch,
    Count,
    Presence,
    Read,
    Sound,
    SavePrefs,
}

const fn entry(verb: Verb, key: Key, route: NoticesRoute) -> Entry {
    Entry {
        verb,
        key,
        route: NativeRoute::Notices(route),
    }
}

pub const ROUTES: &[Entry] = &[
    entry(Verb::Get, Key::Path("/notices"), NoticesRoute::List),
    entry(Verb::Get, Key::Path("/notices/prefs"), NoticesRoute::Prefs),
    entry(Verb::Get, Key::Path("/notices/watch"), NoticesRoute::Watch),
    entry(Verb::Get, Key::Path("/notifs/count"), NoticesRoute::Count),
    entry(Verb::Post, Key::Raw("/presence"), NoticesRoute::Presence),
    entry(Verb::Post, Key::Raw("/notices/read"), NoticesRoute::Read),
    entry(Verb::Post, Key::Raw("/notices/sound"), NoticesRoute::Sound),
    entry(
        Verb::Post,
        Key::Raw("/notices/prefs"),
        NoticesRoute::SavePrefs,
    ),
];

/// `notices_is_live()`: las terminales vivas según tmux.
#[derive(Debug, Clone)]
pub struct LiveSet {
    panes: HashSet<(String, String)>,
    sessions: HashSet<String>,
}

impl LiveSet {
    /// `(s, p or "") in live if p else s in sessions`. Las tuplas del
    /// conjunto solo tienen cadenas: un no-str nunca está dentro.
    pub fn check(&self, session: &Value, pane: &Value) -> bool {
        let Some(s) = session.as_str() else {
            return false;
        };
        if truthy(pane) {
            pane.as_str()
                .is_some_and(|p| self.panes.contains(&(s.to_owned(), p.to_owned())))
        } else {
            self.sessions.contains(s)
        }
    }
}

/// `tmux list-panes -a -F "#{session_name}\t#{pane_id}"` con el plazo del
/// Python (5 s). Código distinto de cero, plazo vencido, tmux ausente o
/// salida ilegible: `None`, no se filtra nada (como el `except Exception`).
pub async fn is_live(tmux: &Tmux) -> Option<LiveSet> {
    let out = tmux
        .run(&["list-panes", "-a", "-F", "#{session_name}\t#{pane_id}"])
        .await
        .ok()?;
    if !out.ok {
        return None;
    }
    let panes: HashSet<(String, String)> = py::splitlines(&out.stdout)
        .into_iter()
        .filter_map(|line| line.split_once('\t'))
        .map(|(s, p)| (s.to_owned(), p.to_owned()))
        .collect();
    let sessions = panes.iter().map(|(s, _)| s.clone()).collect();
    Some(LiveSet { panes, sessions })
}

/// Presta el `LiveCheck` del store durante `body`.
pub fn with_live<T>(live: &Option<LiveSet>, body: impl FnOnce(LiveCheck<'_>) -> T) -> T {
    match live {
        Some(set) => {
            let check = |s: &Value, p: &Value| set.check(s, p);
            body(Some(&check))
        }
        None => body(None),
    }
}

/// `int(text)`: `Ok(Ok(n))`, `Ok(Err(texto del ValueError))` (que la ruta
/// convierte en 400) o la negativa si no se puede reproducir con certeza.
fn int_param(text: &str) -> Result<Result<i64, String>, Fault> {
    match py::int(text) {
        Ok(value) => Ok(Ok(value)),
        Err(NumError::Exotic) => Err(Fault::Decline),
        Err(NumError::Invalid) => py::int_error_message(text).map(Err).ok_or(Fault::Decline),
    }
}

/// 200 de una ruta de solo lectura. Cualquier fallo (de la base o del
/// codificador portado) declina: no hubo efectos y el Python responde lo suyo.
fn read_reply(result: Result<Value, StoreError>) -> Answer {
    let value = result.map_err(|_| Fault::Decline)?;
    Reply::json(StatusCode::OK, &value).map_err(|_| Fault::Decline)
}

/// `except (ValueError, TypeError) as exc: 400 str(exc)`; SQLite → 500.
fn write_reply(result: Result<Value, StoreError>) -> Answer {
    match result {
        Ok(value) => reply(StatusCode::OK, &value),
        Err(StoreError::Validation(message)) => error(StatusCode::BAD_REQUEST, &message),
        Err(_) => Err(HandlerError::Failure.into()),
    }
}

/// `str(value or "")` con escalares de forma segura; lo demás declina.
fn text_or_empty(value: Option<&Value>) -> Result<String, Fault> {
    match value.filter(|v| truthy(v)) {
        None => Ok(String::new()),
        Some(v) => py::str_scalar(v).ok_or(Fault::Decline),
    }
}

/// `merge_prefs` evalúa `mode not in MODES` (un `set`) tras validar la
/// categoría: una lista u objeto lanza `TypeError: unhashable type`, que la
/// ruta convierte en 400. El store Rust responde «Modo de aviso inválido»;
/// aquí se reproduce el texto del Python antes de cualquier escritura.
fn unhashable_mode(update: &Map<String, Value>) -> Option<&'static str> {
    let modes = update.get("modes")?.as_object()?;
    for (category, mode) in modes {
        if !CATEGORIES.contains(&category.as_str()) {
            return None;
        }
        match mode {
            Value::Array(_) => return Some("unhashable type: 'list'"),
            Value::Object(_) => return Some("unhashable type: 'dict'"),
            Value::String(s) if matches!(s.as_str(), "visual" | "sound") => {}
            _ => return None,
        }
    }
    None
}

/// Las categorías de `default_prefs()["modes"]` (las únicas que `load_prefs` deja).
const CATEGORIES: [&str; 7] = [
    "attention",
    "error",
    "focus",
    "done",
    "news",
    "usage",
    "info",
];

/// `notification_delivery.revision(conn)`, un trabajo corto del worker.
async fn revision(native: &Native) -> Result<String, Fault> {
    native
        .with_state(|b| nd::revision(&b.conn))
        .await?
        .map_err(|_| Fault::Decline)
}

pub async fn answer(native: &Native, route: NoticesRoute, request: &Request) -> Answer {
    let tmux = &native.options().tmux;
    let now = (native.options().clock)();
    match route {
        NoticesRoute::List => {
            let query = Query::parse(&request.target)?;
            let after = match int_param(query.first("after").unwrap_or("0"))? {
                Ok(value) => value,
                Err(message) => return error(StatusCode::BAD_REQUEST, &message),
            };
            let limit = match int_param(query.first("limit").unwrap_or("100"))? {
                Ok(value) => value,
                Err(message) => return error(StatusCode::BAD_REQUEST, &message),
            };
            let device = query.first("deviceId").map(str::to_owned);
            let live = is_live(tmux).await;
            read_reply(
                native
                    .with_state(move |b| {
                        let focus = nd::focus_block_active(&b.conn);
                        with_live(&live, |check| {
                            let mut page = nd::list_notices(
                                &b.conn,
                                after,
                                limit,
                                now,
                                device.as_deref(),
                                focus,
                                check,
                            )?;
                            // El mismo número de la campana en todas las superficies.
                            let badge = nd::badge_count(&b.conn, check)?;
                            if let Some(map) = page.as_object_mut() {
                                map.insert("badge".into(), json!(badge));
                            }
                            Ok(page)
                        })
                    })
                    .await?,
            )
        }
        NoticesRoute::Prefs => read_reply(native.with_state(|b| nd::load_prefs(&b.conn)).await?),
        NoticesRoute::Watch => {
            let query = Query::parse(&request.target)?;
            let seen = query.first("rev").unwrap_or("").to_owned();
            let wait = match py::float(query.first("wait").unwrap_or("25")) {
                Ok(x) => py::clamp_py_float(x, 0.0, 25.0),
                Err(NumError::Invalid) => 25.0,
                Err(NumError::Exotic) => return Err(Fault::Decline),
            };
            let deadline =
                Instant::now() + Duration::try_from_secs_f64(wait).unwrap_or(Duration::ZERO);
            let mut rev = revision(native).await?;
            // Ruling 3: la espera es async; cada vuelta es un trabajo corto del
            // worker, así cincuenta esperas abiertas no retienen la base.
            while rev == seen && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(200)).await;
                rev = revision(native).await?;
            }
            let live = is_live(tmux).await;
            read_reply(
                native
                    .with_state(move |b| {
                        with_live(&live, |check| {
                            let history = nd::recent(&b.conn)?;
                            let badge = nd::badge_count(&b.conn, check)?;
                            let unread = nd::unread_notice_ids(&b.conn, None)?;
                            let tail = unread[unread.len().saturating_sub(2000)..].to_vec();
                            let pending = live_pending(&history, check);
                            let latest = latest_sequence(&b.conn)?;
                            Ok(json!({"rev": rev, "badge": badge, "unread": tail,
                                      "pending": pending, "latest": latest}))
                        })
                    })
                    .await?,
            )
        }
        NoticesRoute::Count => {
            let live = is_live(tmux).await;
            let count = match native
                .with_state(move |b| with_live(&live, |check| nd::badge_count(&b.conn, check)))
                .await
            {
                Ok(Ok(n)) => n,
                // Esquema nuevo o conjunto apagado: se reenvía todo.
                Err(Fault::Decline) => return Err(Fault::Decline),
                // `except Exception: count = 0`.
                _ => 0,
            };
            reply(StatusCode::OK, &json!({"count": count}))
        }
        NoticesRoute::Presence => {
            let data = data(request)?;
            // `str(data.get("kind") or "web")` se evalúa antes de la llamada,
            // pero no puede fallar en el Python; la validación de deviceId sí.
            let device = match data.get("deviceId") {
                Some(Value::String(s)) => s.clone(),
                _ => return error(StatusCode::BAD_REQUEST, "deviceId inválido"),
            };
            let kind = match data.get("kind").filter(|v| truthy(v)) {
                None => "web".to_owned(),
                Some(v) => py::str_scalar(v).ok_or(Fault::Decline)?,
            };
            let is_true = |key: &str| data.get(key) == Some(&Value::Bool(true));
            let (visible, audio, interaction) = (
                is_true("visible"),
                is_true("canPlayAudio"),
                is_true("interaction"),
            );
            write_reply(
                native
                    .with_state(move |b| {
                        nd::record_presence(
                            &b.conn,
                            &device,
                            visible,
                            audio,
                            interaction,
                            now,
                            &json!(kind),
                        )
                        .map(|()| json!({"ok": true}))
                    })
                    .await?,
            )
        }
        NoticesRoute::Read => {
            let data = data(request)?;
            let all = data.get("all") == Some(&Value::Bool(true));
            let ids = data.get("eventIds").cloned();
            let project = match data.get("project") {
                None => None,
                Some(v) if !truthy(v) => None,
                Some(Value::String(s)) => Some(s.clone()),
                // Un proyecto verdadero que no es str no casa con nada: raro, se declina.
                Some(_) if all => return Err(Fault::Decline),
                Some(_) => None,
            };
            if !all && !matches!(ids, Some(Value::Array(_))) {
                return error(StatusCode::BAD_REQUEST, "eventIds inválido");
            }
            write_reply(
                native
                    .with_state(move |b| {
                        let ids: Vec<Value> = if all {
                            // Todo el historial, no solo los últimos 500 eventos.
                            nd::unread_notice_ids(&b.conn, project.as_deref())?
                                .into_iter()
                                .map(Value::String)
                                .collect()
                        } else {
                            match ids {
                                Some(Value::Array(ids)) => ids,
                                _ => Vec::new(),
                            }
                        };
                        let read = nd::mark_read(&b.conn, &ids, now)?;
                        Ok(json!({"ok": true, "read": read}))
                    })
                    .await?,
            )
        }
        NoticesRoute::Sound => {
            let data = data(request)?;
            let event = text_or_empty(data.get("eventId"))?;
            let device = text_or_empty(data.get("deviceId"))?;
            write_reply(
                native
                    .with_state(move |b| {
                        let focus = nd::focus_block_active(&b.conn);
                        nd::claim_sound(&b.conn, &event, &device, now, Some(focus))
                    })
                    .await?,
            )
        }
        NoticesRoute::SavePrefs => {
            let data = data(request)?;
            if let Some(message) = unhashable_mode(data) {
                return error(StatusCode::BAD_REQUEST, message);
            }
            let update = Value::Object(data.clone());
            write_reply(
                native
                    .with_state(move |b| nd::save_prefs(&b.conn, &update))
                    .await?,
            )
        }
    }
}
