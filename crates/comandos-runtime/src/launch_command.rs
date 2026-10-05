//! Piezas del lanzamiento de un CLI con su cuenta (`bin/cc-dash`).
//!
//! Por ahora solo `inherit_trust_for_switch` (2398, árbol vivo D8) en su rama
//! Claude, la que usa POST `/session-new` (2f-1/T4). La rama Codex
//! (`_inherit_codex_trust`, D8) y `configuration_command` las porta 2f-2/T1 en
//! este mismo módulo: hasta entonces `harness == "codex"` es `Unsure` y quien
//! llama declina antes de cualquier efecto.
use crate::{Unsure, claude_trust};
use comandos_core::json::truthy;
use serde_json::Value;

/// `_claude_config_dir(alias)` (2390): `None` para `main` (vive en el HOME).
fn claude_config_dir(registry: &Value, alias: &str, home: &str) -> Result<Option<String>, Unsure> {
    if alias.is_empty() || alias == "main" {
        return Ok(None);
    }
    // `(registry.get("harnesses") or {}).get("claude") or {}`: el registro ya
    // está validado; otra forma lanzaría dentro del `try` del llamador.
    let spec = &registry["harnesses"]["claude"];
    let root = &spec["accountsRoot"];
    let root = if truthy(root) {
        root.as_str().ok_or(Unsure)?
    } else {
        "~/.claude-accounts"
    };
    let root = claude_trust::expanduser(root, home)?;
    Ok(Some(if root.is_empty() || root.ends_with('/') {
        format!("{root}{alias}")
    } else {
        format!("{root}/{alias}")
    }))
}

/// `inherit_trust_for_switch(cwd, from_alias, to_alias, harness)`: copia la
/// aceptación de la carpeta de la cuenta origen a la destino, solo si la
/// cuenta cambia y el origen ya la tenía. `Ok(false)` también para toda
/// excepción del Python (la traga). `home` es `os.path.expanduser("~")`.
///
/// El `cc_usage.record_change(... "trust_inherited" ...)` que sigue a un
/// acierto lo escribe quien llama (la base de uso es del frente): su nota es
/// `trust_note(cwd, from_alias, to_alias)`.
pub fn inherit_trust_for_switch(
    registry: &Value,
    home: &str,
    cwd: &str,
    from_alias: &str,
    to_alias: &str,
    harness: &str,
) -> Result<bool, Unsure> {
    let or_main = |alias: &str| {
        if alias.is_empty() {
            "main".to_owned()
        } else {
            alias.to_owned()
        }
    };
    if !matches!(harness, "claude" | "codex")
        || cwd.is_empty()
        || or_main(to_alias) == or_main(from_alias)
    {
        return Ok(false);
    }
    if harness == "codex" {
        // `_inherit_codex_trust` (D8): 2f-2/T1.
        return Err(Unsure);
    }
    let Some(dest) = claude_config_dir(registry, to_alias, home)? else {
        return Ok(false);
    };
    let source = claude_config_dir(registry, from_alias, home)?;
    Ok(claude_trust::inherit_cwd_trust(cwd, source.as_deref(), &dest, home)? == Some(true))
}

/// La nota del registro de cambios tras heredar la confianza.
pub fn trust_note(cwd: &str, from_alias: &str, to_alias: &str) -> String {
    fn or_main(alias: &str) -> &str {
        if alias.is_empty() { "main" } else { alias }
    }
    format!(
        "trust heredado {} -> {} en {cwd}",
        or_main(from_alias),
        or_main(to_alias)
    )
}

/// `claude_trust::probe` de lo que leería `inherit_trust_for_switch` (Claude,
/// de `from_alias` a `to_alias`): `Unsure` si algún archivo no se reproduce.
/// Sin herencia posible (misma cuenta, destino `main`) no lee nada.
pub fn trust_probe(
    registry: &Value,
    home: &str,
    from_alias: &str,
    to_alias: &str,
) -> Result<(), Unsure> {
    let or_main = |alias: &str| {
        if alias.is_empty() {
            "main".to_owned()
        } else {
            alias.to_owned()
        }
    };
    if or_main(to_alias) == or_main(from_alias) {
        return Ok(());
    }
    let Some(dest) = claude_config_dir(registry, to_alias, home)? else {
        return Ok(());
    };
    let source = claude_config_dir(registry, from_alias, home)?;
    claude_trust::probe(source.as_deref(), &dest, home)
}
