#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
use comandos_app::jobs::Jobs;
use comandos_app::proc::{ProcSpec, run};
use std::time::{Duration, Instant};

fn spec(program: &str, args: &[&str], timeout_ms: u64) -> ProcSpec {
    ProcSpec {
        program: program.into(),
        args: args.iter().map(Into::into).collect(),
        stdin: None,
        env: Vec::new(),
        clear_env: false,
        env_remove: Vec::new(),
        cwd: None,
        timeout: Duration::from_millis(timeout_ms),
    }
}

#[test]
fn stdout_and_stderr_over_64k_do_not_deadlock() {
    // 1 MiB a stderr ANTES de escribir stdout: con lecturas en serie se bloquea.
    let s = spec(
        "sh",
        &[
            "-c",
            "head -c 1048576 /dev/zero >&2; head -c 1048576 /dev/zero",
        ],
        5000,
    );
    let out = run(&s).unwrap();
    assert_eq!((out.stdout.len(), out.stderr.len()), (1 << 20, 1 << 20));
    assert_eq!(out.code, Some(0));
}

#[test]
fn timeout_kills_the_child() {
    let started = Instant::now();
    let out = run(&spec("sleep", &["30"], 300)).unwrap();
    assert!(out.timed_out && out.code.is_none());
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn stdin_is_fed_concurrently() {
    let mut s = spec("cat", &[], 5000);
    s.stdin = Some(vec![b'y'; 1 << 20]);
    assert_eq!(run(&s).unwrap().stdout.len(), 1 << 20);
}

#[test]
fn jobs_deliver_on_the_main_context_two_at_a_time() {
    let ctx = glib::MainContext::new();
    let _guard = ctx.acquire().unwrap();
    let main_loop = glib::MainLoop::new(Some(&ctx), false);
    ctx.with_thread_default(|| {
        let jobs = Jobs::new(2, "prueba");
        let main_thread = std::thread::current().id();
        let done = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let started = Instant::now();
        for i in 0..4 {
            let done = done.clone();
            let ml = main_loop.clone();
            jobs.spawn(
                move || {
                    std::thread::sleep(Duration::from_millis(200));
                    i
                },
                move |v| {
                    assert_eq!(
                        std::thread::current().id(),
                        main_thread,
                        "done en el hilo de GLib"
                    );
                    done.borrow_mut().push(v);
                    if done.borrow().len() == 4 {
                        ml.quit();
                    }
                },
            );
        }
        main_loop.run();
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(390) && elapsed < Duration::from_millis(700),
            "{elapsed:?}"
        );
        let mut got = done.borrow().clone();
        got.sort_unstable();
        assert_eq!(got, [0, 1, 2, 3]);
    })
    .unwrap();
}

#[test]
fn descendants_cannot_hold_the_timeout_open() {
    let started = Instant::now();
    let out = run(&spec("sh", &["-c", "sleep 30 & wait"], 150)).unwrap();
    assert!(out.timed_out);
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn output_is_capped_while_the_rest_is_drained() {
    let out = run(&spec("head", &["-c", "17000000", "/dev/zero"], 5000)).unwrap();
    assert_eq!(out.stdout.len(), comandos_app::proc::OUTPUT_CAP);
    assert!(out.truncated);
    assert_eq!(out.code, Some(0));
}

#[test]
fn jobs_shutdown_cancels_queued_work_and_waits_only_to_its_deadline() {
    let context = glib::MainContext::new();
    let _guard = context.acquire().unwrap();
    context
        .with_thread_default(|| {
            let jobs = Jobs::new(2, "shutdown");
            let running = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            for _ in 0..8 {
                let running = running.clone();
                jobs.spawn(
                    move || {
                        running.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                        std::thread::sleep(Duration::from_millis(200));
                    },
                    |_| {},
                );
            }
            while running.load(std::sync::atomic::Ordering::Acquire) < 2 {
                std::thread::sleep(Duration::from_millis(1));
            }
            let started = Instant::now();
            jobs.shutdown(Duration::from_millis(20));
            assert!(started.elapsed() < Duration::from_millis(100));
            std::thread::sleep(Duration::from_millis(220));
            assert_eq!(running.load(std::sync::atomic::Ordering::Acquire), 2);
        })
        .unwrap();
}
