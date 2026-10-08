//! Broker compartido de servidores MCP: un upstream stdio por nombre, multiplexado entre
//! las sesiones que se conectan por un socket Unix. `mux` es el núcleo puro; el resto es E/S.
mod actor;
pub mod client;
pub mod daemon;
pub mod mux;
mod registry;
mod scan;
mod session_access;
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

/// Un upstream solo se comparte con el mismo nombre, directorio y configuración efectiva.
/// `env` contiene TODO el entorno que recibirá el proceso, incluidos los overrides.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    name: String,
    cwd: PathBuf,
    /// Hash del spec canónico con el entorno efectivo completo y sin `shared`.
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

/// Las conexiones nuevas usan la cuenta, entorno y cwd globales del catálogo/daemon.
/// El cwd del cliente se conserva como contexto; no determina el proceso compartido.
/// No se transmite el entorno heredado del cliente ni se expanden sus credenciales.
pub fn attach_request(name: &str, _spec: &Value, catalog: &Path) -> Option<Value> {
    let here = std::env::current_dir().ok()?;
    if !here.is_dir() {
        return None;
    }
    let home = crate::home_dir().ok()?;
    let catalog = std::path::absolute(catalog).ok()?;
    let mut request =
        json!({"attach":name,"cwd":here,"env":{},"catalog":catalog,"home":home,"global":true});
    add_session_binding(
        &mut request,
        std::env::var_os("COMANDOS_MCP_SESSION_BINDING"),
    );
    Some(request)
}

fn add_session_binding(request: &mut Value, binding: Option<std::ffi::OsString>) {
    if let Some(binding) = binding {
        // An invalid inherited value must remain present so the receiver fails closed.
        request["session_binding"] = binding
            .into_string()
            .map(Value::String)
            .unwrap_or(Value::Null);
    }
}

/// v2 no se adjunta a un daemon antiguo que ignore la autoridad global del catálogo.
/// Codex omite XDG_RUNTIME_DIR: se descubre el runtime privado del mismo usuario.
pub fn socket_path() -> PathBuf {
    let uid = nix::unistd::getuid().as_raw();
    socket_path_for(
        std::env::var_os("XDG_RUNTIME_DIR").as_deref(),
        uid,
        &PathBuf::from(format!("/run/user/{uid}")),
    )
}

fn socket_path_for(runtime: Option<&std::ffi::OsStr>, uid: u32, system_run: &Path) -> PathBuf {
    if let Some(runtime) = runtime.filter(|r| !r.is_empty()) {
        return PathBuf::from(runtime).join("comandos/broker-v2.sock");
    }
    if check_private_dir_for_uid(system_run, uid).is_ok() {
        return system_run.join("comandos/broker-v2.sock");
    }
    PathBuf::from(format!("/tmp/comandos-{uid}")).join("broker-v2.sock")
}

/// Servidor stdio sin filtro de herramientas: el mismo caso que `serve` ejecuta directo.
pub fn direct_stdio(spec: &Value) -> bool {
    // Mismas condiciones (verdad de Python) que `extension_proxy.serve`.
    spec["command"].as_str().is_some_and(|c| !c.is_empty())
        && spec.get("enabled_tools").is_none()
        && !spec.get("disabled_tools").is_some_and(crate::py_truthy)
}

/// Stdio y HTTP comparten el broker; la fachada preserva filtros y autenticación HTTP.
pub fn brokerable(spec: &Value, name: &str) -> bool {
    (spec["command"].as_str().is_some_and(|s| !s.is_empty())
        || spec["url"].as_str().is_some_and(|s| !s.is_empty()))
        && crate::config::is_shared(spec, name)
}

/// El directorio del socket debe ser nuestro, real (no un enlace) y sin permisos para otros.
fn check_private_dir(dir: &Path) -> io::Result<()> {
    check_private_dir_for_uid(dir, nix::unistd::getuid().as_raw())
}

fn check_private_dir_for_uid(dir: &Path, uid: u32) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(dir)?;
    let mine = meta.uid() == uid;
    if !meta.is_dir() || !mine || meta.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::other("directorio del broker inseguro"));
    }
    Ok(())
}

/// Lee una línea sin el `\n` final en `buf`. `Ok(false)` en EOF sin datos; error si la
/// línea supera [`MAX_LINE`]. Solo usa `fill_buf`/`consume`: no lee más allá del `\n`.
async fn read_line<R: AsyncBufRead + Unpin>(r: &mut R, buf: &mut Vec<u8>) -> io::Result<bool> {
    read_line_with(r, buf, true).await
}

/// Como [`read_line`]; con `keep_partial = false`, una línea sin `\n` cortada por el EOF se
/// descarta y cuenta como EOF (un daemon que muere a mitad de escribirla).
async fn read_line_with<R: AsyncBufRead + Unpin>(
    r: &mut R,
    buf: &mut Vec<u8>,
    keep_partial: bool,
) -> io::Result<bool> {
    buf.clear();
    loop {
        let chunk = r.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(keep_partial && !buf.is_empty());
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
    use super::{Key, socket_path_for};
    use serde_json::json;
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    fn key(spec: serde_json::Value, env: &[(&str, &str)]) -> Key {
        let env: Vec<_> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Key::new("eco", Path::new("/w"), &spec, &env)
    }

    #[test]
    fn optional_pane_binding_preserves_absence_and_invalid_explicit_values() {
        use std::os::unix::ffi::OsStringExt;
        let mut request = json!({"global":true,"env":{}});
        super::add_session_binding(&mut request, None);
        assert!(request.get("session_binding").is_none());
        let binding = "/private/tmux.sock|123|456|$7|%8|901";
        super::add_session_binding(&mut request, Some(binding.into()));
        assert_eq!(request["session_binding"], binding);
        assert_eq!(request["env"], json!({}));
        super::add_session_binding(&mut request, Some("".into()));
        assert_eq!(request["session_binding"], "");
        super::add_session_binding(&mut request, Some(std::ffi::OsString::from_vec(vec![0xff])));
        assert!(request.get("session_binding").unwrap().is_null());
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

    #[test]
    fn socket_discovery_uses_only_an_owned_private_system_runtime() {
        let uid = nix::unistd::getuid().as_raw();
        let root = std::env::temp_dir().join(format!("cc-socket-discovery-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let fallback = std::path::PathBuf::from(format!("/tmp/comandos-{uid}/broker-v2.sock"));
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            socket_path_for(None, uid, &root),
            root.join("comandos/broker-v2.sock")
        );
        assert_eq!(
            socket_path_for(Some(std::ffi::OsStr::new("")), uid, &root),
            root.join("comandos/broker-v2.sock")
        );
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(socket_path_for(None, uid, &root), fallback);
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            socket_path_for(None, uid.wrapping_add(1), &root),
            std::path::PathBuf::from(format!(
                "/tmp/comandos-{}/broker-v2.sock",
                uid.wrapping_add(1)
            ))
        );
        let link = root.join("linked");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        assert_eq!(socket_path_for(None, uid, &link), fallback);
        assert_eq!(socket_path_for(None, uid, &root.join("missing")), fallback);
        assert_eq!(
            socket_path_for(
                Some(std::ffi::OsStr::new("/private/custom-runtime")),
                uid,
                &root
            ),
            Path::new("/private/custom-runtime/comandos/broker-v2.sock")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
