//! Oráculo congelado sobre HOME temporal: replay requiere un dorado y nunca lanza Python.
//! Mismo entorno que el oráculo de `comandos-server` (`tests/support/oracle.rs`):
//! los ejecutables con efectos fuera del HOME son enlaces a `true`, y el guion no
//! ve el tmux, el systemd ni el DBus de la sesión real.
#![allow(dead_code)]
use std::{ffi::OsStr, path::Path};
#[path = "python_golden.rs"]
mod golden;

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
    assert!(
        matches!(
            env!("CARGO_CRATE_NAME"),
            "extension_prepare_oracle"
                | "tmux_snapshot_oracle"
                | "extension_inventory_oracle"
                | "session_profiles_oracle"
                | "pane_extensions_oracle"
                | "session_configuration_oracle"
                | "cli_catalog_oracle"
                | "extension_observations_oracle"
                | "limits_oracle"
                | "model_watch_oracle"
                | "news_editions_oracle"
                | "pane_snapshot_oracle"
                | "providers_oracle"
                | "providers_public_oracle"
                | "ssh_config_oracle"
                | "tui_state_oracle"
        ),
        "caller no inventariado: falta contrato/dorado explícito"
    );
    golden::run_python(script, args, home)
}
