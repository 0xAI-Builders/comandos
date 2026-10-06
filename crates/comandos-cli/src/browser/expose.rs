use crate::browser::remote::load_target;
use nix::fcntl::{Flock, FlockArg};
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug)]
struct ForwardError(String);

pub fn run(args: &[String], home: impl AsRef<Path>) -> i32 {
    if let Some(help) = help_text(args) {
        print!("{help}");
        return 0;
    }
    if let Err((usage, message)) = validate_args(args) {
        eprintln!(
            "usage: {usage}\ncc-browser-expose{}: error: {message}",
            if usage.starts_with("cc-browser-expose start ") {
                " start"
            } else if usage.starts_with("cc-browser-expose stop ") {
                " stop"
            } else {
                ""
            }
        );
        return 2;
    }
    match run_inner(args, home.as_ref()) {
        Ok(()) => 0,
        Err(ForwardError(e)) => {
            eprintln!("{e}");
            1
        }
    }
}

fn run_inner(args: &[String], home: &Path) -> Result<(), ForwardError> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err(ForwardError(
            "usage: cc-browser-expose {start,stop,status}".to_owned(),
        ));
    };
    let root = directory(home, action == "start")?;
    if action == "status" {
        status(&root, home)?;
        return Ok(());
    }
    if action == "stop" && !root.exists() {
        let remote = parse_port(args.get(1), 1024)?;
        println!("No owned forward on remote port {remote}");
        return Ok(());
    }
    let lock = open_private(&root.join(".lock"), true, 0o600)?;
    let metadata = lock
        .metadata()
        .map_err(|_| ForwardError("Cannot access private SSH forward state".into()))?;
    if !metadata.is_file()
        || metadata.uid() != current_uid()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(ForwardError(
            "Cannot access private SSH forward state".into(),
        ));
    }
    let _lock = Flock::lock(lock, FlockArg::LockExclusive)
        .map_err(|_| ForwardError("Cannot access private SSH forward state".to_owned()))?;
    match action {
        "start" => {
            let local = parse_port(args.get(1), 1)?;
            let remote = match args.get(2) {
                Some(_) => parse_port(args.get(2), 1024)?,
                None => local,
            };
            if remote < 1024 {
                return Err(ForwardError(
                    "A local privileged port requires an explicit remote port from 1024 to 65535"
                        .to_owned(),
                ));
            }
            start(&root, local, remote, home)
        }
        "stop" => stop(&root, parse_port(args.get(1), 1024)?, home),
        _ => Err(ForwardError(
            "usage: cc-browser-expose {start,stop,status}".to_owned(),
        )),
    }
}

fn parse_port(value: Option<&String>, minimum: u16) -> Result<u16, ForwardError> {
    let Some(value) = value else {
        return Err(ForwardError(format!(
            "Port must be a decimal integer from {minimum} to 65535"
        )));
    };
    if !value.is_ascii() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ForwardError(format!(
            "Port must be a decimal integer from {minimum} to 65535"
        )));
    }
    let port = value.parse::<u32>().map_err(|_| {
        ForwardError(format!(
            "Port must be a decimal integer from {minimum} to 65535"
        ))
    })?;
    if port < u32::from(minimum) || port > 65535 {
        return Err(ForwardError(format!(
            "Port must be a decimal integer from {minimum} to 65535"
        )));
    }
    Ok(port as u16)
}

fn directory(home: &Path, create: bool) -> Result<PathBuf, ForwardError> {
    let root = if let Some(path) = std::env::var_os("CC_BROWSER_FORWARD_DIR") {
        PathBuf::from(path)
    } else if let Some(path) = std::env::var_os("XDG_RUNTIME_DIR") {
        PathBuf::from(path).join("cc-browser-forwards")
    } else {
        home.join(".local/state/cc-browser-forwards")
    };
    if !root.is_absolute() {
        return Err(ForwardError(
            "Forward state directory must be an absolute path".to_owned(),
        ));
    }
    if create && fs::symlink_metadata(&root).is_err() {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)
            .map_err(|_| ForwardError("Cannot access private SSH forward state".to_owned()))?;
    }
    let Ok(meta) = fs::symlink_metadata(&root) else {
        return Ok(root);
    };
    if !meta.is_dir() || meta.uid() != current_uid() || meta.permissions().mode() & 0o077 != 0 {
        return Err(ForwardError(
            "Forward state directory must be owned by you, private, and not a symlink".to_owned(),
        ));
    }
    Ok(root)
}

fn socket_path(root: &Path, remote: u16) -> Result<PathBuf, ForwardError> {
    let path = root.join(format!("{remote}.sock"));
    if path.as_os_str().as_encoded_bytes().len() >= 104 {
        return Err(ForwardError(
            "SSH control path is too long; set CC_BROWSER_FORWARD_DIR to a shorter private directory"
                .to_owned(),
        ));
    }
    if path.is_symlink() {
        return Err(ForwardError(
            "Owned control socket must not be a symlink".to_owned(),
        ));
    }
    Ok(path)
}

fn read_mapping(
    root: &Path,
    remote: u16,
    host: &str,
) -> Result<Option<serde_json::Value>, ForwardError> {
    let path = root.join(format!("{remote}.json"));
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(&path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(ForwardError(
                "Cannot access private SSH forward state".to_owned(),
            ));
        }
    };
    let meta = file
        .metadata()
        .map_err(|_| ForwardError("Cannot access private SSH forward state".to_owned()))?;
    if !meta.is_file() || meta.uid() != current_uid() || meta.permissions().mode() & 0o077 != 0 {
        return Err(ForwardError(
            "Forward mapping must be a private regular file owned by you".to_owned(),
        ));
    }
    let mut raw = String::new();
    file.read_to_string(&mut raw)
        .map_err(|_| ForwardError("Invalid owned forward mapping".to_owned()))?;
    let v: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|_| ForwardError("Invalid owned forward mapping".to_owned()))?;
    let local = v.get("local_port").and_then(|v| v.as_u64()).unwrap_or(0);
    let valid = v == json!({"version":1,"host":host,"local_port":local,"remote_port":remote})
        && (1..=65535).contains(&local);
    if !valid {
        return Err(ForwardError("Invalid owned forward mapping".to_owned()));
    }
    Ok(Some(v))
}

fn ssh(args: &[String]) -> bool {
    let Ok(mut child) = Command::new("ssh")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn control(socket: &Path, operation: &str, host: &str) -> bool {
    ssh(&[
        "-S".into(),
        socket.display().to_string(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ConnectTimeout=8".into(),
        "-O".into(),
        operation.into(),
        host.into(),
    ])
}

fn start(root: &Path, local: u16, remote: u16, home: &Path) -> Result<(), ForwardError> {
    let target = load_target(home).map_err(ForwardError)?;
    let host = target.host;
    let socket = socket_path(root, remote)?;
    let mapping = read_mapping(root, remote, &host)?;
    if let Some(mapping) = mapping {
        if mapping["local_port"].as_u64() != Some(u64::from(local)) {
            return Err(ForwardError(
                "Remote port already belongs to a different local port; stop that mapping first"
                    .to_owned(),
            ));
        }
        if control(&socket, "check", &host) {
            println!("Already running: Mac http://127.0.0.1:{remote} -> local 127.0.0.1:{local}");
            return Ok(());
        }
        if socket.exists() {
            return Err(ForwardError(
                "Owned SSH master is not responding; inspect or stop it before restarting"
                    .to_owned(),
            ));
        }
    } else if socket.exists() {
        return Err(ForwardError(
            "Control socket exists without an owned mapping; refusing to use it".to_owned(),
        ));
    }
    let path = root.join(format!("{remote}.json"));
    if !path.exists() {
        let mut file = open_private(&path, false, 0o600)?;
        let text = format!(
            "{{\"version\": 1, \"host\": {}, \"local_port\": {local}, \"remote_port\": {remote}}}\n",
            serde_json::to_string(&host).unwrap()
        );
        file.write_all(text.as_bytes())
            .map_err(|_| ForwardError("Cannot access private SSH forward state".to_owned()))?;
        file.sync_all()
            .map_err(|_| ForwardError("Cannot access private SSH forward state".to_owned()))?;
    }
    let args = vec![
        "-M".into(),
        "-S".into(),
        socket.display().to_string(),
        "-fNT".into(),
        "-o".into(),
        "ControlPersist=no".into(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ExitOnForwardFailure=yes".into(),
        "-o".into(),
        "ConnectTimeout=8".into(),
        "-R".into(),
        format!("127.0.0.1:{remote}:127.0.0.1:{local}"),
        host.clone(),
    ];
    if ssh(&args) && control(&socket, "check", &host) {
        println!("Started: Mac http://127.0.0.1:{remote} -> local 127.0.0.1:{local}");
        return Ok(());
    }
    if !socket.exists() || control(&socket, "exit", &host) {
        let _ = fs::remove_file(path);
    }
    Err(ForwardError(
        "SSH forward did not pass its master check; no ready tunnel confirmed".to_owned(),
    ))
}

fn stop(root: &Path, remote: u16, home: &Path) -> Result<(), ForwardError> {
    let target = load_target(home).map_err(ForwardError)?;
    let host = target.host;
    let socket = socket_path(root, remote)?;
    let mapping = read_mapping(root, remote, &host)?;
    if mapping.is_none() {
        println!("No owned forward on remote port {remote}");
        return Ok(());
    }
    if socket.exists() && !control(&socket, "exit", &host) {
        return Err(ForwardError(
            "Owned SSH master did not stop; mapping retained for inspection".to_owned(),
        ));
    }
    fs::remove_file(root.join(format!("{remote}.json")))
        .map_err(|_| ForwardError("Cannot access private SSH forward state".to_owned()))?;
    println!("Stopped owned forward on remote port {remote}");
    Ok(())
}

fn status(root: &Path, home: &Path) -> Result<(), ForwardError> {
    let target = load_target(home).map_err(ForwardError)?;
    let host = target.host;
    let entries = if root.exists() {
        fs::read_dir(root)
            .map_err(|_| ForwardError("Cannot access private SSH forward state".to_owned()))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if entries.is_empty() {
        println!("No forwards configured");
        return Ok(());
    }
    let mut entries = entries;
    entries.sort();
    for entry in entries {
        let Some(stem) = entry.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(remote) = stem.parse::<u16>() else {
            continue;
        };
        if remote < 1024 {
            continue;
        }
        let Some(mapping) = read_mapping(root, remote, &host)? else {
            continue;
        };
        let state = if control(&socket_path(root, remote)?, "check", &host) {
            "running"
        } else {
            "not running"
        };
        println!(
            "{state}: Mac 127.0.0.1:{remote} -> local 127.0.0.1:{}",
            mapping["local_port"].as_u64().unwrap_or(0)
        );
    }
    Ok(())
}

fn open_private(path: &Path, truncate: bool, mode: u32) -> Result<File, ForwardError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .create_new(!truncate)
        .truncate(false)
        .mode(mode)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| ForwardError("Cannot access private SSH forward state".to_owned()))
}

fn current_uid() -> u32 {
    nix::unistd::Uid::current().as_raw()
}

fn help_text(args: &[String]) -> Option<&'static str> {
    let action = match args.first()?.as_str() {
        "-h" | "--help" => None,
        action @ ("start" | "stop" | "status")
            if args[1..]
                .iter()
                .any(|arg| matches!(arg.as_str(), "-h" | "--help")) =>
        {
            Some(action)
        }
        _ => return None,
    };
    Some(match action {
        None => {
            "usage: cc-browser-expose [-h] {start,stop,status} ...\n\nExpose a local development port to the Mac browser over an owned SSH tunnel.\n\npositional arguments:\n  {start,stop,status}\n\noptions:\n  -h, --help           show this help message and exit\n"
        }
        Some("start") => {
            "usage: cc-browser-expose start [-h] local_port [remote_port]\n\npositional arguments:\n  local_port\n  remote_port\n\noptions:\n  -h, --help   show this help message and exit\n"
        }
        Some("stop") => {
            "usage: cc-browser-expose stop [-h] remote_port\n\npositional arguments:\n  remote_port\n\noptions:\n  -h, --help   show this help message and exit\n"
        }
        Some("status") => {
            "usage: cc-browser-expose status [-h]\n\noptions:\n  -h, --help  show this help message and exit\n"
        }
        _ => unreachable!(),
    })
}

fn validate_args(args: &[String]) -> Result<(), (String, String)> {
    let main = "cc-browser-expose [-h] {start,stop,status} ...";
    let (usage, fields): (&str, &[(&str, u16)]) = match args.first().map(String::as_str) {
        Some("start") => (
            "cc-browser-expose start [-h] local_port [remote_port]",
            &[("local_port", 1), ("remote_port", 1024)],
        ),
        Some("stop") => (
            "cc-browser-expose stop [-h] remote_port",
            &[("remote_port", 1024)],
        ),
        Some("status") => (main, &[]),
        None => {
            return Err((
                main.into(),
                "the following arguments are required: action".into(),
            ));
        }
        Some(action) => {
            return Err((
                main.into(),
                format!(
                    "argument action: invalid choice: '{action}' (choose from start, stop, status)"
                ),
            ));
        }
    };
    if args.len() > fields.len() + 1 {
        return Err((
            main.into(),
            format!(
                "unrecognized arguments: {}",
                args[fields.len() + 1..].join(" ")
            ),
        ));
    }
    for (index, (field, minimum)) in fields.iter().enumerate() {
        if index == 1 && args.len() <= index + 1 {
            continue;
        }
        if args.get(index + 1).is_none() {
            return Err((
                usage.into(),
                format!("the following arguments are required: {field}"),
            ));
        }
        if let Err(ForwardError(error)) = parse_port(args.get(index + 1), *minimum) {
            return Err((usage.into(), format!("argument {field}: {error}")));
        }
    }
    if args.first().is_some_and(|arg| arg == "start")
        && args.len() == 2
        && parse_port(args.get(1), 1).unwrap() < 1024
    {
        return Err((
            main.into(),
            "A local privileged port requires an explicit remote port from 1024 to 65535".into(),
        ));
    }
    Ok(())
}
