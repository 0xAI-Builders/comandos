//! Planificador de Pomodoro del frente (`pomodoro_scheduler_loop` 6550 de
//! `bin/cc-dash`) y el `PomodoroStore` con los callbacks de `pomodoro_store()`
//! (6496): evento N1 del bloque completado (`_pomodoro_emit` 6469), premio de
//! foco (`_pomodoro_rewards` 6492), registro en stderr (`_pomodoro_log` 6465)
//! y, un segundo después del evento, el sonido de escritorio reclamado una
//! sola vez entre todos los dispositivos (`_desktop_notice_sound` 1022).
//!
//! Todo lo que toca app-state pasa por el worker (`Native::with_state`); la base
//! de uso, por su carril. El bucle tiene un único temporizador: espera
//! `min(30, max(0.05, (deadline - now)/1000 + 0.02))` segundos o hasta que
//! alguien llame a `wake()` (el `_POMODORO_WAKE.set()` de POST `/pomodoro`).
//! `settle_due` es una transacción por bloque vencido: con el planificador del
//! Python vivo sobre la misma base, cada bloque se cierra una sola vez.
use super::Stop;
use crate::dash::native::{
    Clock, Fault, Native, NativeOptions,
    files::{Strict, read_json_strict},
    procs::{spawn_detached, which_in},
    py::clamp_py_float,
    state::StateBackend,
};
use comandos_core::{focus::policy_v1, notifications::LOCAL_SPEAKER};
use comandos_store::{focus, notifications as nd, pomodoro};
use rusqlite::{Connection, types::ValueRef};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    ffi::OsString,
    fmt::Write as _,
    io,
    path::PathBuf,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

/// Espera máxima entre vueltas (el `delay = 30.0` del Python).
const IDLE_SECONDS: f64 = 30.0;
/// Espera tras un error (`delay = 2.0`).
const RETRY_SECONDS: f64 = 2.0;
/// `threading.Timer(1.0, …)` de `_pomodoro_emit`.
const SOUND_DELAY: Duration = Duration::from_secs(1);
/// `POMODORO_END_SOUND`, relativo al checkout del heredado.
const END_SOUND: &str = "assets/sounds/pomodoro-complete.wav";

/// `_POMODORO_WAKE`: un `threading.Event` de proceso. `WAKES` cuenta los
/// `set()`; cada planificador recuerda el último que vio, así un `wake()` que
/// llega mientras el planificador trabaja no se pierde (como el `Event`, que
/// queda puesto hasta el `clear()`), y despierta a todos los planificadores
/// del proceso (las pruebas levantan varios).
static WAKE: Notify = Notify::const_new();
static WAKES: AtomicU64 = AtomicU64::new(0);

/// `_POMODORO_WAKE.set()`: el planificador recalcula ya su espera.
pub fn wake() {
    WAKES.fetch_add(1, Ordering::AcqRel);
    WAKE.notify_waiters();
}

/// `uuid.uuid4().hex`.
pub(crate) fn uuid4_hex() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
    if let Some(b) = bytes.get_mut(6) {
        *b = (*b & 0x0f) | 0x40;
    }
    if let Some(b) = bytes.get_mut(8) {
        *b = (*b & 0x3f) | 0x80;
    }
    Ok(hex(&bytes))
}

/// `secrets.token_hex(n)`.
pub(crate) fn token_hex(n: usize) -> io::Result<String> {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(hex(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn io_error(error: io::Error) -> pomodoro::Error {
    pomodoro::Error::Persistence(comandos_store::Error::Io(error))
}

/// `pomodoro_store()` con sus callbacks, dentro del worker de app-state. Devuelve
/// lo que devuelva `run` y los eventos nuevos cuyo sonido hay que reclamar
/// (solo si la transacción del store se confirmó). La política de foco se
/// activa una vez por proceso (`focus_progress.ensure_policy`, idempotente).
pub(crate) fn with_store<T>(
    b: &mut StateBackend,
    clock: &Clock,
    run: impl FnOnce(&pomodoro::PomodoroStore<'_>) -> pomodoro::Result<T>,
) -> pomodoro::Result<(T, Vec<String>)> {
    let policy = policy_v1();
    if !b.pomodoro_policy {
        focus::ensure_policy(&b.conn, &policy, clock())?;
        b.pomodoro_policy = true;
    }
    // Un bloque nuevo toma este id (`uuid.uuid4().hex` de `command`).
    let block_id = uuid4_hex().map_err(io_error)?;
    let new_id = move || block_id.clone();
    let now = || clock();
    let sounds = RefCell::new(Vec::new());
    // `_pomodoro_emit`: el evento entra en la MISMA transacción del cierre.
    let emit = |conn: &Connection, event: &Value| -> pomodoro::Result<()> {
        let at = u64::try_from(clock()).unwrap_or(0);
        let fallback = uuid4_hex().map_err(io_error)?;
        let receipt = format!("receipt-{}", uuid4_hex().map_err(io_error)?);
        let stored = comandos_store::append_event(conn, event, at, &fallback, &receipt)?;
        if stored.get("duplicate") != Some(&Value::Bool(true))
            && let Some(id) = stored.get("eventId").and_then(Value::as_str)
        {
            sounds.borrow_mut().push(id.to_owned());
        }
        Ok(())
    };
    let rewards = |conn: &Connection, record: &Value, at: i64| -> pomodoro::Result<()> {
        focus::award(conn, record, &policy, at)?;
        Ok(())
    };
    // `_pomodoro_log`: `sys.stderr.write(message.rstrip() + "\n")`.
    let log = |message: &str| eprintln!("{}", message.trim_end());
    let mut store = pomodoro::PomodoroStore::new(&b.conn, &now, &new_id);
    store.emit = Some(&emit);
    store.rewards = Some(&rewards);
    store.log = Some(&log);
    let out = run(&store)?;
    Ok((out, sounds.into_inner()))
}

/// Un segundo después de cada evento nuevo, su sonido de escritorio (el
/// `threading.Timer(1.0, …)` de `_pomodoro_emit`). Tarea registrada (D12).
pub(crate) fn schedule_sounds(native: &Arc<Native>, events: Vec<String>) {
    for event in events {
        let task_native = Arc::clone(native);
        let _ = native.tasks().spawn(async move {
            tokio::time::sleep(SOUND_DELAY).await;
            desktop_sound(&task_native, event).await;
        });
    }
}

/// `_safe_desktop_sound` → `_desktop_notice_sound` para un evento de Pomodoro:
/// el primer dispositivo de (`DESKTOP_DEVICE`, `local-speaker`) que gana el
/// reclamo toca el jingle con el volumen de las preferencias de avisos.
async fn desktop_sound(native: &Arc<Native>, event: String) {
    let clock = native.options().clock.clone();
    let device = native.options().desktop_device.clone();
    let id = event.clone();
    let claimed = native
        .with_state(move |b| -> Result<Option<Value>, String> {
            let now = clock();
            for device in [device.as_str(), LOCAL_SPEAKER] {
                let claim =
                    nd::claim_sound(&b.conn, &id, device, now, None).map_err(|e| e.to_string())?;
                if claim.get("play") == Some(&Value::Bool(true)) {
                    let prefs = nd::load_prefs(&b.conn).map_err(|e| e.to_string())?;
                    // `load_prefs(conn).get("volume", 0.6)`.
                    return Ok(Some(prefs.get("volume").cloned().unwrap_or(json!(0.6))));
                }
            }
            Ok(None)
        })
        .await;
    let volume = match claimed {
        Ok(Ok(Some(volume))) => volume,
        Ok(Ok(None)) => return,
        Ok(Err(message)) => {
            eprintln!("notice sound {event}: {message}");
            return;
        }
        // Conjunto apagado o worker caído: el reclamo queda para el Python.
        Err(_) => return,
    };
    let task_native = Arc::clone(native);
    let played =
        tokio::task::spawn_blocking(move || play_end_sound(task_native.options(), &volume)).await;
    if let Ok(Err(message)) = played {
        eprintln!("notice sound {event}: {message}");
    }
}

/// `float(volume)` de un valor JSON de las preferencias.
fn py_float(value: &Value) -> Result<f64, String> {
    match value {
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::Number(n) => n
            .as_str()
            .parse::<f64>()
            .map_err(|_| format!("could not convert string to float: {n}")),
        Value::Null => {
            Err("float() argument must be a string or a real number, not 'NoneType'".into())
        }
        _ => Err("float() argument must be a string or a real number".into()),
    }
}

/// `_play_local_sound(POMODORO_END_SOUND, volume)`: el primer reproductor que
/// exista (`pw-play`, `paplay`, `afplay`) con el archivo presente, en su propio
/// grupo de procesos. Bloquea (comprueba archivos): `spawn_blocking`.
fn play_end_sound(opts: &NativeOptions, volume: &Value) -> Result<bool, String> {
    let v = clamp_py_float(py_float(volume)?, 0.0, 1.0);
    let Some(sound) = opts.repo_root.as_ref().map(|root| root.join(END_SOUND)) else {
        return Ok(false);
    };
    let fixed = format!("{v:.2}");
    let players: [(&str, Vec<OsString>); 3] = [
        (
            "pw-play",
            vec![format!("--volume={fixed}").into(), sound.clone().into()],
        ),
        (
            "paplay",
            vec![
                format!("--volume={}", (v * 65536.0).trunc() as i64).into(),
                sound.clone().into(),
            ],
        ),
        (
            "afplay",
            vec!["-v".into(), fixed.clone().into(), sound.clone().into()],
        ),
    ];
    let search = opts.search_path.as_deref();
    for (name, args) in players {
        if let Some(path) = which_in(search, name)
            && sound.exists()
        {
            spawn_detached(&opts.program(path), &args, &[]).map_err(|e| e.to_string())?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Arranca el planificador como tarea registrada. Guarda un `Weak`: el frente
/// apagado (o soltado) termina el bucle en su próxima vuelta. `owner`: el
/// frente es el dueño (`front`) y hace la migración de arranque; con `legacy`
/// la hace el Python y el frente solo cierra bloques vencidos.
pub(crate) fn spawn(native: &Arc<Native>, stop: Arc<Stop>, owner: bool) -> bool {
    let weak = Arc::downgrade(native);
    native.tasks().spawn(scheduler(weak, stop, owner)).is_ok()
}

async fn scheduler(weak: Weak<Native>, stop: Arc<Stop>, owner: bool) {
    if owner && let Some(native) = weak.upgrade() {
        adopt_legacy(&native).await;
        import_history(&native).await;
    }
    let mut seen = WAKES.load(Ordering::Acquire);
    loop {
        if stop.is_set() {
            return;
        }
        let delay = match weak.upgrade() {
            Some(native) if native.enabled() => tick(&native).await,
            _ => return,
        };
        // `_POMODORO_WAKE.wait(delay); _POMODORO_WAKE.clear()`. Se apunta a la
        // notificación ANTES de mirar el contador: un `wake()` entre las dos
        // cosas no se pierde.
        let woken = WAKE.notified();
        tokio::pin!(woken);
        woken.as_mut().enable();
        if WAKES.load(Ordering::Acquire) == seen && !stop.is_set() {
            tokio::select! {
                () = tokio::time::sleep(delay) => {}
                () = &mut woken => {}
                () = stop.wait() => {}
            }
        }
        seen = WAKES.load(Ordering::Acquire);
    }
}

fn seconds(value: f64) -> Duration {
    Duration::try_from_secs_f64(value).unwrap_or(Duration::from_secs(30))
}

/// Una vuelta: `settle_due` y la espera hasta el próximo vencimiento.
async fn tick(native: &Arc<Native>) -> Duration {
    let clock = native.options().clock.clone();
    let job_clock = clock.clone();
    let settled = native
        .with_state(move |b| {
            with_store(b, &job_clock, |store| {
                store.settle_due(None)?;
                store.next_deadline_ms()
            })
        })
        .await;
    match settled {
        Ok(Ok((deadline, sounds))) => {
            schedule_sounds(native, sounds);
            let delay = deadline.map_or(IDLE_SECONDS, |deadline| {
                let left = deadline.saturating_sub(clock()) as f64 / 1000.0 + 0.02;
                IDLE_SECONDS.min(left.max(0.05))
            });
            seconds(delay)
        }
        Ok(Err(error)) => {
            eprintln!("cc-dash pomodoro scheduler: {error}");
            seconds(RETRY_SECONDS)
        }
        // Declinado: el conjunto se apaga (la próxima vuelta lo ve) o la base
        // está ocupada ahora; se reintenta como tras un error.
        Err(Fault::Decline) => seconds(RETRY_SECONDS),
        Err(Fault::Error(_)) => {
            eprintln!("cc-dash pomodoro scheduler: el trabajo en app-state no respondió");
            seconds(RETRY_SECONDS)
        }
    }
}

/// `pomodoro_adopt_legacy` (6508): el bloque vivo de `focus.json` (la
/// autoridad anterior) se adopta y el archivo se renombra una vez.
async fn adopt_legacy(native: &Arc<Native>) {
    let path = native.options().hooks.join("focus.json");
    let read_path = path.clone();
    // `json.load`: `OSError`/`ValueError` → no hay nada que adoptar.
    let data = match tokio::task::spawn_blocking(move || read_json_strict(&read_path)).await {
        Ok(Strict::Value(data)) => data,
        _ => return,
    };
    let clock = native.options().clock.clone();
    let adopted = native
        .with_state(move |b| with_store(b, &clock, |store| store.import_legacy_focus(&data, None)))
        .await;
    match adopted {
        Ok(Ok((_, sounds))) => {
            schedule_sounds(native, sounds);
            // `os.replace(FOCUS_FILE, f"{FOCUS_FILE}.migrated-{int(time.time())}")`.
            let stamp = (native.options().clock_seconds)().floor() as i64;
            let mut target = OsString::from(path.as_os_str());
            target.push(format!(".migrated-{stamp}"));
            let target = PathBuf::from(target);
            let _ = tokio::task::spawn_blocking(move || std::fs::rename(path, target)).await;
        }
        Ok(Err(error)) => {
            eprintln!("cc-dash pomodoro: no se pudo adoptar focus.json: {error}");
        }
        Err(_) => {}
    }
}

/// `pomodoro_store().import_legacy_history(cc_usage.focus_legacy_rows(USAGE_DB))`.
async fn import_history(native: &Arc<Native>) {
    // `focus_legacy_rows`: sin base, `[]` y la base no se crea.
    let db = native.options().usage_db.clone();
    let exists = tokio::task::spawn_blocking(move || db.try_exists().unwrap_or(false))
        .await
        .unwrap_or(false);
    let rows = if exists {
        match native.usage.with(|u| legacy_rows(&u.conn)).await {
            Ok(Some(rows)) => rows,
            // Un BLOB o el carril apagado: lo mapea el Python.
            _ => return,
        }
    } else {
        Vec::new()
    };
    let clock = native.options().clock.clone();
    let imported = native
        .with_state(move |b| {
            with_store(b, &clock, |store| store.import_legacy_history(&rows, None))
        })
        .await;
    if let Ok(Err(error)) = imported {
        eprintln!("cc-dash pomodoro: no se pudo mapear el historial anterior: {error}");
    }
}

/// `focus_legacy_rows` (cc_usage 2275): cada bloque anterior a 1.0, del más
/// antiguo al más nuevo, como `dict(row)`. Un error de SQLite → `[]`. `None`
/// si una celda es un BLOB (el Python la pasaría como `bytes`).
fn legacy_rows(conn: &Connection) -> Option<Vec<Value>> {
    const COLUMNS: [&str; 9] = [
        "id",
        "mode",
        "project",
        "tmux_session",
        "tmux_pane",
        "planned_minutes",
        "started_at_ms",
        "ended_at_ms",
        "status",
    ];
    let query = || -> rusqlite::Result<Option<Vec<Value>>> {
        let mut stmt = conn.prepare(
            "select id,mode,project,tmux_session,tmux_pane,planned_minutes,
              started_at_ms,ended_at_ms,status from focus_blocks order by started_at_ms, id",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mut object = serde_json::Map::new();
            for (i, name) in COLUMNS.iter().enumerate() {
                let value = match row.get_ref(i)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(n) => json!(n),
                    ValueRef::Real(x) => json!(x),
                    ValueRef::Text(t) => match std::str::from_utf8(t) {
                        Ok(text) => json!(text),
                        Err(_) => return Ok(None),
                    },
                    ValueRef::Blob(_) => return Ok(None),
                };
                object.insert((*name).to_owned(), value);
            }
            out.push(Value::Object(object));
        }
        Ok(Some(out))
    };
    query().unwrap_or(Some(Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid4_hex_has_version_and_variant() {
        let id = uuid4_hex().unwrap();
        assert_eq!(id.len(), 32);
        assert_eq!(&id[12..13], "4");
        assert!(matches!(&id[16..17], "8" | "9" | "a" | "b"));
        assert_eq!(token_hex(16).unwrap().len(), 32);
    }

    #[test]
    fn volume_floats_like_python() {
        assert_eq!(py_float(&json!(0.6)), Ok(0.6));
        assert_eq!(py_float(&json!(true)), Ok(1.0));
        assert!(py_float(&Value::Null).is_err());
        assert!(py_float(&json!("x")).is_err());
    }

    #[test]
    fn legacy_rows_reads_in_order_and_tolerates_missing_table() {
        let conn = Connection::open_in_memory().unwrap();
        assert_eq!(legacy_rows(&conn), Some(Vec::new()));
        conn.execute_batch(
            "create table focus_blocks(id text, mode text, project text, tmux_session text,
               tmux_pane text, planned_minutes integer, started_at_ms integer,
               ended_at_ms integer, status text);
             insert into focus_blocks values('b','focus','p','s','%1',25,20,null,'completed');
             insert into focus_blocks values('a','focus','p','s','%1',25.5,10,15,'completed');",
        )
        .unwrap();
        let rows = legacy_rows(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], "a");
        assert_eq!(rows[0]["planned_minutes"], json!(25.5));
        assert_eq!(rows[1]["ended_at_ms"], Value::Null);
        conn.execute("insert into focus_blocks(id) values(x'00')", [])
            .unwrap();
        assert_eq!(legacy_rows(&conn), None);
    }
}
