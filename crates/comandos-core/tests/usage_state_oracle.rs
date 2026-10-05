//! Conversiones y ensamblado de `comandos_core::usage_state` contra `bin/cc_usage.py`.
use comandos_core::json::{response_dumps, workspace_loads};
use comandos_core::usage_state::{self, LocalZone, UsageError};
use serde_json::{Map, Value, json};
use std::{collections::HashMap, path::Path, process::Command};

/// Claves del entorno que cambian lo que el Python calcula (D7): fuera del oráculo.
const D7_KEYS: &[&str] = &[
    "COMANDOS_DAILY_BUDGET_USD",
    "COMANDOS_USAGE_DAILY_BUDGET_USD",
    "COMANDOS_CODEX_DAILY_TOKEN_LIMIT",
    "COMANDOS_CODEX_WEEKLY_TOKEN_LIMIT",
    "COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT",
    "COMANDOS_CLAUDE_WEEKLY_TOKEN_LIMIT",
    "CODEX_DAILY_TOKEN_LIMIT",
    "CODEX_WEEKLY_TOKEN_LIMIT",
    "CLAUDE_DAILY_TOKEN_LIMIT",
    "CLAUDE_WEEKLY_TOKEN_LIMIT",
    "COMANDOS_USAGE_LOCAL_DAYS",
    "COMANDOS_USAGE_CLAUDE_MAX_FILES",
    "COMANDOS_USAGE_CODEX_MAX_FILES",
    "COMANDOS_CLAUDE_PROJECTS_DIR",
    "COMANDOS_OPENCODE_DB",
    "OPENAI_ADMIN_KEY",
    "ANTHROPIC_ADMIN_KEY",
    "COMANDOS_USAGE_DB",
    "COMANDOS_STATE_DB",
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "GROK_HOME",
];

fn repo() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `python3 -c <guion> <repo> <entrada>` con un HOME temporal; `None` sin python3.
/// `cc_usage` es solo biblioteca estándar y no lanza procesos al importarse.
fn run_python(script: &str, input: &str) -> Option<String> {
    let available = Command::new("python3")
        .args(["-c", "import sys"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !available {
        eprintln!("python3 no está instalado: se salta la comparación con el oráculo");
        return None;
    }
    // Un HOME propio por llamada: las pruebas corren en paralelo en el mismo proceso.
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let home = std::env::temp_dir().join(format!("cmd-usage-state-{}-{call}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let mut command = Command::new("python3");
    command
        .arg("-c")
        .arg(script)
        .arg(repo())
        .arg(input)
        .current_dir(&home)
        .env("HOME", &home)
        .env("TZ", "America/Mexico_City")
        .env("LANG", "C.UTF-8")
        .env_remove("LC_ALL")
        .env_remove("LC_CTYPE")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("XDG_RUNTIME_DIR", home.join("xdg-runtime"))
        .env_remove("TMUX");
    for key in D7_KEYS {
        command.env_remove(key);
    }
    let out = command.output().unwrap();
    let _ = std::fs::remove_dir_all(&home);
    assert!(
        out.status.success(),
        "oráculo: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(String::from_utf8(out.stdout).unwrap())
}

const PRELUDE: &str = r#"
import json, os, sys, datetime
repo = sys.argv[1]
sys.path[:0] = [os.path.join(repo, "bin"), os.path.join(repo, "lib")]
import cc_usage
cases = json.loads(sys.argv[2])
def guard(fn):
    try:
        return fn()
    except OverflowError:
        return "ERR:Overflow"
    except (ValueError, TypeError):
        return "ERR:Raises"
"#;

fn mx() -> chrono_tz::Tz {
    chrono_tz::America::Mexico_City
}

fn err(e: UsageError) -> Option<Value> {
    match e {
        UsageError::Overflow => Some("ERR:Overflow".into()),
        UsageError::Raises => Some("ERR:Raises".into()),
        UsageError::Unsure => None,
    }
}

/// Compara con el oráculo los casos que el Rust no declina (`Unsure`).
fn compare(script: &str, cases: &Value, rust: impl Fn(&Value) -> Option<Value>) {
    let input = response_dumps(cases).unwrap();
    let Some(out) = run_python(&format!("{PRELUDE}{script}"), &input) else {
        return;
    };
    let expected = workspace_loads(out.trim_end()).unwrap();
    let (Some(cases), Some(expected)) = (cases.as_array(), expected.as_array()) else {
        panic!("oráculo sin lista");
    };
    assert_eq!(cases.len(), expected.len());
    let mut compared = 0;
    for (case, want) in cases.iter().zip(expected) {
        if let Some(got) = rust(case) {
            assert_eq!(
                response_dumps(&got).unwrap(),
                response_dumps(want).unwrap(),
                "caso {case}"
            );
            compared += 1;
        }
    }
    assert!(compared > 0);
}

fn input(raw: &str) -> Value {
    workspace_loads(raw).unwrap()
}

const NUMERIC_CASES: &str = r#"[null, "", " 1_000 ", "1.5", 3.9, -3.9, NaN, Infinity, -Infinity, true, false,
  "abc", "-12", "+7", [1], {}, 12345678901234, 1e20, " 42\n", "4_2", "_42", "0x10", "inf", "nan",
  "1e3", " -2.5 ", "٣", 0, 0.0, -0.0, "--1", "1__0"]"#;

#[test]
fn as_int_and_as_float_match_python() {
    let cases = input(NUMERIC_CASES);
    compare(
        "print(json.dumps([guard(lambda: cc_usage._as_int(c, 7)) for c in cases]))",
        &cases,
        |c| match usage_state::as_int(c, 7) {
            Ok(n) => Some(n.into()),
            Err(e) => err(e),
        },
    );
    compare(
        "print(json.dumps([guard(lambda: cc_usage._as_float(c, 0.5)) for c in cases]))",
        &cases,
        |c| match usage_state::as_float(c, 0.5) {
            Ok(x) => Some(usage_state::float_value(x)),
            Err(e) => err(e),
        },
    );
}

#[test]
fn text_and_real_model_match_python() {
    let cases = input(
        r#"[null, "x", 1, 1.0, 0.1, 1e16, 1e-5, -0.0, true, false, Infinity, NaN, 123456789012345678,
        "", "  claude-fable-5  ", "<synthetic>", "--model", 5, "\u001c gpt  ", "  ", 0, 2.5e-7,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]"#,
    );
    compare(
        "print(json.dumps([[guard(lambda: cc_usage._text(c)), guard(lambda: cc_usage._real_model(c))] for c in cases]))",
        &cases,
        |c| {
            let text = usage_state::text(c).ok()?;
            let model = usage_state::real_model(c).ok()?;
            Some(json!([text, model]))
        },
    );
}

#[test]
fn epochs_match_python_310() {
    let cases = input(
        r#"[1.9, -1.9, true, "", 0, "1791115200", "1791115200.7", "inf", "-inf", "nan", "NaN",
        "2026-10-04T10:00:00Z", "2026-10-04T10:00:00", "2026-10-04", "2026-10-04 10:00",
        "2021-04-04T02:30:00", "2021-10-31T01:30:00", "2021-10-31T01:30:00.5", "2021-10-31T00:59:59.999999",
        [1], {"a": 1}, "garbage", NaN, Infinity, "2026-10-04T10:00:00.123Z", "2026-10-04T10:00:00.12Z",
        "2026-10-04T10:00:00+05:30", "2026-10-04T10:00:00-05:30:15.123456", "2026-10-04T10:00 +05:30",
        "2026-10-04T10:00:00:123", "2026-10-04T10:00:00:12", "2026-10-04T10:00\u0000", "2026-10-04ñ10:00",
        "0001-01-01T00:00:00", "0001-01-01T12:00:00", "9999-12-31T23:59:59", "9999-12-31T23:59:59-23:59",
        "2026-10-04T24:00", "2026-02-30", "2026-10-04T10:00+05:60", "2026-10-04T10:00:00+24:00",
        "2026-10-04T10:00:00+05:30:15.5", "2026-10-04T10:00:00+05:30:15:123456", "2026-10-04x",
        "2026-10-04T10:00:60", "1969-12-31T23:59:59.5", "1969-12-31T23:59:59.5+00:00", "١٢", " 12 ",
        "1e3", "12abc", "2026-10-04T10:00:00+00:00:00.500000", "2026-10-04T10:00:00-00:00:00.999999",
        "2026-10-04T10:00:00-00:00:01.500000"]"#,
    );
    let zone = mx();
    compare(
        r#"print(json.dumps([[guard(lambda: cc_usage._as_epoch(c)),
            cc_usage._iso_epoch(c) if isinstance(c, str) else None] for c in cases]))"#,
        &cases,
        |c| {
            let epoch = match usage_state::as_epoch(c, &zone) {
                Ok(n) => Value::from(n),
                Err(e) => err(e)?,
            };
            let iso = c
                .as_str()
                .map_or(Value::Null, |s| usage_state::iso_epoch(s, &zone).into());
            Some(json!([epoch, iso]))
        },
    );
}

#[test]
fn day_start_matches_python_including_dst_folds() {
    // Mexico City tuvo horario de verano hasta 2022: 2021-04-04 y 2021-10-31.
    let cases = json!([
        1_791_115_200i64,
        1_617_525_000i64,
        1_617_530_400i64,
        1_635_660_000i64,
        1_635_663_600i64,
        1_635_667_200i64,
        1_635_670_800i64,
        0,
        -86_400
    ]);
    let zone = mx();
    compare(
        r#"print(json.dumps([int(datetime.datetime.fromtimestamp(c).replace(hour=0, minute=0, second=0,
            microsecond=0).timestamp()) for c in cases]))"#,
        &cases,
        |c| zone.day_start(c.as_i64()?).map(Value::from),
    );
}

#[test]
fn normalize_pane_identity_matches_python() {
    let cases = input(
        r#"[{"session": "s1", "pane": "%1", "cwd": "/r", "agent": "codex", "pid": "12", "model": "<synthetic>", "started_at": 5.7},
        {"session": "s2", "pane_pwd": "/q", "provider": "x", "tab_label": "T", "pid": 0, "agent_pid": 3.2, "git_root": "/g"},
        {},
        {"session": "s3", "pid": "bad"},
        {"session": "s3", "pid": Infinity},
        {"session": 12, "agent": "", "model": 4.5, "reasoning_effort": "high", "cwd": "/a/é"},
        {"session": "s1", "tab_label": "propia", "agent": "grok", "started_at": "17"},
        {"z": [1, {"b": 2, "a": NaN}], "session": "s9", "agent": true}]"#,
    );
    let mut labels = HashMap::new();
    labels.insert("s1".to_string(), "Etiqueta".to_string());
    labels.insert("s2".to_string(), String::new());
    compare(
        r#"labels = {"s1": "Etiqueta", "s2": ""}
print(json.dumps([guard(lambda: cc_usage.normalize_pane_identity(c, labels, 1791115200)) for c in cases]))"#,
        &cases,
        |c| match usage_state::normalize_pane_identity(c, &labels, 1_791_115_200) {
            Ok(row) => Some(Value::Object(row)),
            Err(e) => err(e),
        },
    );
}

#[test]
fn percentile_and_wilson_match_python() {
    let cases = json!([
        [[], 0.5],
        [[5], 0.9],
        [[3, 1, 2], 0.5],
        [[1, 2, 3, 4], 0.9],
        [[1, 2, 3, 4], -1],
        [[10, 7, 7, 1, 1000], 2],
        [[1500, 2200, 1800], 0.5],
        [[0, 3], 0.25]
    ]);
    compare(
        "print(json.dumps([cc_usage.percentile(c[0], c[1]) for c in cases]))",
        &cases,
        |c| {
            let values: Vec<f64> = c
                .get(0)?
                .as_array()?
                .iter()
                .filter_map(Value::as_f64)
                .collect();
            let q = c.get(1)?.as_f64()?;
            Some(usage_state::percentile(&values, q).map_or(Value::Null, usage_state::float_value))
        },
    );
    let pairs = json!([
        [0, 0],
        [3, 10],
        [10, 10],
        [0, 5],
        [7, 13],
        [1, 1],
        [12, 20],
        [999, 1000]
    ]);
    compare(
        "print(json.dumps([list(cc_usage.wilson_interval(c[0], c[1])) for c in cases]))",
        &pairs,
        |c| {
            let (lo, hi) = usage_state::wilson_interval(c.get(0)?.as_i64()?, c.get(1)?.as_i64()?);
            Some(json!([
                usage_state::float_value(lo),
                usage_state::float_value(hi)
            ]))
        },
    );
}

#[test]
fn env_text_matches_python_parse_env_file() {
    let raw = "export A=1\n# c\nB = 'two' \n C=\"3\"\nD='x\"\nnoeq\n=v\nE=a=b\nexport  F = 4 \nA=9\r\nG=\u{e9}\nH\u{e9}=1\n'\nI=''\nJ=\"\nexport\nK.L=1\n\u{1c}M=2\u{1c}\n";
    let cases = json!([raw]);
    compare(
        r#"path = os.path.join(os.getcwd(), "usage.env")
open(path, "w").write(cases[0])
values = cc_usage._parse_env_file(path)
print(json.dumps([{k: v for k, v in values.items() if k.isascii()}]))"#,
        &cases,
        |c| {
            let mut map = Map::new();
            for (k, v) in usage_state::parse_env_text(c.as_str()?) {
                map.insert(k, v.into());
            }
            Some(Value::Object(map))
        },
    );
}

#[test]
fn attach_token_counts_matches_python() {
    let cases = input(
        r#"[[[{"provider": "codex", "tokens_7d": 0}, {"provider": "claude", "tokens_today": 5}, {"provider": "grok"}, {"id": "x"}],
            {"items": [{"metric": "tokens", "provider": "codex", "window": "weekly", "used": 70},
                       {"metric": "tokens", "provider": "codex", "window": "daily", "used": 0},
                       {"metric": "tokens", "provider": "claude", "window": "daily", "used": 9},
                       {"metric": "tokens", "provider": "claude", "window": "weekly", "used": 90},
                       {"metric": "usd", "provider": "grok", "window": "weekly", "used": 3},
                       {"metric": "tokens", "provider": "", "window": "weekly", "used": 3},
                       {"metric": "tokens", "provider": "grok", "window": "hourly", "used": 3}]}],
          [[{"provider": "codex"}], {}],
          [[{"provider": "codex"}], {"items": null}]]"#,
    );
    compare(
        "print(json.dumps([cc_usage.attach_token_counts(c[0], c[1]) for c in cases]))",
        &cases,
        |c| {
            let mut limits: Vec<Map<String, Value>> = c
                .get(0)?
                .as_array()?
                .iter()
                .filter_map(|v| v.as_object().cloned())
                .collect();
            usage_state::attach_token_counts(&mut limits, c.get(1)?);
            Some(Value::Array(
                limits.into_iter().map(Value::Object).collect(),
            ))
        },
    );
}

#[test]
fn credential_health_is_cc_dash_shape() {
    let mut env = Map::new();
    env.insert("OPENAI_ADMIN_KEY".into(), json!("sk"));
    env.insert("ANTHROPIC_ADMIN_KEY".into(), json!(""));
    assert_eq!(
        response_dumps(&usage_state::credential_health(&env)).unwrap(),
        "{\"openai\": {\"provider\": \"openai\", \"configured\": true, \"status\": \"configured\"}, \
         \"anthropic\": {\"provider\": \"anthropic\", \"configured\": false, \"status\": \"missing\"}}"
    );
}

struct Utc;
impl LocalZone for Utc {
    fn offset_at(&self, _epoch: i64) -> Option<i64> {
        Some(0)
    }
}

#[test]
fn python_conversions() {
    assert_eq!(usage_state::as_int(&json!(" 1_000 "), 0), Ok(1000));
    assert_eq!(usage_state::as_int(&json!("1.5"), 7), Ok(7));
    assert_eq!(usage_state::as_int(&json!(3.9), 0), Ok(3));
    assert_eq!(
        usage_state::as_epoch(&json!("inf"), &Utc),
        Err(UsageError::Overflow)
    );
    // Python 3.10: sin `Z`, fracción de 3 o 6 dígitos, zona con dos puntos.
    assert_eq!(
        usage_state::fromisoformat_epoch("2026-10-04T10:00:00+00:00", &Utc),
        Some(1_791_108_000)
    );
    assert_eq!(
        usage_state::fromisoformat_epoch("2026-10-04T10:00:00Z", &Utc),
        None
    );
    assert_eq!(
        usage_state::fromisoformat_epoch("2026-10-04 10:00:00.1234", &Utc),
        None
    );
    assert_eq!(
        usage_state::fromisoformat_epoch("2026-10-04T10:00:00.123", &Utc),
        Some(1_791_108_000)
    );
    assert_eq!(usage_state::text(&json!(1.0)).unwrap(), "1.0");
    assert_eq!(usage_state::text(&json!(true)).unwrap(), "True");
}

#[test]
fn window_percent_is_int_100_when_rounding_reaches_100() {
    let turns = vec![
        json!({"provider": "claude", "total_tokens": 1000, "turn_finished_at": 10})
            .as_object()
            .cloned()
            .unwrap(),
    ];
    let mut settings = Map::new();
    settings.insert("CLAUDE_DAILY_TOKEN_LIMIT".into(), json!("500"));
    let state = usage_state::build_state(
        20,
        vec![],
        &usage_state::StateTurns::from_rows(&turns),
        &[],
        &[],
        &settings,
    )
    .unwrap();
    let item = &state["windows"]["items"][2];
    assert_eq!(response_dumps(&item["percent"]).unwrap(), "100");
    settings.insert("CLAUDE_DAILY_TOKEN_LIMIT".into(), json!("1001"));
    let state = usage_state::build_state(
        20,
        vec![],
        &usage_state::StateTurns::from_rows(&turns),
        &[],
        &[],
        &settings,
    )
    .unwrap();
    assert_eq!(
        response_dumps(&state["windows"]["items"][2]["percent"]).unwrap(),
        "99.9"
    );
}
