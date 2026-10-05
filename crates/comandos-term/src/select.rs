//! Selección, copia, enlaces y ligaduras: la parte pura (sin DOM) de lo que
//! xterm.js 5.5.0 y `dash/term.html` hacen sobre la rejilla. A6 pinta y
//! cablea el ratón; aquí solo se decide qué texto, qué enlaces y qué
//! secuencias.
//!
//! Las filas son **absolutas**: `0` es la primera fila de pantalla, la
//! historia es negativa (`-1` la fila justo encima) y no depende del
//! desplazamiento de la vista (`display_offset`).
//!
//! ## Diferencias con xterm.js / `term.html` (documentadas, no accidentales)
//!
//! - **Espacios finales.** `translateToString(true)` de xterm.js recorta las
//!   celdas *nunca escritas*, pero conserva los espacios que la aplicación
//!   escribió. alacritty no distingue una celda borrada de un espacio
//!   escrito (ambas son `' '`), así que aquí se recortan todos los espacios
//!   finales. Es lo que pide el criterio de A8 (`"abc   "` se copia como
//!   `"abc"`); el único efecto es que una fila «llena de espacios» ya no
//!   cuenta como llena en el proveedor de URL envueltas.
//! - **Cabeza inclusiva.** xterm.js guarda el final de la selección como
//!   exclusivo; [`Selection::head`] es la última celda seleccionada. Un clic
//!   sin arrastre no debe crear una [`Selection`] (xterm.js tampoco la
//!   considera selección).
//! - **Separadores de palabra** (`wordSeparator` por defecto de xterm.js 5.5):
//!   espacio y `()[]{}',"```. Salen del bundle, no de la lista del plan
//!   (que olvidaba la coma y el acento grave).
//! - **Enlaces.** Se reúnen tres fuentes, como en `term.html`: el proveedor
//!   de URL envueltas por el wrap duro de tmux (regla copiada de
//!   `term.html`), `WebLinksAddon` 0.11.0 (URL en una fila o envueltas con
//!   el flag de wrap, con su expresión y su ventana de 2048 caracteres) y
//!   los hipervínculos OSC 8 (solo http/https, una fila por enlace). Donde
//!   dos se solapan gana, en este orden, OSC 8, el proveedor de tmux y
//!   `WebLinksAddon`. La validación `new URL()` de `WebLinksAddon` se
//!   reduce a «el anfitrión no está vacío»; `regex` no se usa (se evita una
//!   dependencia nueva): los dos patrones se evalúan a mano.
//! - **Ligaduras.** Los rangos son índices de carácter (`char`), no de
//!   UTF-16 ni de byte; el addon usa `m.index` (UTF-16) como columna, que
//!   coincide mientras no haya caracteres anchos ni astrales antes.
//!
//! ## Indexación
//!
//! `Grid` solo ofrece `Index` por `Line`: el único acceso está en [`cells`],
//! tras comprobar el rango (excepción a la regla de no indexar, como en
//! `render`).
use crate::engine::Engine;
use alacritty_terminal::{
    grid::Dimensions,
    index::Line,
    term::cell::{Cell, Flags},
};

/// Punto de la rejilla: (fila absoluta, columna).
pub type Point = (i32, u16);

/// Cómo se interpreta una [`Selection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectMode {
    /// Arrastre: de `anchor` a `head`, ambas celdas incluidas.
    Simple,
    /// Doble clic (y arrastre): crece a palabras enteras en los dos extremos.
    Word,
    /// Triple clic: líneas lógicas enteras (con todas sus filas envueltas).
    Line,
}

/// Selección en coordenadas absolutas. `anchor` y `head` pueden venir en
/// cualquier orden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Point,
    pub head: Point,
    pub mode: SelectMode,
}

/// Enlace encontrado: `start` y `end` son la primera y la última celda
/// (inclusivas); pueden estar en filas distintas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlSpan {
    pub start: Point,
    pub end: Point,
    pub url: String,
}

/// `wordSeparator` por defecto de xterm.js 5.5.0.
const WORD_SEPARATORS: &str = " ()[]{}',\"`";
/// Tope de caracteres a cada lado de la ventana de `WebLinksAddon`.
const WINDOW_LIMIT: usize = 2048;

// --- acceso a la rejilla ---------------------------------------------------

/// Celdas de la fila absoluta `row`, si existe (pantalla o historia).
fn cells(engine: &Engine, row: i32) -> Option<&[Cell]> {
    let grid = engine.term().grid();
    let line = Line(row);
    if line < grid.topmost_line() || line > grid.bottommost_line() {
        return None;
    }
    // `Grid` solo ofrece `Index`; el rango se comprobó arriba.
    Some(&grid[line][..])
}

fn first_row(engine: &Engine) -> i32 {
    engine.term().grid().topmost_line().0
}

fn last_row(engine: &Engine) -> i32 {
    engine.term().grid().bottommost_line().0
}

fn columns(engine: &Engine) -> usize {
    engine.term().grid().columns()
}

fn col16(col: usize) -> u16 {
    u16::try_from(col).unwrap_or(u16::MAX)
}

/// `isWrapped` de xterm.js: la fila anterior continúa en esta (alacritty
/// marca `WRAPLINE` en la última celda de la fila que se desborda).
fn is_wrapped(engine: &Engine, row: i32) -> bool {
    row.checked_sub(1)
        .and_then(|prev| cells(engine, prev))
        .and_then(<[Cell]>::last)
        .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE))
}

fn is_spacer(cell: &Cell) -> bool {
    cell.flags
        .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
}

fn has_content(cell: &Cell) -> bool {
    cell.c != ' ' || cell.zerowidth().is_some_and(|marks| !marks.is_empty())
}

/// Espacio «de verdad»: ni mitad de un carácter ancho ni otro carácter.
fn is_space(cell: &Cell) -> bool {
    cell.c == ' ' && !is_spacer(cell)
}

/// `getTrimmedLength`: columnas hasta la última celda con contenido (un
/// carácter ancho ocupa dos).
fn trimmed_len(row: &[Cell]) -> usize {
    row.iter().rposition(has_content).map_or(0, |index| {
        let wide = row
            .get(index)
            .is_some_and(|cell| cell.flags.contains(Flags::WIDE_CHAR));
        (index + 1 + usize::from(wide)).min(row.len())
    })
}

/// `translateToString(true, start, end)`: texto de las columnas
/// `start..end` recortado por la derecha; la segunda mitad de un carácter
/// ancho no aporta nada.
fn row_text(row: &[Cell], start: usize, end: Option<usize>) -> String {
    let limit = trimmed_len(row).min(end.unwrap_or(usize::MAX));
    let mut out = String::new();
    for cell in row.get(start..limit).unwrap_or_default() {
        if is_spacer(cell) {
            continue;
        }
        out.push(cell.c);
        for mark in cell.zerowidth().unwrap_or_default() {
            out.push(*mark);
        }
    }
    out
}

// --- selección --------------------------------------------------------------

/// Extremos de la selección ya ordenados, recortados a la rejilla y
/// expandidos según el modo; `None` si la rejilla no tiene filas.
pub fn selection_bounds(engine: &Engine, s: &Selection) -> Option<(Point, Point)> {
    let (first, last) = (first_row(engine), last_row(engine));
    let max_col = col16(columns(engine).saturating_sub(1));
    let clamp = |(row, col): Point| (row.clamp(first, last), col.min(max_col));
    let (a, b) = (clamp(s.anchor), clamp(s.head));
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    Some(match s.mode {
        SelectMode::Simple => (lo, hi),
        SelectMode::Word => {
            let start = word_extent(engine, lo, true).map_or(lo, |(start, _)| start);
            let end = word_extent(engine, hi, true).map_or(hi, |(_, end)| end);
            (start, end)
        }
        SelectMode::Line => {
            let mut top = lo.0;
            while top > first && is_wrapped(engine, top) {
                top -= 1;
            }
            let mut bottom = hi.0;
            while bottom < last && is_wrapped(engine, bottom + 1) {
                bottom += 1;
            }
            ((top, 0), (bottom, max_col))
        }
    })
}

/// Texto seleccionado como lo copia xterm.js: las filas envueltas se unen sin
/// salto, el resto se separa con `\n`, cada fila va sin espacios finales y
/// el espacio de no separación se copia como espacio normal.
pub fn selected_text(engine: &Engine, s: &Selection) -> String {
    let Some(((row0, col0), (row1, col1))) = selection_bounds(engine, s) else {
        return String::new();
    };
    let mut parts: Vec<String> = Vec::new();
    let piece = |row: i32, from: usize, to: Option<usize>| {
        cells(engine, row)
            .map(|line| row_text(line, from, to))
            .unwrap_or_default()
    };
    let end_col = usize::from(col1) + 1;
    parts.push(piece(
        row0,
        usize::from(col0),
        (row0 == row1).then_some(end_col),
    ));
    let push = |row: i32, text: String, parts: &mut Vec<String>| match parts.last_mut() {
        Some(last) if is_wrapped(engine, row) => last.push_str(&text),
        _ => parts.push(text),
    };
    let mut row = row0.saturating_add(1);
    while row < row1 {
        push(row, piece(row, 0, None), &mut parts);
        row += 1;
    }
    if row0 != row1 {
        push(row1, piece(row1, 0, Some(end_col)), &mut parts);
    }
    parts.join("\n").replace('\u{a0}', " ")
}

/// Palabra bajo `point` como la elige el doble clic de xterm.js
/// (`_getWordAt`): `None` sobre una racha de espacios. Devuelve la primera y
/// la última celda de la palabra, que sigue por las filas envueltas.
pub fn word_at(engine: &Engine, point: Point) -> Option<(Point, Point)> {
    word_extent(engine, point, false)
}

fn is_separator(cell: &Cell) -> bool {
    !is_spacer(cell)
        && cell.zerowidth().is_none_or(<[char]>::is_empty)
        && WORD_SEPARATORS.contains(cell.c)
}

/// Columnas `(inicio, fin)` de la palabra de `col` dentro de una fila.
fn word_cols(row: &[Cell], col: usize, allow_space: bool) -> Option<(usize, usize)> {
    let last = row.len().checked_sub(1)?;
    let mut col = col.min(last);
    // Mitad derecha de un carácter ancho: se parte de su primera mitad.
    if col > 0 && row.get(col).is_some_and(is_spacer) {
        col -= 1;
    }
    let at = |index: usize| row.get(index);
    if at(col).is_some_and(is_space) {
        if !allow_space {
            return None;
        }
        let mut start = col;
        while start > 0 && at(start - 1).is_some_and(is_space) {
            start -= 1;
        }
        let mut end = col;
        while at(end + 1).is_some_and(is_space) {
            end += 1;
        }
        return Some((start, end));
    }
    let mut start = col;
    while start > 0 && at(start - 1).is_some_and(|cell| !is_separator(cell)) {
        start -= 1;
    }
    // Si se paró sobre la mitad derecha de un separador ancho, no es suya.
    if at(start).is_some_and(is_spacer) && start < col {
        start += 1;
    }
    let mut end = col;
    while at(end + 1).is_some_and(|cell| !is_separator(cell)) {
        end += 1;
    }
    Some((start, end))
}

/// Palabra con la extensión por filas envueltas de `_getWordAt`: si llega a
/// un borde y la fila vecina continúa sin espacio, la palabra sigue allí.
fn word_extent(engine: &Engine, point: Point, allow_space: bool) -> Option<(Point, Point)> {
    let (row, col) = point;
    let line = cells(engine, row)?;
    let (s, e) = word_cols(line, usize::from(col), allow_space)?;
    let mut start = (row, col16(s));
    let mut end = (row, col16(e));
    let width = line.len();
    // Hacia atrás.
    let (mut cur, mut cur_start) = (row, s);
    while cur_start == 0
        && cells(engine, cur)
            .and_then(<[Cell]>::first)
            .is_some_and(|c| !is_space(c))
        && is_wrapped(engine, cur)
    {
        let prev = cur - 1;
        let Some(prev_cells) = cells(engine, prev) else {
            break;
        };
        if prev_cells.last().is_none_or(is_space) {
            break;
        }
        let Some((ps, _)) = word_cols(prev_cells, width, false) else {
            break;
        };
        start = (prev, col16(ps));
        cur = prev;
        cur_start = ps;
    }
    // Hacia delante.
    let (mut cur, mut cur_end) = (row, e);
    while cur_end + 1 == width
        && cells(engine, cur)
            .and_then(<[Cell]>::last)
            .is_some_and(|c| !is_space(c))
    {
        let next = cur + 1;
        let Some(next_cells) = cells(engine, next) else {
            break;
        };
        if !is_wrapped(engine, next) || next_cells.first().is_none_or(is_space) {
            break;
        }
        let Some((_, ne)) = word_cols(next_cells, 0, false) else {
            break;
        };
        end = (next, col16(ne));
        cur = next;
        cur_end = ne;
    }
    Some((start, end))
}

// --- enlaces -----------------------------------------------------------------

/// Un carácter de una fila con la celda donde está.
#[derive(Clone, Copy)]
struct Glyph {
    ch: char,
    row: i32,
    col: u16,
}

/// Caracteres de la fila recortada, uno por carácter (las marcas combinantes
/// comparten la columna de su base).
fn row_glyphs(row_cells: &[Cell], row: i32) -> Vec<Glyph> {
    let mut out = Vec::new();
    for (col, cell) in row_cells.iter().take(trimmed_len(row_cells)).enumerate() {
        if is_spacer(cell) {
            continue;
        }
        let col = col16(col);
        out.push(Glyph {
            ch: cell.c,
            row,
            col,
        });
        for mark in cell.zerowidth().unwrap_or_default() {
            out.push(Glyph {
                ch: *mark,
                row,
                col,
            });
        }
    }
    out
}

/// Longitud en UTF-16 del texto de la fila (`lineTxt(i).length` de term.html).
fn utf16_len(glyphs: &[Glyph]) -> usize {
    glyphs.iter().map(|g| g.ch.len_utf16()).sum()
}

/// `\s` de JavaScript.
fn js_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

/// `https?://` (o `HTTPS?://` si `upper_ok`) al comienzo de `text`; devuelve su
/// longitud en caracteres.
fn scheme_len(text: &[Glyph], upper_ok: bool) -> Option<usize> {
    let starts = |wanted: &str| {
        wanted
            .chars()
            .enumerate()
            .all(|(i, w)| text.get(i).is_some_and(|g| g.ch == w))
    };
    let variants: &[&str] = if upper_ok {
        &["https://", "http://", "HTTPS://", "HTTP://"]
    } else {
        &["https://", "http://"]
    };
    variants
        .iter()
        .find(|wanted| starts(wanted))
        .map(|wanted| wanted.len())
}

/// `/https?:\/\/[^\s'"<>\])]+/g` de `term.html`: rangos `[inicio, fin)` en
/// caracteres.
fn provider_matches(text: &[Glyph]) -> Vec<(usize, usize)> {
    let class = |c: char| !js_space(c) && !matches!(c, '\'' | '"' | '<' | '>' | ']' | ')');
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let Some(rest) = text.get(i..) else { break };
        if let Some(head) = scheme_len(rest, false) {
            let run = rest
                .get(head..)
                .unwrap_or_default()
                .iter()
                .take_while(|g| class(g.ch))
                .count();
            if run > 0 {
                out.push((i, i + head + run));
                i += head + run;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Expresión de `WebLinksAddon` 0.11.0:
/// `/(https?|HTTPS?):[/]{2}[^\s"'!*(){}|\\\^<>`]*[^\s"':,.!?{}|\\\^~\[\]`()<>]/`.
fn weblink_matches(text: &[Glyph]) -> Vec<(usize, usize)> {
    let body = |c: char| {
        !js_space(c)
            && !matches!(
                c,
                '"' | '\'' | '!' | '*' | '(' | ')' | '{' | '}' | '|' | '\\' | '^' | '<' | '>' | '`'
            )
    };
    let last = |c: char| {
        !js_space(c)
            && !matches!(
                c,
                '"' | '\''
                    | ':'
                    | ','
                    | '.'
                    | '!'
                    | '?'
                    | '{'
                    | '}'
                    | '|'
                    | '\\'
                    | '^'
                    | '~'
                    | '['
                    | ']'
                    | '`'
                    | '('
                    | ')'
                    | '<'
                    | '>'
            )
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let Some(rest) = text.get(i..) else { break };
        let found = scheme_len(rest, true).and_then(|head| {
            let tail = rest.get(head..).unwrap_or_default();
            let run = tail.iter().take_while(|g| body(g.ch)).count();
            // Retroceso de `[^…]*` hasta que el siguiente carácter sirva de final.
            (0..=run)
                .rev()
                .find(|k| tail.get(*k).is_some_and(|g| last(g.ch)))
                .map(|k| head + k + 1)
        });
        match found {
            Some(len) => {
                let candidate: String = rest.iter().take(len).map(|g| g.ch).collect();
                if valid_web_url(&candidate) {
                    out.push((i, i + len));
                    i += len;
                    continue;
                }
                i += 1;
            }
            None => i += 1,
        }
    }
    out
}

/// Reducción de la validación `new URL(..)` de `WebLinksAddon`: tras
/// `://` debe haber un anfitrión (no vacío, empieza por letra, cifra o `[`).
fn valid_web_url(url: &str) -> bool {
    let Some((_, rest)) = url.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    host.chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '[')
}

/// Tramo de glifos `[from, to)` como `UrlSpan` (extremos inclusivos).
fn span_of(glyphs: &[Glyph], from: usize, to: usize) -> Option<UrlSpan> {
    let first = glyphs.get(from)?;
    let last = glyphs.get(to.checked_sub(1)?)?;
    Some(UrlSpan {
        start: (first.row, first.col),
        end: (last.row, last.col),
        url: glyphs.get(from..to)?.iter().map(|g| g.ch).collect(),
    })
}

/// Proveedor de URL envueltas por el wrap duro de tmux de `term.html`: junta
/// el bloque de filas llenas contiguas que toca `line` y devuelve las URL
/// que cruzan filas.
fn tmux_wrapped(engine: &Engine, line: i32) -> Vec<UrlSpan> {
    let width = columns(engine);
    let (first, last) = (first_row(engine), last_row(engine));
    let glyphs_of = |row: i32| {
        cells(engine, row)
            .map(|c| row_glyphs(c, row))
            .unwrap_or_default()
    };
    let full = |row: i32| utf16_len(&glyphs_of(row)) >= width;
    let (mut top, mut bottom) = (line, line);
    while top > first && full(top - 1) {
        top -= 1;
    }
    while bottom < last && full(bottom) {
        bottom += 1;
    }
    if top == bottom {
        return Vec::new();
    }
    let mut joined: Vec<Glyph> = Vec::new();
    for row in top..=bottom {
        joined.extend(glyphs_of(row));
    }
    provider_matches(&joined)
        .into_iter()
        .filter_map(|(s, e)| span_of(&joined, s, e))
        .filter(|span| span.start.0 != span.end.0)
        .filter(|span| span.start.0 <= line && line <= span.end.0)
        .collect()
}

/// Ventana de `WebLinksAddon` (`_getWindowedLineStrings`): la fila pedida
/// más las envueltas que la continúan por arriba y por abajo.
fn weblink_window(engine: &Engine, line: i32) -> Vec<Glyph> {
    let glyphs_of = |row: i32| {
        cells(engine, row)
            .map(|c| row_glyphs(c, row))
            .unwrap_or_default()
    };
    let has_space = |glyphs: &[Glyph]| glyphs.iter().any(|g| g.ch == ' ');
    let own = glyphs_of(line);
    let mut above: Vec<Vec<Glyph>> = Vec::new();
    if is_wrapped(engine, line) && own.first().is_some_and(|g| g.ch != ' ') {
        let (mut row, mut count) = (line, 0_usize);
        loop {
            row -= 1;
            if cells(engine, row).is_none() || count >= WINDOW_LIMIT {
                break;
            }
            let glyphs = glyphs_of(row);
            count += utf16_len(&glyphs);
            let keep_going = is_wrapped(engine, row) && !has_space(&glyphs);
            above.push(glyphs);
            if !keep_going {
                break;
            }
        }
    }
    let mut window: Vec<Glyph> = above.into_iter().rev().flatten().collect();
    window.extend(own);
    let (mut row, mut count) = (line, 0_usize);
    loop {
        row += 1;
        if cells(engine, row).is_none() || !is_wrapped(engine, row) || count >= WINDOW_LIMIT {
            break;
        }
        let glyphs = glyphs_of(row);
        count += utf16_len(&glyphs);
        let stop = has_space(&glyphs);
        window.extend(glyphs);
        if stop {
            break;
        }
    }
    window
}

fn weblinks(engine: &Engine, line: i32) -> Vec<UrlSpan> {
    let window = weblink_window(engine, line);
    weblink_matches(&window)
        .into_iter()
        .filter_map(|(s, e)| span_of(&window, s, e))
        .filter(|span| span.start.0 <= line && line <= span.end.0)
        .collect()
}

/// Hipervínculos OSC 8 de la fila: tramos seguidos de celdas con el mismo
/// enlace, solo http/https (como `OscLinkProvider` de xterm.js).
fn osc8(engine: &Engine, line: i32) -> Vec<UrlSpan> {
    let Some(row) = cells(engine, line) else {
        return Vec::new();
    };
    let limit = trimmed_len(row);
    let mut out = Vec::new();
    let mut run: Option<(usize, alacritty_terminal::term::cell::Hyperlink)> = None;
    let mut close = |run: Option<(usize, alacritty_terminal::term::cell::Hyperlink)>,
                     end: usize| {
        let Some((from, link)) = run else { return };
        let uri = link.uri();
        let lower = uri.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            out.push(UrlSpan {
                start: (line, col16(from)),
                end: (line, col16(end.saturating_sub(1))),
                url: uri.to_owned(),
            });
        }
    };
    for (col, cell) in row.iter().take(limit).enumerate() {
        let link = cell.hyperlink();
        let same = match (&run, &link) {
            (Some((_, current)), Some(link)) => current == link,
            _ => false,
        };
        if !same {
            close(run.take(), col);
            run = link.map(|link| (col, link));
        }
    }
    close(run, limit);
    out
}

fn overlaps(a: &UrlSpan, b: &UrlSpan) -> bool {
    a.start <= b.end && b.start <= a.end
}

/// Enlaces que tocan la fila absoluta `line`, ordenados por posición. Reúne
/// OSC 8, URL envueltas por tmux y URL de texto (ver la cabecera del módulo).
pub fn find_urls(engine: &Engine, line: i32) -> Vec<UrlSpan> {
    let mut out: Vec<UrlSpan> = Vec::new();
    for source in [
        osc8(engine, line),
        tmux_wrapped(engine, line),
        weblinks(engine, line),
    ] {
        for span in source {
            if !out.iter().any(|kept| overlaps(kept, &span)) {
                out.push(span);
            }
        }
    }
    out.sort_by_key(|span| span.start);
    out
}

// --- ligaduras ---------------------------------------------------------------

/// Secuencias candidatas de `assets/xterm/addon-ligatures-web.js`, en el
/// orden del addon (dato copiado tal cual; el addon las ordena por longitud
/// y aquí se elige la más larga que case en cada posición).
const CANDIDATES: [&str; 116] = [
    "<===>", "<==>", "<-->", "<-<<", "<<--", "<<==", "==>>", "<==>", "<==<", "===>", "<=>", "<=<",
    "<==", "===", "==>", "!==", "<--", "-->", "-<<", "->>", "<<-", "<<=", ">>=", "||=", "&&&",
    "+++", "---", "***", "...", "::=", "::<", "::>", "//=", "//<", "//>", "///", "###", "##_",
    "?!.", "?::", "?<>", "~~>", "<~~", "|||", "|=>", "||>", "|>=", "<|>", "<|=", "|=<", "}}<",
    "{{-", "!!}", "<=", ">=", "==", "!=", "=>", "->", "<-", "<<", ">>", "::", "&&", "||", "++",
    "--", "**", "//", "/*", "*/", "</", "/>", "<>", "<|", "|>", "##", "..", "::", ".=", ".?", "?=",
    "?.", "?:", "~=", "~@", "~~", "^=", "<~", "~>", "$>", "#!", "#(", "#{", "#[", "#:", "#?", "#=",
    "#_", "\\/", "/\\", "_(", "__", "@_", "}}", "{{", "{|", "[|", "[<", "|-", "|=", "|]", "|}",
    "|{", ";:", "@?",
];

/// Rangos `[inicio, fin)` (en caracteres) de las secuencias candidatas a
/// ligadura de `text`: de izquierda a derecha, sin solaparse, y en cada
/// posición la candidata más larga (la búsqueda voraz del addon).
pub fn ligature_runs(text: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let rest = chars.get(i..).unwrap_or_default();
        let best = CANDIDATES
            .iter()
            .filter(|cand| {
                let mut it = rest.iter();
                cand.chars().all(|c| it.next() == Some(&c))
            })
            .map(|cand| cand.chars().count())
            .max();
        match best {
            Some(len) => {
                out.push((i, i + len));
                i += len;
            }
            None => i += 1,
        }
    }
    out
}
