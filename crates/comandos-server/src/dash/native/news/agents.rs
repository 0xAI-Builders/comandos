//! Chat y traducción de los Resúmenes con agentes ACP (plan 2f-4, Tarea 3):
//! las ramas POST `/news/chat` y `/news/translate` de `news_post` (906) de
//! `bin/cc-dash`, con `_news_asker` (848) y `_news_agent_job` (853).
//!
//! Orden del Python: el id del cuerpo (`_news_int`), después la cadena de
//! agentes (`make_asker` sobre `news-editions.json`; sin cadena → `503` con su
//! texto y SIN escribir nada), después `start_chat`/`start_translation` en el
//! worker de app-state (sus `LookupError`/`ValueError`/`RuntimeError` →
//! 404/400/409) y por último el trabajo de agente.
//!
//! El trabajo de agente corre en un hilo de sistema (`comandos-news-agent`,
//! nunca en el runtime) que espera a uno de los DOS puestos del proceso (el
//! `_NEWS_AGENTS = Semaphore(2)` del Python), abre su propia conexión a
//! app-state (`connect` + `migrate`) y habla con el agente por el cliente ACP
//! síncrono de `comandos_runtime`. Un trabajo vivo se registra en
//! `Native::tasks()` para que cuente como tarea larga del frente. Lo que
//! falle en el hilo se escribe en stderr con el prefijo
//! `comandos dash news agent:` (el `cc-dash news agent:` del Python).
//!
//! Diferencias deliberadas con el Python:
//! - La cadena se construye UNA vez por petición (el Python la vuelve a leer
//!   dentro de `_news_agent_job`, ya con la pendiente escrita).
//! - El historial de la respuesta del chat se lee en el mismo trabajo del
//!   worker que crea la pendiente, antes de lanzar el agente: la respuesta
//!   muestra siempre la pendiente (en el Python depende de la carrera con el
//!   hilo del agente).
//! - Lo que el Python solo rechazaría con un `TypeError` al preguntar (pasos
//!   que no son objetos, `agent` o `model` no textuales) y la falta del
//!   checkout del heredado (`repo_root`, de donde sale `providers.json`)
//!   declinan antes de escribir.
use super::{
    NewsRoute,
    read::{news_int, read_config, trace_decline},
};
use crate::{
    HandlerError, Request,
    dash::native::{
        Answer, Fault, Native, NativeOptions,
        light::{data, error},
        reply,
    },
};
use comandos_runtime::news_agents::{self, AcpEnv, Asker, AskerError, Opener};
use comandos_store::news::{self, NewsError};
use http::StatusCode;
use serde_json::{Map, Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, MutexGuard},
};

/// Trabajos de agente a la vez en todo el proceso (`Semaphore(2)`).
const MAX_AGENT_JOBS: usize = 2;
/// `BUSY_TIMEOUT_MS` de `app_state.connect`.
const BUSY_MS: u64 = 5000;

/// Los puestos de trabajo de agente: un contador con su `Condvar`.
struct Slots {
    busy: Mutex<usize>,
    freed: Condvar,
}

static AGENT_SLOTS: Slots = Slots {
    busy: Mutex::new(0),
    freed: Condvar::new(),
};

/// Un puesto ocupado; al soltarse lo devuelve (también si el trabajo revienta).
struct Slot(&'static Slots);

impl Slots {
    fn lock(&self) -> MutexGuard<'_, usize> {
        self.busy.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `with _NEWS_AGENTS:` — bloquea el hilo de sistema hasta que haya puesto.
    fn acquire(&'static self) -> Slot {
        let mut busy = self.lock();
        while *busy >= MAX_AGENT_JOBS {
            busy = self.freed.wait(busy).unwrap_or_else(|e| e.into_inner());
        }
        *busy += 1;
        Slot(self)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut busy = self.0.lock();
        *busy = busy.saturating_sub(1);
        drop(busy);
        self.0.freed.notify_one();
    }
}

pub async fn answer(native: &Arc<Native>, route: NewsRoute, request: &Request) -> Answer {
    let d = data(request)?.clone();
    let task_native = Arc::clone(native);
    // La petición entera en su propia tarea: si el cliente se va, la escritura
    // y el lanzamiento del trabajo terminan igual (como el hilo del Python).
    let job = tokio::spawn(async move { post(&task_native, route, &d).await });
    job.await.map_err(|_| failure())?
}

fn failure() -> Fault {
    Fault::Error(HandlerError::Failure)
}

/// `_news_int(value)` sobre un valor del cuerpo (la misma regla que las
/// escrituras de la Tarea 2): texto → `int(str(…))`; número → solo el texto
/// de un entero; lo demás `None`.
fn news_int_value(value: Option<&Value>) -> Result<Option<i64>, Fault> {
    match value {
        Some(Value::String(s)) => news_int(s),
        Some(Value::Number(n)) => Ok(n.as_i64().filter(|n| 0 < *n && *n < (1 << 53))),
        _ => Ok(None),
    }
}

/// Lo que hace el hilo del trabajo.
enum Work {
    Chat { story: i64, pending: i64 },
    Translate { source: i64 },
}

/// `_default_acp_open` con lo que el frente tomó al arrancar: el registro del
/// checkout del heredado, `XDG_STATE_HOME/comandos/news-acp`, el `PATH` de
/// `which` y el entorno de los hijos.
fn opener(opts: &NativeOptions, repo: &Path) -> Opener {
    let state = opts
        .xdg_state_home
        .clone()
        .unwrap_or_else(|| opts.home.join(".local/state"));
    news_agents::default_opener(AcpEnv {
        providers_json: repo.join("config/providers.json"),
        cwd: state.join("comandos/news-acp"),
        search_path: opts.search_path.clone(),
        home: opts.home.clone(),
        base_env: opts.child_env.clone(),
    })
}

async fn post(native: &Arc<Native>, route: NewsRoute, d: &Map<String, Value>) -> Answer {
    let field = match route {
        NewsRoute::ChatPost => "storyId",
        NewsRoute::TranslatePost => "sourceId",
        _ => return Err(Fault::Decline),
    };
    let id = news_int_value(d.get(field))?;
    let opts = native.options();
    let Some(repo) = opts.repo_root.clone() else {
        return Err(Fault::Decline);
    };
    // `_news_asker()`: la configuración en el pool de bloqueo (nunca en el
    // worker de la base) y la cadena antes de escribir nada.
    let path = opts.hooks.join("news-editions.json");
    let config = tokio::task::spawn_blocking(move || read_config(&path))
        .await
        .map_err(|_| Fault::Decline)?;
    let config = match config {
        Ok(config) => config.map_or(Value::Null, Value::Object),
        Err(fault) => {
            trace_decline(&fault);
            return Err(Fault::Decline);
        }
    };
    let asker = match news_agents::make_asker(&config, opener(opts, &repo)) {
        Ok(asker) => asker,
        Err(AskerError::Runtime(message)) => {
            return error(StatusCode::SERVICE_UNAVAILABLE, &message);
        }
        Err(AskerError::Unsure(what)) => {
            eprintln!("comandos dash news: se reenvía al heredado ({what})");
            return Err(Fault::Decline);
        }
    };
    let now = (opts.clock)();
    match route {
        NewsRoute::ChatPost => {
            let message = d.get("message").cloned().unwrap_or(Value::Null);
            let started = native
                .with_state(move |backend| start_chat(&backend.conn, id, &message, now))
                .await?;
            let (story, pending, history) = match started {
                Ok(started) => started,
                Err(e) => return respond_error(e),
            };
            if let Err(message) = spawn_job(native, Work::Chat { story, pending }, asker) {
                return error(StatusCode::CONFLICT, &message);
            }
            match history {
                Ok(messages) => reply(StatusCode::OK, &json!({"messages": messages})),
                Err(fault) => {
                    eprintln!("comandos dash news: fallo tras escribir ({fault}); 500");
                    Err(failure())
                }
            }
        }
        _ => {
            let started = native
                .with_state(move |backend| news::start_translation(&backend.conn, id, now))
                .await?;
            let (start, state) = match started {
                Ok(started) => started,
                Err(e) => return respond_error(e),
            };
            if let (true, Some(source)) = (start, id)
                && let Err(message) = spawn_job(native, Work::Translate { source }, asker)
            {
                return error(StatusCode::CONFLICT, &message);
            }
            reply(StatusCode::OK, &json!({"translation": state}))
        }
    }
}

/// `start_chat` y el historial que devuelve la ruta, en un solo trabajo del
/// worker: `(noticia, pendiente, historial)`.
type ChatStart = (i64, i64, news::Result<Vec<Value>>);

fn start_chat(
    conn: &rusqlite::Connection,
    story: Option<i64>,
    message: &Value,
    now: i64,
) -> Result<ChatStart, NewsError> {
    let pending = news::start_chat(conn, story, message, now)?;
    // `start_chat` sin noticia ya dio `LookupError`.
    let Some(story) = story else {
        return Err(NewsError::Lookup("noticia no encontrada".into()));
    };
    Ok((story, pending, news::chat_history(conn, story)))
}

/// `_news_agent_job(work)`: el hilo de sistema con su puesto y su conexión.
/// `Err` es el `RuntimeError` de `threading.Thread.start` (409 del Python).
fn spawn_job(native: &Arc<Native>, work: Work, asker: Asker) -> Result<(), String> {
    let opts = native.options();
    let db: PathBuf = opts.state_db.clone();
    let clock = Arc::clone(&opts.clock);
    let (done, finished) = tokio::sync::oneshot::channel::<()>();
    std::thread::Builder::new()
        .name("comandos-news-agent".into())
        .spawn(move || {
            let _slot = AGENT_SLOTS.acquire();
            if let Err(message) = run_work(&db, &work, &asker, &|| clock()) {
                eprintln!("comandos dash news agent: {message}");
            }
            let _ = done.send(());
        })
        .map_err(|_| "can't start new thread".to_owned())?;
    // El trabajo cuenta como tarea larga del frente mientras viva.
    let _ = native.tasks().spawn(async move {
        let _ = finished.await;
    });
    Ok(())
}

fn run_work(db: &Path, work: &Work, asker: &Asker, now: &dyn Fn() -> i64) -> Result<(), String> {
    let conn = comandos_runtime::open_state(db, BUSY_MS).map_err(|e| format!("{e:?}"))?;
    let done = match work {
        Work::Chat { story, pending } => news_agents::run_chat(&conn, *story, *pending, asker),
        Work::Translate { source } => news_agents::run_translation(&conn, *source, asker, now),
    };
    done.map_err(|e| format!("{e:?}"))
}

/// Las excepciones que captura `news_post`; lo dudoso antes de escribir
/// declina y lo que falla con la escritura ya hecha es el 500 del Python.
fn respond_error(failed: NewsError) -> Answer {
    match failed {
        NewsError::Lookup(message) => error(StatusCode::NOT_FOUND, message.trim_matches('\'')),
        NewsError::Value(message) => error(StatusCode::BAD_REQUEST, &message),
        NewsError::Runtime(message) => error(StatusCode::CONFLICT, &message),
        NewsError::Fault(news::Fault::Raise(_)) => Err(failure()),
        NewsError::Fault(fault) => {
            trace_decline(&fault);
            Err(Fault::Decline)
        }
        NewsError::AfterWrite(fault) => {
            eprintln!("comandos dash news: fallo tras escribir ({fault}); 500");
            Err(failure())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn slots_never_exceed_two() {
        let live = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..6)
            .map(|_| {
                let (live, most) = (Arc::clone(&live), Arc::clone(&most));
                std::thread::spawn(move || {
                    let _slot = AGENT_SLOTS.acquire();
                    let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    live.fetch_sub(1, Ordering::SeqCst);
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert!(most.load(Ordering::SeqCst) <= MAX_AGENT_JOBS);
        assert_eq!(*AGENT_SLOTS.lock(), 0);
    }

    #[test]
    fn body_ids_follow_news_int() {
        let got = |text: &str| news_int_value(Some(&serde_json::from_str(text).unwrap())).ok();
        assert_eq!(got("10"), Some(Some(10)));
        assert_eq!(got("\"10\""), Some(Some(10)));
        assert_eq!(got("10.0"), Some(None));
        assert_eq!(got("true"), Some(None));
    }
}
