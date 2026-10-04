use chrono::{DateTime, FixedOffset};
use comandos_runtime::quick_terminal::{self as qt, CallbackError, Callbacks, Error, Options};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    sync::{Arc, Barrier, Mutex},
    time::Duration,
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(comandos_runtime::fresh_id("quick-fixture").unwrap());
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn db(&self) -> Connection {
        let c = comandos_store::state::connect(&self.0.join("state.sqlite3")).unwrap();
        c.busy_timeout(Duration::from_secs(3)).unwrap();
        comandos_store::state::migrate(&c, comandos_store::state::MIGRATIONS, 0.0).unwrap();
        c
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn at(s: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(s).unwrap()
}
fn now() -> DateTime<FixedOffset> {
    at("2026-09-29T16:11:12Z")
}
#[derive(Default)]
struct Shells {
    live: RefCell<Vec<String>>,
    calls: RefCell<Vec<String>>,
    failure: Cell<u8>,
}
impl Shells {
    fn exists(&self, s: &str) -> std::result::Result<bool, CallbackError> {
        self.calls.borrow_mut().push(format!("exists:{s}"));
        Ok(self.live.borrow().iter().any(|x| x == s))
    }
    fn launch(&self, s: &str, c: &str, p: &str) -> std::result::Result<(), CallbackError> {
        self.calls.borrow_mut().push(format!("launch:{s}:{c}:{p}"));
        let fail = self.failure.replace(0);
        if fail == 2 {
            self.live.borrow_mut().push(s.into());
        }
        if fail != 0 {
            return Err(CallbackError::new("timeout after creating the session"));
        }
        self.live.borrow_mut().push(s.into());
        Ok(())
    }
    fn register(&self, s: &str, l: &str, c: &str) -> std::result::Result<(), CallbackError> {
        self.calls
            .borrow_mut()
            .push(format!("register:{s}:{l}:{c}"));
        Ok(())
    }
    fn open(&self, c: &Connection, base: &Path, id: &Value) -> qt::Result<Value> {
        qt::open_quick_terminal(
            c,
            id,
            &Options::new(base, now()),
            &Callbacks {
                exists: &|s| self.exists(s),
                launch: &|s, c, p| self.launch(s, c, p),
                register: &|s, l, c| self.register(s, l, c),
                clock: &|| 100.0,
                sleep: &|_| panic!("unexpected wait"),
            },
        )
    }
}
fn quick(e: Error) -> qt::QuickTerminalError {
    match e {
        Error::Quick(e) => e,
        e => panic!("{e:?}"),
    }
}
fn count(c: &Connection) -> i64 {
    c.query_row("SELECT count(*) FROM quick_terminal_requests", [], |r| {
        r.get(0)
    })
    .unwrap()
}
#[test]
fn ascii_request_boundaries_and_exact_identity() {
    for good in ["a".to_string(), "a._:-Z09".into(), "x".repeat(128)] {
        assert!(qt::valid_request_id(&json!(good)));
    }
    for bad in [
        json!(null),
        json!(7),
        json!(true),
        json!(""),
        json!("x".repeat(129)),
        json!("a b"),
        json!("../x"),
        json!("_x"),
        json!("é"),
        json!("a\n"),
    ] {
        assert!(!qt::valid_request_id(&bad));
    }
    assert_eq!(
        qt::identity("req-1"),
        (
            "term-q9456bdfa12ea".into(),
            "pane-q9456bdfa12ea76959c94a357".into()
        )
    );
}
#[test]
fn approved_base_uses_only_supplied_home_and_nonempty_override() {
    assert_eq!(
        qt::default_base(Path::new("/fixture-home"), None),
        PathBuf::from("/fixture-home/codebase/0xJesus/Terminal")
    );
    assert_eq!(
        qt::default_base(Path::new("/fixture-home"), Some(Path::new(""))),
        PathBuf::from("/fixture-home/codebase/0xJesus/Terminal")
    );
    assert_eq!(
        qt::default_base(Path::new("/fixture-home"), Some(Path::new("relative"))),
        PathBuf::from("relative")
    );
}
#[test]
fn mexico_current_midnight_historical_dst_and_suffix_content() {
    let f = Fixture::new();
    for (stamp, name) in [
        ("2026-09-29T05:59:59Z", "T-2026-09-28-23-59-59"),
        ("2022-07-01T05:00:00Z", "T-2022-07-01-00-00-00"),
        ("2026-09-29T16:11:12Z", "T-2026-09-29-10-11-12"),
    ] {
        assert_eq!(
            qt::reserve_directory(&f.0, at(stamp))
                .unwrap()
                .file_name()
                .unwrap(),
            name
        );
    }
    let taken = f.0.join("T-2026-09-29-10-11-12");
    std::fs::write(taken.join("notes"), "keep").unwrap();
    assert_eq!(
        qt::reserve_directory(&f.0, now()).unwrap(),
        f.0.join("T-2026-09-29-10-11-12-2")
    );
    assert_eq!(
        std::fs::read_to_string(taken.join("notes")).unwrap(),
        "keep"
    );
}
#[test]
fn concurrent_atomic_reservations_are_unique() {
    let f = Fixture::new();
    let barrier = Arc::new(Barrier::new(12));
    std::thread::scope(|scope| {
        let mut jobs = vec![];
        for _ in 0..12 {
            let b = barrier.clone();
            let base = &f.0;
            jobs.push(scope.spawn(move || {
                b.wait();
                qt::reserve_directory(base, now()).unwrap()
            }));
        }
        let unique = jobs
            .into_iter()
            .map(|j| j.join().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(unique.len(), 12);
        assert!(unique.iter().all(|p| p.is_dir()));
    });
}
#[test]
fn launch_identity_is_durable_before_callback_and_ready_reopens() {
    let f = Fixture::new();
    let c = f.db();
    let calls = RefCell::new(vec![]);
    let clock = || 100.0;
    let callbacks = Callbacks {
        exists: &|s| {
            calls.borrow_mut().push(format!("exists:{s}"));
            Ok(false)
        },
        launch: &|s, cwd, p| {
            let reopen = Connection::open(f.0.join("state.sqlite3")).unwrap();
            let row: (String, String, String, String, f64) = reopen
                .query_row(
                    "SELECT state,cwd,session,pane_key,lease_until FROM quick_terminal_requests",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .unwrap();
            assert_eq!(
                row,
                ("launching".into(), cwd.into(), s.into(), p.into(), 130.0)
            );
            calls.borrow_mut().push("launch".into());
            Ok(())
        },
        register: &|_, l, cwd| {
            assert_eq!(l, "T-2026-09-29-10-11-12");
            assert!(Path::new(cwd).is_dir());
            calls.borrow_mut().push("register".into());
            Ok(())
        },
        clock: &clock,
        sleep: &|_| panic!(),
    };
    let out = qt::open_quick_terminal(&c, &json!("req-1"), &Options::new(&f.0, now()), &callbacks)
        .unwrap();
    assert_eq!(
        out,
        json!({"tabId":"term-q9456bdfa12ea","paneKey":"pane-q9456bdfa12ea76959c94a357","cwd":f.0.join("T-2026-09-29-10-11-12").to_str().unwrap(),"label":"T-2026-09-29-10-11-12","created":true})
    );
    assert_eq!(
        *calls.borrow(),
        vec!["exists:term-q9456bdfa12ea", "launch", "register"]
    );
    drop(c);
    let c = f.db();
    let shells = Shells::default();
    let again = shells.open(&c, &f.0, &json!("req-1")).unwrap();
    assert_eq!(again["created"], false);
    assert_eq!(again["cwd"], out["cwd"]);
    assert!(shells.calls.borrow().is_empty());
}
#[test]
fn invalid_ids_and_caller_transactions_have_no_effects() {
    let f = Fixture::new();
    let c = f.db();
    let shells = Shells::default();
    let base = f.0.join("unused");
    assert_eq!(
        quick(shells.open(&c, &base, &json!(7)).unwrap_err()).code,
        "request"
    );
    c.execute_batch("BEGIN; CREATE TABLE caller_work(x); INSERT INTO caller_work VALUES(1)")
        .unwrap();
    assert!(matches!(
        shells.open(&c, &base, &json!("valid")),
        Err(Error::CallerTransaction)
    ));
    assert!(!c.is_autocommit());
    assert_eq!(
        c.query_row("SELECT count(*) FROM caller_work", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(!base.exists());
    assert!(shells.calls.borrow().is_empty());
    assert_eq!(count(&c), 0);
    c.execute_batch("ROLLBACK").unwrap();
}
#[test]
fn folder_failure_is_retryable_and_stores_nothing() {
    let f = Fixture::new();
    let c = f.db();
    let shells = Shells::default();
    let base = f.0.join("file");
    std::fs::write(&base, "not a directory").unwrap();
    let e = quick(shells.open(&c, &base, &json!("request")).unwrap_err());
    assert_eq!(e.code, "folder");
    assert!(e.retryable && e.cwd.is_none());
    assert!(e.message.starts_with(&format!(
        "No se pudo crear la carpeta en {}: ",
        base.display()
    )));
    assert_eq!(count(&c), 0);
    assert!(c.is_autocommit());
    assert!(shells.calls.borrow().is_empty());
}
#[test]
fn launch_failure_reuses_folder_content_and_recreates_removed_folder() {
    for remove in [false, true] {
        let f = Fixture::new();
        let c = f.db();
        let shells = Shells::default();
        shells.failure.set(1);
        let e = quick(shells.open(&c, &f.0, &json!("one")).unwrap_err());
        assert_eq!(e.code, "launch");
        assert!(e.retryable);
        let cwd = e.cwd.unwrap();
        if remove {
            std::fs::remove_dir(&cwd).unwrap();
        } else {
            std::fs::write(Path::new(&cwd).join("draft"), "keep").unwrap();
        }
        let out = shells.open(&c, &f.0, &json!("one")).unwrap();
        assert_eq!(out["cwd"], cwd);
        assert_eq!(out["created"], true);
        assert!(Path::new(&cwd).is_dir());
        if !remove {
            assert_eq!(
                std::fs::read_to_string(Path::new(&cwd).join("draft")).unwrap(),
                "keep"
            );
        }
        assert_eq!(shells.live.borrow().len(), 1);
    }
}
#[test]
fn ambiguous_launch_failure_probes_exact_session_without_duplicate_launch() {
    let f = Fixture::new();
    let c = f.db();
    let shells = Shells::default();
    shells.failure.set(2);
    assert!(shells.open(&c, &f.0, &json!("one")).is_err());
    let out = shells.open(&c, &f.0, &json!("one")).unwrap();
    assert_eq!(
        shells
            .calls
            .borrow()
            .iter()
            .filter(|s| s.starts_with("launch:"))
            .count(),
        1
    );
    assert_eq!(shells.live.borrow()[0], out["tabId"]);
}
#[test]
fn registration_failure_truncates_unicode_storage_but_returns_full_message() {
    let f = Fixture::new();
    let c = f.db();
    let text = "🦀é".repeat(300);
    let live = Cell::new(false);
    let launches = Cell::new(0);
    let fail = Cell::new(true);
    let cb = Callbacks {
        exists: &|_| Ok(live.get()),
        launch: &|_, _, _| {
            live.set(true);
            launches.set(launches.get() + 1);
            Ok(())
        },
        register: &|_, _, _| {
            if fail.replace(false) {
                Err(CallbackError::new(&text))
            } else {
                Ok(())
            }
        },
        clock: &|| 100.0,
        sleep: &|_| panic!(),
    };
    let e = quick(
        qt::open_quick_terminal(&c, &json!("unicode"), &Options::new(&f.0, now()), &cb)
            .unwrap_err(),
    );
    assert_eq!(e.message, format!("No se pudo abrir la terminal: {text}"));
    let stored: String = c
        .query_row("SELECT error FROM quick_terminal_requests", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(stored, text.chars().take(500).collect::<String>());
    let out =
        qt::open_quick_terminal(&c, &json!("unicode"), &Options::new(&f.0, now()), &cb).unwrap();
    assert_eq!(out["created"], true);
    assert_eq!(launches.get(), 1);
}
#[test]
fn empty_callback_message_uses_exception_class() {
    let f = Fixture::new();
    let c = f.db();
    let cb = Callbacks {
        exists: &|_| {
            Err(CallbackError {
                message: String::new(),
                class_name: "ProbeError".into(),
            })
        },
        launch: &|_, _, _| panic!(),
        register: &|_, _, _| panic!(),
        clock: &|| 0.0,
        sleep: &|_| panic!(),
    };
    let e = quick(
        qt::open_quick_terminal(&c, &json!("one"), &Options::new(&f.0, now()), &cb).unwrap_err(),
    );
    assert_eq!(e.message, "No se pudo abrir la terminal: ProbeError");
}
fn seed(c: &Connection, id: &str, cwd: &Path, lease: f64) {
    c.execute("INSERT INTO quick_terminal_requests VALUES(?,'launching',?,'term-qs','pane-qs',NULL,?,0,0)",params![id,cwd.to_str().unwrap(),lease]).unwrap();
}
#[test]
fn waiter_uses_injected_poll_and_exact_deadline() {
    let f = Fixture::new();
    let c = f.db();
    seed(&c, "busy", &f.0.join("old"), 1000.0);
    let clock = Cell::new(100.0);
    let sleeps = Cell::new(0);
    let cb = Callbacks {
        exists: &|_| panic!(),
        launch: &|_, _, _| panic!(),
        register: &|_, _, _| panic!(),
        clock: &|| clock.get(),
        sleep: &|seconds| {
            assert_eq!(seconds, 0.05);
            sleeps.set(sleeps.get() + 1);
            clock.set(clock.get() + seconds);
        },
    };
    let mut options = Options::new(&f.0, now());
    options.wait = 0.1;
    let e = quick(qt::open_quick_terminal(&c, &json!("busy"), &options, &cb).unwrap_err());
    assert_eq!(e.code, "busy");
    assert_eq!(
        e.message,
        "La terminal se está abriendo; reintenta en unos segundos"
    );
    assert!(e.retryable && e.cwd.is_none());
    assert_eq!(sleeps.get(), 2);
    assert!(!f.0.join("old").exists());
}
#[test]
fn expired_lease_at_equality_is_taken_over_with_original_identity() {
    let f = Fixture::new();
    let c = f.db();
    let cwd = f.0.join("old");
    seed(&c, "stale", &cwd, 100.0);
    let shells = Shells::default();
    let out = shells.open(&c, &f.0, &json!("stale")).unwrap();
    assert_eq!(
        out,
        json!({"tabId":"term-qs","paneKey":"pane-qs","cwd":cwd.to_str().unwrap(),"label":"old","created":true})
    );
}
#[test]
fn sql_insert_failure_rolls_back_but_reports_reserved_directory() {
    let f = Fixture::new();
    let c = f.db();
    c.execute_batch("CREATE TRIGGER fail_claim BEFORE INSERT ON quick_terminal_requests BEGIN SELECT RAISE(ABORT,'injected claim'); END").unwrap();
    let shells = Shells::default();
    match shells.open(&c, &f.0, &json!("one")).unwrap_err() {
        Error::Sql {
            source,
            reserved_cwd: Some(cwd),
        } => {
            assert!(source.to_string().contains("injected claim"));
            assert!(cwd.is_dir());
        }
        e => panic!("{e:?}"),
    };
    assert_eq!(count(&c), 0);
    assert!(c.is_autocommit());
    assert!(shells.calls.borrow().is_empty());
}
#[test]
fn sql_finish_failure_keeps_launching_and_retry_probes_existing_shell() {
    let f = Fixture::new();
    let c = f.db();
    c.execute_batch("CREATE TRIGGER fail_finish BEFORE UPDATE OF state ON quick_terminal_requests WHEN NEW.state='ready' BEGIN SELECT RAISE(ABORT,'finish'); END").unwrap();
    let shells = Shells::default();
    assert!(matches!(
        shells.open(&c, &f.0, &json!("one")),
        Err(Error::Sql { .. })
    ));
    assert!(c.is_autocommit());
    assert_eq!(
        c.query_row("SELECT state FROM quick_terminal_requests", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "launching"
    );
    c.execute_batch("DROP TRIGGER fail_finish; UPDATE quick_terminal_requests SET lease_until=0")
        .unwrap();
    shells.open(&c, &f.0, &json!("one")).unwrap();
    assert_eq!(
        shells
            .calls
            .borrow()
            .iter()
            .filter(|s| s.starts_with("launch:"))
            .count(),
        1
    );
}
#[test]
fn clock_unwind_rolls_back_owned_claim_and_finish() {
    let f = Fixture::new();
    let c = f.db();
    let calls = Cell::new(0);
    let shells = Shells::default();
    let cb = Callbacks {
        exists: &|s| shells.exists(s),
        launch: &|s, c, p| shells.launch(s, c, p),
        register: &|s, l, c| shells.register(s, l, c),
        clock: &|| {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                panic!("claim clock")
            }
            0.0
        },
        sleep: &|_| panic!(),
    };
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| qt::open_quick_terminal(
            &c,
            &json!("one"),
            &Options::new(&f.0, now()),
            &cb
        )))
        .is_err()
    );
    assert!(c.is_autocommit());
    assert_eq!(count(&c), 0);
}
#[test]
fn concurrent_same_request_lease_launches_once() {
    let f = Fixture::new();
    drop(f.db());
    let barrier = Arc::new(Barrier::new(6));
    let live = Arc::new(Mutex::new(false));
    let launches = Arc::new(Mutex::new(0));
    std::thread::scope(|scope| {
        let mut jobs = vec![];
        for _ in 0..6 {
            let b = barrier.clone();
            let live = live.clone();
            let launches = launches.clone();
            let base = &f.0;
            jobs.push(scope.spawn(move || {
                let c = Connection::open(base.join("state.sqlite3")).unwrap();
                c.busy_timeout(Duration::from_secs(3)).unwrap();
                b.wait();
                qt::open_quick_terminal(
                    &c,
                    &json!("same"),
                    &Options::new(base, now()),
                    &Callbacks {
                        exists: &|_| Ok(*live.lock().unwrap()),
                        launch: &|_, _, _| {
                            std::thread::sleep(Duration::from_millis(100));
                            *launches.lock().unwrap() += 1;
                            *live.lock().unwrap() = true;
                            Ok(())
                        },
                        register: &|_, _, _| Ok(()),
                        clock: &|| 100.0,
                        sleep: &|s| std::thread::sleep(Duration::from_secs_f64(s)),
                    },
                )
                .unwrap()
            }));
        }
        let results = jobs
            .into_iter()
            .map(|j| j.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|r| r["created"] == true).count(), 1);
        assert!(results.iter().all(|r| r["cwd"] == results[0]["cwd"]));
    });
    assert_eq!(*launches.lock().unwrap(), 1);
}

#[test]
fn distinct_requests_reserve_separate_folders_in_supplied_base() {
    let f = Fixture::new();
    let c = f.db();
    let shells = Shells::default();
    let base = f.0.join("explicit-base");
    let first = shells.open(&c, &base, &json!("one")).unwrap();
    let second = shells.open(&c, &base, &json!("two")).unwrap();
    assert_ne!(first["tabId"], second["tabId"]);
    assert_ne!(first["paneKey"], second["paneKey"]);
    assert_eq!(
        Path::new(first["cwd"].as_str().unwrap()).parent(),
        Some(base.as_path())
    );
    assert_eq!(second["label"], "T-2026-09-29-10-11-12-2");
    assert_eq!(shells.live.borrow().len(), 2);
}

#[test]
fn retry_directory_failure_is_durable_and_never_probes_shell() {
    let f = Fixture::new();
    let c = f.db();
    let shells = Shells::default();
    shells.failure.set(1);
    let failed = quick(shells.open(&c, &f.0, &json!("one")).unwrap_err());
    let cwd = failed.cwd.unwrap();
    std::fs::remove_dir(&cwd).unwrap();
    std::fs::write(&cwd, "replaced by a file").unwrap();
    shells.calls.borrow_mut().clear();
    let err = quick(
        shells
            .open(&c, &f.0.join("different-base"), &json!("one"))
            .unwrap_err(),
    );
    assert_eq!(err.code, "launch");
    assert_eq!(err.cwd, Some(cwd.clone()));
    assert!(err.retryable);
    assert!(shells.calls.borrow().is_empty());
    assert_eq!(std::fs::read_to_string(&cwd).unwrap(), "replaced by a file");
    assert!(!f.0.join("different-base").exists());
    let durable = Connection::open(f.0.join("state.sqlite3")).unwrap();
    let row: (String, String) = durable
        .query_row("SELECT state,error FROM quick_terminal_requests", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(row.0, "failed");
    assert!(!row.1.is_empty());
}

#[test]
fn finish_clock_unwind_preserves_committed_claim_and_shell_for_retry() {
    let f = Fixture::new();
    let c = f.db();
    let shells = Shells::default();
    let calls = Cell::new(0);
    let cb = Callbacks {
        exists: &|s| shells.exists(s),
        launch: &|s, c, p| shells.launch(s, c, p),
        register: &|s, l, c| shells.register(s, l, c),
        clock: &|| {
            calls.set(calls.get() + 1);
            if calls.get() == 3 {
                panic!("finish clock")
            }
            100.0
        },
        sleep: &|_| panic!("unexpected wait"),
    };
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| qt::open_quick_terminal(
            &c,
            &json!("one"),
            &Options::new(&f.0, now()),
            &cb
        )))
        .is_err()
    );
    assert!(c.is_autocommit());
    assert_eq!(
        c.query_row("SELECT state FROM quick_terminal_requests", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "launching"
    );
    c.execute("UPDATE quick_terminal_requests SET lease_until=100", [])
        .unwrap();
    let out = shells.open(&c, &f.0, &json!("one")).unwrap();
    assert_eq!(out["created"], true);
    assert_eq!(
        shells
            .calls
            .borrow()
            .iter()
            .filter(|s| s.starts_with("launch:"))
            .count(),
        1
    );
}

#[test]
fn callback_unwind_preserves_lease_without_turning_into_failed_exception() {
    let f = Fixture::new();
    let c = f.db();
    let cb = Callbacks {
        exists: &|_| Ok(false),
        launch: &|_, _, _| panic!("callback unwind"),
        register: &|_, _, _| panic!("unreachable register"),
        clock: &|| 100.0,
        sleep: &|_| panic!("unexpected wait"),
    };
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| qt::open_quick_terminal(
            &c,
            &json!("one"),
            &Options::new(&f.0, now()),
            &cb
        )))
        .is_err()
    );
    assert!(c.is_autocommit());
    let row: (String, Option<String>) = c
        .query_row("SELECT state,error FROM quick_terminal_requests", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(row, ("launching".into(), None));
    let shells = Shells::default();
    c.execute("UPDATE quick_terminal_requests SET lease_until=100", [])
        .unwrap();
    assert!(shells.open(&c, &f.0, &json!("one")).is_ok());
}
