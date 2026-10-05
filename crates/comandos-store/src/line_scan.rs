//! Lectura en flujo de una línea JSON muy larga sin guardarla: solo se anotan
//! los textos de unas pocas rutas de claves (p. ej. `type` y `payload.type`).
//! Con eso el importador decide si el Python habría ignorado la línea (un tipo
//! que no usa) sin reservar ni analizar los varios MB de una salida de
//! herramienta. Si no puede probarlo, la línea se lee entera como siempre.
//!
//! Solo importa acertar con un JSON válido: un JSON inválido el Python lo
//! ignora (`json.loads` lanza y el bucle sigue), así que descartarlo nunca
//! cambia filas. Sobre un JSON válido las capturas son exactas, con la última
//! clave repetida ganando (como `dict` en `json.loads`). Una clave o un valor
//! capturado con escapes (`\u0074ype`) no se interpreta: la línea se lee entera.

/// Lo que se vio en una ruta, con la última aparición ganando.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Seen {
    /// La ruta no está (o un contenedor del camino no es un objeto).
    Missing,
    /// Un texto corto, con sus bytes tal cual.
    Text(Vec<u8>),
    /// Un texto más largo que `MAX_CAPTURE`: no es ninguno de los que se buscan.
    Long,
    /// Un valor que no es texto (número, objeto, lista, literal).
    Other,
}

impl Seen {
    pub(crate) fn is(&self, want: &str) -> bool {
        matches!(self, Seen::Text(t) if t.as_slice() == want.as_bytes())
    }
}

/// Textos más largos no se guardan: ninguno de los buscados llega a esto.
const MAX_CAPTURE: usize = 64;

/// Rutas de claves a capturar. Ninguna es prefijo de otra.
pub(crate) type Targets = &'static [&'static [&'static str]];

/// Un objeto en el camino de alguna ruta.
struct Frame {
    path: Vec<&'static str>,
    want_key: bool,
}

/// Qué es la cadena que se está leyendo.
enum Role {
    /// Una clave de un objeto seguido.
    Key,
    /// El valor de una ruta buscada.
    Capture(usize),
    /// Cualquier otra cadena.
    Other,
}

/// Lo que dejó la última clave de un objeto seguido.
struct Pending {
    target: Option<usize>,
    child: Option<Vec<&'static str>>,
}

pub(crate) struct LineScan {
    targets: Targets,
    seen: Vec<Seen>,
    poisoned: bool,
    /// Objetos seguidos, desde la raíz.
    stack: Vec<Frame>,
    /// Contenedores anidados que no se siguen (dentro del último seguido).
    skip_depth: usize,
    started: bool,
    root_done: bool,
    pending: Option<Pending>,
    /// Dentro de una cadena: su papel, si el byte anterior fue `\` y lo leído.
    string: Option<(Role, bool)>,
    text: Vec<u8>,
    text_long: bool,
    text_escaped: bool,
}

impl LineScan {
    pub(crate) fn new(targets: Targets) -> Self {
        Self {
            targets,
            seen: vec![Seen::Missing; targets.len()],
            poisoned: false,
            stack: Vec::new(),
            skip_depth: 0,
            started: false,
            root_done: false,
            pending: None,
            string: None,
            text: Vec::new(),
            text_long: false,
            text_escaped: false,
        }
    }

    /// Las capturas, o `None` si no se pudo probar nada (escapes en una clave o
    /// un valor que importa): la línea hay que leerla entera.
    pub(crate) fn finish(self) -> Option<Vec<Seen>> {
        (!self.poisoned).then_some(self.seen)
    }

    pub(crate) fn feed(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() && !self.poisoned && !self.root_done {
            if self.string.is_some() {
                bytes = self.in_string(bytes);
                continue;
            }
            let Some((&b, rest)) = bytes.split_first() else {
                break;
            };
            bytes = rest;
            self.byte(b);
        }
    }

    /// Avanza dentro de una cadena; devuelve lo que queda tras ella (o nada).
    fn in_string<'b>(&mut self, bytes: &'b [u8]) -> &'b [u8] {
        let Some((role, escaped)) = self.string.take() else {
            return bytes;
        };
        let keep = !matches!(role, Role::Other);
        let mut escaped = escaped;
        let mut i = 0;
        while let Some(&b) = bytes.get(i) {
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            match b {
                b'\\' => {
                    escaped = true;
                    if keep {
                        self.text_escaped = true;
                    }
                }
                b'"' => {
                    if keep {
                        self.push_text(bytes.get(..i).unwrap_or_default());
                    }
                    self.end_string(role);
                    return bytes.get(i + 1..).unwrap_or_default();
                }
                _ => {}
            }
            i += 1;
        }
        if keep {
            self.push_text(bytes);
        }
        self.string = Some((role, escaped));
        &[]
    }

    fn push_text(&mut self, part: &[u8]) {
        if self.text_long {
            return;
        }
        if self.text.len() + part.len() > MAX_CAPTURE {
            self.text_long = true;
            self.text = Vec::new();
        } else {
            self.text.extend_from_slice(part);
        }
    }

    fn start_string(&mut self, role: Role) {
        self.text.clear();
        self.text_long = false;
        self.text_escaped = false;
        self.string = Some((role, false));
    }

    fn end_string(&mut self, role: Role) {
        match role {
            Role::Key => self.end_key(),
            Role::Capture(t) => {
                if self.text_escaped {
                    self.poisoned = true;
                } else if let Some(slot) = self.seen.get_mut(t) {
                    *slot = if self.text_long {
                        Seen::Long
                    } else {
                        Seen::Text(std::mem::take(&mut self.text))
                    };
                }
            }
            Role::Other => {}
        }
    }

    /// Una clave de un objeto seguido: la nueva ruta reemplaza lo que hubiera
    /// bajo ella (la última clave repetida gana).
    fn end_key(&mut self) {
        let Some(frame) = self.stack.last() else {
            return;
        };
        if self.text_escaped {
            self.poisoned = true;
            return;
        }
        let mut child = frame.path.clone();
        let key = if self.text_long {
            None
        } else {
            std::str::from_utf8(&self.text).ok()
        };
        let mut pending = Pending {
            target: None,
            child: None,
        };
        if let Some(key) = key {
            let depth = child.len();
            let mut matched: Option<&'static str> = None;
            for (t, path) in self.targets.iter().enumerate() {
                let prefix_ok = path.get(..depth) == Some(child.as_slice());
                if prefix_ok && path.get(depth) == Some(&key) {
                    matched = path.get(depth).copied();
                    if let Some(slot) = self.seen.get_mut(t) {
                        *slot = Seen::Missing;
                    }
                    if path.len() == depth + 1 {
                        pending.target = Some(t);
                    }
                }
            }
            if let Some(key) = matched {
                child.push(key);
                let longer = self.targets.iter().any(|p| {
                    p.len() > child.len() && p.get(..child.len()) == Some(child.as_slice())
                });
                if longer {
                    pending.child = Some(child);
                }
            }
        }
        self.pending = Some(pending);
    }

    /// Empieza un valor en un objeto seguido (o la raíz).
    fn begin_value(&mut self) -> Pending {
        self.pending.take().unwrap_or(Pending {
            target: None,
            child: None,
        })
    }

    fn byte(&mut self, b: u8) {
        if self.skip_depth > 0 {
            match b {
                b'"' => self.start_string(Role::Other),
                b'{' | b'[' => self.skip_depth += 1,
                b'}' | b']' => self.skip_depth -= 1,
                _ => {}
            }
            return;
        }
        if b.is_ascii_whitespace() {
            return;
        }
        if !self.started {
            self.started = true;
            match b {
                b'{' => self.stack.push(Frame {
                    path: Vec::new(),
                    want_key: true,
                }),
                // Una raíz que no es objeto: el Python ignora la línea; nada que seguir.
                _ => self.root_done = true,
            }
            return;
        }
        let Some(frame) = self.stack.last_mut() else {
            self.root_done = true;
            return;
        };
        match b {
            b'"' if frame.want_key => self.start_string(Role::Key),
            b':' => {
                frame.want_key = false;
            }
            b',' => {
                frame.want_key = true;
                self.pending = None;
            }
            b'}' | b']' => {
                self.stack.pop();
                self.pending = None;
                if self.stack.is_empty() {
                    self.root_done = true;
                }
            }
            b'"' => {
                let p = self.begin_value();
                match p.target {
                    Some(t) => self.start_string(Role::Capture(t)),
                    None => self.start_string(Role::Other),
                }
            }
            b'{' => {
                let p = self.begin_value();
                if let Some(t) = p.target
                    && let Some(slot) = self.seen.get_mut(t)
                {
                    *slot = Seen::Other;
                }
                match p.child {
                    Some(path) => self.stack.push(Frame {
                        path,
                        want_key: true,
                    }),
                    None => self.skip_depth = 1,
                }
            }
            b'[' => {
                let p = self.begin_value();
                if let Some(t) = p.target
                    && let Some(slot) = self.seen.get_mut(t)
                {
                    *slot = Seen::Other;
                }
                self.skip_depth = 1;
            }
            _ => {
                // Un número o un literal: solo cuenta su primer byte.
                if let Some(p) = self.pending.take()
                    && let Some(t) = p.target
                    && let Some(slot) = self.seen.get_mut(t)
                {
                    *slot = Seen::Other;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LineScan, Seen, Targets};

    const CODEX: Targets = &[&["type"], &["payload", "type"]];

    fn scan(line: &str, chunk: usize) -> Option<Vec<Seen>> {
        let mut s = LineScan::new(CODEX);
        for part in line.as_bytes().chunks(chunk.max(1)) {
            s.feed(part);
        }
        s.finish()
    }

    fn text(s: &str) -> Seen {
        Seen::Text(s.as_bytes().to_vec())
    }

    #[test]
    fn captures_the_last_value_of_each_path_in_any_chunking() {
        let cases: &[(&str, Option<Vec<Seen>>)] = &[
            (
                r#"{"timestamp":"x","type":"response_item","payload":{"type":"function_call_output","output":"a\"}b"}}"#,
                Some(vec![text("response_item"), text("function_call_output")]),
            ),
            // El orden no importa y la última clave repetida gana.
            (
                r#"{"payload":{"type":"task_complete","big":[1,{"type":"x"}]},"type":"event_msg"}"#,
                Some(vec![text("event_msg"), text("task_complete")]),
            ),
            (
                r#"{"type":"response_item","x":{"type":"no"},"type":"session_meta"}"#,
                Some(vec![text("session_meta"), Seen::Missing]),
            ),
            // Un `payload` nuevo borra el `type` del anterior.
            (
                r#"{"type":"event_msg","payload":{"type":"task_complete"},"payload":{"x":1}}"#,
                Some(vec![text("event_msg"), Seen::Missing]),
            ),
            (
                r#"{"type":"event_msg","payload":{"type":"task_complete"},"payload":[]}"#,
                Some(vec![text("event_msg"), Seen::Missing]),
            ),
            (
                r#" { "type" : 7 , "payload" : { "type" : null } } "#,
                Some(vec![Seen::Other, Seen::Other]),
            ),
            (
                r#"{"type":["session_meta"]}"#,
                Some(vec![Seen::Other, Seen::Missing]),
            ),
            (
                r#"["session_meta"]"#,
                Some(vec![Seen::Missing, Seen::Missing]),
            ),
            // Escapes en una clave seguida o en un valor capturado: no se prueba nada.
            (r#"{"t\u0079pe":"session_meta"}"#, None),
            (r#"{"type":"session\u005fmeta"}"#, None),
            // Una clave con escapes en un objeto seguido podría ser `type`.
            (r#"{"o\u0074ro":1,"type":"compacted"}"#, None),
            // Escapes en lo que no importa, sí.
            (
                r#"{"x":{"t\u0079pe":"session_meta"},"msg":"a\u0000\"b","type":"compacted"}"#,
                Some(vec![text("compacted"), Seen::Missing]),
            ),
        ];
        for (line, want) in cases {
            for chunk in [1, 2, 3, 7, 64, 4096] {
                assert_eq!(&scan(line, chunk), want, "{line} en trozos de {chunk}");
            }
        }
        let long = format!(r#"{{"type":"{}"}}"#, "x".repeat(100));
        assert_eq!(scan(&long, 5), Some(vec![Seen::Long, Seen::Missing]));
    }

    #[test]
    fn huge_nesting_costs_no_memory() {
        let line = format!(
            r#"{{"type":"x","a":{}{}}}"#,
            "[".repeat(1 << 20),
            "]".repeat(1 << 20)
        );
        let mut s = LineScan::new(CODEX);
        s.feed(line.as_bytes());
        assert!(s.stack.capacity() < 16);
        assert_eq!(s.finish(), Some(vec![text("x"), Seen::Missing]));
    }
}
