#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
mod support;
use comandos_app::config::RunMode;
use comandos_app::tmux::{MUTATE_VERBS, READ_VERBS, TmuxError, check_read_args};
use support::tmux::TestTmux;

#[test]
fn sandbox_socket_never_user_default() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    let uid = nix::unistd::getuid().as_raw();
    let user_default = std::path::PathBuf::from(format!("/tmp/tmux-{uid}/default"));
    assert_ne!(f.ctl.socket_path(), user_default);
    assert!(
        f.ctl
            .socket_path()
            .starts_with(f.config.sandbox_root().unwrap())
    );
    assert!(f.ctl.socket_path().ends_with("tmux/t"));
}

#[test]
fn every_call_carries_explicit_socket() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    f.tmux.new_session("s1", 100, 30);
    // Si TmuxCtl no pasara -S, este has-session iría al servidor por omisión y fallaría.
    assert!(f.ctl.read(&["has-session", "-t", "=s1"]).unwrap().ok());
    let argv = f.ctl.attach_argv("s1");
    assert_eq!(argv[0], "/bin/sh");
    assert!(
        argv[2].contains(&format!("-S '{}'", f.ctl.socket_path().display())),
        "{argv:?}"
    );
}

#[test]
fn mutate_refuses_kill_verbs() {
    let Some(f) = TestTmux::for_mode(RunMode::Live) else {
        return;
    };
    f.tmux.new_session("victim", 80, 24);
    for args in [
        ["kill-server"].as_slice(),
        &["kill-session", "-t", "=victim"],
    ] {
        assert!(
            matches!(f.ctl.mutate(args), Err(TmuxError::Forbidden(_))),
            "{args:?}"
        );
        assert!(
            matches!(f.ctl.read(args), Err(TmuxError::Forbidden(_))),
            "{args:?}"
        );
    }
    assert!(
        f.ctl.read(&["has-session", "-t", "=victim"]).unwrap().ok(),
        "la sesión sigue viva"
    );
    assert!(!MUTATE_VERBS.contains(&"kill-session") && !READ_VERBS.contains(&"kill-server"));
}

#[test]
fn abbreviated_verbs_are_refused() {
    let Some(f) = TestTmux::for_mode(RunMode::Live) else {
        return;
    };
    for verb in [
        "kill-ser", "kill-ses", "killw", "ls", "show", "lsp", "send", "display",
    ] {
        assert!(
            matches!(f.ctl.mutate(&[verb]), Err(TmuxError::Forbidden(_))),
            "{verb}"
        );
        assert!(
            matches!(f.ctl.read(&[verb]), Err(TmuxError::Forbidden(_))),
            "{verb}"
        );
    }
}

#[test]
fn read_args_must_not_mutate() {
    assert!(
        check_read_args(&["display-message", "-t", "=s:", "#{pane_id}"]).is_err(),
        "sin -p"
    );
    assert!(check_read_args(&["display-message", "-p", "-t", "=s:", "#{pane_id}"]).is_ok());
    assert!(
        check_read_args(&["capture-pane", "-t", "%3"]).is_err(),
        "sin -p crea un buffer"
    );
    assert!(check_read_args(&["capture-pane", "-p", "-t", "%3"]).is_ok());
    assert!(check_read_args(&["save-buffer", "/tmp/x"]).is_err());
    assert!(check_read_args(&["save-buffer", "-"]).is_ok());
    assert!(check_read_args(&["display-message", "-p", "#(rm -rf ~)"]).is_err());
    assert!(
        check_read_args(&[
            "list-panes",
            "-s",
            "-t",
            "=sess",
            "-F",
            "#{pane_current_command}"
        ])
        .is_ok()
    );
    assert!(check_read_args(&["list-panes", "-t", "=s; kill-server", "-F", "x"]).is_err());
    for target in [
        "=s",
        "=s:",
        "=s:claude",
        "=s:^",
        "=s:2",
        "%12",
        "@3",
        "$4",
        "term-12-3",
    ] {
        assert!(
            check_read_args(&["display-message", "-p", "-t", target, "x"]).is_ok(),
            "{target}"
        );
    }
}

#[test]
fn shadow_refuses_every_mutating_verb() {
    let Some(f) = TestTmux::for_mode(RunMode::Shadow) else {
        return;
    };
    f.tmux.new_session("s1", 80, 24);
    for verb in MUTATE_VERBS {
        assert!(
            matches!(
                f.ctl.mutate(&[verb, "-t", "=s1"]),
                Err(TmuxError::ShadowRefused(_))
            ),
            "{verb}"
        );
    }
    assert!(matches!(
        f.ctl
            .mutate_with_stdin(&["load-buffer", "-b", "x", "-"], b"hola"),
        Err(TmuxError::ShadowRefused(_))
    ));
    assert!(
        f.ctl
            .read(&["list-sessions", "-F", "#{session_name}"])
            .unwrap()
            .stdout
            .contains("s1")
    );
}

#[test]
fn shadow_attach_keeps_session_size() {
    let Some(f) = TestTmux::for_mode(RunMode::Shadow) else {
        return;
    };
    f.tmux.new_session("big", 163, 44);
    assert!(
        f.ctl.attach_argv("big").is_empty(),
        "Shadow never constructs an attach command"
    );
    let before = f.tmux.raw(&["list-clients"]).stdout;
    for _ in 0..3 {
        let frame = comandos_app::term::shadow::capture_when(&f.ctl, "big", &|| true).unwrap();
        assert_eq!((frame.cols, frame.rows), (163, 44));
        assert_eq!(f.tmux.session_size("big"), (163, 44));
        assert_eq!(f.tmux.raw(&["list-clients"]).stdout, before);
    }
}

#[test]
fn owned_kill_needs_proof() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    f.tmux.new_session("term-9-1", 80, 24);
    f.tmux
        .raw(&["send-keys", "-t", "=term-9-1:", "sleep 300", "Enter"]);
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(
        f.ctl.idle_scratch("term-9-1").unwrap().is_none(),
        "corre algo: no se mata"
    );
    f.tmux.new_session("term-9-2", 80, 24);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let owned = f
        .ctl
        .idle_scratch("term-9-2")
        .unwrap()
        .expect("shell ocioso");
    assert!(f.ctl.kill_owned_session(owned).unwrap().ok());
    assert!(
        !f.ctl
            .read(&["has-session", "-t", "=term-9-2"])
            .unwrap()
            .ok()
    );
    assert!(
        f.ctl.idle_scratch("proyecto").unwrap().is_none(),
        "solo term-*"
    );
}

#[test]
fn placeholder_can_be_removed_by_its_creator() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    let (out, owned) = f
        .ctl
        .new_placeholder_session(&[
            "new-session",
            "-d",
            "-P",
            "-F",
            "#{pane_id}",
            "-s",
            "ph",
            "sleep",
            "2147483647",
        ])
        .unwrap();
    assert!(out.ok());
    assert!(
        f.ctl
            .kill_owned_session(owned.expect("token"))
            .unwrap()
            .ok()
    );
}

#[test]
fn large_output_never_blocks() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    let big = "x".repeat(1 << 20); // 1 MiB por stdin y de vuelta por stdout
    assert!(
        f.ctl
            .mutate_with_stdin(&["load-buffer", "-b", "big", "-"], big.as_bytes())
            .unwrap()
            .ok()
    );
    let started = std::time::Instant::now();
    let out = f.ctl.read(&["show-buffer", "-b", "big"]).unwrap();
    assert_eq!(out.stdout.len(), 1 << 20);
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
}

#[test]
fn window_size_ignores_tiny_sessions() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    f.tmux.new_session("ok", 120, 40);
    f.tmux.new_session("tiny", 10, 3);
    assert_eq!(f.ctl.window_size("ok"), Some((120, 40)));
    assert_eq!(
        f.ctl.window_size("tiny"),
        None,
        "cols >= 20 y rows >= 5 (_tmux_window_size)"
    );
    assert_eq!(f.ctl.window_size("nope"), None);
}

#[test]
fn tmux_command_chains_are_refused_before_execution() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    for args in [
        vec!["list-sessions", ";", "kill-server"],
        vec!["capture-pane", "-p", ";kill-server"],
        vec!["display-message", "-p", "-I"],
    ] {
        assert!(f.ctl.read(&args).is_err());
    }
    assert!(
        f.ctl
            .mutate(&["new-session", "-d", ";", "kill-server"])
            .is_err()
    );
    assert!(f.ctl.read(&["has-session", "-t", "=__keep"]).unwrap().ok());
}

#[test]
fn owned_token_cannot_kill_a_different_server() {
    let first = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    let second = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    let (_, token) = first
        .ctl
        .new_placeholder_session(&["new-session", "-d", "-s", "ph", "sleep", "30"])
        .unwrap();
    second.tmux.new_session("ph", 80, 24);
    assert!(second.ctl.kill_owned_session(token.unwrap()).is_err());
    assert!(second.ctl.read(&["has-session", "-t", "=ph"]).unwrap().ok());
}

#[test]
fn reads_preserve_sessions_buffers_and_clients() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.tmux.new_session("s", 80, 24);
    f.ctl
        .mutate_with_stdin(&["load-buffer", "-b", "fixture", "-"], b"hello")
        .unwrap();
    let snapshot =
        || ["list-sessions", "list-buffers", "list-clients"].map(|verb| f.tmux.raw(&[verb]).stdout);
    let before = snapshot();
    let cases = [
        vec!["has-session", "-t", "=s"],
        vec!["list-sessions"],
        vec!["list-windows", "-t", "=s"],
        vec!["list-panes", "-s", "-t", "=s"],
        vec!["list-clients"],
        vec!["list-buffers"],
        vec!["display-message", "-p", "-t", "=s:", "#{pane_id}"],
        vec!["show-options", "-t", "s"],
        vec!["show-buffer", "-b", "fixture"],
        vec!["save-buffer", "-b", "fixture", "-"],
        vec!["capture-pane", "-p", "-t", "=s:"],
        vec!["show-environment", "-t", "=s"],
    ];
    for args in cases {
        assert!(f.ctl.read(&args).unwrap().ok(), "{args:?}");
        assert_eq!(snapshot(), before, "{args:?}");
    }
}

#[test]
fn stale_token_does_not_kill_a_recreated_session() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    let (_, token) = f
        .ctl
        .new_placeholder_session(&["new-session", "-d", "-s", "ph", "sleep", "30"])
        .unwrap();
    f.tmux.raw(&["kill-session", "-t", "=ph"]);
    f.tmux.new_session("ph", 80, 24);
    assert!(f.ctl.kill_owned_session(token.unwrap()).is_err());
    assert!(f.ctl.read(&["has-session", "-t", "=ph"]).unwrap().ok());
}

#[test]
fn placeholder_cannot_claim_an_existing_session() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.tmux.new_session("existing", 80, 24);
    for args in [
        vec!["new-session", "-Ad", "-s", "existing"],
        vec!["new-session", "-A", "-d", "-s", "existing"],
        vec!["new-session", "-d", "-s", "existing", "-s", "other"],
    ] {
        assert!(f.ctl.new_placeholder_session(&args).is_err());
    }
    assert!(
        f.ctl
            .read(&["has-session", "-t", "=existing"])
            .unwrap()
            .ok()
    );
}

#[test]
fn cold_sandbox_child() {
    let Some(marker) = std::env::var_os("COMANDOS_COLD_MARKER") else {
        return;
    };
    let f = TestTmux::cold_for_mode(RunMode::Sandbox).unwrap();
    assert!(
        f.ctl
            .mutate(&["new-session", "-d", "-s", "cold"])
            .unwrap()
            .ok()
    );
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert!(
        !std::path::Path::new(&marker).exists(),
        "se cargó la configuración personal sintética"
    );
    let home = f.ctl.read(&["show-environment", "-g", "HOME"]).unwrap();
    assert_eq!(
        home.stdout.trim(),
        format!(
            "HOME={}",
            f.config.sandbox_root().unwrap().join("home").display()
        )
    );
    assert!(
        !f.ctl
            .read(&["show-environment", "-g", "PERSONAL_SECRET"])
            .unwrap()
            .ok()
    );
    let shell = f
        .ctl
        .read(&[
            "display-message",
            "-p",
            "-t",
            "=cold:",
            "#{pane_current_command}",
        ])
        .unwrap();
    assert_eq!(shell.stdout.trim(), "sh");
}

#[test]
fn first_sandbox_server_does_not_load_personal_config_or_environment() {
    let outer = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    let marker = outer.config.home().join("config-was-loaded");
    std::fs::write(
        outer.config.home().join(".tmux.conf"),
        format!("run-shell 'touch {}'\n", marker.display()),
    )
    .unwrap();
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cold_sandbox_child", "--nocapture"])
        .env_clear()
        .env("HOME", outer.config.home())
        .env("SHELL", "/bin/sh")
        .env("PATH", "/usr/bin:/bin")
        .env("TERM", "xterm-256color")
        .env("PERSONAL_SECRET", "fake-value-never-inherited")
        .env("COMANDOS_COLD_MARKER", &marker)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn placeholder_identity_is_from_creation_even_if_replaced_by_hook() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    let hook = "set-hook -gu after-new-session; kill-session -t =ph; new-session -d -s ph sleep 30";
    let configured = f.tmux.raw(&["set-hook", "-g", "after-new-session", hook]);
    assert!(configured.status.success());
    let (out, token) = f
        .ctl
        .new_placeholder_session(&[
            "new-session",
            "-d",
            "-P",
            "-F",
            "#{pane_id}",
            "-s",
            "ph",
            "sleep",
            "30",
        ])
        .unwrap();
    assert!(out.ok());
    assert!(
        out.stdout.trim().starts_with('%'),
        "se conserva el formato del llamador"
    );
    assert!(
        f.ctl.kill_owned_session(token.unwrap()).is_err(),
        "el token no puede reclamar la sustituta"
    );
    assert!(f.ctl.read(&["has-session", "-t", "=ph"]).unwrap().ok());
}

#[test]
fn placeholder_keeps_the_callers_print_contract() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    for (name, print, format, expected) in [
        ("silent", false, None, ""),
        ("default", true, None, "default:\n"),
        (
            "custom",
            true,
            Some("#{session_name}|#{pane_id}"),
            "custom|",
        ),
    ] {
        let mut args = vec!["new-session", "-d", "-s", name];
        if print {
            args.push("-P");
        }
        if let Some(format) = format {
            args.extend(["-F", format]);
        }
        args.extend(["sleep", "30"]);
        let (out, owned) = f.ctl.new_placeholder_session(&args).unwrap();
        if format.is_some() {
            assert!(out.stdout.starts_with(expected));
        } else {
            assert_eq!(out.stdout, expected);
        }
        assert!(f.ctl.kill_owned_session(owned.unwrap()).unwrap().ok());
    }
    let argv = f.ctl.attach_argv("__keep");
    assert!(argv[2].contains("env -i HOME="));
    assert!(argv[2].contains("-f /dev/null attach"));
}
