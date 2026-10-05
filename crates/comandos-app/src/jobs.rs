//! Trabajo fuera del hilo de GTK. `Jobs` es un grupo de hilos fijos para tareas
//! cortas (HTTP ≤ 15 s, tmux ≤ 5 s); los bucles largos (`poll_state_loop`,
//! `ws_poll_loop`, `tmux_clipboard_loop`, esperas de cuenta) van en su propio hilo
//! con `spawn_loop`. El resultado vuelve al bucle de GLib por `async_channel`.
use std::sync::{Arc, Mutex, mpsc};

type Work = Box<dyn FnOnce() + Send + 'static>;

#[derive(Clone)]
pub struct Jobs {
    tx: mpsc::Sender<Work>,
}

impl Jobs {
    pub fn new(workers: usize, name: &str) -> Jobs {
        let (tx, rx) = mpsc::channel::<Work>();
        let rx = Arc::new(Mutex::new(rx));
        for i in 0..workers.max(2) {
            let rx = Arc::clone(&rx);
            let _ = std::thread::Builder::new()
                .name(format!("comandos-{name}-{i}"))
                .spawn(move || {
                    loop {
                        let next = match rx.lock() {
                            Ok(guard) => guard.recv(),
                            Err(_) => break,
                        };
                        match next {
                            Ok(work) => work(),
                            Err(_) => break,
                        }
                    }
                });
        }
        Jobs { tx }
    }

    /// `work` en un hilo del grupo; `done(resultado)` después, en el hilo de GLib.
    pub fn spawn<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(T) + 'static,
    ) {
        let back = to_main(done);
        let _ = self.tx.send(Box::new(move || {
            let _ = back.send_blocking(work());
        }));
    }
}

/// Canal hacia el hilo de GLib: lo que se envíe llega a `done` una vez.
pub fn to_main<T: Send + 'static>(done: impl FnOnce(T) + 'static) -> async_channel::Sender<T> {
    let (tx, rx) = async_channel::bounded::<T>(1);
    glib::MainContext::ref_thread_default().spawn_local(async move {
        if let Ok(value) = rx.recv().await {
            done(value);
        }
    });
    tx
}

/// Hilo propio con nombre para un bucle largo (equivale a `threading.Thread(daemon=True)`).
pub fn spawn_loop(name: &str, body: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name(format!("comandos-{name}"))
        .spawn(body)
        .map(|_| ())
}
