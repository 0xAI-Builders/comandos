use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

const TARGETS: &[&str] = &[
    "chrome-bg",
    "chrome-current",
    "chrome-devtools",
    "chrome-devtools-current",
];
const PLUGIN: &str = "chrome-devtools-mcp@chrome-devtools-plugins";

#[derive(Debug)]
struct MigrationError(String);

pub fn run(args: &[String], home: impl AsRef<Path>) -> i32 {
    match run_inner(args, home.as_ref()) {
        Ok(()) => 0,
        Err(MigrationError(e)) => {
            eprintln!("{e}");
            1
        }
    }
}

fn run_inner(args: &[String], home: &Path) -> Result<(), MigrationError> {
    match args.first().map(String::as_str) {
        Some("discover") => {
            let configs = discover_configs(home)?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"configs": configs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()})
                )
                .unwrap()
            );
            Ok(())
        }
        Some("dry-run") => dry_run(&args[1..], home),
        Some("apply") => apply_cmd(&args[1..]),
        _ => Err(MigrationError(
            "uso: comandos browser migrate-config <discover|dry-run|apply>".to_owned(),
        )),
    }
}

fn default_wrapper(home: &Path) -> String {
    home.join(".local/bin/cc-browser-remote")
        .display()
        .to_string()
}

fn dry_run(args: &[String], home: &Path) -> Result<(), MigrationError> {
    let mut configs = Vec::new();
    let mut wrapper = default_wrapper(home);
    let mut plan = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--config" => {
                i += 1;
                configs.push(PathBuf::from(
                    args.get(i)
                        .ok_or_else(|| MigrationError("falta --config".to_owned()))?,
                ));
            }
            "--wrapper" => {
                i += 1;
                wrapper = args
                    .get(i)
                    .ok_or_else(|| MigrationError("falta --wrapper".to_owned()))?
                    .to_owned();
            }
            "--plan" => {
                i += 1;
                plan = Some(PathBuf::from(
                    args.get(i)
                        .ok_or_else(|| MigrationError("falta --plan".to_owned()))?,
                ));
            }
            other => return Err(MigrationError(format!("argumento desconocido: {other}"))),
        }
        i += 1;
    }
    let plan_path = plan.ok_or_else(|| MigrationError("falta --plan".to_owned()))?;
    let plan = build_plan(&configs, &wrapper)?;
    private_write(
        &plan_path,
        (serde_json::to_string_pretty(&plan).unwrap() + "\n").as_bytes(),
        0o600,
    )?;
    let files = plan["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| json!({"path":item["path"],"changed":item["changed"]}))
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"plan":plan_path.display().to_string(),"files":files})
        )
        .unwrap()
    );
    Ok(())
}

fn apply_cmd(args: &[String]) -> Result<(), MigrationError> {
    let mut plan = None;
    let mut backup_dir = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--plan" => {
                i += 1;
                plan = Some(PathBuf::from(
                    args.get(i)
                        .ok_or_else(|| MigrationError("falta --plan".to_owned()))?,
                ));
            }
            "--backup-dir" => {
                i += 1;
                backup_dir =
                    Some(PathBuf::from(args.get(i).ok_or_else(|| {
                        MigrationError("falta --backup-dir".to_owned())
                    })?));
            }
            other => return Err(MigrationError(format!("argumento desconocido: {other}"))),
        }
        i += 1;
    }
    let raw = read_regular(&plan.ok_or_else(|| MigrationError("falta --plan".to_owned()))?)?.0;
    let plan: Value = serde_json::from_slice(&raw)
        .map_err(|_| MigrationError("Invalid migration plan".to_owned()))?;
    let backups = apply_plan(
        &plan,
        &backup_dir.ok_or_else(|| MigrationError("falta --backup-dir".to_owned()))?,
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"changed_files":backups.len(),"backups":backups}))
            .unwrap()
    );
    Ok(())
}

pub fn transform_json(text: &str, wrapper: &str) -> Result<String, String> {
    let mut data: Value =
        serde_json::from_str(text).map_err(|_| "Invalid JSON configuration".to_owned())?;
    if !data.is_object() {
        return Err("Configuration must be a JSON object".to_owned());
    }
    let original = data.clone();
    rewrite_json_servers(&mut data, wrapper);
    if let Some(projects) = data.get_mut("projects").and_then(Value::as_object_mut) {
        for project in projects.values_mut() {
            rewrite_json_servers(project, wrapper);
        }
    }
    if let Some(plugins) = data
        .get_mut("enabledPlugins")
        .and_then(Value::as_object_mut)
        && plugins.contains_key(PLUGIN)
    {
        plugins.insert(PLUGIN.to_owned(), Value::Bool(false));
    }
    if data == original {
        return Ok(text.to_owned());
    }
    Ok(serde_json::to_string_pretty(&data).unwrap() + "\n")
}

fn rewrite_json_servers(data: &mut Value, wrapper: &str) {
    let Some(entries) = data.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return;
    };
    if !TARGETS.iter().any(|name| entries.contains_key(*name)) {
        return;
    }
    for name in TARGETS {
        entries.remove(*name);
    }
    entries.insert(
        "chrome-bg".to_owned(),
        json!({"type":"stdio","command":wrapper,"args":[]}),
    );
}

pub fn transform_toml(text: &str, wrapper: &str) -> Result<String, String> {
    if !TARGETS.iter().any(|name| {
        text.contains(&format!("[mcp_servers.{name}]"))
            || text.contains(&format!("[mcp_servers.\"{name}\"]"))
    }) {
        return Ok(text.to_owned());
    }
    if text.contains("mcp_servers = {") {
        return Err(
            "Unsupported TOML layout; targeted servers must use separate table blocks".to_owned(),
        );
    }
    let mut out = String::new();
    let mut dropping = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("[") {
            dropping = TARGETS.iter().any(|name| {
                trimmed.starts_with(&format!("[mcp_servers.{name}]"))
                    || trimmed.starts_with(&format!("[mcp_servers.\"{name}\"]"))
                    || trimmed.starts_with(&format!("[mcp_servers.{name}."))
                    || trimmed.starts_with(&format!("[mcp_servers.\"{name}\"."))
            });
        }
        if !dropping {
            out.push_str(line);
        }
    }
    if !out.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out.push_str("\n[mcp_servers.chrome-bg]\ncommand = ");
    out.push_str(&serde_json::to_string(wrapper).unwrap());
    out.push_str("\nargs = []\n");
    Ok(out)
}

fn build_plan(paths: &[PathBuf], wrapper: &str) -> Result<Value, MigrationError> {
    if !Path::new(wrapper).is_absolute() || wrapper.contains('\n') || wrapper.contains('\r') {
        return Err(MigrationError(
            "Wrapper must be an absolute single-line path".to_owned(),
        ));
    }
    let mut paths = paths
        .iter()
        .map(|p| fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    let mut files = Vec::new();
    for path in paths {
        let (raw, meta) = read_regular(&path)?;
        let changed = transform(&path, &raw, wrapper)?;
        files.push(json!({
            "path":path.display().to_string(),
            "before_sha256":digest(&raw),
            "after_sha256":digest(&changed),
            "changed":changed != raw,
            "device":meta.dev(),
            "inode":meta.ino()
        }));
    }
    Ok(json!({"version":1,"wrapper":wrapper,"files":files}))
}

fn apply_plan(plan: &Value, backup_dir: &Path) -> Result<Vec<String>, MigrationError> {
    if plan.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(MigrationError("Unsupported migration plan".to_owned()));
    }
    let files = plan
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| MigrationError("Unsupported migration plan".to_owned()))?;
    let wrapper = plan
        .get("wrapper")
        .and_then(Value::as_str)
        .ok_or_else(|| MigrationError("Unsupported migration plan".to_owned()))?;
    let mut staged = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in files {
        let path = PathBuf::from(entry["path"].as_str().unwrap_or(""));
        if !path.is_absolute() || !seen.insert(path.clone()) {
            return Err(MigrationError(
                "Plan contains relative or duplicate paths".to_owned(),
            ));
        }
        let (raw, meta) = verify_entry(entry)?;
        let changed = transform(&path, &raw, wrapper)?;
        if digest(&changed) != entry["after_sha256"].as_str().unwrap_or("")
            || entry["changed"].as_bool().unwrap_or(false) != (raw != changed)
        {
            return Err(MigrationError(format!(
                "Plan transformation no longer matches review: {}",
                path.display()
            )));
        }
        if raw != changed {
            staged.push((
                entry.clone(),
                path,
                raw,
                changed,
                meta.permissions().mode() & 0o777,
            ));
        }
    }
    if staged.is_empty() {
        return Ok(Vec::new());
    }
    if backup_dir.is_symlink() {
        return Err(MigrationError(
            "Backup directory must not be a symlink".to_owned(),
        ));
    }
    fs::create_dir_all(backup_dir).map_err(|e| MigrationError(e.to_string()))?;
    fs::set_permissions(backup_dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| MigrationError(e.to_string()))?;
    let run = backup_dir.join(format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&run).map_err(|e| MigrationError(e.to_string()))?;
    fs::set_permissions(&run, fs::Permissions::from_mode(0o700))
        .map_err(|e| MigrationError(e.to_string()))?;
    let mut backups = Vec::new();
    for (index, (entry, path, raw, _, _)) in staged.iter().enumerate() {
        let backup = run.join(format!(
            "{index}-{}",
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("config")
        ));
        private_write(&backup, raw, 0o600)?;
        backups.push(backup.display().to_string());
        let _ = entry;
    }
    for entry in files {
        verify_entry(entry)?;
    }
    for (entry, path, _, changed, mode) in staged {
        verify_entry(&entry)?;
        private_write(&path, &changed, mode)?;
    }
    Ok(backups)
}

fn verify_entry(entry: &Value) -> Result<(Vec<u8>, fs::Metadata), MigrationError> {
    let path = PathBuf::from(entry["path"].as_str().unwrap_or(""));
    let (raw, meta) = read_regular(&path)?;
    if digest(&raw) != entry["before_sha256"].as_str().unwrap_or("")
        || meta.dev() != entry["device"].as_u64().unwrap_or(0)
        || meta.ino() != entry["inode"].as_u64().unwrap_or(0)
    {
        return Err(MigrationError(format!(
            "Configuration changed since review: {}",
            path.display()
        )));
    }
    Ok((raw, meta))
}

fn transform(path: &Path, raw: &[u8], wrapper: &str) -> Result<Vec<u8>, MigrationError> {
    let text = std::str::from_utf8(raw)
        .map_err(|_| MigrationError(format!("Configuration is not UTF-8: {}", path.display())))?;
    let out = match path.extension().and_then(|s| s.to_str()) {
        Some("json") => transform_json(text, wrapper).map_err(MigrationError)?,
        Some("toml") => transform_toml(text, wrapper).map_err(MigrationError)?,
        _ => {
            return Err(MigrationError(format!(
                "Unsupported configuration extension: {}",
                path.display()
            )));
        }
    };
    Ok(out.into_bytes())
}

fn read_regular(path: &Path) -> Result<(Vec<u8>, fs::Metadata), MigrationError> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| {
            MigrationError(format!(
                "Cannot read regular configuration file: {}",
                path.display()
            ))
        })?;
    let meta = file
        .metadata()
        .map_err(|_| MigrationError("Configuration is not a regular file".to_owned()))?;
    if !meta.is_file() {
        return Err(MigrationError(
            "Configuration is not a regular file".to_owned(),
        ));
    }
    let mut raw = Vec::new();
    file.read_to_end(&mut raw)
        .map_err(|e| MigrationError(e.to_string()))?;
    Ok((raw, meta))
}

fn private_write(path: &Path, data: &[u8], mode: u32) -> Result<(), MigrationError> {
    let tmp = path.with_file_name(format!(
        ".{}.{}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("tmp"),
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(&tmp)
        .map_err(|e| MigrationError(e.to_string()))?;
    file.set_permissions(fs::Permissions::from_mode(mode))
        .map_err(|e| MigrationError(e.to_string()))?;
    file.write_all(data)
        .map_err(|e| MigrationError(e.to_string()))?;
    file.sync_all().map_err(|e| MigrationError(e.to_string()))?;
    fs::rename(&tmp, path).map_err(|e| MigrationError(e.to_string()))
}

fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn discover_configs(home: &Path) -> Result<Vec<PathBuf>, MigrationError> {
    let mut paths = Vec::new();
    let claude = home.join(".claude.json");
    if claude.is_file() && !claude.is_symlink() {
        paths.push(claude);
    }
    for (kind, root) in [
        ("claude", home.join(".claude")),
        ("codex", home.join(".codex")),
        ("grok", home.join(".grok")),
    ] {
        let candidates = [root.clone()];
        for candidate in candidates {
            if kind == "claude" {
                for name in [".claude.json", "claude.json", "settings.json"] {
                    let path = candidate.join(name);
                    if path.is_file() && !path.is_symlink() {
                        paths.push(path);
                    }
                }
            } else {
                let path = candidate.join("config.toml");
                if path.is_file() && !path.is_symlink() {
                    paths.push(path);
                }
            }
        }
    }
    paths.sort();
    Ok(paths)
}
