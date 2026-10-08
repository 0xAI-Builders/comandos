//! Capacidad del protocolo de estado; un manifiesto ausente o inválido vale cero.
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
pub const STATE_PROTOCOL: u32 = 2;
pub fn release_protocol(dir: &Path) -> u32 {
    let path = dir.join("manifest.json");
    if !path.symlink_metadata().is_ok_and(|m| m.is_file()) {
        return 0;
    }
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v.get("state_protocol").and_then(|p| p.as_u64()))
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
}
/// Se escribe bajo el candado de la release antes de que exista el binario final.
pub(crate) fn write_manifest(dir: &Path) -> Result<(), String> {
    let path = dir.join("manifest.json");
    comandos_store::files::write_atomic(
        &path,
        format!("{{\"state_protocol\":{STATE_PROTOCOL}}}\n").as_bytes(),
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    fs::File::open(&path)
        .and_then(|f| f.sync_all())
        .and_then(|()| fs::File::open(dir)?.sync_all())
        .map_err(|e| format!("{}: {e}", path.display()))
}
