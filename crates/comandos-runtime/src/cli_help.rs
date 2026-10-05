//! Arranques de cada CLI leídos de su propio `<cli> --help`: port de
//! `lib/cli_help.py` (`parse_help`, `_item`, `_values`) y de la parte pura de
//! `help_for` (cómo se arma el texto y la llave de su caché).
//!
//! Las expresiones son las del Python con las clases de `re` sobre `str`. Las
//! de Rust coinciden con ellas en todo carácter ASCII salvo U+001C–U+001F
//! (blancos para Python, no para Rust); fuera de ASCII, `\w` difiere en marcas,
//! conectores y números que no son dígitos. Un texto con cualquiera de esos
//! caracteres no se interpreta: `Unsure` (quien llama declina).
use crate::Unsure;
use comandos_core::text::{is_space, splitlines, strip};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::LazyLock,
};

fn compile(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

static RUST_WORD: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"^\w$"));
// `\w` de `re` para `str`: `str.isalnum()` (letras L* y números N*) o `_`.
static PY_WORD: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"^[\p{L}\p{N}_]$"));
static ANSI: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]"));
static SECTION: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"^([A-Z][\w ]*?(?: of [\w-]+)?):\s*$"));
static YOLO: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile(
        r"(?i)\b(?:bypass\w*|skip) all (?:permission|confirmation)|\bauto-approve (?:all|permissions)",
    )
});
static ONLY_ALLOWS: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"(?i)\bas an option\b|without it being enabled"));
static VALUES: LazyLock<Option<[Regex; 3]>> = LazyLock::new(|| {
    Some([
        compile(r"\[possible values: ([^\]]+)\]")?,
        compile(r"[\[(]choices: ([^\])]+)[\])]")?,
        compile(r"\(([\w-]+(?:\|[\w-]+)+)\)")?,
    ])
});
static YARGS_TAIL: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile(
        r"(\s*\[(?:boolean|string|number|array|count)\])?(\s*\[(?:default: \[\]|(?:default|choices|aliases)[^\]]*)\])*\s*$",
    )
});
static BULLET: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"^-\s+([\w.-]+)(?::|$)"));
static SPACES2: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"\s{2,}"));
static ARG_SPLIT: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"\s+[<\[]"));
static COMMA_SPLIT: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r",\s*"));
static ANGLE: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"[<\[]"));
static TYPED_ARG: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"\[(?:string|number|array)\]"));
static TYPED_ANY: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"\[(?:boolean|string|number|array)\]"));
static VALUE_SPLIT: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"[,|]"));
static HELPISH: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"^\s*(help|completions?)\b"));
static STARTS_ITEM: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"^--?[A-Za-z0-9]"));
static BLOCKS: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"[█▀▄⠀]"));

fn re(cell: &'static LazyLock<Option<Regex>>) -> Result<&'static Regex, Unsure> {
    cell.as_ref().ok_or(Unsure)
}

/// Un carácter en el que `re` de Python y `regex` de Rust no clasifican igual
/// (`\w`, `\s`, `\b`, `(?i)`).
fn ambiguous(c: char) -> bool {
    if c.is_ascii() {
        return ('\u{1c}'..='\u{1f}').contains(&c);
    }
    // Plegado de mayúsculas que cae en ASCII o en `å`.
    if matches!(c, '\u{17f}' | '\u{212a}' | '\u{212b}') {
        return true;
    }
    let mut buf = [0u8; 4];
    let s = c.encode_utf8(&mut buf);
    match (RUST_WORD.as_ref(), PY_WORD.as_ref()) {
        (Some(rust), Some(py)) => rust.is_match(s) != py.is_match(s),
        _ => true,
    }
}

/// El texto se puede interpretar con las expresiones de Rust con la misma
/// respuesta que el Python.
pub fn regex_safe(text: &str) -> bool {
    !text.chars().any(ambiguous)
}

/// `str.rstrip()`.
fn rstrip(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// `str.split()` sin argumentos.
fn words(s: &str) -> Vec<&str> {
    s.split(is_space).filter(|w| !w.is_empty()).collect()
}

/// `_indent(line)`: espacios iniciales (en caracteres).
fn indent(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

/// `_values(desc, bullets)`.
fn values(desc: &str, bullets: Vec<String>) -> Result<Vec<String>, Unsure> {
    let rules = VALUES.as_ref().ok_or(Unsure)?;
    for rule in rules {
        let Some(group) = rule.captures(desc).and_then(|c| c.get(1)) else {
            continue;
        };
        let vals: Vec<String> = re(&VALUE_SPLIT)?
            .split(group.as_str())
            .filter(|v| !strip(v).is_empty())
            .map(|v| strip(v).trim_matches('"').to_owned())
            .collect();
        return Ok(vals
            .into_iter()
            .filter(|v| !v.contains(':') && !v.contains(' '))
            .collect());
    }
    Ok(bullets)
}

/// `_item(head, desc_lines, binary, section)`.
fn item(head: &str, desc_lines: &[String], binary: &str, section: &str) -> Result<Value, Unsure> {
    let head = strip(head);
    let mut bullets = Vec::new();
    for d in desc_lines {
        if let Some(m) = re(&BULLET)?.captures(strip(d)).and_then(|c| c.get(1)) {
            bullets.push(m.as_str().to_owned());
        }
    }
    let joined = desc_lines
        .iter()
        .map(|d| strip(d))
        .filter(|d| !d.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let desc = re(&SPACES2)?.replace_all(&joined, " ").into_owned();
    let (text, takes_arg) = if head.starts_with('-') {
        let before = re(&ARG_SPLIT)?.split(head).next().unwrap_or("");
        let names: Vec<&str> = re(&COMMA_SPLIT)?.split(before).map(strip).collect();
        let flag = names
            .iter()
            .find(|n| n.starts_with("--"))
            .or_else(|| names.first())
            .copied()
            .unwrap_or("");
        let flag = flag.split('=').next().unwrap_or("");
        let takes = re(&ANGLE)?.is_match(head) || re(&TYPED_ARG)?.is_match(&desc);
        (format!("{binary} {flag}"), takes)
    } else {
        let mut w = words(head);
        if w.first() == Some(&binary) {
            w.remove(0);
        }
        match w.first() {
            Some(first) if !first.starts_with(['[', '<']) => {
                let name = first.split('|').next().unwrap_or("");
                (format!("{binary} {name}"), w.len() > 1)
            }
            // «opencode [project]» = arranque normal.
            _ => (binary.to_owned(), false),
        }
    };
    let shown = if re(&TYPED_ANY)?.is_match(&desc) {
        strip(&re(&YARGS_TAIL)?.replace_all(&desc, "")).to_owned()
    } else {
        desc.clone()
    };
    let vals = values(&desc, bullets)?;
    let yolo = re(&YOLO)?.is_match(&shown) && !re(&ONLY_ALLOWS)?.is_match(&shown);
    let suffix = if takes_arg || !vals.is_empty() {
        " "
    } else {
        ""
    };
    Ok(json!({
        "text": format!("{text}{suffix}"),
        "head": head,
        "description": shown,
        "args": vals,
        "yolo": yolo,
        "section": section,
    }))
}

struct Section {
    title: String,
    items: Vec<Value>,
}

/// `parse_help(text, binary)`: `{"summary", "sections": [{"title", "items"}]}`
/// en el orden del CLI.
pub fn parse_help(text: &str, binary: &str) -> Result<Map<String, Value>, Unsure> {
    let cleaned = re(&ANSI)?.replace_all(text, "").replace('\t', "    ");
    if !regex_safe(&cleaned) || !regex_safe(binary) {
        return Err(Unsure);
    }
    let mut summary: Vec<&str> = Vec::new();
    let mut sections: Vec<Section> = Vec::new();
    let mut item_now: Option<(String, Vec<String>)> = None;
    let mut item_ind = 0usize;
    let mut para_done = false;
    let prefix = format!("{binary} ");
    let flush = |item_now: &mut Option<(String, Vec<String>)>,
                 sections: &mut Vec<Section>|
     -> Result<(), Unsure> {
        if let Some((head, desc)) = item_now.take()
            && let Some(cur) = sections.last_mut()
            && (!re(&HELPISH)?.is_match(&head) || strip(&head).starts_with('-'))
        {
            let built = item(&head, &desc, binary, &cur.title)?;
            cur.items.push(built);
        }
        Ok(())
    };
    for raw in splitlines(&cleaned) {
        let line = rstrip(raw);
        if !line.starts_with(' ')
            && let Some(title) = re(&SECTION)?.captures(line).and_then(|c| c.get(1))
        {
            flush(&mut item_now, &mut sections)?;
            sections.push(Section {
                title: title.as_str().to_owned(),
                items: Vec::new(),
            });
            continue;
        }
        if sections.is_empty() {
            let s = strip(line);
            if s.is_empty() {
                para_done = para_done || !summary.is_empty();
            } else if !para_done
                && !s.to_lowercase().starts_with("usage")
                && !re(&BLOCKS)?.is_match(s)
                && !s.starts_with(&prefix)
            {
                summary.push(s); // primer párrafo de la ayuda
            }
            continue;
        }
        let s = strip(line);
        if s.is_empty() {
            continue;
        }
        let ind = indent(line);
        let starts_item =
            re(&STARTS_ITEM)?.is_match(s) && (item_now.is_none() || ind <= item_ind + 4);
        if let Some((_, desc)) = item_now.as_mut()
            && !starts_item
            && ind > item_ind
        {
            desc.push(s.to_owned()); // continuación de la descripción
            continue;
        }
        if starts_item || ind <= 4 {
            flush(&mut item_now, &mut sections)?;
            let mut parts = re(&SPACES2)?.splitn(s, 2);
            let head = parts.next().unwrap_or("").to_owned();
            let desc = parts.next().map(|d| vec![d.to_owned()]).unwrap_or_default();
            item_now = Some((head, desc));
            item_ind = ind;
            continue;
        }
        if let Some((_, desc)) = item_now.as_mut() {
            desc.push(s.to_owned());
        }
    }
    flush(&mut item_now, &mut sections)?;
    let mut out = Vec::new();
    for sec in sections {
        let lower = sec.title.to_lowercase();
        if matches!(lower.as_str(), "arguments" | "positionals") || sec.items.is_empty() {
            continue;
        }
        // flag de Go no titula la sección: «Usage of agy».
        let title = if lower.starts_with("usage of") {
            "Options".to_owned()
        } else {
            sec.title
        };
        out.push(json!({"title": title, "items": sec.items}));
    }
    let mut parsed = Map::new();
    parsed.insert(
        "summary".into(),
        Value::from(strip(&summary.join(" ")).to_owned()),
    );
    parsed.insert("sections".into(), Value::Array(out));
    Ok(parsed)
}

/// El texto que `help_for` interpreta: la salida estándar y, si está vacía
/// (solo blancos), la de errores tras un salto. `None` si no queda nada.
pub fn help_text(stdout: &str, stderr: &str) -> Option<String> {
    let text = if !stderr.is_empty() && strip(stdout).is_empty() {
        format!("{stdout}\n{stderr}")
    } else {
        stdout.to_owned()
    };
    (!strip(&text).is_empty()).then_some(text)
}

/// `(realpath(exe), st_mtime_ns, st_size)`: la llave de `_HELP_CACHE` y de
/// `_DETECT_CACHE`. `None` si no se puede leer (`OSError`).
pub type FileKey = (PathBuf, i128, u64);

pub fn file_key(path: &Path) -> Option<FileKey> {
    let real = std::fs::canonicalize(path).ok()?;
    let meta = std::fs::metadata(&real).ok()?;
    let mtime = i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec());
    Some((real, mtime, meta.size()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambiguous_chars() {
        assert!(regex_safe("Opciones: --help  muestra ayuda… é ñ"));
        assert!(regex_safe("plain ascii --x  y"));
        assert!(!regex_safe("a\u{1c}b"));
        assert!(!regex_safe("x\u{301}")); // marca combinante
        assert!(!regex_safe("²"));
        assert!(regex_safe("café – ñ"));
    }

    #[test]
    fn help_text_uses_stderr_only_when_stdout_is_blank() {
        assert_eq!(help_text("out", "err").as_deref(), Some("out"));
        assert_eq!(help_text("  ", "err").as_deref(), Some("  \nerr"));
        assert_eq!(help_text(" \n", ""), None);
    }
}
