//! Markdown a markup de Pango (`md_to_pango`, `_fmt_table` y el troceo de
//! `build_full_widget` de `bin/cc-notifyd`). Las expresiones regulares del
//! Python se reescriben a mano con su misma semántica de `re` (codicia,
//! retroceso y el orden de las alternativas), sin dependencias.

/// Espacio de `\s` / `str.strip()` / `str.isspace()` de Python.
pub fn is_py_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

/// `str.strip()` de Python.
pub fn py_strip(text: &str) -> &str {
    text.trim_matches(is_py_space)
}

/// `str.splitlines()` de Python: corta en `\n`, `\r`, `\r\n`, `\v`, `\f`,
/// `\x1c`–`\x1e`, `\x85`, ` ` y ` `; sin elemento vacío final.
///
/// Pendiente tras fusionar con `main`: allí llega `comandos_core::text::splitlines`
/// con la misma regla; unificar en una sola (esta, sin indexado) en una pasada
/// posterior (M10 de la revisión final). No está en esta rama.
pub fn py_splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let is_break = matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !is_break {
            continue;
        }
        lines.push(text.get(start..index).unwrap_or_default());
        let mut next = index + c.len_utf8();
        if c == '\r' && chars.peek().is_some_and(|&(_, n)| n == '\n') {
            chars.next();
            next += 1;
        }
        start = next;
    }
    if start < text.len() {
        lines.push(text.get(start..).unwrap_or_default());
    }
    lines
}

/// `GLib.markup_escape_text`: la misma función de la libglib del sistema que
/// usa el Python (en 2.72: `&amp; &lt; &gt; &apos; &quot;` y `&#x..;` para los
/// caracteres de control). No necesita pantalla.
pub fn markup_escape(text: &str) -> String {
    gtk::glib::markup_escape_text(text).to_string()
}

/// `re.sub(r"\*\*(.+?)\*\*", repl, text)`: el `.+?` perezoso toma lo mínimo
/// (al menos un carácter) hasta el siguiente `**`.
fn sub_bold(text: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("**") {
        let after = rest.get(at + 2..).unwrap_or_default();
        // Un carácter como mínimo dentro: el cierre se busca después de él.
        let first = after.chars().next().map_or(0, char::len_utf8);
        let closing = if first == 0 {
            None
        } else {
            after
                .get(first..)
                .and_then(|tail| tail.find("**"))
                .map(|pos| pos + first)
        };
        match closing {
            Some(end) => {
                out.push_str(rest.get(..at).unwrap_or_default());
                out.push_str(open);
                out.push_str(after.get(..end).unwrap_or_default());
                out.push_str(close);
                rest = after.get(end + 2..).unwrap_or_default();
            }
            None => {
                // Sin cierre aquí: la búsqueda sigue un carácter más adelante.
                out.push_str(rest.get(..at + 1).unwrap_or_default());
                rest = rest.get(at + 1..).unwrap_or_default();
            }
        }
    }
    out.push_str(rest);
    out
}

/// `re.sub(r"(^|[\s(])\*([^*\n]+)\*(?=$|[\s).,;:!?])", r"\1<i>\2</i>", text)`.
fn sub_italic(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let opener = |c: char| is_py_space(c) || c == '(';
    let follower = |c: char| is_py_space(c) || ").,;:!?".contains(c);
    // A partir de `start` (tras el grupo 1): `*`, corrida sin `*` ni `\n`, `*`, anticipación.
    let body_end = |start: usize| -> Option<usize> {
        if chars.get(start) != Some(&'*') {
            return None;
        }
        let mut q = start + 1;
        while chars.get(q).is_some_and(|&c| c != '*' && c != '\n') {
            q += 1;
        }
        if q == start + 1 || chars.get(q) != Some(&'*') {
            return None;
        }
        match chars.get(q + 1) {
            None => Some(q + 1),
            Some(&c) if follower(c) => Some(q + 1),
            _ => None,
        }
    };
    let mut out = String::with_capacity(text.len());
    let mut p = 0;
    while p < chars.len() {
        // Alternativas en el orden del patrón: `^` primero, después `[\s(]`.
        let mut found = None;
        if p == 0 {
            found = body_end(0).map(|end| (0, end));
        }
        if found.is_none() && chars.get(p).is_some_and(|&c| opener(c)) {
            found = body_end(p + 1).map(|end| (p + 1, end));
        }
        match found {
            Some((star, end)) => {
                out.extend(chars.get(p..star).unwrap_or_default());
                out.push_str("<i>");
                out.extend(chars.get(star + 1..end - 1).unwrap_or_default());
                out.push_str("</i>");
                p = end;
            }
            None => {
                out.extend(chars.get(p));
                p += 1;
            }
        }
    }
    out
}

/// `re.sub(r"`([^`]+)`", r"<tt>\1</tt>", text)`.
fn sub_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('`') {
        let after = rest.get(at + 1..).unwrap_or_default();
        match after.find('`') {
            Some(end) if end > 0 => {
                out.push_str(rest.get(..at).unwrap_or_default());
                out.push_str("<tt>");
                out.push_str(after.get(..end).unwrap_or_default());
                out.push_str("</tt>");
                rest = after.get(end + 1..).unwrap_or_default();
            }
            _ => {
                out.push_str(rest.get(..at + 1).unwrap_or_default());
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `re.sub(r"\[([^\]]+)\]\([^)\s]+\)", r"<u>\1</u>", text)`.
fn sub_link(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let link_end = |p: usize| -> Option<usize> {
        if chars.get(p) != Some(&'[') {
            return None;
        }
        let mut q = p + 1;
        while chars.get(q).is_some_and(|&c| c != ']') {
            q += 1;
        }
        if q == p + 1 || chars.get(q) != Some(&']') || chars.get(q + 1) != Some(&'(') {
            return None;
        }
        let mut r = q + 2;
        while chars.get(r).is_some_and(|&c| c != ')' && !is_py_space(c)) {
            r += 1;
        }
        if r == q + 2 || chars.get(r) != Some(&')') {
            return None;
        }
        Some(r + 1)
    };
    let mut out = String::with_capacity(text.len());
    let mut p = 0;
    while p < chars.len() {
        match link_end(p) {
            Some(end) => {
                // El texto del enlace va de `[`+1 al primer `]`.
                let close = chars
                    .iter()
                    .skip(p + 1)
                    .position(|&c| c == ']')
                    .map_or(p + 1, |i| p + 1 + i);
                out.push_str("<u>");
                out.extend(chars.get(p + 1..close).unwrap_or_default());
                out.push_str("</u>");
                p = end;
            }
            None => {
                out.extend(chars.get(p));
                p += 1;
            }
        }
    }
    out
}

/// `re.match(r"^#{1,6} (.*)", text)`: el grupo si la línea es un título.
fn header_text(text: &str) -> Option<&str> {
    let hashes = text.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    text.get(hashes..)?.strip_prefix(' ')
}

/// `re.sub(r"^(\s*)[-*] ", "\\1• ", text)`.
fn sub_bullet(text: &str) -> String {
    let rest = text.trim_start_matches(is_py_space);
    let indent = text.get(..text.len() - rest.len()).unwrap_or_default();
    match rest
        .strip_prefix(['-', '*'])
        .and_then(|tail| tail.strip_prefix(' '))
    {
        Some(tail) => format!("{indent}• {tail}"),
        None => text.to_string(),
    }
}

/// `re.match(r"^\s*\|.*\|\s*$", line)`: fila de tabla Markdown.
pub fn is_table_row(line: &str) -> bool {
    let core = py_strip(line);
    core.len() >= 2 && core.starts_with('|') && core.ends_with('|')
}

/// `line.strip().startswith("```")`.
fn is_fence(line: &str) -> bool {
    py_strip(line).starts_with("```")
}

/// `_fmt_table(rows)`: tabla Markdown a líneas `<tt>` alineadas por columna.
pub fn fmt_table<S: AsRef<str>>(rows: &[S]) -> Vec<String> {
    let mut parsed: Vec<Vec<String>> = Vec::new();
    for row in rows {
        let cells: Vec<String> = py_strip(row.as_ref())
            .trim_matches('|')
            .split('|')
            .map(|cell| sub_bold(py_strip(cell), "", "").replace('`', ""))
            .collect();
        let separator = cells
            .iter()
            .all(|cell| cell.chars().all(|c| is_py_space(c) || c == '-' || c == ':'));
        if separator {
            continue; // la fila separadora |---|---|
        }
        parsed.push(cells);
    }
    let ncol = parsed.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..ncol)
        .map(|i| {
            parsed
                .iter()
                .map(|p| p.get(i).map_or(0, |c| c.chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    parsed
        .iter()
        .map(|p| {
            let line = widths
                .iter()
                .enumerate()
                .map(|(i, &width)| {
                    let cell = p.get(i).map_or("", String::as_str);
                    let pad = width.saturating_sub(cell.chars().count());
                    format!("{cell}{}", " ".repeat(pad))
                })
                .collect::<Vec<_>>()
                .join("  ");
            let line = line.trim_end_matches(is_py_space);
            format!("<tt>{}</tt>", markup_escape(line))
        })
        .collect()
}

/// `md_to_pango(text)`: negritas, cursivas, código, títulos, viñetas, enlaces
/// (solo el texto) y tablas alineadas; lo demás, escapado.
pub fn md_to_pango(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_code = false;
    let mut table: Vec<&str> = Vec::new();
    for line in py_splitlines(text) {
        if is_fence(line) {
            out.extend(fmt_table(&table));
            table.clear();
            in_code = !in_code;
            continue;
        }
        if !in_code && is_table_row(line) {
            table.push(line);
            continue;
        }
        let escaped = markup_escape(line);
        if in_code {
            out.push(format!("<tt>{escaped}</tt>"));
            continue;
        }
        out.extend(fmt_table(&table));
        table.clear();
        let mut esc = sub_bold(&escaped, "<b>", "</b>");
        esc = sub_italic(&esc);
        esc = sub_code(&esc);
        esc = sub_link(&esc);
        if let Some(title) = header_text(&esc) {
            esc = format!("<b>{title}</b>");
        }
        out.push(sub_bullet(&esc));
    }
    out.extend(fmt_table(&table));
    out.join("\n")
}

/// Bloque del texto completo (`build_full_widget`): párrafos con ajuste o
/// tablas sin ajuste (con su propio desplazamiento horizontal). Cada bloque
/// guarda también el texto crudo del `except` del Python (`set_text(chunk)`
/// y `set_text("\n".join(rows))`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// Un trozo de prosa (`add_text`): su markup y el trozo tal cual.
    Text { markup: String, raw: String },
    /// Una tabla (`add_table`): sus líneas `<tt>` unidas por `\n` y las filas tal cual.
    Table { markup: String, raw: String },
}

/// Lo que se pone en la etiqueta: markup válido o, si no, el texto crudo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelText<'a> {
    Markup(&'a str),
    Plain(&'a str),
}

/// ¿Lo acepta Pango? (`gtk_label_set_markup` usa este mismo análisis, sin
/// acelerador). No necesita pantalla.
pub fn valid_markup(markup: &str) -> bool {
    gtk::pango::parse_markup(markup, '\0').is_ok()
}

impl Block {
    pub fn markup(&self) -> &str {
        match self {
            Block::Text { markup, .. } | Block::Table { markup, .. } => markup,
        }
    }

    /// Ruling del controlador (el usuario quiere ver SIEMPRE el texto
    /// completo): con markup que Pango rechaza (p. ej. `**` y `` ` `` cruzados),
    /// el Python deja la etiqueta en blanco porque `set_markup` no lanza; aquí
    /// se pone el texto crudo, como pretendía su `except`. Diferencia aceptada.
    pub fn label_text(&self) -> LabelText<'_> {
        match self {
            Block::Text { markup, raw } | Block::Table { markup, raw } => {
                if valid_markup(markup) {
                    LabelText::Markup(markup)
                } else {
                    LabelText::Plain(raw)
                }
            }
        }
    }
}

/// El troceo de `build_full_widget(full)`, con los mismos descartes: trozos
/// de prosa vacíos y tablas sin filas útiles no producen bloque.
pub fn blocks(full: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let add_text = |out: &mut Vec<Block>, lines: &[&str]| {
        let chunk = lines.join("\n");
        if !py_strip(&chunk).is_empty() {
            out.push(Block::Text {
                markup: md_to_pango(&chunk),
                raw: chunk,
            });
        }
    };
    let add_table = |out: &mut Vec<Block>, rows: &[&str]| {
        let lines = fmt_table(rows);
        if !lines.is_empty() {
            out.push(Block::Table {
                markup: lines.join("\n"),
                raw: rows.join("\n"),
            });
        }
    };
    let mut buf: Vec<&str> = Vec::new();
    let mut table: Vec<&str> = Vec::new();
    let mut in_code = false;
    for line in py_splitlines(full) {
        if is_fence(line) {
            // Igual que el Python: la valla no vacía la tabla pendiente.
            in_code = !in_code;
            buf.push(line);
            continue;
        }
        if !in_code && is_table_row(line) {
            if !buf.is_empty() {
                add_text(&mut out, &buf);
                buf.clear();
            }
            table.push(line);
            continue;
        }
        if !table.is_empty() {
            add_table(&mut out, &table);
            table.clear();
        }
        buf.push(line);
    }
    if !buf.is_empty() {
        add_text(&mut out, &buf);
    }
    if !table.is_empty() {
        add_table(&mut out, &table);
    }
    out
}

/// Los primeros `n` caracteres (`texto[:n]` de Python).
pub fn py_prefix(text: &str, n: usize) -> &str {
    match text.char_indices().nth(n) {
        Some((index, _)) => text.get(..index).unwrap_or(text),
        None => text,
    }
}
