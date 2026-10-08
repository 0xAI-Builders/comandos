//! Oráculo dorado sobre HOME privado; Python solo en record/check explícitos.
//! Mismo entorno que el oráculo de `comandos-server` (`tests/support/oracle.rs`):
//! los ejecutables con efectos fuera del HOME son enlaces a `true`, y el guion no
//! ve el tmux, el systemd ni el DBus de la sesión real.
#![allow(dead_code)]
use sha2::{Digest, Sha256};
use std::{ffi::OsStr, path::Path, process::Command};

/// Claves del entorno que cambian lo que calcula el Python de uso (D7 del plan 2e):
/// el lado Rust las recibe por parámetro, así que el oráculo no debe heredarlas.
pub const D7_KEYS: &[&str] = &[
    "COMANDOS_DAILY_BUDGET_USD",
    "COMANDOS_USAGE_DAILY_BUDGET_USD",
    "COMANDOS_CODEX_DAILY_TOKEN_LIMIT",
    "COMANDOS_CODEX_WEEKLY_TOKEN_LIMIT",
    "COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT",
    "COMANDOS_CLAUDE_WEEKLY_TOKEN_LIMIT",
    "CODEX_DAILY_TOKEN_LIMIT",
    "CODEX_WEEKLY_TOKEN_LIMIT",
    "CLAUDE_DAILY_TOKEN_LIMIT",
    "CLAUDE_WEEKLY_TOKEN_LIMIT",
    "COMANDOS_USAGE_LOCAL_DAYS",
    "COMANDOS_USAGE_CLAUDE_MAX_FILES",
    "COMANDOS_USAGE_CODEX_MAX_FILES",
    "COMANDOS_CLAUDE_PROJECTS_DIR",
    "COMANDOS_OPENCODE_DB",
    "OPENAI_ADMIN_KEY",
    "ANTHROPIC_ADMIN_KEY",
];

pub fn repo() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn run_python(script: &str, args: &[&OsStr], home: &Path) -> Option<String> {
    let fakebin = home.join("fakebin");
    let runtime = home.join("xdg-runtime");
    let tmux = home.join("tmux");
    for dir in [&fakebin, &runtime, &tmux] {
        std::fs::create_dir_all(dir).unwrap();
    }
    for name in [
        "systemctl",
        "wmctrl",
        "cc-webterm",
        "cc-webterm-attach",
        "systemd-run",
        "tailscale",
        "notify-send",
        "pw-play",
        "paplay",
        "spd-say",
        "piper",
        "xdg-open",
        "tmux",
    ] {
        let link = fakebin.join(name);
        if !link.exists() {
            std::os::unix::fs::symlink("/bin/true", &link).unwrap();
        }
    }
    let path = format!(
        "{}:{}",
        fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let repo_root = repo().canonicalize().unwrap();
    let parent = home
        .parent()
        .filter(|p| *p != std::env::temp_dir())
        .unwrap_or(home);
    let mut roots = vec![
        ("{{HOME}}", home),
        ("{{REPO}}", repo_root.as_path()),
        ("{{FIXTURE}}", parent),
    ];
    let aliases = if matches!(
        env!("CARGO_CRATE_NAME"),
        "usage_import_oracle"
            | "extension_inventory_oracle"
            | "extension_prepare_oracle"
            | "session_profiles_oracle"
    ) {
        derived_aliases(home, &roots)
    } else {
        Vec::new()
    };
    roots.extend(
        aliases
            .iter()
            .map(|(token, path)| (token.as_str(), path.as_path())),
    );
    let tmux_aliases = if env!("CARGO_CRATE_NAME") == "tmux_snapshot_oracle" {
        fixture_tmux_aliases(args)
    } else {
        Vec::new()
    };
    roots.extend(
        tmux_aliases
            .iter()
            .map(|(token, path)| (token.as_str(), path.as_path())),
    );
    let args_input: Vec<_> = args
        .iter()
        .map(|arg| {
            let text = arg.to_string_lossy();
            let normalized =
                String::from_utf8(comandos_oracle::normalize(text.as_bytes(), &roots)).unwrap();
            let file = std::fs::read(Path::new(arg)).ok().map(|bytes| {
                format!(
                    "{:x}",
                    Sha256::digest(comandos_oracle::normalize(&bytes, &roots))
                )
            });
            serde_json::json!({"arg":normalized,"file":file})
        })
        .collect();
    let script_input =
        String::from_utf8(comandos_oracle::normalize(script.as_bytes(), &roots)).unwrap();
    let mut bins = Vec::new();
    if let Ok(entries) = std::fs::read_dir(home.join("bin")) {
        for entry in entries {
            let entry = entry.unwrap();
            if let Ok(bytes) = std::fs::read(entry.path()) {
                bins.push((
                    entry.file_name().to_string_lossy().into_owned(),
                    format!(
                        "{:x}",
                        Sha256::digest(comandos_oracle::normalize(&bytes, &roots))
                    ),
                ));
            }
        }
        bins.sort();
    }
    let mut input = serde_json::json!({"script":script_input,"args":args_input,"bin_inputs":bins});
    if matches!(
        env!("CARGO_CRATE_NAME"),
        "extension_inventory_oracle" | "session_profiles_oracle" | "extension_prepare_oracle"
    ) {
        input["fixtures"] = fixture_identity(home, &roots);
    }
    let artifact_root = if env!("CARGO_CRATE_NAME") == "tmux_snapshot_oracle" {
        home
    } else if home.file_name().is_some_and(|n| n == "home")
        && parent != Path::new("/tmp")
        && parent != std::env::temp_dir()
    {
        parent
    } else {
        home
    };
    let run = || {
        let mut command = Command::new("python3");

        for key in D7_KEYS {
            command.env_remove(key);
        }
        let out = command
            .arg("-c")
            .arg(script)
            .arg(repo())
            .args(args)
            .current_dir(repo())
            .env("HOME", home)
            .env("PATH", &path)
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("TMUX_TMPDIR", &tmux)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            // Codificación de `open()` fija: la del lado Rust (UTF-8).
            .env("LANG", "C.UTF-8")
            .env_remove("LC_ALL")
            .env_remove("LC_CTYPE")
            .env_remove("TMUX")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .env_remove("GROK_HOME")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("OPENCODE_CONFIG")
            .env_remove("OPENCODE_CONFIG_DIR")
            .env_remove("COMANDOS_STATE_DB")
            .env_remove("COMANDOS_USAGE_DB")
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "oráculo: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).map_err(|e| e.to_string())
    };
    let result = if env!("CARGO_CRATE_NAME") == "extension_prepare_oracle" {
        prepare_reference(&input, home, &roots, run)
    } else if (env!("CARGO_CRATE_NAME") == "tmux_snapshot_oracle"
        && !script.contains("save_closed_pane_snapshot"))
        || matches!(
            env!("CARGO_CRATE_NAME"),
            "pane_extensions_oracle"
                | "session_configuration_oracle"
                | "extension_inventory_oracle"
                | "session_profiles_oracle"
        )
    {
        let output = comandos_oracle::oracle_at(
            &comandos_oracle::golden_root(Path::new(env!("CARGO_MANIFEST_DIR"))),
            "shared-python-pure",
            &input,
            || run().map(|stdout| comandos_oracle::normalize(stdout.as_bytes(), &roots)),
        );
        String::from_utf8(comandos_oracle::restore(&output, &roots)).unwrap()
    } else {
        comandos_oracle::text_with_tree_at(
            &comandos_oracle::golden_root(Path::new(env!("CARGO_MANIFEST_DIR"))),
            "shared-python",
            &input,
            artifact_root,
            &roots,
            run,
        )
        .unwrap_or_else(|e| panic!("{e}"))
    };
    Some(result)
}

// Los IDs derivados de rutas se rehidratan con la fórmula de referencia,
// independiente del código nativo que sigue sujeto a las aserciones de las pruebas.
fn derived_aliases(home: &Path, roots: &[(&str, &Path)]) -> Vec<(String, std::path::PathBuf)> {
    fn walk(dir: &Path, roots: &[(&str, &Path)], aliases: &mut Vec<(String, std::path::PathBuf)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                let real = path.canonicalize().unwrap();
                let real = real.to_string_lossy();
                let normalized = comandos_oracle::normalize(real.as_bytes(), roots);
                aliases.push((
                    format!(
                        "config/{{{{PLUGIN_ROOT:{:x}}}}}/",
                        Sha256::digest(&normalized)
                    ),
                    format!(
                        "config/{}/",
                        &format!("{:x}", Sha256::digest(real.as_bytes()))[..8]
                    )
                    .into(),
                ));
                walk(&path, roots, aliases);
            } else if meta.is_file() && path.file_name().is_some_and(|n| n == "SKILL.md") {
                let real = path.canonicalize().unwrap();
                let real = real.to_string_lossy();
                let normalized =
                    String::from_utf8(comandos_oracle::normalize(real.as_bytes(), roots)).unwrap();
                aliases.push((
                    format!(
                        "{{{{SKILL_ID:{:x}}}}}",
                        Sha256::digest(normalized.as_bytes())
                    ),
                    format!("{:x}", Sha256::digest(real.as_bytes()))[..24].into(),
                ));
            } else if meta.is_file()
                && path.to_string_lossy().contains("/extensions/sizes/")
                && path.extension().is_some_and(|n| n == "json")
            {
                if let Ok(bytes) = std::fs::read(&path)
                    && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
                    && let Some(stamp) = value["measuredAt"].as_i64()
                {
                    let name = path.file_stem().unwrap().to_string_lossy();
                    for space in ["", " "] {
                        aliases.push((
                            format!("\"measuredAt\":{space}{{{{MEASURED:{name}}}}}"),
                            format!("\"measuredAt\":{space}{stamp}").into(),
                        ));
                    }
                }
            } else if meta.is_file() && path.extension().is_some_and(|n| n == "jsonl") {
                let Ok(body) = std::fs::read_to_string(&path) else {
                    continue;
                };
                for (i, line) in body.lines().enumerate() {
                    let Ok(row) = comandos_core::json::workspace_loads(line) else {
                        continue;
                    };
                    let update = &row["params"]["update"];
                    if update["sessionUpdate"] != "turn_completed" {
                        continue;
                    }
                    let prompt = update["prompt_id"].as_str().unwrap_or("");
                    let original = format!("{}\x1f{}\x1f{prompt}", path.display(), i + 1);
                    let normalized =
                        String::from_utf8(comandos_oracle::normalize(original.as_bytes(), roots))
                            .unwrap();
                    let token = format!(
                        "{{{{GROK_ID:{:x}}}}}",
                        Sha256::digest(normalized.as_bytes())
                    );
                    aliases.push((
                        token,
                        format!("{:x}", Sha256::digest(original.as_bytes()))[..32].into(),
                    ));
                }
            }
        }
    }
    let mut aliases = Vec::new();
    walk(home, roots, &mut aliases);
    aliases
}

fn fixture_identity(home: &Path, roots: &[(&str, &Path)]) -> serde_json::Value {
    fn walk(
        home: &Path,
        dir: &Path,
        roots: &[(&str, &Path)],
        out: &mut std::collections::BTreeMap<String, String>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let rel = path
                .strip_prefix(home)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let first = rel.split('/').next().unwrap_or("");
            if matches!(
                first,
                ".oracle"
                    | "fakebin"
                    | "xdg-runtime"
                    | "tmux"
                    | "bin"
                    | ".git"
                    | "runtime"
                    | ".cache"
            ) || first.starts_with("runtime-")
                || first.starts_with("native-")
                || rel.starts_with(".local/state/comandos/extensions/sizes/")
                || rel.contains("/runtime/")
                || rel.contains(".sqlite")
            {
                continue;
            }
            let m = std::fs::symlink_metadata(&path).unwrap();
            if m.is_dir() {
                walk(home, &path, roots, out);
            } else if m.is_file() {
                let bytes = std::fs::read(&path).unwrap();
                out.insert(
                    rel,
                    format!(
                        "{:x}",
                        Sha256::digest(comandos_oracle::normalize(&bytes, roots))
                    ),
                );
            } else if m.file_type().is_symlink() {
                let target = std::fs::read_link(&path).unwrap();
                out.insert(
                    rel,
                    String::from_utf8(comandos_oracle::normalize(
                        target.to_string_lossy().as_bytes(),
                        roots,
                    ))
                    .unwrap(),
                );
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(home, home, roots, &mut out);
    serde_json::json!(out)
}

fn fixture_tmux_aliases(args: &[&OsStr]) -> Vec<(String, std::path::PathBuf)> {
    let Some(last) = args.last() else {
        return Vec::new();
    };
    let Ok(rows) = serde_json::from_str::<serde_json::Value>(&last.to_string_lossy()) else {
        return Vec::new();
    };
    let Some(rows) = rows.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in rows {
        let (Some(pane), Some(pid), Some(start)) = (
            row["pane"].as_str(),
            row["pid"].as_i64(),
            row["start"].as_i64(),
        ) else {
            continue;
        };
        for space in ["", " "] {
            out.push((
                format!(
                    "\"pid\":{space}{{{{PID:{pane}}}}},{space}\"start\":{space}{{{{START:{pane}}}}}"
                ),
                format!("\"pid\":{space}{pid},{space}\"start\":{space}{start}").into(),
            ));
        }
    }
    out
}

// El bundle de referencia se graba como datos. Las sumas de sus archivos se
// rehidratan desde el contenido grabado, nunca desde el resultado nativo.
fn prepare_reference(
    input: &serde_json::Value,
    home: &Path,
    roots: &[(&str, &Path)],
    run: impl FnOnce() -> Result<String, String>,
) -> String {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let output = comandos_oracle::oracle_at(
        &comandos_oracle::golden_root(Path::new(env!("CARGO_MANIFEST_DIR"))),
        "shared-python-prepare",
        input,
        || {
            let raw = run()?;
            let mut value: serde_json::Value =
                serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            let private = value[0]["manifest"]
                .as_str()
                .map(|p| Path::new(p).parent().unwrap().to_path_buf());
            let mut local_roots = roots.to_vec();
            if let Some(private) = &private {
                local_roots.push(("{{ORACLE_PRIVATE}}", private.as_path()));
            }
            let mut files = serde_json::Map::new();
            if let Some(artifacts) = value[1]
                .get_mut("artifacts")
                .and_then(serde_json::Value::as_object_mut)
            {
                for (path, digest) in artifacts {
                    let path = Path::new(path);
                    if !path.starts_with(home) {
                        return Err("artefacto de referencia fuera del fixture".into());
                    }
                    let body = std::fs::read(path).map_err(|e| e.to_string())?;
                    if digest.as_str() != Some(&format!("{:x}", Sha256::digest(&body))) {
                        return Err("digest de referencia inválido".into());
                    }
                    let name = path.file_name().unwrap().to_string_lossy().into_owned();
                    let text = String::from_utf8(comandos_oracle::normalize(&body, &local_roots))
                        .map_err(|e| e.to_string())?;
                    files.insert(name.clone(), text.into());
                    *digest = format!("{{{{ARTIFACT_SHA:{name}}}}}").into();
                }
            }
            let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
            let value: serde_json::Value =
                serde_json::from_slice(&comandos_oracle::normalize(&bytes, &local_roots))
                    .map_err(|e| e.to_string())?;
            serde_json::to_vec(&serde_json::json!({"value":value,"files":files}))
                .map_err(|e| e.to_string())
        },
    );
    let artifact: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let private = home
        .join(".oracle/reference")
        .join(&comandos_oracle::key(input).unwrap()[..16]);
    std::fs::create_dir_all(&private).unwrap();
    let mut local_roots = roots.to_vec();
    local_roots.push(("{{ORACLE_PRIVATE}}", private.as_path()));
    let mut value = String::from_utf8(comandos_oracle::restore(
        &serde_json::to_vec(&artifact["value"]).unwrap(),
        &local_roots,
    ))
    .unwrap();
    for (name, body) in artifact["files"].as_object().unwrap() {
        assert!(
            Path::new(name).components().count() == 1 && name != ".." && name != ".",
            "nombre de artefacto inválido"
        );
        let body = comandos_oracle::restore(body.as_str().unwrap().as_bytes(), &local_roots);
        let path = private.join(name);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        file.write_all(&body).unwrap();
        value = value.replace(
            &format!("{{{{ARTIFACT_SHA:{name}}}}}"),
            &format!("{:x}", Sha256::digest(&body)),
        );
    }
    let parsed: serde_json::Value = serde_json::from_str(&value).unwrap();
    if parsed[0].get("manifest").is_some() {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(private.join("manifest.json"))
            .unwrap();
        file.write_all(&serde_json::to_vec(&parsed[1]).unwrap())
            .unwrap();
    }
    value
}
