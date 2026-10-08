use comandos_notifyd::{dash::DashClient, entry::parse_options};
use std::{ffi::OsString, path::PathBuf, process::ExitCode};

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn original_notifyd_defaults_are_parsed_without_starting_gtk() {
    let options = parse_options(&[]).expect("default options");
    assert!(!options.headless);
    assert_eq!(options.port, 4778);
    assert_eq!(
        options.dash,
        DashClient::parse("http://127.0.0.1:4777").expect("URL")
    );
    assert_eq!(
        options.hooks,
        PathBuf::from(std::env::var_os("HOME").expect("private HOME")).join(".claude/hooks")
    );
    assert_eq!(options.repo_root, None);
    assert_eq!(options.tmux_socket, None);
}

#[test]
fn explicit_options_and_last_value_preserve_the_existing_entry() {
    let options = parse_options(&args(&[
        "--headless",
        "--port",
        "7312",
        "--port",
        "0",
        "--hooks-dir",
        "/private/hooks",
        "--dash-url",
        "http://127.0.0.1:7311",
        "--repo-root",
        "/private/repo",
        "--tmux-socket",
        "/private/own.sock",
    ]))
    .expect("parse only");
    assert!(options.headless);
    assert_eq!(options.port, 0);
    assert_eq!(options.hooks, PathBuf::from("/private/hooks"));
    assert_eq!(
        options.dash,
        DashClient::parse("http://127.0.0.1:7311").expect("URL")
    );
    assert_eq!(options.repo_root, Some(PathBuf::from("/private/repo")));
    assert_eq!(
        options.tmux_socket,
        Some(PathBuf::from("/private/own.sock"))
    );
}

#[test]
fn original_errors_are_reported_before_gtk_or_a_listener() {
    for (values, error) in [
        (
            vec!["--unknown-fixture"],
            "opción desconocida: --unknown-fixture",
        ),
        (vec!["--help"], "opción desconocida: --help"),
        (vec!["--version"], "opción desconocida: --version"),
        (vec!["--port"], "--port necesita un número"),
        (vec!["--port", "65536"], "--port necesita un número"),
        (vec!["--hooks-dir"], "--hooks-dir necesita una ruta"),
        (
            vec!["--dash-url", "https://127.0.0.1:7311"],
            "--dash-url necesita http://<host>[:<puerto>]",
        ),
        (vec!["--repo-root"], "--repo-root necesita una ruta"),
        (vec!["--tmux-socket"], "--tmux-socket necesita una ruta"),
    ] {
        assert_eq!(
            parse_options(&args(&values)).expect_err("invalid option"),
            error
        );
    }
}

#[test]
fn adapter_uses_the_existing_entry_error_path() {
    let invalid = args(&["--unknown-fixture"]);
    assert_eq!(comandos_app::notifyd::run(&invalid), ExitCode::from(2));
    assert_eq!(comandos_notifyd::entry::run(&invalid), ExitCode::from(2));
}

#[cfg(unix)]
#[test]
fn native_path_arguments_preserve_non_utf8_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let path = OsString::from_vec(b"/private/hooks-\xff".to_vec());
    let options = parse_options(&["--hooks-dir".into(), path.clone()]).expect("native path");
    assert_eq!(options.hooks, PathBuf::from(path));
}
