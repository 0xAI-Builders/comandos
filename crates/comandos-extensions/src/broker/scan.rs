//! Lectura en streaming del `id` y del `method` de primer nivel de una línea JSON-RPC, sin
//! guardar el resto: sirve para líneas de cientos de MiB (upstream) y para seguir las
//! peticiones en vuelo del cliente fino sin deserializar cada respuesta.
use serde_json::Value;

/// Bytes máximos de una clave, un `id` o un `method` de primer nivel que se recuerdan.
const FIELD: usize = 256;

#[derive(Clone, Copy, PartialEq)]
enum Target {
    Id,
    Method,
}

/// Recorre un objeto JSON y recuerda su `id` y su `method` de primer nivel. Un `"id"`
/// anidado (dentro de `result` o de una cadena) no cuenta.
#[derive(Default)]
pub(super) struct IdScan {
    depth: u32,
    in_str: bool,
    escaped: bool,
    /// En el objeto de primer nivel, el siguiente valor de cadena es una clave.
    key_next: bool,
    key: Vec<u8>,
    capturing: Option<Target>,
    value: Vec<u8>,
    id: Option<Vec<u8>>,
    method: Option<Vec<u8>>,
}

impl IdScan {
    pub(super) fn of(line: &[u8]) -> Self {
        let mut scan = Self::default();
        scan.feed(line);
        scan
    }

    pub(super) fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.byte(b);
        }
    }

    fn byte(&mut self, b: u8) {
        let top = self.depth == 1;
        if self.in_str {
            if self.capturing.is_some() {
                self.push_value(b);
            } else if top && self.key_next && self.key.len() <= FIELD {
                self.key.push(b);
            }
            if self.escaped {
                self.escaped = false;
            } else if b == b'\\' {
                self.escaped = true;
            } else if b == b'"' {
                self.in_str = false;
                if top && self.key_next && self.capturing.is_none() {
                    self.key.pop(); // comilla de cierre
                }
            }
            return;
        }
        match b {
            b'"' => {
                self.in_str = true;
                if self.capturing.is_some() {
                    self.push_value(b);
                } else if top && self.key_next {
                    self.key.clear();
                }
            }
            b'{' | b'[' => {
                if self.capturing.is_some() {
                    self.push_value(b);
                }
                self.depth += 1;
                if self.depth == 1 {
                    self.key_next = true;
                }
            }
            b'}' | b']' => {
                if top {
                    self.finish();
                } else if self.capturing.is_some() {
                    self.push_value(b);
                }
                self.depth = self.depth.saturating_sub(1);
            }
            b':' if top => {
                self.key_next = false;
                self.capturing = match self.key.as_slice() {
                    b"id" if self.id.is_none() => Some(Target::Id),
                    b"method" if self.method.is_none() => Some(Target::Method),
                    _ => None,
                };
                self.value.clear();
            }
            b',' if top => {
                self.finish();
                self.key_next = true;
            }
            _ if self.capturing.is_some() => self.push_value(b),
            _ => {}
        }
    }

    fn push_value(&mut self, b: u8) {
        if self.value.len() <= FIELD {
            self.value.push(b);
        }
    }

    fn finish(&mut self) {
        let value = std::mem::take(&mut self.value);
        match self.capturing.take() {
            Some(Target::Id) => self.id = Some(value),
            Some(Target::Method) => self.method = Some(value),
            None => {}
        }
    }

    /// `id` de primer nivel si es un número o una cadena (los únicos válidos en JSON-RPC).
    pub(super) fn id(&self) -> Option<Value> {
        let id: Value = serde_json::from_slice(self.id.as_deref()?).ok()?;
        (id.is_number() || id.is_string()).then_some(id)
    }

    /// `method` de primer nivel, si es una cadena.
    pub(super) fn method(&self) -> Option<String> {
        serde_json::from_slice(self.method.as_deref()?).ok()
    }

    pub(super) fn has_method(&self) -> bool {
        self.method.is_some()
    }

    /// Error -32603 para el `id` de una respuesta (no de un request ni una notificación del
    /// upstream, que no tienen a quién avisar). El `Mux` lo traduce y lo entrega a su dueño.
    pub(super) fn error_reply(&self) -> Option<Vec<u8>> {
        if self.has_method() {
            return None;
        }
        let id = self.id().filter(|id| id.is_u64() || id.is_string())?;
        let reply = serde_json::json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":"respuesta demasiado grande"}});
        serde_json::to_vec(&reply).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::IdScan;
    use serde_json::json;

    fn scan(line: &str) -> IdScan {
        let mut scan = IdScan::default();
        // En dos trozos partidos a mitad de la clave: el estado sobrevive entre llamadas.
        let (a, b) = line.split_at(line.len() / 2);
        scan.feed(a.as_bytes());
        scan.feed(b.as_bytes());
        scan
    }

    fn reply(line: &str) -> Option<String> {
        (scan(line).error_reply()).map(|r| String::from_utf8(r).unwrap())
    }

    #[test]
    fn finds_the_top_level_id_wherever_it_is() {
        let first = reply(r#"{"jsonrpc":"2.0","id":5,"result":{"id":9,"text":"x"}}"#).unwrap();
        assert!(first.contains(r#""id":5"#), "{first}");
        let last =
            reply(r#"{"result":{"content":[{"id":9,"t":"a\"id\":3"}]},"jsonrpc":"2.0","id":12}"#)
                .unwrap();
        assert!(last.contains(r#""id":12"#), "{last}");
        let string = reply(r#"{ "id" : "a\"b" , "result" : {} }"#).unwrap();
        assert!(string.contains(r#""id":"a\"b""#), "{string}");
        assert!(last.contains(r#""code":-32603"#) && last.contains("respuesta demasiado grande"));
    }

    #[test]
    fn requests_notifications_and_bad_ids_get_no_reply() {
        let request = r#"{"jsonrpc":"2.0","id":3,"method":"sampling/createMessage","params":{}}"#;
        assert_eq!(reply(request), None);
        let note = r#"{"jsonrpc":"2.0","method":"notifications/message","params":{"id":1}}"#;
        assert_eq!(reply(note), None);
        assert_eq!(reply(r#"{"jsonrpc":"2.0","result":{"id":1}}"#), None);
        assert_eq!(reply(r#"{"jsonrpc":"2.0","id":{"x":1},"result":{}}"#), None);
        assert_eq!(reply(r#"{"jsonrpc":"2.0","id":null,"result":{}}"#), None);
    }

    #[test]
    fn reads_method_and_id_of_a_request() {
        let s = scan(r#"{"params":{"method":"x","id":2},"method":"tools/call","id":"r-1"}"#);
        assert_eq!(s.method().as_deref(), Some("tools/call"));
        assert_eq!(s.id(), Some(json!("r-1")));
        let n = scan(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        assert_eq!(n.method().as_deref(), Some("notifications/initialized"));
        assert_eq!(n.id(), None);
    }
}
