//! Una release antigua solo se activa después de recuperar sus fuentes legado.
use std::path::Path;
pub fn rollback_state(home: &Path, target_release: &Path) -> Result<(), String> {
    if super::manifest::release_protocol(target_release) >= super::manifest::STATE_PROTOCOL {
        return Ok(());
    }
    let path = comandos_store::unified::unified_path(home);
    if !path.exists() {
        for d in comandos_store::domains::catalog::DOMAINS {
            if comandos_store::unified::seal_guard_path(&path, d.name)
                .map_err(|e| e.to_string())?
                .symlink_metadata()
                .is_ok()
            {
                return Err("base ausente con dominio sellado; export-legacy requerido".into());
            }
        }
        return Ok(());
    }
    let c = comandos_store::unified::open_unified(&path).map_err(|e| e.to_string())?;
    comandos_store::migrate::lifecycle::demote_all(
        home,
        &c,
        comandos_store::migrate::journal::now_ms().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
