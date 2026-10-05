//! Kit 2f (Tarea 1 del maestro): procesos con stdin y sueltos, candado con
//! espera, censo de declinaciones, cortes desactivados y dueño de fondo.
//!
//! Confinamiento: ningún tmux; solo `/bin/cat`, `/bin/sh` y `/bin/sleep` por
//! ruta absoluta (nunca un nombre resuelto por el PATH del desarrollador), un
//! HOME temporal y un heredado falso. El censo se escribe solo en archivos del
//! HOME temporal: nunca en el `$XDG_RUNTIME_DIR` real.
mod support;

use comandos_server::dash::{
    build,
    native::{
        Background, Cut, NativeOptions,
        census::DeclineCensus,
        delete_body,
        files::FileLock,
        procs::{
            TaskTracker, gui_env_with, run_program_input, spawn_detached, spawn_detached_blocking,
            which_in,
        },
        tmux::{Program, RunError},
    },
    parse_args, parse_args_env,
};
use http::Method;
use std::{
    collections::BTreeSet,
    ffi::OsString,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};
use support::{FakeLegacy, TestHome, config, front, get, request_body};

const CAT: &str = "/bin/cat";
const SH: &str = "/bin/sh";
const SLEEP: &str = "/bin/sleep";

#[tokio::test]
async fn input_reaches_stdin_like_subprocess_run() {
    let out = run_program_input(
        &Program::named(CAT),
        &[],
        "a\r\nb".as_bytes(),
        Duration::from_secs(2),
    )
    .await
    .unwrap();
    assert!(out.ok);
    assert_eq!(out.stdout, "a\nb"); // saltos universales, como text=True
    assert_eq!(out.stderr, "");
}

/// `communicate()` del Python lee la salida mientras escribe la entrada: con
/// 1 MiB, escribir todo antes de leer llenaría la tubería y nunca acabaría.
#[tokio::test]
async fn input_and_output_flow_together() {
    let input = "x".repeat(1 << 20);
    let out = run_program_input(
        &Program::named(CAT),
        &[],
        input.as_bytes(),
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    assert_eq!(out.stdout.len(), 1 << 20);
}

#[tokio::test]
async fn input_timeout_kills_like_timeout_expired() {
    let started = Instant::now();
    let out = run_program_input(
        &Program::named(SLEEP),
        &["30"],
        b"",
        Duration::from_millis(200),
    )
    .await;
    assert!(matches!(out, Err(RunError::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// Hijos del hilo que llama (`/proc/thread-self/children`): con el runtime
/// `current_thread` de la prueba, los procesos los lanza este mismo hilo. Leer
/// el hilo principal del proceso daría siempre una lista vacía (B7).
fn own_children() -> Vec<i32> {
    std::fs::read_to_string("/proc/thread-self/children")
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|pid| pid.parse().ok())
        .collect()
}

fn is_zombie(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(") ")
                .map(|(_, rest)| rest.starts_with('Z'))
        })
        .unwrap_or(false)
}

fn cmdline(pid: i32) -> String {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|raw| String::from_utf8_lossy(&raw).replace('\0', " "))
        .unwrap_or_default()
}

/// Mata (con su pid exacto, hijo propio) los `sleep` que esta prueba lanzó.
fn kill_own_sleeps(children: &[i32]) {
    for pid in children {
        if cmdline(*pid).starts_with(SLEEP) {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(*pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

fn echo_args(mark: &Path) -> Vec<OsString> {
    vec![
        OsString::from("-c"),
        OsString::from(format!("echo x >> '{}'", mark.display())),
    ]
}

#[tokio::test]
async fn detached_children_are_reaped() {
    let home = TestHome::new("kit2f-detached");
    let mark = home.root.join("mark");
    for _ in 0..20 {
        spawn_detached(&Program::named(SH), &echo_args(&mark), &[]).unwrap();
    }
    spawn_detached(&Program::named(SLEEP), &[OsString::from("30")], &[]).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let children = own_children();
    let zombies: Vec<i32> = children.iter().copied().filter(|p| is_zombie(*p)).collect();
    // Solo el `sleep 30` sigue vivo y no retiene el runtime: la prueba ya
    // terminó de esperar con él corriendo.
    let alive = children.len();
    kill_own_sleeps(&children);
    assert_eq!(
        zombies,
        Vec::<i32>::new(),
        "hijos sin recoger: {children:?}"
    );
    assert_eq!(alive, 1, "hijos vivos: {children:?}");
    assert!(cmdline(children[0]).starts_with(SLEEP));
    let lines = std::fs::read_to_string(&mark).unwrap().lines().count();
    assert_eq!(lines, 20);
    // El `sleep` muerto también se recoge (no queda zombi del propio kill).
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(own_children().iter().all(|p| !is_zombie(*p)));
}

/// Desde un hilo de sistema (sin contexto de runtime) se lanza con el
/// `Handle`: lo recoge el mismo runtime, sin un hilo por hijo.
#[tokio::test]
async fn detached_from_a_system_thread_uses_the_runtime() {
    let home = TestHome::new("kit2f-detached-thread");
    let mark = home.root.join("mark");
    let handle = tokio::runtime::Handle::current();
    let (done, finished) = tokio::sync::oneshot::channel();
    let thread_mark = mark.clone();
    std::thread::spawn(move || {
        for _ in 0..5 {
            spawn_detached_blocking(&handle, &Program::named(SH), &echo_args(&thread_mark), &[])
                .unwrap();
        }
        // El runtime (hilo de la prueba) recoge mientras este hilo espera.
        std::thread::sleep(Duration::from_millis(500));
        let children = own_children();
        let zombies = children.iter().filter(|p| is_zombie(**p)).count();
        let _ = done.send((children.len(), zombies));
    });
    let (alive, zombies) = finished.await.unwrap();
    assert_eq!((alive, zombies), (0, 0));
    assert_eq!(std::fs::read_to_string(&mark).unwrap().lines().count(), 5);
}

#[test]
fn detached_outside_a_runtime_is_an_error_not_a_panic() {
    let err = spawn_detached(
        &Program::named(SH),
        &[OsString::from("-c"), "true".into()],
        &[],
    );
    assert!(err.is_err());
}

#[test]
fn detached_env_is_passed_to_the_child() {
    let home = TestHome::new("kit2f-detached-env");
    let mark = home.root.join("mark");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        spawn_detached(
            &Program::named(SH),
            &[
                OsString::from("-c"),
                OsString::from(format!("printf %s \"$KIT2F\" > '{}'", mark.display())),
            ],
            &[(OsString::from("KIT2F"), OsString::from("sí"))],
        )
        .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
    });
    assert_eq!(std::fs::read_to_string(&mark).unwrap(), "sí");
}

#[test]
fn gui_env_defaults_display_like_setdefault() {
    assert_eq!(
        gui_env_with(None),
        vec![(OsString::from("DISPLAY"), OsString::from(":1"))]
    );
    // `setdefault`: un DISPLAY presente, aunque esté vacío, no se toca.
    assert!(gui_env_with(Some(OsString::new())).is_empty());
    assert!(gui_env_with(Some(OsString::from(":0"))).is_empty());
}

#[test]
fn which_in_looks_only_at_the_given_path() {
    let home = TestHome::new("kit2f-which");
    let bin = home.root.join("bin");
    let tool = bin.join("kit2f-tool");
    std::fs::write(&tool, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(bin.join("kit2f-plain"), "").unwrap();
    let path = bin.clone().into_os_string();
    assert_eq!(which_in(Some(&path), "kit2f-tool"), Some(tool));
    assert_eq!(which_in(Some(&path), "kit2f-plain"), None);
    assert_eq!(which_in(Some(&path), "sh"), None);
    assert_eq!(which_in(Some(&OsString::new()), "kit2f-tool"), None);
}

#[tokio::test]
async fn task_tracker_counts_running_tasks_even_if_they_panic() {
    let tracker = TaskTracker::default();
    let (go, wait) = tokio::sync::oneshot::channel::<()>();
    tracker
        .spawn(async move {
            let _ = wait.await;
        })
        .unwrap();
    tracker
        .spawn(async {
            panic!("tarea que revienta a propósito");
        })
        .unwrap();
    assert_eq!(tracker.len(), 2);
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(tracker.len(), 1);
    let _ = go.send(());
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(tracker.is_empty());
}

#[test]
fn file_lock_acquire_waits_for_the_holder() {
    let home = TestHome::new("kit2f-lock");
    let target = home.hooks().join("app-tabs.json");
    let held = FileLock::try_acquire(&target).unwrap().unwrap();
    // Con el candado tomado, otro intento sin espera no lo consigue.
    assert!(FileLock::try_acquire(&target).unwrap().is_none());
    let started = Instant::now();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        drop(held);
    });
    let waited = FileLock::acquire(&target).unwrap();
    assert!(started.elapsed() >= Duration::from_millis(250));
    release.join().unwrap();
    assert!(FileLock::try_acquire(&target).unwrap().is_none());
    drop(waited);
    assert!(FileLock::try_acquire(&target).unwrap().is_some());
    let mode = std::fs::metadata(home.hooks().join("app-tabs.json.lock"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn task_tracker_outside_runtime_is_an_error_not_a_panic() {
    let tracker = TaskTracker::default();
    assert!(tracker.spawn(async {}).is_err());
    assert!(tracker.is_empty());
}

/// Revisión de T1: N peticiones esperando el mismo candado ocupan a lo sumo UN
/// hilo de bloqueo; el plazo vence sin abrir otro hilo; al soltarse el dueño
/// pasan todas en orden.
#[tokio::test]
async fn file_lock_waiters_share_one_blocking_thread_and_time_out() {
    let home = TestHome::new("kit2f-lockq");
    let target = home.hooks().join("snippets.json");
    let held = FileLock::try_acquire(&target).unwrap().unwrap();
    let mut waiters = Vec::new();
    for _ in 0..8 {
        let path = target.clone();
        waiters.push(tokio::spawn(async move {
            FileLock::acquire_timeout(&path, Duration::from_secs(10))
                .await
                .map(drop)
        }));
    }
    let started = Instant::now();
    while FileLock::blocked_waiters(&target) == 0 && started.elapsed() < Duration::from_secs(3) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(FileLock::blocked_waiters(&target), 1);
    let asked = Instant::now();
    let error = FileLock::acquire_timeout(&target, Duration::from_millis(200))
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(asked.elapsed() < Duration::from_secs(2));
    assert_eq!(FileLock::blocked_waiters(&target), 1);
    drop(held);
    for waiter in waiters {
        waiter.await.unwrap().unwrap();
    }
    assert_eq!(FileLock::blocked_waiters(&target), 0);
    assert!(FileLock::try_acquire(&target).unwrap().is_some());
}

#[test]
fn census_bounded_and_atomic() {
    let home = TestHome::new("kit2f-census");
    let census = DeclineCensus::default();
    for i in 0..2000 {
        census.note(&Method::GET, &format!("/ruta-{i}?q=1"));
    }
    census.note(&Method::GET, "/state?x=1");
    census.note(&Method::GET, "/state");
    let snap = census.snapshot();
    assert!(snap["since"].as_i64().unwrap() > 0);
    let counts = snap["counts"].as_object().unwrap();
    // 512 rutas con 1; las 1488 distintas siguientes y las dos de /state, a «(otras)».
    assert_eq!(counts.len(), 513);
    assert!(!counts.contains_key("GET /state"));
    assert_eq!(counts["(otras)"].as_u64(), Some(1490));
    assert_eq!(counts["GET /ruta-0"].as_u64(), Some(1));
    // Una ruta ya contada sigue sumando aunque el censo esté lleno.
    census.note(&Method::GET, "/ruta-0");
    let file = home.root.join("run/declines.json");
    census.flush(&file).unwrap();
    let back: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(back["counts"]["GET /ruta-0"].as_u64(), Some(2));
    assert_eq!(back["since"], snap["since"]);
    // Sin temporales sueltos junto al archivo (escritura por renombrado).
    let names: Vec<String> = std::fs::read_dir(home.root.join("run"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["declines.json".to_owned()]);
}

#[tokio::test]
async fn cuts_off_declines_whole_cut_before_effects() {
    let home = TestHome::new("kit2f-cuts");
    let legacy = FakeLegacy::start().await;
    let mut opts = home.options();
    opts.cuts_off.insert(Cut::Tabs);
    let fr = front(&home, legacy.port, opts).await;
    // Ruta del corte `tabs` aún sin entrada: igual se reenvía; la prueba fija el
    // contrato para cuando 2f-1/T2 la añada (ver dash_native_tabs.rs).
    let wire = request_body(
        fr.port,
        "POST",
        "/tab-register",
        "",
        r#"{"session":"s1","label":"x"}"#,
    )
    .await;
    assert_eq!(wire.text(), r#"{"legacy": true}"#);
    assert!(
        legacy
            .requests()
            .iter()
            .any(|line| line.starts_with("POST /tab-register "))
    );
    assert!(!home.hooks().join("app-tab-open.json").exists());
    fr.stop().await;
}

/// D3: `Cut::Base` (2b–2e) no se apaga por corte, aunque alguien lo meta.
#[tokio::test]
async fn base_routes_ignore_cuts_off() {
    let home = TestHome::new("kit2f-cuts-base");
    let legacy = FakeLegacy::start().await;
    let mut opts = home.options();
    opts.cuts_off.extend([
        Cut::Base,
        Cut::Tabs,
        Cut::Ops,
        Cut::Services,
        Cut::News,
        Cut::Residue,
    ]);
    let fr = front(&home, legacy.port, opts).await;
    let wire = get(fr.port, "/tabs").await;
    assert_eq!(wire.status, 200);
    assert_ne!(wire.text(), r#"{"legacy": true}"#);
    assert!(legacy.requests().is_empty());
    fr.stop().await;
}

/// Cada `Decline` del despachador cuenta; el apagado ordenado escribe el
/// censo en `census_path` (aquí un archivo del HOME temporal).
#[tokio::test]
async fn declines_are_counted_and_flushed_on_shutdown() {
    let home = TestHome::new("kit2f-census-flush");
    let legacy = FakeLegacy::start().await;
    let file = home.root.join("run/comandos-dash-declines.json");
    let mut opts = home.options();
    opts.census_path = Some(file.clone());
    let fr = front(&home, legacy.port, opts).await;
    // Sin `place: "sidebar"`, POST /terminal/quick declina antes de efectos.
    for _ in 0..2 {
        let wire = request_body(fr.port, "POST", "/terminal/quick", "", "{}").await;
        assert_eq!(wire.text(), r#"{"legacy": true}"#);
    }
    // Lo nativo que responde no cuenta.
    assert_eq!(get(fr.port, "/tabs").await.status, 200);
    fr.stop().await;
    let back: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(
        back["counts"],
        serde_json::json!({"POST /terminal/quick": 2})
    );
}

/// Con el conjunto nativo apagado (base ilegible) todo lo nativo se reenvía y
/// también cuenta en el censo.
#[tokio::test]
async fn declines_of_a_disabled_native_set_are_counted() {
    let home = TestHome::new("kit2f-census-off");
    let legacy = FakeLegacy::start().await;
    let file = home.root.join("run/declines.json");
    let mut opts = home.options();
    // Un directorio en lugar de la base: no abre y el conjunto se apaga.
    opts.state_db = home.root.clone();
    opts.census_path = Some(file.clone());
    let fr = front(&home, legacy.port, opts).await;
    assert_eq!(
        get(fr.port, "/tabs?a=1").await.text(),
        r#"{"legacy": true}"#
    );
    fr.stop().await;
    let back: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(back["counts"], serde_json::json!({"GET /tabs": 1}));
}

#[test]
fn background_presets() {
    let legacy = Background::legacy();
    assert!(legacy.pomodoro);
    assert!(
        !(legacy.model_watch
            || legacy.news
            || legacy.limits_snapshot
            || legacy.notices_push
            || legacy.webterm_restore)
    );
    let front = Background::front();
    assert!(
        front.pomodoro
            && front.model_watch
            && front.news
            && front.limits_snapshot
            && front.notices_push
            && front.webterm_restore
    );
    let home = TestHome::new("kit2f-bg-default");
    let opts = NativeOptions::for_home(&home.root, home.state_db());
    assert_eq!(opts.background, Background::legacy());
    assert!(opts.cuts_off.is_empty());
    assert_eq!(opts.census_path, None);
    assert_eq!(opts.webterm_health_ports, [4779, 4780]);
}

#[test]
fn flags_and_env_choose_background_and_cuts() {
    let home = Path::new("/home/kit2f");
    let args = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
    let cfg = parse_args(&args(&[]), home, None).unwrap();
    assert_eq!(cfg.background, None);
    assert!(cfg.cuts_off.is_empty());
    assert_eq!(cfg.census_path, None);

    let cfg = parse_args(
        &args(&["--background=front", "--cuts-off=tabs, ops"]),
        home,
        None,
    )
    .unwrap();
    assert_eq!(cfg.background, Some(Background::front()));
    assert_eq!(cfg.cuts_off, BTreeSet::from([Cut::Tabs, Cut::Ops]));
    let cfg = parse_args(
        &args(&["--background", "legacy", "--cuts-off="]),
        home,
        None,
    )
    .unwrap();
    assert_eq!(cfg.background, Some(Background::legacy()));
    assert!(cfg.cuts_off.is_empty());

    assert!(parse_args(&args(&["--background=todo"]), home, None).is_err());
    assert!(parse_args(&args(&["--background"]), home, None).is_err());
    assert!(parse_args(&args(&["--cuts-off=tabs,base"]), home, None).is_err());
    assert!(parse_args(&args(&["--cuts-off=noticias"]), home, None).is_err());

    // Sin bandera manda el entorno, con la misma validación.
    let cfg = parse_args_env(&args(&[]), home, None, Some("front"), Some("news,residue")).unwrap();
    assert_eq!(cfg.background, Some(Background::front()));
    assert_eq!(cfg.cuts_off, BTreeSet::from([Cut::News, Cut::Residue]));
    let cfg = parse_args_env(&args(&[]), home, None, Some(""), Some("")).unwrap();
    assert_eq!(cfg.background, None);
    assert!(cfg.cuts_off.is_empty());
    assert!(parse_args_env(&args(&[]), home, None, Some("x"), None).is_err());
    assert!(parse_args_env(&args(&[]), home, None, None, Some("tabs,x")).is_err());
    // La bandera gana al entorno (también una lista vacía explícita).
    let cfg = parse_args_env(
        &args(&["--background=legacy", "--cuts-off="]),
        home,
        None,
        Some("front"),
        Some("tabs"),
    )
    .unwrap();
    assert_eq!(cfg.background, Some(Background::legacy()));
    assert!(cfg.cuts_off.is_empty());
}

/// B4: `build` une los cortes del arranque con los inyectados y solo pisa el
/// dueño de fondo si vino explícito; el censo inyectado se respeta.
#[test]
fn build_keeps_injected_cuts_background_and_census() {
    let home = TestHome::new("kit2f-build");
    let mut cfg = config(&home, support::dead_port());
    cfg.cuts_off.insert(Cut::Ops);
    cfg.census_path = Some(home.root.join("desde-config.json"));
    let mut opts = home.options();
    opts.cuts_off.insert(Cut::Tabs);
    opts.background = Background::front();
    let injected = home.root.join("inyectado.json");
    opts.census_path = Some(injected.clone());
    let (_, native) = build(cfg.clone(), Some(opts));
    let native = native.unwrap();
    let o = native.options();
    assert_eq!(o.cuts_off, BTreeSet::from([Cut::Tabs, Cut::Ops]));
    assert_eq!(o.background, Background::front());
    assert_eq!(o.census_path.as_deref(), Some(injected.as_path()));

    cfg.background = Some(Background::legacy());
    let mut opts = home.options();
    opts.background = Background::front();
    let (_, native) = build(cfg.clone(), Some(opts));
    let native = native.unwrap();
    assert_eq!(native.options().background, Background::legacy());
    // Sin censo inyectado, el de la configuración.
    assert_eq!(
        native.options().census_path.as_deref(),
        Some(home.root.join("desde-config.json").as_path())
    );
    assert_eq!(native.tasks().len(), 0);
    assert!(
        native.census().snapshot()["counts"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn delete_body_is_the_admitted_object() {
    let request = |data: Option<serde_json::Value>| comandos_server::Request {
        method: Method::DELETE,
        target: "/push/subscription".into(),
        peer: "127.0.0.1:9".parse().unwrap(),
        headers: Vec::new(),
        data,
        body: bytes::Bytes::new(),
        internal_producer: false,
    };
    let with = request(Some(serde_json::json!({"endpoint": "x"})));
    assert_eq!(delete_body(&with).ok().unwrap()["endpoint"], "x");
    assert!(delete_body(&request(None)).is_err());
    assert!(delete_body(&request(Some(serde_json::json!([1])))).is_err());
}

/// El transporte ya aplica la puerta de `do_DELETE` antes del manejador
/// (64 000 → 413, JSON roto → 400, no-objeto → 400; los límites los prueba
/// `transport.rs`): `delete_body` solo recibe objetos admitidos.
#[tokio::test]
async fn delete_gate_matches_do_delete() {
    let home = TestHome::new("kit2f-delete-gate");
    let legacy = FakeLegacy::start().await;
    let fr = front(&home, legacy.port, home.options()).await;
    let wire = request_body(fr.port, "DELETE", "/push/subscription", "", "[1]").await;
    assert_eq!(wire.status, 400);
    assert_eq!(
        wire.text(),
        r#"{"error": "El cuerpo debe ser un objeto JSON"}"#
    );
    let wire = request_body(fr.port, "DELETE", "/push/subscription", "", "{").await;
    assert_eq!(wire.status, 400);
    assert_eq!(wire.text(), r#"{"error": "JSON invalido"}"#);
    assert!(legacy.requests().is_empty());
    fr.stop().await;
}

/// Ronda 1 de la 2f-1/T2: `spawn_handle` cuenta la tarea mientras corre (también
/// si nadie espera su `JoinHandle`) y devuelve su resultado.
#[tokio::test]
async fn task_tracker_spawn_handle_counts_and_returns() {
    let tracker = TaskTracker::default();
    let (go, wait) = tokio::sync::oneshot::channel::<()>();
    let job = tracker
        .spawn_handle(async move {
            let _ = wait.await;
            7
        })
        .unwrap();
    assert_eq!(tracker.len(), 1);
    let _ = go.send(());
    assert_eq!(job.await.unwrap(), 7);
    assert_eq!(tracker.len(), 0);
    // Soltar el `JoinHandle` no cancela la tarea: sigue contada hasta terminar.
    let (go, wait) = tokio::sync::oneshot::channel::<()>();
    drop(
        tracker
            .spawn_handle(async move {
                let _ = wait.await;
            })
            .unwrap(),
    );
    assert_eq!(tracker.len(), 1);
    let _ = go.send(());
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(tracker.len(), 0);
}
