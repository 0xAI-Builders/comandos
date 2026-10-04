//! Serial ownership of a synchronous backend, separate from the network runtime.
use crate::{Handler, HandlerError, Reply, Request};
use std::{
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot};

struct Job {
    request: Request,
    reply: oneshot::Sender<Result<Reply, HandlerError>>,
    permit: OwnedSemaphorePermit,
}
struct Shared {
    // The sole sender is private. No clone can outlive this short critical
    // section, so taking it reliably wakes an idle receiver on shutdown.
    sender: Mutex<Option<mpsc::Sender<Job>>>,
    slots: Arc<Semaphore>,
    stopping: AtomicBool,
}
impl Shared {
    fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        self.slots.close();
        self.sender.lock().unwrap_or_else(|p| p.into_inner()).take();
    }
}

/// Owner of one synchronous backend. Call `shutdown` before runtime teardown.
/// Drop also stops and joins, but blocks the dropping thread. Callbacks must
/// finish in bounded time and must not depend on that thread making progress.
/// Running work is never forcibly aborted; it may commit after its reply is lost.
pub struct BlockingWorker {
    shared: Arc<Shared>,
    done: Option<oneshot::Receiver<()>>,
    thread: Option<JoinHandle<()>>,
}
impl BlockingWorker {
    /// Queue capacity excludes the currently executing job. Transport limits
    /// separately bound callers waiting for admission and their input bytes.
    pub fn start<F>(capacity: usize, mut work: F) -> io::Result<Self>
    where
        F: FnMut(Request) -> Result<Reply, HandlerError> + Send + 'static,
    {
        if !(1..=65536).contains(&capacity) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid worker capacity",
            ));
        }
        let (sender, receiver) = mpsc::channel::<Job>();
        let (finished, done) = oneshot::channel();
        let shared = Arc::new(Shared {
            sender: Mutex::new(Some(sender)),
            slots: Arc::new(Semaphore::new(capacity)),
            stopping: AtomicBool::new(false),
        });
        let state = shared.clone();
        let thread = thread::Builder::new()
            .name("comandos-handler".into())
            .spawn(move || {
                while let Ok(Job {
                    request,
                    reply,
                    permit,
                }) = receiver.recv()
                {
                    drop(permit);
                    if state.stopping.load(Ordering::Acquire) {
                        break;
                    }
                    if reply.is_closed() {
                        continue;
                    }
                    // Mutable backend state may be inconsistent after unwinding.
                    // Retire the worker rather than reusing it after a panic.
                    let result = catch_unwind(AssertUnwindSafe(|| work(request)));
                    let panicked = result.is_err();
                    let _ = reply.send(result.unwrap_or(Err(HandlerError::Failure)));
                    if panicked {
                        break;
                    }
                }
                state.stop();
                drop(receiver);
                drop(work);
                let _ = finished.send(());
            })?;
        Ok(Self {
            shared,
            done: Some(done),
            thread: Some(thread),
        })
    }

    pub fn handler(&self) -> Handler {
        let shared = self.shared.clone();
        Arc::new(move |request| {
            let shared = shared.clone();
            Box::pin(async move {
                let permit = shared
                    .slots
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| HandlerError::Failure)?;
                let (reply, result) = oneshot::channel();
                {
                    let sender = shared.sender.lock().map_err(|_| HandlerError::Failure)?;
                    if shared.stopping.load(Ordering::Acquire) {
                        return Err(HandlerError::Failure);
                    }
                    sender
                        .as_ref()
                        .ok_or(HandlerError::Failure)?
                        .send(Job {
                            request,
                            reply,
                            permit,
                        })
                        .map_err(|_| HandlerError::Failure)?;
                }
                result.await.unwrap_or(Err(HandlerError::Failure))
            })
        })
    }

    /// Stop admission, skip queued work and await the active operation and join.
    /// Canceling this future uses the synchronous Drop fallback.
    pub async fn shutdown(mut self) -> io::Result<()> {
        self.shared.stop();
        let completed = self.done.take().expect("worker completion receiver").await;
        let joined = self.thread.take().expect("worker thread").join();
        if completed.is_err() || joined.is_err() {
            Err(io::Error::other("blocking worker terminated unexpectedly"))
        } else {
            Ok(())
        }
    }
}
impl Drop for BlockingWorker {
    fn drop(&mut self) {
        self.shared.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
