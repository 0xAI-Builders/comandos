use comandos_core::{focus as f, pomodoro as p};
use serde_json::{Value, json};
const MIN: i64 = 60_000;
fn block(id: &str, start: i64, minutes: i64, status: &str) -> Value {
    json!({"blockId":id,"mode":"focus","status":status,"activeMs":minutes*MIN,"startedAtMs":start,"endedAtMs":start+minutes*MIN,"provenance":"measured"})
}
#[test]
fn elapsed_excludes_pause_and_clamps_only_elapsed() {
    let b = json!({"status":"paused","targetMs":25*MIN,"activeMs":2*MIN,"resumedAtMs":null});
    assert_eq!(p::elapsed_ms(&b, 9_000_000), 2 * MIN);
    assert_eq!(p::remaining_ms(&b, 9_000_000), 23 * MIN);
    let b = json!({"status":"running","targetMs":25*MIN,"activeMs":2*MIN,"resumedAtMs":1000});
    assert_eq!(p::elapsed_ms(&b, 1000 + 3 * MIN), 5 * MIN);
    assert_eq!(p::elapsed_ms(&b, 0), 2 * MIN);
    assert_eq!(p::elapsed_ms(&b, i64::MAX), 25 * MIN);
}
#[test]
fn iana_midnights_cover_historical_dst_current_and_other_zones() {
    assert_eq!(
        p::local_day(1_656_651_599_999, p::REPORT_TZ).unwrap(),
        "2022-06-30"
    );
    assert_eq!(
        p::local_day(1_791_172_799_999, p::REPORT_TZ).unwrap(),
        "2026-10-04"
    );
    assert_eq!(p::local_day(0, "Asia/Tokyo").unwrap(), "1970-01-01");
    assert!(p::local_day(0, "Invalid/Zone").is_err());
}
#[test]
fn policy_is_exact_and_progress_deduplicates_and_excludes_breaks() {
    let policy = f::policy_v1();
    assert_eq!(policy["xpPerMinute"], 10);
    assert_eq!(policy["xpPerLevel"], 1000);
    assert_eq!(policy["achievements"][2]["title"], "3 días seguidos");
    let a = block("a", 2_000_000_000_000, 25, "completed");
    let b = block("b", 2_000_000_000_000, 12, "cancelled");
    let mut br = block("br", 2_000_000_000_000, 5, "completed");
    br["mode"] = json!("break");
    let out = f::progress(&[a.clone(), b, br, a], &policy, 2_000_000_000_000, None).unwrap();
    assert_eq!(out["xp"], 370);
    assert_eq!(out["focusMinutes"], 37);
    assert_eq!(out["todayMinutes"], 37);
    assert_eq!(out["completedBlocks"], 1);
    assert_eq!(out["levelPct"], 37.0);
    assert_eq!(out["xpToNextLevel"], 630);
}
#[test]
fn activation_and_fractional_minutes_and_trial_policy() {
    let policy = json!({"policyVersion":"trial","xpPerMinute":1,"xpPerLevel":50,"dailyGoalMinutes":30,"countCancelledActive":false,"achievements":[]});
    let mut a = block("a", 2_000_000_000_000, 101, "completed");
    a["activeMs"] = json!(101 * MIN + 59_999);
    let b = block("b", 2_000_000_000_000, 30, "cancelled");
    let out = f::progress(&[a.clone(), b], &policy, 2_000_000_000_000, None).unwrap();
    assert_eq!(out["xp"], 101);
    assert_eq!(out["level"], 3);
    assert_eq!(
        f::progress(&[a], &policy, 0, Some(3_000_000_000_000)).unwrap()["xp"],
        0
    );
}
#[test]
fn streak_survives_yesterday_but_keeps_max_achievement() {
    let start = 2_000_000_000_000;
    let blocks = (0..3)
        .map(|i| block(&i.to_string(), start + i * 86_400_000, 25, "completed"))
        .collect::<Vec<_>>();
    let out = f::progress(&blocks, &f::policy_v1(), start + 3 * 86_400_000, None).unwrap();
    assert_eq!(out["streakDays"], 3);
    let out = f::progress(&blocks, &f::policy_v1(), start + 10 * 86_400_000, None).unwrap();
    assert_eq!(out["streakDays"], 0);
    assert_eq!(out["maxStreakDays"], 3);
    assert_eq!(out["achievements"][2]["unlocked"], true);
}
#[test]
fn percentage_uses_python_binary_rounding_at_decimal_ties() {
    let mut policy = f::policy_v1();
    policy["xpPerMinute"] = json!(1);
    policy["xpPerLevel"] = json!(4000);
    let out = f::progress(
        &[block("a", 2_000_000_000_000, 1, "completed")],
        &policy,
        2_000_000_000_000,
        None,
    )
    .unwrap();
    assert_eq!(out["levelPct"], 0.03);
}
#[test]
fn legacy_integer_coercion_keeps_precision_sign_and_unicode() {
    for (input, expected) in [
        (json!(" +١_٢ "), "12"),
        (json!("-000"), "0"),
        (json!(-1.9), "-1"),
        (json!(true), "1"),
    ] {
        assert_eq!(f::integer_string(&input).unwrap(), expected);
    }
    assert!(f::integer_string(&json!("1__2")).is_err());
    let huge: Value = serde_json::from_str("99999999999999999999999999999999999").unwrap();
    assert_eq!(
        f::integer_string(&huge).unwrap(),
        "99999999999999999999999999999999999"
    );
    assert!(f::int(&huge).is_err());
}
#[test]
fn iana_conversion_distinguishes_2022_summer_offset_and_modern_midnight() {
    assert_eq!(
        p::local_day(1_656_651_600_000, p::REPORT_TZ).unwrap(),
        "2022-07-01"
    );
    assert_eq!(
        p::local_day(1_791_179_999_999, p::REPORT_TZ).unwrap(),
        "2026-10-04"
    );
    assert_eq!(
        p::local_day(1_791_180_000_000, p::REPORT_TZ).unwrap(),
        "2026-10-05"
    );
    assert_eq!(
        p::local_day(1_656_651_600_000, "America/New_York").unwrap(),
        "2022-07-01"
    );
}
#[test]
fn python_destination_number_strings_keep_nonfinite_spelling() {
    let overflow: Value = serde_json::from_str("1e9999").unwrap();
    assert_eq!(p::text(&overflow), "inf");
    for (raw, expected) in [("NaN", "nan"), ("Infinity", "inf"), ("-Infinity", "-inf")] {
        let value = comandos_core::json::workspace_loads(raw).unwrap();
        assert_eq!(p::text(&value), expected);
    }
}
#[test]
fn strict_request_integers_and_optional_cycles_exclude_nonfinite_numbers() {
    for raw in ["NaN", "Infinity", "-Infinity", "1e9999"] {
        let n = comandos_core::json::workspace_loads(raw).unwrap();
        assert!(p::integer(&n, "targetMs").is_err(), "{raw}");
        assert_eq!(p::optional_integer(&n, 1, 99), None, "{raw}");
    }
}

#[test]
fn activation_compares_python_numbers_before_storage_conversion() {
    let policy = f::policy_v1();
    for (raw, activation, expected) in [
        ("2000000060000.0", 2_000_000_000_000, true),
        (
            "9999999999999999999999999999999999999999999999999999999999",
            2_000_000_000_000,
            true,
        ),
        (
            "-9999999999999999999999999999999999999999999999999999999999",
            2_000_000_000_000,
            false,
        ),
        ("9007199254740992.0", 9_007_199_254_740_993, false),
        ("-9007199254740992.0", -9_007_199_254_740_991, false),
        ("-0.5", 0, false),
        ("0.5", 0, true),
        ("Infinity", 2_000_000_000_000, true),
        ("-Infinity", 2_000_000_000_000, false),
        ("NaN", 2_000_000_000_000, true),
        ("true", 1, true),
    ] {
        let mut b = block("a", 2_000_000_000_000, 1, "completed");
        b["endedAtMs"] = comandos_core::json::workspace_loads(raw).unwrap();
        assert_eq!(
            f::eligible(&b, &policy, Some(activation)),
            expected,
            "{raw} compared with {activation}"
        );
    }
    let mut b = block("decimal", 2_000_000_000_000, 1, "completed");
    b["endedAtMs"] = json!(2_000_000_060_000.0);
    assert_eq!(
        f::progress(&[b], &policy, 2_000_000_000_000, Some(2_000_000_000_000)).unwrap()["xp"],
        10
    );
}

#[test]
fn container_repr_uses_python_printability_and_escape_widths() {
    for (input, expected) in [
        (json!(["a\u{a0}b"]), "['a\\xa0b']"),
        (json!(["a\u{2028}b"]), "['a\\u2028b']"),
        (json!(["a\u{200d}b"]), "['a\\u200db']"),
        (json!(["a\u{e0001}b"]), "['a\\U000e0001b']"),
        (json!(["a\u{0378}b"]), "['a\\u0378b']"),
        (json!(["a\u{e000}b"]), "['a\\ue000b']"),
        (json!(["a b"]), "['a b']"),
        (json!(["a🦀b"]), "['a🦀b']"),
        (json!({"a\u{2028}b":"x\u{a0}y"}), "{'a\\u2028b': 'x\\xa0y'}"),
    ] {
        assert_eq!(p::text(&input), expected, "{input}");
    }
    assert_eq!(p::text(&json!("a\u{2028}b")), "a\u{2028}b");
    let expected = format!("['{}']", "\\xa0".repeat(40))
        .chars()
        .take(160)
        .collect::<String>();
    assert_eq!(p::text(&json!(["\u{a0}".repeat(40)])), expected);
}
