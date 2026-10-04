use comandos_runtime::pane_typing::TmuxResult;
use comandos_runtime::terminal_history::{
    HISTORY_LIMIT, HistoryError, PANE_FORMAT, capture, friendly_path,
};
use serde_json::{Value, json};

const PANES: &str = "%1\t1\t0\t0\t40\t24\tcodex\n%2\t0\t41\t0\t39\t24\tgrok\t/srv/data\n";

fn run(
    data: &Value,
    panes: &str,
    text: &str,
) -> (
    Result<comandos_runtime::terminal_history::HistoryResponse, HistoryError>,
    Vec<Vec<String>>,
) {
    let mut calls = Vec::new();
    let result = capture(
        |args| {
            calls.push(args.iter().map(|s| s.to_string()).collect());
            TmuxResult {
                stdout: if args[0] == "list-panes" {
                    panes.into()
                } else {
                    text.into()
                },
                ..TmuxResult::default()
            }
        },
        data,
        "/home/dev",
    );
    (result, calls)
}

#[test]
fn pointer_selects_split_with_exact_read_only_commands_and_preserves_input() {
    let request = json!({"session":"term-1", "col":50, "row":2});
    let original = request.clone();
    let (result, calls) = run(&request, PANES, "á🙂 same same\n");
    let response = result.unwrap();
    assert_eq!(response.pane, "%2");
    assert_eq!(response.text, "á🙂 same same\n");
    assert_eq!(request, original);
    assert_eq!(
        calls,
        vec![
            vec!["list-panes", "-t", "=term-1", "-F", PANE_FORMAT],
            vec!["capture-pane", "-p", "-J", "-t", "%2", "-S", "-2000"]
        ]
    );
    assert!(response.ok && response.read_only);
    assert!(!response.truncated);
    assert_eq!(response.source, "tmux");
    let json = response.to_json();
    assert_eq!(json["readOnly"], true);
    assert_eq!(json["source"], "tmux");
    assert_eq!(json["lines"], 2000);
    assert_eq!(json["panes"][0]["path"], "");
    assert_eq!(json["panes"][1]["active"], false);
}

#[test]
fn explicit_pane_has_priority_over_coordinates_and_active_pane_is_default() {
    for (request, selected) in [
        (json!({"session":"term-1", "pane":"%2", "col":false}), "%2"),
        (json!({"session":"term-1"}), "%1"),
        (json!({"session":"term-1", "pane":""}), "%1"),
    ] {
        assert_eq!(run(&request, PANES, "").0.unwrap().pane, selected);
    }
}

#[test]
fn foreign_or_missing_pane_never_captures() {
    for (request, panes, error) in [
        (
            json!({"session":"term-1", "pane":"%999"}),
            PANES,
            HistoryError::ForeignPane,
        ),
        (json!({"session":"term-1"}), "", HistoryError::PaneNotFound),
        (
            json!({"session":"term-1"}),
            "%1\t0\t0\t0\t40\t24\tzsh",
            HistoryError::PaneNotFound,
        ),
    ] {
        let (result, calls) = run(&request, panes, "");
        assert_eq!(result.unwrap_err(), error);
        assert_eq!(calls.len(), 1);
    }
}

#[test]
fn session_validation_rejects_injection_unicode_and_length_before_any_command() {
    for session in [
        "",
        "x;kill",
        "x\n",
        "x\\;",
        "á",
        " x",
        "x/y",
        &"x".repeat(121),
    ] {
        let (result, calls) = run(&json!({"session":session}), PANES, "");
        assert_eq!(
            result.unwrap_err(),
            HistoryError::InvalidSession,
            "{session:?}"
        );
        assert!(calls.is_empty());
    }
    for session in ["-a", "a_b-0", &"x".repeat(120)] {
        let (_, calls) = run(&json!({"session":session}), PANES, "");
        assert_eq!(calls[0][2], format!("={session}"));
    }
}

#[test]
fn session_keeps_legacy_scalar_conversion_and_empty_values_are_invalid() {
    for (session, target) in [
        (json!(123), "=123"),
        (json!(true), "=True"),
        (json!(-1), "=-1"),
    ] {
        let (_, calls) = run(&json!({"session":session}), PANES, "");
        assert_eq!(calls[0][2], target);
    }
    for session in [
        json!(0),
        json!(false),
        Value::Null,
        json!([]),
        json!({}),
        json!(1.0),
    ] {
        let (result, calls) = run(&json!({"session":session}), PANES, "");
        assert_eq!(result.unwrap_err(), HistoryError::InvalidSession);
        assert!(calls.is_empty());
    }
}

#[test]
fn lines_are_bounded_integers_and_boolean_is_invalid() {
    for lines in [
        json!(true),
        json!(false),
        json!(0),
        json!(-1),
        json!(5001),
        json!(1.0),
        json!("2"),
        Value::Null,
    ] {
        let (result, calls) = run(&json!({"session":"term-1", "lines":lines}), PANES, "");
        assert_eq!(result.unwrap_err(), HistoryError::InvalidLines, "{lines}");
        assert!(calls.is_empty());
    }
    for lines in [1, 5000] {
        let (result, calls) = run(&json!({"session":"term-1", "lines":lines}), PANES, "");
        assert_eq!(result.unwrap().lines, lines);
        assert_eq!(calls[1][6], format!("-{lines}"));
    }
}

#[test]
fn coordinate_boundaries_exclude_right_and_bottom_and_split_gap() {
    for (col, row, selected) in [
        (0, 0, Some("%1")),
        (39, 23, Some("%1")),
        (40, 0, None),
        (41, 0, Some("%2")),
        (79, 23, Some("%2")),
        (80, 0, None),
        (0, 24, None),
    ] {
        let (result, calls) = run(
            &json!({"session":"term-1", "col":col, "row":row}),
            PANES,
            "",
        );
        if let Some(pane) = selected {
            assert_eq!(result.unwrap().pane, pane);
        } else {
            assert_eq!(result.unwrap_err(), HistoryError::NoPaneAtPosition);
            assert_eq!(calls.len(), 1);
        }
    }
}

#[test]
fn partial_negative_boolean_and_noninteger_coordinates_never_capture() {
    for coordinates in [
        json!({"col":0}),
        json!({"row":0}),
        json!({"col":-1,"row":0}),
        json!({"col":true,"row":0}),
        json!({"col":0,"row":false}),
        json!({"col":1.0,"row":0}),
        json!({"col":"1","row":0}),
    ] {
        let mut request = coordinates;
        request["session"] = json!("term-1");
        let (result, calls) = run(&request, PANES, "");
        assert_eq!(result.unwrap_err(), HistoryError::InvalidCoordinates);
        assert_eq!(calls.len(), 1);
    }
}

#[test]
fn pane_title_and_friendly_path_limits_count_unicode_characters() {
    let listing = format!(
        "%1\t1\t0\t0\t40\t24\t{}\t/home/dev/{}",
        "🙂".repeat(101),
        "á".repeat(301)
    );
    let response = run(&json!({"session":"term-1"}), &listing, "").0.unwrap();
    assert_eq!(response.panes[0].title, "🙂".repeat(100));
    assert_eq!(response.panes[0].path, format!("~/{}", "á".repeat(298)));
}

#[test]
fn friendly_home_shortening_matches_only_path_prefix_boundaries() {
    for (path, home, expected) in [
        ("/home/dev", "/home/dev/", "~"),
        ("/home/dev/app", "/home/dev", "~/app"),
        ("/home/developer", "/home/dev", "/home/developer"),
        ("/srv/data", "", "/srv/data"),
        ("/srv/data", "/", "/srv/data"),
    ] {
        assert_eq!(friendly_path(path, home), expected);
    }
}

#[test]
fn malformed_numeric_geometry_and_overflow_return_typed_errors_without_capture() {
    for listing in [
        "%1\t1\tx\t0\t40\t24\tzsh",
        "%1\t1\t9223372036854775807\t0\t40\t24\tzsh",
        "%1\t1\t0\t0\t-1\t24\tzsh",
        "%1\t1\t0\t0\t40\tnope\tzsh",
        "\t1\t0\t0\t40\t24\tzsh",
    ] {
        let (result, calls) = run(&json!({"session":"term-1"}), listing, "");
        assert_eq!(result.unwrap_err(), HistoryError::MalformedPanes);
        assert_eq!(calls.len(), 1);
    }
}

#[test]
fn large_integer_coordinates_miss_the_pane_without_overflow() {
    let (result, calls) = run(
        &json!({"session":"term-1", "col":u64::MAX, "row":0}),
        PANES,
        "",
    );
    assert_eq!(result.unwrap_err(), HistoryError::NoPaneAtPosition);
    assert_eq!(calls.len(), 1);
}

#[test]
fn malformed_pane_ids_never_reach_capture_even_if_active_or_explicitly_selected() {
    for pane in ["%", "%1;", "-a", "foreign", "%á"] {
        let listing = format!("{pane}\t1\t0\t0\t40\t24\tzsh");
        for request in [
            json!({"session":"term-1"}),
            json!({"session":"term-1", "pane":pane}),
        ] {
            let (result, calls) = run(&request, &listing, "");
            assert_eq!(result.unwrap_err(), HistoryError::MalformedPanes);
            assert_eq!(calls.len(), 1);
        }
    }
}

#[test]
fn arbitrary_precision_integer_coordinates_remain_valid_and_miss_without_capture() {
    let request: Value = serde_json::from_str("{\"session\":\"term-1\",\"col\":99999999999999999999999999999999999999999999999,\"row\":0}").unwrap();
    let (result, calls) = run(&request, PANES, "");
    assert_eq!(result.unwrap_err(), HistoryError::NoPaneAtPosition);
    assert_eq!(calls.len(), 1);
}

#[test]
fn path_field_preserves_tabs_after_seven_separators() {
    let response = run(
        &json!({"session":"term-1"}),
        "%1\t1\t0\t0\t40\t24\tzsh\t/srv/a\tb",
        "",
    )
    .0
    .unwrap();
    assert_eq!(response.panes[0].path, "/srv/a\tb");
}

#[test]
fn malformed_short_rows_are_skipped_and_optional_path_is_compatible() {
    let listing = format!("malformed\n{PANES}");
    let response = run(&json!({"session":"term-1"}), &listing, "").0.unwrap();
    assert_eq!(response.panes.len(), 2);
    assert_eq!(response.panes[0].path, "");
}

#[test]
fn command_failures_stop_and_return_distinct_errors() {
    for fail_at in [1, 2] {
        let mut calls = 0;
        let error = capture(
            |_| {
                calls += 1;
                TmuxResult {
                    returncode: i32::from(calls == fail_at),
                    stdout: PANES.into(),
                    stderr: "failed".into(),
                }
            },
            &json!({"session":"term-1"}),
            "",
        )
        .unwrap_err();
        assert_eq!(calls, fail_at);
        assert_eq!(
            error,
            if fail_at == 1 {
                HistoryError::SessionNotFound
            } else {
                HistoryError::CaptureFailed
            }
        );
    }
}

#[test]
fn trailing_history_limit_counts_characters_and_reports_truncation_truthfully() {
    for (text, truncated, expected) in [
        (
            "🙂".repeat(HISTORY_LIMIT),
            false,
            "🙂".repeat(HISTORY_LIMIT),
        ),
        (
            format!("discard{}á", "🙂".repeat(HISTORY_LIMIT - 1)),
            true,
            format!("{}á", "🙂".repeat(HISTORY_LIMIT - 1)),
        ),
    ] {
        let response = run(&json!({"session":"term-1"}), PANES, &text).0.unwrap();
        assert_eq!(response.text, expected);
        assert_eq!(response.truncated, truncated);
        assert!(response.read_only);
        assert_eq!(response.to_json()["truncated"], truncated);
    }
}
