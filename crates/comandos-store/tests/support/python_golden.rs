//! Oráculo dorado sobre HOME privado; Python solo en record/check explícitos.
//! Copia del ayudante de `comandos-runtime` (`tests/support/python.rs`): los
//! ejecutables con efectos fuera del HOME son enlaces a `true` (incluido `tmux`), el
//! guion no ve el systemd ni el DBus de la sesión real, y se quitan las claves del
//! entorno que cambian lo que calcula el Python (D7 del plan 2e). Zona fija
//! `America/Mexico_City` y `LANG=C.UTF-8`, iguales en el lado Rust.
#![allow(dead_code)]
use sha2::{Digest, Sha256};
use std::{ffi::OsStr, path::Path, process::Command};

pub const TZ: &str = "America/Mexico_City";

/// Claves que el Python lee del entorno y que el Rust recibe por parámetro (D7).
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
    for dir in [&fakebin, &runtime] {
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
        "git",
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
    let input = serde_json::json!({"script":script_input,"args":args_input,"bin_inputs":bins});
    let artifact_root = if home.file_name().is_some_and(|n| n == "home")
        && parent != Path::new("/tmp")
        && parent != std::env::temp_dir()
    {
        parent
    } else {
        home
    };
    let result = comandos_oracle::text_with_tree_at(
        &comandos_oracle::golden_root(Path::new(env!("CARGO_MANIFEST_DIR"))),
        "shared-python",
        &input,
        artifact_root,
        &roots,
        || {
            let mut command = Command::new("python3");

            command
                .arg("-c")
                .arg(script)
                .arg(repo())
                .args(args)
                .current_dir(home)
                .env("HOME", home)
                .env("PATH", &path)
                .env("TZ", TZ)
                .env("LANG", "C.UTF-8")
                .env_remove("LC_ALL")
                .env_remove("LC_CTYPE")
                .env("XDG_RUNTIME_DIR", &runtime)
                .env("XDG_STATE_HOME", home.join(".local/state"))
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .env_remove("TMUX")
                .env_remove("TMUX_TMPDIR")
                .env_remove("CLAUDE_CONFIG_DIR")
                .env_remove("CODEX_HOME")
                .env_remove("GROK_HOME")
                .env_remove("COMANDOS_STATE_DB")
                .env_remove("COMANDOS_USAGE_DB")
                .env_remove("DBUS_SESSION_BUS_ADDRESS")
                .env_remove("DISPLAY")
                .env_remove("WAYLAND_DISPLAY");
            for key in D7_KEYS {
                command.env_remove(key);
            }
            let out = command.output().unwrap();
            assert!(
                out.status.success(),
                "oráculo: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).map_err(|e| e.to_string())
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
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
                walk(&path, roots, aliases);
            } else if meta.is_file() && path.file_name().is_some_and(|n| n == "SKILL.md") {
                let real = path.canonicalize().unwrap();
                let real = real.to_string_lossy();
                let normalized =
                    String::from_utf8(comandos_oracle::normalize(real.as_bytes(), roots)).unwrap();
                aliases.push((
                    format!("{{{{SKILL_ID:{normalized}}}}}"),
                    format!("{:x}", Sha256::digest(real.as_bytes()))[..24].into(),
                ));
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
