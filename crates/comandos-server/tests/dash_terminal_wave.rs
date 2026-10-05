//! Una ola de POST `/terminal-panes` con tmux lento ocupa como mucho un hilo
//! de bloqueo (la puerta asíncrona de `terminal.rs`) y los estáticos siguen
//! respondiendo rápido mientras dura. Va sola en su binario: cuenta los hilos
//! `tokio-rt-worker` del proceso.
mod support;

use comandos_server::dash::{native::tmux::Program, native::tmux::Tmux, runtime};
use std::{
    ffi::OsString,
    time::{Duration, Instant},
};
use support::{TestHome, dead_port, front, get, request_body};

/// Hilos del pool de bloqueo vivos ahora (en este runtime, todos se llaman así).
fn pool_threads() -> usize {
    std::fs::read_dir("/proc/self/task")
        .map(|dir| {
            dir.filter_map(Result::ok)
                .filter(|t| {
                    std::fs::read_to_string(t.path().join("comm"))
                        .is_ok_and(|c| c.trim() == "tokio-rt-worker")
                })
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn a_slow_tmux_wave_holds_one_pool_thread_and_statics_stay_fast() {
    let rt = runtime().unwrap();
    rt.block_on(async {
        let home = TestHome::new("term-wave");
        let mut opts = home.options();
        // Falso: cada llamada a tmux tarda 300 ms y sale con 1 (no mira argumentos);
        // la ola de 8 dura ≈ 2,4 s porque la librería las serializa.
        opts.tmux = Tmux {
            program: Program {
                path: "sh".into(),
                prefix: vec![
                    OsString::from("-c"),
                    OsString::from("sleep 0.3; exit 1"),
                    OsString::from("sh"),
                ],
                env: vec![],
                env_remove: vec![],
            },
            timeout: Duration::from_secs(5),
        };
        let front = front(&home, dead_port(), opts).await;
        let port = front.port;
        let wave: Vec<_> = (0..8)
            .map(|i| {
                tokio::spawn(async move {
                    let body = format!(r#"{{"session": "s{i}", "action": "list"}}"#);
                    request_body(port, "POST", "/terminal-panes", "", &body).await
                })
            })
            .collect();
        tokio::time::sleep(Duration::from_millis(150)).await;
        let mut most = 0;
        let mut slowest = Duration::ZERO;
        for _ in 0..10 {
            most = most.max(pool_threads());
            let t = Instant::now();
            assert_eq!(get(port, "/term.html").await.status, 200);
            slowest = slowest.max(t.elapsed());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        for job in wave {
            // `list-panes` sale con 1: «No se encuentra la sesión».
            assert_eq!(job.await.unwrap().status, 400);
        }
        // La ola y, a ratos, la lectura del estático: dos como mucho.
        assert!(most <= 2, "{most} hilos de bloqueo durante la ola");
        assert!(
            slowest < Duration::from_millis(250),
            "el estático esperó a la ola: {slowest:?}"
        );
        front.stop().await;
    });
}
