//! Test-only real process actors. No Python, host inventory or external services.
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
// Deliberately do not reap here: the native owner under test must clean up
// a descendant after its leader exits, matching the original process actor.
#[allow(clippy::zombie_processes)]
fn descendant(root: &Path, delay: u64, ignore_term: bool) {
    let executable = std::env::current_exe().unwrap();
    let mut child = if ignore_term {
        // POSIX exec preserves an ignored SIGTERM disposition. The native
        // cleanup must therefore still reach SIGKILL, just as with the original actor.
        let mut c = Command::new("/bin/sh");
        c.args(["-c", "trap '' TERM; exec \"$0\" \"$@\""])
            .arg(executable);
        c
    } else {
        Command::new(executable)
    };
    child
        .arg("descendant")
        .arg(root)
        .arg(delay.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Inherit the test leader's group; never detach or address another group.
    let _owned = child.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !root.join("ready").exists() {
        assert!(Instant::now() < deadline, "owned descendant did not start");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let argv0 = PathBuf::from(&args[0]);
    let (mode, root) = if argv0.file_name().unwrap() == "fake-proc-worker" {
        (
            args[1].to_str().unwrap().to_owned(),
            args.get(2).map(PathBuf::from),
        )
    } else {
        (
            argv0.file_name().unwrap().to_str().unwrap().to_owned(),
            argv0.parent().map(Path::to_owned),
        )
    };
    match mode.as_str() {
        "exit7" => std::process::exit(7),
        "timeout" => std::thread::sleep(Duration::from_secs(30)),
        "output-cap" => loop {
            if io::stdout().write_all(&[b'x'; 65536]).is_err() {
                break;
            }
        },
        "group" => {
            descendant(root.as_deref().unwrap(), 400, false);
            println!(
                "C={} TZ={}",
                std::env::var("LC_ALL").unwrap(),
                std::env::var("TZ").unwrap()
            );
        }
        "partial" => {
            print!("p812\nn/own/cwd\n");
            io::stdout().flush().unwrap();
            std::process::exit(1);
        }
        "acp-live" | "acp-exit" => {
            descendant(root.as_deref().unwrap(), 500, true);
            if mode == "acp-exit" {
                std::process::exit(7);
            }
            std::thread::sleep(Duration::from_secs(30));
        }
        "descendant" => {
            let root = root.unwrap();
            std::fs::write(root.join("ready"), "ready").unwrap();
            let delay = args[3].to_str().unwrap().parse().unwrap();
            std::thread::sleep(Duration::from_millis(delay));
            std::fs::write(root.join("survived"), "survived").unwrap();
        }
        _ => panic!("unknown test actor: {mode}"),
    }
}
