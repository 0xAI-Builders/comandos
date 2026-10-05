use comandos_core::{analytics_week as a, json::workspace_loads};
use serde_json::{Value, json};
fn rows(input: &Value) -> a::WeekRows {
    a::WeekRows::from_rows(
        input["turns"].as_array().unwrap(),
        input["spans"].as_array().unwrap(),
        comandos_core::focus::float(&input["now"]).unwrap(),
    )
}
fn week<'a>(input: &'a Value, rows: &'a a::WeekRows) -> a::WeekInput<'a> {
    a::WeekInput {
        now: comandos_core::focus::float(&input["now"]).unwrap(),
        offset: input["offset"].as_i64().unwrap(),
        limits: input["limits"].as_array().unwrap(),
        rows,
        snapshots: input["snapshots"].as_array().unwrap(),
        records: input["records"].as_array().unwrap(),
        tz_name: input["tz_name"].as_str().unwrap(),
    }
}
#[test]
fn aliases_projects_and_duration_labels_follow_reference() {
    assert_eq!(a::account_of(&json!(" unknown ")).unwrap(), "main");
    assert_eq!(a::account_of(&json!(" Work ")).unwrap(), "Work");
    assert_eq!(a::project_of(&json!("/x/ComandOS///")).unwrap(), "ComandOS");
    assert_eq!(a::project_of(&Value::Null).unwrap(), "Sin carpeta");
    assert_eq!(
        a::pomodoro_project(&json!(" ComandOS ⎇ branch ⫽30")).unwrap(),
        "ComandOS"
    );
    assert_eq!(a::pomodoro_project(&json!("X⫽30")).unwrap(), "X⫽30");
    assert_eq!(a::fmt_left(&json!(3599.9)).unwrap(), "59 min");
}
#[test]
fn sidebar_keeps_accounts_and_opencode_sessions_models_separate() {
    let accounts = vec![json!({"id":"claude:main","provider":"claude","alias":"main","week":70})];
    let original = accounts.clone();
    let turns = vec![
        json!({"provider":"groq","agent":"opencode","account":"unknown","finished":100,"tokens":100,"cost":0.5,"session":"s","model":"m"}),
        json!({"provider":"opencode","account":"main","finished":99,"tokens":200,"cost":0.25,"session":"s","model":"m"}),
        json!({"provider":"opencode","account":"work","finished":99,"tokens":400,"cost":2,"session":"s","model":"other"}),
    ];
    let out = a::sidebar_accounts(
        &accounts,
        &[json!({"provider":"claude","account":"main","plan":"Max"})],
        &turns,
        100.,
    )
    .unwrap();
    assert_eq!(accounts, original);
    assert_eq!(out[0]["plan"], "Max");
    assert_eq!(out[1]["id"], "opencode:main");
    assert_eq!(
        out[1]["measured"],
        json!({"sessions":1,"tokens":300,"costUsd":0.75,"models":["m"]})
    );
    assert_eq!(out[2]["measured"]["tokens"], 400);
}
#[test]
fn analytics_fixed_source_reference_preserves_dst_and_account_models() {
    let f = workspace_loads(include_str!("analytics_week_fixture.json")).unwrap();
    for (case, row) in f.as_array().unwrap().iter().enumerate() {
        let input = &row["input"];
        let result = match row["kind"].as_str().unwrap() {
            "week" => a::build_week(&week(input, &rows(input))),
            "sessions" => a::sessions(
                input["turns"].as_array().unwrap(),
                input["spans"].as_array().unwrap(),
                input["tz_name"].as_str().unwrap(),
            )
            .map(Value::Array),
            "sidebar" => a::sidebar_accounts(
                input["accounts"].as_array().unwrap(),
                input["limits"].as_array().unwrap(),
                input["turns"].as_array().unwrap(),
                comandos_core::focus::float(&input["now"]).unwrap(),
            )
            .map(Value::Array),
            "account" => a::account_of(input).map(Value::String),
            "project" => a::project_of(input).map(Value::String),
            "pomodoro_project" => a::pomodoro_project(input).map(Value::String),
            "left" => a::fmt_left(input).map(Value::String),
            _ => panic!("unknown kind"),
        };
        if let Some(e) = row["error"].as_str() {
            assert_eq!(result.unwrap_err().kind, e, "{row}");
        } else {
            assert_json_close(
                &result.unwrap_or_else(|e| panic!("case {case} unexpected {e:?}")),
                &row["expected"],
                &format!("case {case}"),
            );
        }
    }
}
#[test]
fn empty_week_has_all_observable_model_fields_without_accounts() {
    let input = json!({"now":1790854560.,"offset":0,"limits":[],"turns":[],"spans":[],"snapshots":[],"records":[],"tz_name":a::TZ});
    let out = a::build_week(&week(&input, &rows(&input))).unwrap();
    assert_eq!(out["days"][0], json!(["2026-10-01", "jue", "1 oct"]));
    assert_eq!(out["week"]["label"], "24 sep – 1 oct");
    for key in ["accounts", "sessions", "waste", "pomodoros"] {
        assert_eq!(out[key], json!([]));
    }
    assert_eq!(out["lastWeek"], json!({}));
}
fn assert_json_close(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => {
            if a.as_str() == b.as_str() {
                return;
            }
            if !a.as_str().contains(['.', 'e', 'E']) && !b.as_str().contains(['.', 'e', 'E']) {
                assert_eq!(a.as_str(), b.as_str(), "{path}");
                return;
            }
            let x = a.as_str().parse::<f64>().unwrap();
            let y = b.as_str().parse::<f64>().unwrap();
            assert!(
                (x.is_nan() && y.is_nan())
                    || x == y
                    || (x.is_finite() && y.is_finite() && (x - y).abs() <= 1e-12 * y.abs().max(1.)),
                "{path}: {actual} != {expected}"
            );
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                assert_json_close(x, y, &format!("{path}/{i}"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (k, v) in b {
                assert_json_close(a.get(k).unwrap(), v, &format!("{path}/{k}"));
            }
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}
#[test]
fn week_rows_do_not_depend_on_turn_and_span_interleaving() {
    let turns = vec![
        json!({"provider":"claude","account":"main","git_root":"/r/a","pane_pwd":"/r/a","started":1790850000,"finished":1790850100,"tokens":10}),
        json!({"provider":"codex","account":"w","git_root":null,"pane_pwd":"/r/b","started":1790851000,"finished":1790851200,"tokens":20}),
    ];
    let spans = vec![
        json!({"provider":"grok","account":"main","git_root":"/r/c","started":1790849000,"finished":1790849500}),
        json!({"provider":"claude","account":"main","git_root":"/r/a","started":1790850100,"finished":1790850300}),
    ];
    let input = json!({"now":1790854560.,"offset":0,"limits":[],"snapshots":[],"records":[],"tz_name":a::TZ});
    let ordered = a::WeekRows::from_rows(&turns, &spans, 1790854560.);
    let mut mixed = a::WeekRows::new(1790854560.);
    mixed.push_span(&spans[0]);
    mixed.push_turn(&turns[0]);
    mixed.push_span(&spans[1]);
    mixed.push_turn(&turns[1]);
    let out = a::build_week(&week(&input, &ordered)).unwrap();
    assert_eq!(out, a::build_week(&week(&input, &mixed)).unwrap());
    assert_eq!(
        out["sessions"].as_array().unwrap().len(),
        a::sessions(&turns, &spans, a::TZ)
            .unwrap()
            .iter()
            .filter(|s| s["d"] == "2026-10-01")
            .count()
    );
}
#[test]
fn sidebar_counts_a_new_opencode_account_only_from_the_turn_that_adds_it() {
    let accounts = vec![json!({"id":"claude:main","provider":"claude","alias":"main"})];
    // `opencode:w` + alias `main` y `opencode` + alias `w:main` dan la misma cuenta.
    let turns = vec![
        json!({"provider":"opencode:w","account":null,"finished":100,"tokens":7}),
        json!({"provider":"codex","account":"x","finished":100,"tokens":"no es un número"}),
        json!({"provider":"opencode","account":"w:main","finished":100,"tokens":5,"model":"m"}),
        json!({"provider":"opencode:w","account":null,"finished":100,"tokens":11}),
        json!({"provider":"claude","account":"main","finished":100,"tokens":3}),
    ];
    let out = a::sidebar_accounts(&accounts, &[], &turns, 100.).unwrap();
    assert_eq!(out[1]["id"], "opencode:w:main");
    assert_eq!(out[1]["alias"], "w:main");
    assert_eq!(
        out[1]["measured"],
        json!({"sessions":0,"tokens":16,"costUsd":0.0,"models":["m"]})
    );
    assert_eq!(out[0]["measured"]["tokens"], 3);
    // Un turno contado que lanza gana al de después; uno que no se cuenta, no lanza.
    let mut bad = turns.clone();
    bad.push(json!({"provider":"claude","account":"main","finished":"x"}));
    bad.insert(
        4,
        json!({"provider":"claude","account":"main","finished":100,"tokens":[1]}),
    );
    let err = a::sidebar_accounts(&accounts, &[], &bad, 100.).unwrap_err();
    assert_eq!(err.kind, "TypeError");
    let rows = a::WeekRows::from_rows(&bad, &[], 100.);
    assert_eq!(
        a::sidebar_accounts_from(&accounts, &[], &rows)
            .unwrap_err()
            .kind,
        "TypeError"
    );
}
