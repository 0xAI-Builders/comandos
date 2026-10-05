//! Lectores locales de límites (`comandos_runtime::limits`) contra `bin/cc_usage.py`,
//! `lib/grok_state.py` y las funciones de `bin/cc-dash` que los envuelven.
#[path = "support/python.rs"]
mod python;

use comandos_core::json::{response_dumps, workspace_loads};
use comandos_runtime::limits::{
    self, AbortRefresh, Raised, claude_account_creds, grok_account_homes, oauth_token,
    parse_claude_oauth_limits, parse_groq_ratelimit_headers, read_agy_quota,
    read_codex_rate_limits, read_grok_credit_limits, read_groq_headers, user_quotas,
};
use python::run_python;
use serde_json::{Map, Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// 2026-10-04T12:00:00Z.
const NOW: i64 = 1_791_115_200;

const ORACLE: &str = r#"
import importlib.machinery, importlib.util, json, os, sys, time
repo, home, now = sys.argv[1], sys.argv[2], int(sys.argv[3])
os.environ["TZ"] = "America/Mexico_City"
time.tzset()
time.time = lambda: float(now)
sys.path[:0] = [os.path.join(repo, "bin"), os.path.join(repo, "lib")]
import cc_usage, grok_state
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
cases = json.load(open(os.path.join(home, "case.json")))
def oauth(p):
    try:
        return {"ok": cc_usage.parse_claude_oauth_limits(p, now=now)}
    except Exception as e:
        return {"err": str(e)}
def groq(h):
    try:
        return {"ok": cc_usage.parse_groq_ratelimit_headers(h, now=now)}
    except Exception:
        return {"err": True}
sessions = os.path.join(home, ".codex/sessions")
print(json.dumps({
  "oauth": [oauth(p) for p in cases["oauth"]],
  "codex": cc_usage.read_codex_rate_limits(sessions, now=now),
  "codex_one": cc_usage.read_codex_rate_limits(sessions, now=now, max_files=1),
  "codex_stale": cc_usage.read_codex_rate_limits(os.path.join(home, ".codex/stale"), now=now),
  "codex_none": cc_usage.read_codex_rate_limits(os.path.join(home, ".codex/nada"), now=now),
  "grok": cc_usage.read_grok_credit_limits([os.path.join(home, ".grok")], now=now),
  "grok_homes": [[a, str(h)] for a, h in grok_state.account_homes()],
  "grok_all": cc_usage.read_grok_credit_limits([str(h) for _a, h in grok_state.account_homes()], now=now),
  "agy": cc_usage.read_agy_quota(os.path.join(home, ".claude/hooks/agy-quota.json"), now=float(now)),
  "agy_missing": cc_usage.read_agy_quota(os.path.join(home, "nada.json"), now=float(now)),
  "groq": [groq(h) for h in cases["groq"]],
  "groq_file": dash.read_groq_rate_limits(),
  "creds": [[a, p] for a, p in dash._claude_account_creds()],
  "quotas": dash.user_quotas(),
}))
"#;

fn temp_home(tag: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("cmd-limits-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    home
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn touch(path: &Path, secs_ago: u64) {
    let when = SystemTime::UNIX_EPOCH + Duration::from_secs(NOW as u64 - secs_ago);
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

fn zone() -> chrono_tz::Tz {
    chrono_tz::America::Mexico_City
}

/// Rollouts de Codex: el último snapshot por archivo, la ventana más fresca entre
/// archivos, cola de 256 KiB, `splitlines` con U+2028 y snapshots vacíos.
fn seed_codex(home: &Path) {
    let day = home.join(".codex/sessions/2026/10/04");
    let a = day.join("rollout-a.jsonl");
    write(
        &a,
        concat!(
            r#"{"timestamp":"2026-10-04T11:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":12.34,"window_minutes":300,"resets_at":1791126000},"secondary":null}}}"#,
            "\n",
            r#"{"timestamp":"2026-10-04T11:50:00.500Z","payload":{"rate_limits":{"primary":{"used_percent":"55","window_minutes":10080,"resets_at":"2026-10-08T00:00:00Z"},"secondary":{"used_percent":null,"window_minutes":300},"plan_type":"plus"}}}"#,
            "\n",
            "esto no es json \"rate_limits\"\n",
        ),
    );
    touch(&a, 100);
    let b = day.join("rollout-b.jsonl");
    write(
        &b,
        concat!(
            r#"{"timestamp":"2026-10-04T11:45:00+00:00","payload":{"rate_limits":{"primary":{"used_percent":20,"window_minutes":300,"resets_at":"1791130000.9"},"secondary":{"used_percent":60.06,"window_minutes":10080,"resets_at":1791500000},"plan_type":"pro"}}}"#,
            "\r\n",
            r#"{"payload":{"rate_limits":{}}}"#,
            "\n",
            "{\"nota\":\"x\u{2028}y\",\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":99,\"window_minutes\":300}}}}\n",
        ),
    );
    touch(&b, 50);
    let old = day.join("rollout-old.jsonl");
    write(
        &old,
        r#"{"timestamp":"2026-10-04T01:00:00Z","payload":{"rate_limits":{"primary":{"used_percent":80,"window_minutes":300}}}}"#,
    );
    touch(&old, 10_000);
    // El snapshot queda fuera de la cola de 256 KiB; el relleno parte caracteres.
    let big = day.join("rollout-big.jsonl");
    let mut text = String::from(
        r#"{"timestamp":"2026-10-04T11:59:00Z","payload":{"rate_limits":{"primary":{"used_percent":1,"window_minutes":300}}}}"#,
    );
    text.push('\n');
    while text.len() < 300 * 1024 {
        text.push_str("relleno ñandú ✓ sin límites\n");
    }
    write(&big, &text);
    touch(&big, 200);
    // No es `.jsonl`.
    write(&day.join("rollout-c.json"), "{}");
    // Ventana de 5 h vieja (11 h > 7.5 h): se oculta; la semanal reciente queda.
    let stale = home.join(".codex/stale/x/rollout.jsonl");
    write(
        &stale,
        concat!(
            r#"{"timestamp":"2026-10-04T01:00:00Z","payload":{"rate_limits":{"primary":{"used_percent":70,"window_minutes":300,"resets_at":1791090000},"secondary":{"used_percent":5,"window_minutes":10080},"plan_type":null}}}"#,
            "\n"
        ),
    );
}

/// Log de Grok: la línea de cobro queda a más de 512 KiB del final (la cola crece).
fn seed_grok(home: &Path) {
    let mut text = String::new();
    text.push_str(r#"{"ts":"2026-10-04T10:00:00.123Z","msg":"billing: fetched credits config","ctx":{"subscriptionTier":"SuperGrok","config":{"creditUsagePercent":37.5,"currentPeriod":{"start":"2026-09-30T00:00:00+00:00","end":"2026-10-07T00:00:00+00:00"}}}}"#);
    text.push('\n');
    text.push_str(r#"{"ts":"2026-10-04T09:00:00Z","msg":"billing: fetched credits config","ctx":{"subscriptionTier":"SuperGrok","config":{"creditUsagePercent":"12","currentPeriod":{"start":"2026-09-30T00:00:00+00:00"}}}}"#);
    text.push('\n');
    text.push_str(r#"{"ts":"ayer","msg":"billing: fetched credits config","ctx":{"config":{"creditUsagePercent":0,"currentPeriod":{"end":"2026-10-08T00:00:00Z"}}}}"#);
    text.push('\n');
    text.push_str("no json: fetched credits config\n");
    while text.len() < 600 * 1024 {
        text.push_str(r#"{"ts":"2026-10-04T11:00:00Z","msg":"otra cosa","ctx":{"n":1234567890}}"#);
        text.push('\n');
    }
    write(&home.join(".grok/logs/unified.jsonl"), &text);
    // Otra cuenta con la línea más nueva pero de un periodo vencido: `stale_period`.
    write(
        &home.join(".grok-accounts/beta/logs/unified.jsonl"),
        concat!(
            r#"{"ts":"2026-10-04T11:00:00+00:00","msg":"billing: fetched credits config","ctx":{"subscriptionTier":7,"config":{"creditUsagePercent":88.8,"currentPeriod":{"start":"2026-09-20T00:00:00","end":"2026-09-27T00:00:00"}}}}"#,
            "\n"
        ),
    );
    fs::create_dir_all(home.join(".grok-accounts/alfa")).unwrap();
    fs::create_dir_all(home.join(".grok-accounts/.oculta")).unwrap();
    fs::create_dir_all(home.join(".grok-accounts/-guion")).unwrap();
    write(&home.join(".grok-accounts/archivo"), "x");
}

fn seed_hooks(home: &Path) {
    let hooks = home.join(".claude/hooks");
    write(
        &hooks.join("agy-quota.json"),
        &format!(
            r#"{{"captured_at": {}, "plan_tier": "pro", "quota": {{"gemini-weekly": {{"remaining_fraction": 0.42, "reset_time": "2026-10-09T00:00:00Z"}}, "gemini-5h": {{"remaining_fraction": 0.9, "reset_time": "2026-10-04T10:00:00Z"}}, "3p-weekly": {{"remaining_fraction": 1, "reset_time": "2026-10-10T12:00:00.5+00:00"}}, "otra": {{"remaining_fraction": 0.1}}}}}}"#,
            NOW - 60
        ),
    );
    write(
        &hooks.join("groq-ratelimit.json"),
        &format!(
            r#"{{"at": {}, "headers": {{"x-ratelimit-limit-requests": "1000", "x-ratelimit-remaining-requests": "998", "x-ratelimit-reset-requests": "7.66s", "X-RateLimit-Limit-Tokens": "6000", "x-ratelimit-remaining-tokens": "4500", "x-ratelimit-reset-tokens": "1m30s"}}}}"#,
            NOW - 30
        ),
    );
    write(
        &hooks.join("provider-quotas.json"),
        r#"{"grok": {"tokens_7d": 500000}, "groq": {"tokens_7d": "250000"}}"#,
    );
    write(&home.join(".claude/.credentials.json"), "{}");
    write(&home.join(".claude-accounts/beta/.credentials.json"), "{}");
    write(&home.join(".claude-accounts/alfa/.credentials.json"), "{}");
    write(
        &home.join(".claude-accounts/x.lock/.credentials.json"),
        "{}",
    );
    fs::create_dir_all(home.join(".claude-accounts/gamma")).unwrap();
}

/// Casos con `Infinity` literal: se escriben como texto.
const CASES: &str = r#"{
 "oauth": [
  {"limits": [
    {"kind": "session", "percent": 12.0, "resets_at": "2026-10-04T15:00:00.000000+00:00"},
    {"kind": "weekly_all", "percent": 41, "resets_at": 1791500000, "severity": null},
    {"kind": "weekly_scoped", "percent": "7.25", "resets_at": "2026-10-06T00:00:00Z",
     "scope": {"model": {"display_name": "Opus 4.1"}}},
    {"kind": "weekly_scoped", "percent": 1, "scope": {"model": {"display_name": null}}},
    {"kind": "Extra-Usage", "percent": 3, "severity": "warning", "is_active": false},
    {"percent": 5, "is_active": null},
    {"kind": "session", "percent": null},
    "x"
  ]},
  {"limits": [], "five_hour": {"utilization": 33.333, "resets_at": "2026-10-04T16:00:00Z"},
   "seven_day": {"utilization": 0, "resets_at": null}},
  {"limits": {"a": 1}, "five_hour": 5, "seven_day": {"utilization": null}},
  [],
  "texto",
  {"limits": [{"kind": "session", "percent": 1, "resets_at": Infinity}]},
  {"limits": [{"kind": "session", "percent": 1, "resets_at": NaN}]}
 ],
 "groq": [
  {"x-ratelimit-limit-requests": "1000", "x-ratelimit-remaining-requests": "998",
   "x-ratelimit-reset-requests": "7.66s", "X-RateLimit-Limit-Tokens": "6000",
   "x-ratelimit-remaining-tokens": "4500", "x-ratelimit-reset-tokens": "1m30s"},
  {"x-ratelimit-limit-tokens": 12000, "x-ratelimit-remaining-tokens": 0,
   "x-ratelimit-reset-tokens": "1791115300"},
  {"x-ratelimit-limit-tokens": "10", "x-ratelimit-remaining-tokens": "5",
   "x-ratelimit-reset-tokens": "1.2.3s"},
  {"x-ratelimit-limit-requests": "100", "x-ratelimit-remaining-requests": null,
   "x-ratelimit-reset-requests": "30"},
  {"x-ratelimit-limit-requests": "0", "x-ratelimit-remaining-requests": "1",
   "x-ratelimit-limit-tokens": "50", "x-ratelimit-remaining-tokens": "",
   "otra": [1, 2]},
  {"x-ratelimit-limit-tokens": "50", "x-ratelimit-remaining-tokens": "70",
   "x-ratelimit-reset-tokens": " 2h 5x "}
 ]
}"#;

fn paths(pairs: Vec<(String, PathBuf)>) -> Value {
    Value::Array(
        pairs
            .into_iter()
            .map(|(a, p)| json!([a, p.to_string_lossy()]))
            .collect(),
    )
}

#[test]
fn limits_readers_match_python() {
    let home = temp_home("oracle");
    seed_codex(&home);
    seed_grok(&home);
    seed_hooks(&home);
    write(&home.join("case.json"), CASES);
    let Some(out) = run_python(ORACLE, &[home.as_os_str(), NOW.to_string().as_ref()], &home) else {
        return;
    };
    let cases = workspace_loads(CASES).unwrap();
    let zone = zone();
    let oauth: Vec<Value> = cases["oauth"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| match parse_claude_oauth_limits(p, NOW, &zone) {
            Ok(rows) => json!({"ok": rows}),
            Err(Raised::Exception(e)) => json!({"err": e}),
            Err(Raised::Unsure) => panic!("oauth incierto: {p}"),
        })
        .collect();
    let groq: Vec<Value> = cases["groq"]
        .as_array()
        .unwrap()
        .iter()
        .map(
            |h| match parse_groq_ratelimit_headers(h.as_object().unwrap(), NOW) {
                Ok(rows) => json!({"ok": rows}),
                Err(Raised::Exception(_)) => json!({"err": true}),
                Err(Raised::Unsure) => panic!("groq incierto: {h}"),
            },
        )
        .collect();
    let sessions = home.join(".codex/sessions");
    let grok_homes = grok_account_homes(&home);
    let all: Vec<PathBuf> = grok_homes.iter().map(|(_, h)| h.clone()).collect();
    let hooks = home.join(".claude/hooks");
    let opt = |m: Option<Map<String, Value>>| m.map_or(Value::Null, Value::Object);
    let rust = json!({
        "oauth": oauth,
        "codex": read_codex_rate_limits(&sessions, NOW, 16, &zone).unwrap(),
        "codex_one": read_codex_rate_limits(&sessions, NOW, 1, &zone).unwrap(),
        "codex_stale": read_codex_rate_limits(&home.join(".codex/stale"), NOW, 16, &zone).unwrap(),
        "codex_none": read_codex_rate_limits(&home.join(".codex/nada"), NOW, 16, &zone).unwrap(),
        "grok": opt(read_grok_credit_limits(&[home.join(".grok")], NOW, &zone)),
        "grok_homes": paths(grok_homes),
        "grok_all": opt(read_grok_credit_limits(&all, NOW, &zone)),
        "agy": read_agy_quota(&hooks.join("agy-quota.json"), NOW as f64, &zone),
        "agy_missing": read_agy_quota(&home.join("nada.json"), NOW as f64, &zone),
        "groq": groq,
        "groq_file": read_groq_headers(&hooks.join("groq-ratelimit.json"), NOW),
        "creds": paths(claude_account_creds(&home)),
        "quotas": user_quotas(&hooks.join("provider-quotas.json")),
    });
    let expected = workspace_loads(out.trim()).unwrap();
    for key in rust.as_object().unwrap().keys() {
        assert_eq!(
            response_dumps(&rust[key]).unwrap(),
            response_dumps(&expected[key]).unwrap(),
            "{key}"
        );
    }
    assert_eq!(
        response_dumps(&rust).unwrap(),
        out.trim(),
        "bytes del oráculo"
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn oauth_token_shapes() {
    let home = temp_home("token");
    let path = home.join(".credentials.json");
    assert_eq!(oauth_token(&path), Ok(String::new()), "ausente");
    let cases: &[(&[u8], Result<&str, AbortRefresh>)] = &[
        (b"[]", Err(AbortRefresh)),
        (br#"{"claudeAiOauth": {"accessToken": 5}}"#, Ok("5")),
        (
            br#"{"claudeAiOauth": {"accessToken": "tok-a"}}"#,
            Ok("tok-a"),
        ),
        (br#"{"claudeAiOauth": {"accessToken": true}}"#, Ok("True")),
        (br#"{"claudeAiOauth": {"accessToken": 1.5}}"#, Ok("1.5")),
        (br#"{"claudeAiOauth": {"accessToken": null}}"#, Ok("")),
        (br#"{"claudeAiOauth": null}"#, Ok("")),
        (br#"{"claudeAiOauth": 0}"#, Ok("")),
        (br#"{"claudeAiOauth": []}"#, Ok("")),
        (br#"{"claudeAiOauth": 7}"#, Err(AbortRefresh)),
        (br#"{"claudeAiOauth": "x"}"#, Err(AbortRefresh)),
        (br#"{"otra": 1}"#, Ok("")),
        (b"{roto", Ok("")),
        (b"\xef\xbb\xbf{}", Ok("")),
        (b"{\"claudeAiOauth\": {\"accessToken\": \"\xff\"}}", Ok("")),
        (b"5", Err(AbortRefresh)),
    ];
    for (bytes, want) in cases {
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            oauth_token(&path),
            want.map(str::to_owned),
            "{}",
            String::from_utf8_lossy(bytes)
        );
    }
    // Un directorio es un `OSError` (`IsADirectoryError`): token vacío.
    fs::remove_file(&path).unwrap();
    fs::create_dir_all(&path).unwrap();
    assert_eq!(oauth_token(&path), Ok(String::new()));
    assert_eq!(
        limits::CLAUDE_OAUTH_USAGE_URL,
        "https://api.anthropic.com/api/oauth/usage"
    );
    let _ = fs::remove_dir_all(&home);
}
