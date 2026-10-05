//! Motor de GET /usage/state (latente, Tarea 6 de la 2e): bytes contra el
//! oráculo, memo de un solo cómputo, declinar con una base más nueva y
//! paneles vivos con su raíz git y `record_pane`.
mod support;
use comandos_server::dash::native::{
    Fault, Native, NativeOptions,
    tmux::{Program, Tmux},
    usage::state,
    wall_clock_ms,
};
use std::{
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};
use support::{TestHome, get, oracle::oracle, seed_usage, tmux_available};

/// Los campos de `enrich_limits` que dependen del `time.time()` de cada lado:
/// se comparan con tolerancia relativa en vez de borrarse.
const TIMED: [&str; 3] = ["pace", "burn", "runsOutIn"];
/// Tolerancia relativa de `TIMED` (los dos relojes distan poco: `pace` y
/// `runsOutIn` cambian ~1e-5 relativo por segundo con ventanas de horas).
const TOLERANCE: f64 = 1e-3;

/// Lo volátil entre el frente y el Python (dos `time.time()` distintos):
/// `generated_at`, `last_seen_at`, `captured_at` de los límites y
/// `started_at` solo de los paneles vivos (`live`, por sesión; el de un panel
/// guardado viene de la base y debe coincidir). Devuelve el cuerpo enmascarado
/// y, aparte, los valores de `TIMED` por fila de límites.
fn mask(text: &str, live: &[&str]) -> (String, Vec<Vec<Option<f64>>>) {
    let mut v: serde_json::Value = serde_json::from_str(text).unwrap();
    v["generated_at"] = serde_json::json!(0);
    for p in v["panes"].as_array_mut().into_iter().flatten() {
        p["last_seen_at"] = serde_json::json!(0);
        let session = p["tmux_session"].as_str().unwrap_or_default();
        if live.contains(&session) {
            p["started_at"] = serde_json::json!(0);
        }
    }
    let mut timed = Vec::new();
    for l in v["limits"].as_array_mut().into_iter().flatten() {
        if l.get("captured_at").is_some_and(|x| !x.is_null()) {
            l["captured_at"] = serde_json::json!(0);
        }
        let mut row = Vec::new();
        for key in TIMED {
            row.push(l.get(key).and_then(serde_json::Value::as_f64));
            if l.get(key).is_some_and(|x| !x.is_null()) {
                l[key] = serde_json::json!(0);
            }
        }
        timed.push(row);
    }
    (comandos_core::json::response_dumps(&v).unwrap(), timed)
}

/// Cuerpos iguales salvo lo volátil, y `TIMED` dentro de `TOLERANCE`.
fn assert_same_body(ours: &[u8], theirs: &str, live: &[&str]) {
    let (ours, ours_timed) = mask(std::str::from_utf8(ours).unwrap(), live);
    let (theirs, theirs_timed) = mask(theirs, live);
    assert_eq!(ours, theirs);
    for (a, b) in ours_timed
        .iter()
        .flatten()
        .zip(theirs_timed.iter().flatten())
    {
        match (a, b) {
            (Some(a), Some(b)) => {
                let scale = a.abs().max(b.abs()).max(1.0);
                assert!((a - b).abs() <= TOLERANCE * scale, "{a} frente a {b}");
            }
            (a, b) => assert_eq!(a, b),
        }
    }
}

/// `tmux` que siempre sale con 1 (sin servidor): ninguna prueba sin paneles
/// alcanza un tmux de verdad, ni siquiera el privado.
fn no_tmux(opts: &mut NativeOptions) {
    opts.tmux = Tmux {
        program: Program::named("/bin/false"),
        timeout: Duration::from_secs(5),
    };
}

/// La caché de límites cargada como la del Python vivo (su refresco de
/// arranque): sin credenciales en el HOME no hay red (OAuth falso de todos modos).
async fn warm_limits(native: &Native) {
    assert!(
        native
            .limits()
            .get_loaded(&native.refresh_deps(), Duration::from_secs(5))
            .await
            .is_some()
    );
}

fn seed_basic(home: &TestHome, now: i64) {
    seed_usage(
        home,
        &format!(
            "insert into usage_panes(tmux_session,tmux_pane,pane_pwd,git_root,agent,provider,started_at,last_seen_at,raw) \
             values('s1','%1','/r/a','/r/a','codex','codex',{a},{b},'{{}}');\
             insert into usage_turns(id,provider,agent,tmux_session,tmux_pane,pane_pwd,git_root,model,turn_started_at,\
             turn_finished_at,total_tokens,cost_usd,source,confidence,raw) values('t1','codex','codex','s1','%1','/r/a','/r/a',\
             'gpt-5.6-sol',{c},{c},420,0.0,'codex_rollout','measured','{{}}');\
             insert into usage_interactions(id,tmux_session,tmux_pane,started_at_ms,finished_at_ms,duration_ms,\
             completion_status,source,confidence,created_at) values('i1','s1','%1',{c}000,{c}500,500,'ok','hook','measured',{c});\
             insert into usage_alerts(id,kind,level,message,provider,created_at,last_seen_at,raw) values('a1','tokens','warn','aviso','codex',{c},{c},'{{}}');\
             insert into usage_settings(key,value) values('COMANDOS_CODEX_WEEKLY_TOKEN_LIMIT','1000');",
            a = now - 500,
            b = now - 5,
            c = now - 30
        ),
    );
}

#[tokio::test]
async fn usage_state_body_matches_python_without_live_panes() {
    let home = TestHome::new("ustate");
    let now = wall_clock_ms() / 1000;
    seed_basic(&home, now);
    home.write(
        "cc-notify.conf",
        "CC_LANG=es\nCOMANDOS_DAILY_BUDGET_USD='3'\n",
    );
    home.write(
        "usage.env",
        "export COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT=\"500\"\n",
    );
    // Una fila de límite de Codex con reinicios futuros: `attach_token_counts`
    // le añade los tokens de las ventanas y `enrich_limits` la enriquece.
    let rollout = home.root.join(".codex/sessions/2026/10/05/rollout-x.jsonl");
    std::fs::create_dir_all(rollout.parent().unwrap()).unwrap();
    std::fs::write(
        &rollout,
        format!(
            "{{\"timestamp\":\"2026-10-05T11:45:00Z\",\"payload\":{{\"rate_limits\":{{\"primary\":             {{\"used_percent\":20,\"window_minutes\":300,\"resets_at\":{p}}},\"secondary\":             {{\"used_percent\":60.06,\"window_minutes\":10080,\"resets_at\":{s}}},\"plan_type\":\"pro\"}}}}}}\n",
            p = now + 3_600,
            s = now + 400_000
        ),
    )
    .unwrap();
    let Some(py) = oracle(&home).await else {
        return;
    };
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    no_tmux(&mut opts);
    let native = Native::new(opts);
    warm_limits(&native).await;
    let ours = state::compute(&native).await.ok().unwrap();
    let theirs = get(py.port, "/usage/state").await;
    assert_eq!(theirs.status, 200, "{}", theirs.text());
    assert_same_body(&ours.body.to_vec(), &theirs.text(), &[]);
    assert!(ours.live_panes.is_empty());
    assert!(ours.tmux_panes.is_none(), "list-panes falló: no se sabe");
    let body: serde_json::Value = serde_json::from_slice(&ours.body.to_vec()).unwrap();
    let codex = body["limits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["provider"] == "codex")
        .unwrap_or_else(|| panic!("sin fila de Codex: {body}"));
    assert_eq!(codex["tokens_7d"], 420, "{codex}");
    assert!(codex.get("verdict").is_some(), "{codex}");
}

#[tokio::test]
async fn usage_state_memo_single_flight() {
    let home = TestHome::new("ustate-flight");
    seed_usage(&home, "");
    let mut opts = home.options();
    opts.usage_effects = false;
    no_tmux(&mut opts);
    let native = Arc::new(Native::new(opts));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let n = native.clone();
        tasks.push(tokio::spawn(async move {
            state::compute(&n).await.ok().map(|r| r.state)
        }));
    }
    let mut states = Vec::new();
    for t in tasks {
        states.push(t.await.unwrap().unwrap());
    }
    assert!(
        states.windows(2).all(|w| Arc::ptr_eq(&w[0], &w[1])),
        "un solo cálculo, el mismo Arc"
    );
    native.usage_engine.bump();
    let after = state::compute(&native).await.ok().unwrap().state;
    assert!(
        !Arc::ptr_eq(&after, &states[0]),
        "otra generación: memo nuevo"
    );
}

#[tokio::test]
async fn usage_state_does_not_wait_for_the_state_scan() {
    // I2 de la revisión de T6: con las cachés de `/state` tomadas (su escaneo
    // de `/proc` en curso), `/usage/state` carga el registro con su propio
    // candado corto y termina; antes aparcaba un hilo del pool en ese candado.
    let home = TestHome::new("ustate-unparked");
    seed_usage(&home, "");
    let mut opts = home.options();
    opts.usage_effects = false;
    no_tmux(&mut opts);
    let native = Native::new(opts);
    let caches = native.state_caches().clone();
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let _guard = caches.lock().unwrap_or_else(|p| p.into_inner());
        held_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(30));
    });
    held_rx.recv().unwrap();
    let done = tokio::time::timeout(Duration::from_secs(10), state::compute(&native)).await;
    release_tx.send(()).unwrap();
    holder.join().unwrap();
    assert!(
        matches!(done, Ok(Ok(_))),
        "/usage/state esperó al escaneo de /state"
    );
    native.shutdown().await;
}

#[tokio::test]
async fn usage_state_newer_schema_declines() {
    let home = TestHome::new("ustate-newer");
    seed_usage(&home, "pragma user_version = 12;");
    let mut opts = home.options();
    no_tmux(&mut opts);
    let native = Native::new(opts);
    assert!(matches!(state::compute(&native).await, Err(Fault::Decline)));
}

#[tokio::test]
async fn usage_state_undecodable_conf_is_500() {
    let home = TestHome::new("ustate-conf");
    seed_usage(&home, "");
    std::fs::write(home.hooks().join("usage.env"), b"A=\xff\n").unwrap();
    let mut opts = home.options();
    no_tmux(&mut opts);
    let native = Native::new(opts);
    assert!(matches!(
        state::compute(&native).await,
        Err(Fault::Error(comandos_server::HandlerError::Failure))
    ));
}

#[tokio::test]
async fn git_root_for_path_falls_back_to_path() {
    assert_eq!(state::git_root_for_path(Path::new("")).await, "");
    let missing = Path::new("/no-existe/ustate");
    assert_eq!(state::git_root_for_path(missing).await, "/no-existe/ustate");
}

/// `git init` en `dir` con un HOME temporal (sin la configuración real).
fn git_init(home: &TestHome, dir: &Path) -> bool {
    std::fs::create_dir_all(dir).unwrap();
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .env("HOME", &home.root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Un pane del tmux privado (`-S`) con un agente falso que corre en `dir`.
fn agent_pane(home: &TestHome, session: &str, dir: &Path, agent: &Path) {
    let status = home
        .tmux_command()
        .args(["new-session", "-d", "-s", session, "-c"])
        .arg(dir)
        .arg(format!(
            "env -i HOME={} PATH=/usr/bin:/bin {} 600",
            home.root.display(),
            agent.display()
        ))
        .status()
        .unwrap();
    assert!(status.success());
}

async fn pane_rows(native: &Native) -> i64 {
    native
        .usage_lane()
        .with(|u| {
            u.conn
                .query_row("select count(*) from usage_panes", [], |r| r.get(0))
                .unwrap_or(-1)
        })
        .await
        .ok()
        .unwrap()
}

/// `record_pane` va en un trabajo del carril que la respuesta no espera:
/// las filas se miran hasta que lleguen a `want` (o 5 s).
async fn settled_pane_rows(native: &Native, want: i64) -> i64 {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let rows = pane_rows(native).await;
        if rows == want || std::time::Instant::now() > deadline {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn usage_state_live_pane_records_and_git_root() {
    let home = TestHome::new("ustate-live");
    if !tmux_available() {
        eprintln!("tmux no está: se salta");
        return;
    }
    let repo = home.root.join("proj");
    if !git_init(&home, &repo) {
        eprintln!("git no está: se salta");
        return;
    }
    seed_usage(&home, "");
    let codex = support::fake_agent(&home, "codex");
    agent_pane(&home, "cx", &repo, &codex);
    tokio::time::sleep(Duration::from_millis(300)).await;
    for effects in [false, true] {
        let mut opts = home.options();
        opts.usage_effects = effects;
        let native = Native::new(opts);
        let reply = state::compute(&native).await.ok().unwrap();
        let body: serde_json::Value = serde_json::from_slice(&reply.body.to_vec()).unwrap();
        let panes = body["panes"].as_array().unwrap();
        assert_eq!(panes.len(), 1, "{body}");
        assert_eq!(panes[0]["git_root"], repo.display().to_string());
        assert_eq!(panes[0]["tmux_session"], "cx");
        assert_eq!(panes[0]["agent"], "codex");
        assert_eq!(reply.live_panes.len(), 1);
        // Todos los panes de tmux para acotar los avisos de nivel (Tarea 7b).
        let tmux_panes = reply.tmux_panes.as_ref().unwrap();
        assert!(tmux_panes.contains(reply.live_panes[0]["tmux_pane"].as_str().unwrap()));
        // Sin efectos ni siquiera se lanza el trabajo de registro.
        assert_eq!(
            settled_pane_rows(&native, i64::from(effects)).await,
            i64::from(effects)
        );
        native.shutdown().await;
    }
}

#[tokio::test]
async fn usage_state_live_pane_matches_python() {
    let home = TestHome::new("ustate-livepy");
    if !tmux_available() {
        eprintln!("tmux no está: se salta");
        return;
    }
    let repo = home.root.join("proj");
    if !git_init(&home, &repo) {
        eprintln!("git no está: se salta");
        return;
    }
    let now = wall_clock_ms() / 1000;
    seed_basic(&home, now);
    let codex = support::fake_agent(&home, "codex");
    agent_pane(&home, "cx", &repo, &codex);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let Some(py) = oracle(&home).await else {
        return;
    };
    let mut opts = home.options();
    opts.clock = Arc::new(wall_clock_ms);
    let native = Native::new(opts);
    warm_limits(&native).await;
    let ours = state::compute(&native).await.ok().unwrap();
    let theirs = get(py.port, "/usage/state").await;
    assert_eq!(theirs.status, 200, "{}", theirs.text());
    let bytes = ours.body.to_vec();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.contains("\"cx\""), "{text}");
    assert_same_body(&bytes, &theirs.text(), &["cx"]);
}

/// Tiempos de un cómputo con 5000 turnos en la ventana: sin memo (un salto de
/// bloqueo más, el ensamblado) y con memo. Solo se imprimen; la cota es holgada.
#[tokio::test]
async fn usage_state_timing_with_and_without_memo() {
    let home = TestHome::new("ustate-time");
    let now = support::NOW_MS / 1000;
    let mut sql = String::from("begin;");
    for i in 0..5000 {
        sql.push_str(&format!(
            "insert into usage_turns(id,provider,agent,tmux_session,tmux_pane,pane_pwd,git_root,model,\
             turn_started_at,turn_finished_at,total_tokens,cost_usd,source,confidence,raw) values \
             ('t{i}','codex','codex','s{s}','%{s}','/r/{p}','/r/{p}','gpt-5.6-sol',{t},{t},{i},0.01,'hook','measured','{{}}');",
            s = i % 7,
            p = i % 13,
            t = now - (i as i64 % 1_000_000)
        ));
    }
    sql.push_str("commit;");
    seed_usage(&home, &sql);
    let mut opts = home.options();
    opts.usage_effects = false;
    no_tmux(&mut opts);
    let native = Native::new(opts);
    let started = std::time::Instant::now();
    let miss = state::compute(&native).await.ok().unwrap();
    let miss_ms = started.elapsed();
    let started = std::time::Instant::now();
    let hit = state::compute(&native).await.ok().unwrap();
    let hit_ms = started.elapsed();
    eprintln!(
        "usage/state: sin memo {miss_ms:?} ({} bytes), con memo {hit_ms:?}",
        miss.body.len()
    );
    assert!(Arc::ptr_eq(&miss.state, &hit.state));
    assert!(hit_ms < Duration::from_secs(2));
}
