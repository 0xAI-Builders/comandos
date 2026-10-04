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

/// Línea JSON más larga (bytes) aceptada en cada conexión de cliente y en cada upstream: la
/// misma que el proxy directo ([`crate::transport::MAX_RESPONSE`], 256 MiB). Una línea de
/// cliente mayor cierra esa conexión; una del upstream se descarta y su petición recibe un
/// error (ver `upstream::read_upstream`), sin cerrar el upstream compartido.
const MAX_LINE: usize = crate::transport::MAX_RESPONSE;

/// Un upstream solo se comparte si el proceso sería idéntico: mismo servidor, mismo
/// directorio de trabajo y mismo spec efectivo (`command`, `args`, `cwd`, `env` ya expandido…).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    name: String,
    cwd: PathBuf,
    /// sha256 del JSON canónico (claves ordenadas a todos los niveles) del spec con su `env`
    /// sustituido por el expandido de la sesión y sin `shared` (no cambia el proceso).
    spec_hash: String,
}

impl Key {
    fn new(name: &str, cwd: &Path, spec: &Value, env: &[(String, String)]) -> Self {
        let mut effective = spec.clone();
        if let Some(o) = effective.as_object_mut() {
            o.remove("shared");
            let env: Map<String, Value> = (env.iter())
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            o.insert("env".into(), Value::Object(env));
        }
        let canonical = serde_json::to_vec(&canonical(&effective)).unwrap_or_default();
        let spec_hash = format!("{:x}", Sha256::digest(canonical));
        Self {
            name: name.into(),
            cwd: cwd.into(),
            spec_hash,
        }
    }
}

/// Copia de `v` con las claves de cada objeto en orden (`serde_json` aquí conserva el orden
/// de inserción, así que dos specs iguales escritos en otro orden darían otro hash).
fn canonical(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let sorted: BTreeMap<&String, Value> =
                m.iter().map(|(k, v)| (k, canonical(v))).collect();
            Value::Object(sorted.into_iter().map(|(k, v)| (k.clone(), v)).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let short = &self.spec_hash[..self.spec_hash.len().min(8)];
        write!(
            f,
            "{} (cwd {}, spec {short})",
            self.name,
            self.cwd.display()
        )
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
    let mut attach =
        json!({"attach": name, "cwd": cwd, "env": env, "catalog": catalog, "home": home});
    // El daemon resuelve el ejecutable y arma el PATH del upstream con el de la sesión.
    if let Some(path) = std::env::var("PATH").ok().filter(|p| !p.is_empty()) {
        attach["path"] = Value::String(path);
    }
    Some(attach)
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

#[cfg(test)]
mod tests {
    use super::Key;
    use serde_json::json;
    use std::path::Path;

    fn key(spec: serde_json::Value, env: &[(&str, &str)]) -> Key {
        let env: Vec<_> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Key::new("eco", Path::new("/w"), &spec, &env)
    }

    #[test]
    fn key_covers_the_effective_spec_but_not_key_order_or_shared() {
        let base = key(
            json!({"command":"a","args":["1"],"env":{"X":"$H"}}),
            &[("X", "/h")],
        );
        let same = key(
            json!({"env":{"X":"otro"},"args":["1"],"command":"a","shared":true}),
            &[("X", "/h")],
        );
        assert_eq!(
            base, same,
            "orden de claves, `shared` y el env sin expandir no cuentan"
        );
        assert_ne!(
            base,
            key(json!({"command":"a","args":["2"]}), &[("X", "/h")])
        );
        assert_ne!(
            base,
            key(json!({"command":"b","args":["1"]}), &[("X", "/h")])
        );
        assert_ne!(
            base,
            key(
                json!({"command":"a","args":["1"],"cwd":"sub"}),
                &[("X", "/h")]
            )
        );
        assert_ne!(
            base,
            key(json!({"command":"a","args":["1"]}), &[("X", "/g")])
        );
    }
}
