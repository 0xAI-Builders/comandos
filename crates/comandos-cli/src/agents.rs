//! Agent configuration and presence checks. Presence never runs an agent CLI.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
fn present(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            fs::metadata(dir.join(name))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}
fn marker(home: &Path, path: &str, text: &str) -> bool {
    fs::read(home.join(path)).is_ok_and(|bytes| {
        regex::bytes::RegexBuilder::new(text)
            .unicode(false)
            .build()
            .is_ok_and(|pattern| pattern.is_match(&bytes))
    })
}
fn line(out: &mut String, color: u8, symbol: &str, text: &str) {
    out.push_str(&format!("  \x1b[{color}m{symbol}\x1b[0m {text}\n"));
}
fn ok(out: &mut String, text: &str) {
    line(out, 32, "✓", text)
}
fn missing(out: &mut String, text: &str) {
    line(out, 90, "·", text)
}
fn warn(out: &mut String, text: &str) {
    line(out, 33, "!", text)
}
pub fn main(args: &[String]) -> i32 {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let mut out = String::new();
    if args.first().is_some_and(|s| s == "setup") {
        out.push_str("Conectando agentes a ComandOS:\n");
        ok(&mut out, "claude: hooks nativos (los instala install.sh)");
        setup(&home, &mut out);
        out.push_str("\nLos eventos aparecen en el tablero con el badge de su agente.\n");
    } else {
        out.push_str("Integraciones de agentes:\n");
        ok(&mut out, "claude: hooks nativos");
        if !present("codex") {
            missing(&mut out, "codex: no instalado");
        } else {
            match (
                marker(&home, ".codex/config.toml", "codex-notify.sh"),
                marker(&home, ".codex/hooks.json", "codex-hooks.sh"),
            ) {
                (true, true) => ok(
                    &mut out,
                    "codex: conectado (hooks lifecycle + notify fallback)",
                ),
                (_, true) => ok(&mut out, "codex: conectado (hooks lifecycle)"),
                (true, false) => warn(
                    &mut out,
                    "codex: solo notify legacy (corre: cc-agents setup para cards)",
                ),
                _ => warn(
                    &mut out,
                    "codex: instalado pero SIN conectar (corre: cc-agents setup)",
                ),
            }
        }
        if !present("grok") {
            missing(&mut out, "grok: no instalado");
        } else if marker(&home, ".grok/hooks/comandos.json", "StopFailure")
            && marker(&home, ".grok/hooks/comandos.json", "StopCancelled")
        {
            ok(
                &mut out,
                "grok: conectado (hooks heredados + errores/cancelaciones)",
            );
        } else {
            warn(
                &mut out,
                "grok: instalado pero faltan hooks de error/cancelacion (corre: cc-agents setup)",
            );
        }
        for (name, bin, path, needle) in [
            (
                "opencode",
                "opencode",
                ".config/opencode/plugin/comandos.js",
                "comandos",
            ),
            (
                "gemini",
                "gemini",
                ".gemini/settings.json",
                "gemini-hooks.sh",
            ),
            (
                "antigravity (agy)",
                "agy",
                ".gemini/config/hooks.json",
                "agy-hooks.sh",
            ),
        ] {
            if !present(bin) {
                missing(&mut out, &format!("{name}: no instalado"));
            } else if marker(&home, path, needle) {
                ok(&mut out, &format!("{name}: conectado"));
            } else {
                warn(
                    &mut out,
                    &format!("{name}: instalado pero SIN conectar (corre: cc-agents setup)"),
                );
            }
        }
        out.push_str("\nPara conectar las que falten:  cc-agents setup\n");
    }
    print!("{out}");
    0
}

use comandos_core::json::{indent_dumps, workspace_loads};
use comandos_store::files::{FileLock, write_atomic};
use serde_json::{Map, Value, json};
use std::{
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    time::{SystemTime, UNIX_EPOCH},
};

fn script(home: &Path, name: &str) -> String {
    if name == "cc-notify.sh" {
        return home
            .join(".claude/hooks/cc-notify.sh")
            .to_string_lossy()
            .into_owned();
    }
    home.join(".local/bin")
        .join(name)
        .to_string_lossy()
        .into_owned()
}
fn shell_script(home: &Path, name: &str) -> String {
    let path = script(home, name);
    if path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/_-.".contains(&b))
    {
        path
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}
fn owned(command: &str, name: &str, home: &Path) -> bool {
    let unquoted = command
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .map(|s| s.replace("'\\''", "'"));
    let command = unquoted.as_deref().unwrap_or(command);
    let expanded = command.strip_prefix("~/").map(|rest| home.join(rest));
    let command = expanded
        .as_ref()
        .map_or(command, |p| p.to_str().unwrap_or(command));
    command == script(home, name)
        || command.ends_with(&format!("/adapters/{name}"))
        || (name == "cc-notify.sh"
            && command == home.join(".claude/hooks/cc-notify.sh").to_string_lossy())
}

fn private_dirs(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}
/// Back up bytes before mutation. Follow a config symlink, preserving that symlink
/// and its target mode. Exclusive backup names retain all same-second revisions.
fn save(path: &Path, body: &[u8]) -> io::Result<bool> {
    save_with(path, body, true)
}
fn save_with(path: &Path, body: &[u8], follow_symlink: bool) -> io::Result<bool> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    save_at(path, body, follow_symlink, seconds)
}
fn save_at(path: &Path, body: &[u8], follow_symlink: bool, seconds: u64) -> io::Result<bool> {
    let existing = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    if existing.as_deref() == Some(body) {
        return Ok(false);
    }
    private_dirs(
        path.parent()
            .ok_or_else(|| io::Error::other("config sin directorio"))?,
    )?;
    let target = match fs::symlink_metadata(path) {
        Ok(m) if follow_symlink && m.file_type().is_symlink() => fs::canonicalize(path)?,
        Ok(_) => path.to_path_buf(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => path.to_path_buf(),
        Err(e) => return Err(e),
    };
    let mode = fs::metadata(&target)
        .map(|m| m.permissions().mode() & 0o7777)
        .unwrap_or(0o600);
    let stamp = chrono::DateTime::from_timestamp(seconds as i64, 0)
        .ok_or_else(|| io::Error::other("fecha inválida"))?
        .format("%Y%m%d-%H%M%S");
    let mut index = 0usize;
    loop {
        let suffix = if index == 0 {
            String::new()
        } else {
            format!(".{index}")
        };
        let backup = path.with_file_name(format!(
            "{}.bak-comandos-{stamp}{suffix}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&backup)
        {
            Ok(mut f) => {
                f.write_all(existing.as_deref().unwrap_or_default())?;
                f.sync_all()?;
                fs::set_permissions(&backup, fs::Permissions::from_mode(mode))?;
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => index += 1,
            Err(e) => return Err(e),
        }
    }
    write_atomic(&target, body)?;
    Ok(true)
}
fn read_object(path: &Path) -> io::Result<Value> {
    let text = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(json!({})),
        Err(e) => return Err(e),
    };
    let value =
        workspace_loads(&text).map_err(|e| io::Error::other(format!("JSON inválido: {e}")))?;
    if !value.is_object() {
        return Err(io::Error::other("config JSON debe ser un objeto"));
    }
    Ok(value)
}
fn object(value: &mut Value) -> io::Result<&mut Map<String, Value>> {
    value
        .as_object_mut()
        .ok_or_else(|| io::Error::other("hooks debe ser un objeto"))
}
fn groups(value: &mut Value) -> io::Result<&mut Vec<Value>> {
    value
        .as_array_mut()
        .ok_or_else(|| io::Error::other("event hooks debe ser una lista"))
}
fn save_json(path: &Path, value: &Value, ascii: bool) -> io::Result<bool> {
    let mut text = indent_dumps(value, 2, ascii).map_err(io::Error::other)?;
    text.push('\n');
    save(path, text.as_bytes())
}
fn add_group(
    list: &mut Vec<Value>,
    mut hook: Value,
    matcher: Option<&str>,
    home: &Path,
    name: &str,
) -> bool {
    let mut found = false;
    for group in list.iter_mut() {
        if let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) {
            for h in hooks {
                if h.get("type").and_then(Value::as_str) == Some("command")
                    && h.get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|c| owned(c, name, home))
                {
                    h["command"] = json!(shell_script(home, name));
                    found = true;
                }
            }
        }
    }
    if found {
        return false;
    }
    hook["command"] = json!(shell_script(home, name));
    let mut group = json!({"hooks":[hook]});
    if let Some(m) = matcher {
        group["matcher"] = json!(m)
    }
    list.push(group);
    true
}
fn setup(home: &Path, out: &mut String) {
    if !["codex", "grok", "opencode", "gemini", "agy"]
        .iter()
        .any(|bin| present(bin))
    {
        for name in ["codex", "grok", "opencode", "gemini", "antigravity (agy)"] {
            missing(out, &format!("{name}: no instalado"));
        }
        return;
    }
    if !home.is_absolute() || home.to_str().is_none() {
        warn(out, "HOME debe ser una ruta absoluta UTF-8");
        return;
    }
    // Only setup creates controls; status performs presence and file reads only.
    let lock = home.join(".local/share/comandos/agents.lock");
    if let Err(e) = private_dirs(lock.parent().unwrap()) {
        warn(out, &format!("agents: {e}"));
        return;
    }
    let _guard = match FileLock::exclusive(&lock) {
        Ok(g) => g,
        Err(e) => {
            warn(out, &format!("agents: {e}"));
            return;
        }
    };
    for (name, bin, configure) in [
        (
            "codex",
            "codex",
            setup_codex as fn(&Path, &mut String) -> io::Result<()>,
        ),
        ("grok", "grok", setup_grok),
        ("opencode", "opencode", setup_opencode),
        ("gemini", "gemini", setup_gemini),
        ("antigravity (agy)", "agy", setup_agy),
    ] {
        if !present(bin) {
            missing(out, &format!("{name}: no instalado"));
        } else if let Err(e) = configure(home, out) {
            warn(out, &format!("{name}: no pude configurar — {e}"));
        }
    }
}
fn setup_codex(home: &Path, out: &mut String) -> io::Result<()> {
    let cfg = home.join(".codex/config.toml");
    let raw = match fs::read_to_string(&cfg) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let data: toml::Value = raw.parse().map_err(io::Error::other)?;
    if let Some(notify) = data.get("notify") {
        if let Some(command) = notify
            .as_array()
            .and_then(|a| a.first())
            .and_then(toml::Value::as_str)
            .filter(|c| owned(c, "codex-notify.sh", home))
        {
            save(
                &cfg,
                raw.replace(command, &script(home, "codex-notify.sh"))
                    .as_bytes(),
            )?;
            ok(out, "codex: notify fallback ya configurado");
        } else {
            warn(
                out,
                &format!(
                    "codex: ya tienes OTRO notify en {} — dejo solo hooks lifecycle",
                    cfg.display()
                ),
            );
        }
    } else {
        let line = format!(
            "notify = [{}]\n",
            serde_json::to_string(&script(home, "codex-notify.sh"))?
        );
        let mut candidate = if let Some(i) = raw
            .lines()
            .scan(0usize, |offset, line| {
                let i = *offset;
                *offset += line.len() + 1;
                Some((i, line))
            })
            .find(|(_, line)| line.trim_start().starts_with('['))
            .map(|(i, _)| i)
        {
            format!("{}{line}{}", &raw[..i], &raw[i..])
        } else {
            format!(
                "{raw}{}{line}",
                if raw.is_empty() || raw.ends_with('\n') {
                    ""
                } else {
                    "\n"
                }
            )
        };
        // A bracket inside a multiline string is not a section. Validate the root
        // insertion without reformatting the user's comments or values.
        if candidate
            .parse::<toml::Value>()
            .ok()
            .and_then(|v| v.get("notify").cloned())
            .is_none()
        {
            candidate = format!("{line}{raw}");
        }
        let parsed: toml::Value = candidate.parse().map_err(io::Error::other)?;
        if parsed.get("notify").is_none() {
            return Err(io::Error::other("notify no es top-level"));
        }
        save(&cfg, candidate.as_bytes())?;
        ok(
            out,
            &format!(
                "codex: notify fallback -> {}",
                script(home, "codex-notify.sh")
            ),
        );
    }
    let hj = home.join(".codex/hooks.json");
    let mut data = read_object(&hj)?;
    let hooks = object(
        data.as_object_mut()
            .unwrap()
            .entry("hooks")
            .or_insert_with(|| json!({})),
    )?;
    let mut changed = false;
    for (event, matcher) in [
        ("UserPromptSubmit", None),
        ("PermissionRequest", Some("*")),
        ("Stop", None),
    ] {
        changed |= add_group(
            groups(hooks.entry(event).or_insert_with(|| json!([])))?,
            json!({"type":"command","command":"","timeout":30,"statusMessage":"ComandOS"}),
            matcher,
            home,
            "codex-hooks.sh",
        );
    }
    if changed
        || fs::read_to_string(&hj)
            .ok()
            .and_then(|s| workspace_loads(&s).ok())
            .as_ref()
            != Some(&data)
    {
        save_json(&hj, &data, true)?;
    }
    ok(
        out,
        &format!(
            "codex: lifecycle hooks -> {} (working/waiting/done)",
            script(home, "codex-hooks.sh")
        ),
    );
    Ok(())
}
fn setup_grok(home: &Path, out: &mut String) -> io::Result<()> {
    let path = home.join(".grok/hooks/comandos.json");
    let mut data = read_object(&path)?;
    let hooks = object(
        data.as_object_mut()
            .unwrap()
            .entry("hooks")
            .or_insert_with(|| json!({})),
    )?;
    for event in ["StopFailure", "StopCancelled"] {
        add_group(
            groups(hooks.entry(event).or_insert_with(|| json!([])))?,
            json!({"type":"command","command":"","timeout":30}),
            None,
            home,
            "cc-notify.sh",
        );
    }
    save_json(&path, &data, true)?;
    let alias = home.join(".local/bin/grok-hooks.py");
    private_dirs(alias.parent().unwrap())?;
    match fs::symlink_metadata(&alias) {
        Ok(m)
            if m.file_type().is_symlink()
                && fs::read_link(&alias)?
                    .to_string_lossy()
                    .ends_with("/adapters/grok-hooks.py") =>
        {
            fs::remove_file(&alias)?;
            std::os::unix::fs::symlink(home.join(".local/bin/comandos"), &alias)?;
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            std::os::unix::fs::symlink(home.join(".local/bin/comandos"), &alias)?
        }
        Err(e) => return Err(e),
        _ => {}
    }
    ok(
        out,
        "grok: hooks nativos + StopFailure/StopCancelled conectados",
    );
    Ok(())
}
fn setup_opencode(home: &Path, out: &mut String) -> io::Result<()> {
    let path = home.join(".config/opencode/plugin/comandos.js");
    let helper = home.join(".local/share/comandos/opencode-bridge.mjs");
    save(&helper, include_bytes!("opencode_bridge.mjs"))?;
    let uri =
        url::Url::from_file_path(&helper).map_err(|_| io::Error::other("ruta plugin inválida"))?;
    let shim = format!(
        "import {{ bridge }} from {};\nexport const Comandos = async (sdk) => bridge(sdk, {});\nexport default Comandos;\n",
        serde_json::to_string(uri.as_str())?,
        serde_json::to_string(&script(home, "comandos"))?
    );
    save_with(&path, shim.as_bytes(), false)?;
    ok(
        out,
        "opencode: plugin enlazado (idle/permisos/errores llegan a ComandOS)",
    );
    Ok(())
}
fn setup_gemini(home: &Path, out: &mut String) -> io::Result<()> {
    let path = home.join(".gemini/settings.json");
    let mut data = read_object(&path)?;
    let hooks = object(
        data.as_object_mut()
            .unwrap()
            .entry("hooks")
            .or_insert_with(|| json!({})),
    )?;
    let cmd = format!("CC_AGENT=gemini {}", shell_script(home, "gemini-hooks.sh"));
    let mut exists = false;
    for event in ["BeforeAgent", "AfterAgent", "Notification", "SessionEnd"] {
        let list = groups(hooks.entry(event).or_insert_with(|| json!([])))?;
        let mut found = false;
        for group in list.iter_mut() {
            if let Some(hs) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                for h in hs {
                    if h.get("command")
                        .and_then(Value::as_str)
                        .and_then(|c| c.strip_prefix("CC_AGENT=gemini "))
                        .is_some_and(|c| owned(c, "gemini-hooks.sh", home))
                    {
                        h["command"] = json!(cmd);
                        found = true;
                    }
                }
            }
        }
        exists |= found;
        if !found {
            list.push(json!({"hooks":[{"type":"command","command":cmd,"name":"comandos"}]}));
        }
    }
    save_json(&path, &data, false)?;
    if exists {
        ok(out, "gemini: hooks ya configurados")
    } else {
        ok(
            out,
            &format!("gemini: hooks conectados en {}", path.display()),
        )
    }
    Ok(())
}
fn setup_agy(home: &Path, out: &mut String) -> io::Result<()> {
    let path = home.join(".gemini/config/hooks.json");
    let mut data = read_object(&path)?;
    let own = object(
        data.as_object_mut()
            .unwrap()
            .entry("comandos")
            .or_insert_with(|| json!({})),
    )?;
    let mut already = true;
    for (event, arg) in [("PreInvocation", "working"), ("Stop", "done")] {
        let list = groups(own.entry(event).or_insert_with(|| json!([])))?;
        let mut found = false;
        for h in list.iter_mut() {
            if h.get("command")
                .and_then(Value::as_str)
                .and_then(|c| c.strip_suffix(&format!(" {arg}")))
                .is_some_and(|c| owned(c, "agy-hooks.sh", home))
            {
                h["command"] = json!(format!("{} {arg}", shell_script(home, "agy-hooks.sh")));
                found = true;
            }
        }
        already &= found;
        if !found {
            list.push(json!({"type":"command","command":format!("{} {arg}",shell_script(home,"agy-hooks.sh")),"timeout":10}));
        }
    }
    save_json(&path, &data, false)?;
    if already {
        ok(out, "antigravity (agy): hooks ya configurados")
    } else {
        ok(
            out,
            &format!(
                "antigravity (agy): hooks globales en {} (working/done)",
                path.display()
            ),
        )
    }
    let st = home.join(".gemini/antigravity-cli/settings.json");
    let mut data = read_object(&st)?;
    if let Some(status) = data.get_mut("statusLine") {
        if status
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|c| owned(c, "agy-statusline.py", home))
        {
            status["command"] = json!(shell_script(home, "agy-statusline.py"));
            save_json(&st, &data, false)?;
            ok(out, "antigravity (agy): cuota para Analytics ya conectada");
        } else {
            warn(
                out,
                &format!(
                    "antigravity (agy): ya tienes otra statusLine en {} — Analytics no verá la cuota de agy",
                    st.display()
                ),
            );
        }
    } else {
        data["statusLine"] = json!({"type":"command","command":shell_script(home,"agy-statusline.py"),"stack_with_default":true});
        save_json(&st, &data, false)?;
        ok(
            out,
            &format!(
                "antigravity (agy): cuota para Analytics conectada (statusLine en {})",
                st.display()
            ),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_second_backups_are_exclusive_and_retain_all_revisions() {
        let root =
            std::env::temp_dir().join(format!("agents-backup-collision-{}", std::process::id()));
        private_dirs(&root).unwrap();
        let path = root.join("config.json");
        fs::write(&path, b"one").unwrap();
        save_at(&path, b"two", true, 6000).unwrap();
        save_at(&path, b"three", true, 6000).unwrap();
        let backups: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p != &path)
            .collect();
        assert_eq!(backups.len(), 2);
        assert!(
            backups
                .iter()
                .any(|p| p.file_name().unwrap().to_string_lossy().ends_with(".1"))
        );
        let mut bytes: Vec<_> = backups.iter().map(|p| fs::read(p).unwrap()).collect();
        bytes.sort();
        assert_eq!(bytes, vec![b"one".to_vec(), b"two".to_vec()]);
        fs::remove_dir_all(root).unwrap();
    }
}
