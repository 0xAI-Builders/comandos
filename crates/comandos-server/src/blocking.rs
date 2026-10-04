//! Serial ownership of a synchronous backend, separate from the network runtime.
//! `BackendWorker<B>` owns `B` on one thread; `BackendCaller<B>` submits closures.
//! `BlockingWorker` keeps the request-shaped API of the previous phase.
use crate::{Handler, HandlerError, Reply, Request};
use std::{
    io,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot};

/// One queued closure over the backend. `abandoned` lets the thread skip work
/// whose caller already gave up; `run` reports whether the closure panicked.
trait Task<B>: Send {
    fn abandoned(&self) -> bool;
    fn run(self: Box<Self>, backend: &mut B) -> bool;
}

struct Call<B, T, F> {
    job: F,
    reply: oneshot::Sender<Result<T, HandlerError>>,
    // fn(&mut B) keeps Call Send/Sync independent of B, which never crosses threads here.
    _backend: PhantomData<fn(&mut B)>,
}

impl<B, T, F> Task<B> for Call<B, T, F>
where
    F: FnOnce(&mut B) -> T + Send,
    T: Send,
{
    fn abandoned(&self) -> bool {
        self.reply.is_closed()
    }
    fn run(self: Box<Self>, backend: &mut B) -> bool {
        let Call { job, reply, .. } = *self;
        let result = catch_unwind(AssertUnwindSafe(|| job(backend)));
        let panicked = result.is_err();
        let _ = reply.send(result.map_err(|_| HandlerError::Failure));
        panicked
    }
}

struct Queued<B> {
    task: Box<dyn Task<B>>,
    permit: OwnedSemaphorePermit,
}

struct Shared<B> {
    // The sole sender is private. No clone can outlive this short critical
    // section, so taking it reliably wakes an idle receiver on shutdown.
    sender: Mutex<Option<mpsc::Sender<Queued<B>>>>,
    slots: Arc<Semaphore>,
    stopping: AtomicBool,
}

impl<B> Shared<B> {
    fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        self.slots.close();
        self.sender.lock().unwrap_or_else(|p| p.into_inner()).take();
    }
}

/// Owner of one synchronous backend. Call `shutdown` before runtime teardown.
/// Drop also stops and joins, but blocks the dropping thread. Closures must
/// finish in bounded time and must not depend on that thread making progress.
/// Running work is never forcibly aborted; it may commit after its reply is lost.
pub struct BackendWorker<B> {
    shared: Arc<Shared<B>>,
    done: Option<oneshot::Receiver<()>>,
    thread: Option<JoinHandle<()>>,
}

impl<B: Send + 'static> BackendWorker<B> {
    /// Queue capacity excludes the currently executing job. Transport limits
    /// separately bound callers waiting for admission and their input bytes.
    pub fn start(capacity: usize, backend: B) -> io::Result<Self> {
        if !(1..=65536).contains(&capacity) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid worker capacity",
            ));
        }
        let (sender, receiver) = mpsc::channel::<Queued<B>>();
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
                let mut backend = backend;
                while let Ok(Queued { task, permit }) = receiver.recv() {
                    drop(permit);
                    if state.stopping.load(Ordering::Acquire) {
                        break;
                    }
                    if task.abandoned() {
                        continue;
                    }
                    // Mutable backend state may be inconsistent after unwinding.
                    // Retire the worker rather than reusing it after a panic.
                    if task.run(&mut backend) {
                        break;
                    }
                }
                state.stop();
                drop(receiver);
                drop(backend);
                let _ = finished.send(());
            })?;
        Ok(Self {
            shared,
            done: Some(done),
            thread: Some(thread),
        })
    }

    pub fn caller(&self) -> BackendCaller<B> {
        BackendCaller {
            shared: self.shared.clone(),
        }
    }

    /// Stop admission, skip queued work and await the active operation and join.
    /// Canceling this future uses the synchronous Drop fallback.
    pub async fn shutdown(mut self) -> io::Result<()> {
        self.shared.stop();
        // Ambos existen hasta aquí: solo `shutdown` (que consume) y `Drop` los toman.
        let (Some(done), Some(thread)) = (self.done.take(), self.thread.take()) else {
            return Err(io::Error::other("blocking worker already stopped"));
        };
        let completed = done.await;
        let joined = thread.join();
        if completed.is_err() || joined.is_err() {
            Err(io::Error::other("blocking worker terminated unexpectedly"))
        } else {
            Ok(())
        }
    }
}

impl<B> Drop for BackendWorker<B> {
    fn drop(&mut self) {
        self.shared.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Cloneable submitter. Waiting for a queue slot is cancel-safe; once queued,
/// a dropped caller future only marks the job abandoned.
pub struct BackendCaller<B> {
    shared: Arc<Shared<B>>,
}

impl<B> Clone for BackendCaller<B> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<B> BackendCaller<B> {
    /// True once the worker stops admitting work (shutdown or retirement
    /// after a panic). It never becomes false again.
    pub fn stopped(&self) -> bool {
        self.shared.stopping.load(Ordering::Acquire)
    }
}

impl<B: 'static> BackendCaller<B> {
    pub async fn call<T, F>(&self, job: F) -> Result<T, HandlerError>
    where
        F: FnOnce(&mut B) -> T + Send + 'static,
        T: Send + 'static,
    {
        let permit = self
            .shared
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| HandlerError::Failure)?;
        let (reply, result) = oneshot::channel();
        {
            let sender = self
                .shared
                .sender
                .lock()
                .map_err(|_| HandlerError::Failure)?;
            if self.shared.stopping.load(Ordering::Acquire) {
                return Err(HandlerError::Failure);
            }
            let task: Box<dyn Task<B>> = Box::new(Call {
                job,
                reply,
                _backend: PhantomData,
            });
            sender
                .as_ref()
                .ok_or(HandlerError::Failure)?
                .send(Queued { task, permit })
                .map_err(|_| HandlerError::Failure)?;
        }
        result.await.unwrap_or(Err(HandlerError::Failure))
    }
}

type Work = Box<dyn FnMut(Request) -> Result<Reply, HandlerError> + Send>;

/// Request-shaped worker of the previous phase, now a thin `BackendWorker`.
pub struct BlockingWorker {
    inner: BackendWorker<Work>,
}

impl BlockingWorker {
    pub fn start<F>(capacity: usize, work: F) -> io::Result<Self>
    where
        F: FnMut(Request) -> Result<Reply, HandlerError> + Send + 'static,
    {
        Ok(Self {
            inner: BackendWorker::start(capacity, Box::new(work) as Work)?,
        })
    }

    pub fn handler(&self) -> Handler {
        let caller = self.inner.caller();
        Arc::new(move |request| {
            let caller = caller.clone();
            Box::pin(async move { caller.call(move |work: &mut Work| work(request)).await? })
        })
    }

    pub async fn shutdown(self) -> io::Result<()> {
        self.inner.shutdown().await
    }
}
