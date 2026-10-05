//! El runtime del frente acota su pool de bloqueo: una ola de trabajos
//! simultáneos (los iframes de `term.html` que despiertan a la vez) no crea un
//! hilo por trabajo, nada se pierde por el tope y un trabajo largo (el tecleo
//! de `/terminal/type`) no bloquea a los demás.
use comandos_server::dash::{BLOCKING_KEEP_ALIVE, MAX_BLOCKING_THREADS, runtime};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[test]
fn a_wave_of_blocking_jobs_never_exceeds_the_cap_and_all_finish() {
    let rt = runtime().unwrap();
    let seen = Arc::new(Mutex::new(HashSet::new()));
    let done = rt.block_on(async {
        let jobs: Vec<_> = (0..32)
            .map(|_| {
                let seen = seen.clone();
                tokio::task::spawn_blocking(move || {
                    seen.lock().unwrap().insert(thread::current().id());
                    thread::sleep(Duration::from_millis(20));
                })
            })
            .collect();
        let mut done = 0;
        for job in jobs {
            job.await.unwrap();
            done += 1;
        }
        done
    });
    assert_eq!(done, 32, "ningún trabajo se pierde por el tope");
    let threads = seen.lock().unwrap().len();
    assert!(
        (1..=MAX_BLOCKING_THREADS).contains(&threads),
        "{threads} hilos de bloqueo > {MAX_BLOCKING_THREADS}"
    );
}

#[test]
fn a_long_blocking_job_leaves_room_for_the_rest() {
    // Con un solo hilo el tecleo lo acapararía.
    const { assert!(MAX_BLOCKING_THREADS >= 2) };
    let rt = runtime().unwrap();
    let elapsed = rt.block_on(async {
        // El tecleo largo ocupa un hilo durante todo el lote.
        let long = tokio::task::spawn_blocking(|| thread::sleep(Duration::from_millis(1500)));
        tokio::time::sleep(Duration::from_millis(20)).await;
        let start = Instant::now();
        for _ in 0..20 {
            tokio::task::spawn_blocking(|| thread::sleep(Duration::from_millis(2)))
                .await
                .unwrap();
        }
        let elapsed = start.elapsed();
        long.await.unwrap();
        elapsed
    });
    assert!(
        elapsed < Duration::from_millis(1000),
        "los saltos cortos esperaron al largo: {elapsed:?}"
    );
}

#[test]
fn idle_blocking_threads_retire_sooner_than_tokio_default() {
    assert!(BLOCKING_KEEP_ALIVE < Duration::from_secs(10));
    assert!(BLOCKING_KEEP_ALIVE >= Duration::from_millis(500));
}
