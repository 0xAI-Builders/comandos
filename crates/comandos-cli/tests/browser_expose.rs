use std::{path::PathBuf, process::Command};

fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cli-expose-{}-{}",
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
fn expose_start_status_stop_uses_owned_ssh_master() {
    let home = tempdir();
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = home.join("ssh.log");
    let fake = bin.join("ssh");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nsocket=''\nprev=''\nfor arg in \"$@\"; do if [ \"$prev\" = '-S' ]; then socket=\"$arg\"; fi; prev=\"$arg\"; done\ncase \"$*\" in *' -O check '*) test -e \"$socket\";; *' -O exit '*) rm -f \"$socket\"; exit 0;; *) touch \"$socket\";; esac\n",
            log.display()
        ),
    )
    .unwrap();
    std::os::unix::fs::PermissionsExt::set_mode(&mut fake.metadata().unwrap().permissions(), 0o755);
    Command::new("chmod")
        .arg("755")
        .arg(&fake)
        .status()
        .unwrap();
    let state = home.join("state");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let out = Command::new(env!("CARGO_BIN_EXE_comandos"))
        .args(["browser", "expose", "start", "3000", "13000"])
        .env("HOME", &home)
        .env("PATH", &path)
        .env("CC_BROWSER_FORWARD_DIR", &state)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("Started: Mac http://127.0.0.1:13000"));
    let mapping = std::fs::read_to_string(state.join("13000.json")).unwrap();
    assert_eq!(
        mapping,
        r#"{"version":1,"host":"macmini","local_port":3000,"remote_port":13000}"#.to_owned() + "\n"
    );
}
