//! 2f-3/T5: Web Push inerte (D10), POST `/pomodoro` y el planificador de
//! Pomodoro del frente.
//!
//! Confinamiento: ninguna ruta de esta tarea lanza procesos ni toca tmux. Las
//! comparaciones de mutaciones van por el gemelo (`support::twin`: dos HOME,
//! oráculo con la red cerrada y `fakebin` confinado); el planificador del
//! frente solo corre en las pruebas que lo arrancan sobre un `Native` propio
//! con `Background::front()` y lo paran al terminar. El sonido de fin de bloque
//! nunca suena: el `search_path` del frente es el `bin` vacío del HOME
//! temporal y el `pw-play` del oráculo es un falso que solo anota.
mod support;

use comandos_server::{
    Request,
    dash::native::{
        Background, Native, NativeRoute, Outcome, background, push::PushRoute, route, wall_clock_ms,
    },
};
use http::Method;
use regex::Regex;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};
use support::{
    NOW_MS, TestHome,
    oracle::{OracleOpts, oracle_with},
    twin::{Twin, TwinOpts, normalize},
};

fn post_request(target: &str, body: &Value) -> Request {
    let text = serde_json::to_vec(body).unwrap();
    Request {
        method: Method::POST,
        target: target.into(),
        peer: "127.0.0.1:12345".parse().unwrap(),
        headers: vec![],
        data: Some(body.clone()),
        body: bytes::Bytes::from(text),
        internal_producer: false,
    }
}

/// POST `/pomodoro` directo sobre `Native` (sin transporte): estado y cuerpo.
async fn pomodoro(native: &Arc<Native>, body: Value) -> (u16, Value) {
    let request = post_request("/pomodoro", &body);
    match native
        .dispatch(NativeRoute::Push(PushRoute::PomodoroPost), &request)
        .await
    {
        Ok(Outcome::Reply(reply)) => {
            let status = reply.status.as_u16();
            let bytes = match reply.body {
                comandos_server::ReplyBody::Bytes(bytes) => bytes,
                _ => panic!("cuerpo en streaming"),
            };
            (status, serde_json::from_slice(&bytes).unwrap())
        }
        Ok(Outcome::Decline) => panic!("declinó"),
        Err(error) => panic!("error {error:?}"),
    }
}

fn count(home: &TestHome, sql: &str) -> i64 {
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn block_status(home: &TestHome) -> Option<String> {
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn.query_row(
        "select b.status from pomodoro_state s join pomodoro_blocks b on b.block_id=s.block_id",
        [],
        |r| r.get(0),
    )
    .ok()
}

fn vencido(home: &TestHome) -> Option<String> {
    let conn = rusqlite::Connection::open(home.state_db()).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn.query_row(
        "select status from pomodoro_blocks where block_id='vencido'",
        [],
        |r| r.get(0),
    )
    .ok()
}

/// Los ids aleatorios (`uuid4().hex`, `token_hex(16)`) difieren entre lados.
fn ids(text: &str) -> String {
    let re = Regex::new("[0-9a-f]{32}").unwrap();
    re.replace_all(&normalize(text), "ID").into_owned()
}

#[test]
fn push_routes_claim_exactly_the_python_paths() {
    let push = |r| Some(NativeRoute::Push(r));
    assert_eq!(route(&Method::GET, "/push/key"), push(PushRoute::Key));
    assert_eq!(route(&Method::GET, "/push/key?x=1"), push(PushRoute::Key));
    assert_eq!(route(&Method::GET, "/push/keys"), None);
    assert_eq!(
        route(&Method::POST, "/push/subscription"),
        push(PushRoute::SubscriptionPost)
    );
    assert_eq!(
        route(&Method::DELETE, "/push/subscription"),
        push(PushRoute::SubscriptionDelete)
    );
    assert_eq!(route(&Method::DELETE, "/push/subscription?x"), None);
    assert_eq!(route(&Method::POST, "/push/test"), push(PushRoute::Test));
    assert_eq!(
        route(&Method::POST, "/pomodoro"),
        push(PushRoute::PomodoroPost)
    );
    assert_eq!(route(&Method::POST, "/pomodoro?x"), None);
    // La GET sigue en la base (P3).
    assert_eq!(
        route(&Method::GET, "/pomodoro"),
        Some(NativeRoute::Pomodoro)
    );
}

#[tokio::test]
async fn push_and_pomodoro_post_match_python_twin() {
    let seed = |home: &TestHome| home.write("focus-queue.jsonl", "{\"title\": \"aviso\"}\n");
    let opts = TwinOpts {
        // El reloj del temporizador del oráculo es el del frente (`NOW_MS`).
        python_prelude: format!("dash.POMODORO_CLOCK = lambda: {NOW_MS}"),
        ..TwinOpts::default()
    };
    let Some(twin) = Twin::start_with("pomo-twin", seed, opts).await else {
        return;
    };
    let same = |run: &support::twin::TwinRun, status: u16| {
        assert_eq!(run.front.status, status, "{}", run.front.text());
        assert_eq!(run.oracle.status, status, "{}", run.oracle.text());
        assert_eq!(ids(&run.front.text()), ids(&run.oracle.text()));
    };
    // Web Push: los cuatro 503 de hoy (D10).
    let key = twin.get("/push/key").await;
    same(&key, 503);
    key.assert_same();
    assert_eq!(
        key.front.text(),
        r#"{"available": false, "error": "Push no disponible: falta pywebpush (ModuleNotFoundError); ver requirements-push.txt"}"#
    );
    same(
        &twin
            .post("/push/subscription", r#"{"subscription": {}}"#)
            .await,
        503,
    );
    same(
        &twin
            .request("DELETE", "/push/subscription", r#"{"endpoint": "x"}"#)
            .await,
        503,
    );
    same(&twin.post("/push/test", r#"{"endpoint": "x"}"#).await, 503);
    // Ajustes.
    same(
        &twin
            .post("/pomodoro", r#"{"settings": {"style": "neon"}}"#)
            .await,
        400,
    );
    same(
        &twin
            .post("/pomodoro", r#"{"settings": {"style": 1}}"#)
            .await,
        400,
    );
    same(&twin.post("/pomodoro", r#"{"settings": [1]}"#).await, 400);
    let saved = twin
        .post(
            "/pomodoro",
            r#"{"settings": {"style": "garden", "cycles": 4, "otra": 1, "autoBreak": true, "focusMinutes": 50.5}}"#,
        )
        .await;
    same(&saved, 200);
    saved.assert_same();
    // Orden del temporizador: inicio, repetición, revisión vieja.
    let start = r#"{"requestId": "r1", "expectedRevision": null, "action": "start", "mode": "focus", "targetMs": 1500000, "project": "p"}"#;
    let started = twin.post("/pomodoro", start).await;
    same(&started, 200);
    assert!(started.front.text().contains(r#""status": "running""#));
    let replay = twin.post("/pomodoro", start).await;
    same(&replay, 200);
    assert!(replay.front.text().contains(r#""replayed": true"#));
    let stale = twin
        .post(
            "/pomodoro",
            r#"{"requestId": "r2", "expectedRevision": 0, "action": "pause"}"#,
        )
        .await;
    same(&stale, 409);
    same(&twin.post("/pomodoro", r#"{"action": 5}"#).await, 400);
    // Cuerpo anterior a 1.0: `stop` cancela, `mins` inicia.
    same(&twin.post("/pomodoro", r#"{"stop": true}"#).await, 200);
    let legacy = twin
        .post("/pomodoro", r#"{"mins": "30", "mode": "focus"}"#)
        .await;
    same(&legacy, 200);
    assert!(legacy.front.text().contains(r#""targetMs": 1800000"#));
    same(&twin.post("/pomodoro", r#"{"mins": 0.5}"#).await, 409);
    // `ack` borra la cola en los dos.
    let ack = twin.post("/pomodoro", r#"{"ack": 1}"#).await;
    same(&ack, 200);
    ack.assert_same();
    assert!(!twin.a.hooks().join("focus-queue.jsonl").exists());
    assert!(!twin.b.hooks().join("focus-queue.jsonl").exists());
    // El estado que leen después (GET nativa contra el Python).
    let state = twin.get("/pomodoro").await;
    same(&state, 200);
    assert!(state.front.text().contains(
        r#""settings": {"style": "garden", "cycles": 4, "autoBreak": true, "focusMinutes": 50.5}"#
    ));
}

/// Reloj falso que corre con el real desde `base` (para mover el tiempo del
/// temporizador sin esperar de verdad).
fn moving_clock(base: &Arc<AtomicI64>) -> Arc<dyn Fn() -> i64 + Send + Sync> {
    let base = Arc::clone(base);
    let t0 = Instant::now();
    Arc::new(move || base.load(Ordering::SeqCst) + i64::try_from(t0.elapsed().as_millis()).unwrap())
}

/// Lo que adopta el dueño al arrancar: `focus.json` (la autoridad anterior a
/// 1.0, con `until` en segundos) y dos bloques del historial de la base de uso.
fn seed_legacy(home: &TestHome, until_seconds: i64) {
    let started = until_seconds - 1500;
    home.write(
        "focus.json",
        &format!(
            r#"{{"until": {until_seconds}, "startedAt": {started}, "mins": 25, "mode": "focus", "project": "p"}}"#
        ),
    );
    let conn = comandos_store::usage::open_usage_db_at(&home.usage_db()).unwrap();
    comandos_store::usage::ensure_schema(&conn).unwrap();
    conn.execute_batch(
        "insert into focus_blocks(id,mode,project,planned_minutes,started_at_ms,ended_at_ms,status)
           values('h1','focus','p',25,1700000000000,1700001500000,'completed'),
                 ('h2','focus','p',50,1700003000000,1700006000000,'completed')",
    )
    .unwrap();
}

fn migrated(home: &TestHome) -> usize {
    std::fs::read_dir(home.hooks())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("focus.json.migrated-")
        })
        .count()
}

/// D6: el planificador corre con `legacy` y con `front`, pero la migración de
/// arranque (`focus.json` y el historial) solo la hace el dueño (`front`).
#[tokio::test]
async fn scheduler_runs_in_both_modes_but_only_the_owner_migrates() {
    for (background, owner) in [(Background::legacy(), false), (Background::front(), true)] {
        let home = TestHome::new_short(if owner { "pomo-own-f" } else { "pomo-own-l" });
        seed_legacy(&home, NOW_MS / 1000 + 600);
        let mut opts = home.options();
        opts.background = background;
        let native = Arc::new(Native::new(opts));
        assert!(native.ready().await);
        let runner = background::start(&native);
        assert!(runner.pomodoro());
        // Con `front` también el vigilante de modelos y el bucle de límites
        // (2f-3/T6; esperan 90 s y 300 s antes de su primera vuelta).
        assert_eq!(native.tasks().len(), if owner { 3 } else { 1 });
        let legacy_records =
            "select count(*) from pomodoro_records where provenance='legacy-planned'";
        if owner {
            let deadline = Instant::now() + Duration::from_secs(10);
            while count(&home, legacy_records) < 2 {
                assert!(Instant::now() < deadline, "el dueño no migró");
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert!(!home.hooks().join("focus.json").exists());
            assert_eq!(migrated(&home), 1);
            assert_eq!(block_status(&home).as_deref(), Some("running"));
        } else {
            // Su primera vuelta (cerrar vencidos) ya pasó; nada que adoptar.
            tokio::time::sleep(Duration::from_millis(500)).await;
            assert!(home.hooks().join("focus.json").exists());
            assert_eq!(migrated(&home), 0);
            assert_eq!(count(&home, "select count(*) from pomodoro_records"), 0);
            assert_eq!(block_status(&home), None);
        }
        runner.stop();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !native.tasks().is_empty() {
            assert!(Instant::now() < deadline, "el planificador no paró");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        native.shutdown().await;
    }
}

#[tokio::test]
async fn pomodoro_post_wakes_scheduler() {
    let home = TestHome::new_short("pomo-wake");
    let base = Arc::new(AtomicI64::new(NOW_MS));
    let clock = moving_clock(&base);
    let mut opts = home.options();
    opts.clock = clock.clone();
    opts.background = Background::front();
    let native = Arc::new(Native::new(opts));
    assert!(native.ready().await);
    let runner = background::start(&native);
    assert!(runner.pomodoro());
    let start = json!({"requestId": "w1", "expectedRevision": null, "action": "start",
                       "mode": "focus", "targetMs": 60_000});
    let (status, body) = pomodoro(&native, start).await;
    assert_eq!(status, 200, "{body}");
    let deadline = body["block"]["deadlineMs"].as_i64().unwrap();
    // El planificador ya recalculó con el bloque recién empezado: 60 s por
    // delante, así que duerme sus 30 s máximos.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(block_status(&home).as_deref(), Some("running"));
    // Faltan 2 s de reloj del temporizador. Una orden que falla (revisión
    // vieja → 409) también despierta al planificador (el `finally`). Sin el
    // despertar dormiría aún ~29,7 s: el margen de 10 s no se confunde con eso
    // y aguanta una máquina cargada.
    base.fetch_add(deadline - 2_000 - clock(), Ordering::SeqCst);
    let woke = Instant::now();
    let pause = json!({"requestId": "w2", "expectedRevision": 0, "action": "pause"});
    let (status, body) = pomodoro(&native, pause).await;
    assert_eq!(status, 409, "{body}");
    while block_status(&home).as_deref() != Some("completed") {
        assert!(
            woke.elapsed() < Duration::from_secs(10),
            "el bloque no se cerró a tiempo: el POST no despertó al planificador"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        woke.elapsed() >= Duration::from_millis(1900),
        "cerró antes de vencer"
    );
    assert_eq!(count(&home, "select count(*) from pomodoro_records"), 1);
    assert_eq!(
        count(
            &home,
            "select count(*) from events where kind='focus_completed'"
        ),
        1
    );
    runner.stop();
    native.shutdown().await;
}

#[tokio::test]
async fn two_schedulers_settle_once() {
    let home = TestHome::new_short("pomo-two");
    // Un bloque de foco de 1 min que venció hace 9 min, con la política de
    // foco activa desde antes (el cierre paga un premio). Además, lo que migra
    // el dueño (el Python, con `legacy`): un `focus.json` ya vencido (no se
    // adopta, pero se renombra) y dos bloques del historial.
    let now = wall_clock_ms();
    seed_legacy(&home, now / 1000 - 60);
    std::fs::create_dir_all(home.state_db().parent().unwrap()).unwrap();
    {
        let conn = comandos_store::state::connect(&home.state_db()).unwrap();
        comandos_store::state::migrate(&conn, comandos_store::state::MIGRATIONS, 0.0).unwrap();
        comandos_store::focus::ensure_policy(
            &conn,
            &comandos_core::focus::policy_v1(),
            now - 700_000,
        )
        .unwrap();
        let clock = || now - 600_000;
        let id = || "vencido".to_owned();
        let store = comandos_store::pomodoro::PomodoroStore::new(&conn, &clock, &id);
        store
            .command(
                &json!({"requestId": "seed", "expectedRevision": null, "action": "start",
                             "mode": "focus", "targetMs": 60_000}),
            )
            .unwrap();
    }
    assert_eq!(vencido(&home).as_deref(), Some("running"));
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    // Con el Python vivo (lo de por omisión en la 2f).
    opts.background = Background::legacy();
    let native = Arc::new(Native::new(opts));
    assert!(native.ready().await);
    // Los dos planificadores sobre la MISMA base (P51): el del frente y el
    // del `cc-dash` confinado, que arranca el suyo en `main()`.
    let (runner, py) = tokio::join!(
        async { background::start(&native) },
        oracle_with(&home, OracleOpts::default())
    );
    assert!(runner.pomodoro());
    let Some(py) = py else {
        runner.stop();
        native.shutdown().await;
        return;
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while vencido(&home).as_deref() != Some("completed") || migrated(&home) == 0 {
        assert!(Instant::now() < deadline, "nadie cerró el bloque");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Margen para que los dos hayan dado su primera vuelta y para el
    // reclamo del sonido (1 s después del evento).
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let event = "pomodoro:vencido:completed";
    assert_eq!(
        count(
            &home,
            "select count(*) from pomodoro_records where provenance='measured'"
        ),
        1
    );
    assert_eq!(
        count(
            &home,
            "select count(*) from pomodoro_records where provenance='legacy-planned'"
        ),
        2
    );
    assert_eq!(count(&home, "select count(*) from focus_rewards"), 1);
    assert!(!home.hooks().join("focus.json").exists());
    assert_eq!(migrated(&home), 1);
    assert_eq!(
        count(
            &home,
            &format!("select count(*) from events where event_id='{event}'")
        ),
        1
    );
    assert_eq!(
        count(
            &home,
            &format!("select count(*) from event_receipts where event_id='{event}'")
        ),
        1
    );
    assert_eq!(
        count(
            &home,
            &format!(
                "select count(*) from deliveries where event_id='{event}' and channel='sound'"
            )
        ),
        1
    );
    drop(py);
    runner.stop();
    native.shutdown().await;
}
