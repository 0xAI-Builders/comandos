//! Oversized launch lines use an owned private tmux buffer, never a giant argv.
#[path = "support/ops_lab.rs"]
mod ops_lab;
#[path = "support/private_tmux.rs"]
mod private_tmux;
use comandos_runtime::{pane_exit, pane_typing::TmuxResult};
use ops_lab::OpsLab;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[test]
fn long_unicode_command_reaches_private_shell_exactly_and_cleans_transport() {
    let Some(lab) = OpsLab::start("long-line") else {
        return;
    };
    let mut env = lab.env();
    pane_exit::send_shell_line(&env, &lab.other, "exec /bin/bash --noprofile --norc").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while lab.show(&lab.other, "#{pane_current_command}") != "bash" {
        assert!(Instant::now() < deadline, "private bash did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    let payload = "á🦀λ'\\$\" ".repeat(2500);
    let output = lab.home.join("exact-output");
    let quote = comandos_core::text::shlex_quote;
    let command = format!(
        "printf %s {} > {}",
        quote(&payload),
        quote(output.to_str().unwrap())
    );
    assert!(command.len() > 24 * 1024);
    let original = env.tmux.clone();
    let transport = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
    let seen = transport.clone();
    let expected = command.clone();
    env.tmux = Arc::new(move |args| {
        assert!(
            args.iter().all(|arg| arg.len() < 4096),
            "tmux still receives an oversized argument"
        );
        if args.first() == Some(&"load-buffer") {
            let path = PathBuf::from(args.last().unwrap());
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), expected);
            seen.lock().unwrap().push(path);
        }
        if args.first() == Some(&"paste-buffer") {
            assert!(args.contains(&"-r"));
            assert!(
                !args.contains(&"-p"),
                "shell must not receive bracketed paste codes"
            );
        }
        original(args)
    });
    pane_exit::send_shell_line(&env, &lab.other, &command).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if fs::read(&output).ok().as_deref() == Some(payload.as_bytes()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "private shell did not receive exact Unicode bytes"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let paths = transport.lock().unwrap();
    assert_eq!(paths.len(), 1);
    assert!(!paths[0].exists(), "temporary command file leaked");
    assert!(
        lab.tmux
            .run(&["list-buffers", "-F", "#{buffer_name}"])
            .trim()
            .is_empty(),
        "private tmux buffer leaked"
    );
}

fn rejected_transport(stage: &'static str) {
    let Some(lab) = OpsLab::start(stage) else {
        return;
    };
    let mut env = lab.env();
    let original = env.tmux.clone();
    let calls = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let captured = calls.clone();
    let files = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
    let captured_files = files.clone();
    env.tmux = Arc::new(move |args| {
        captured
            .lock()
            .unwrap()
            .push(args.iter().map(|s| s.to_string()).collect());
        if args.first() == Some(&"load-buffer") {
            captured_files
                .lock()
                .unwrap()
                .push(PathBuf::from(args.last().unwrap()));
        }
        if args.first() == Some(&stage) {
            return Ok(TmuxResult {
                returncode: 1,
                stdout: String::new(),
                stderr: "private injected transport failure".into(),
            });
        }
        original(args)
    });
    let result = pane_exit::send_shell_line(&env, &lab.other, &"x".repeat(25 * 1024));
    assert!(result.is_err());
    let calls = calls.lock().unwrap();
    assert!(
        calls
            .iter()
            .any(|c| c.first().map(String::as_str) == Some(stage)),
        "transport stage was never reached"
    );
    assert!(
        !calls
            .iter()
            .any(|c| c.first().map(String::as_str) == Some("send-keys")
                && c.iter().any(|a| a == "Enter")),
        "failed transport executed the shell line"
    );
    assert!(
        calls
            .iter()
            .any(|c| c.first().map(String::as_str) == Some("delete-buffer")),
        "failed transport did not clean its buffer"
    );
    assert!(
        files.lock().unwrap().iter().all(|p| !p.exists()),
        "temporary command file leaked"
    );
    assert!(
        lab.tmux
            .run(&["list-buffers", "-F", "#{buffer_name}"])
            .trim()
            .is_empty()
    );
}
#[test]
fn load_failure_cleans_up_without_enter() {
    rejected_transport("load-buffer");
}
#[test]
fn paste_failure_cleans_up_without_enter() {
    rejected_transport("paste-buffer");
}
