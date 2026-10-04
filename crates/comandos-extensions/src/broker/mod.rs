//! Broker compartido de servidores MCP: un upstream stdio por nombre, multiplexado entre
//! las sesiones que se conectan por un socket Unix. `mux` es el núcleo puro; el resto es E/S.
mod actor;
pub mod client;
pub mod daemon;
pub mod mux;
mod translate;
mod upstream;

use serde_json::Value;
use std::{
    io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// Línea JSON más larga (bytes) aceptada de un cliente o de un upstream.
const MAX_LINE: usize = 64 * 1024 * 1024;

/// `$XDG_RUNTIME_DIR/comandos/broker.sock`, o `/tmp/comandos-<uid>/broker.sock` sin él.
pub fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        Some(run) => PathBuf::from(run).join("comandos/broker.sock"),
        None => {
            PathBuf::from(format!("/tmp/comandos-{}", nix::unistd::getuid())).join("broker.sock")
        }
    }
}

/// Servidor stdio sin filtro de herramientas: el mismo caso que `serve` ejecuta directo.
pub fn direct_stdio(spec: &Value) -> bool {
    // Mismas condiciones (verdad de Python) que `extension_proxy.serve`.
    spec["command"].as_str().is_some_and(|c| !c.is_empty())
        && spec.get("enabled_tools").is_none()
        && !spec.get("disabled_tools").is_some_and(crate::py_truthy)
}

/// Lo que el broker acepta compartir: stdio directo y compartible según el catálogo.
pub fn brokerable(spec: &Value, name: &str) -> bool {
    direct_stdio(spec) && crate::config::is_shared(spec, name)
}

/// El directorio del socket debe ser nuestro, real (no un enlace) y sin permisos para otros.
fn check_private_dir(dir: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(dir)?;
    let mine = meta.uid() == nix::unistd::getuid().as_raw();
    if !meta.is_dir() || !mine || meta.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::other("directorio del broker inseguro"));
    }
    Ok(())
}

/// Lee una línea sin el `\n` final en `buf`. `Ok(false)` en EOF sin datos; error si la
/// línea supera [`MAX_LINE`]. Solo usa `fill_buf`/`consume`: no lee más allá del `\n`.
async fn read_line<R: AsyncBufRead + Unpin>(r: &mut R, buf: &mut Vec<u8>) -> io::Result<bool> {
    buf.clear();
    loop {
        let chunk = r.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(!buf.is_empty());
        }
        let (n, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        buf.extend_from_slice(&chunk[..n]);
        r.consume(n);
        if done {
            buf.pop();
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return Ok(true);
        }
        if buf.len() > MAX_LINE {
            return Err(io::Error::other("línea demasiado larga"));
        }
    }
}

/// Línea vacía o solo espacios: se ignora en ambos sentidos.
fn blank(line: &[u8]) -> bool {
    line.iter().all(u8::is_ascii_whitespace)
}
