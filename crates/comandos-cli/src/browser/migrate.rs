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
            let mut selected_home = home.to_path_buf();
            let mut process_env = true;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--home" => {
                        i += 1;
                        selected_home = args
                            .get(i)
                            .map(PathBuf::from)
                            .ok_or_else(|| MigrationError("falta --home".into()))?;
                    }
                    "--no-process-env" => process_env = false,
                    _ => return Err(MigrationError("argumento desconocido".into())),
                }
                i += 1;
            }
            let configs =
                discover_configs(&selected_home, process_env.then_some(Path::new("/proc")))?;
            println!(
                "{}",
                pretty(
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
    if configs.is_empty() {
        return Err(MigrationError("falta --config".into()));
    }
    let destination = validate_plan_destination(&plan_path, &configs)?;
    let plan = build_plan(&configs, &wrapper)?;
    // Revalidate immediately before writing, and use the resolved directory entry
    // rather than following the original parent aliases again.
    if validate_plan_destination(&plan_path, &configs)? != destination {
        return Err(plan_destination_error());
    }
    private_write(
        &destination,
        (pretty(&plan).unwrap() + "\n").as_bytes(),
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
        pretty(&json!({"plan":plan_path.display().to_string(),"files":files})).unwrap()
    );
    Ok(())
}

fn plan_destination_error() -> MigrationError {
    MigrationError("Plan path must be separate from configurations and not a symlink".into())
}

// Resolve only the ancestors. Configuration leaf admission still uses O_NOFOLLOW.
fn resolved_entry(path: &Path) -> Result<PathBuf, MigrationError> {
    let absolute = std::path::absolute(path).map_err(|_| plan_destination_error())?;
    let leaf = absolute.file_name().ok_or_else(plan_destination_error)?;
    let parent = fs::canonicalize(absolute.parent().ok_or_else(plan_destination_error)?)
        .map_err(|_| plan_destination_error())?;
    Ok(parent.join(leaf))
}

fn validate_plan_destination(plan: &Path, configs: &[PathBuf]) -> Result<PathBuf, MigrationError> {
    let destination = resolved_entry(plan)?;
    let metadata = fs::symlink_metadata(&destination).ok();
    if metadata.as_ref().is_some_and(|m| m.is_symlink()) {
        return Err(plan_destination_error());
    }
    for config in configs {
        let config_entry = resolved_entry(&lexical_absolute(config)?)?;
        if destination == config_entry {
            return Err(plan_destination_error());
        }
        // Hardlinks have distinct directory entries but the same file identity.
        if let (Some(plan_meta), Ok(config_meta)) = (&metadata, fs::symlink_metadata(&config_entry))
            && config_meta.is_file()
            && plan_meta.dev() == config_meta.dev()
            && plan_meta.ino() == config_meta.ino()
        {
            return Err(plan_destination_error());
        }
    }
    Ok(destination)
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
        pretty(&json!({"changed_files":backups.len(),"backups":backups})).unwrap()
    );
    Ok(())
}

pub fn transform_json(text: &str, wrapper: &str) -> Result<String, String> {
    let mut data: Value = comandos_core::json::parse_unique_value(text).map_err(|e| {
        if e.to_string().contains("Duplicate key") {
            "Duplicate JSON key; refusing a potentially lossy rewrite".to_owned()
        } else {
            "Invalid JSON configuration".to_owned()
        }
    })?;
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
    Ok(comandos_core::json::indent_dumps(&data, 2, false)? + "\n")
}

fn rewrite_json_servers(data: &mut Value, wrapper: &str) {
    let Some(entries) = data.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return;
    };
    if !TARGETS.iter().any(|name| entries.contains_key(*name)) {
        return;
    }
    for name in TARGETS {
        entries.shift_remove(*name);
    }
    entries.insert(
        "chrome-bg".to_owned(),
        json!({"type":"stdio","command":wrapper,"args":[]}),
    );
}

pub fn transform_toml(text: &str, wrapper: &str) -> Result<String, String> {
    let data: toml::Value =
        toml::from_str(text).map_err(|_| "Invalid TOML configuration".to_owned())?;
    let Some(servers) = data.get("mcp_servers").and_then(toml::Value::as_table) else {
        return Ok(text.to_owned());
    };
    if !TARGETS.iter().any(|name| servers.contains_key(*name)) {
        return Ok(text.to_owned());
    }
    let mut desired = data.clone();
    let entries = desired
        .get_mut("mcp_servers")
        .and_then(toml::Value::as_table_mut)
        .unwrap();
    for name in TARGETS {
        entries.remove(*name);
    }
    entries.insert(
        "chrome-bg".into(),
        toml::Value::Table(toml::map::Map::from_iter([
            ("command".into(), toml::Value::String(wrapper.into())),
            ("args".into(), toml::Value::Array(Vec::new())),
        ])),
    );
    if desired == data {
        return Ok(text.to_owned());
    }
    let mut out = String::new();
    let mut dropping = false;
    let mut string_state = None;
    for line in text.split_inclusive('\n') {
        if string_state.is_none()
            && let Some(header) = header_path(line)
        {
            dropping = header.len() >= 2
                && header[0] == "mcp_servers"
                && TARGETS.contains(&header[1].as_str());
        }
        if !dropping {
            out.push_str(line);
        }
        string_state = next_string_state(line, string_state);
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\n[mcp_servers.chrome-bg]\ncommand = ");
    out.push_str(&serde_json::to_string(wrapper).unwrap());
    out.push_str("\nargs = []\n");
    if toml::from_str::<toml::Value>(&out).ok().as_ref() != Some(&desired) {
        return Err(
            "Unsupported TOML layout; targeted servers must use separate table blocks".into(),
        );
    }
    Ok(out)
}

fn header_path(line: &str) -> Option<Vec<String>> {
    let line = line.trim_start();
    if !line.starts_with('[') {
        return None;
    }
    let mut node: toml::Value =
        toml::from_str(&format!("{line}\n__migration_marker__ = true\n")).ok()?;
    let mut keys = Vec::new();
    loop {
        let table = node.as_table()?;
        if table.contains_key("__migration_marker__") {
            return Some(keys);
        }
        if table.len() != 1 {
            return None;
        }
        let (key, value) = table.iter().next()?;
        keys.push(key.clone());
        node = value.clone();
        if let Some(array) = node.as_array()
            && array.len() == 1
        {
            node = array[0].clone();
        }
    }
}

fn next_string_state(line: &str, mut state: Option<(u8, usize)>) -> Option<(u8, usize)> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some((quote, length)) = state {
            if quote == b'"' && bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i..].starts_with(&vec![quote; length]) {
                i += length;
                state = None;
            } else {
                i += 1;
            }
        } else if bytes[i] == b'#' {
            break;
        } else if matches!(bytes[i], b'"' | b'\'') {
            let length = if bytes[i..].starts_with(&[bytes[i]; 3]) {
                3
            } else {
                1
            };
            state = Some((bytes[i], length));
            i += length;
        } else {
            i += 1;
        }
    }
    state
}

fn lexical_absolute(path: &Path) -> Result<PathBuf, MigrationError> {
    let path =
        std::path::absolute(path).map_err(|_| MigrationError("Cannot resolve path".into()))?;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            part => out.push(part.as_os_str()),
        }
    }
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
        .map(|p| lexical_absolute(p))
        .collect::<Result<Vec<_>, _>>()?;
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
                meta.permissions().mode() & 0o7777,
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
    let mut entropy = [0u8; 16];
    getrandom::fill(&mut entropy)
        .map_err(|_| MigrationError("Cannot create private backup directory".into()))?;
    let run = backup_dir.join(
        entropy
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    );
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
    let manifest = json!({"files": staged.iter().zip(&backups).map(|((entry, _, _, _, _), backup)| json!({"path": entry["path"], "backup": backup, "before_sha256": entry["before_sha256"]})).collect::<Vec<_>>()});
    private_write(
        &run.join("manifest.json"),
        (pretty(&manifest).unwrap() + "\n").as_bytes(),
        0o600,
    )?;
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
    let mut entropy = [0u8; 16];
    getrandom::fill(&mut entropy)
        .map_err(|_| MigrationError("Cannot create private temporary".into()))?;
    let suffix = entropy
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let tmp = path.with_file_name(format!(
        ".{}.{}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("tmp"),
        suffix
    ));
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&tmp)
            .map_err(|e| MigrationError(e.to_string()))?;
        created = true;
        file.write_all(data)
            .map_err(|e| MigrationError(e.to_string()))?;
        file.set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|e| MigrationError(e.to_string()))?;
        file.sync_all().map_err(|e| MigrationError(e.to_string()))?;
        fs::rename(&tmp, path).map_err(|e| MigrationError(e.to_string()))
    })();
    if created {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn discover_configs(home: &Path, proc_root: Option<&Path>) -> Result<Vec<PathBuf>, MigrationError> {
    discover_configs_with(home, &std::env::vars_os().collect(), proc_root)
}

fn discover_configs_with(
    home: &Path,
    environ: &std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
    proc_root: Option<&Path>,
) -> Result<Vec<PathBuf>, MigrationError> {
    const VARIABLES: &[(&str, &str)] = &[
        ("CLAUDE_CONFIG_DIR", "claude"),
        ("CODEX_HOME", "codex"),
        ("GROK_HOME", "grok"),
    ];
    let mut roots = BTreeSet::new();
    for (variable, kind) in VARIABLES {
        roots.insert((*kind, home.join(format!(".{kind}"))));
        if let Some(root) = environ.get(std::ffi::OsStr::new(variable)) {
            roots.insert((*kind, PathBuf::from(root)));
        }
    }
    if let Some(proc_root) = proc_root
        && let Ok(processes) = fs::read_dir(proc_root)
    {
        for process in processes.flatten() {
            if !process
                .file_name()
                .as_encoded_bytes()
                .iter()
                .all(u8::is_ascii_digit)
                || process
                    .metadata()
                    .map(|m| m.uid() != nix::unistd::Uid::current().as_raw())
                    .unwrap_or(true)
            {
                continue;
            }
            if let Ok(stream) = fs::File::open(process.path().join("environ")) {
                let mut raw = Vec::new();
                let _ = stream.take(1024 * 1024).read_to_end(&mut raw);
                for item in raw.split(|byte| *byte == 0) {
                    if let Some(index) = item.iter().position(|byte| *byte == b'=') {
                        for (variable, kind) in VARIABLES {
                            if &item[..index] == variable.as_bytes() {
                                use std::os::unix::ffi::OsStrExt;
                                roots.insert((
                                    *kind,
                                    PathBuf::from(std::ffi::OsStr::from_bytes(&item[index + 1..])),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    if let Ok(children) = fs::read_dir(home) {
        for child in children.flatten().filter(|child| child.path().is_dir()) {
            let name = child.file_name();
            let name = name.to_string_lossy();
            for (_, kind) in VARIABLES {
                if name.starts_with(&format!(".{kind}-")) || name.starts_with(&format!(".{kind}_"))
                {
                    roots.insert((*kind, child.path()));
                }
            }
        }
    }
    let mut paths = BTreeSet::from([home.join(".claude.json")]);
    for (kind, root) in roots {
        if !root.is_absolute() {
            continue;
        }
        let mut candidates = vec![root.clone()];
        for branch in ["accounts", "profiles"] {
            if let Ok(children) = fs::read_dir(root.join(branch)) {
                candidates.extend(
                    children
                        .flatten()
                        .map(|child| child.path())
                        .filter(|path| path.is_dir()),
                );
            }
        }
        for candidate in candidates {
            if kind == "claude" {
                for name in [".claude.json", "claude.json", "settings.json"] {
                    paths.insert(candidate.join(name));
                }
            } else {
                paths.insert(candidate.join("config.toml"));
            }
        }
    }
    Ok(paths
        .into_iter()
        .filter(|path| path.is_file() && !path.is_symlink())
        .collect())
}

fn pretty(value: &Value) -> Result<String, String> {
    comandos_core::json::indent_dumps(value, 2, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tree() -> PathBuf {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).unwrap();
        let dir = std::env::temp_dir().join(format!("migration-{:x}", u128::from_ne_bytes(bytes)));
        fs::create_dir(&dir).unwrap();
        dir
    }
    #[test]
    fn process_discovery_keeps_only_allowlisted_paths_and_provider_environment() {
        let root = tree();
        let home = root.join("home");
        fs::create_dir(&home).unwrap();
        let process = root.join("proc/123");
        fs::create_dir_all(&process).unwrap();
        let account = root.join("account");
        fs::create_dir(&account).unwrap();
        fs::write(account.join("config.toml"), b"").unwrap();
        let external = root.join("external/profiles/work");
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("settings.json"), b"{}").unwrap();
        let unrelated = root.join("unrelated");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("config.toml"), b"").unwrap();
        fs::write(
            process.join("environ"),
            format!(
                "CODEX_HOME={}\0TOKEN=secret\0OTHER={}\0",
                account.display(),
                unrelated.display()
            ),
        )
        .unwrap();
        let env = std::collections::BTreeMap::from([
            (
                "CLAUDE_CONFIG_DIR".into(),
                root.join("external").into_os_string(),
            ),
            ("OTHER_SECRET".into(), unrelated.into_os_string()),
        ]);
        let found = discover_configs_with(&home, &env, Some(&root.join("proc"))).unwrap();
        assert_eq!(
            found,
            BTreeSet::from([account.join("config.toml"), external.join("settings.json")])
                .into_iter()
                .collect::<Vec<_>>()
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn private_temporary_write_never_follows_predictable_symlink() {
        let root = tree();
        let config = root.join("config.json");
        let victim = root.join("victim");
        fs::write(&victim, b"original").unwrap();
        let predictable = root.join(format!(".config.json.{}", std::process::id()));
        std::os::unix::fs::symlink(&victim, &predictable).unwrap();
        private_write(&config, b"plan", 0o600).unwrap();
        assert_eq!(fs::read(&victim).unwrap(), b"original");
        assert!(predictable.is_symlink());
        assert_eq!(fs::read(&config).unwrap(), b"plan");
        assert_eq!(
            fs::metadata(config).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(root).unwrap();
    }
}
