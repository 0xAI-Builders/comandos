//! Retarget only owned command strings. Unrelated bytes and comments stay intact.
use comandos_store::files::{FileLock, write_atomic};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::{fs, io, os::unix::fs::PermissionsExt, path::PathBuf};

pub fn config_paths(home: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = [
        ".codex/config.toml",
        ".codex/hooks.json",
        ".gemini/settings.json",
        ".gemini/config/hooks.json",
        ".gemini/antigravity-cli/settings.json",
        ".config/opencode/opencode.json",
        ".config/opencode/opencode.jsonc",
        ".claude/settings.json",
        ".grok/hooks/comandos.json",
    ]
    .into_iter()
    .map(|p| home.join(p))
    .collect::<Vec<_>>();
    match fs::read_dir(home.join(".claude-accounts")) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                    paths.push(entry.path().join("settings.json"));
                }
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    paths.sort();
    Ok(paths)
}

/// Preflight every config before changing any; restore our writes if a later
/// write fails. A user edit made after preflight is never overwritten.
pub fn apply(home: &Path, repo: &Path, dry: bool) -> Result<Vec<String>, String> {
    super::release::check_app_parents(&home.join("placeholder"))?;
    let _lock = if dry {
        None
    } else {
        Some(
            FileLock::exclusive(&home.join(".local/share/comandos/agents.lock"))
                .map_err(|e| e.to_string())?,
        )
    };
    let mut changes = Vec::new();
    for path in config_paths(home)? {
        super::release::check_app_parents(&path)?;
        let metadata = match path.symlink_metadata() {
            Ok(value) if value.is_file() => value,
            Ok(_) => return Err(format!("{} must be a regular config file", path.display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let before = fs::read(&path).map_err(|e| e.to_string())?;
        let text = std::str::from_utf8(&before).map_err(|e| e.to_string())?;
        let after = rewrite(text, repo, home)?.into_bytes();
        if before != after {
            changes.push((path, before, after, metadata.permissions().mode() & 0o777));
        }
    }
    let mut report = Vec::new();
    if dry {
        for (path, _, _, _) in changes {
            report.push(format!("dry-run: retarget {} with backup", path.display()));
        }
        return Ok(report);
    }
    for (path, before, after, _) in &changes {
        if fs::read(path).map_err(|e| e.to_string())? != *before {
            return Err(format!(
                "{} changed during config preflight",
                path.display()
            ));
        }
        for (old, new) in mappings(repo, home)? {
            if std::str::from_utf8(before).is_ok_and(|value| value.contains(&old)) {
                let destination = Path::new(&new);
                if !destination
                    .metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                {
                    return Err(format!(
                        "native command is not installed: {}",
                        destination.display()
                    ));
                }
            }
        }
        // Verify generated JSON where applicable; JSONC retains its comments.
        if path.extension().is_some_and(|ext| ext == "json") {
            serde_json::from_slice::<serde_json::Value>(after)
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        if path.extension().is_some_and(|ext| ext == "toml") {
            std::str::from_utf8(after)
                .map_err(|e| e.to_string())?
                .parse::<toml::Value>()
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    let mut written = 0usize;
    let result = (|| {
        for (path, before, after, mode) in &changes {
            if fs::read(path).map_err(|e| e.to_string())? != *before {
                return Err(format!("{} changed during installation", path.display()));
            }
            let digest = format!("{:x}", Sha256::digest(before));
            let backup = path.with_file_name(format!(
                "{}.pre-comandos-{}",
                path.file_name()
                    .ok_or("config without name")?
                    .to_string_lossy(),
                digest.get(..12).ok_or("invalid digest")?
            ));
            match backup.symlink_metadata() {
                Ok(meta)
                    if meta.is_file()
                        && fs::read(&backup).map_err(|e| e.to_string())? == *before => {}
                Ok(_) => return Err(format!("backup conflict: {}", backup.display())),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    use std::io::Write;
                    use std::os::unix::fs::OpenOptionsExt;
                    let mut file = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(*mode)
                        .open(&backup)
                        .map_err(|e| e.to_string())?;
                    file.write_all(before)
                        .and_then(|()| file.sync_all())
                        .map_err(|e| e.to_string())?;
                }
                Err(e) => return Err(e.to_string()),
            }
            write_atomic(path, after).map_err(|e| e.to_string())?;
            written += 1;
            report.push(format!(
                "retarget {}; backup {}",
                path.display(),
                backup.display()
            ));
        }
        Ok(())
    })();
    if let Err(mut error) = result {
        for (path, before, after, _) in changes.iter().take(written).rev() {
            if fs::read(path).is_ok_and(|current| current == *after) {
                if let Err(e) = write_atomic(path, before) {
                    error.push_str(&format!("; restore {}: {e}", path.display()));
                }
            } else {
                error.push_str(&format!("; {} changed externally", path.display()));
            }
        }
        return Err(error);
    }
    Ok(report)
}

fn mappings(repo: &Path, home: &Path) -> Result<Vec<(String, String)>, String> {
    if !repo.is_absolute() || !home.is_absolute() {
        return Err("absolute repo and HOME required".into());
    }
    let mut pairs = Vec::new();
    for name in crate::dispatch::alias_names().chain(std::iter::once("cc-app")) {
        let native = super::link_path(home, name)
            .to_str()
            .ok_or("HOME must be UTF-8")?
            .to_owned();
        for dir in ["bin", "hooks", "adapters"] {
            let source = repo.join(dir).join(name);
            pairs.push((
                source.to_str().ok_or("repo must be UTF-8")?.to_owned(),
                native.clone(),
            ));
        }
    }
    pairs.sort_by_key(|pair| std::cmp::Reverse(pair.0.len()));
    Ok(pairs)
}

fn command(value: &str, pairs: &[(String, String)], is_command: bool) -> String {
    let mut output = value.to_owned();
    for (old, new) in pairs {
        if output == *old {
            output = if is_command {
                comandos_core::text::shlex_quote(new)
            } else {
                new.clone()
            };
            continue;
        }
        let quoted = comandos_core::text::shlex_quote(new);
        for prior in [format!("'{old}'"), format!("\"{old}\""), old.clone()] {
            for interpreter in [
                "bash ",
                "sh ",
                "python ",
                "python3 ",
                "python3.11 ",
                "python3 -u ",
            ] {
                output = output.replace(&format!("{interpreter}{prior}"), &quoted);
            }
            // Avoid changing a different command with the same name prefix.
            let pattern = format!(r#"{}(?:$|[\s\"';&|)])"#, regex::escape(&prior));
            if let Ok(re) = regex::Regex::new(&pattern) {
                output = re
                    .replace_all(&output, |captures: &regex::Captures<'_>| {
                        let matched = captures.get(0).map_or("", |m| m.as_str());
                        format!("{quoted}{}", matched.get(prior.len()..).unwrap_or_default())
                    })
                    .into_owned();
            }
        }
    }
    output
}

pub fn rewrite(raw: &str, repo: &Path, home: &Path) -> Result<String, String> {
    let pairs = mappings(repo, home)?;
    let mut text = raw.to_owned();
    // argv arrays must lose an obsolete interpreter when their script becomes
    // a native executable. Extra arguments and the rest of the document remain.
    for (old, new) in &pairs {
        for quote in ['"', '\''] {
            let old_token = if quote == '"' {
                serde_json::to_string(old).map_err(|e| e.to_string())?
            } else {
                format!("'{old}'")
            };
            let pattern = format!(
                r#"{q}(?:bash|sh|python|python3(?:\.[0-9]+)?){q}\s*,\s*{}"#,
                regex::escape(&old_token),
                q = quote
            );
            let re = regex::Regex::new(&pattern).map_err(|e| e.to_string())?;
            let encoded = serde_json::to_string(new).map_err(|e| e.to_string())?;
            text = re
                .replace_all(&text, regex::NoExpand(&encoded))
                .into_owned();
        }
    }
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut copied = 0;
    let mut output = String::new();
    let mut last_key = String::new();
    while i < bytes.len() {
        if bytes.get(i..i + 2) == Some(b"//") || bytes.get(i) == Some(&b'#') {
            while bytes.get(i).is_some_and(|b| *b != b'\n') {
                i += 1;
            }
            continue;
        }
        if bytes.get(i..i + 2) == Some(b"/*") {
            i += 2;
            while i < bytes.len() && bytes.get(i..i + 2) != Some(b"*/") {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        let Some(&quote) = bytes.get(i).filter(|b| matches!(b, b'"' | b'\'')) else {
            i += 1;
            continue;
        };
        let start = i;
        i += 1;
        while let Some(&b) = bytes.get(i) {
            if b == quote {
                break;
            }
            if quote == b'"' && b == b'\\' {
                i += 1;
            }
            i += 1;
        }
        if i >= bytes.len() {
            return Err("unterminated configuration string".into());
        }
        i += 1;
        let token = text.get(start..i).ok_or("invalid string boundary")?;
        let decoded = if quote == b'\'' {
            token
                .get(1..token.len() - 1)
                .ok_or("invalid literal string")?
                .to_owned()
        } else {
            serde_json::from_str::<String>(token)
                .map_err(|e| e.to_string())
                .or_else(|_| {
                    format!("v={token}")
                        .parse::<toml::Value>()
                        .map_err(|e| e.to_string())
                        .and_then(|v| {
                            v.get("v")
                                .and_then(toml::Value::as_str)
                                .map(String::from)
                                .ok_or("invalid TOML string".into())
                        })
                })?
        };
        let after = text.get(i..).unwrap_or_default().trim_start();
        if after.starts_with(':') || after.starts_with('=') {
            last_key = decoded;
            continue;
        }
        let changed = command(&decoded, &pairs, last_key == "command");
        let prefix = repo.to_str().ok_or("repo must be UTF-8")?;
        if ["bin", "hooks", "adapters"]
            .iter()
            .any(|dir| changed.contains(&format!("{prefix}/{dir}/")))
        {
            return Err(
                "agent configuration refers to a command without a native replacement".into(),
            );
        }
        if changed != decoded {
            output.push_str(text.get(copied..start).ok_or("invalid copy boundary")?);
            let encoded = if quote == b'\'' && !changed.contains('\'') {
                format!("'{changed}'")
            } else {
                serde_json::to_string(&changed).map_err(|e| e.to_string())?
            };
            output.push_str(&encoded);
            copied = i;
        }
    }
    output.push_str(text.get(copied..).ok_or("invalid copy boundary")?);
    Ok(output)
}
