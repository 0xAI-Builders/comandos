//! Broker compartido de servidores MCP: un upstream stdio por nombre, multiplexado entre
//! las sesiones que se conectan por un socket Unix. `mux` es el núcleo puro; el resto es E/S.
mod actor;
pub mod client;
pub mod daemon;
pub mod mux;
mod registry;
mod translate;
mod upstream;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// Línea JSON más larga (bytes) aceptada en cada conexión de cliente y en cada upstream.
/// 16 MiB: cabe una captura de pantalla de Chrome en base64 con holgura; una línea mayor
/// cierra esa conexión (o ese upstream), no las demás.
const MAX_LINE: usize = 16 * 1024 * 1024;

/// Un upstream solo se comparte si el proceso sería idéntico: mismo servidor, mismo
/// directorio de trabajo y mismo `env` del catálogo ya expandido.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    name: String,
    cwd: PathBuf,
    /// sha256 del JSON canónico (claves ordenadas) del `env` expandido.
    env_hash: String,
}

impl Key {
    fn new(name: &str, cwd: &Path, env: &[(String, String)]) -> Self {
        let sorted: BTreeMap<&str, &str> = env.iter().map(|(k, v)| (&**k, &**v)).collect();
        let canonical = serde_json::to_vec(&sorted).unwrap_or_default();
        let env_hash = format!("{:x}", Sha256::digest(canonical));
        Self {
            name: name.into(),
            cwd: cwd.into(),
            env_hash,
        }
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let short = &self.env_hash[..self.env_hash.len().min(8)];
        write!(f, "{} (cwd {}, env {short})", self.name, self.cwd.display())
    }
}

/// Línea `attach` de la sesión: lo que el proxy directo habría usado para lanzar el proceso.
/// `None` si el directorio de trabajo no existe: el proxy directo da el error de siempre.
pub fn attach_request(name: &str, spec: &Value, catalog: &Path) -> Option<Value> {
    let here = std::env::current_dir().ok()?;
    let cwd = match spec
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
    {
        Some(cwd) => here.join(crate::expand_user(cwd)),
        None => here,
    };
    if !cwd.is_dir() {
        return None;
    }
    let env: Map<String, Value> = (crate::spec_env(spec).ok()?.into_iter())
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    let home = crate::home_dir().ok()?;
    let catalog = std::path::absolute(catalog).ok()?;
    Some(json!({"attach": name, "cwd": cwd, "env": env, "catalog": catalog, "home": home}))
}

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
