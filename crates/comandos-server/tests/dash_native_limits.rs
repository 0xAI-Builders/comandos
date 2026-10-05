//! Caché de límites: TTL, un refresco en vuelo, 429, red colgada y el refresco
//! completo contra `_refresh_provider_limits` del Python.
mod support;
use comandos_core::json::{response_dumps, workspace_loads};
use comandos_server::dash::native::{Native, usage::limits::LimitsCache};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};
use support::{FakeAnswer, FakeOauth, NOW_MS, TestHome, seed_usage};

fn creds(home: &TestHome, rel: &str, token: &str) {
    write(
        &home.root.join(rel),
        &json!({"claudeAiOauth": {"accessToken": token}}).to_string(),
    );
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn payload(percent: f64) -> Value {
    json!({"limits": [{"kind": "weekly_all", "percent": percent, "resets_at": "2026-10-06T00:00:00+00:00"}]})
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
async fn limits_429_keeps_last_rows_and_backs_off() {
    let home = TestHome::new("limits-429");
    creds(&home, ".claude/.credentials.json", "tok-main");
    let oauth = Arc::new(FakeOauth::default());
    oauth.set("tok-main", FakeAnswer::Json(payload(41.0)));
    let clock = Arc::new(AtomicI64::new(NOW_MS));
    let mut opts = home.options();
    opts.oauth = oauth.clone();
    let c2 = clock.clone();
    opts.clock = Arc::new(move || c2.load(Ordering::SeqCst));
    let native = Native::new(opts);
    let deps = native.refresh_deps();
    let cache = Arc::new(LimitsCache::default());
    // Primera vez: vacío y lanza el refresco.
    assert!(cache.get(&deps).rows.is_empty());
    settle(&cache).await;
    let first = cache.get(&deps);
    assert_eq!(first.rows[0]["percent"], json!(41.0));
    assert_eq!(first.health["claude_oauth"]["status"], json!("ok"));
    // 61 s después: 429; se conservan las filas de `main` con stale=true.
    oauth.set(
        "tok-main",
        FakeAnswer::Error("HTTP Error 429: Too Many Requests".into()),
    );
    clock.fetch_add(61_000, Ordering::SeqCst);
    cache.get(&deps);
    settle(&cache).await;
    let after = cache.get(&deps);
    assert_eq!(after.rows[0]["percent"], json!(41.0));
    assert_eq!(after.health["claude_oauth"]["stale"], json!(true));
    assert_eq!(
        after.health["claude_oauth"]["error"],
        json!("HTTP Error 429: Too Many Requests")
    );
    // Con error, el TTL es 180 s: a los 120 s no hay llamada nueva; a los 181 s sí.
    let calls = oauth.calls();
    clock.fetch_add(120_000, Ordering::SeqCst);
    cache.get(&deps);
    assert!(!cache.refreshing());
    assert_eq!(oauth.calls(), calls);
    clock.fetch_add(61_000, Ordering::SeqCst);
    cache.get(&deps);
    settle(&cache).await;
    assert_eq!(oauth.calls(), calls + 1);
    native.shutdown().await;
}

#[tokio::test]
async fn limits_hung_network_never_blocks() {
    let home = TestHome::new("limits-hang");
    creds(&home, ".claude/.credentials.json", "tok-main");
    let oauth = Arc::new(FakeOauth::default());
    oauth.set("tok-main", FakeAnswer::Hang);
    let mut opts = home.options();
    opts.oauth = oauth.clone();
    let native = Native::new(opts);
    let deps = native.refresh_deps();
    let cache = Arc::new(LimitsCache::default());
    let mut waited = 0;
    while oauth.calls() == 0 {
        let started = std::time::Instant::now();
        assert!(cache.get(&deps).rows.is_empty());
        assert!(started.elapsed() < Duration::from_millis(50));
        tokio::time::sleep(Duration::from_millis(5)).await;
        waited += 1;
        assert!(waited < 400, "el refresco no llegó a la red");
    }
    for _ in 0..20 {
        let started = std::time::Instant::now();
        assert!(cache.get(&deps).rows.is_empty());
        assert!(started.elapsed() < Duration::from_millis(50));
        tokio::task::yield_now().await;
    }
    assert_eq!(oauth.calls(), 1, "un solo refresco en vuelo");
    assert!(cache.refreshing());
}

#[tokio::test]
async fn attach_tokens_mutates_cached_rows_once() {
    let cache = LimitsCache::with_rows(vec![
        json!({"provider": "codex", "tokens_7d": 0})
            .as_object()
            .cloned()
            .unwrap(),
    ]);
    let windows = json!({"items": [{"metric": "tokens", "provider": "codex", "window": "weekly", "used": 70}]});
    assert_eq!(cache.attach_tokens(&windows)[0]["tokens_7d"], json!(70));
    let later = json!({"items": [{"metric": "tokens", "provider": "codex", "window": "weekly", "used": 90}]});
    // D5: el primer valor se queda hasta el siguiente refresco.
    assert_eq!(cache.attach_tokens(&later)[0]["tokens_7d"], json!(70));
}

#[tokio::test]
async fn limits_without_usage_effects_never_refresh() {
    let home = TestHome::new("limits-shadow");
    creds(&home, ".claude/.credentials.json", "tok-main");
    let oauth = Arc::new(FakeOauth::default());
    oauth.set("tok-main", FakeAnswer::Json(payload(41.0)));
    let mut opts = home.options();
    opts.oauth = oauth.clone();
    opts.usage_effects = false;
    let native = Native::new(opts);
    let deps = native.refresh_deps();
    let cache = Arc::new(LimitsCache::default());
    for _ in 0..5 {
        assert!(cache.get(&deps).rows.is_empty());
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(!cache.refreshing());
    assert_eq!(oauth.calls(), 0, "sombra sin red");
}

#[tokio::test]
async fn refresh_never_creates_the_usage_db() {
    let home = TestHome::new("limits-nodb");
    write(
        &home.hooks().join("groq-ratelimit.json"),
        r#"{"at": 1791115100, "headers": {"x-ratelimit-limit-tokens": "10", "x-ratelimit-remaining-tokens": "5"}}"#,
    );
    let native = Native::new(home.options());
    assert!(native.ready().await);
    let deps = native.refresh_deps();
    let cache = native.limits();
    // A demanda (como lo pide una ruta): el arranque ya no refresca.
    cache.get(&deps);
    settle(cache).await;
    // Hubo refresco (filas de cabeceras de Groq) y la base no existe.
    assert_eq!(cache.get(&deps).rows[0]["id"], json!("groq_tokens"));
    assert!(!home.usage_db().exists());
    native.shutdown().await;
}

/// Arrancar el frente lee los límites una vez (D3, `STARTUP_LIMITS_REFRESH`
/// desde la Tarea 8: la barra lateral no pierde los % de cuota tras un
/// reinicio); sin efectos de uso (sombra), ninguna llamada.
#[tokio::test]
async fn boot_refreshes_limits_once_only_with_usage_effects() {
    for effects in [true, false] {
        let home = TestHome::new(if effects {
            "limits-boot"
        } else {
            "limits-boot-shadow"
        });
        creds(&home, ".claude/.credentials.json", "tok-main");
        let oauth = Arc::new(FakeOauth::default());
        oauth.set("tok-main", FakeAnswer::Json(payload(41.0)));
        let mut opts = home.options();
        opts.oauth = oauth.clone();
        opts.usage_effects = effects;
        let front = support::front(&home, support::dead_port(), opts).await;
        // Una ruta nativa cualquiera: el frente ya atendió y abrió la base.
        let _ = support::get(front.port, "/prefs").await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(
            oauth.calls(),
            usize::from(effects),
            "efectos de uso: {effects}"
        );
        front.stop().await;
    }
}

/// Una respuesta OAuth que el port no interpreta con certeza no aborta el
/// refresco tras la red: error propio y el TTL de error, una sola llamada.
#[tokio::test]
async fn unsure_oauth_body_keeps_the_ttl() {
    let home = TestHome::new("limits-unsure");
    creds(&home, ".claude/.credentials.json", "tok-main");
    let oauth = Arc::new(FakeOauth::default());
    // `kind` no ASCII: el `slug` de Unicode del Python no se reproduce.
    oauth.set(
        "tok-main",
        FakeAnswer::Json(json!({"limits": [{"kind": "sesión", "percent": 5}]})),
    );
    let clock = Arc::new(AtomicI64::new(NOW_MS));
    let mut opts = home.options();
    opts.oauth = oauth.clone();
    let c2 = clock.clone();
    opts.clock = Arc::new(move || c2.load(Ordering::SeqCst));
    let native = Native::new(opts);
    let deps = native.refresh_deps();
    let cache = Arc::new(LimitsCache::default());
    cache.get(&deps);
    settle(&cache).await;
    let health = cache.get(&deps).health;
    assert_eq!(health["claude_oauth"]["status"], json!("error"));
    assert_eq!(
        health["claude_oauth"]["error"],
        json!(comandos_server::dash::native::usage::limits::OAUTH_UNSURE_ERROR)
    );
    clock.fetch_add(61_000, Ordering::SeqCst);
    cache.get(&deps);
    assert!(!cache.refreshing(), "con error el TTL es 180 s");
    assert_eq!(oauth.calls(), 1, "una sola petición");
    native.shutdown().await;
}

/// Un aborto (excepción no capturada del Python) DESPUÉS de la red fija `at`:
/// dentro del TTL no hay otra petición; al vencer, una.
#[tokio::test]
async fn abort_after_network_keeps_the_ttl() {
    let home = TestHome::new("limits-abort");
    creds(&home, ".claude/.credentials.json", "tok-main");
    // `int("x")` del Python en `user_quotas()["grok"]["tokens_7d"]`: el hilo moría.
    home.write("provider-quotas.json", r#"{"grok": {"tokens_7d": "x"}}"#);
    seed_usage(&home, &turn("g1", "grok", NOW_MS / 1000 - 600, 100));
    let oauth = Arc::new(FakeOauth::default());
    oauth.set("tok-main", FakeAnswer::Json(payload(41.0)));
    let clock = Arc::new(AtomicI64::new(NOW_MS));
    let mut opts = home.options();
    opts.oauth = oauth.clone();
    let c2 = clock.clone();
    opts.clock = Arc::new(move || c2.load(Ordering::SeqCst));
    let native = Native::new(opts);
    let deps = native.refresh_deps();
    let cache = Arc::new(LimitsCache::default());
    cache.get(&deps);
    settle(&cache).await;
    assert!(
        cache.get(&deps).rows.is_empty(),
        "el aborto no publica filas"
    );
    assert_eq!(oauth.calls(), 1);
    clock.fetch_add(30_000, Ordering::SeqCst);
    cache.get(&deps);
    assert!(!cache.refreshing(), "dentro del TTL no hay refresco");
    assert_eq!(oauth.calls(), 1);
    clock.fetch_add(31_000, Ordering::SeqCst);
    cache.get(&deps);
    settle(&cache).await;
    assert_eq!(oauth.calls(), 2, "vencido el TTL, otra petición");
    native.shutdown().await;
}

// ---------------------------------------------------------------- oráculo

const REFRESH: &str = r#"
import importlib.machinery, importlib.util, json, os, sqlite3, sys, time
repo, home, now = sys.argv[1], sys.argv[2], int(sys.argv[3])
os.environ["TZ"] = "America/Mexico_City"
time.tzset()
time.time = lambda: float(now)
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
case = json.load(open(os.path.join(home, "oauth-case.json")))
def fake(url, headers, timeout=8):
    ans = case["answers"].get(headers["Authorization"][len("Bearer "):])
    if ans is None:
        raise Exception("sin guion")
    if "error" in ans:
        raise Exception(ans["error"])
    return ans["json"]
dash.cc_usage._http_json = fake
dash._limits_cache["limits"] = case["prior"]
dash._refresh_provider_limits()
con = sqlite3.connect(dash.USAGE_DB)
snaps = [list(r) for r in con.execute(
    "select limit_id, provider, account, win, scope, resets_at, percent, captured_at"
    " from usage_quota_snapshots order by limit_id, resets_at")]
print(json.dumps({"limits": dash._limits_cache["limits"], "health": dash._limits_cache["health"],
                  "snaps": snaps}))
"#;

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

/// `python3 -c` sobre un HOME temporal sin el entorno real (D7, tmux, DBus).
fn run_python(script: &str, home: &Path, now: i64) -> Option<String> {
    let available = Command::new("python3")
        .args(["-c", "import sys"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !available {
        eprintln!("python3 no está instalado: se salta la comparación con el oráculo");
        return None;
    }
    let fakebin = home.join("fakebin");
    std::fs::create_dir_all(&fakebin).unwrap();
    for name in ["tmux", "systemctl", "notify-send", "xdg-open", "tailscale"] {
        let link = fakebin.join(name);
        if !link.exists() {
            std::os::unix::fs::symlink("/bin/true", &link).unwrap();
        }
    }
    let path = format!(
        "{}:{}",
        fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new("python3");
    for key in support::oracle::D7_KEYS {
        command.env_remove(key);
    }
    let out = command
        .arg("-c")
        .arg(script)
        .arg(support::repo())
        .arg(home)
        .arg(now.to_string())
        .current_dir(home)
        .env("HOME", home)
        .env("PATH", path)
        .env("LANG", "C.UTF-8")
        .env_remove("LC_ALL")
        .env_remove("LC_CTYPE")
        .env("XDG_RUNTIME_DIR", home.join("xdg-runtime"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("TMUX_TMPDIR", home.join("tmux"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("TMUX")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("GROK_HOME")
        .env_remove("COMANDOS_STATE_DB")
        .env_remove("COMANDOS_USAGE_DB")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "oráculo: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(String::from_utf8(out.stdout).unwrap())
}

fn turn(id: &str, provider: &str, finished: i64, tokens: i64) -> String {
    format!(
        "insert into usage_turns (id, provider, agent, tmux_session, tmux_pane, pane_pwd, git_root, \
         turn_started_at, turn_finished_at, total_tokens, source, confidence) values \
         ('{id}', '{provider}', '{provider}', 's', '%1', '/w', '/w', {s}, {finished}, {tokens}, 'hook', 'exact');",
        s = finished - 30
    )
}

fn seed_refresh_home(home: &TestHome) {
    let now = NOW_MS / 1000;
    creds(home, ".claude/.credentials.json", "tok-main");
    creds(home, ".claude-accounts/alt/.credentials.json", "tok-alt");
    write(
        &home.root.join(".claude-accounts/zeta/.credentials.json"),
        r#"{"claudeAiOauth": {}}"#,
    );
    write(
        &home.root.join(".claude/.claude.json"),
        r#"{"oauthAccount": {"emailAddress": "yo@example.com"}}"#,
    );
    write(
        &home.root.join(".codex/sessions/2026/10/04/rollout.jsonl"),
        concat!(
            r#"{"timestamp":"2026-10-04T11:45:00Z","payload":{"rate_limits":{"primary":{"used_percent":20,"window_minutes":300,"resets_at":1791130000},"secondary":{"used_percent":60.06,"window_minutes":10080,"resets_at":1791500000},"plan_type":"pro"}}}"#,
            "\n"
        ),
    );
    write(
        &home.root.join(".grok/logs/unified.jsonl"),
        concat!(
            r#"{"ts":"2026-10-04T10:00:00Z","msg":"billing: fetched credits config","ctx":{"subscriptionTier":"SuperGrok","config":{"creditUsagePercent":37.5,"currentPeriod":{"start":"2026-09-30T00:00:00+00:00","end":"2026-10-07T00:00:00+00:00"}}}}"#,
            "\n"
        ),
    );
    home.write(
        "agy-quota.json",
        &format!(
            r#"{{"captured_at": {}, "plan_tier": "pro", "quota": {{"gemini-weekly": {{"remaining_fraction": 0.42, "reset_time": "2026-10-09T00:00:00Z"}}}}}}"#,
            now - 60
        ),
    );
    home.write(
        "groq-ratelimit.json",
        &format!(
            r#"{{"at": {}, "headers": {{"x-ratelimit-limit-tokens": "6000", "x-ratelimit-remaining-tokens": "4500", "x-ratelimit-reset-tokens": "1m30s"}}}}"#,
            now - 30
        ),
    );
    home.write(
        "provider-quotas.json",
        r#"{"grok": {"tokens_7d": 500000}, "groq": {"tokens_7d": "250000"}}"#,
    );
    let sql = [
        turn("g1", "grok", now - 3600, 1200),
        turn("g2", "grok", now - 3 * 86_400, 800),
        turn("q1", "groq", now - 600, 4000),
    ]
    .concat();
    seed_usage(home, &sql);
}

fn snapshots(db: &Path) -> Value {
    let conn = rusqlite::Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare(
            "select limit_id, provider, account, win, scope, resets_at, percent, captured_at \
             from usage_quota_snapshots order by limit_id, resets_at",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok(json!([
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, f64>(6)?,
                r.get::<_, i64>(7)?
            ]))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    Value::Array(rows)
}

#[tokio::test]
async fn refresh_matches_python() {
    let home = TestHome::new("limits-oracle");
    seed_refresh_home(&home);
    let prior = json!([
        {"id": "alt:claude_weekly", "provider": "claude", "kind": "weekly_all", "label": "Semana",
         "percent": 77.0, "resets_at": 1791400000, "window": "7d", "account": "alt",
         "captured_at": 1791110000},
        {"id": "claude_weekly", "provider": "claude", "percent": 1.0, "account": "main"},
        {"id": "codex_weekly", "provider": "codex", "percent": 2.0}
    ]);
    let answers = json!({
        "tok-main": {"json": {"limits": [
            {"kind": "session", "percent": 12.5, "resets_at": "2026-10-04T15:00:00+00:00"},
            {"kind": "weekly_scoped", "percent": 30, "resets_at": 1791500000,
             "scope": {"model": {"display_name": "Opus"}}}
        ]}},
        "tok-alt": {"error": "HTTP Error 429: Too Many Requests"}
    });
    write(
        &home.root.join("oauth-case.json"),
        &json!({"answers": answers, "prior": prior}).to_string(),
    );
    let python_home = home.root.with_file_name(format!(
        "{}-py",
        home.root.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&python_home);
    copy_tree(&home.root, &python_home);
    let out = run_python(REFRESH, &python_home, NOW_MS / 1000);
    let python_db = python_home.join(".claude/hooks/comandos-usage.sqlite");
    let python_snaps = out.as_ref().map(|_| snapshots(&python_db));
    let _ = std::fs::remove_dir_all(&python_home);
    let Some(out) = out else {
        return;
    };

    let oauth = Arc::new(FakeOauth::default());
    oauth.set(
        "tok-main",
        FakeAnswer::Json(answers["tok-main"]["json"].clone()),
    );
    oauth.set(
        "tok-alt",
        FakeAnswer::Error("HTTP Error 429: Too Many Requests".into()),
    );
    let mut opts = home.options();
    opts.oauth = oauth.clone();
    let native = Native::new(opts);
    let deps = native.refresh_deps();
    let rows = prior
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_object().cloned().unwrap())
        .collect();
    let cache = Arc::new(LimitsCache::with_rows(rows));
    cache.get(&deps);
    settle(&cache).await;
    let got = cache.get(&deps);
    let rust = json!({
        "limits": got.rows,
        "health": got.health,
        "snaps": snapshots(&home.usage_db()),
    });
    let expected = workspace_loads(out.trim()).unwrap();
    assert_eq!(
        expected["snaps"],
        python_snaps.unwrap(),
        "lectura de la base"
    );
    for key in ["limits", "health", "snaps"] {
        assert_eq!(
            response_dumps(&rust[key]).unwrap(),
            response_dumps(&expected[key]).unwrap(),
            "{key}"
        );
    }
    assert_eq!(oauth.calls(), 2, "main y alt; zeta no tiene token");
    native.shutdown().await;
}

#[test]
fn shadow_flag_turns_usage_effects_off() {
    let home = std::path::PathBuf::from("/tmp/no-existe-home");
    let cfg = comandos_server::dash::parse_args(&[], &home, None).unwrap();
    assert!(cfg.usage_effects);
    let cfg =
        comandos_server::dash::parse_args(&["--no-usage-effects".into()], &home, None).unwrap();
    assert!(!cfg.usage_effects);
    assert_eq!(cfg.port, 4777);
}
