//! `cc-browser-remote`: pasa el MCP por `ssh -W` al broker de macmini (D2: puerto como dato).
use std::path::Path;

pub struct RemoteTarget {
    pub host: String,
    pub port: u16,
}

pub fn load_target(home: &Path) -> Result<RemoteTarget, String> {
    let path = home.join(".config/comandos/browser.json");
    let raw = match std::fs::read(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RemoteTarget {
                host: "macmini".into(),
                port: 19441,
            });
        }
        Err(e) => return Err(format!("no se pudo leer {}: {e}", path.display())),
    };
    let v: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|e| format!("{} inválido: {e}", path.display()))?;
    let host = v.get("host").and_then(|h| h.as_str()).unwrap_or("macmini");
    let valid = !host.is_empty()
        && !host.starts_with('-')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !valid {
        return Err(format!("host inválido en {}", path.display()));
    }
    let port = v
        .get("port")
        .and_then(|p| p.as_u64())
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p >= 1)
        .ok_or_else(|| format!("puerto inválido en {}", path.display()))?;
    Ok(RemoteTarget {
        host: host.to_string(),
        port,
    })
}

pub fn remote_argv(t: &RemoteTarget) -> Vec<String> {
    [
        "ssh",
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "ConnectTimeout=8",
        "-W",
    ]
    .into_iter()
    .map(String::from)
    .chain([format!("127.0.0.1:{}", t.port), t.host.clone()])
    .collect()
}

pub fn run(home: impl AsRef<Path>) -> i32 {
    use std::os::unix::process::CommandExt;
    let target = match load_target(home.as_ref()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let argv = remote_argv(&target);
    let Some((program, rest)) = argv.split_first() else {
        return 127;
    };
    let err = std::process::Command::new(program).args(rest).exec();
    eprintln!("cc-browser-remote: no se pudo ejecutar ssh: {err}");
    127
}
