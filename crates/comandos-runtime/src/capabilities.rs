//! Read-only extension declarations. All environment and home roots are explicit.
use crate::{accounts, extension_launch, mcp_descriptions};
use comandos_core::json::{truthy, workspace_loads};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use unicode_casefold::UnicodeCaseFold;
#[derive(Debug)]
pub enum Fault {
    Invalid(String),
    Uncertain(String),
    Io(std::io::Error),
}
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(s) | Self::Uncertain(s) => f.write_str(s),
            Self::Io(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for Fault {}
pub type Result<T> = std::result::Result<T, Fault>;
pub struct Paths {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub admin_skills: PathBuf,
}
impl Paths {
    pub fn new(home: &Path, cwd: &Path) -> Self {
        Self {
            home: home.into(),
            cwd: cwd.into(),
            env: BTreeMap::new(),
            admin_skills: "/etc/codex/skills".into(),
        }
    }
}
pub(crate) fn object(v: &Value) -> serde_json::Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}
pub(crate) fn list(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}
pub(crate) fn error(errors: &mut Vec<Value>, source: &str, code: &str) {
    let v = json!({"source":source,"code":code});
    if !errors.contains(&v) {
        errors.push(v)
    }
}
pub(crate) fn resolved(path: &Path) -> PathBuf {
    if let Ok(p) = std::fs::canonicalize(path) {
        return p;
    }
    let mut out = if let Some(parent) = path.parent().filter(|p| *p != path) {
        if let Some(name) = path.file_name() {
            resolved(parent).join(name)
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    };
    let mut normalized = PathBuf::new();
    for part in out.components() {
        match part {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            _ => normalized.push(part),
        }
    }
    out = normalized;
    out
}

pub(crate) fn expand(value: &str, paths: &Paths) -> PathBuf {
    let p = if value == "~" {
        paths.home.clone()
    } else if let Some(t) = value.strip_prefix("~/") {
        paths.home.join(t)
    } else {
        PathBuf::from(value)
    };
    resolved(&if p.is_absolute() {
        p
    } else {
        paths.cwd.join(p)
    })
}
pub(crate) fn jsonc(text: &str) -> Option<Value> {
    let re = regex::Regex::new(r#""(?:\\.|[^"\\])*"|//[^\n]*|/\*[\s\S]*?\*/"#).ok()?;
    let clean = re.replace_all(text, |c: &regex::Captures<'_>| {
        if c[0].starts_with('"') {
            c[0].to_owned()
        } else {
            " ".into()
        }
    });
    let re = regex::Regex::new(r#"("(?:\\.|[^"\\])*")|,\s*([}\]])"#).ok()?;
    let clean = re.replace_all(&clean, |c: &regex::Captures<'_>| {
        c.get(1)
            .or_else(|| c.get(2))
            .map_or("", |m| m.as_str())
            .to_owned()
    });
    workspace_loads(&clean).ok()
}
/// Python 3.10 fallback projection: public extension settings only.
fn configuration_fields(data: &Value) -> Value {
    fn keep(v: &Value, keys: &[&str]) -> Value {
        Value::Object(
            object(v)
                .into_iter()
                .filter(|(k, _)| keys.contains(&k.as_str()))
                .collect(),
        )
    }
    let mut out = serde_json::Map::new();
    if let Some(v) = data.get("skills") {
        let mut skills = v.clone();
        if v.is_object() {
            skills = keep(
                v,
                &[
                    "config",
                    "paths",
                    "ignore",
                    "disabled",
                    "enabled",
                    "bundled",
                    "include_instructions",
                ],
            );
            if let Some(entries) = v["config"].as_array() {
                skills["config"] = entries
                    .iter()
                    .filter(|e| e.is_object())
                    .map(|e| {
                        let mut x = keep(e, &["path", "name", "enabled"]);
                        if object(e)
                            .keys()
                            .any(|k| !matches!(k.as_str(), "path" | "name" | "enabled"))
                        {
                            x["_unsupportedKeys"] = true.into()
                        }
                        x
                    })
                    .collect::<Vec<_>>()
                    .into();
            }
        }
        out.insert("skills".into(), skills);
    }
    if let Some(v) = data.get("features") {
        out.insert(
            "features".into(),
            Value::Object(
                object(v)
                    .into_iter()
                    .filter(|(_, v)| v.is_boolean())
                    .collect(),
            ),
        );
    }
    if let Some(v) = data.get("compat") {
        out.insert(
            "compat".into(),
            Value::Object(
                object(v)
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            k,
                            Value::Object(
                                object(&v)
                                    .into_iter()
                                    .filter(|(_, v)| v.is_boolean())
                                    .collect(),
                            ),
                        )
                    })
                    .collect(),
            ),
        );
    }
    if let Some(v) = data.get("plugins") {
        let mut plugins = serde_json::Map::new();
        for (name, spec) in object(v) {
            if matches!(name.as_str(), "paths" | "disabled" | "enabled") && spec.is_array() {
                plugins.insert(name, spec);
            } else if spec.is_object() {
                let mut x = keep(&spec, &["enabled"]);
                if spec.get("mcp_servers").is_some() {
                    x["mcp_servers"] = Value::Object(
                        object(&spec["mcp_servers"])
                            .into_iter()
                            .filter(|(_, s)| s.is_object() && s.get("enabled").is_some())
                            .map(|(n, s)| {
                                (
                                    n,
                                    Value::Object(serde_json::Map::from_iter([(
                                        "enabled".into(),
                                        s["enabled"].clone(),
                                    )])),
                                )
                            })
                            .collect(),
                    );
                }
                plugins.insert(name, x);
            }
        }
        out.insert("plugins".into(), plugins.into());
    }
    if let Some(v) = data.get("marketplaces") {
        out.insert(
            "marketplaces".into(),
            Value::Object(
                object(v)
                    .into_iter()
                    .filter(|(_, s)| s.is_object() && s["source_type"] == "local")
                    .map(|(n, s)| {
                        (
                            n,
                            Value::Object(serde_json::Map::from_iter([
                                ("source_type".into(), "local".into()),
                                ("source".into(), s["source"].clone()),
                            ])),
                        )
                    })
                    .collect(),
            ),
        );
    }
    if let Some(v) = data.get("projects") {
        out.insert(
            "projects".into(),
            Value::Object(
                object(v)
                    .into_iter()
                    .filter(|(_, s)| s.is_object())
                    .map(|(n, s)| {
                        (
                            n,
                            Value::Object(serde_json::Map::from_iter([(
                                "trust_level".into(),
                                s["trust_level"].clone(),
                            )])),
                        )
                    })
                    .collect(),
            ),
        );
    }
    if let Some(v) = data.get("mcp_servers") {
        out.insert(
            "mcp_servers".into(),
            Value::Object(
                object(v)
                    .into_iter()
                    .filter(|(_, s)| s.is_object())
                    .map(|(n, s)| (n, keep(&s, &["enabled", "disabled", "description"])))
                    .collect(),
            ),
        );
    }
    out.into()
}

pub fn read_config(path: &Path, errors: &mut Vec<Value>, source: &str) -> Value {
    let result = (|| {
        use std::{io::Read, os::unix::fs::OpenOptionsExt};
        let meta = match std::fs::metadata(path) {
            Ok(meta) => meta,
            Err(e)
                if e.raw_os_error()
                    .is_some_and(|n| [2, 20, 9, 40].contains(&n)) =>
            {
                return Some(json!({}));
            }
            Err(_) => return None,
        };
        if !meta.is_file() || meta.len() > 2 * 1024 * 1024 {
            return None;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NONBLOCK)
            .open(path)
            .ok()?;
        let opened = file.metadata().ok()?;
        if !opened.is_file() || opened.len() > 2 * 1024 * 1024 {
            return None;
        }
        let mut bytes = vec![];
        file.take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > 2 * 1024 * 1024 {
            return None;
        }
        let text = String::from_utf8(bytes).ok()?;
        let v = match path.extension().and_then(|s| s.to_str()) {
            Some("toml") => extension_launch::parse_inventory_toml(&text)
                .ok()
                .flatten()
                .map(|v| configuration_fields(&v)),
            Some("jsonc") => jsonc(&text),
            _ => workspace_loads(&text).ok(),
        }?;
        v.is_object().then_some(v)
    })();
    result.unwrap_or_else(|| {
        error(errors, source, "configuration_unreadable");
        json!({})
    })
}
pub fn provider_home(
    registry: &Value,
    harness: &str,
    alias: &str,
    paths: &Paths,
) -> Result<Option<PathBuf>> {
    let spec = &registry["harnesses"][harness];
    if !spec.is_object() {
        return Err(Fault::Invalid("CLI no registrado".into()));
    }
    let alias =
        accounts::validate_alias(&json!(alias)).map_err(|e| Fault::Invalid(e.to_string()))?;
    if truthy(&spec["capabilities"]["accounts"]) {
        let home = accounts::account_home(
            registry,
            harness,
            &json!(alias),
            &accounts::Paths::new(&paths.home, &paths.cwd),
        )
        .map_err(|e| Fault::Invalid(e.to_string()))?;
        if alias != "main" && !home.is_dir() {
            return Err(Fault::Invalid("cuenta no encontrada".into()));
        }
        return Ok(Some(home));
    }
    if alias != "main" {
        return Err(Fault::Invalid("cuentas no soportadas para este CLI".into()));
    }
    if let Some(s) = spec["defaultHome"].as_str().filter(|s| !s.is_empty()) {
        return Ok(Some(expand(s, paths)));
    }
    Ok(match harness {
        "opencode" => Some(
            paths
                .env
                .get("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| paths.home.join(".config"))
                .join("opencode"),
        ),
        "gemini" | "agy" => Some(paths.home.join(".gemini")),
        _ => None,
    })
}
pub fn claude_user_json(home: &Path, alias: &str, paths: &Paths) -> PathBuf {
    let inner = home.join(".claude.json");
    if inner.exists() || alias != "main" || home != resolved(&paths.home.join(".claude")) {
        inner
    } else {
        paths.home.join(".claude.json")
    }
}
#[derive(Clone)]
pub struct Layer {
    pub path: PathBuf,
    pub scope: String,
    pub source: String,
    pub data: Value,
}
#[derive(Clone)]
pub struct Plugin {
    pub root: PathBuf,
    pub key: String,
    pub name: String,
    pub scope: String,
    pub enabled: Value,
    pub source: String,
    pub manifest: Value,
}
pub struct Context {
    pub harness: String,
    pub alias: String,
    pub home: Option<PathBuf>,
    pub project: PathBuf,
    pub parents: Vec<PathBuf>,
    pub layers: Vec<Layer>,
    pub settings: Value,
    pub errors: Vec<Value>,
    pub limitations: Vec<Value>,
    pub plugins: Vec<Plugin>,
    pub user_data: Value,
    pub(crate) uncertain: bool,
}
fn merge_value(left: &mut Value, right: &Value) {
    if let (Some(l), Some(r)) = (left.as_object_mut(), right.as_object()) {
        for (k, v) in r {
            if v.is_object()
                && let Some(old) = l.get_mut(k).filter(|old| old.is_object())
            {
                merge_value(old, v)
            } else {
                l.insert(k.clone(), v.clone());
            }
        }
    }
}
fn add_layer(ctx: &mut Context, path: PathBuf, scope: &str, source: &str) {
    let path = resolved(&path);
    if path.to_str().is_none() {
        ctx.uncertain = true;
        return;
    }
    if ctx.layers.iter().any(|l| l.path == path) {
        return;
    }
    let data = read_config(&path, &mut ctx.errors, source);
    ctx.layers.push(Layer {
        path,
        scope: scope.into(),
        source: source.into(),
        data,
    });
}
pub fn configuration(
    registry: &Value,
    harness: &str,
    alias: &str,
    cwd: &Path,
    paths: &Paths,
) -> Result<Context> {
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err(Fault::Invalid("cwd inválido".into()));
    }
    let home = provider_home(registry, harness, alias, paths)?;
    let project = resolved(cwd);
    if project.to_str().is_none() || home.as_ref().is_some_and(|p| p.to_str().is_none()) {
        return Err(Fault::Uncertain("nonutf8 inventory path".into()));
    }
    let mut parents = vec![];
    let mut current = project.clone();
    loop {
        parents.push(current.clone());
        if current.join(".git").exists() {
            break;
        }
        let Some(p) = current.parent() else { break };
        if p == current {
            break;
        }
        current = p.into();
    }
    parents.reverse();
    let mut ctx = Context {
        harness: harness.into(),
        alias: alias.into(),
        home: home.clone(),
        project: project.clone(),
        parents: parents.clone(),
        layers: vec![],
        settings: json!({}),
        errors: vec![],
        limitations: vec![],
        plugins: vec![],
        user_data: json!({}),
        uncertain: false,
    };
    let mut limitation = "";
    match harness {
        "codex" | "grok" => {
            let h = home
                .as_ref()
                .ok_or_else(|| Fault::Uncertain("missing provider home".into()))?;
            add_layer(
                &mut ctx,
                h.join("config.toml"),
                "user",
                &format!("{harness}-user"),
            );
            let chain = if harness == "codex" {
                parents
            } else {
                let Some(first) = parents.first() else {
                    return Err(Fault::Uncertain("empty parent chain".into()));
                };
                let mut p = vec![first.clone()];
                if *first != project {
                    p.push(project.clone())
                }
                p
            };
            for p in chain {
                add_layer(
                    &mut ctx,
                    p.join(format!(".{harness}/config.toml")),
                    "project",
                    &format!("{harness}-project"),
                );
            }
            limitation = "La confianza del proyecto, políticas administradas y overrides del proceso no se verifican en este inventario.";
        }
        "claude" => {
            let h = home
                .as_ref()
                .ok_or_else(|| Fault::Uncertain("missing provider home".into()))?;
            add_layer(
                &mut ctx,
                claude_user_json(h, alias, paths),
                "user",
                "claude-user",
            );
            ctx.user_data = ctx
                .layers
                .first()
                .map(|l| l.data.clone())
                .unwrap_or_else(|| json!({}));
            add_layer(&mut ctx, project.join(".mcp.json"), "project", "mcp-json");
            for (p, s) in [
                (h.join("settings.json"), "user"),
                (project.join(".claude/settings.json"), "project"),
                (project.join(".claude/settings.local.json"), "local"),
            ] {
                let data = read_config(&p, &mut ctx.errors, &format!("claude-{s}-settings"));
                merge_value(&mut ctx.settings, &data);
            }
            limitation = "No incluye conectores de claude.ai ni políticas administradas remotas; la aprobación del proyecto y los flags del proceso pueden limitar la carga.";
        }
        "opencode" => {
            let h = home
                .as_ref()
                .ok_or_else(|| Fault::Uncertain("missing provider home".into()))?;
            for suffix in ["json", "jsonc"] {
                add_layer(
                    &mut ctx,
                    h.join(format!("opencode.{suffix}")),
                    "user",
                    "opencode-user",
                )
            }
            if let Some(p) = paths.env.get("OPENCODE_CONFIG") {
                add_layer(&mut ctx, expand(p, paths), "custom", "opencode-custom")
            }
            for p in &parents {
                for suffix in ["json", "jsonc"] {
                    add_layer(
                        &mut ctx,
                        p.join(format!("opencode.{suffix}")),
                        "project",
                        "opencode-project",
                    )
                }
            }
            for p in &parents {
                for suffix in ["json", "jsonc"] {
                    add_layer(
                        &mut ctx,
                        p.join(format!(".opencode/opencode.{suffix}")),
                        "project",
                        "opencode-directory",
                    )
                }
            }
            if let Some(p) = paths.env.get("OPENCODE_CONFIG_DIR") {
                for suffix in ["json", "jsonc"] {
                    add_layer(
                        &mut ctx,
                        expand(p, paths).join(format!("opencode.{suffix}")),
                        "custom",
                        "opencode-custom-directory",
                    )
                }
            }
            limitation = "No ejecuta plugins JS ni consulta configuración remota; overrides del proceso y permisos por agente no se verifican.";
        }
        "gemini" => {
            add_layer(
                &mut ctx,
                home.as_ref()
                    .ok_or_else(|| Fault::Uncertain("missing provider home".into()))?
                    .join("settings.json"),
                "user",
                "gemini-user",
            );
            add_layer(
                &mut ctx,
                project.join(".gemini/settings.json"),
                "project",
                "gemini-project",
            );
            limitation = "No verifica configuración administrada, flags del proceso ni skills integradas en el binario.";
        }
        "agy" => {
            add_layer(
                &mut ctx,
                home.as_ref()
                    .ok_or_else(|| Fault::Uncertain("missing provider home".into()))?
                    .join("config/mcp_config.json"),
                "user",
                "agy-user",
            );
            add_layer(
                &mut ctx,
                project.join(".agents/mcp_config.json"),
                "project",
                "agy-project",
            );
            limitation = "Rutas de Antigravity CLI documentadas; no verifica migración desde versiones antiguas ni plugins del IDE.";
        }
        "acp" => {
            limitation = "ACP depende del agente y cuenta de la ruta seleccionada; no tiene un inventario de extensiones propio."
        }
        "shell" => {}
        _ => limitation = "Detección de extensiones no implementada para este CLI registrado.",
    }
    if harness != "claude" {
        for layer in &ctx.layers {
            merge_value(&mut ctx.settings, &layer.data)
        }
    }
    if !limitation.is_empty() {
        ctx.limitations.push(limitation.into())
    }
    if harness == "codex"
        && ctx
            .layers
            .iter()
            .any(|l| l.scope == "project" && truthy(&l.data["skills"]["config"]))
    {
        ctx.limitations.push("Codex ignora skills.config del proyecto; las reglas de skills se leen de la cuenta y de flags de sesión.".into())
    }
    let key = server_key(harness);
    for l in &ctx.layers {
        if l.data.get(key).is_some_and(invalid_servers) {
            ctx.errors
                .push(json!({"source":l.source,"code":"mcp_definition_invalid"}));
        }
    }
    discover_plugins(&mut ctx, paths);
    if ctx.uncertain {
        return Err(Fault::Uncertain("nonutf8 inventory path".into()));
    }
    Ok(ctx)
}
fn server_key(h: &str) -> &str {
    match h {
        "codex" | "grok" => "mcp_servers",
        "opencode" => "mcp",
        _ => "mcpServers",
    }
}
fn invalid_servers(v: &Value) -> bool {
    !v.is_object()
        || object(v).values().any(|s| {
            !s.is_object()
                || ["enabled", "disabled"]
                    .iter()
                    .any(|k| s.get(k).is_some_and(|v| !v.is_boolean()))
        })
}
pub(crate) fn children(path: &Path) -> Vec<PathBuf> {
    let mut out: Vec<_> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(|r| r.ok().map(|e| e.path()))
        .collect();
    out.sort();
    out
}
fn manifest(root: &Path, ctx: &mut Context) -> Value {
    for rel in [
        ".codex-plugin/plugin.json",
        ".claude-plugin/plugin.json",
        ".grok-plugin/plugin.json",
        "plugin.json",
        "gemini-extension.json",
    ] {
        let p = root.join(rel);
        if p.is_file() {
            return read_config(&p, &mut ctx.errors, &format!("{}-plugin", ctx.harness));
        }
    }
    json!({})
}
fn safe_name(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= 160 && !s.chars().any(|c| c < ' ')
}
fn add_plugin(
    ctx: &mut Context,
    root: PathBuf,
    key: String,
    scope: &str,
    enabled: Value,
    source: &str,
    paths: &Paths,
) {
    let root = expand(&root.to_string_lossy(), paths);
    if root.to_str().is_none() {
        ctx.uncertain = true;
        return;
    }
    if !root.is_dir() {
        ctx.errors
            .push(json!({"source":source,"code":"plugin_files_missing"}));
        return;
    }
    let manifest = manifest(&root, ctx);
    let name = if truthy(&manifest["name"]) {
        let Some(name) = manifest["name"].as_str() else {
            return;
        };
        name
    } else {
        key.split('@').next().unwrap_or("")
    };
    if !safe_name(name) {
        return;
    }
    let p = Plugin {
        root: root.clone(),
        key: key.clone(),
        name: name.into(),
        scope: scope.into(),
        enabled,
        source: source.into(),
        manifest,
    };
    if let Some(old) = ctx.plugins.iter_mut().find(|p| p.root == root) {
        *old = p
    } else {
        ctx.plugins.push(p)
    }
}
fn version_key(p: &Path) -> (bool, Vec<(u8, usize, String)>) {
    let name = p.file_name().unwrap_or_default().to_string_lossy();
    static VERSION: std::sync::LazyLock<std::result::Result<regex::Regex, regex::Error>> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\d+|\D+"));
    let Ok(re) = VERSION.as_ref() else {
        return (name == "default", vec![(0, 0, name.into_owned())]);
    };
    (
        name == "default",
        re.find_iter(&name)
            .map(|m| {
                let s = m.as_str();
                if s.bytes().all(|b| b.is_ascii_digit()) {
                    let n = s.trim_start_matches('0');
                    (1, n.len(), n.into())
                } else {
                    (0, 0, s.into())
                }
            })
            .collect(),
    )
}

fn grok_add(
    ctx: &mut Context,
    root: PathBuf,
    scope: &str,
    source: &str,
    default: bool,
    paths: &Paths,
) {
    use sha2::{Digest, Sha256};
    let root = expand(&root.to_string_lossy(), paths);
    let m = manifest(&root, ctx);
    let name = m["name"]
        .as_str()
        .unwrap_or_else(|| root.file_name().and_then(|s| s.to_str()).unwrap_or(""));
    let hash = format!("{:x}", Sha256::digest(root.to_string_lossy().as_bytes()));
    let id = format!("{scope}/{}/{name}", &hash[..8]);
    let cfg = &ctx.settings["plugins"];
    let disabled = list(&cfg["disabled"]);
    let enabled = list(&cfg["enabled"]);
    let state = if disabled.contains(&json!(name)) || disabled.contains(&json!(id)) {
        false
    } else if enabled.contains(&json!(name)) || enabled.contains(&json!(id)) {
        true
    } else {
        default
    };
    add_plugin(ctx, root, id, scope, state.into(), source, paths)
}
fn discover_plugins(ctx: &mut Context, paths: &Paths) {
    let Some(home) = ctx.home.clone() else { return };
    match ctx.harness.as_str() {
        "codex" => {
            let cfg = object(&ctx.settings["plugins"]);
            let cache = home.join("plugins/cache");
            let mut keys: std::collections::BTreeSet<String> = cfg.keys().cloned().collect();
            for market in children(&cache) {
                for p in children(&market) {
                    if p.is_dir() {
                        keys.insert(format!(
                            "{}@{}",
                            p.file_name().unwrap_or_default().to_string_lossy(),
                            market.file_name().unwrap_or_default().to_string_lossy()
                        ));
                    }
                }
            }
            let Ok(re) = regex::Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,159}$") else {
                ctx.uncertain = true;
                return;
            };
            for key in keys.into_iter().take(500) {
                if !key.contains('@') || key.contains('/') || key.contains('\\') {
                    continue;
                }
                let Some((name, market)) = key.rsplit_once('@') else {
                    continue;
                };
                if !re.is_match(name) || !re.is_match(market) {
                    ctx.errors
                        .push(json!({"source":"codex-plugin","code":"plugin_identifier_invalid"}));
                    continue;
                }
                let mut versions: Vec<_> = children(&cache.join(market).join(name))
                    .into_iter()
                    .filter(|p| p.is_dir())
                    .collect();
                versions.sort_by_key(|p| version_key(p));
                if let Some(root) = versions.last() {
                    let state = cfg
                        .get(&key)
                        .and_then(|v| v.get("enabled"))
                        .filter(|v| v.is_boolean())
                        .cloned()
                        .unwrap_or(Value::Null);
                    add_plugin(ctx, root.clone(), key, "user", state, "codex-plugin", paths)
                } else if cfg.contains_key(&key) {
                    ctx.errors
                        .push(json!({"source":"codex-plugin","code":"plugin_files_missing"}));
                }
            }
        }
        "claude" => {
            let installed = read_config(
                &home.join("plugins/installed_plugins.json"),
                &mut ctx.errors,
                "claude-plugin-index",
            );
            for (key, records) in object(&installed["plugins"]) {
                let mut candidates: Vec<Value> = list(&records)
                    .into_iter()
                    .filter(|r| {
                        r["installPath"].is_string()
                            && (!matches!(r["scope"].as_str(), Some("project" | "local"))
                                || r["projectPath"] == ctx.project.to_string_lossy().as_ref())
                    })
                    .collect();
                candidates.sort_by_key(|r| match r["scope"].as_str().unwrap_or("user") {
                    "user" => 0,
                    "project" => 1,
                    "local" => 2,
                    "managed" => 3,
                    _ => -1,
                });
                if let Some(r) = candidates.last() {
                    let state = ctx.settings["enabledPlugins"][&key]
                        .as_bool()
                        .map(Value::Bool)
                        .unwrap_or(Value::Null);
                    add_plugin(
                        ctx,
                        r["installPath"].as_str().unwrap_or_default().into(),
                        key,
                        r["scope"].as_str().unwrap_or("user"),
                        state,
                        "claude-plugin",
                        paths,
                    )
                }
            }
        }
        "grok" => {
            let cfg = ctx.settings["plugins"].clone();
            let installed = read_config(
                &home.join("installed-plugins/registry.json"),
                &mut ctx.errors,
                "grok-plugin-index",
            );
            for record in object(&installed["repos"]).values() {
                if let Some(p) = record["path"].as_str() {
                    for spec in object(&record["plugins"]).values() {
                        grok_add(
                            ctx,
                            Path::new(p).join(spec["subdir"].as_str().unwrap_or("")),
                            "user",
                            "grok-installed-plugin",
                            false,
                            paths,
                        )
                    }
                }
            }
            for p in list(&cfg["paths"]) {
                if let Some(p) = p.as_str() {
                    grok_add(ctx, p.into(), "config", "grok-plugin-path", true, paths)
                }
            }
            let mut roots = vec![(home.join("plugins"), "user")];
            roots.extend(
                ctx.parents
                    .iter()
                    .map(|p| (p.join(".grok/plugins"), "project")),
            );
            for (parent, scope) in roots {
                for root in children(&parent).into_iter().take(500) {
                    if root.is_dir()
                        && (truthy(&manifest(&root, ctx))
                            || root.join("skills").is_dir()
                            || root.join(".mcp.json").is_file())
                    {
                        grok_add(ctx, root, scope, "grok-plugin", false, paths)
                    }
                }
            }
            if !ctx.plugins.is_empty() {
                ctx.limitations.push("Los plugins de Grok también requieren confianza; el estado configurado no confirma la carga del servidor.".into())
            }
        }
        "gemini" => {
            let activation = read_config(
                &home.join("extensions/extension-enablement.json"),
                &mut ctx.errors,
                "gemini-extension-enablement",
            );
            for root in children(&home.join("extensions"))
                .into_iter()
                .filter(|p| p.join("gemini-extension.json").is_file())
                .take(500)
            {
                let m = read_config(
                    &root.join("gemini-extension.json"),
                    &mut ctx.errors,
                    "gemini-extension",
                );
                let name = m["name"].as_str().unwrap_or_else(|| {
                    root.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default()
                });
                let mut state = true;
                for rule in list(&activation[name]["overrides"]) {
                    if let Some(rule) = rule.as_str() {
                        let pattern = rule.strip_prefix('!').unwrap_or(rule);
                        let expression =
                            format!("^{}$", regex::escape(pattern).replace("\\*", ".*"));
                        if regex::Regex::new(&expression).is_ok_and(|re| {
                            re.is_match(&format!(
                                "{}/",
                                ctx.project.to_string_lossy().trim_end_matches('/')
                            ))
                        }) {
                            state = !rule.starts_with('!')
                        }
                    }
                }
                add_plugin(
                    ctx,
                    root.clone(),
                    name.into(),
                    "user",
                    state.into(),
                    "gemini-extension",
                    paths,
                )
            }
        }
        _ => {}
    }
}
fn server_entries(
    v: &Value,
    h: &str,
    scope: &str,
    source: &str,
    disabled: &[Value],
    inherit: bool,
) -> Vec<Value> {
    let mut out = vec![];
    for (name, spec) in object(v) {
        if !safe_name(&name) || !spec.is_object() {
            continue;
        }
        let enabled = !disabled.contains(&json!(name))
            && spec["enabled"] != false
            && spec["disabled"] != true;
        let mut row = json!({"name":name,"provider":h,"scope":scope,"source":source,"sources":[source],"enabled":enabled,"status":if enabled{"configured"}else{"disabled"},"confidence":"configured","effectiveNow":null,"runtimeEnabled":null});
        for (k, v) in object(&mcp_descriptions::metadata(&name, &spec["description"])) {
            row[k] = v
        }
        if ["enabled", "disabled"]
            .iter()
            .any(|k| spec.get(k).is_some_and(|v| !v.is_boolean()))
        {
            row["enabled"] = Value::Null;
            row["status"] = "invalid".into()
        }
        if inherit {
            row["_inheritEnabled"] = (spec.get("enabled").is_none()
                && spec.get("disabled").is_none()
                && !disabled.contains(&json!(name)))
            .into()
        }
        out.push(row)
    }
    out
}
fn plugin_mcps(ctx: &mut Context) -> Vec<Value> {
    let mut out = vec![];
    for p in ctx.plugins.clone() {
        let data = read_config(&p.root.join(".mcp.json"), &mut ctx.errors, &p.source);
        let mut servers = object(data.get("mcpServers").unwrap_or(&data));
        let declared = &p.manifest["mcpServers"];
        if declared.is_object() {
            servers.extend(object(declared.get("mcpServers").unwrap_or(declared)))
        } else {
            let rels = declared
                .as_str()
                .map(|s| vec![json!(s)])
                .unwrap_or_else(|| list(declared));
            for rel in rels {
                if let Some(rel) = rel.as_str() {
                    let path = resolved(&p.root.join(rel));
                    if !path.starts_with(&p.root) {
                        ctx.errors
                            .push(json!({"source":p.source,"code":"plugin_path_outside_root"}));
                        continue;
                    }
                    let data = read_config(&path, &mut ctx.errors, &p.source);
                    servers.extend(object(data.get("mcpServers").unwrap_or(&data)))
                }
            }
        }
        let servers = Value::Object(servers);
        if invalid_servers(&servers) {
            ctx.errors
                .push(json!({"source":p.source,"code":"mcp_definition_invalid"}));
        }
        for mut row in server_entries(&servers, &ctx.harness, &p.scope, &p.source, &[], false) {
            let Some(name) = row["name"].as_str().map(str::to_owned) else {
                ctx.uncertain = true;
                continue;
            };
            row["name"] = format!("plugin:{}:{name}", p.key).into();
            row["serverName"] = name.clone().into();
            row["plugin"] = p.key.clone().into();
            row["pluginEnabled"] = p.enabled.clone();
            row["sourcePath"] = p.root.to_string_lossy().to_string().into();
            row["declarations"] = json!([{"source":p.source,"scope":p.scope,"path":p.root}]);
            let state = if ctx.settings["plugins"][&p.key]["mcp_servers"][&name]["enabled"] == false
            {
                json!(false)
            } else {
                p.enabled.clone()
            };
            row["enabled"] = if state == false {
                false.into()
            } else if state == true {
                row["enabled"].clone()
            } else {
                Value::Null
            };
            row["status"] = status(&row["enabled"]).into();
            out.push(row)
        }
    }
    out
}
pub(crate) fn status(v: &Value) -> &str {
    if *v == false {
        "disabled"
    } else if *v == true {
        "configured"
    } else {
        "installed"
    }
}
pub fn session_capabilities(
    registry: &Value,
    h: &str,
    alias: &str,
    cwd: &Path,
    paths: &Paths,
) -> Result<Value> {
    let mut ctx = configuration(registry, h, alias, cwd, paths)?;
    Ok(from_context(&mut ctx, paths))
}
pub(crate) fn from_context(ctx: &mut Context, paths: &Paths) -> Value {
    let h = ctx.harness.clone();
    let mut entries = vec![];
    if h == "grok" {
        let user = ctx
            .layers
            .first()
            .map(|l| l.data.clone())
            .unwrap_or(Value::Null);
        for (vendor, files) in [
            (
                "claude",
                vec![
                    paths.home.join(".claude.json"),
                    ctx.project.join(".mcp.json"),
                ],
            ),
            (
                "cursor",
                vec![
                    paths.home.join(".cursor/mcp.json"),
                    ctx.project.join(".cursor/mcp.json"),
                ],
            ),
        ] {
            if user["compat"][vendor]["mcps"] != false {
                for (i, p) in files.into_iter().enumerate() {
                    let source = if i == 0 {
                        format!("{vendor}-compatible")
                    } else if vendor == "claude" {
                        "mcp-json".into()
                    } else {
                        "cursor-project".into()
                    };
                    entries.extend(server_entries(
                        &read_config(&p, &mut vec![], &source)["mcpServers"],
                        &h,
                        if i == 0 { "compatible" } else { "project" },
                        &source,
                        &[],
                        false,
                    ))
                }
            }
        }
    }
    for l in &ctx.layers {
        let disabled = if h == "claude" && l.scope == "project" {
            list(&ctx.settings["disabledMcpjsonServers"])
        } else {
            vec![]
        };
        let mut rows = server_entries(
            &l.data[server_key(&h)],
            &h,
            &l.scope,
            &l.source,
            &disabled,
            matches!(h.as_str(), "codex" | "opencode" | "gemini"),
        );
        for row in &mut rows {
            row["sourcePath"] = l.path.to_string_lossy().to_string().into();
            row["declarations"] = json!([{"source":l.source,"scope":l.scope,"path":l.path}]);
        }
        entries.extend(rows)
    }
    if h == "claude" {
        let local = &ctx.user_data["projects"][ctx.project.to_string_lossy().as_ref()];
        entries.extend(server_entries(
            &local["mcpServers"],
            &h,
            "local",
            "claude-local",
            &[],
            false,
        ));
        let mut disabled = list(&local["disabledMcpServers"]);
        disabled.extend(list(&ctx.user_data["disabledMcpServers"]));
        for row in &mut entries {
            if disabled.contains(&row["name"]) {
                row["enabled"] = false.into();
                row["status"] = "disabled".into()
            }
        }
    }
    if h == "gemini" {
        let excluded = list(&ctx.settings["mcp"]["excluded"]);
        let allowed = ctx.settings["mcp"]["allowed"].as_array();
        for row in &mut entries {
            if excluded.contains(&row["name"]) || allowed.is_some_and(|a| !a.contains(&row["name"]))
            {
                row["enabled"] = false.into();
                row["status"] = "disabled".into();
                row["_inheritEnabled"] = false.into()
            }
        }
    }
    entries.extend(plugin_mcps(ctx));
    let mut merged: serde_json::Map<String, Value> = serde_json::Map::new();
    for mut row in entries {
        let Some(key) = row["name"].as_str().map(str::to_owned) else {
            ctx.uncertain = true;
            continue;
        };
        let inherit = row
            .as_object_mut()
            .and_then(|m| m.shift_remove("_inheritEnabled"))
            == Some(json!(true));
        if let Some(old) = merged.get(&key) {
            let mut sources = list(&old["sources"]);
            for s in list(&row["sources"]) {
                if !sources.contains(&s) {
                    sources.push(s)
                }
            }
            row["sources"] = sources.into();
            let mut decl = list(&old["declarations"]);
            decl.extend(list(&row["declarations"]));
            row["declarations"] = decl.into();
            if inherit {
                row["enabled"] = old["enabled"].clone()
            }
            if matches!(h.as_str(), "codex" | "opencode" | "gemini")
                && row["descriptionSource"] != "configuration"
                && old["descriptionSource"] == "configuration"
            {
                row["description"] = old["description"].clone();
                row["descriptionSource"] = "configuration".into()
            }
            if row["status"] != "invalid" {
                row["status"] = status(&row["enabled"]).into()
            }
        }
        merged.insert(key, row);
    }
    let mut rows: Vec<_> = merged.into_values().collect();
    rows.sort_by_key(|r| {
        let n = r["name"].as_str().unwrap_or_default();
        (n.case_fold().collect::<String>(), n.to_owned())
    });
    let status = if h == "shell" {
        "unsupported"
    } else if ctx.home.is_none() {
        "unknown"
    } else if !ctx.errors.is_empty() {
        "incomplete"
    } else if !rows.is_empty() {
        "configured"
    } else {
        "empty"
    };
    json!({"harness":h,"account":ctx.alias,"mcps":rows,"status":status,"confidence":if matches!(status,"unknown"|"unsupported"){"unknown"}else{"configuration_files"},"effectiveNow":null,"runtimeEnabled":null,"limitations":ctx.limitations,"errors":ctx.errors})
}
