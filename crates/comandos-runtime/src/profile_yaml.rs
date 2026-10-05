//! Bounded PyYAML-compatible frontmatter construction over a native YAML parser.
//! Scalars retain quoted/plain style; aliases and merge keys preserve map precedence.
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use yaml_rust::{
    parser::{Event, EventReceiver, Parser},
    scanner::{TScalarStyle, TokenType},
};
static INTEGER_PATTERN: std::sync::LazyLock<std::result::Result<regex::Regex, regex::Error>> =
    std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"^([-+]?0b[0-1_]+|[-+]?0[0-7_]+|[-+]?(0|[1-9][0-9_]*)|[-+]?0x[0-9a-fA-F_]+|[-+]?[1-9][0-9_]*(:[0-5]?[0-9])+)$",
        )
    });
static FLOAT_PATTERN: std::sync::LazyLock<std::result::Result<regex::Regex, regex::Error>> =
    std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"^([-+]?[0-9][0-9_]*\.[0-9_]*([eE][-+][0-9]+)?|\.[0-9_]+([eE][-+][0-9]+)?|[-+]?[0-9][0-9_]*(:[0-5]?[0-9])+\.[0-9_]*|[-+]?\.(inf|Inf|INF)|\.(nan|NaN|NAN))$",
        )
    });
static TIMESTAMP_PATTERN: std::sync::LazyLock<std::result::Result<regex::Regex, regex::Error>> =
    std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"^([0-9]{4}-[0-9]{2}-[0-9]{2}|[0-9]{4}-[0-9]{1,2}-[0-9]{1,2}([Tt]|[ \t]+)[0-9]{1,2}:[0-9]{2}:[0-9]{2}(\.[0-9]*)?([ \t]*(Z|[-+][0-9]{1,2}(:[0-9]{2})?))?)$",
        )
    });
#[derive(Clone)]
struct Node {
    value: Value,
    merge: bool,
    weight: usize,
}
impl Node {
    fn new(value: Value) -> Self {
        Self {
            value,
            merge: false,
            weight: 1,
        }
    }
}
enum Frame {
    Sequence {
        values: Vec<Value>,
        weight: usize,
        anchor: usize,
    },
    Mapping {
        pairs: Vec<(Node, Node)>,
        key: Option<Node>,
        weight: usize,
        anchor: usize,
    },
}
#[derive(Default)]
struct Loader {
    stack: Vec<Frame>,
    anchors: HashMap<usize, Node>,
    docs: Vec<Value>,
    failed: bool,
    budget: usize,
}
impl Loader {
    fn push(&mut self, node: Node, anchor: usize) {
        self.budget = self.budget.saturating_add(node.weight);
        if self.budget > 50000 {
            self.failed = true;
            return;
        }
        if anchor > 0 {
            self.anchors.insert(anchor, node.clone());
        }
        match self.stack.last_mut() {
            Some(Frame::Sequence { values, weight, .. }) => {
                *weight = weight.saturating_add(node.weight);
                values.push(node.value)
            }
            Some(Frame::Mapping {
                pairs, key, weight, ..
            }) => {
                *weight = weight.saturating_add(node.weight);
                if let Some(k) = key.take() {
                    pairs.push((k, node));
                } else {
                    *key = Some(node)
                }
            }
            None => self.docs.push(node.value),
        }
    }
    fn finish_mapping(&mut self, pairs: Vec<(Node, Node)>, weight: usize, anchor: usize) {
        let mut merged = Map::new();
        let mut explicit = Map::new();
        for (k, v) in pairs {
            if k.merge {
                let maps = match v.value {
                    Value::Object(m) => vec![m],
                    Value::Array(a) => {
                        let mut maps = vec![];
                        for v in a {
                            if let Value::Object(m) = v {
                                maps.push(m)
                            } else {
                                self.failed = true;
                                return;
                            }
                        }
                        maps
                    }
                    _ => {
                        self.failed = true;
                        return;
                    }
                };
                for map in maps {
                    for (k, v) in map {
                        merged.entry(k).or_insert(v);
                    }
                }
            } else if let Value::String(k) = k.value {
                explicit.insert(k, v.value);
            } else if k.value.is_array() || k.value.is_object() {
                self.failed = true;
                return;
            }
        }
        merged.extend(explicit);
        self.push(
            Node {
                value: merged.into(),
                merge: false,
                weight,
            },
            anchor,
        )
    }
}
impl EventReceiver for Loader {
    fn on_event(&mut self, event: Event) {
        if self.stack.len() > 256 {
            self.failed = true;
            return;
        }
        if self.failed {
            return;
        }
        match event {
            Event::Scalar(text, style, anchor, tag) => match scalar(&text, style, tag) {
                Some(node) => self.push(node, anchor),
                None => self.failed = true,
            },
            Event::Alias(id) => self.push(
                self.anchors
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| Node::new(Value::Null)),
                0,
            ),
            Event::SequenceStart(anchor) => self.stack.push(Frame::Sequence {
                values: vec![],
                weight: 1,
                anchor,
            }),
            Event::SequenceEnd => match self.stack.pop() {
                Some(Frame::Sequence {
                    values,
                    weight,
                    anchor,
                }) => self.push(
                    Node {
                        value: values.into(),
                        merge: false,
                        weight,
                    },
                    anchor,
                ),
                _ => self.failed = true,
            },
            Event::MappingStart(anchor) => self.stack.push(Frame::Mapping {
                pairs: vec![],
                key: None,
                weight: 1,
                anchor,
            }),
            Event::MappingEnd => match self.stack.pop() {
                Some(Frame::Mapping {
                    pairs,
                    key: None,
                    weight,
                    anchor,
                }) => self.finish_mapping(pairs, weight, anchor),
                _ => self.failed = true,
            },
            _ => {}
        }
    }
}
fn matches(
    pattern: &std::sync::LazyLock<std::result::Result<regex::Regex, regex::Error>>,
    text: &str,
) -> bool {
    pattern.as_ref().is_ok_and(|r| r.is_match(text))
}
pub(crate) fn integer(text: &str) -> Option<Value> {
    let text = text.replace('_', "");
    let negative = text.starts_with('-');
    let body = text.trim_start_matches(['-', '+']);
    // Decimal digits keep Python's arbitrary precision, including base-prefixed
    // and sexagesimal values, without narrowing through a machine integer.
    let mut digits = vec![0u8];
    fn step(digits: &mut Vec<u8>, base: u32, value: u32) {
        let mut carry = value;
        for d in digits.iter_mut().rev() {
            carry += u32::from(*d) * base;
            *d = (carry % 10) as u8;
            carry /= 10;
        }
        while carry > 0 {
            digits.insert(0, (carry % 10) as u8);
            carry /= 10;
        }
    }
    if body.contains(':') {
        for part in body.split(':') {
            step(&mut digits, 60, part.parse::<u32>().ok()?);
        }
    } else {
        let (base, body) = if let Some(v) = body.strip_prefix("0b") {
            (2, v)
        } else if let Some(v) = body.strip_prefix("0x") {
            (16, v)
        } else if body.starts_with('0') && body.len() > 1 {
            (8, body)
        } else {
            (10, body)
        };
        if body.is_empty() {
            return None;
        }
        for ch in body.chars() {
            step(&mut digits, base, ch.to_digit(base)?);
        }
    }
    while digits.len() > 1 && digits[0] == 0 {
        digits.remove(0);
    }
    let mut out = String::new();
    if negative && digits.iter().any(|d| *d != 0) {
        out.push('-')
    }
    for d in digits {
        out.push(char::from(b'0' + d));
    }
    Some(Value::Number(serde_json::Number::from_string_unchecked(
        out,
    )))
}

fn floating(text: &str) -> Option<Value> {
    let text = text.replace('_', "");
    let lower = text.to_lowercase();
    let n = if matches!(lower.as_str(), ".inf" | "+.inf") {
        f64::INFINITY
    } else if lower == "-.inf" {
        f64::NEG_INFINITY
    } else if lower == ".nan" {
        f64::NAN
    } else if text.contains(':') {
        let sign = if text.starts_with('-') { -1.0 } else { 1.0 };
        let mut n = 0.0;
        for part in text.trim_start_matches(['-', '+']).split(':') {
            n = n * 60.0 + part.parse::<f64>().ok()?;
        }
        n * sign
    } else {
        text.parse().ok()?
    };
    Some(if n.is_finite() {
        json!(n)
    } else {
        Value::Number(serde_json::Number::from_string_unchecked(
            if n.is_nan() {
                "NaN"
            } else if n.is_sign_positive() {
                "Infinity"
            } else {
                "-Infinity"
            }
            .into(),
        ))
    })
}
fn timestamp(text: &str) -> Option<Value> {
    use chrono::Datelike;
    let date = text.split(['T', 't', ' ', '\t']).next()?;
    let d = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    if d.year() < 1 {
        return None;
    }
    if text.len() > date.len() {
        let rest = text[date.len()..].trim_start_matches(['T', 't', ' ', '\t']);
        let time = rest.split(['Z', '+', '-', ' ', '\t']).next()?;
        let mut parts = time.split(':');
        let hour = parts.next()?.parse::<u32>().ok()?;
        let minute = parts.next()?.parse::<u32>().ok()?;
        let second = parts.next()?.split('.').next()?.parse::<u32>().ok()?;
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
    }
    Some(Value::Null)
}
fn scalar(text: &str, style: TScalarStyle, tag: Option<TokenType>) -> Option<Node> {
    let explicit = match tag {
        Some(TokenType::Tag(handle, suffix)) => {
            if handle == "!!" {
                Some(suffix)
            } else if handle.is_empty() {
                Some(suffix.strip_prefix("tag:yaml.org,2002:")?.into())
            } else {
                return None;
            }
        }
        None => None,
        _ => return None,
    };
    if explicit.as_deref() == Some("str") || (explicit.is_none() && style != TScalarStyle::Plain) {
        return Some(Node::new(text.into()));
    }
    let boolean = match text {
        "yes" | "Yes" | "YES" | "true" | "True" | "TRUE" | "on" | "On" | "ON" => Some(true),
        "no" | "No" | "NO" | "false" | "False" | "FALSE" | "off" | "Off" | "OFF" => Some(false),
        _ => None,
    };

    let value = if let Some(tag) = explicit {
        match tag.as_str() {
            "bool" => boolean?.into(),
            "int" => integer(text)?,
            "float" => floating(text)?,
            "null" => Value::Null,
            "timestamp" => timestamp(text)?,
            "binary" => {
                use base64::Engine;
                let clean = text
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>();
                base64::engine::general_purpose::STANDARD
                    .decode(clean)
                    .ok()?;
                Value::Null
            }
            "merge" => {
                return Some(Node {
                    value: json!("<<"),
                    merge: true,
                    weight: 1,
                });
            }
            _ => return None,
        }
    } else if let Some(b) = boolean {
        b.into()
    } else if matches!(text, "" | "~" | "null" | "Null" | "NULL") {
        Value::Null
    } else if matches(&INTEGER_PATTERN, text) {
        integer(text)?
    } else if matches(&FLOAT_PATTERN, text) {
        floating(text)?
    } else if matches(&TIMESTAMP_PATTERN, text) {
        timestamp(text)?
    } else if text == "<<" {
        return Some(Node {
            value: json!("<<"),
            merge: true,
            weight: 1,
        });
    } else {
        text.into()
    };
    Some(Node::new(value))
}
pub(crate) fn parse(text: &str) -> Value {
    let mut loader = Loader::default();
    let mut parser = Parser::new(text.chars());
    if parser.load(&mut loader, true).is_err() || loader.failed || loader.docs.len() != 1 {
        return json!({});
    }
    loader
        .docs
        .pop()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}
