#![allow(clippy::unwrap_used, clippy::expect_used)]
use comandos_app_mac::{
    app::{App, AppConfig, RunMode},
    jobs::{Backend, Jobs, SystemBackend, Task},
};
use comandos_desktop::{Lang, proc::ProcOutput};
use serde_json::{Value, json};
use std::{
    io::Read,
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
#[test]
fn error_payloads_never_publish_fake_credentials() {
    use comandos_app_mac::jobs::Backend;
    use std::io::Write;
    let listener = TcpListener::bind("127.0.0.1:7347").unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut buf = [0; 4096];
        let n = socket.read(&mut buf).unwrap();
        assert!(buf[..n].starts_with(b"GET /prefs"));
        let body = r#"{"error":"fake_secret_needle","token":"owned_fake_token"}"#;
        write!(
            socket,
            "HTTP/1.0 500 Error\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let cfg = AppConfig {
        mode: RunMode::Sandbox {
            tmux_socket: std::env::temp_dir().join("never-used"),
            hooks: std::env::temp_dir(),
        },
        dash_url: "http://127.0.0.1:7347".into(),
        dump_dom: None,
    };
    let service = SystemBackend::new(&cfg).unwrap();
    let error = service.get("/prefs").unwrap_err();
    assert_eq!(error, "HTTP 500");
    assert!(!error.contains("fake_secret"));
    server.join().unwrap();
}
#[test]
fn close_interrupts_a_real_owned_http_read_and_joins_worker() {
    let listener = TcpListener::bind("127.0.0.1:7346").unwrap();
    let (tx, rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut buf = [0; 4096];
        let n = socket.read(&mut buf).unwrap();
        assert!(buf[..n].starts_with(b"GET /webterm-token"));
        tx.send(()).unwrap();
        let _ = socket.read(&mut buf);
    });
    let home = std::env::temp_dir();
    let cfg = AppConfig {
        mode: RunMode::Sandbox {
            tmux_socket: home.join("never-used-owned-socket"),
            hooks: home.join("never-read-hooks"),
        },
        dash_url: "http://127.0.0.1:7346".into(),
        dump_dom: None,
    };
    let backend = Arc::new(SystemBackend::new(&cfg).unwrap());
    let mut jobs = Jobs::new(backend, Arc::new(|| {})).unwrap();
    let app = App::new("noche", Lang::Es);
    jobs.submit(
        app.ticket(),
        Task::Boot {
            hooks: home.clone(),
            home,
        },
    )
    .unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let start = Instant::now();
    jobs.close();
    assert!(start.elapsed() < Duration::from_secs(1));
    server.join().unwrap();
    assert!(jobs.drain().is_empty());
}
struct Blocking {
    started: mpsc::Sender<()>,
    cancelled: AtomicBool,
}
impl Backend for Blocking {
    fn get(&self, path: &str) -> Result<Value, String> {
        Ok(if path == "/webterm-token" {
            json!({"token":"owned token"})
        } else {
            json!({})
        })
    }
    fn post(&self, _: &str, _: &Value) -> Result<Value, String> {
        Ok(json!({}))
    }
    fn tmux(&self, _: &[&str], allowed: &dyn Fn() -> bool) -> Result<ProcOutput, String> {
        self.started.send(()).unwrap();
        while allowed() {
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(ProcOutput {
            code: None,
            ..Default::default()
        })
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
#[test]
fn job_scope_cancellation_stops_active_tmux_even_without_destroying_model() {
    let (tx, rx) = mpsc::channel();
    let backend = Arc::new(Blocking {
        started: tx,
        cancelled: AtomicBool::new(false),
    });
    let mut jobs = Jobs::new(backend.clone(), Arc::new(|| {})).unwrap();
    let app = App::new("noche", Lang::Es);
    let home = std::env::temp_dir();
    jobs.submit(
        app.ticket(),
        Task::Boot {
            hooks: home.clone(),
            home,
        },
    )
    .unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let start = Instant::now();
    jobs.close();
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(app.ticket().current());
    assert!(backend.cancelled.load(Ordering::Acquire));
    assert!(jobs.drain().is_empty());
}
#[test]
fn backlog_is_bounded_and_destroyed_actions_never_replay() {
    let (tx, rx) = mpsc::channel();
    let backend = Arc::new(Blocking {
        started: tx,
        cancelled: AtomicBool::new(false),
    });
    let mut jobs = Jobs::new(backend, Arc::new(|| {})).unwrap();
    let mut app = App::new("noche", Lang::Es);
    let task = || Task::Boot {
        hooks: std::env::temp_dir(),
        home: std::env::temp_dir(),
    };
    jobs.submit(app.ticket(), task()).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let count = (0..64)
        .filter(|_| jobs.submit(app.ticket(), task()).is_ok())
        .count();
    assert_eq!(count, 8);
    app.shutdown();
    jobs.close();
    assert!(jobs.drain().is_empty());
    assert!(jobs.submit(app.ticket(), task()).is_err());
}

struct Unauthenticated {
    calls: std::sync::Mutex<Vec<String>>,
}
impl Backend for Unauthenticated {
    fn get(&self, path: &str) -> Result<Value, String> {
        self.calls.lock().unwrap().push(path.into());
        Ok(match path {
            "/prefs" => json!({"theme":"bruno"}),
            "/conf" => json!({"_lang":"en"}),
            _ => json!({}),
        })
    }
    fn post(&self, _: &str, _: &Value) -> Result<Value, String> {
        panic!("unauthenticated boot must not POST");
    }
    fn tmux(&self, _: &[&str], _: &dyn Fn() -> bool) -> Result<ProcOutput, String> {
        panic!("unauthenticated boot must not access tmux");
    }
    fn cancel(&self) {}
}
#[test]
fn startup_preferences_arrive_even_when_terminal_authentication_fails() {
    use comandos_app_mac::jobs::ResultData;
    let (notify, notified) = mpsc::channel();
    let backend = Arc::new(Unauthenticated {
        calls: std::sync::Mutex::new(vec![]),
    });
    let mut jobs = Jobs::new(
        backend.clone(),
        Arc::new(move || {
            let _ = notify.send(());
        }),
    )
    .unwrap();
    let app = App::new("noche", Lang::Es);
    jobs.submit(
        app.ticket(),
        Task::Preferences {
            env_lang: "es".into(),
        },
    )
    .unwrap();
    jobs.submit(
        app.ticket(),
        Task::Boot {
            hooks: std::env::temp_dir().join("missing-owned-auth"),
            home: std::env::temp_dir(),
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut results = vec![];
    while results.len() < 2 {
        notified
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        results.extend(jobs.drain());
    }
    assert!(
        matches!(&results[0].result, Ok(ResultData::Preferences { theme, lang:Lang::En }) if theme=="bruno")
    );
    assert!(results[1].result.is_err());
    assert_eq!(
        *backend.calls.lock().unwrap(),
        vec!["/prefs", "/conf", "/webterm-token"]
    );
    assert!(app.token().is_empty());
    jobs.close();
}

struct Fast;
impl Backend for Fast {
    fn get(&self, _: &str) -> Result<Value, String> {
        Ok(json!({}))
    }
    fn post(&self, _: &str, _: &Value) -> Result<Value, String> {
        panic!("preferences cannot POST")
    }
    fn tmux(&self, _: &[&str], _: &dyn Fn() -> bool) -> Result<ProcOutput, String> {
        panic!("preferences cannot invoke tmux")
    }
    fn cancel(&self) {}
}
#[test]
fn queued_actions_are_not_lost_when_main_temporarily_stalls_delivery() {
    use comandos_app_mac::jobs::ResultData;
    let mut jobs = Jobs::new(Arc::new(Fast), Arc::new(|| {})).unwrap();
    let app = App::new("noche", Lang::Es);
    let deadline = Instant::now() + Duration::from_secs(2);
    for _ in 0..16 {
        while jobs
            .submit(
                app.ticket(),
                Task::Preferences {
                    env_lang: "es".into(),
                },
            )
            .is_err()
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    std::thread::sleep(Duration::from_millis(20));
    let mut delivered = 0;
    while delivered < 16 && Instant::now() < deadline {
        delivered += jobs
            .drain()
            .into_iter()
            .filter(|d| matches!(d.result, Ok(ResultData::Preferences { .. })))
            .count();
        std::thread::sleep(Duration::from_millis(1));
    }
    jobs.close();
    assert_eq!(delivered, 16);
}
