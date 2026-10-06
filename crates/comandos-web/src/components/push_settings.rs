pub const ENABLED_KEY: &str = "comandos.push.enabled";
pub const DEVICE_KEY: &str = "comandos.deviceId";
pub const DENIED_HELP: &str = "El permiso de notificaciones esta bloqueado para este sitio. Activalo en los ajustes del navegador (candado de la barra de direcciones > Notificaciones) y vuelve a pulsar Activar.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Support {
    pub ok: bool,
    pub reason: &'static str,
}

pub fn support(secure: bool, service_worker: bool, push_manager: bool, notification: bool) -> Support {
    if !secure {
        return Support {
            ok: false,
            reason: "Los avisos push necesitan abrir CommandOS por HTTPS (la URL de Tailscale).",
        };
    }
    if !service_worker {
        return Support {
            ok: false,
            reason: "Este navegador no tiene service worker.",
        };
    }
    if !push_manager {
        return Support {
            ok: false,
            reason: "Este navegador no soporta avisos push.",
        };
    }
    if !notification {
        return Support {
            ok: false,
            reason: "Este navegador no permite notificaciones.",
        };
    }
    Support { ok: true, reason: "" }
}

pub fn url_base64_to_bytes(value: &str) -> Vec<u8> {
    let mut s = value.replace('-', "+").replace('_', "/");
    while !s.len().is_multiple_of(4) {
        s.push('=');
    }
    base64_decode(&s).unwrap_or_default()
}

fn base64_val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let a = *chunk.first()?;
        let b = *chunk.get(1)?;
        let c = *chunk.get(2).unwrap_or(&b'=');
        let d = *chunk.get(3).unwrap_or(&b'=');
        let v0 = base64_val(a)? as u32;
        let v1 = base64_val(b)? as u32;
        let v2 = if c == b'=' { 0 } else { base64_val(c)? as u32 };
        let v3 = if d == b'=' { 0 } else { base64_val(d)? as u32 };
        let n = (v0 << 18) | (v1 << 12) | (v2 << 6) | v3;
        out.push(((n >> 16) & 0xff) as u8);
        if c != b'=' {
            out.push(((n >> 8) & 0xff) as u8);
        }
        if d != b'=' {
            out.push((n & 0xff) as u8);
        }
    }
    Some(out)
}

pub fn event_url_after_routing(path: &str, query: &str, hash: &str) -> (Option<String>, String) {
    let mut kept = Vec::new();
    let mut event = None;
    for pair in query.trim_start_matches('?').split('&').filter(|s| !s.is_empty()) {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        if key == "event" {
            event = Some(percent_decode(value).chars().take(128).collect());
        } else {
            kept.push(pair.to_string());
        }
    }
    let qs = if kept.is_empty() {
        String::new()
    } else {
        format!("?{}", kept.join("&"))
    };
    (event, format!("{path}{qs}{hash}"))
}

fn percent_decode(s: &str) -> String {
    let mut out = String::new();
    let mut bytes = s.as_bytes().iter().copied();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let hi = bytes.next();
            let lo = bytes.next();
            if let (Some(hi), Some(lo)) = (hi, lo)
                && let (Some(h), Some(l)) = (hex(hi), hex(lo))
            {
                out.push(char::from(h * 16 + l));
                continue;
            }
        }
        out.push(if b == b'+' { ' ' } else { char::from(b) });
    }
    out
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(target_arch = "wasm32")]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    use comandos_web_dom::bridge::global_set;
    use js_sys::Object;
    global_set("PushSettings", &Object::new().into())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{event_url_after_routing, support, url_base64_to_bytes};

    #[test]
    fn insecure_origins_and_missing_support_are_explained_not_prompted() {
        let r = support(false, true, true, true);
        assert!(!r.ok);
        assert!(r.reason.contains("HTTPS"));
        assert!(!support(true, false, true, true).ok);
        assert!(!support(true, true, false, true).ok);
        assert!(!support(true, true, true, false).ok);
    }

    #[test]
    fn vapid_key_decodes_to_65_bytes() {
        let key = "BOrKqD0jdhZ4Ta4Q2S7XqVy9YkKxzwP3xqg7r5RkVY8H8Yb3x8o5JH3fSxPp9QmW1dYkGiJ0rJ6nq3M0Qm2a1sQ";
        assert_eq!(url_base64_to_bytes(key).len(), 65);
    }

    #[test]
    fn event_id_is_routed_once_and_removed_from_address_bar() {
        let (event, url) = event_url_after_routing("/", "?event=ev%2042&x=1", "");
        assert_eq!(event.as_deref(), Some("ev 42"));
        assert_eq!(url, "/?x=1");
    }
}
