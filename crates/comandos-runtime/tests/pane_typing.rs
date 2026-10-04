use comandos_runtime::pane_typing::{
    MAX_CHARS, PaneTypingLocks, TmuxResult, TypingOptions, type_literal, validate,
};
use std::sync::{Arc, Barrier, mpsc};

#[test]
fn literal_unicode_and_semicolon_use_exact_args_without_enter() {
    let mut calls = Vec::new();
    let mut slept = Vec::new();
    let result = type_literal(
        |args| {
            calls.push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            TmuxResult::default()
        },
        "%3",
        "á🙂; Enter",
        |step| slept.push(step),
        TypingOptions {
            delay: 0.02,
            budget: 10.0,
        },
    )
    .unwrap();
    assert_eq!(result.typed, 9);
    assert_eq!(
        calls
            .iter()
            .map(|args| args[5].as_str())
            .collect::<Vec<_>>(),
        ["á", "🙂", "\\;", " ", "E", "n", "t", "e", "r"]
    );
    assert!(
        calls
            .iter()
            .all(|args| args[..5] == ["send-keys", "-t", "%3", "-l", "--"])
    );
    assert!(calls.iter().all(|args| !args.iter().any(|s| s == "Enter")));
    assert_eq!(slept, vec![0.02; 8]);
}

#[test]
fn validation_counts_unicode_characters_and_preserves_text() {
    let text = "🙂".repeat(MAX_CHARS);
    assert_eq!(validate(&text).unwrap(), text);
    let error = validate(&(text + "á")).unwrap_err();
    assert_eq!(error.code, "invalid");
    assert_eq!(error.typed, 0);
    assert_eq!(error.message, "Texto demasiado largo (máx. 2000)");
    assert_eq!(validate(" á ").unwrap(), " á ");
}

#[test]
fn invalid_input_makes_no_callback_or_sleep_calls() {
    for text in [
        "", "  ", "\u{2003}", "ls\n", "a\rb", "\x1b[A", "a\tb", "a\x7fb", "\0",
    ] {
        let error = type_literal(
            |_| panic!("invalid text reached tmux"),
            "%3",
            text,
            |_| panic!("invalid text slept"),
            TypingOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.code, "invalid", "{text:?}");
        assert_eq!(error.typed, 0);
    }
    assert_eq!(validate("").unwrap_err().message, "Texto vacío");
    assert_eq!(
        validate("x\n").unwrap_err().message,
        "El texto no puede contener saltos de línea ni caracteres de control"
    );
}

#[test]
fn sleeps_only_between_characters_and_fits_total_budget() {
    for (text, expected, step) in [("x".repeat(200), 199, 1.2 / 199.0), ("x".into(), 0, 0.022)] {
        let mut slept = Vec::new();
        let result = type_literal(
            |_| TmuxResult::default(),
            "%3",
            &text,
            |delay| slept.push(delay),
            TypingOptions::default(),
        )
        .unwrap();
        assert_eq!(result.typed, text.chars().count());
        assert_eq!(slept.len(), expected);
        assert!(slept.iter().all(|delay| (delay - step).abs() < 1e-12));
        assert!(slept.iter().sum::<f64>() <= 1.2 + 1e-9);
    }
}

#[test]
fn first_and_middle_failure_report_only_successful_characters() {
    for (failure, stderr, message) in [
        (1, "", "tmux send-keys falló"),
        (3, " no such pane\n", "no such pane"),
    ] {
        let mut calls = 0;
        let mut slept = Vec::new();
        let error = type_literal(
            |_| {
                calls += 1;
                TmuxResult {
                    returncode: i32::from(calls == failure),
                    stderr: stderr.into(),
                    ..TmuxResult::default()
                }
            },
            "%3",
            "abcdef",
            |step| slept.push(step),
            TypingOptions::default(),
        )
        .unwrap_err();
        assert_eq!(calls, failure);
        assert_eq!(error.typed, failure - 1);
        assert_eq!(error.code, "tmux");
        assert_eq!(error.message, message);
        assert_eq!(slept.len(), failure - 1);
    }
}

#[test]
fn lock_release_is_idempotent_and_other_panes_remain_available() {
    let locks = PaneTypingLocks::default();
    assert!(locks.acquire("%3"));
    assert!(!locks.acquire("%3"));
    assert!(locks.acquire("%4"));
    locks.release("%unknown");
    assert!(!locks.acquire("%3"));
    locks.release("%3");
    locks.release("%3");
    assert!(locks.acquire("%3"));
}

#[test]
fn concurrent_same_pane_has_one_owner_while_different_panes_can_acquire() {
    let locks = Arc::new(PaneTypingLocks::default());
    let barrier = Arc::new(Barrier::new(9));
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|scope| {
        for index in 0..8 {
            let locks = Arc::clone(&locks);
            let barrier = Arc::clone(&barrier);
            let tx = tx.clone();
            scope.spawn(move || {
                barrier.wait();
                let same = locks.acquire("%3");
                let other = locks.acquire(&format!("%other{index}"));
                tx.send((same, other)).unwrap();
            });
        }
        barrier.wait();
    });
    let results = rx.iter().take(8).collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|(same, _)| *same).count(), 1);
    assert!(results.iter().all(|(_, other)| *other));
    locks.release("%3");
    assert!(locks.acquire("%3"));
}
