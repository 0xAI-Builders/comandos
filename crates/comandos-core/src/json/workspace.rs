//! Python workspace JSON read policy, separate from bounded metadata parsing.
use serde_json::{Map, Number, Value};

/// Workspace persistence ceiling, counted in nested arrays and objects.
/// This protects callers that clone or drop recursively owned serde Values.
/// It is separate from the extension metadata encoder's 128-level policy.
pub const MAX_WORKSPACE_JSON_DEPTH: usize = 1000;

/// Validate borrowed selected fields before a caller clones recursive Values.
/// `enclosing` counts containers in the final document above this value.
pub(crate) fn validate_workspace_depth(value: &Value, enclosing: usize) -> Result<(), String> {
    enum Children<'a> {
        Array(std::slice::Iter<'a, Value>),
        Object(serde_json::map::Values<'a>),
    }
    impl<'a> Children<'a> {
        fn next(&mut self) -> Option<&'a Value> {
            match self {
                Self::Array(values) => values.next(),
                Self::Object(values) => values.next(),
            }
        }
    }
    let mut frames = vec![(
        Children::Array(std::slice::from_ref(value).iter()),
        enclosing,
    )];
    while let Some((mut children, depth)) = frames.pop() {
        if let Some(value) = children.next() {
            frames.push((children, depth));
            let descendants = match value {
                Value::Array(values) => Some(Children::Array(values.iter())),
                Value::Object(values) => Some(Children::Object(values.values())),
                _ => None,
            };
            if let Some(descendants) = descendants {
                if depth >= MAX_WORKSPACE_JSON_DEPTH {
                    return Err("Workspace JSON nesting limit reached".into());
                }
                frames.push((descendants, depth + 1));
            }
        }
    }
    Ok(())
}

/// Read persisted Python workspace JSON, including NaN and signed Infinity.
/// Containers use heap frames and the workspace ceiling of 1000 containers.
/// Keys are ordinary strings, and duplicate keys retain their final value.
pub fn workspace_loads(raw: &str) -> Result<Value, serde_json::Error> {
    enum Frame {
        Array(Vec<Value>),
        Object(Map<String, Value>, String),
    }
    let mut input = Reader { raw, position: 0 };
    let mut frames = Vec::new();
    loop {
        let mut value = match input.peek() {
            Some(b'[' | b'{') if frames.len() >= MAX_WORKSPACE_JSON_DEPTH => {
                return Err(<serde_json::Error as serde::de::Error>::custom(
                    "Workspace JSON nesting limit reached",
                ));
            }
            Some(b'[') => {
                input.position += 1;
                if input.peek() == Some(b']') {
                    input.position += 1;
                    Value::Array(Vec::new())
                } else {
                    frames.push(Frame::Array(Vec::new()));
                    continue;
                }
            }
            Some(b'{') => {
                input.position += 1;
                if input.peek() == Some(b'}') {
                    input.position += 1;
                    Value::Object(Map::new())
                } else {
                    let key = input.key()?;
                    frames.push(Frame::Object(Map::new(), key));
                    continue;
                }
            }
            Some(b'"') => Value::String(input.string()?),
            _ => input.atom()?,
        };
        loop {
            let Some(frame) = frames.pop() else {
                return if input.peek().is_none() {
                    Ok(value)
                } else {
                    Err(input.error())
                };
            };
            match frame {
                Frame::Array(mut values) => {
                    values.push(value);
                    match input.peek() {
                        Some(b']') => {
                            input.position += 1;
                            value = Value::Array(values);
                        }
                        Some(b',') => {
                            input.position += 1;
                            frames.push(Frame::Array(values));
                            break;
                        }
                        _ => return Err(input.error()),
                    }
                }
                Frame::Object(mut values, key) => {
                    values.insert(key, value);
                    match input.peek() {
                        Some(b'}') => {
                            input.position += 1;
                            value = Value::Object(values);
                        }
                        Some(b',') => {
                            input.position += 1;
                            let key = input.key()?;
                            frames.push(Frame::Object(values, key));
                            break;
                        }
                        _ => return Err(input.error()),
                    }
                }
            }
        }
    }
}

struct Reader<'a> {
    raw: &'a str,
    position: usize,
}
impl Reader<'_> {
    fn error(&self) -> serde_json::Error {
        <serde_json::Error as serde::de::Error>::custom(format!(
            "Invalid workspace JSON at byte {}",
            self.position
        ))
    }
    fn peek(&mut self) -> Option<u8> {
        let bytes = self.raw.as_bytes();
        while bytes
            .get(self.position)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
        {
            self.position += 1;
        }
        bytes.get(self.position).copied()
    }
    fn key(&mut self) -> Result<String, serde_json::Error> {
        let key = self.string()?;
        if self.peek() != Some(b':') {
            return Err(self.error());
        }
        self.position += 1;
        Ok(key)
    }
    fn string(&mut self) -> Result<String, serde_json::Error> {
        if self.peek() != Some(b'"') {
            return Err(self.error());
        }
        let start = self.position;
        self.position += 1;
        while let Some(byte) = self.raw.as_bytes().get(self.position) {
            self.position += 1;
            match byte {
                b'"' => return serde_json::from_str(&self.raw[start..self.position]),
                b'\\' => self.position += 1,
                _ => {}
            }
        }
        Err(self.error())
    }
    fn atom(&mut self) -> Result<Value, serde_json::Error> {
        self.peek();
        let start = self.position;
        while self.raw.as_bytes().get(self.position).is_some_and(|byte| {
            !matches!(
                byte,
                b' ' | b'\t' | b'\r' | b'\n' | b',' | b':' | b'[' | b']' | b'{' | b'}'
            )
        }) {
            self.position += 1;
        }
        let token = &self.raw[start..self.position];
        match token {
            "null" => Ok(Value::Null),
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            "NaN" | "Infinity" | "-Infinity" => {
                Ok(Value::Number(Number::from_string_unchecked(token.into())))
            }
            "-0" => Ok(Value::Number(Number::from(0))),
            _ => {
                let mut number: Number = token.parse()?;
                if token.contains(['.', 'e', 'E']) {
                    let float = token.parse::<f64>().map_err(|_| self.error())?;
                    number = Number::from_f64(float).unwrap_or_else(|| {
                        Number::from_string_unchecked(
                            if float.is_sign_negative() {
                                "-Infinity"
                            } else {
                                "Infinity"
                            }
                            .into(),
                        )
                    });
                }
                Ok(Value::Number(number))
            }
        }
    }
}
