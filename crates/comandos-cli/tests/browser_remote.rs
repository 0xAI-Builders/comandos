use comandos_cli::browser::remote::{RemoteTarget, load_target, remote_argv};
use std::path::PathBuf;

fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cli-remote-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn default_target_matches_shell_script() {
    let home = tempdir();
    let t = load_target(&home).ok();
    assert_eq!(
        t.as_ref().map(|t| (t.host.as_str(), t.port)),
        Some(("macmini", 19441))
    );
    let argv = remote_argv(&RemoteTarget {
        host: "macmini".into(),
        port: 19441,
    });
    assert_eq!(
        argv.join(" "),
        "ssh -T -o BatchMode=yes -o ExitOnForwardFailure=yes -o ConnectTimeout=8 -W 127.0.0.1:19441 macmini"
    );
}

#[test]
fn config_file_selects_rust_broker_port() {
    let home = tempdir();
    std::fs::create_dir_all(home.join(".config/comandos")).ok();
    std::fs::write(
        home.join(".config/comandos/browser.json"),
        r#"{"host":"macmini","port":19442}"#,
    )
    .ok();
    assert_eq!(load_target(&home).map(|t| t.port).ok(), Some(19442));
}

#[test]
fn host_starting_with_dash_is_rejected() {
    let home = tempdir();
    std::fs::create_dir_all(home.join(".config/comandos")).ok();
    std::fs::write(
        home.join(".config/comandos/browser.json"),
        r#"{"host":"-oProxyCommand=x","port":1}"#,
    )
    .ok();
    assert!(load_target(&home).is_err());
}

#[test]
fn npx_flags_route_only_the_first_package_to_remote() {
    use comandos_cli::browser::npx_guard::is_browser_package;
    for args in [
        vec!["-y", "--no-install", "chrome-devtools-mcp"],
        vec!["--yes", "-y", "--no-install", "chrome-devtools-mcp@1.0"],
    ] {
        assert!(is_browser_package(
            &args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>()
        ));
    }
    assert!(!is_browser_package(&[
        "-y".into(),
        "other".into(),
        "chrome-devtools-mcp".into()
    ]));
}

#[test]
fn npx_resolver_avoids_guard_recursion_and_has_no_pinned_fallback() {
    use std::{os::unix::fs::PermissionsExt, process::Command};
    let home = tempdir();
    let fake = home.join("bin");
    std::fs::create_dir_all(&fake).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_comandos"), fake.join("node")).unwrap();
    let node = home.join("real/bin/node");
    std::fs::create_dir_all(node.parent().unwrap()).unwrap();
    std::fs::write(&node, b"#!/bin/sh\nprintf 'node:%s\\n' \"$@\"\n").unwrap();
    std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cli = home.join("real/lib/node_modules/npm/bin/npx-cli.js");
    std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
    std::fs::write(&cli, b"fake npx").unwrap();
    let guard_cli = home.join("lib/node_modules/npm/bin/npx-cli.js");
    std::fs::create_dir_all(guard_cli.parent().unwrap()).unwrap();
    std::fs::write(guard_cli, b"guard candidate").unwrap();
    let path = std::env::join_paths([fake.as_path(), node.parent().unwrap()]).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args(["browser", "npx-guard", "other"])
        .env("HOME", &home)
        .env("PATH", path)
        .env_remove("COMANDOS_REAL_NPX")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains(cli.to_str().unwrap()));
    let pinned = home.join(".nvm/versions/node/v22.19.0/bin");
    std::fs::create_dir_all(&pinned).unwrap();
    std::fs::copy(&node, pinned.join("node")).unwrap();
    let pinned_cli = home.join(".nvm/versions/node/v22.19.0/lib/node_modules/npm/bin/npx-cli.js");
    std::fs::create_dir_all(pinned_cli.parent().unwrap()).unwrap();
    std::fs::write(&pinned_cli, b"fake npx").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args(["browser", "npx-guard", "other"])
        .env("HOME", &home)
        .env("PATH", &fake)
        .env_remove("COMANDOS_REAL_NPX")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(127));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "No encuentro el npx real; define COMANDOS_REAL_NPX\n"
    );
    let remote = home.join(".local/bin/cc-browser-remote");
    std::fs::create_dir_all(remote.parent().unwrap()).unwrap();
    std::fs::write(&remote, b"#!/bin/sh\nprintf remote\n").unwrap();
    std::fs::set_permissions(&remote, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args([
            "browser",
            "npx-guard",
            "-y",
            "--no-install",
            "chrome-devtools-mcp",
        ])
        .env("HOME", &home)
        .env("PATH", &fake)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"remote");
    std::fs::remove_dir_all(home).unwrap();
}
