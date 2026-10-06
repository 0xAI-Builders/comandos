//! Per-launch profile inventory. Shared provider configuration is never edited.
use crate::{
    accounts,
    capabilities::{self as c, Context, Fault, Paths, Result},
    mcp_descriptions,
};
use comandos_core::json::{response_dumps_unicode, truthy};
use comandos_store::session_profiles::valid_extension_name;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
};
use unicode_casefold::UnicodeCaseFold;
pub fn launch_capabilities(h: &str) -> Value {
    let next =
        "Se aplica al iniciar una sesión nueva. Las sesiones abiertas conservan su configuración.";
    json!({"skills":{"status":if h=="codex"{"next_launch"}else{"unsupported"},"supported":h=="codex","reason":if h=="codex"{next}else{"Este CLI no ofrece selección individual verificada de skills por lanzamiento."}},"mcps":{"status":if h=="codex"{"next_launch"}else if h=="claude"{"conditional"}else{"unsupported"},"supported":matches!(h,"codex"|"claude"),"reason":if h=="codex"{next}else if h=="claude"{"Selección de servidores JSON por lanzamiento; sin recarga del agente vivo."}else{"Selección por lanzamiento no verificada para este CLI."}},"hotReload":false})
}
fn invalid(s: &str) -> Fault {
    Fault::Invalid(s.into())
}
fn claude_sources(
    home: &Path,
    cwd: &Path,
    alias: &str,
    paths: &Paths,
) -> Result<serde_json::Map<String, Value>> {
    let mut errors = vec![];
    let global = c::read_config(
        &c::claude_user_json(home, alias, paths),
        &mut errors,
        "claude-user",
    );
    let project = c::read_config(&cwd.join(".mcp.json"), &mut errors, "mcp-json");
    if truthy(&global["projects"][c::resolved(cwd).to_string_lossy().as_ref()]["mcpServers"]) {
        return Err(invalid(
            "MCPs locales de Claude presentes: selección aislada todavía no soportada",
        ));
    }
    for p in [
        home.join("settings.json"),
        cwd.join(".claude/settings.json"),
        cwd.join(".claude/settings.local.json"),
    ] {
        let data = c::read_config(&p, &mut errors, "claude-settings");
        let plugins = &data["enabledPlugins"];
        if truthy(plugins) && (!plugins.is_object() || c::object(plugins).values().any(truthy)) {
            return Err(invalid(
                "plugins de Claude presentes: selección aislada de MCPs todavía no soportada",
            ));
        }
    }
    if !errors.is_empty() {
        return Err(invalid(
            "configuración de Claude ilegible: no se puede preservar la selección de MCPs",
        ));
    }
    let mut servers = c::object(&global["mcpServers"]);
    servers.extend(c::object(&project["mcpServers"]));
    Ok(servers)
}
pub(crate) fn codex_configs(home: &Path) -> Result<Vec<Value>> {
    let mut errors = vec![];
    let data = c::read_config(&home.join("config.toml"), &mut errors, "codex-user");
    if !errors.is_empty() {
        return Err(invalid(
            "configuración TOML ilegible; no se pueden conservar overrides",
        ));
    }
    Ok(vec![data])
}
pub(crate) fn overrides(configs: &[Value]) -> Vec<Value> {
    let mut out = vec![];
    for cfg in configs {
        if let Some(a) = cfg["skills"]["config"].as_array() {
            out = a.iter().filter(|v| v.is_object()).cloned().collect()
        }
    }
    out
}
fn skill_path(v: &Value, base: &Path, paths: &Paths) -> Option<PathBuf> {
    v.as_str().map(|s| {
        let p = if s.starts_with('~') {
            c::expand(s, paths)
        } else {
            PathBuf::from(s)
        };
        c::resolved(&if p.is_absolute() { p } else { base.join(p) })
    })
}
pub(crate) fn skill_roots(
    ctx: &mut Context,
    paths: &Paths,
) -> Vec<(PathBuf, String, String, Option<c::Plugin>)> {
    let mut roots = vec![];
    let Some(home) = ctx.home.clone() else {
        return roots;
    };
    let h = ctx.harness.clone();
    let cwd = ctx.project.clone();
    let mut add =
        |p: PathBuf, scope: &str, source: &str| roots.push((p, scope.into(), source.into(), None));
    match h.as_str() {
        "shell" => return roots,
        "codex" => {
            add(home.join("skills"), "user", "skills-directory");
            add(paths.home.join(".agents/skills"), "user", "shared-skills");
            add(paths.admin_skills.clone(), "admin", "admin-skills");
            for p in &ctx.parents {
                add(p.join(".agents/skills"), "project", "shared-skills");
                add(p.join(".codex/skills"), "project", "skills-directory")
            }
        }
        "claude" => {
            add(home.join("skills"), "user", "skills-directory");
            for p in &ctx.parents {
                add(p.join(".claude/skills"), "project", "skills-directory")
            }
        }
        "grok" => {
            let user = ctx
                .layers
                .first()
                .map(|l| l.data.clone())
                .unwrap_or(Value::Null);
            for vendor in ["claude", "cursor"] {
                if user["compat"][vendor]["skills"] != false {
                    add(
                        paths.home.join(format!(".{vendor}/skills")),
                        "compatible",
                        &format!("{vendor}-skills"),
                    );
                    add(
                        cwd.join(format!(".{vendor}/skills")),
                        "project",
                        &format!("{vendor}-skills"),
                    )
                }
            }
            add(home.join("skills"), "user", "skills-directory");
            for p in &ctx.parents {
                add(p.join(".grok/skills"), "project", "skills-directory")
            }
            for p in c::list(&user["skills"]["paths"]) {
                if let Some(p) = p.as_str() {
                    add(
                        if p.starts_with('~') || Path::new(p).is_absolute() {
                            c::expand(p, paths)
                        } else {
                            cwd.join(p)
                        },
                        "custom",
                        "grok-skill-path",
                    )
                }
            }
        }
        "opencode" => {
            for b in [
                paths.home.join(".claude"),
                paths.home.join(".agents"),
                home.clone(),
            ] {
                add(b.join("skills"), "user", "skills-directory")
            }
            for p in &ctx.parents {
                for dir in [".claude", ".agents", ".opencode"] {
                    add(p.join(dir).join("skills"), "project", "skills-directory")
                }
            }
            if let Some(p) = paths.env.get("OPENCODE_CONFIG_DIR") {
                add(
                    c::expand(p, paths).join("skills"),
                    "custom",
                    "opencode-custom-directory",
                )
            }
            for p in c::list(&ctx.settings["skills"]["paths"]) {
                if let Some(p) = p.as_str() {
                    add(
                        if p.starts_with('~') || Path::new(p).is_absolute() {
                            c::expand(p, paths)
                        } else {
                            cwd.join(p)
                        },
                        "custom",
                        "opencode-skill-path",
                    )
                }
            }
        }
        "gemini" => {
            for (b, s) in [
                (home.clone(), "user"),
                (paths.home.join(".agents"), "user"),
                (cwd.join(".gemini"), "project"),
                (cwd.join(".agents"), "project"),
            ] {
                add(b.join("skills"), s, "skills-directory")
            }
        }
        "agy" => {
            add(home.join("config/skills"), "user", "agy-skills");
            add(cwd.join(".agent/skills"), "project", "agy-legacy-skills");
            add(cwd.join(".agents/skills"), "project", "agy-skills")
        }
        _ => {}
    }
    for plugin in ctx.plugins.clone() {
        let declared = plugin
            .manifest
            .get("skills")
            .cloned()
            .unwrap_or(json!("./skills"));
        let rels = declared
            .as_str()
            .map(|s| vec![json!(s)])
            .unwrap_or_else(|| c::list(&declared));
        for rel in rels {
            if let Some(rel) = rel.as_str() {
                let p = c::resolved(&plugin.root.join(rel));
                if !p.starts_with(&plugin.root) {
                    ctx.errors
                        .push(json!({"source":plugin.source,"code":"plugin_path_outside_root"}));
                    continue;
                }
                roots.push((
                    p,
                    plugin.scope.clone(),
                    plugin.source.clone(),
                    Some(plugin.clone()),
                ))
            }
        }
    }
    if matches!(h.as_str(), "claude" | "grok" | "gemini") {
        roots.sort_by_key(|r| r.3.is_none())
    }
    roots
}
fn skill_files(root: &Path, errors: &mut Vec<Value>, source: &str) -> Vec<PathBuf> {
    let mut pending = VecDeque::from([(root.to_path_buf(), 0)]);
    let mut seen = HashSet::new();
    let mut found = vec![];
    while !pending.is_empty() && seen.len() < 2000 && found.len() < 500 {
        let Some((p, depth)) = pending.pop_front() else {
            break;
        };
        let real = match std::fs::canonicalize(&p) {
            Ok(real) => real,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                continue;
            }
            Err(_) => {
                errors.push(json!({"source":source,"code":"skill_directory_unreadable"}));
                continue;
            }
        };
        if !p.is_dir() {
            continue;
        }
        if !seen.insert(real) {
            continue;
        }
        if p.join("SKILL.md").is_file() {
            found.push(p.join("SKILL.md"));
            continue;
        }
        if depth < 5 {
            match std::fs::read_dir(&p) {
                Ok(iter) => {
                    let mut dirs: Vec<_> = iter
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|p| {
                            p.is_dir()
                                && (p
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .is_some_and(|s| !s.starts_with('.') || s == ".system"))
                        })
                        .collect();
                    dirs.sort();
                    pending.extend(dirs.into_iter().map(|p| (p, depth + 1)));
                }
                Err(_) => errors.push(json!({"source":source,"code":"skill_directory_unreadable"})),
            }
        }
    }
    if !pending.is_empty() {
        errors.push(json!({"source":source,"code":"skill_scan_truncated"}))
    }
    found
}
fn read_prefix(path: &Path, limit: usize) -> Option<String> {
    use std::{io::Read, os::unix::fs::OpenOptionsExt};
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut reader = std::io::BufReader::new(file);
    let mut out = String::new();
    let mut count = 0;
    let mut after_cr = false;
    while count < limit {
        let mut bytes = [0u8; 4];
        match reader.read(&mut bytes[..1]) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => return None,
        }
        let width = match bytes[0] {
            0..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return None,
        };
        reader.read_exact(&mut bytes[1..width]).ok()?;
        let text = std::str::from_utf8(&bytes[..width]).ok()?;
        if after_cr && text == "\n" {
            after_cr = false;
            continue;
        }
        after_cr = text == "\r";
        out.push_str(if after_cr { "\n" } else { text });
        count += 1;
    }
    Some(out)
}

fn frontmatter(path: &Path) -> Option<Value> {
    let raw = read_prefix(path, 16000)?;
    static FRONT: std::sync::LazyLock<std::result::Result<regex::Regex, regex::Error>> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?s)^---\n(.*?)\n---"));
    let re = FRONT.as_ref().ok()?;
    let Some(m) = re.captures(&raw) else {
        return Some(json!({}));
    };
    Some(crate::profile_yaml::parse(&m[1]))
}

pub(crate) fn skills(ctx: &mut Context, caps: &Value, paths: &Paths) -> Vec<Value> {
    let policy_pattern =
        regex::Regex::new(r"(?m)^\s+allow_implicit_invocation:\s*false\s*(?:#.*)?$").ok();
    let ovs = if ctx.harness == "codex" {
        overrides(
            &ctx.layers
                .iter()
                .filter(|l| l.scope == "user")
                .map(|l| l.data.clone())
                .collect::<Vec<_>>(),
        )
    } else {
        vec![]
    };
    let mut rows: Vec<Value> = vec![];
    for (root, scope, source, plugin) in skill_roots(ctx, paths) {
        for path in skill_files(&root, &mut ctx.errors, &source) {
            let real = c::resolved(&path);
            if real.to_str().is_none() {
                ctx.uncertain = true;
                return vec![];
            }
            let text = real.to_string_lossy().to_string();
            if let Some(old) = rows.iter_mut().find(|r| r["path"] == text) {
                if let Some(sources) = old["sources"].as_array_mut() {
                    if !sources.contains(&json!(source)) {
                        sources.push(source.clone().into());
                    }
                } else {
                    ctx.uncertain = true;
                }

                continue;
            }
            let Some(front) = frontmatter(&path) else {
                ctx.errors
                    .push(json!({"source":source,"code":"skill_unreadable"}));
                continue;
            };
            let fallback = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let mut name = front["name"]
                .as_str()
                .filter(|s| !s.is_empty() && valid_extension_name(s))
                .unwrap_or(fallback)
                .to_owned();
            if !valid_extension_name(&name) {
                continue;
            }
            let mut enabled = json!(true);
            for entry in &ovs {
                if entry.get("path").is_some() == entry.get("name").is_some() {
                    continue;
                }
                if ctx
                    .home
                    .as_ref()
                    .and_then(|home| skill_path(&entry["path"], home, paths))
                    .as_ref()
                    == Some(&real)
                    || entry["name"].as_str().is_some_and(|s| s.trim() == name)
                {
                    enabled = json!(entry["enabled"] != false)
                }
            }
            if matches!(ctx.harness.as_str(), "gemini" | "grok") {
                let settings = if ctx.harness == "grok" {
                    ctx.layers.first().map(|l| &l.data).unwrap_or(&Value::Null)
                } else {
                    &ctx.settings
                };
                let cfg = &settings["skills"];
                if cfg["enabled"] == false || c::list(&cfg["disabled"]).contains(&json!(name)) {
                    enabled = false.into()
                }
                for ignore in c::list(&cfg["ignore"]) {
                    if skill_path(&ignore, &ctx.project, paths).is_some_and(|p| real.starts_with(p))
                    {
                        enabled = false.into()
                    }
                }
            }
            if let Some(p) = &plugin {
                enabled = if p.enabled == false {
                    false.into()
                } else if p.enabled == true {
                    enabled
                } else {
                    Value::Null
                };
                if ctx.harness != "gemini" {
                    name = format!("{}:{name}", p.name)
                }
            }
            if ctx.harness == "codex"
                && path.components().any(|p| p.as_os_str() == ".system")
                && ctx.settings["skills"]["bundled"]["enabled"] == false
            {
                enabled = false.into()
            }
            let mut automatic = front["disable-model-invocation"] != true
                && front["disable-model-invocation"] != "true"
                && front["disable-model-invocation"].as_f64() != Some(1.0);
            if ctx.harness == "codex" {
                if ctx.settings["skills"]["include_instructions"] == false {
                    automatic = false
                }
                if let Some(policy) = path
                    .parent()
                    .map(|p| p.join("agents/openai.yaml"))
                    .filter(|p| p.is_file())
                {
                    match read_prefix(&policy, 16000) {
                        Some(s) => {
                            if policy_pattern.as_ref().is_some_and(|re| re.is_match(&s)) {
                                automatic = false
                            }
                        }
                        None => ctx
                            .errors
                            .push(json!({"source":source,"code":"skill_policy_unreadable"})),
                    }
                }
            }
            let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
            let mut row = json!({"id":&hash[..24],"name":name,"path":text,"scope":scope,"source":source,"sources":[source],"enabled":enabled,"configuredEnabled":enabled,"effectiveNow":null,"runtimeEnabled":null,"status":c::status(&enabled),"automaticInvocation":automatic,"toggleable":caps["skills"]["supported"]==true&&plugin.is_none()});
            for (k, v) in c::object(&mcp_descriptions::metadata("", &front["description"])) {
                row[k] = v
            }
            if let Some(p) = &plugin {
                row["plugin"] = p.key.clone().into()
            }
            rows.push(row)
        }
    }
    if matches!(
        ctx.harness.as_str(),
        "claude" | "grok" | "opencode" | "gemini"
    ) {
        let mut winners = std::collections::HashMap::new();
        for i in 0..rows.len() {
            let name = rows[i]["name"].as_str().unwrap_or_default().to_owned();
            if let Some(old) = winners.insert(name, i) {
                let old: &mut Value = &mut rows[old];
                old["status"] = "shadowed".into();
                old["enabled"] = false.into();
                old["configuredEnabled"] = false.into();
                old["toggleable"] = false.into()
            }
        }
    }
    rows.sort_by_key(|r| {
        (
            r["name"]
                .as_str()
                .unwrap_or_default()
                .case_fold()
                .collect::<String>(),
            r["path"].as_str().unwrap_or_default().to_owned(),
        )
    });
    rows
}
pub fn inventory(
    registry: &Value,
    h: &str,
    alias: &str,
    cwd: &Path,
    paths: &Paths,
) -> Result<Value> {
    let mut ctx = c::configuration(registry, h, alias, cwd, paths)?;
    let mut caps = launch_capabilities(h);
    if h == "claude"
        && let Some(home) = &ctx.home
        && let Err(e) = claude_sources(home, cwd, alias, paths)
    {
        caps["mcps"] = json!({"status":"unsupported","supported":false,"reason":e.to_string()});
    }
    let base = c::from_context(&mut ctx, paths);
    let mut skills = skills(&mut ctx, &caps, paths);
    if ctx.uncertain {
        return Err(Fault::Uncertain("nonutf8 inventory path".into()));
    }
    if !ctx.errors.is_empty() {
        for kind in ["skills", "mcps"] {
            caps[kind] = json!({"status":"unsupported","supported":false,"reason":"Inventario incompleto: hay configuración o extensiones ilegibles."})
        }
        for row in &mut skills {
            row["toggleable"] = false.into()
        }
    }
    let mcps: Vec<_> = c::list(&base["mcps"])
        .into_iter()
        .map(|mut r| {
            let name = r["name"].as_str().unwrap_or_default().to_owned();
            r["id"] = name.clone().into();
            r["configuredEnabled"] = r["enabled"].clone();
            r["effectiveNow"] = Value::Null;
            r["toggleable"] = (caps["mcps"]["supported"] == true
                && !truthy(&r["plugin"])
                && valid_extension_name(&name)
                && !(h == "codex" && name.contains('.')))
            .into();
            r
        })
        .collect();
    let status = if !ctx.errors.is_empty() {
        "incomplete"
    } else if !skills.is_empty() && base["status"] == "empty" {
        "configured"
    } else {
        base["status"]
            .as_str()
            .ok_or_else(|| Fault::Uncertain("inventory status shape".into()))?
    };
    Ok(
        json!({"harness":h,"account":alias,"skills":skills,"mcps":mcps,"status":status,"confidence":base["confidence"],"limitations":ctx.limitations,"errors":ctx.errors,"capabilities":caps,"provenance":"configuration_files","effectiveNow":null,"runtimeEnabled":null,"note":"Inventario de archivos; no confirma las extensiones cargadas por un proceso vivo."}),
    )
}
pub fn launch_draft(profile: &Value) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for k in [
        "harness",
        "motor",
        "routeId",
        "model",
        "effort",
        "harnessAccount",
        "motorAccount",
    ] {
        out.insert(k.into(), profile.get(k).cloned().unwrap_or(json!("")));
    }
    out.insert(
        "agent".into(),
        profile.get("harness").cloned().unwrap_or(json!("codex")),
    );
    out.insert(
        "profileId".into(),
        profile
            .get("id")
            .cloned()
            .ok_or_else(|| Fault::Uncertain("profile id missing".into()))?,
    );
    if !truthy(&out["routeId"]) {
        out.shift_remove("routeId");
    }
    Ok(out.into())
}
fn toml_value(v: &Value) -> Result<String> {
    if let Some(b) = v.as_bool() {
        return Ok(b.to_string());
    }
    if v.is_string() {
        return response_dumps_unicode(v).map_err(Fault::Uncertain);
    }
    Err(invalid("override de skill inválido"))
}
pub fn launch_args(
    profile: &Value,
    registry: &Value,
    cwd: &Path,
    runtime_dir: &Path,
    dry_run: bool,
    paths: &Paths,
) -> Result<Vec<String>> {
    let h = profile["harness"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("codex");
    let skills = c::object(&profile["skills"]);
    let mcps = c::object(&profile["mcps"]);
    let caps = launch_capabilities(h);
    if !skills.is_empty() && caps["skills"]["supported"] != true {
        return Err(invalid(
            "selección individual de skills no soportada para este CLI",
        ));
    }
    if !mcps.is_empty() && caps["mcps"]["supported"] != true {
        return Err(invalid("selección de MCPs no soportada para este CLI"));
    }
    if skills.is_empty() && mcps.is_empty() {
        return Ok(vec![]);
    }
    let alias = profile["harnessAccount"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("main");
    let inv = inventory(registry, h, alias, cwd, paths)?;
    for (kind, selected) in [("skills", &skills), ("mcps", &mcps)] {
        if !selected.is_empty() && inv["capabilities"][kind]["supported"] != true {
            return Err(invalid(
                inv["capabilities"][kind]["reason"]
                    .as_str()
                    .unwrap_or("inventario inválido"),
            ));
        }
    }
    let skill_rows = c::list(&inv["skills"]);
    let mcp_rows = c::list(&inv["mcps"]);
    for (kind, selected, rows, key, message) in [
        (
            "skills",
            &skills,
            &skill_rows,
            "id",
            "skill no disponible en el inventario de esta cuenta y carpeta",
        ),
        (
            "mcps",
            &mcps,
            &mcp_rows,
            "name",
            "MCP no disponible en el inventario de esta cuenta y carpeta",
        ),
    ] {
        let _ = kind;
        for id in selected.keys() {
            if !rows.iter().any(|r| r[key] == *id) {
                return Err(invalid(message));
            }
        }
    }
    if skills
        .values()
        .chain(mcps.values())
        .any(|v| !v.is_boolean())
    {
        return Err(invalid("estado de extensión inválido"));
    }
    for (selected, rows, key) in [(&skills, &skill_rows, "id"), (&mcps, &mcp_rows, "name")] {
        if selected.keys().any(|id| {
            rows.iter()
                .find(|r| r[key] == *id)
                .is_none_or(|r| !truthy(&r["toggleable"]))
        }) {
            return Err(invalid(
                "selección individual de esta extensión no soportada",
            ));
        }
    }
    let home = accounts::account_home(
        registry,
        h,
        &json!(alias),
        &accounts::Paths::new(&paths.home, &paths.cwd),
    )
    .map_err(|e| Fault::Invalid(e.to_string()))?;
    let mut args = vec![];
    if h == "codex" {
        if !skills.is_empty() {
            let mut entries = overrides(&codex_configs(&home)?);
            if entries.iter().any(|e| {
                c::object(e)
                    .keys()
                    .any(|k| !matches!(k.as_str(), "path" | "name" | "enabled"))
                    || !e["enabled"].is_boolean()
            }) {
                return Err(invalid(
                    "override de skill existente no soportado; no se puede conservar",
                ));
            }
            let changed: Vec<_> = skills
                .iter()
                .map(|(id, v)| {
                    let row = skill_rows.iter().find(|r| r["id"] == *id).ok_or_else(|| {
                        invalid("skill no disponible en el inventario de esta cuenta y carpeta")
                    })?;
                    let path = row["path"]
                        .as_str()
                        .ok_or_else(|| Fault::Uncertain("skill path shape".into()))?;
                    Ok((path.to_owned(), v.clone()))
                })
                .collect::<Result<Vec<_>>>()?;
            entries.retain(|e| {
                !changed.iter().any(|(p, _)| {
                    skill_path(&e["path"], &home, paths).is_some_and(|ep| ep == Path::new(p))
                })
            });
            entries.extend(
                changed
                    .into_iter()
                    .map(|(path, enabled)| json!({"path":path,"enabled":enabled})),
            );
            let mut encoded = vec![];
            for e in entries {
                let mut fields = vec![];
                for key in ["path", "name", "enabled"] {
                    if let Some(v) = e.get(key) {
                        fields.push(format!("{key}={}", toml_value(v)?))
                    }
                }
                encoded.push(format!("{{{}}}", fields.join(",")))
            }
            args.extend([
                "-c".into(),
                format!("skills.config=[{}]", encoded.join(",")),
            ])
        }
        let mut names: Vec<_> = mcps.keys().collect();
        names.sort();
        for name in names {
            args.extend([
                "-c".into(),
                format!("mcp_servers.{name}.enabled={}", toml_value(&mcps[name])?),
            ])
        }
    } else if h == "claude" && !mcps.is_empty() {
        let mut servers = claude_sources(&home, cwd, alias, paths)?;
        servers.retain(|name, _| {
            mcps.get(name).cloned().unwrap_or_else(|| {
                mcp_rows
                    .iter()
                    .find(|r| r["name"] == *name)
                    .map(|r| r["enabled"].clone())
                    .unwrap_or(json!(true))
            }) == true
        });
        let serialized =
            response_dumps_unicode(&Value::Object(servers.clone())).map_err(Fault::Uncertain)?;
        if serialized.contains("${")
            || servers.values().any(|s| {
                s["cwd"]
                    .as_str()
                    .is_some_and(|p| !p.is_empty() && !Path::new(p).is_absolute())
            })
        {
            return Err(invalid(
                "MCP con expansión o cwd relativo: selección aislada no soportada",
            ));
        }
        if dry_run {
            return Ok(vec![
                "--strict-mcp-config".into(),
                "--mcp-config".into(),
                "<private-launch-config>".into(),
            ]);
        }
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(runtime_dir)
            .map_err(Fault::Io)?;
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| Fault::Uncertain(e.to_string()))?;
        let path = runtime_dir.join(format!(
            "profile-mcp-{}.json",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(Fault::Io)?;
        use std::io::Write;
        file.write_all(
            response_dumps_unicode(&json!({"mcpServers":servers}))
                .map_err(Fault::Uncertain)?
                .as_bytes(),
        )
        .map_err(Fault::Io)?;
        args.extend([
            "--strict-mcp-config".into(),
            "--mcp-config".into(),
            path.to_str()
                .ok_or_else(|| Fault::Uncertain("nonutf8 launch path".into()))?
                .into(),
        ]);
    }
    Ok(args)
}
