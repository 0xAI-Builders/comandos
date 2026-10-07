//! Native O5 executor. No shell, shadow HOME, fork, or process replacement by PID.
//! A successful launch replaces this process with exec, retaining the same PID.
use super::*;
use std::{collections::HashMap, ffi::OsString, os::unix::process::CommandExt};

pub const ERROR: &str = "No se pudo aplicar la configuración privada de extensiones.";
#[derive(Debug)]
pub struct ResolvedCommand {
    pub argv: Vec<String>,
    pub environment: HashMap<OsString, OsString>,
    pub settings: Option<Value>,
}
fn other<T>(r: Result<T, impl std::fmt::Debug>) -> Result<T, LaunchError> {
    r.map_err(|_| LaunchError::Other)
}
fn strings(v: &Value) -> Result<Vec<String>, LaunchError> {
    v.as_array()
        .ok_or(LaunchError::Other)?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or(LaunchError::Other))
        .collect()
}
fn parse(text: &str) -> Result<Value, LaunchError> {
    py_json(text)?.map_err(LaunchError::Value)
}
fn merge(left: &mut Value, right: &Value) -> Result<(), LaunchError> {
    let l = left.as_object_mut().ok_or(LaunchError::Other)?;
    for (k, v) in right.as_object().ok_or(LaunchError::Other)? {
        if v.is_object() && l.get(k).is_some_and(Value::is_object) {
            merge(l.get_mut(k).ok_or(LaunchError::Other)?, v)?;
        } else {
            l.insert(k.clone(), v.clone());
        }
    }
    Ok(())
}
fn toml_rules(text: &str) -> Result<Vec<Value>, LaunchError> {
    let parsed = parse_toml(text)?.ok_or(LaunchError::Other)?;
    parsed
        .get("skills")
        .and_then(|v| v.get("config"))
        .and_then(Value::as_array)
        .cloned()
        .ok_or(LaunchError::Other)
}
/// Full `_resolve_command`, including the transformed argv, merged env and Claude settings.
/// Unrelated environment bytes are retained; only managed keys require JSON/UTF-8.
pub fn resolve_command(
    data: &Value,
    words: &[String],
    environ: &HashMap<OsString, OsString>,
) -> Result<ResolvedCommand, LaunchError> {
    let data = data.as_object().ok_or(LaunchError::Other)?;
    let text_env = environ
        .iter()
        .filter_map(|(k, v)| Some((k.to_str()?.to_owned(), v.to_str()?.to_owned())))
        .collect();
    // Shared original validations preserve wrap_command's exception contract.
    super::resolve_command(data, words, &text_env)?;
    let mut command = words.to_vec();
    let mut env = environ.clone();
    if command.first().is_some_and(|w| w == "env") {
        command.remove(0);
        while command.first().is_some_and(|w| w.starts_with('-')) {
            let flag = command.remove(0);
            if matches!(flag.as_str(), "-u" | "--unset") && !command.is_empty() {
                env.remove(&OsString::from(command.remove(0)));
            } else if let Some(k) = flag.strip_prefix("--unset=") {
                env.remove(&OsString::from(k));
            } else if flag == "--" {
                break;
            } else {
                return Err(LaunchError::Other);
            }
        }
    }
    while let Some((k, v)) = command.first().and_then(|w| assignment(w)) {
        env.insert(k.into(), v.into());
        command.remove(0);
    }
    let mut extra = strings(data.get("args").ok_or(LaunchError::Other)?)?;
    let harness = data
        .get("bundle")
        .and_then(|v| v.get("harness"))
        .and_then(Value::as_str)
        .ok_or(LaunchError::Other)?;
    let selected_env = data
        .get("env")
        .and_then(Value::as_object)
        .ok_or(LaunchError::Other)?;
    if harness == "opencode" {
        let prior = env
            .get(&OsString::from("OPENCODE_CONFIG_CONTENT"))
            .map(|v| v.to_str().ok_or(LaunchError::Other))
            .transpose()?
            .unwrap_or("{}");
        let mut prior = parse(prior)?;
        if prior.get("permission").is_some_and(Value::is_string) {
            prior["permission"] = json!({"*":prior["permission"]});
        }
        let selected = parse(
            selected_env
                .get("OPENCODE_CONFIG_CONTENT")
                .and_then(Value::as_str)
                .ok_or(LaunchError::Other)?,
        )?;
        merge(&mut prior, &selected)?;
        env.insert(
            "OPENCODE_CONFIG_CONTENT".into(),
            compact_response(&prior)?.into(),
        );
    } else {
        for (k, v) in selected_env {
            env.insert(k.into(), v.as_str().ok_or(LaunchError::Other)?.into());
        }
    }
    // Never allow a malformed private manifest to replace HOME via its overlay.
    if env.get(&OsString::from("HOME")).and_then(|v| v.to_str())
        != data.get("home").and_then(Value::as_str)
    {
        return Err(LaunchError::Other);
    }
    if harness == "codex" {
        let mut old = Vec::new();
        for (i, word) in command.iter().enumerate() {
            let val = if matches!(word.as_str(), "-c" | "--config") {
                command.get(i + 1).map_or("", String::as_str)
            } else {
                word.strip_prefix("--config=").unwrap_or("")
            };
            if val.starts_with("skills.config=") {
                old = toml_rules(val)?;
            }
        }
        if !old.is_empty() {
            for word in &mut extra {
                if word.starts_with("skills.config=") {
                    let mut rules = toml_rules(word)?;
                    let count = data
                        .get("codexSelectedSkillCount")
                        .and_then(Value::as_u64)
                        .ok_or(LaunchError::Other)?;
                    let count = usize::try_from(count).map_err(|_| LaunchError::Other)?;
                    let selected = rules.split_off(rules.len().saturating_sub(count));
                    rules.extend(old.clone());
                    rules.extend(selected);
                    *word = format!("skills.config={}", launch_toml(&json!(rules))?);
                }
            }
        }
    }
    let settings = if harness == "claude" {
        let mut settings = json!({});
        let mut cleaned = Vec::new();
        let mut i = 0;
        while let Some(word) = command.get(i) {
            i += 1;
            if word == "--settings" || word.starts_with("--settings=") {
                let val = if word == "--settings" {
                    let val = command.get(i).ok_or(LaunchError::Other)?;
                    i += 1;
                    val.as_str()
                } else {
                    word.split_once('=').map_or("", |(_, v)| v)
                };
                let raw = if strip_start(val).starts_with('{') {
                    val.to_owned()
                } else {
                    universal_text(other(fs::read(val))?).ok_or(LaunchError::Other)?
                };
                merge(&mut settings, &parse(&raw)?)?;
            } else if word == "--mcp-config" {
                while command.get(i).is_some_and(|w| !w.starts_with('-')) {
                    i += 1;
                }
            } else if word.starts_with("--mcp-config=")
                || matches!(
                    word.as_str(),
                    "--strict-mcp-config" | "--disable-slash-commands"
                )
            {
            } else {
                cleaned.push(word.clone());
            }
        }
        command = cleaned;
        let i = extra
            .iter()
            .position(|w| w == "--settings")
            .ok_or(LaunchError::Other)?;
        let selected = universal_text(other(fs::read(
            extra.get(i + 1).ok_or(LaunchError::Other)?,
        ))?)
        .ok_or(LaunchError::Other)?;
        merge(&mut settings, &parse(&selected)?)?;
        Some(settings)
    } else {
        None
    };
    command.extend(extra);
    Ok(ResolvedCommand {
        argv: command,
        environment: env,
        settings,
    })
}
fn exec(argv: &[String], env: &HashMap<OsString, OsString>) -> Result<(), LaunchError> {
    let (bin, args) = argv.split_first().ok_or(LaunchError::Other)?;
    let error = std::process::Command::new(bin)
        .args(args)
        .env_clear()
        .envs(env)
        .exec();
    Err(if error.raw_os_error().is_some() {
        LaunchError::Other
    } else {
        LaunchError::Unsure
    })
}
/// `_run_environment`: restore exactly the four captured OpenCode keys before exec.
pub fn run_environment(path: &Path, digest: &str, command: &[String]) -> Result<(), LaunchError> {
    let meta = other(fs::metadata(path))?;
    let parent = path.parent().ok_or(LaunchError::Other)?;
    if is_link(path) || !private_mode(&meta) || !private_mode(&other(fs::metadata(parent))?) {
        return Err(LaunchError::Other);
    }
    let raw = other(fs::read(path))?;
    if sha256_hex(&raw) != digest {
        return Err(LaunchError::Other);
    }
    let data = py_json_bytes(&raw)?.map_err(LaunchError::Value)?;
    if !python_eq(&data["version"], &json!(1)) {
        return Err(LaunchError::Other);
    }
    let values = data["values"].as_object().ok_or(LaunchError::Other)?;
    if values
        .keys()
        .any(|k| !OPENCODE_ENV_KEYS.contains(&k.as_str()))
    {
        return Err(LaunchError::Other);
    }
    let mut env: HashMap<OsString, OsString> = std::env::vars_os().collect();
    for key in OPENCODE_ENV_KEYS {
        env.remove(&OsString::from(key));
    }
    for (k, v) in values {
        env.insert(k.into(), v.as_str().ok_or(LaunchError::Other)?.into());
    }
    exec(command, &env)
}
fn platform_mounts(mounts: &[Value]) -> Result<(), LaunchError> {
    if !mounts.is_empty() && !cfg!(target_os = "linux") {
        return Err(LaunchError::Other);
    }
    Ok(())
}
/// Validate the private manifest, create exclusive receipts, then exec in this PID.
pub fn run_manifest(path: &Path, command: &[String]) -> Result<(), LaunchError> {
    let data = load_manifest(path, None)?;
    let mounts = data["mounts"].as_array().ok_or(LaunchError::Other)?;
    platform_mounts(mounts)?; // Darwin refuses before any generated file or namespace effect.
    let mut resolved = resolve_command(&data, command, &std::env::vars_os().collect())?;
    let parent = path.parent().ok_or(LaunchError::Other)?;
    let mut artifacts = Map::new();
    if let Some(settings) = resolved.settings {
        let settings_path = parent.join(format!("settings-run-{}.json", uuid4_hex()?));
        other(write_private(
            &settings_path,
            other(response_dumps(&settings))?.as_bytes(),
        ))?;
        let name = settings_path.to_str().ok_or(LaunchError::Unsure)?;
        artifacts.insert(
            name.into(),
            sha256_hex(&other(fs::read(&settings_path))?).into(),
        );
        let i = resolved
            .argv
            .iter()
            .rposition(|w| w == "--settings")
            .ok_or(LaunchError::Other)?;
        *resolved.argv.get_mut(i + 1).ok_or(LaunchError::Other)? = name.into();
    }
    let digest = sha256_hex(&other(fs::read(path))?);
    let receipt = parent.join(format!("receipt-{}.json", uuid4_hex()?));
    let mut receipt_env = Map::new();
    for key in data["env"]
        .as_object()
        .ok_or(LaunchError::Other)?
        .keys()
        .map(String::as_str)
        .chain(
            if data["bundle"]["harness"] == "opencode" {
                OPENCODE_ENV_KEYS.as_slice()
            } else {
                &[]
            }
            .iter()
            .copied(),
        )
    {
        receipt_env.insert(
            key.into(),
            resolved
                .environment
                .get(&OsString::from(key))
                .map(|v| v.to_str().map(Value::from).ok_or(LaunchError::Other))
                .transpose()?
                .unwrap_or(Value::Null),
        );
    }
    other(write_private(&receipt,other(response_dumps(&json!({"manifestSha256":digest,"argv":resolved.argv,"env":receipt_env,"artifacts":artifacts})))?.as_bytes()))?;
    for (k, v) in [
        (
            MARKER,
            data["bundle"]["operationId"]
                .as_str()
                .ok_or(LaunchError::Other)?
                .to_owned(),
        ),
        (
            MANIFEST_ENV,
            path.to_str().ok_or(LaunchError::Unsure)?.to_owned(),
        ),
        (DIGEST_ENV, digest),
        (
            RECEIPT_ENV,
            receipt.to_str().ok_or(LaunchError::Unsure)?.to_owned(),
        ),
        (RECEIPT_HASH_ENV, sha256_hex(&other(fs::read(&receipt))?)),
    ] {
        resolved.environment.insert(k.into(), v.into());
    }
    if !mounts.is_empty() {
        namespace(mounts)?;
    }
    exec(&resolved.argv, &resolved.environment)
}
#[cfg(target_os = "linux")]
fn namespace(mounts: &[Value]) -> Result<(), LaunchError> {
    use nix::{
        mount::{MsFlags, mount},
        sched::{CloneFlags, unshare},
    };
    // Own this process's namespaces; no target process is signalled or modified.
    let user = nix::unistd::getuid().as_raw();
    let group = nix::unistd::getgid().as_raw();
    if user == 0 {
        return Err(LaunchError::Other);
    } // Exec of uid 0 would retain namespace capabilities.
    other(unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNS))?;
    other(fs::write("/proc/self/setgroups", "deny"))?;
    other(fs::write("/proc/self/uid_map", format!("{user} {user} 1")))?;
    other(fs::write(
        "/proc/self/gid_map",
        format!("{group} {group} 1"),
    ))?;
    other(mount::<str, str, str, str>(
        None,
        "/",
        None,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None,
    ))?;
    bind_mounts(mounts)?;
    other(nix::sys::prctl::set_no_new_privs())
}
#[cfg(target_os = "linux")]
fn bind_mounts(mounts: &[Value]) -> Result<(), LaunchError> {
    use nix::mount::{MsFlags, mount};
    for entry in mounts {
        let source = Path::new(entry["source"].as_str().ok_or(LaunchError::Other)?);
        let target = Path::new(entry["target"].as_str().ok_or(LaunchError::Other)?);
        other(mount::<Path, Path, str, str>(
            Some(source),
            target,
            None,
            MsFlags::MS_BIND,
            None,
        ))?;
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn namespace(_mounts: &[Value]) -> Result<(), LaunchError> {
    Err(LaunchError::Other)
}
#[cfg(target_os = "linux")]
fn namespace_proof(path: &Path) -> Result<(), LaunchError> {
    let mut data = py_json_bytes(&other(fs::read(path))?)?.map_err(LaunchError::Value)?;
    let mounts = data["mounts"].as_array().ok_or(LaunchError::Other)?;
    if nix::unistd::getuid().as_raw() == 0 {
        return Err(LaunchError::Other);
    }
    let exe = other(std::env::current_exe())?;
    match data.get("namespaceReady") {
        None => {
            // unshare conserva capacidades tras exec; setpriv aún podrá retirar CapBnd.
            let mut inherited = Map::new();
            for name in ["user", "mnt"] {
                let link = other(fs::read_link(format!("/proc/self/ns/{name}")))?;
                inherited.insert(name.into(), link.to_str().ok_or(LaunchError::Other)?.into());
            }
            let staged = path
                .parent()
                .ok_or(LaunchError::Other)?
                .join(format!("namespace-ready-{}.json", uuid4_hex()?));
            let object = data.as_object_mut().ok_or(LaunchError::Other)?;
            object.insert("namespaceReady".into(), Value::Bool(true));
            object.insert("namespaceParent".into(), Value::Object(inherited));
            other(write_private(
                &staged,
                other(response_dumps(&data))?.as_bytes(),
            ))?;
            let _ = std::process::Command::new("/usr/bin/unshare")
                .args([
                    "--user",
                    "--mount",
                    "--keep-caps",
                    "--propagation",
                    "private",
                    "--map-user",
                ])
                .arg(nix::unistd::getuid().as_raw().to_string())
                .arg("--map-group")
                .arg(nix::unistd::getgid().as_raw().to_string())
                .arg("--")
                .arg(exe)
                .args(["extension-session", "--namespace-proof"])
                .arg(staged)
                .exec();
            return Err(LaunchError::Other);
        }
        Some(Value::Bool(true)) => {}
        _ => return Err(LaunchError::Other),
    }
    // Guardar las identidades antes de unshare evita leer procfs de otro namespace.
    let inherited = data
        .get("namespaceParent")
        .and_then(Value::as_object)
        .ok_or(LaunchError::Other)?;
    for name in ["user", "mnt"] {
        let current = other(fs::read_link(format!("/proc/self/ns/{name}")))?;
        let previous = inherited
            .get(name)
            .and_then(Value::as_str)
            .ok_or(LaunchError::Other)?;
        if current.to_str().ok_or(LaunchError::Other)? == previous {
            return Err(LaunchError::Other);
        }
    }
    for (name, id) in [
        ("uid", nix::unistd::getuid().as_raw()),
        ("gid", nix::unistd::getgid().as_raw()),
    ] {
        let mapping = other(fs::read_to_string(format!("/proc/self/{name}_map")))?;
        let mut fields = mapping.split_whitespace();
        let read_id = |field: Option<&str>| field.and_then(|value| value.parse::<u32>().ok());
        if read_id(fields.next()) != Some(id)
            || read_id(fields.next()) != Some(id)
            || read_id(fields.next()) != Some(1)
            || fields.next().is_some()
        {
            return Err(LaunchError::Other);
        }
    }
    if other(fs::read_to_string("/proc/self/setgroups"))?.trim() != "deny" {
        return Err(LaunchError::Other);
    }
    bind_mounts(mounts)?;
    // Retirar todos los conjuntos antes del último exec, sin evaluar comandos shell.
    let _ = std::process::Command::new("/usr/bin/setpriv")
        .args([
            "--no-new-privs",
            "--bounding-set=-all",
            "--inh-caps=-all",
            "--ambient-caps=-all",
            "--",
        ])
        .arg(exe)
        .args(["extension-session", "--namespace-check"])
        .arg(path)
        .exec();
    Err(LaunchError::Other)
}
#[cfg(not(target_os = "linux"))]
fn namespace_proof(_path: &Path) -> Result<(), LaunchError> {
    Err(LaunchError::Other)
}
fn capabilities_are_zero(status: &str) -> bool {
    const FIELDS: [&str; 5] = ["CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"];
    FIELDS.iter().all(|field| {
        let mut values = status.lines().filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name == *field).then_some(value.trim())
        });
        let Some(value) = values.next() else {
            return false;
        };
        value.len() == 16
            && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            && u64::from_str_radix(value, 16) == Ok(0)
            && values.next().is_none()
    })
}
fn namespace_check(path: &Path) -> Result<(), LaunchError> {
    let data = py_json_bytes(&other(fs::read(path))?)?.map_err(LaunchError::Value)?;
    let target = data["target"].as_str().ok_or(LaunchError::Other)?;
    if other(fs::read(target))? != b"child"
        || std::env::var("HOME").ok().as_deref() != data["home"].as_str()
    {
        return Err(LaunchError::Other);
    }
    let status = other(fs::read_to_string("/proc/self/status"))?;
    if !capabilities_are_zero(&status) {
        return Err(LaunchError::Other);
    }
    println!("namespace-proof-ok");
    Ok(())
}
#[derive(Default)]
struct Options {
    manifest: Option<String>,
    environment: Option<String>,
    digest: Option<String>,
    proof: Option<String>,
    check: Option<String>,
    command: Vec<String>,
    help: bool,
}
fn options(args: &[String]) -> Result<Options, ()> {
    let mut opts = Options::default();
    let mut i = 0;
    while let Some(word) = args.get(i) {
        i += 1;
        if word == "--" {
            opts.command = args.get(i..).unwrap_or_default().to_vec();
            break;
        }
        if !word.starts_with('-') {
            opts.command = args.get(i - 1..).unwrap_or_default().to_vec();
            break;
        }
        if word == "--help" || word == "-h" {
            opts.help = true;
            return Ok(opts);
        }
        let (flag, val) = word
            .split_once('=')
            .map_or((word.as_str(), None), |(k, v)| (k, Some(v.to_owned())));
        let slot = match flag {
            "--manifest" => &mut opts.manifest,
            "--environment-file" => &mut opts.environment,
            "--environment-sha256" => &mut opts.digest,
            "--namespace-proof" => &mut opts.proof,
            "--namespace-check" => &mut opts.check,
            _ => return Err(()),
        };
        let val = match val {
            Some(v) => v,
            None => {
                let v = args
                    .get(i)
                    .filter(|v| !v.starts_with('-'))
                    .ok_or(())?
                    .clone();
                i += 1;
                v
            }
        };
        *slot = Some(val);
    }
    Ok(opts)
}
pub fn main(args: &[String]) -> i32 {
    let Ok(opts) = options(args) else {
        eprintln!("{ERROR}");
        return 2;
    };
    if opts.help {
        println!(
            "uso: cc-extension-session [--manifest PATH | --environment-file PATH --environment-sha256 HASH | --namespace-proof PATH] -- COMMAND [ARGS...]"
        );
        return 0;
    }
    let result = if let Some(path) = opts.environment {
        run_environment(
            Path::new(&path),
            opts.digest.as_deref().unwrap_or(""),
            &opts.command,
        )
    } else if let Some(path) = opts.proof {
        namespace_proof(Path::new(&path))
    } else if let Some(path) = opts.check {
        namespace_check(Path::new(&path))
    } else if let Some(path) = opts.manifest {
        run_manifest(Path::new(&path), &opts.command)
    } else {
        Err(LaunchError::Other)
    };
    if result.is_err() {
        eprintln!("{ERROR}");
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    const CAPABILITY_FIELDS: [&str; 5] = ["CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"];

    fn status(change: Option<(&str, &str)>, omit: Option<&str>) -> String {
        CAPABILITY_FIELDS
            .iter()
            .filter(|field| Some(**field) != omit)
            .map(|field| {
                let value = change
                    .filter(|(changed, _)| changed == field)
                    .map_or("0000000000000000", |(_, value)| value);
                format!("{field}:\t{value}\n")
            })
            .collect()
    }

    #[test]
    fn namespace_capabilities_require_all_five_unique_valid_zero_fields() {
        let complete = status(None, None);
        assert!(super::capabilities_are_zero(&format!(
            "Name:\tprivate-proof\n{complete}"
        )));
        for field in CAPABILITY_FIELDS {
            assert!(
                !super::capabilities_are_zero(&status(None, Some(field))),
                "missing {field}"
            );
            assert!(
                !super::capabilities_are_zero(&format!("{complete}{field}:\t0000000000000000\n")),
                "duplicate {field}"
            );
            for value in [
                "0000000000000001",
                "g000000000000000",
                "0000000000000000 extra",
                "",
                "+000000000000000",
                "00000000000000000",
            ] {
                assert!(
                    !super::capabilities_are_zero(&status(Some((field, value)), None)),
                    "invalid or nonzero {field}: {value:?}"
                );
            }
        }
    }
}
