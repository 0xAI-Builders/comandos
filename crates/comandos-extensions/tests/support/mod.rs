//! Utilidades compartidas por las pruebas de integración.
pub mod fake_mcp_http;

use std::path::Path;

/// Enlaza (solo lectura) el venv real del oráculo Python bajo un HOME temporal: con
/// HOME distinto el oráculo no ve el `mcp` del site de usuario y `bin/cc-extensions`
/// cae al venv de extensiones bajo HOME.
pub fn link_oracle_venv(home: &Path) {
    let real_home = std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let venv = real_home.join(".local/share/comandos/extensions-venv");
    assert!(venv.exists(), "falta el venv del oráculo en {venv:?}");
    std::fs::create_dir_all(home.join(".local/share/comandos")).unwrap();
    std::os::unix::fs::symlink(&venv, home.join(".local/share/comandos/extensions-venv")).unwrap();
}
