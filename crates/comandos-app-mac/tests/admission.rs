#![allow(clippy::unwrap_used)]
use comandos_app_mac::{
    app::{Action, App},
    jobs::{Backend, Jobs, ResultData, Task},
    tabs_ops::TabsOps,
};
use comandos_desktop::{Lang, proc::ProcOutput};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

struct Held {
    started: mpsc::Sender<()>,
    first: AtomicBool,
    release: AtomicBool,
    selected: Mutex<Vec<String>>,
    saved: Mutex<Vec<Value>>,
}
impl Backend for Held {
    fn get(&self, _: &str) -> Result<Value, String> {
        Ok(Value::Null)
    }
    fn post(&self, _: &str, _: &Value) -> Result<Value, String> {
        panic!("existing-session admission fixture must not POST")
    }
    fn cancel(&self) {}
    fn tmux(&self, args: &[&str], allowed: &dyn Fn() -> bool) -> Result<ProcOutput, String> {
        assert_eq!(args[0], "select-window");
        if !self.first.swap(true, Ordering::AcqRel) {
            self.started.send(()).unwrap();
            while !self.release.load(Ordering::Acquire) && allowed() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        if allowed() {
            self.selected.lock().unwrap().push(args[2].into());
        }
        Ok(ProcOutput {
            code: Some(0),
            ..Default::default()
        })
    }
    fn save_tabs(&self, value: &Value, allowed: &dyn Fn() -> bool) -> Result<(), String> {
        if allowed() {
            self.saved.lock().unwrap().push(value.clone());
        }
        Ok(())
    }
}
fn held() -> (Arc<Held>, mpsc::Receiver<()>) {
    let (started, receiver) = mpsc::channel();
    (
        Arc::new(Held {
            started,
            first: AtomicBool::new(false),
            release: AtomicBool::new(false),
            selected: Mutex::new(vec![]),
            saved: Mutex::new(vec![]),
        }),
        receiver,
    )
}
fn open(session: String) -> Task {
    Task::Open {
        action: Action::Open {
            session,
            win: "claude".into(),
            label: None,
        },
        existing: true,
    }
}
fn save(ops: &mut TabsOps, version: u32) -> Task {
    Task::Scoped {
        key: "\0save".into(),
        operation: ops.start_operation("\0save"),
        work: Box::new(Task::Save {
            value: json!({"version":version}),
        }),
    }
}

#[test]
fn restore_burst_and_latest_save_survive_worker_backpressure() {
    let (backend, started) = held();
    let mut jobs = Jobs::new(backend.clone(), Arc::new(|| {})).unwrap();
    let mut ops = TabsOps::new(App::new("noche", Lang::En));
    let ticket = ops.ticket();
    ops.begin_restore(vec![]);
    ops.finish_boot(&ticket, "owned");
    for index in 0..128 {
        assert!(
            ops.queue(Action::Open {
                session: format!("owned-{index}"),
                win: "claude".into(),
                label: None,
            })
            .unwrap()
            .is_empty()
        );
    }
    jobs.enqueue(ticket.clone(), open("hold".into())).unwrap();
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    for action in ops.finish_restore(&ticket) {
        jobs.enqueue(
            ticket.clone(),
            Task::Open {
                action,
                existing: true,
            },
        )
        .unwrap();
    }
    jobs.enqueue(ticket.clone(), save(&mut ops, 1)).unwrap();
    jobs.enqueue(ticket.clone(), save(&mut ops, 2)).unwrap();
    backend.release.store(true, Ordering::Release);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut opened = 0;
    let mut saved = 0;
    while (opened != 129 || saved != 1) && Instant::now() < deadline {
        for delivery in jobs.drain() {
            match delivery.result.unwrap() {
                ResultData::Opened(_) => opened += 1,
                ResultData::Scoped { result, .. } if matches!(*result, Ok(ResultData::Saved)) => {
                    saved += 1
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    jobs.close();
    assert_eq!((opened, saved), (129, 1));
    let selected = backend.selected.lock().unwrap();
    assert_eq!(selected.len(), 129);
    for index in 0..128 {
        assert!(selected.contains(&format!("=owned-{index}:claude")));
    }
    assert_eq!(*backend.saved.lock().unwrap(), vec![json!({"version":2})]);
}

#[test]
fn saturated_admission_stays_bounded_and_shutdown_never_replays_it() {
    let (backend, started) = held();
    let mut jobs = Jobs::new(backend.clone(), Arc::new(|| {})).unwrap();
    let app = App::new("noche", Lang::En);
    jobs.enqueue(app.ticket(), open("hold".into())).unwrap();
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let accepted = (0..1024)
        .filter(|index| {
            jobs.enqueue(app.ticket(), open(format!("owned-{index}")))
                .is_ok()
        })
        .count();
    assert_eq!(
        accepted, 136,
        "8 worker slots plus 128 bounded deferred actions"
    );
    let now = Instant::now();
    jobs.close();
    assert!(now.elapsed() < Duration::from_secs(1));
    assert!(backend.selected.lock().unwrap().is_empty());
    assert!(jobs.drain().is_empty());
    assert!(jobs.enqueue(app.ticket(), open("closed".into())).is_err());
}
