//! Lector de `~/.ssh/config` portado de `parse_ssh_config` (`bin/cc-dash`
//! 7350) y `ssh_host_entry` (7493). Solo lectura: las altas, bajas y
//! ediciones son de quien lo consuma.
use comandos_core::text::{is_space, splitlines, strip};
use serde_json::{Map, Value};
use std::{io, path::Path};

/// `SSH_HOST_RE = ^[A-Za-z0-9._-]{1,60}\Z` (clase ASCII explícita).
pub fn is_host(s: &str) -> bool {
    (1..=60).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// `line.split(None, 1)` sobre una línea ya sin blancos a los lados: la
/// primera palabra y el resto tras los blancos que las separan.
fn split_once_ws(line: &str) -> Option<(&str, &str)> {
    let at = line.find(is_space)?;
    let (key, rest) = line.split_at(at);
    Some((key, rest.trim_start_matches(is_space)))
}

/// `parse_ssh_config(text)`: una entrada por nombre de `Host` sin `*` ni `?`,
/// con `hostname`, `user`, `port` e `identity` en el orden en que aparecen.
pub fn parse(text: &str) -> Vec<Map<String, Value>> {
    let mut hosts: Vec<Map<String, Value>> = Vec::new();
    // Índice de la entrada en curso (`cur` del Python).
    let mut cur: Option<usize> = None;
    for raw in splitlines(text) {
        let line = strip(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = split_once_ws(line) else {
            continue;
        };
        // `str.lower()` es Unicode; para las claves que importan coincide con
        // `to_lowercase` y cualquier otra clave se ignora igual.
        let key = key.to_lowercase();
        let value = strip(value);
        if key == "host" {
            cur = None;
            for name in value.split(is_space).filter(|n| !n.is_empty()) {
                if !name.contains(['*', '?']) {
                    let mut entry = Map::new();
                    entry.insert("host".into(), Value::from(name));
                    hosts.push(entry);
                    cur = Some(hosts.len() - 1);
                }
            }
            continue;
        }
        let Some(entry) = cur.and_then(|i| hosts.get_mut(i)) else {
            continue;
        };
        match key.as_str() {
            "hostname" | "user" | "port" => {
                entry.insert(key, Value::from(value));
            }
            "identityfile" => {
                entry.insert("identity".into(), Value::from(value));
            }
            _ => {}
        }
    }
    hosts
}

/// `open(os.path.expanduser("~/.ssh/config")).read()` en modo texto: `None`
/// si no existe (`FileNotFoundError`), saltos universales (`\r\n` y `\r` →
/// `\n`) y UTF-8 estricto (`UnicodeDecodeError` → `InvalidData`). Cualquier
/// otro `OSError` sube, como en el Python.
pub fn read(home: &Path) -> io::Result<Option<String>> {
    let bytes = match std::fs::read(home.join(".ssh/config")) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let text =
        String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(text.replace("\r\n", "\n").replace('\r', "\n")))
}

/// `parse_ssh_config()` sin texto: el archivo del HOME, o `[]` si no existe.
pub fn hosts(home: &Path) -> io::Result<Vec<Map<String, Value>>> {
    Ok(read(home)?.map(|text| parse(&text)).unwrap_or_default())
}

/// `ssh_host_entry(host)`: la primera entrada con ese nombre.
pub fn host_entry(home: &Path, host: &str) -> io::Result<Option<Map<String, Value>>> {
    Ok(hosts(home)?
        .into_iter()
        .find(|h| h.get("host").and_then(Value::as_str) == Some(host)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_re_is_ascii_and_bounded() {
        assert!(is_host("srv-1.a_b"));
        assert!(!is_host(""));
        assert!(!is_host(&"a".repeat(61)));
        assert!(!is_host("ñ"));
        assert!(!is_host("a b"));
        assert!(!is_host("a\n"));
    }

    #[test]
    fn split_keeps_inner_blanks() {
        assert_eq!(split_once_ws("User  a b "), Some(("User", "a b ")));
        assert_eq!(split_once_ws("Host"), None);
    }
}
