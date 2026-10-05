//! Contexto de sugerencias sin el heredado (Tarea 5a de la 2e): la guardia
//! con pronóstico (`token_guard_with_forecast`, bin/cc-dash) y la latencia de
//! `experiment_analytics` contra el Python de este checkout, y los casos en
//! que el frente declina en vez de adivinar.
mod support;
use comandos_core::{
    json::{response_dumps, workspace_loads},
    usage_state::UsageError,
};
use comandos_server::dash::native::{Fault, Native, usage::guard, usage::limits::LimitsCache};
use serde_json::{Map, Value, json};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use support::{FakeAnswer, FakeOauth, NOW_MS, TestHome, oracle::run_python, seed_usage};

const NOW: f64 = NOW_MS as f64 / 1000.0;

/// Cada caso en su propia llamada: una excepción no tapa las demás. El
/// informe local es fijo (`{"r": 1}`); solo cambian las filas de límites.
const FORECASTS: &str = r#"
import importlib.machinery, importlib.util, json, os, sys, time
repo, now, cases = sys.argv[1], float(sys.argv[2]), json.loads(sys.argv[3])
os.environ["TZ"] = "America/Mexico_City"
time.tzset()
time.time = lambda: now
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
dash.cc_usage.token_guard_report = lambda db, now=None: {"r": 1}
for rows in cases:
    dash.usage_provider_limits = lambda force=False, rows=rows: {"limits": rows, "health": {}}
    try:
        print(json.dumps(dash.token_guard_with_forecast()))
    except Exception as e:
        print(json.dumps({"raise": type(e).__name__}))
"#;

/// Refresco real con OAuth falso, guardia y latencia sobre la base sembrada.
const GUARD: &str = r#"
import importlib.machinery, importlib.util, json, os, sys, time
repo, home, now = sys.argv[1], sys.argv[2], float(sys.argv[3])
os.environ["TZ"] = "America/Mexico_City"
time.tzset()
time.time = lambda: now
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
answers = json.load(open(os.path.join(home, "oauth-case.json")))
def fake(url, headers, timeout=8):
    ans = answers.get(headers["Authorization"][len("Bearer "):])
    if ans is None:
        raise Exception("sin guion")
    return ans
dash.cc_usage._http_json = fake
dash._refresh_provider_limits()
guard = dash.token_guard_with_forecast()
stats = dash.cc_usage.experiment_analytics(dash.USAGE_DB, 7)
latency = [[c.get("model"), c.get("effort"), c.get("durationP50Ms"), c.get("attempts")]
           for c in stats.get("configurations", []) if c.get("durationP50Ms")]
print(json.dumps({"guard": guard, "latency": latency}))
"#;

fn rows(value: Value) -> Vec<Map<String, Value>> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_object().cloned().unwrap())
        .collect()
}

fn dumps(text: &str) -> String {
    response_dumps(&workspace_loads(text).unwrap()).unwrap()
}

fn row(percent: Value, resets_at: Value) -> Value {
    json!({"provider": "claude", "window": "7d", "percent": percent, "resets_at": resets_at})
}

#[test]
fn forecasts_match_python_rules() {
    let day = 86_400.0;
    let mut scoped = row(json!(90), json!(NOW + 5.0 * day));
    scoped["scope"] = json!("Fable");
    let mut general = row(json!(80), json!(NOW + day));
    general["scope"] = json!("");
    let cases: Vec<(Value, bool)> = vec![
        // (filas, el frente declina porque el Python da algo que no reproduce)
        (json!([general.clone()]), false),
        (json!([scoped.clone(), general.clone()]), false),
        // Aviso: `downtime > 0` pero < 24 s (la comparación es en segundos).
        (json!([row(json!(50), json!(NOW + 302_405.0))]), false),
        // `round(2.5)` = 2 y `round(3.5)` = 4 (mitad al par).
        (json!([row(json!(50), json!(NOW + 306_900.0))]), false),
        (json!([row(json!(50), json!(NOW + 308_700.0))]), false),
        // Filtros: otra cuenta, otra ventana, otro proveedor; cuenta vacía = main.
        (
            json!([
                {"provider": "claude", "account": "relotto", "window": "7d", "percent": 99},
                {"provider": "claude", "window": "5h", "percent": 50},
                {"provider": "codex", "window": "7d", "percent": 50},
                {"window": "7d", "percent": 50},
                {"provider": "claude", "account": "", "window": "7d", "percent": 10, "resets_at": NOW + day},
                {"provider": "claude", "account": 0, "window": "7d", "percent": 10, "resets_at": NOW + day},
                {"provider": "claude", "account": "main", "window": "7d", "percent": 10, "resets_at": NOW + day}
            ]),
            false,
        ),
        // `float()` de texto, bool, vacíos y contenedores.
        (json!([row(json!("45.5"), json!(NOW + day))]), false),
        (json!([row(json!(" 1_0 "), json!(NOW + day))]), false),
        (json!([row(json!(""), json!(NOW + day))]), false),
        (json!([row(json!(null), json!(NOW + day))]), false),
        (json!([row(json!(true), json!(NOW + day))]), false),
        (json!([row(json!(false), json!(NOW + day))]), false),
        (json!([row(json!([]), json!(NOW + day))]), false),
        (json!([row(json!("abc"), json!(NOW + day))]), false),
        (json!([row(json!([1]), json!(NOW + day))]), false),
        (json!([row(json!({"a": 1}), json!(NOW + day))]), false),
        (json!([row(json!(30), json!("1791200000"))]), false),
        (
            json!([row(json!(30), json!("2026-10-06T00:00:00+00:00"))]),
            false,
        ),
        (json!([row(json!(30), json!(null))]), false),
        (json!([row(json!(0), json!(NOW + day))]), false),
        (json!([row(json!(-5), json!(NOW + day))]), false),
        (json!([row(json!(100), json!(NOW + day))]), false),
        (json!([row(json!(150), json!(NOW + 3.0 * day))]), false),
        (json!([row(json!(40), json!(NOW - day))]), false),
        // No finitos: `int(inf)`/`int(nan)` lanzan; `nan`/`-inf` en el
        // porcentaje no llegan a ningún `int()` (B10 solo cubre los `int()`).
        (json!([row(json!("inf"), json!(NOW + day))]), false),
        (json!([row(json!("nan"), json!(NOW + day))]), false),
        (json!([row(json!("-inf"), json!(NOW + day))]), false),
        (json!([row(json!(30), json!("inf"))]), false),
        (json!([row(json!(30), json!("-inf"))]), false),
        (json!([row(json!(30), json!("nan"))]), false),
        // Alcances: `str()` de números y bool; el de una lista no se reproduce.
        (
            json!([
                {"provider": "claude", "window": "7d", "percent": 10, "resets_at": NOW + day, "scope": 5},
                {"provider": "claude", "window": "7d", "percent": 10, "resets_at": NOW + day, "scope": true},
                {"provider": "claude", "window": "7d", "percent": 10, "resets_at": NOW + day, "scope": 1.5},
                {"provider": "claude", "window": "7d", "percent": 10, "resets_at": NOW + day, "scope": null}
            ]),
            false,
        ),
        (
            json!([{"provider": "claude", "window": "7d", "percent": 10, "resets_at": NOW + day, "scope": ["x"]}]),
            true,
        ),
        // Enteros de Python sin cota: el frente declina.
        (json!([row(json!(30), json!(1e300))]), true),
        (
            json!([row(
                workspace_loads("1000000000000000000000000000000").unwrap(),
                json!(NOW + day)
            )]),
            true,
        ),
        (json!([]), false),
    ];
    let encoded = Value::Array(cases.iter().map(|(rows, _)| rows.clone()).collect()).to_string();
    let Some(out) = run_python(
        FORECASTS,
        &[NOW.to_string().as_ref(), std::ffi::OsStr::new(&encoded)],
        &TestHome::new("guard-forecasts").root,
    ) else {
        return;
    };
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), cases.len(), "{out}");
    let mut raised = 0;
    for ((case, unsure), line) in cases.iter().zip(lines) {
        let python = workspace_loads(line).unwrap();
        let mut report = Map::new();
        report.insert("r".into(), json!(1));
        match guard::with_forecasts(report, &rows(case.clone()), NOW) {
            Ok(got) => {
                assert!(!unsure, "{case}");
                assert_eq!(response_dumps(&got).unwrap(), dumps(line), "{case}");
            }
            Err(UsageError::Raises | UsageError::Overflow) => {
                assert!(python.get("raise").is_some(), "{case}: Python dio {line}");
                raised += 1;
            }
            Err(UsageError::Unsure) => {
                assert!(*unsure, "{case}: Python dio {line}");
                assert!(python.get("raise").is_none(), "{case}");
            }
        }
    }
    assert!(raised >= 6, "faltan casos de excepción: {raised}");
    // El API de forma corta: `None` = la excepción del Python.
    assert!(guard::forecasts(&rows(json!([row(json!("abc"), json!(1))])), NOW).is_none());
    assert_eq!(
        guard::forecasts(&rows(json!([general])), NOW)
            .unwrap()
            .len(),
        1
    );
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else if entry.file_type().unwrap().is_file() {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn turn(id: &str, root: &str, model: &str, finished: i64, tokens: i64, raw: &str) -> String {
    format!(
        "insert into usage_turns (id, provider, agent, tmux_session, tmux_pane, pane_pwd, git_root, \
         model, turn_started_at, turn_finished_at, total_tokens, source, confidence, raw) values \
         ('{id}', 'claude', 'claude', 's', '%1', '{root}', '{root}', '{model}', {s}, {finished}, \
         {tokens}, 'claude_jsonl', 'exact', '{raw}');",
        s = finished - 30
    )
}

/// Turnos de Claude para la guardia (dos proyectos, subagentes, uno viejo y
/// uno de otro origen) y configuraciones con duraciones para la latencia.
fn seed_guard_home(home: &TestHome) {
    let now = NOW_MS / 1000;
    let ms = NOW_MS;
    let mut sql = [
        turn(
            "t1",
            "/w/Proyecto",
            "claude-opus-5",
            now - 60,
            1000,
            r#"{"path": "/x/subagents/a.jsonl"}"#,
        ),
        turn(
            "t2",
            "/w/Proyecto",
            "claude-opus-5",
            now - 120,
            2000,
            r#"{"path": "/x/subagents/b.jsonl"}"#,
        ),
        turn(
            "t3",
            "/w/Proyecto",
            "claude-sonnet-5",
            now - 1200,
            500,
            "{}",
        ),
        turn("t4", "", "claude-haiku-5", now - 30, 50, "no-json"),
        turn("t5", "/w/Viejo", "claude-opus-5", now - 7200, 9, "{}"),
        turn("t6", "/w/Otro", "gpt-5", now - 60, 9, "{}"),
    ]
    .concat();
    sql.push_str(&format!(
        "insert into usage_session_configs (id, tmux_session, tmux_pane, effective_at, harness, motor, \
         model, effort, route_id, source, confidence) values \
         ('c1', 's', '%1', {now}, 'claude', 'codex', 'gpt-5.6-luna', 'low', 'claude:codex', 'hook', 'exact'), \
         ('c2', 's', '%1', {now}, 'claude', 'claude', 'claude-sonnet-5', 'low', 'claude:claude', 'hook', 'exact'), \
         ('c3', 's', '%1', {now}, 'grok', 'grok', 'grok-4.5', 'low', 'grok:grok', 'hook', 'exact');"
    ));
    for (i, (config, duration, age_ms)) in [
        ("c1", "4000", 3_600_000),
        ("c1", "6000", 7_200_000),
        ("c1", "5000", 86_400_000),
        ("c2", "12000", 60_000),
        ("c2", "null", 60_000),
        ("c3", "null", 60_000),
        ("c1", "999999", 9 * 86_400_000),
    ]
    .into_iter()
    .enumerate()
    {
        let finished = ms - age_ms;
        sql.push_str(&format!(
            "insert into usage_interactions (id, tmux_session, tmux_pane, config_id, finished_at_ms, \
             duration_ms, source, confidence, created_at) values \
             ('i{i}', 's', '%1', '{config}', {finished}, {duration}, 'hook', 'exact', {now});"
        ));
    }
    seed_usage(home, &sql);
    write(
        &home.root.join(".claude/.credentials.json"),
        &json!({"claudeAiOauth": {"accessToken": "tok-main"}}).to_string(),
    );
}

fn oauth_answer() -> Value {
    json!({"limits": [
        {"kind": "weekly_all", "percent": 92.5, "resets_at": NOW as i64 + 5 * 86_400},
        {"kind": "weekly_scoped", "percent": 40, "resets_at": NOW as i64 + 86_400,
         "scope": {"model": {"display_name": "Fable"}}},
        {"kind": "session", "percent": 99, "resets_at": NOW as i64 + 3_600}
    ]})
}

async fn settle(cache: &LimitsCache) {
    for _ in 0..500 {
        if !cache.refreshing() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("el refresco no terminó");
}

#[tokio::test]
async fn guard_and_latency_match_python() {
    let home = TestHome::new("guard-oracle");
    seed_guard_home(&home);
    write(
        &home.root.join("oauth-case.json"),
        &json!({"tok-main": oauth_answer()}).to_string(),
    );
    // El Python sobre una copia: su refresco escribe fotos de cuota.
    let python_home = home.root.with_file_name(format!(
        "{}-py",
        home.root.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&python_home);
    copy_tree(&home.root, &python_home);
    let out = run_python(
        GUARD,
        &[python_home.as_os_str(), NOW.to_string().as_ref()],
        &python_home,
    );
    let _ = std::fs::remove_dir_all(&python_home);
    let Some(out) = out else {
        return;
    };
    let python = workspace_loads(&out).unwrap();

    let oauth = Arc::new(FakeOauth::default());
    oauth.set("tok-main", FakeAnswer::Json(oauth_answer()));
    let mut opts = home.options();
    opts.oauth = oauth.clone();
    let native = Native::new(opts);
    native.limits().get(&native.refresh_deps());
    settle(native.limits()).await;
    let got = guard::token_guard_with_forecast(&native)
        .await
        .ok()
        .flatten()
        .unwrap();
    assert_eq!(
        response_dumps(&got).unwrap(),
        response_dumps(&python["guard"]).unwrap()
    );
    // La prueba cubre de verdad la guardia y el pronóstico.
    assert_eq!(got["projects"][0]["project"], json!("Proyecto"));
    assert_eq!(got["projects"][0]["subagentFiles"], json!(2));
    assert_eq!(got["forecastLevel"], json!("critical"));
    assert_eq!(got["forecasts"].as_array().unwrap().len(), 2);
    let latency = guard::latency(&native).await.ok().unwrap();
    let flat: Vec<Value> = latency
        .into_iter()
        .map(|((m, e), (p, a))| json!([m, e, p, a]))
        .collect();
    assert_eq!(
        response_dumps(&Value::Array(flat.clone())).unwrap(),
        response_dumps(&python["latency"]).unwrap()
    );
    assert_eq!(flat.len(), 2, "{flat:?}");
    assert_eq!(oauth.calls(), 1);
}

#[tokio::test]
async fn guard_python_exception_is_empty_and_skips_limits() {
    // `total_tokens` infinito: `int(inf)` lanza `OverflowError` → `{}`
    // (`Ok(None)`), sin pedir los límites (el Python no llega a pedirlos).
    let home = TestHome::new("guard-raises");
    seed_usage(
        &home,
        &format!(
            "insert into usage_turns (id, provider, agent, tmux_session, tmux_pane, pane_pwd, git_root, \
             model, turn_started_at, turn_finished_at, total_tokens, source, confidence) values \
             ('t', 'claude', 'claude', 's', '%1', '/w', '/w', 'claude-opus-5', 1, {}, 9e999, \
             'claude_jsonl', 'exact');",
            NOW_MS / 1000 - 60
        ),
    );
    let native = Native::new(home.options());
    assert!(matches!(
        guard::token_guard_with_forecast(&native).await,
        Ok(None)
    ));
    assert!(!native.limits().refreshing());
    assert!(!native.limits().current().loaded);
}

#[tokio::test]
async fn guard_undecodable_lane_down_and_unloaded_limits_decline() {
    // BLOB en `model`: el Python tal vez lo aceptaría → declinar.
    let home = TestHome::new("guard-blob");
    seed_usage(
        &home,
        &format!(
            "insert into usage_turns (id, provider, agent, tmux_session, tmux_pane, pane_pwd, git_root, \
             model, turn_started_at, turn_finished_at, source, confidence) values \
             ('t', 'claude', 'claude', 's', '%1', '/w', '/w', X'636c617564652d6f707573', 1, {}, \
             'claude_jsonl', 'exact');",
            NOW_MS / 1000 - 60
        ),
    );
    let native = Native::new(home.options());
    assert!(matches!(
        guard::token_guard_with_forecast(&native).await,
        Err(Fault::Decline)
    ));

    // Carril apagado (base más nueva).
    let home = TestHome::new("guard-lane-down");
    seed_usage(&home, "pragma user_version = 12;");
    let native = Native::new(home.options());
    assert!(matches!(
        guard::token_guard_with_forecast(&native).await,
        Err(Fault::Decline)
    ));
    assert!(matches!(guard::latency(&native).await, Err(Fault::Decline)));

    // Sin efectos de uso (la sombra) la caché nunca se llena: declina sin red.
    let home = TestHome::new("guard-no-effects");
    seed_usage(&home, "");
    let mut opts = home.options();
    opts.usage_effects = false;
    let oauth = Arc::new(FakeOauth::default());
    opts.oauth = oauth.clone();
    let native = Native::new(opts);
    let started = Instant::now();
    assert!(matches!(
        guard::token_guard_with_forecast(&native).await,
        Err(Fault::Decline)
    ));
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(oauth.calls(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn guard_waits_first_refresh_but_never_hangs() {
    // Caché vacía: la primera guardia lanza el refresco y lo espera (sin
    // credenciales termina enseguida) → responde con la caché ya llena.
    let home = TestHome::new("guard-first");
    seed_usage(&home, "");
    let native = Native::new(home.options());
    assert!(!native.limits().current().loaded);
    let got = guard::token_guard_with_forecast(&native)
        .await
        .ok()
        .flatten()
        .unwrap();
    assert_eq!(got["forecastLevel"], json!("normal"));
    assert!(native.limits().current().loaded);

    // Red colgada: espera acotada, el runtime sigue libre y declina.
    let home = TestHome::new("guard-hung");
    seed_usage(&home, "");
    write(
        &home.root.join(".claude/.credentials.json"),
        &json!({"claudeAiOauth": {"accessToken": "tok-main"}}).to_string(),
    );
    let oauth = Arc::new(FakeOauth::default());
    oauth.set("tok-main", FakeAnswer::Hang);
    let mut opts = home.options();
    opts.oauth = oauth.clone();
    let native = Arc::new(Native::new(opts));
    let ticks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let ticker = {
        let ticks = ticks.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                ticks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        })
    };
    let started = Instant::now();
    assert!(matches!(
        guard::token_guard_with_forecast(&native).await,
        Err(Fault::Decline)
    ));
    let waited = started.elapsed();
    ticker.abort();
    assert!(waited >= guard::LIMITS_WAIT && waited < guard::LIMITS_WAIT + Duration::from_secs(1));
    assert!(
        ticks.load(std::sync::atomic::Ordering::SeqCst) >= 20,
        "runtime bloqueado"
    );
    assert_eq!(oauth.calls(), 1);
    assert!(native.limits().refreshing());
}

#[tokio::test]
async fn guard_declines_until_main_reads_once_then_keeps_stale_rows() {
    // I1 (revisión de la 5a): tras un reinicio, si la primera llamada OAuth de
    // `main` falla (429), el frente no tiene «último dato bueno» y el Python
    // sí: el pronóstico no puede salir `[]`, declina. Tras una lectura buena se
    // calcula; un fallo posterior conserva esas filas (`stale`), como el Python.
    use comandos_server::dash::native::states::context::Context;
    use std::sync::atomic::{AtomicI64, Ordering};
    let home = TestHome::new("guard-main-429");
    seed_usage(&home, "");
    write(
        &home.root.join(".claude/.credentials.json"),
        &json!({"claudeAiOauth": {"accessToken": "tok-main"}}).to_string(),
    );
    let oauth = Arc::new(FakeOauth::default());
    oauth.set(
        "tok-main",
        FakeAnswer::Error("HTTP Error 429: Too Many Requests".into()),
    );
    let clock = Arc::new(AtomicI64::new(NOW_MS));
    let mut opts = home.options();
    opts.oauth = oauth.clone();
    opts.clock = {
        let clock = clock.clone();
        Arc::new(move || clock.load(Ordering::SeqCst))
    };
    let native = Native::new(opts);
    let registry = json!({});

    // Primer refresco con error: ni la guardia ni el contexto calculan.
    assert!(matches!(
        guard::token_guard_with_forecast(&native).await,
        Err(Fault::Decline)
    ));
    assert_eq!(oauth.calls(), 1);
    assert!(!native.limits().refreshing());
    let current = native.limits().current();
    assert!(!current.loaded);
    assert_eq!(current.health["claude_oauth"]["status"], json!("error"));
    let ctx = Context::default();
    assert!(ctx.get(&native, &registry, NOW_MS).await.is_err());
    // Dentro del TTL de error (180 s) no hay otra petición y sigue declinando.
    clock.store(NOW_MS + 179_000, Ordering::SeqCst);
    assert!(matches!(
        guard::token_guard_with_forecast(&native).await,
        Err(Fault::Decline)
    ));
    assert_eq!(oauth.calls(), 1);

    // Vencido el TTL, una lectura buena: ya se calcula.
    oauth.set("tok-main", FakeAnswer::Json(oauth_answer()));
    clock.store(NOW_MS + 181_000, Ordering::SeqCst);
    let got = guard::token_guard_with_forecast(&native)
        .await
        .ok()
        .flatten()
        .unwrap();
    assert_eq!(oauth.calls(), 2);
    assert!(native.limits().current().loaded);
    assert_eq!(got["forecasts"].as_array().unwrap().len(), 2);
    let ok = ctx.get(&native, &registry, NOW_MS + 181_000).await.unwrap();
    assert_eq!(ok.guard["forecasts"].as_array().unwrap().len(), 2);

    // Un 429 posterior conserva las últimas filas buenas: sigue calculando.
    oauth.set(
        "tok-main",
        FakeAnswer::Error("HTTP Error 429: Too Many Requests".into()),
    );
    clock.store(NOW_MS + 242_000, Ordering::SeqCst);
    native.limits().get(&native.refresh_deps());
    settle(native.limits()).await;
    assert_eq!(oauth.calls(), 3);
    let current = native.limits().current();
    assert!(current.loaded);
    assert_eq!(current.health["claude_oauth"]["stale"], json!(true));
    let stale = guard::token_guard_with_forecast(&native)
        .await
        .ok()
        .flatten()
        .unwrap();
    assert_eq!(stale["forecasts"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn guard_without_main_credentials_is_loaded() {
    // Sin credenciales de `main` (estado `missing`) el Python tampoco tiene
    // filas de `main`: la caché cuenta como cargada tras el primer refresco.
    let home = TestHome::new("guard-main-missing");
    seed_usage(&home, "");
    let native = Native::new(home.options());
    let got = guard::token_guard_with_forecast(&native)
        .await
        .ok()
        .flatten()
        .unwrap();
    assert_eq!(got["forecasts"], json!([]));
    assert!(native.limits().current().loaded);
}
