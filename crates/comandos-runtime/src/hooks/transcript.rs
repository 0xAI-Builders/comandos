//! Los dos programas jq que leen el transcript JSONL de Claude Code: la respuesta
//! completa del turno (`turn_text`) y el último bloque del asistente (para
//! `AskUserQuestion`). Cualquier error de jq (línea inválida, `.campo` sobre un
//! escalar) deja la salida vacía, igual que en el bash con `2>/dev/null`.
use super::jq::{jq_join, jq_r, jq_tostring};
use super::text::{jq_lossy, strip_nl, tail_lines};
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
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

/// Líneas que lee el bash: `tail -800` para el turno; el último bloque usa
/// `tail -80`, que son las últimas 80 de esas mismas 800.
pub const TURN_LINES: usize = 800;
const BLOCK_LINES: usize = 80;
const CHUNK: u64 = 64 * 1024;

/// `tail -n N archivo` leyendo hacia atrás en bloques de 64 KiB: nunca carga el
/// transcript entero (los vivos pesan cientos de MB). Vacío si no se puede leer.
pub fn read_tail(path: &Path, lines: usize) -> Vec<u8> {
    read_tail_counted(path, lines).0
}

/// `read_tail` y los bytes leídos del disco.
fn read_tail_counted(path: &Path, lines: usize) -> (Vec<u8>, u64) {
    let Ok(mut file) = File::open(path) else {
        return (Vec::new(), 0);
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return (Vec::new(), 0);
    };
    let (mut chunks, mut pos, mut newlines, mut read) = (Vec::new(), len, 0usize, 0u64);
    while pos > 0 && lines > 0 {
        let start = pos.saturating_sub(CHUNK);
        let mut chunk = vec![0u8; (pos - start) as usize];
        if file.seek(SeekFrom::Start(start)).is_err() || file.read_exact(&mut chunk).is_err() {
            return (Vec::new(), read);
        }
        read += chunk.len() as u64;
        newlines += chunk.iter().filter(|&&b| b == b'\n').count();
        // El `\n` final del archivo cierra la última línea, no separa otra.
        if pos == len && chunk.last() == Some(&b'\n') {
            newlines -= 1;
        }
        chunks.push(chunk);
        pos = start;
        if newlines >= lines {
            break;
        }
    }
    chunks.reverse();
    let buffer = chunks.concat();
    (tail_lines(&buffer, lines).to_vec(), read)
}

/// `| jq -s` sobre un `tail`: la lista de valores o error.
fn slurp(tail: &[u8], lines: usize) -> Jq<Vec<Value>> {
    let text = jq_lossy(tail_lines(tail, lines));
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

/// `turn_text` sobre las últimas `TURN_LINES` líneas (`read_tail`): todo el texto
/// del asistente desde el último prompt real.
pub fn turn_text(tail: &[u8]) -> Vec<u8> {
    let run = || -> Jq<String> {
        let entries = slurp(tail, TURN_LINES)?;
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
pub fn last_block(tail: &[u8]) -> Option<Value> {
    let run = || -> Jq<Option<Value>> {
        let mut last = &NULL;
        let entries = slurp(tail, BLOCK_LINES)?;
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
        assert_eq!(turn_text(&read_tail(&path, TURN_LINES)), b"uno\n\ndos");
        std::fs::write(&path, "{\"type\":\"user\"}\nno json\n").unwrap();
        assert!(turn_text(&read_tail(&path, TURN_LINES)).is_empty());
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

    #[test]
    fn tail_reads_backwards_and_matches_full_read() {
        let path = std::env::temp_dir().join(format!(
            "comandos-transcript-tail-{}.jsonl",
            std::process::id()
        ));
        // ~6 MiB sintéticos con líneas de largo variable.
        let line = |n: usize| {
            format!(
                "{{\"type\":\"assistant\",\"n\":{n},\"pad\":\"{}\"}}\n",
                "x".repeat(n % 700)
            )
        };
        let text: String = (0..17_000).map(line).collect();
        assert!(text.len() > 5 * 1024 * 1024);
        for (body, n) in [
            (text.clone(), TURN_LINES),
            (text.clone(), BLOCK_LINES),
            (text.trim_end().to_string(), TURN_LINES),
        ] {
            std::fs::write(&path, &body).unwrap();
            let (tail, read) = read_tail_counted(&path, n);
            let full = std::fs::read(&path).unwrap();
            assert_eq!(tail, tail_lines(&full, n), "{n}");
            assert!(
                read <= tail.len() as u64 + CHUNK,
                "leyó {read} para una cola de {}",
                tail.len()
            );
        }
        for (body, n) in [
            ("", 800),
            ("sin salto", 800),
            ("a\nb\n\n", 2),
            ("a\nb", 0),
            ("1\n2\n3\n", 800),
        ] {
            std::fs::write(&path, body).unwrap();
            assert_eq!(
                read_tail(&path, n),
                tail_lines(body.as_bytes(), n),
                "{body:?}"
            );
        }
        std::fs::remove_file(path).unwrap();
        assert!(read_tail(Path::new("/nonexistent/transcript.jsonl"), 800).is_empty());
    }
}
