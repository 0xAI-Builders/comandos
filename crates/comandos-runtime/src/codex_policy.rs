//! Only explicit YOLO changes child launch options; prompts and flag values stay literal.
pub const YOLO: &str = "--dangerously-bypass-approvals-and-sandbox";
const VALUES: &[&str] = &[
    "-c",
    "--config",
    "-m",
    "--model",
    "-p",
    "--profile",
    "-s",
    "--sandbox",
    "-a",
    "--ask-for-approval",
    "-C",
    "--cd",
    "--add-dir",
    "-i",
    "--image",
    "--remote",
    "--remote-auth-token-env",
    "--local-provider",
    "--enable",
    "--disable",
    "-o",
    "--output-last-message",
    "--output-schema",
];
fn chunks(args: &[String]) -> Vec<&[String]> {
    let mut out = vec![];
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--" {
            out.push(&args[i..]);
            break;
        }
        let key = args[i].split('=').next().unwrap_or("");
        let n = if VALUES.contains(&key) && !args[i].contains('=') && i + 1 < args.len() {
            2
        } else {
            1
        };
        out.push(&args[i..i + n]);
        i += n;
    }
    out
}
pub fn explicit(args: &[String]) -> bool {
    chunks(args)
        .iter()
        .any(|p| matches!(p[0].as_str(), "--yolo" | YOLO))
}
pub fn normalize(args: &[String]) -> Result<Vec<String>, String> {
    let parts = chunks(args);
    if !parts
        .iter()
        .any(|p| matches!(p[0].as_str(), "--yolo" | YOLO))
    {
        return Ok(args.to_vec());
    }
    let mut out = vec![];
    for p in parts {
        let arg = p[0].as_str();
        let key = arg.split('=').next().unwrap_or("");
        if arg == "--" {
            out.extend_from_slice(p);
            continue;
        }
        if matches!(key, "--remote" | "--remote-auth-token-env") {
            return Err(
                "YOLO remoto debe configurarse en el equipo del servidor; no se cambiará a local"
                    .into(),
            );
        }
        if matches!(
            arg,
            "--yolo" | YOLO | "--no-daemon" | "--approve-for-me" | "--full-auto"
        ) || matches!(key, "-s" | "--sandbox" | "-a" | "--ask-for-approval")
        {
            continue;
        }
        let config = if matches!(arg, "-c" | "--config") {
            p.get(1).map(String::as_str).unwrap_or("")
        } else {
            arg.strip_prefix("--config=").unwrap_or("")
        };
        let config_key = config.split('=').next().unwrap_or("").trim();
        if matches!(
            config_key,
            "sandbox_mode" | "approval_policy" | "approvals_reviewer" | "permissions"
        ) || config_key.starts_with("permissions.")
        {
            continue;
        }
        out.extend_from_slice(p);
    }
    let mut offset = 0;
    let mut insert = 0;
    for p in chunks(&out) {
        if p[0] == "--" {
            break;
        }
        if !p[0].starts_with('-') {
            if matches!(p[0].as_str(), "resume" | "fork") {
                insert = offset + 1;
            }
            break;
        }
        offset += p.len();
    }
    out.splice(
        insert..insert,
        ["--no-daemon".to_string(), YOLO.to_string()],
    );
    Ok(out)
}
pub fn quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b))
    {
        s.into()
    } else {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    }
}
