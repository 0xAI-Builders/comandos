use bytes::Bytes;
use comandos_server::{HandlerError, Reply, ReplyBody, Request, blocking::BlockingWorker};
use http::{Method, StatusCode};
use serde_json::{Value, json};
use std::{
    future::{Future, poll_fn},
    io,
    pin::Pin,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::Poll,
    time::Duration,
};
use tokio::{sync::oneshot, time::timeout};

const WAIT: Duration = Duration::from_secs(3);
fn request(target: &str) -> Request {
    Request {
        method: Method::GET,
        target: target.into(),
        peer: "127.0.0.1:12345".parse().unwrap(),
        headers: vec![],
        data: None,
        internal_producer: false,
    }
}
fn reply(value: Value) -> Result<Reply, HandlerError> {
    Reply::json(StatusCode::OK, &value)
}
fn body(reply: Reply) -> Value {
    let ReplyBody::Bytes(bytes) = reply.body else {
        panic!("expected finite response")
    };
    serde_json::from_slice(&bytes).unwrap()
}

#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);
impl Gate {
    fn wait(&self) {
        let (lock, changed) = &*self.0;
        let mut open = lock.lock().unwrap();
        while !*open {
            open = changed.wait(open).unwrap();
        }
    }
    fn open(&self) {
        let (lock, changed) = &*self.0;
        *lock.lock().unwrap() = true;
        changed.notify_all();
    }
}
struct Release(Gate);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.open();
    }
}
async fn poll_pending<F: Future>(mut future: Pin<&mut F>) {
    poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

#[test]
fn invalid_capacity_rejects_before_backend_execution() {
    for capacity in [0, 65537, usize::MAX] {
        let error = BlockingWorker::start(capacity, |_| panic!("invalid worker executed backend"))
            .err()
            .expect("invalid capacity");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn serial_owned_state_runs_off_network_thread() {
    let caller = std::thread::current().id();
    let mut sequence = 0;
    let worker = BlockingWorker::start(2, move |_| {
        assert_ne!(std::thread::current().id(), caller);
        sequence += 1;
        reply(json!({"sequence":sequence}))
    })
    .unwrap();
    let handler = worker.handler();
    for expected in 1..=3 {
        assert_eq!(
            body(
                timeout(WAIT, handler(request("/read")))
                    .await
                    .unwrap()
                    .unwrap()
            )["sequence"],
            expected
        );
    }
    timeout(WAIT, worker.shutdown()).await.unwrap().unwrap();
    assert!(handler(request("/closed")).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn full_queue_backpressures_and_canceled_queued_work_has_no_effect() {
    let gate = Gate::default();
    let work_gate = gate.clone();
    let (started, running) = oneshot::channel();
    let mut started = Some(started);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = calls.clone();
    let worker = BlockingWorker::start(1, move |r| {
        observed.lock().unwrap().push(r.target.clone());
        if r.target == "/hold" {
            let _ = started.take().unwrap().send(());
            work_gate.wait();
        }
        reply(json!({"target":r.target}))
    })
    .unwrap();
    // Release is dropped before worker if an assertion unwinds.
    let release = Release(gate.clone());
    let handler = worker.handler();
    let mut active = handler(request("/hold"));
    poll_pending(Pin::new(&mut active)).await;
    timeout(WAIT, running).await.unwrap().unwrap();
    // This timer proves the network executor remains usable during blocked work.
    timeout(WAIT, tokio::time::sleep(Duration::from_millis(5)))
        .await
        .unwrap();
    let mut canceled = handler(request("/canceled"));
    poll_pending(Pin::new(&mut canceled)).await;
    let mut waiting = handler(request("/after"));
    poll_pending(Pin::new(&mut waiting)).await;
    assert_eq!(*calls.lock().unwrap(), ["/hold"]);
    drop(canceled);
    drop(release);
    assert_eq!(
        body(timeout(WAIT, active).await.unwrap().unwrap())["target"],
        "/hold"
    );
    assert_eq!(
        body(timeout(WAIT, waiting).await.unwrap().unwrap())["target"],
        "/after"
    );
    timeout(WAIT, worker.shutdown()).await.unwrap().unwrap();
    assert_eq!(*calls.lock().unwrap(), ["/hold", "/after"]);
}

#[tokio::test(flavor = "current_thread")]
async fn canceled_running_work_finishes_once_and_worker_accepts_next_job() {
    let gate = Gate::default();
    let work_gate = gate.clone();
    let (started, running) = oneshot::channel();
    let mut started = Some(started);
    let mut count = 0;
    let worker = BlockingWorker::start(1, move |r| {
        count += 1;
        if r.target == "/commit" {
            let _ = started.take().unwrap().send(());
            work_gate.wait();
        }
        reply(json!({"count":count}))
    })
    .unwrap();
    let release = Release(gate.clone());
    let handler = worker.handler();
    let mut canceled = handler(request("/commit"));
    poll_pending(Pin::new(&mut canceled)).await;
    timeout(WAIT, running).await.unwrap().unwrap();
    drop(canceled);
    drop(release);
    assert_eq!(
        body(
            timeout(WAIT, handler(request("/next")))
                .await
                .unwrap()
                .unwrap()
        )["count"],
        2
    );
    timeout(WAIT, worker.shutdown()).await.unwrap().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn shutdown_discards_queue_denies_cloned_handlers_and_joins_active_work() {
    let gate = Gate::default();
    let work_gate = gate.clone();
    let (started, running) = oneshot::channel();
    let mut started = Some(started);
    let effects = Arc::new(Mutex::new(Vec::new()));
    let observed = effects.clone();
    let worker = BlockingWorker::start(1, move |r| {
        observed.lock().unwrap().push(r.target.clone());
        let _ = started.take().unwrap().send(());
        work_gate.wait();
        reply(json!({"ok":true}))
    })
    .unwrap();
    let release = Release(gate.clone());
    let handler = worker.handler();
    let mut active = handler(request("/active"));
    poll_pending(Pin::new(&mut active)).await;
    timeout(WAIT, running).await.unwrap().unwrap();
    let mut queued = handler(request("/queued"));
    poll_pending(Pin::new(&mut queued)).await;
    let closing = worker.shutdown();
    tokio::pin!(closing);
    // A canceled shutdown owns the worker; release the active callback before
    // that future is dropped if an assertion below unwinds.
    let _closing_release = Release(gate.clone());
    poll_pending(closing.as_mut()).await;
    assert!(
        timeout(WAIT, handler(request("/late")))
            .await
            .unwrap()
            .is_err()
    );
    drop(release);
    timeout(WAIT, closing).await.unwrap().unwrap();
    assert!(timeout(WAIT, queued).await.unwrap().is_err());
    assert!(timeout(WAIT, active).await.unwrap().is_ok());
    assert_eq!(*effects.lock().unwrap(), ["/active"]);
}

#[tokio::test(flavor = "current_thread")]
async fn backend_panic_retires_worker_and_closes_future_admission() {
    let worker = BlockingWorker::start(1, |_| panic!("fixture backend panic")).unwrap();
    let handler = worker.handler();
    assert!(
        timeout(WAIT, handler(request("/panic")))
            .await
            .unwrap()
            .is_err()
    );
    assert!(
        timeout(WAIT, handler(request("/after")))
            .await
            .unwrap()
            .is_err()
    );
    timeout(WAIT, worker.shutdown()).await.unwrap().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_owner_joins_worker_and_releases_backend_state() {
    struct Witness(Arc<AtomicBool>);
    impl Drop for Witness {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let witness = Witness(dropped.clone());
    let worker = BlockingWorker::start(1, move |_| {
        let _keep = &witness;
        Ok(Reply::bytes(
            StatusCode::OK,
            "text/plain",
            Bytes::from_static(b"ok"),
        ))
    })
    .unwrap();
    let handler = worker.handler();
    timeout(WAIT, handler(request("/once")))
        .await
        .unwrap()
        .unwrap();
    drop(worker);
    assert!(dropped.load(Ordering::SeqCst));
    assert!(
        timeout(WAIT, handler(request("/closed")))
            .await
            .unwrap()
            .is_err()
    );
}
