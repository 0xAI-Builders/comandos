//! A pane controls its access to a global service without restarting either process.
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone)]
pub(super) struct Access {
    home: PathBuf,
    catalog: PathBuf,
    identity: Option<String>,
    server: String,
    unresolved: bool,
}

impl Access {
    pub(super) async fn for_peer(
        home: &Path,
        catalog: &Path,
        pid: Option<i32>,
        server: &str,
        binding: Option<&Value>,
    ) -> Self {
        let identity = match binding {
            Some(binding) => explicit_identity(binding).await.map(Some),
            None => match pid {
                Some(pid) => pane_identity(pid).await,
                None => Ok(None),
            },
        };
        Self {
            home: home.into(),
            catalog: catalog.into(),
            unresolved: identity.is_err(),
            identity: identity.ok().flatten(),
            server: server.into(),
        }
    }

    /// None forwards the message. Some(None) drops a denied notification; Some(Some)
    /// answers locally. Responses, cancellation and initialization always remain usable.
    pub(super) fn intercept(&self, bytes: &[u8]) -> Option<Option<Vec<u8>>> {
        let message: Value = serde_json::from_slice(bytes).ok()?;
        let method = message.get("method").and_then(Value::as_str)?;
        if matches!(
            method,
            "initialize" | "ping" | "notifications/initialized" | "notifications/cancelled"
        ) {
            return None;
        }
        let allowed = self.allowed();
        if matches!(allowed, Ok(true)) {
            return None;
        }
        let Some(id) = message.get("id") else {
            return Some(None);
        };
        let reply = if allowed.is_ok() {
            match method {
                "tools/list" => json!({"jsonrpc":"2.0","id":id,"result":{"tools":[]}}),
                "resources/list" => json!({"jsonrpc":"2.0","id":id,"result":{"resources":[]}}),
                "resources/templates/list" => {
                    json!({"jsonrpc":"2.0","id":id,"result":{"resourceTemplates":[]}})
                }
                "prompts/list" => json!({"jsonrpc":"2.0","id":id,"result":{"prompts":[]}}),
                _ => denied(
                    id,
                    "Conector desconectado para esta sesión en MCPs & Skills",
                ),
            }
        } else {
            denied(id, "No se pudo verificar el acceso MCP de esta sesión")
        };
        Some(Some(reply.to_string().into_bytes()))
    }

    pub(super) fn allowed(&self) -> std::io::Result<bool> {
        if !self.global_enabled()? {
            return Ok(false);
        }
        if self.unresolved {
            return Err(std::io::Error::other("identidad de panel no verificable"));
        }
        self.identity.as_deref().map_or(Ok(true), |identity| {
            comandos_store::extension_gate::enabled(&self.home, identity, &self.server)
        })
    }

    fn global_enabled(&self) -> std::io::Result<bool> {
        use std::io::Read;
        const MAX: u64 = 8 * 1024 * 1024;
        let mut bytes = Vec::new();
        std::fs::File::open(&self.catalog)?
            .take(MAX + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX {
            return Err(std::io::Error::other("catálogo MCP demasiado grande"));
        }
        let catalog = crate::config::parse_json(&bytes).map_err(std::io::Error::other)?;
        if catalog["version"] != 1 || !catalog["servers"].is_object() {
            return Err(std::io::Error::other("catálogo MCP inválido"));
        }
        let spec = &catalog["servers"][&self.server];
        Ok(spec.as_object().is_some_and(|s| !s.is_empty())
            && spec.get("enabled").is_none_or(crate::py_truthy))
    }

    /// Finish replies to calls already accepted, but do not deliver new notifications
    /// or server-initiated requests after this pane disconnects the connector.
    pub(super) fn intercept_upstream(&self, bytes: &[u8]) -> Option<Option<Vec<u8>>> {
        if matches!(self.allowed(), Ok(true)) {
            return None;
        }
        let message: Value = serde_json::from_slice(bytes).ok()?;
        message.get("method").and_then(Value::as_str)?;
        Some(message.get("id").map(|id| {
            denied(id, "Conector desconectado para esta sesión")
                .to_string()
                .into_bytes()
        }))
    }
}

fn denied(id: &Value, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32003,"message":message}})
}

fn proc_fields(pid: i32) -> Option<(i32, String)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<_> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    Some((fields.get(1)?.parse().ok()?, fields.get(19)?.to_string()))
}

fn binding_parts(binding: &Value) -> Result<(&str, &str, &str), ()> {
    let identity = binding.as_str().ok_or(())?;
    let fields: Vec<_> = identity.split('|').collect();
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let pid = |s: &str| digits(s) && s.parse::<i32>().is_ok_and(|n| n > 0);
    if identity.len() > 4096
        || identity.chars().any(char::is_control)
        || fields.len() != 6
        || !Path::new(fields[0]).is_absolute()
        || !pid(fields[1])
        || !digits(fields[2])
        || !fields[3].strip_prefix('$').is_some_and(digits)
        || !fields[4].strip_prefix('%').is_some_and(digits)
        || !pid(fields[5])
    {
        return Err(());
    }
    Ok((identity, fields[0], fields[4]))
}

/// The daemon authenticates the socket peer's UID before accepting this binding.
/// Shared app-server parents need not descend from the pane, but the exact live
/// tmux server/session/pane incarnation must still match the supplied identity.
async fn explicit_identity(binding: &Value) -> Result<String, ()> {
    let (expected, socket, pane) = binding_parts(binding)?;
    let (observed, _) = observed_identity(socket, pane).await?;
    if observed != expected {
        return Err(());
    }
    Ok(observed)
}

/// Codex deliberately filters TMUX out of MCP environments. Inspect only these two
/// variables in our same-user ancestors; credentials are neither retained nor logged.
async fn pane_identity(mut pid: i32) -> Result<Option<String>, ()> {
    let mut location = None;
    let mut ancestors = Vec::new();
    for _ in 0..32 {
        if pid <= 1 {
            break;
        }
        ancestors.push(pid);
        if let Ok(env) = std::fs::read(format!("/proc/{pid}/environ")) {
            let value = |name: &[u8]| {
                env.split(|b| *b == 0).find_map(|entry| {
                    let (key, rest) = entry.split_at(entry.iter().position(|b| *b == b'=')?);
                    (key == name)
                        .then(|| std::str::from_utf8(&rest[1..]).ok())
                        .flatten()
                })
            };
            if let (Some(tmux), Some(pane)) = (value(b"TMUX"), value(b"TMUX_PANE")) {
                if pane
                    .strip_prefix('%')
                    .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
                {
                    let (socket, _) = tmux.rsplit_once(',').ok_or(())?;
                    let (socket, _) = socket.rsplit_once(',').ok_or(())?;
                    if location.is_none() && Path::new(socket).is_absolute() {
                        location = Some((socket.to_owned(), pane.to_owned()));
                    }
                }
            }
        }
        pid = match proc_fields(pid) {
            Some((parent, _)) => parent,
            None if location.is_none() => return Ok(None),
            None => return Err(()),
        };
    }
    let Some((socket, pane)) = location else {
        return Ok(None);
    };
    let (identity, pane_pid) = observed_identity(&socket, &pane).await?;
    if !ancestors.contains(&pane_pid) {
        return Err(());
    }
    Ok(Some(identity))
}

async fn observed_identity(socket: &str, pane: &str) -> Result<(String, i32), ()> {
    let mut command = tokio::process::Command::new("tmux");
    command
        .args([
            "-S",
            socket,
            "display-message",
            "-p",
            "-t",
            pane,
            "#{socket_path}|#{pid}|#{session_id}|#{pane_id}|#{pane_pid}",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(2), command.output())
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    if !output.status.success() || output.stdout.len() > 8192 {
        return Err(());
    }
    let line = std::str::from_utf8(&output.stdout)
        .map_err(|_| ())?
        .trim_end();
    let fields: Vec<_> = line.split('|').collect();
    if fields.len() != 5 || fields[3] != pane {
        return Err(());
    }
    let pane_pid: i32 = fields[4].parse().map_err(|_| ())?;
    let server_pid: i32 = fields[1].parse().map_err(|_| ())?;
    let start = proc_fields(server_pid).ok_or(())?.1;
    Ok((
        format!(
            "{}|{}|{}|{}|{}|{}",
            fields[0], fields[1], start, fields[2], fields[3], fields[4]
        ),
        pane_pid,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_binding_requires_all_six_exact_identity_fields() {
        let valid = json!("/tmp/private/tmux.sock|123|456|$0|%7|890");
        assert_eq!(
            binding_parts(&valid).unwrap(),
            (valid.as_str().unwrap(), "/tmp/private/tmux.sock", "%7")
        );
        for invalid in [
            json!(null),
            json!(true),
            json!(""),
            json!("relative|123|456|$0|%7|890"),
            json!("/tmp/s|0|456|$0|%7|890"),
            json!("/tmp/s|123|bad|$0|%7|890"),
            json!("/tmp/s|123|456|0|%7|890"),
            json!("/tmp/s|123|456|$0|7|890"),
            json!("/tmp/s|123|456|$0|%7|890|extra"),
            json!("/tmp/s|123|456|$0|%7|890\n"),
        ] {
            assert!(binding_parts(&invalid).is_err(), "{invalid}");
        }
    }

    #[tokio::test]
    async fn invalid_explicit_binding_never_falls_back_to_unscoped_access() {
        let home = std::env::temp_dir().join(format!("cx-invalid-binding-{}", std::process::id()));
        let path = catalog(&home, true);
        for binding in [json!(null), json!(""), json!({"pane":"%1"})] {
            let access = Access::for_peer(&home, &path, None, "fixture", Some(&binding)).await;
            assert!(access.unresolved);
            assert!(access.allowed().is_err());
            assert!(
                access
                    .intercept(br#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#)
                    .is_some()
            );
        }
        let unscoped = Access::for_peer(&home, &path, None, "fixture", None).await;
        assert_eq!(unscoped.allowed().unwrap(), true);
        std::fs::remove_dir_all(home).unwrap();
    }
    fn catalog(home: &Path, enabled: bool) -> PathBuf {
        let path = home.join("catalog.json");
        std::fs::create_dir_all(home).unwrap();
        std::fs::write(
            &path,
            json!({"version":1,"servers":{"fixture":{"command":"never-run","enabled":enabled}}})
                .to_string(),
        )
        .unwrap();
        path
    }
    #[test]
    fn global_off_then_on_updates_attached_clients_and_keeps_pane_exclusions() {
        let home = std::env::temp_dir().join(format!("cx-global-gate-{}", std::process::id()));
        let path = catalog(&home, true);
        let pane = Access {
            home: home.clone(),
            catalog: path,
            identity: Some("pane-a".into()),
            server: "fixture".into(),
            unresolved: false,
        };
        let outside = Access {
            identity: None,
            ..pane.clone()
        };
        let call = br#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"echo"}}"#;
        assert!(pane.intercept(call).is_none());
        assert!(outside.intercept(call).is_none());
        catalog(&home, false);
        for access in [&pane, &outside] {
            assert_eq!(access.allowed().unwrap(), false);
            let denied = access.intercept(call).unwrap().unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&denied).unwrap()["error"]["code"],
                -32003
            );
            assert!(access.intercept(br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":9}}"#).is_none());
            assert!(
                access
                    .intercept_upstream(br#"{"jsonrpc":"2.0","id":9,"result":{}}"#)
                    .is_none()
            );
        }
        comandos_store::extension_gate::save(&home, "pane-a", &json!({"fixture":false})).unwrap();
        catalog(&home, true);
        assert!(outside.intercept(call).is_none());
        assert!(pane.intercept(call).is_some());
        comandos_store::extension_gate::save(&home, "pane-a", &json!({"fixture":true})).unwrap();
        assert!(pane.intercept(call).is_none());
        std::fs::write(home.join("catalog.json"), "{").unwrap();
        assert!(pane.allowed().is_err());
        assert!(outside.intercept(call).is_some());
        std::fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn changes_apply_to_only_one_pane_without_stopping_shared_service() {
        let home = std::env::temp_dir().join(format!("cx-gate-{}", std::process::id()));
        let path = catalog(&home, true);
        let a = Access {
            home: home.clone(),
            catalog: path.clone(),
            identity: Some("pane-a".into()),
            server: "fixture".into(),
            unresolved: false,
        };
        let b = Access {
            home: home.clone(),
            catalog: path,
            identity: Some("pane-b".into()),
            server: "fixture".into(),
            unresolved: false,
        };
        let call = br#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"echo"}}"#;
        assert!(a.intercept(call).is_none());
        comandos_store::extension_gate::save(&home, "pane-a", &json!({"fixture":false})).unwrap();
        let blocked = a.intercept(call).unwrap().unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&blocked).unwrap()["error"]["code"],
            -32003
        );
        assert!(b.intercept(call).is_none());
        assert!(
            a.intercept_upstream(
                br#"{"jsonrpc":"2.0","method":"notifications/resources/updated","params":{}}"#
            )
            .unwrap()
            .is_none()
        );
        assert!(
            a.intercept_upstream(br#"{"jsonrpc":"2.0","id":9,"result":{}}"#)
                .is_none()
        );
        assert!(
            a.intercept(br#"{"jsonrpc":"2.0","id":9,"result":{}}"#)
                .is_none()
        );
        assert!(
            a.intercept(
                br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":9}}"#
            )
            .is_none()
        );
        comandos_store::extension_gate::save(&home, "pane-a", &json!({"fixture":true})).unwrap();
        assert!(a.intercept(call).is_none());
        std::fs::remove_dir_all(home).unwrap();
    }
}
