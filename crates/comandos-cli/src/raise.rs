//! Desktop shortcuts retain their original window priority and process contract.
use std::{
    env,
    os::unix::process::{CommandExt, ExitStatusExt},
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
};

fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

fn spawn_error_code(error: std::io::Error) -> i32 {
    if error.kind() == std::io::ErrorKind::NotFound {
        127
    } else {
        126
    }
}

fn raise_window(class: &str) -> bool {
    Command::new("wmctrl")
        .args(["-x", "-a", class])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub fn main(args: &[String]) -> i32 {
    let home = env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    match args.first().map(String::as_str) {
        Some("app") => {
            if raise_window("comandos") {
                return 0;
            }
            Command::new("setsid")
                .arg("-f")
                .arg(home.join(".local/bin/comandos-app"))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_or_else(spawn_error_code, exit_code)
        }
        Some("term") => {
            if ["kitty", "comandos", "tilix"].into_iter().any(raise_window) {
                return 0;
            }
            let error = Command::new(home.join(".local/bin/comandos"))
                .arg("next")
                .exec();
            eprintln!("comandos raise: {error}");
            spawn_error_code(error)
        }
        _ => {
            eprintln!("uso: comandos raise app|term");
            2
        }
    }
}
