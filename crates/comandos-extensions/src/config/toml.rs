//! Editable TOML plus a library-tokenized bigint compatibility layer.
//! toml_edit owns structural validation and edits; toml_parser owns token spans
//! and scalar spelling validation. No unrelated value travels through JSON.
use crate::Result;
use num_bigint::BigInt;
use num_traits::FromPrimitive;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use toml_edit::{DocumentMut, Item, Table, Value as Tv};
use toml_parser::{
    Source,
    decoder::ScalarKind,
    parser::{self, Event, EventKind, RecursionGuard, ValidateWhitespace},
};
#[derive(Clone)]
pub struct Document {
    tree: DocumentMut,
    integers: BTreeMap<String, (String, String)>,
}
fn events(text: &str) -> Result<Vec<Event>> {
    let source = Source::new(text);
    let mut ev = Vec::new();
    let mut errors = Vec::new();
    {
        let mut ws = ValidateWhitespace::new(&mut ev, source);
        let mut guard = RecursionGuard::new(&mut ws, 100);
        parser::parse_document(&source.lex().into_vec(), &mut guard, &mut errors);
    }
    if errors.is_empty() {
        Ok(ev)
    } else {
        Err("Invalid TOML".into())
    }
}
impl Document {
    pub fn parse(text: &str) -> Result<Self> {
        let source = Source::new(text);
        let mut ints = BTreeMap::new();
        let mut replacements = Vec::new();
        let ev = events(text)?;
        let mut strings = Vec::new();
        for e in &ev {
            if e.kind() == EventKind::Scalar {
                let raw = source.get(e).ok_or("Invalid TOML")?;
                let mut decoded = String::new();
                let mut errors = Vec::new();
                let kind = raw.decode_scalar(&mut decoded, &mut errors);
                if !errors.is_empty() {
                    return Err("Invalid TOML".into());
                }
                if matches!(kind, ScalarKind::String) {
                    strings.push(decoded);
                }
            }
        }
        for e in ev {
            if e.kind() != EventKind::Scalar {
                continue;
            }
            let raw = source.get(e).ok_or("Invalid TOML")?;
            let mut decoded = String::new();
            let mut errors = Vec::new();
            if let ScalarKind::Integer(radix) = raw.decode_scalar(&mut decoded, &mut errors) {
                if !errors.is_empty() {
                    return Err("Invalid TOML".into());
                }
                let integer = BigInt::parse_bytes(decoded.as_bytes(), radix.value())
                    .ok_or("Invalid TOML integer")?;
                if integer.to_string().parse::<i64>().is_err() {
                    let mut marker = format!("__comandos_bigint_{}__", ints.len());
                    while strings.contains(&marker) || text.contains(&marker) {
                        marker.push('_');
                    }
                    ints.insert(
                        marker.clone(),
                        (
                            text[e.span().start()..e.span().end()].into(),
                            integer.to_string(),
                        ),
                    );
                    replacements.push((e.span(), format!("\"{marker}\"")));
                }
            }
        }
        let mut masked = text.to_owned();
        for (span, replacement) in replacements.into_iter().rev() {
            masked.replace_range(span.start()..span.end(), &replacement);
        }
        let tree = masked.parse::<DocumentMut>().map_err(|_| "Invalid TOML")?;
        Ok(Self {
            tree,
            integers: ints,
        })
    }
    pub fn value(&self) -> Result<Value> {
        self.item(self.tree.as_item(), false, false)
    }
    fn item(&self, item: &Item, fingerprint: bool, in_array: bool) -> Result<Value> {
        match item {
            Item::None => Err("Invalid TOML item".into()),
            Item::Value(v) => self.scalar(v, fingerprint, in_array),
            Item::Table(t) => {
                let mut out = json!({});
                for (k, v) in t.iter() {
                    out[k] = self.item(v, fingerprint, false)?;
                }
                Ok(out)
            }
            Item::ArrayOfTables(a) => Ok(Value::Array(
                a.iter()
                    .map(|t| self.item(&Item::Table(t.clone()), fingerprint, false))
                    .collect::<Result<_>>()?,
            )),
        }
    }
    fn scalar(&self, v: &Tv, fingerprint: bool, in_array: bool) -> Result<Value> {
        Ok(match v {
            Tv::String(v) => match self.integers.get(v.value()) {
                Some((_, decimal)) => {
                    Value::Number(serde_json::Number::from_string_unchecked(decimal.clone()))
                }
                None => json!(v.value()),
            },
            Tv::Integer(v) => json!(v.value()),
            Tv::Float(v) => {
                let n = *v.value();
                if n.is_finite() {
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
                }
            }
            Tv::Boolean(v) => {
                if fingerprint && in_array {
                    json!(if *v.value() { "True" } else { "False" })
                } else {
                    json!(v.value())
                }
            }
            Tv::Datetime(v) => json!(python_datetime(v.value())),
            Tv::Array(a) => Value::Array(
                a.iter()
                    .map(|v| self.scalar(v, fingerprint, true))
                    .collect::<Result<_>>()?,
            ),
            Tv::InlineTable(t) => {
                let mut out = json!({});
                for (k, v) in t.iter() {
                    out[k] = self.scalar(v, fingerprint, false)?;
                }
                out
            }
        })
    }
    fn patched(&self, data: &Value) -> Result<Self> {
        let mut context = self.clone();
        let original = self.value()?;
        fn contains(v: &Value, s: &str) -> bool {
            match v {
                Value::String(v) => v == s,
                Value::Array(a) => a.iter().any(|v| contains(v, s)),
                Value::Object(o) => o.values().any(|v| contains(v, s)),
                _ => false,
            }
        }
        fn register(
            v: &Value,
            context: &mut Document,
            data: &Value,
            original: &Value,
        ) -> Result<()> {
            match v {
                Value::Number(n)
                    if n.as_i64().is_none()
                        && !n.as_str().contains(['.', 'e', 'E'])
                        && !matches!(n.as_str(), "NaN" | "Infinity" | "-Infinity") =>
                {
                    let decimal = BigInt::parse_bytes(n.as_str().as_bytes(), 10)
                        .ok_or("Invalid TOML integer")?
                        .to_string();
                    if !context.integers.values().any(|(_, n)| n == &decimal) {
                        let mut marker =
                            format!("__comandos_bigint_new_{}__", context.integers.len());
                        while contains(data, &marker)
                            || contains(original, &marker)
                            || context.integers.contains_key(&marker)
                        {
                            marker.push('_');
                        }
                        context.integers.insert(marker, (decimal.clone(), decimal));
                    }
                }
                Value::Array(a) => {
                    for v in a {
                        register(v, context, data, original)?;
                    }
                }
                Value::Object(o) => {
                    for v in o.values() {
                        register(v, context, data, original)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        register(data, &mut context, data, &original)?;
        let mut tree = context.tree.clone();
        for (k, v) in data.as_object().ok_or("Invalid configuration")? {
            let old = tree.get(k);
            if old.is_some_and(|old| {
                context
                    .item(old, false, false)
                    .is_ok_and(|old| python_equal(&old, v))
            }) {
                continue;
            }
            let item = context.make_item(v, old)?;
            tree.insert(k, item);
        }
        context.tree = tree;
        Ok(context)
    }
    fn make_item(&self, value: &Value, old: Option<&Item>) -> Result<Item> {
        if let Some(old) = old
            && python_equal(&self.item(old, false, false)?, value)
        {
            return Ok(old.clone());
        }
        if let Some(obj) = value.as_object() {
            let mut table = Table::new();
            table.set_implicit(true);
            if let Some(old) = old.and_then(Item::as_table) {
                *table.decor_mut() = old.decor().clone();
                table.set_position(old.position());
            }
            for (k, v) in obj {
                table.insert(k, self.make_item(v, old.and_then(|x| x.get(k)))?);
            }
            Ok(Item::Table(table))
        } else {
            Ok(Item::Value(self.make_value(value)?))
        }
    }
    fn make_value(&self, v: &Value) -> Result<Tv> {
        Ok(match v {
            Value::String(s) => {
                if self.integers.contains_key(s) {
                    return Err("Reserved internal TOML representation collision".into());
                }
                Tv::from(s.clone())
            }
            Value::Bool(b) => Tv::from(*b),
            Value::Number(n) => {
                if matches!(n.as_str(), "NaN" | "Infinity" | "-Infinity") {
                    Tv::from(
                        n.as_str()
                            .parse::<f64>()
                            .map_err(|_| "Invalid nonfinite float")?,
                    )
                } else if let Some(i) = n.as_i64() {
                    Tv::from(i)
                } else if let Some((marker, _)) = self
                    .integers
                    .iter()
                    .find(|(_, (_, decimal))| decimal == n.as_str())
                {
                    Tv::from(marker.clone())
                } else if let Some(f) = n.as_f64() {
                    Tv::from(f)
                } else {
                    return Err("Unsupported TOML number".into());
                }
            }
            Value::Array(a) => {
                let mut array = toml_edit::Array::new();
                for v in a {
                    array.push(self.make_value(v)?);
                }
                Tv::Array(array)
            }
            Value::Object(o) => {
                let mut table = toml_edit::InlineTable::new();
                for (k, v) in o {
                    table.insert(k, self.make_value(v)?);
                }
                Tv::InlineTable(table)
            }
            Value::Null => return Err("Unsupported TOML null".into()),
        })
    }
    pub fn render(&self, data: &Value) -> Result<String> {
        let patched = self.patched(data)?;
        let mut text = patched.tree.to_string();
        let source = Source::new(&text);
        let mut replacements = Vec::new();
        for e in events(&text)? {
            if e.kind() != EventKind::Scalar {
                continue;
            }
            let mut decoded = String::new();
            let mut errors = Vec::new();
            let kind = source
                .get(e)
                .ok_or("Invalid TOML")?
                .decode_scalar(&mut decoded, &mut errors);
            if matches!(kind, ScalarKind::String)
                && let Some((raw, _)) = patched.integers.get(&decoded)
            {
                replacements.push((e.span(), raw.clone()));
            }
        }
        for (span, raw) in replacements.into_iter().rev() {
            text.replace_range(span.start()..span.end(), &raw);
        }
        Ok(text)
    }
    pub fn fingerprint_value(&self, data: &Value) -> Result<Value> {
        {
            let patched = self.patched(data)?;
            patched.item(patched.tree.as_item(), true, false)
        }
    }
    pub fn check_fields(&self, path: &[&str], fields: &[&str]) -> Result<()> {
        let mut item = self.tree.as_item();
        for key in path {
            let Some(next) = item.get(key) else {
                return Ok(());
            };
            item = next;
        }
        for field in fields {
            if item
                .get(field)
                .is_some_and(|item| unsupported_policy(item, false))
            {
                return Err(format!(
                    "Unsupported native catalog value: {}",
                    path.last().unwrap_or(&"server")
                ));
            }
        }
        Ok(())
    }
    pub fn check_policy(&self, key: &str, name: &str, field: &str) -> Result<()> {
        if let Some(item) = self
            .tree
            .get(key)
            .and_then(|v| v.get(name))
            .and_then(|v| v.get(field))
            && unsupported_policy(item, false)
        {
            return Err(format!("Unsupported native policy value: {name}"));
        }
        Ok(())
    }
}
fn unsupported_policy(item: &Item, array: bool) -> bool {
    match item {
        Item::Value(v) => unsupported_value(v, array),
        Item::Table(t) => t.iter().any(|(_, v)| unsupported_policy(v, false)),
        Item::ArrayOfTables(t) => t
            .iter()
            .any(|t| t.iter().any(|(_, v)| unsupported_policy(v, false))),
        Item::None => false,
    }
}
fn unsupported_value(v: &Tv, array: bool) -> bool {
    match v {
        Tv::Datetime(_) => true,
        Tv::Boolean(_) => array,
        Tv::Array(a) => a.iter().any(|v| unsupported_value(v, true)),
        Tv::InlineTable(t) => t.iter().any(|(_, v)| unsupported_value(v, false)),
        _ => false,
    }
}
fn python_datetime(v: &toml_edit::Datetime) -> String {
    let mut out = String::new();
    if let Some(d) = v.date {
        out.push_str(&format!("{:04}-{:02}-{:02}", d.year, d.month, d.day));
    }
    if let Some(t) = v.time {
        if v.date.is_some() {
            out.push(' ');
        }
        out.push_str(&format!(
            "{:02}:{:02}:{:02}",
            t.hour,
            t.minute,
            t.second.unwrap_or(0)
        ));
        let micros = t.nanosecond.unwrap_or(0) / 1000;
        if micros != 0 {
            out.push_str(&format!(".{micros:06}"));
        }
    }
    if let Some(offset) = v.offset {
        let raw = offset.to_string();
        out.push_str(if raw == "Z" || raw == "-00:00" {
            "+00:00"
        } else {
            &raw
        });
    }
    out
}
pub fn python_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| python_equal(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|b| python_equal(v, b)))
        }
        (Value::Number(a), Value::Number(b)) => {
            let integer = |n: &serde_json::Number| {
                if !n.as_str().contains(['.', 'e', 'E']) {
                    BigInt::parse_bytes(n.as_str().as_bytes(), 10)
                } else {
                    None
                }
            };
            match (integer(a), integer(b)) {
                (Some(a), Some(b)) => a == b,
                (Some(integer), None) => b.as_str().parse::<f64>().ok().is_some_and(|v| {
                    v.fract() == 0.0 && BigInt::from_f64(v).is_some_and(|v| v == integer)
                }),
                (None, Some(integer)) => a.as_str().parse::<f64>().ok().is_some_and(|v| {
                    v.fract() == 0.0 && BigInt::from_f64(v).is_some_and(|v| v == integer)
                }),
                (None, None) => a
                    .as_str()
                    .parse::<f64>()
                    .ok()
                    .zip(b.as_str().parse::<f64>().ok())
                    .is_some_and(|(a, b)| a == b || a.is_nan() && b.is_nan()),
            }
        }
        (Value::Bool(a), Value::Number(b)) | (Value::Number(b), Value::Bool(a)) => {
            python_equal(&json!(if *a { 1 } else { 0 }), &Value::Number(b.clone()))
        }
        _ => a == b,
    }
}
