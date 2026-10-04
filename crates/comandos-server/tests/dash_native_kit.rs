//! Semántica Python, tmux, archivos y consulta de las rutas nativas.
mod support;

use comandos_server::dash::native::{
    files::{self, Strict},
    py::{self, Conversion, NumError},
    query::Query,
    tmux::{Program, Tmux, TmuxError},
};
use serde_json::json;
use std::{ffi::OsString, fs, os::unix::fs::PermissionsExt, time::Duration};
use support::{TestHome, tmux_available};

#[test]
fn python_int_float_and_repr() {
    assert_eq!(py::int(" 42 ").ok(), Some(42));
    assert_eq!(py::int("-1_000").ok(), Some(-1000));
    assert_eq!(py::int("007").ok(), Some(7));
    assert!(matches!(py::int("1__0"), Err(NumError::Invalid)));
    assert!(matches!(py::int("x"), Err(NumError::Invalid)));
    assert!(matches!(py::int("١٢"), Err(NumError::Exotic)));
    assert!(matches!(
        py::int("99999999999999999999999"),
        Err(NumError::Exotic)
    ));
    assert_eq!(
        py::int_error_message("x'y"),
        Some(r#"invalid literal for int() with base 10: "x'y""#.to_string())
    );
    assert_eq!(
        py::int_error_message("a\tb"),
        Some(r"invalid literal for int() with base 10: 'a\tb'".to_string())
    );
    assert_eq!(py::float(" 2.5 ").ok(), Some(2.5));
    assert_eq!(py::float("1_0e1").ok(), Some(100.0));
    assert!(py::float("nan").unwrap().is_nan());
    assert_eq!(py::float("-Infinity").ok(), Some(f64::NEG_INFINITY));
    assert!(matches!(py::float("1_"), Err(NumError::Invalid)));
    assert_eq!(py::clamp_py_float(f64::NAN, 0.0, 25.0), 25.0);
    assert_eq!(py::clamp_py_float(-3.0, 0.0, 25.0), 0.0);
}

#[test]
fn python_str_and_int_of_json_values() {
    assert_eq!(py::str_scalar(&json!("a")).as_deref(), Some("a"));
    assert_eq!(py::str_scalar(&json!(true)).as_deref(), Some("True"));
    assert_eq!(py::str_scalar(&json!(null)).as_deref(), Some("None"));
    assert_eq!(py::str_scalar(&json!(12)).as_deref(), Some("12"));
    assert_eq!(py::str_scalar(&json!(1.5)), None);
    assert_eq!(py::str_scalar(&json!([1])), None);
    assert_eq!(py::int_of(&json!(1.9)), Ok(1));
    assert_eq!(py::int_of(&json!(-1.9)), Ok(-1));
    assert_eq!(py::int_of(&json!("12")), Ok(12));
    assert_eq!(py::int_of(&json!(true)), Ok(1));
    assert_eq!(
        py::int_of(&json!("z")),
        Err(Conversion::Value(
            "invalid literal for int() with base 10: 'z'".into()
        ))
    );
    assert_eq!(py::int_of(&json!([1])), Err(Conversion::Type));
    let nan: serde_json::Value = serde_json::from_str("NaN").unwrap_or(json!(null));
    if !nan.is_null() {
        assert_eq!(
            py::int_of(&nan),
            Err(Conversion::Value(
                "cannot convert float NaN to integer".into()
            ))
        );
    }
}

#[test]
fn python_lines_and_names() {
    assert_eq!(py::splitlines("a\nb\x0bc\r\nd\n"), ["a", "b", "c", "d"]);
    assert_eq!(py::strip("\u{1c} x \u{3000}"), "x");
    assert!(py::is_session("work_1.a-b"));
    assert!(!py::is_session("a\n"));
    assert!(!py::is_session(&"a".repeat(81)));
    assert!(py::is_pane("%12"));
    assert!(!py::is_pane("%12345678"));
    assert_eq!(py::take_chars("ñandú", 3), "ñan");
}

#[test]
fn query_is_urlsplit_plus_parse_qs() {
    let q = Query::parse("/tab-models?session=a+b&session=c&x=")
        .ok()
        .unwrap();
    assert_eq!(q.first("session"), Some("a b"));
    assert_eq!(q.first("x"), None);
    assert!(
        Query::parse("/notices?after=%FF").is_err(),
        "U+FFFD declina"
    );
    let q = Query::parse("/notices?after=1#frag").ok().unwrap();
    assert_eq!(q.first("after"), Some("1"));
}

#[test]
fn atomic_json_write_keeps_mode_and_python_bytes() {
    let home = TestHome::new("kit-files");
    let path = home.hooks().join("prefs.json");
    files::write_json_atomic(&path, &json!({"b": 1, "a": "ñ"})).unwrap();
    // `write_json_file` usa `ensure_ascii=True` por omisión: «ñ» viaja como `\u00f1`.
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        r#"{"b": 1, "a": "\u00f1"}"#
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
        0o600
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    files::write_json_atomic(&path, &json!({})).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
        0o640
    );
    assert_eq!(files::read_json(&path), Some(json!({})));
    fs::write(&path, "{roto").unwrap();
    assert_eq!(files::read_json(&path), None);
    assert!(matches!(files::read_json_strict(&path), Strict::Unreadable));
    assert!(matches!(
        files::read_json_strict(&home.hooks().join("no-existe.json")),
        Strict::Missing
    ));
    // Ningún temporal huérfano.
    let leftovers: Vec<_> = fs::read_dir(home.hooks())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
}

#[tokio::test]
async fn tmux_runs_against_a_private_server() {
    if !tmux_available() {
        eprintln!("tmux no está instalado: se salta");
        return;
    }
    let home = TestHome::new("kit-tmux");
    let tmux = Tmux::private(&home.tmux_dir());
    let made = tmux
        .run(&["new-session", "-d", "-s", "kit", "cat"])
        .await
        .unwrap();
    assert!(made.ok, "{}", made.stderr);
    let listed = tmux
        .run(&["list-sessions", "-F", "#{session_name}"])
        .await
        .unwrap();
    assert_eq!(py::splitlines(&listed.stdout), ["kit"]);
    let missing = tmux.run(&["has-session", "-t", "=otra"]).await.unwrap();
    assert!(!missing.ok);
    tmux.run(&["kill-server"]).await.unwrap();
}

#[tokio::test]
async fn tmux_timeout_missing_and_decode_errors() {
    // `tail -f /dev/null -- …` nunca termina: simula un tmux colgado.
    let hung = Tmux {
        program: Program {
            path: "tail".into(),
            prefix: vec![OsString::from("-f"), "/dev/null".into(), "--".into()],
            env: vec![],
            env_remove: vec![],
        },
        timeout: Duration::from_millis(300),
    };
    let error = hung.run(&["has-session", "-t", "=x"]).await.unwrap_err();
    assert!(matches!(error, TmuxError::Timeout { .. }));
    assert_eq!(
        error.python_message().as_deref(),
        Some("Command '['tmux', 'has-session', '-t', '=x']' timed out after 0.3 seconds")
    );
    let missing = Tmux {
        program: Program::named("/no-existe/tmux"),
        timeout: Duration::from_secs(5),
    };
    let error = missing.run(&["list-sessions"]).await.unwrap_err();
    assert_eq!(
        error.python_message().as_deref(),
        Some("[Errno 2] No such file or directory: 'tmux'")
    );
    // printf con `%.0s` se traga el argumento extra.
    let bytes = Tmux {
        program: Program {
            path: "printf".into(),
            prefix: vec![OsString::from("a\\r\\nb\\rc\\n%.0s")],
            env: vec![],
            env_remove: vec![],
        },
        timeout: Duration::from_secs(5),
    };
    assert_eq!(bytes.run(&["x"]).await.unwrap().stdout, "a\nb\nc\n");
    let invalid = Tmux {
        program: Program {
            path: "printf".into(),
            prefix: vec![OsString::from("\\377%.0s")],
            env: vec![],
            env_remove: vec![],
        },
        timeout: Duration::from_secs(5),
    };
    let error = invalid.run(&["x"]).await.unwrap_err();
    assert!(matches!(error, TmuxError::Decode));
    assert_eq!(error.python_message(), None);
}
