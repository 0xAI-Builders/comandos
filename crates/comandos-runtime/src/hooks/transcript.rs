//! Los dos programas jq que leen el transcript JSONL de Claude Code: la respuesta
//! completa del turno (`turn_text`) y el último bloque del asistente (para
//! `AskUserQuestion`). Cualquier error de jq (línea inválida, `.campo` sobre un
//! escalar) deja la salida vacía, igual que en el bash con `2>/dev/null`.
use super::jq::{jq_join, jq_r, jq_tostring};
use super::text::{jq_lossy, strip_nl, tail_lines};
use serde_json::Value;
use std::path::Path;

/// Error de jq: aborta el programa entero.
type Jq<T> = Result<T, ()>;
static NULL: Value = Value::Null;

/// `.campo`.
pub fn field<'a>(value: &'a Value, key: &str) -> Jq<&'a Value> {
    match value {
        Value::Object(map) => Ok(map.get(key).unwrap_or(&NULL)),
        Value::Null => Ok(&NULL),
        _ => Err(()),
    }
}

/// `.[n]`.
fn index(value: &Value, n: usize) -> Jq<&Value> {
    match value {
        Value::Array(items) => Ok(items.get(n).unwrap_or(&NULL)),
        Value::Null => Ok(&NULL),
        _ => Err(()),
    }
}

/// `.[]?`.
fn each_opt(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(map) => map.values().collect(),
        _ => Vec::new(),
    }
}

/// `a // b`.
pub fn alt<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    if matches!(a, Value::Null | Value::Bool(false)) {
        b
    } else {
        a
    }
}

/// `tail -N archivo | jq -s`: la lista de valores o error.
fn slurp(path: &Path, lines: usize) -> Jq<Vec<Value>> {
    let bytes = std::fs::read(path).unwrap_or_default();
    let text = jq_lossy(tail_lines(&bytes, lines));
    serde_json::Deserializer::from_str(&text)
        .into_iter::<Value>()
        .collect::<Result<_, _>>()
        .map_err(|_| ())
}

fn is_type(value: &Value, wanted: &str) -> Jq<bool> {
    Ok(field(value, "type")?.as_str() == Some(wanted))
}

fn is_prompt(entry: &Value) -> Jq<bool> {
    if !is_type(entry, "user")? {
        return Ok(false);
    }
    let content = field(field(entry, "message")?, "content")?;
    if content.is_string() {
        return Ok(true);
    }
    for item in each_opt(content) {
        if is_type(item, "tool_result")? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `length > 0` de jq.
fn non_empty(value: &Value) -> Jq<bool> {
    Ok(match value {
        Value::Null => false,
        Value::Bool(_) => return Err(()),
        Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    })
}

/// `turn_text`: todo el texto del asistente desde el último prompt real.
pub fn turn_text(path: &Path) -> Vec<u8> {
    let run = || -> Jq<String> {
        let entries = slurp(path, 800)?;
        let mut last_prompt = None;
        for (i, entry) in entries.iter().enumerate() {
            if is_prompt(entry)? {
                last_prompt = Some(i);
            }
        }
        let start = last_prompt.map_or(0, |u| u + 1);
        let mut texts = Vec::new();
        for entry in &entries[start.min(entries.len())..] {
            if !is_type(entry, "assistant")? {
                continue;
            }
            for item in each_opt(field(field(entry, "message")?, "content")?) {
                if is_type(item, "text")? {
                    let text = field(item, "text")?;
                    if non_empty(text)? {
                        texts.push(text.clone());
                    }
                }
            }
        }
        Ok(jq_join(&texts, "\n\n"))
    };
    run()
        .map(|s| strip_nl(s.as_bytes()).to_vec())
        .unwrap_or_default()
}

/// `tail -80 | jq -cs '[.[]|select(.type=="assistant")]|last|.message.content|last // empty'`.
pub fn last_block(path: &Path) -> Option<Value> {
    let run = || -> Jq<Option<Value>> {
        let mut last = &NULL;
        let entries = slurp(path, 80)?;
        for entry in &entries {
            if is_type(entry, "assistant")? {
                last = entry;
            }
        }
        let content = field(field(last, "message")?, "content")?;
        let block = match content {
            Value::Array(items) => items.last().unwrap_or(&NULL),
            Value::Null => &NULL,
            _ => return Err(()),
        };
        Ok((!matches!(block, Value::Null | Value::Bool(false))).then(|| block.clone()))
    };
    run().ok().flatten()
}

/// La pregunta de un `AskUserQuestion` con lo que el bash saca de ella.
pub struct Question {
    /// `.input.questions[0].question // ""`.
    pub full: Vec<u8>,
    /// Etiquetas separadas por `\x1f` (vacío si jq falla).
    pub options: Vec<u8>,
    /// Opciones numeradas con su descripción (vacío si jq falla).
    pub list: Vec<u8>,
}

/// `Some` solo si el bloque es un `AskUserQuestion` (`.name // ""`).
pub fn question(block: &Value) -> Option<Question> {
    let name = field(block, "name")
        .ok()
        .map(|n| jq_r(alt(n, &Value::String(String::new()))));
    if name.as_deref() != Some(b"AskUserQuestion".as_slice()) {
        return None;
    }
    let first = || -> Jq<&Value> { index(field(field(block, "input")?, "questions")?, 0) };
    let empty = Value::String(String::new());
    let full = first()
        .and_then(|q| field(q, "question"))
        .map(|q| jq_r(alt(q, &empty)))
        .unwrap_or_default();
    let options = || -> Jq<Vec<u8>> {
        let labels = each_opt(field(first()?, "options")?)
            .into_iter()
            .map(|o| field(o, "label").cloned())
            .collect::<Jq<Vec<_>>>()?;
        Ok(strip_nl(jq_join(&labels, "\x1f").as_bytes()).to_vec())
    };
    let list = || -> Jq<Vec<u8>> {
        let mut lines = Vec::new();
        for (key, option) in each_opt(field(first()?, "options")?)
            .into_iter()
            .enumerate()
        {
            let label = jq_tostring(field(option, "label")?);
            let mut line = format!("{}. {label}", key + 1);
            let description = alt(field(option, "description")?, &empty);
            if description != &empty {
                match field(option, "description")? {
                    Value::String(s) => line.push_str(&format!("\n   {s}")),
                    _ => return Err(()),
                }
            }
            lines.push(Value::String(line));
        }
        Ok(strip_nl(jq_join(&lines, "\n").as_bytes()).to_vec())
    };
    Some(Question {
        full,
        options: options().unwrap_or_default(),
        list: list().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write(lines: &[Value]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "comandos-transcript-{}-{}.jsonl",
            std::process::id(),
            crate::fresh_id("t").unwrap()
        ));
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn turn_text_skips_tool_results_and_joins_turn() {
        let path = write(&[
            json!({"type":"user","message":{"content":"viejo"}}),
            json!({"type":"assistant","message":{"content":[{"type":"text","text":"antes"}]}}),
            json!({"type":"user","message":{"content":[{"type":"text","text":"nuevo"}]}}),
            json!({"type":"assistant","message":{"content":[{"type":"text","text":"uno"},{"type":"tool_use","name":"Bash"}]}}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","content":"x"}]}}),
            json!({"type":"assistant","message":{"content":[{"type":"text","text":""},{"type":"text","text":"dos\n"}]}}),
        ]);
        assert_eq!(turn_text(&path), b"uno\n\ndos");
        std::fs::write(&path, "{\"type\":\"user\"}\nno json\n").unwrap();
        assert!(turn_text(&path).is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn ask_user_question_is_extracted() {
        let block = json!({"type":"tool_use","name":"AskUserQuestion","input":{"questions":[{"question":"¿Cuál?","options":[{"label":"A","description":"la a"},{"label":"B"}]}]}});
        let q = question(&block).unwrap();
        assert_eq!(q.full, "¿Cuál?".as_bytes());
        assert_eq!(q.options, b"A\x1fB");
        assert_eq!(q.list, b"1. A\n   la a\n2. B");
        assert!(question(&json!({"name":"Bash"})).is_none());
        assert!(question(&json!("texto")).is_none());
    }
}
