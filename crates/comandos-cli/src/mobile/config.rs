//! NodeServeConfig schema, ownership and pure mutation plan. Services stay intact.
use serde_json::{Map, Value, json};

pub(super) const FRONT: &str = "http://127.0.0.1:4777";
fn object<'a>(v: &'a Value, fields: &[&str], what: &str) -> Result<&'a Map<String, Value>, String> {
    let o = v
        .as_object()
        .ok_or_else(|| format!("{what}: se necesita objeto JSON"))?;
    if let Some(k) = o.keys().find(|k| !fields.contains(&k.as_str())) {
        return Err(format!("{what}: campo desconocido {k}"));
    }
    Ok(o)
}
fn port(s: &str) -> Result<u16, String> {
    s.parse::<u16>()
        .ok()
        .filter(|p| *p > 0 && p.to_string() == s)
        .ok_or_else(|| format!("puerto inválido: {s}"))
}
fn host_port(s: &str) -> Result<u16, String> {
    let (host, p) = s
        .rsplit_once(':')
        .ok_or_else(|| format!("host:puerto inválido: {s}"))?;
    if host.is_empty()
        || host
            .chars()
            .any(|c| c.is_whitespace() || c == '/' || c == ':')
    {
        return Err(format!("host inválido: {s}"));
    }
    port(p)
}
fn string(o: &Map<String, Value>, key: &str) -> Result<(), String> {
    if o.get(key).is_some_and(|v| !v.is_string()) {
        return Err(format!("{key}: se necesita texto"));
    }
    Ok(())
}
fn boolean(o: &Map<String, Value>, key: &str) -> Result<(), String> {
    if o.get(key).is_some_and(|v| !v.is_boolean()) {
        return Err(format!("{key}: se necesita booleano"));
    }
    Ok(())
}
fn map<'a>(o: &'a Map<String, Value>, key: &str) -> Result<Option<&'a Map<String, Value>>, String> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_object()
            .map(Some)
            .ok_or_else(|| format!("{key}: se necesita mapa")),
    }
}
fn node(v: &Value, depth: usize) -> Result<(), String> {
    if depth > 1 {
        return Err("Foreground anidado no soportado".into());
    }
    let o = object(
        v,
        &["TCP", "Web", "Services", "AllowFunnel", "Foreground"],
        "NodeServeConfig",
    )?;
    endpoint(o)?;
    if let Some(f) = map(o, "AllowFunnel")? {
        for (k, v) in f {
            host_port(k)?;
            if !v.is_boolean() {
                return Err("AllowFunnel necesita booleanos".into());
            }
        }
    }
    if let Some(s) = map(o, "Services")? {
        for (k, v) in s {
            if !k.starts_with("svc:") || k.len() <= 4 {
                return Err("Services: nombre inválido".into());
            }
            let o = object(v, &["TCP", "Web", "Tun"], "ServiceConfig")?;
            boolean(o, "Tun")?;
            endpoint(o)?;
        }
    }
    if let Some(f) = map(o, "Foreground")? {
        for v in f.values() {
            node(v, depth + 1)?;
        }
    }
    Ok(())
}
fn endpoint(o: &Map<String, Value>) -> Result<(), String> {
    if let Some(tcp) = map(o, "TCP")? {
        for (p, v) in tcp {
            port(p)?;
            let o = object(
                v,
                &[
                    "HTTPS",
                    "HTTP",
                    "TCPForward",
                    "TerminateTLS",
                    "ProxyProtocol",
                ],
                "TCPPortHandler",
            )?;
            boolean(o, "HTTPS")?;
            boolean(o, "HTTP")?;
            string(o, "TCPForward")?;
            string(o, "TerminateTLS")?;
            if o.get("ProxyProtocol")
                .is_some_and(|v| v.as_u64().is_none_or(|n| n > 2))
            {
                return Err("ProxyProtocol inválido".into());
            }
        }
    }
    if let Some(web) = map(o, "Web")? {
        for (hp, v) in web {
            host_port(hp)?;
            let o = object(v, &["Handlers"], "WebServerConfig")?;
            let handlers = map(o, "Handlers")?.ok_or("Handlers ausente")?;
            for (path, v) in handlers {
                if !path.starts_with('/') || path.chars().any(char::is_control) {
                    return Err("ruta de handler inválida".into());
                }
                let o = object(
                    v,
                    &["Path", "Proxy", "Text", "AcceptAppCaps", "Redirect"],
                    "HTTPHandler",
                )?;
                for key in ["Path", "Proxy", "Text", "Redirect"] {
                    string(o, key)?;
                }
                if o.get("AcceptAppCaps").is_some_and(|v| {
                    v.as_array()
                        .is_none_or(|a| a.iter().any(|v| !v.is_string()))
                }) {
                    return Err("AcceptAppCaps inválido".into());
                }
            }
        }
    }
    Ok(())
}
pub(super) fn parse(bytes: &[u8]) -> Result<Value, String> {
    let mut v: Value =
        serde_json::from_slice(bytes).map_err(|e| format!("NodeServeConfig inválido: {e}"))?;
    if v.is_null() {
        v = json!({});
    }
    node(&v, 0)?;
    Ok(v)
}
fn owned(v: &Value) -> bool {
    v.as_object().is_some_and(|o| o.len() == 1)
        && v.get("Proxy").and_then(Value::as_str).is_some_and(|p| {
            matches!(
                p,
                "http://127.0.0.1:4777"
                    | "http://127.0.0.1:4777/"
                    | "http://127.0.0.1:4779"
                    | "http://127.0.0.1:4779/"
                    | "http://127.0.0.1:4780/term"
                    | "http://127.0.0.1:4780/term/"
            )
        })
}
pub(super) fn safe_to_change(v: &Value) -> Result<(), String> {
    if v.get("Foreground")
        .and_then(Value::as_object)
        .is_some_and(|f| !f.is_empty())
    {
        return Err("Serve tiene configuración Foreground; no se modifica".into());
    }
    if v.get("TCP").and_then(Value::as_object).is_some_and(|tcp| {
        tcp.values().any(|handler| {
            handler
                .get("TCPForward")
                .and_then(Value::as_str)
                .is_some_and(|s| {
                    s.rsplit_once(':').is_some_and(|(host, p)| {
                        matches!(host, "127.0.0.1" | "localhost")
                            && matches!(p, "4777" | "4779" | "4780")
                    })
                })
        })
    }) {
        return Err("TCP directo a backend ComandOS fuera del contrato; no se modifica".into());
    }
    if let Some(f) = v.get("AllowFunnel").and_then(Value::as_object) {
        for (hp, allow) in f {
            if allow == true
                && (host_port(hp)? == 443
                    || v.get("Web")
                        .and_then(|v| v.get(hp))
                        .and_then(|v| v.get("Handlers"))
                        .and_then(Value::as_object)
                        .is_some_and(|h| h.values().any(owned)))
            {
                return Err(format!(
                    "AllowFunnel activo en {hp}: se rechaza exponer ComandOS públicamente"
                ));
            }
        }
    }
    if let Some(web) = v.get("Web").and_then(Value::as_object) {
        for server in web.values() {
            if let Some(handlers) = server.get("Handlers").and_then(Value::as_object) {
                for h in handlers.values() {
                    if h.get("Proxy")
                        .and_then(Value::as_str)
                        .is_some_and(known_backend)
                        && !owned(h)
                    {
                        return Err("handler ComandOS ambiguo; no se sobrescribe".into());
                    }
                }
            }
        }
    }
    Ok(())
}
fn known_backend(s: &str) -> bool {
    url::Url::parse(s).ok().is_some_and(|u| {
        matches!(u.host_str(), Some("127.0.0.1" | "localhost"))
            && matches!(u.port(), Some(4777 | 4779 | 4780))
    })
}
pub(super) fn plan(before: &Value, host: &str, on: bool) -> Result<Value, String> {
    safe_to_change(before)?;
    let hp = format!("{host}:443");
    if on {
        if let Some(tcp) = before.get("TCP").and_then(|v| v.get("443"))
            && *tcp != json!({"HTTPS":true})
        {
            return Err("puerto 443 ocupado por configuración ajena o desconocida".into());
        }
        if let Some(handlers) = before
            .get("Web")
            .and_then(|v| v.get(&hp))
            .and_then(|v| v.get("Handlers"))
            .and_then(Value::as_object)
        {
            for (path, v) in handlers {
                if (path == "/" || path == "/term" || path.starts_with("/term/")) && !owned(v) {
                    return Err(format!("ruta ajena {hp}{path}: no se sobrescribe"));
                }
            }
        }
    }
    let mut after = before.clone();
    let o = after.as_object_mut().ok_or("NodeServeConfig inválido")?;
    let mut removed_ports = Vec::new();
    let mut removed_keys = Vec::new();
    if let Some(web) = o.get_mut("Web").and_then(Value::as_object_mut) {
        for (hp, v) in web.iter_mut() {
            let handlers = v
                .get_mut("Handlers")
                .and_then(Value::as_object_mut)
                .ok_or("Handlers inválido")?;
            let count = handlers.len();
            handlers.retain(|_, v| !owned(v));
            if count != handlers.len() && handlers.is_empty() {
                removed_ports.push(host_port(hp)?.to_string());
                removed_keys.push(hp.clone());
            }
        }
        web.retain(|key, _| !removed_keys.contains(key));
    }
    for p in removed_ports {
        let in_use = o.get("Web").and_then(Value::as_object).is_some_and(|w| {
            w.keys()
                .any(|hp| hp.rsplit_once(':').is_some_and(|(_, port)| port == p))
        });
        if !in_use
            && let Some(tcp) = o.get_mut("TCP").and_then(Value::as_object_mut)
            && tcp.get(&p).is_some_and(|v| *v == json!({"HTTPS":true}))
        {
            tcp.remove(&p);
        }
    }
    if on {
        let tcp = o.entry("TCP").or_insert_with(|| json!({}));
        if tcp.is_null() {
            *tcp = json!({});
        }
        tcp.as_object_mut()
            .ok_or("TCP inválido")?
            .insert("443".into(), json!({"HTTPS":true}));
        let web = o.entry("Web").or_insert_with(|| json!({}));
        if web.is_null() {
            *web = json!({});
        }
        let entry = web
            .as_object_mut()
            .ok_or("Web inválido")?
            .entry(hp)
            .or_insert_with(|| json!({"Handlers":{}}));
        entry
            .get_mut("Handlers")
            .and_then(Value::as_object_mut)
            .ok_or("Handlers inválido")?
            .insert("/".into(), json!({"Proxy":FRONT}));
    }
    // Go's NodeServeConfig uses omitempty for empty maps.
    for key in ["TCP", "Web"] {
        if o.get(key)
            .and_then(Value::as_object)
            .is_some_and(|m| m.is_empty())
        {
            o.remove(key);
        }
    }
    Ok(after)
}
