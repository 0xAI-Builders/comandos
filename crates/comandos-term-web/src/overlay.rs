//! Capas transparentes sobre el canvas del texto, como las de addon-canvas
//! y el addon de ligaduras de `dash/term.html`:
//!
//! - [`Overlay`]: la selección (`SelectionRenderLayer`) y el subrayado del
//!   enlace bajo el ratón (`LinkRenderLayer`).
//! - [`Ligatures`]: las ligaduras de escritorio (`addon-ligatures-web.js`):
//!   tapa las celdas de cada secuencia con el fondo del tema y dibuja encima
//!   la secuencia con la cara `comandos-liga` (JetBrains Mono), centrada.
//!   Se repinta 90 ms después del último cambio, como el addon.
//!
//! ## Diferencias aceptadas
//!
//! - Ligaduras: el navegador rasteriza la secuencia con `fillText` (su
//!   `calt` produce la ligadura) donde el addon trazaba con opentype.js:
//!   ±1 px de suavizado en esas celdas (D10 del índice; la suite `term-liga`
//!   usa `channel: 48`). El tamaño es el de la terminal; el addon usaba
//!   siempre 14 px aunque las preferencias pusieran 11.
//!
//! El cursor tiene su propia capa encima de esta (la de
//! [`crate::canvas::Canvas2d::cursor_element`], z-index 3), como el orden
//! texto / selección / enlace / cursor de addon-canvas.
use comandos_term::select::{Point, ligature_runs};

/// Rectángulo en celdas de la vista.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// `_redrawSelection` de addon-canvas: rectángulos de la selección de
/// `start` a `end` (filas absolutas; `end` excluido) en la vista desplazada
/// `display_offset` filas.
pub fn selection_rects(
    start: Point,
    end: Point,
    display_offset: usize,
    rows: u16,
    cols: u16,
) -> Vec<CellRect> {
    let view = |row: i32| i64::from(row) + display_offset as i64;
    let (s, r) = (view(start.0), view(end.0));
    let rows_i = i64::from(rows);
    let capped_start = s.max(0);
    let capped_end = r.min(rows_i - 1);
    let mut out = Vec::new();
    if capped_start >= rows_i || capped_end < 0 || rows == 0 {
        return out;
    }
    let mut fill = |x: u16, y: i64, w: u16, h: i64| {
        if let (Ok(y), Ok(h)) = (u16::try_from(y), u16::try_from(h))
            && w > 0
            && h > 0
        {
            out.push(CellRect { x, y, w, h });
        }
    };
    let first_col = if s == capped_start {
        start.1.min(cols)
    } else {
        0
    };
    let first_end = if capped_start == r {
        end.1.min(cols)
    } else {
        cols
    };
    fill(
        first_col,
        capped_start,
        first_end.saturating_sub(first_col),
        1,
    );
    let middle = (capped_end - capped_start - 1).max(0);
    fill(0, capped_start + 1, cols, middle);
    if capped_start != capped_end {
        let last_end = if r == capped_end {
            end.1.min(cols)
        } else {
            cols
        };
        fill(0, capped_end, last_end, 1);
    }
    out
}

/// `_redrawSelection` en modo columna (Alt+arrastre): un solo rectángulo de
/// las columnas `left..right` (`right` excluido) en las filas visibles de
/// `top` a `bottom` (absolutas, incluidas).
pub fn block_rects(
    (top, left): Point,
    (bottom, right): Point,
    display_offset: usize,
    rows: u16,
    cols: u16,
) -> Vec<CellRect> {
    let view = |row: i32| i64::from(row) + display_offset as i64;
    let first = view(top).max(0);
    let last = view(bottom).min(i64::from(rows) - 1);
    let (x, end) = (left.min(cols), right.min(cols));
    match (u16::try_from(first), u16::try_from(last - first + 1)) {
        (Ok(y), Ok(h)) if h > 0 && end > x => vec![CellRect {
            x,
            y,
            w: end - x,
            h,
        }],
        _ => Vec::new(),
    }
}

/// Secuencias de [`comandos_term::select::ligature_runs`] que tienen glifo
/// `.liga` en `JetBrainsMonoNerdFontMono-Regular.ttf` (sacado de la tabla
/// `post` de esa fuente: el addon descarta las que no lo tienen).
pub const LIGA_GLYPHS: [&str; 87] = [
    "<==>", "<=>", "<=<", "<==", "===", "==>", "!==", "<--", "-->", "-<<", "->>", "<<-", "<<=",
    ">>=", "||=", "&&&", "+++", "---", "***", "...", "::=", "//=", "///", "###", "~~>", "<~~",
    "|||", "|=>", "||>", "<|>", "!!}", "<=", ">=", "==", "!=", "=>", "->", "<-", "<<", ">>", "::",
    "&&", "||", "++", "--", "//", "/*", "*/", "</", "/>", "<>", "<|", "|>", "##", "..", ".=", ".?",
    "?=", "?.", "?:", "~@", "~~", "^=", "<~", "~>", "$>", "#!", "#(", "#{", "#[", "#:", "#?", "#=",
    "#_", "\\/", "/\\", "__", "@_", "}}", "{{", "{|", "[|", "[<", "|-", "|=", "|]", "|}",
];

/// Métricas `hhea` de la misma fuente (unidades por em, ascendente y
/// descendente): el addon centra la línea con ellas.
const UNITS_PER_EM: f64 = 1000.0;
const HHEA_ASCENT: f64 = 1020.0;
const HHEA_DESCENT: f64 = 300.0;

/// Ligaduras de una fila: `(columna, secuencia)`. Como el addon, la columna
/// es el índice del carácter en el texto de la fila (difiere de la celda si
/// antes hay caracteres anchos), y una secuencia sin glifo no deja probar
/// otra más corta dentro de ella.
pub fn liga_spans(text: &str) -> Vec<(usize, &'static str)> {
    let chars: Vec<char> = text.chars().collect();
    ligature_runs(text)
        .into_iter()
        .filter_map(|(start, end)| {
            let seq: String = chars.get(start..end)?.iter().collect();
            LIGA_GLYPHS
                .iter()
                .find(|glyph| **glyph == seq)
                .map(|glyph| (start, *glyph))
        })
        .collect()
}

/// `_baselineOffset` del addon: línea base dentro de la celda (px CSS).
pub fn liga_baseline(font_size: f64, cell_h: f64) -> f64 {
    let scale = font_size / UNITS_PER_EM;
    let (ascent, descent) = (HHEA_ASCENT * scale, HHEA_DESCENT * scale);
    let pad = ((cell_h - (ascent + descent)) / 2.0).max(0.0);
    pad + ascent
}

/// Ruta de la fuente de ligaduras, relativa a la página de la terminal
/// (`/term/`), como la del addon.
pub const LIGA_FONT_URL: &str =
    "../assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-Regular.ttf";
/// Familia con la que se registra la cara de ligaduras.
pub const LIGA_FAMILY: &str = "comandos-liga";
/// Espera del addon antes de repintar las ligaduras (ms).
pub const LIGA_DEBOUNCE_MS: i32 = 90;

pub use web::{Ligatures, Overlay};

mod web {
    use super::CellRect;
    use crate::{links::Underline, metrics::CellMetrics};
    use wasm_bindgen::{JsCast, JsValue};
    use web_sys::{CanvasRenderingContext2d, Document, HtmlCanvasElement};

    fn layer(
        document: &Document,
        z: &str,
    ) -> Result<(HtmlCanvasElement, CanvasRenderingContext2d), JsValue> {
        let canvas: HtmlCanvasElement = document.create_element("canvas")?.dyn_into()?;
        let style = canvas.style();
        style.set_property("position", "absolute")?;
        style.set_property("left", "0")?;
        style.set_property("top", "0")?;
        style.set_property("pointer-events", "none")?;
        style.set_property("z-index", z)?;
        let opts = js_sys::Object::new();
        js_sys::Reflect::set(&opts, &"alpha".into(), &JsValue::TRUE)?;
        let ctx: CanvasRenderingContext2d = canvas
            .get_context_with_context_options("2d", &opts)?
            .ok_or_else(|| JsValue::from_str("canvas 2d no disponible"))?
            .dyn_into()
            .map_err(JsValue::from)?;
        Ok((canvas, ctx))
    }

    /// Copia el tamaño (almacén y CSS) del canvas del texto. `true` si
    /// cambió (el canvas quedó vacío).
    fn follow(canvas: &HtmlCanvasElement, main: &HtmlCanvasElement) -> bool {
        let (w, h) = (main.width(), main.height());
        let main_style = main.style();
        let style = canvas.style();
        for name in ["width", "height"] {
            let want = main_style.get_property_value(name).unwrap_or_default();
            if style.get_property_value(name).unwrap_or_default() != want {
                let _ = style.set_property(name, &want);
            }
        }
        if (canvas.width(), canvas.height()) == (w, h) {
            return false;
        }
        canvas.set_width(w);
        canvas.set_height(h);
        true
    }

    /// Capa de selección y enlace.
    pub struct Overlay {
        canvas: HtmlCanvasElement,
        ctx: CanvasRenderingContext2d,
        /// Hay que repintarla en el próximo cuadro.
        pub dirty: bool,
        /// Ya hay algo dibujado (si no, borrar sobra).
        painted: bool,
    }

    impl Overlay {
        pub fn new(document: &Document) -> Result<Overlay, JsValue> {
            let (canvas, ctx) = layer(document, "1")?;
            Ok(Overlay {
                canvas,
                ctx,
                dirty: true,
                painted: false,
            })
        }

        pub fn element(&self) -> &HtmlCanvasElement {
            &self.canvas
        }

        /// Repinta la selección (`selection` = color CSS con alfa) y el
        /// subrayado del enlace (`link` = color CSS).
        pub fn paint(
            &mut self,
            main: &HtmlCanvasElement,
            m: &CellMetrics,
            selection: (&[CellRect], &str),
            link: (&[Underline], &str),
        ) {
            self.dirty = false;
            let resized = follow(&self.canvas, main);
            if self.painted && !resized {
                self.ctx.clear_rect(
                    0.0,
                    0.0,
                    f64::from(self.canvas.width()),
                    f64::from(self.canvas.height()),
                );
            }
            self.painted = !selection.0.is_empty() || !link.0.is_empty();
            let (cw, ch) = (f64::from(m.dev_w), f64::from(m.dev_h));
            if !selection.0.is_empty() {
                self.ctx.set_fill_style_str(selection.1);
                for r in selection.0 {
                    self.ctx.fill_rect(
                        f64::from(r.x) * cw,
                        f64::from(r.y) * ch,
                        f64::from(r.w) * cw,
                        f64::from(r.h) * ch,
                    );
                }
            }
            if !link.0.is_empty() {
                self.ctx.set_fill_style_str(link.1);
                for &(x, y, w) in link.0 {
                    // `_fillBottomLineAtCells`: una raya de `dpr` sobre el
                    // último píxel de la celda.
                    self.ctx.fill_rect(
                        f64::from(x) * cw,
                        (f64::from(y) + 1.0) * ch - m.dpr - 1.0,
                        f64::from(w) * cw,
                        m.dpr,
                    );
                }
            }
        }
    }

    /// Capa de ligaduras.
    pub struct Ligatures {
        canvas: HtmlCanvasElement,
        ctx: CanvasRenderingContext2d,
        /// La cara `comandos-liga` terminó de cargar.
        pub loaded: bool,
        /// Temporizador de la espera de 90 ms.
        pub timer: Option<i32>,
        painted: bool,
    }

    impl Ligatures {
        pub fn new(document: &Document) -> Result<Ligatures, JsValue> {
            let (canvas, ctx) = layer(document, "9999")?;
            Ok(Ligatures {
                canvas,
                ctx,
                loaded: false,
                timer: None,
                painted: false,
            })
        }

        pub fn element(&self) -> &HtmlCanvasElement {
            &self.canvas
        }

        /// Repinta todas las ligaduras de la vista. `rows` da, por fila de
        /// la vista, las secuencias `(columna, texto)`.
        pub fn paint(
            &mut self,
            main: &HtmlCanvasElement,
            css_canvas: (f64, f64),
            grid: (u16, u16),
            font_size: f64,
            colors: (&str, &str),
            rows: &[Vec<(usize, &'static str)>],
        ) {
            let resized = follow(&self.canvas, main);
            let (w, h) = (
                f64::from(self.canvas.width()),
                f64::from(self.canvas.height()),
            );
            if self.painted || resized {
                let _ = self.ctx.set_transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
                self.ctx.clear_rect(0.0, 0.0, w, h);
            }
            self.painted = false;
            let (cols, n_rows) = grid;
            if !self.loaded || cols == 0 || n_rows == 0 || css_canvas.0 <= 0.0 {
                return;
            }
            // Como el addon: se dibuja en píxeles CSS con la escala del dpr.
            let (sx, sy) = (w / css_canvas.0, h / css_canvas.1);
            let _ = self.ctx.set_transform(sx, 0.0, 0.0, sy, 0.0, 0.0);
            let cell_w = css_canvas.0 / f64::from(cols);
            let cell_h = css_canvas.1 / f64::from(n_rows);
            let baseline = super::liga_baseline(font_size, cell_h);
            self.ctx
                .set_font(&format!("{font_size}px {}", super::LIGA_FAMILY));
            self.ctx.set_text_baseline("alphabetic");
            self.ctx.set_text_align("left");
            let (bg, fg) = colors;
            for (y, spans) in rows.iter().enumerate() {
                let py = y as f64 * cell_h;
                for &(col, seq) in spans {
                    let px = col as f64 * cell_w;
                    let span_w = seq.chars().count() as f64 * cell_w;
                    self.ctx.set_fill_style_str(bg);
                    self.ctx.fill_rect(px, py, span_w, cell_h);
                    let advance = self.ctx.measure_text(seq).map_or(span_w, |t| t.width());
                    self.ctx.set_fill_style_str(fg);
                    let _ = self
                        .ctx
                        .fill_text(seq, px + (span_w - advance) / 2.0, py + baseline);
                    self.painted = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: u16, y: u16, w: u16, h: u16) -> CellRect {
        CellRect { x, y, w, h }
    }

    #[test]
    fn selection_rects_follow_redraw_selection() {
        // Una fila.
        assert_eq!(
            selection_rects((2, 3), (2, 7), 0, 10, 20),
            vec![rect(3, 2, 4, 1)]
        );
        // Tres filas: final de la primera, la del medio entera, principio de la última.
        assert_eq!(
            selection_rects((1, 15), (3, 4), 0, 10, 20),
            vec![rect(15, 1, 5, 1), rect(0, 2, 20, 1), rect(0, 3, 4, 1)]
        );
        // Empieza en la historia: arriba se rellena desde la columna 0.
        assert_eq!(
            selection_rects((-5, 9), (1, 2), 2, 10, 20),
            vec![rect(0, 0, 20, 1), rect(0, 1, 20, 2), rect(0, 3, 2, 1)]
        );
        // Sigue por debajo de la vista.
        assert_eq!(
            selection_rects((8, 1), (40, 2), 0, 10, 20),
            vec![rect(1, 8, 19, 1), rect(0, 9, 20, 1)]
        );
        // Fuera de la vista.
        assert!(selection_rects((-9, 0), (-8, 3), 0, 10, 20).is_empty());
        assert!(selection_rects((12, 0), (13, 3), 0, 10, 20).is_empty());
        // Una selección de toda la historia no recorre filas invisibles.
        assert_eq!(
            selection_rects((-1_000_000, 0), (1_000_000, 0), 0, 3, 20).len(),
            3
        );
    }

    #[test]
    fn block_rects_are_one_column_band_over_the_visible_rows() {
        assert_eq!(
            block_rects((1, 3), (4, 7), 0, 10, 20),
            vec![CellRect {
                x: 3,
                y: 1,
                w: 4,
                h: 4
            }]
        );
        // Recortado a la vista desplazada y a las columnas.
        assert_eq!(
            block_rects((-9, 15), (1, 30), 2, 10, 20),
            vec![CellRect {
                x: 15,
                y: 0,
                w: 5,
                h: 4
            }]
        );
        assert!(block_rects((12, 0), (14, 3), 0, 10, 20).is_empty());
        assert!(
            block_rects((1, 4), (2, 4), 0, 10, 20).is_empty(),
            "sin ancho"
        );
    }

    #[test]
    fn ligatures_need_a_glyph_in_the_font() {
        assert_eq!(liga_spans("a <=> b != c"), vec![(2, "<=>"), (8, "!=")]);
        // `<===>` no tiene glifo: el addon no prueba una más corta dentro.
        assert_eq!(liga_spans("<===>"), vec![]);
        // `**` tampoco, pero `***` sí.
        assert_eq!(liga_spans("a ** b *** c"), vec![(7, "***")]);
        assert_eq!(liga_spans("fn x() -> u8"), vec![(7, "->")]);
    }

    #[test]
    fn glyph_list_is_a_subset_of_the_candidates() {
        for glyph in LIGA_GLYPHS {
            assert_eq!(
                ligature_runs(glyph),
                vec![(0, glyph.chars().count())],
                "{glyph}"
            );
        }
    }

    #[test]
    fn baseline_uses_the_font_hhea_metrics() {
        // 14 px con interlineado 1,2: la línea (18,48 px) no cabe, sin relleno.
        assert!((liga_baseline(14.0, 16.8) - 14.28).abs() < 1e-9);
        assert!((liga_baseline(10.0, 20.0) - (3.4 + 10.2)).abs() < 1e-9);
    }
}
