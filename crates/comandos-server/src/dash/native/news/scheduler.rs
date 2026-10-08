//! Edition loop: a registered system thread, only when Front owns background work.
use super::{agents, read::read_config_domain};
use crate::dash::native::{Native, background::front_owns, usage::pane_models::notice_emit};
use comandos_runtime::{
    news_agents::{self, Opener},
    news_editions::{self as editions, EditionScheduler, Fetcher, LeadWriter, Summarizer},
    news_radar,
};
use serde_json::Value;
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Default, Clone)]
pub struct Inputs {
    pub fetch: Option<Arc<Fetcher>>,
    pub summarize: Option<Arc<Summarizer>>,
    pub lead: Option<Arc<LeadWriter>>,
    pub opener: Option<Opener>,
}
#[derive(Default)]
struct Stop {
    stopped: Mutex<bool>,
    wake: Condvar,
}
impl Stop {
    fn wait(&self, duration: Duration) -> bool {
        let stopped = self.stopped.lock().unwrap_or_else(|e| e.into_inner());
        let (stopped, _) = self
            .wake
            .wait_timeout_while(stopped, duration, |v| !*v)
            .unwrap_or_else(|e| e.into_inner());
        *stopped
    }
}
struct Completion(Arc<AtomicBool>);
impl Drop for Completion {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
pub struct Runner {
    stop: Arc<Stop>,
    finished: Arc<AtomicBool>,
}
impl Runner {
    pub fn stop(&self) {
        *self.stop.stopped.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.stop.wake.notify_all()
    }
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}
impl Drop for Runner {
    fn drop(&mut self) {
        self.stop()
    }
}
pub fn start(native: &Arc<Native>) -> Option<Runner> {
    start_with(
        native,
        Inputs::default(),
        Duration::from_secs(120),
        Duration::from_secs(60),
    )
}
pub fn start_with(
    native: &Arc<Native>,
    inputs: Inputs,
    first: Duration,
    period: Duration,
) -> Option<Runner> {
    let handle = tokio::runtime::Handle::try_current().ok()?;
    if super::super::cut_is_off(&native.options().cuts_off, super::super::Cut::News)
        || !native.enabled()
        || !front_owns(&native.options().background)
        || native.news_scheduler_started.swap(true, Ordering::AcqRel)
    {
        return None;
    }
    let stop = Arc::new(Stop::default());
    let finished = Arc::new(AtomicBool::new(false));
    let weak = Arc::downgrade(native);
    let (done, rx) = tokio::sync::oneshot::channel();
    if native
        .tasks()
        .spawn(async move {
            let _ = rx.await;
        })
        .is_err()
    {
        native
            .news_scheduler_started
            .store(false, Ordering::Release);
        return None;
    }
    let thread_stop = Arc::clone(&stop);
    let thread_finished = Arc::clone(&finished);
    let spawn = std::thread::Builder::new()
        .name("comandos-news-editions".into())
        .spawn(move || {
            let _completion = Completion(thread_finished);
            let scheduler = EditionScheduler::default();
            let boot = (|| {
                let native = weak
                    .upgrade()
                    .filter(|n| n.enabled())
                    .ok_or("frente detenido")?;
                let conn = comandos_runtime::open_state(&native.options().state_db, 5000)
                    .map_err(|e| e.to_string())?;
                editions::recover_reading(&conn, (native.options().clock)())
            })();
            if let Err(e) = boot {
                eprintln!("comandos dash news editions: {e}");
                return;
            }
            let mut delay = first;
            loop {
                // Wake every 250ms to notice Native teardown without requiring an owner to stop.
                let until = Instant::now() + delay;
                let mut stopped = false;
                while Instant::now() < until {
                    if thread_stop.wait(
                        until
                            .saturating_duration_since(Instant::now())
                            .min(Duration::from_millis(250)),
                    ) || weak.upgrade().is_none_or(|n| !n.enabled())
                    {
                        stopped = true;
                        break;
                    }
                }
                if stopped || thread_stop.wait(Duration::ZERO) {
                    break;
                }
                let Some(native) = weak.upgrade().filter(|n| n.enabled()) else {
                    break;
                };
                if let Err(e) = tick(&native, &scheduler, &inputs, &handle) {
                    eprintln!("comandos dash news editions: {e}")
                }
                drop(native);
                delay = period;
            }
            let _ = done.send(());
        });
    if spawn.is_err() {
        native
            .news_scheduler_started
            .store(false, Ordering::Release);
        return None;
    }
    Some(Runner { stop, finished })
}
fn tick(
    native: &Arc<Native>,
    scheduler: &EditionScheduler,
    inputs: &Inputs,
    handle: &tokio::runtime::Handle,
) -> Result<Value, String> {
    let opts = native.options();
    let config = read_config_domain(opts).map_err(|e| e.to_string())?;
    let env = |key: &str| -> Option<String> {
        if let Some(env) = &opts.child_env {
            env.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.to_string_lossy().into_owned())
        } else {
            std::env::var(key).ok()
        }
    };
    if comandos_store::news::config_status(config.as_ref(), &|k| {
        env(k).is_some_and(|s| !s.is_empty())
    })["configured"]
        != true
    {
        return Ok(serde_json::json!({"built":null}));
    }
    let value = config.clone().map(Value::Object).unwrap_or(Value::Null);
    let opener = inputs.opener.clone().unwrap_or_else(|| {
        opts.repo_root
            .as_ref()
            .map(|repo| agents::opener(opts, repo))
            .unwrap_or_else(|| Arc::new(|_, _| Err("checkout heredado no disponible".into())))
    });
    let summarize = match &inputs.summarize {
        Some(s) => Arc::clone(s),
        None => editions::make_summarizer(
            &value,
            &env,
            Arc::new(editions::HttpPost),
            Arc::clone(&opener),
        )?,
    };
    let lead: Option<Arc<LeadWriter>> = inputs.lead.clone().or_else(|| {
        if inputs.summarize.is_none() {
            news_agents::make_asker(&value, opener)
                .ok()
                .map(|a| Arc::new(news_agents::make_lead_writer(a)) as Arc<LeadWriter>)
        } else {
            None
        }
    });
    let fetch = inputs.fetch.clone().unwrap_or_else(|| {
        let network = news_radar::Network::production();
        let net = network.clone();
        let token = env("X_BEARER_TOKEN");
        let collect = Arc::new(move |now| Ok(news_radar::collect(now, &net, token.as_deref())));
        let db = opts.state_db.clone();
        let clock = Arc::clone(&opts.clock);
        let recent = Arc::new(move || {
            let conn = comandos_runtime::open_state(&db, 5000).map_err(|e| e.to_string())?;
            editions::recent_story_urls(&conn, clock() - 48 * 3600 * 1000)
        });
        let clock = Arc::clone(&opts.clock);
        editions::make_radar_fetcher(
            collect,
            network,
            editions::media_dir(opts.xdg_state_home.as_deref(), &opts.home),
            Arc::new(move || clock() / 1000),
            recent,
            5,
        )
    });
    let n = Arc::clone(native);
    let handle = handle.clone();
    let notify = move |e: &Value| {
        let id = e["id"].as_str().unwrap_or("");
        if !id.is_empty() {
            let (title, body) = editions::notice_text(e);
            handle.block_on(notice_emit(
                &n,
                "news_edition",
                &title,
                &body,
                None,
                Some(&format!("news-edition:{id}")),
            ));
        }
        Ok(())
    };
    scheduler.tick(
        &|| comandos_runtime::open_state(&opts.state_db, 5000).map_err(|e| e.to_string()),
        config.as_ref(),
        &|k| env(k).is_some_and(|s| !s.is_empty()),
        (opts.clock)(),
        fetch.as_ref(),
        summarize.as_ref(),
        Some(&notify),
        lead.as_deref(),
    )
}
