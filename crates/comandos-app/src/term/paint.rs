//! Plan de pintado por fila, sin cairo: rectángulos de fondo, tiras de texto,
//! glifos de caja (U+2500–U+259F y Powerline, de `comandos-term::glyphs`) y
//! líneas de subrayado/tachado, en píxeles del widget. T6 lo ejecuta.
use crate::term::engine::{CellMetrics, DrawOp, Palette, RowRender, RunKind, Underline, draw_ops};

pub fn hex_rgb(h: &str) -> Option<[u8; 3]> {
    let digits = h.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| {
        digits
            .get(i..i + 2)
            .and_then(|s| u8::from_str_radix(s, 16).ok())
    };
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// `set_colors(fg, bg, pal)` + `set_color_cursor(cursor)` de `make_term`.
pub fn palette_from_theme(fg: &str, bg: &str, cursor: &str, pal16: &[&str]) -> Option<Palette> {
    if pal16.len() != 16 {
        return None;
    }
    let (fg, bg, cursor) = (hex_rgb(fg)?, hex_rgb(bg)?, hex_rgb(cursor)?);
    let mut p = Palette::xterm_default(fg, bg, cursor, bg, fg);
    for (slot, hex) in p.ansi.iter_mut().zip(pal16.iter().take(16)) {
        *slot = hex_rgb(hex)?;
    }
    Some(p)
}

/// Atenuado de VTE (`rgb_from_index`): 2/3 de cada canal en 16 bits.
pub fn vte_dim(c: [u8; 3]) -> [u8; 3] {
    c.map(|v| {
        let wide = u32::from(v) * 257 * 2 / 3;
        u8::try_from(wide / 257).unwrap_or(u8::MAX)
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellGeom {
    pub cell_w: f64,
    pub cell_h: f64,
    /// Esquina superior izquierda de la celda (0, 0) dentro del widget (márgenes de `make_term`).
    pub origin_x: f64,
    pub origin_y: f64,
    pub dpr: f64,
    pub font_size: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Under(Underline),
    Strike,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaintOp {
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        rgb: [u8; 3],
    },
    Text {
        col: u16,
        x: f64,
        y: f64,
        cells: u16,
        text: String,
        rgb: [u8; 3],
        bold: bool,
        italic: bool,
    },
    Glyph {
        x: f64,
        y: f64,
        ops: Vec<DrawOp>,
        rgb: [u8; 3],
    },
    Line {
        x: f64,
        y: f64,
        w: f64,
        rgb: [u8; 3],
        kind: LineKind,
    },
}

pub fn plan_row(
    row: &RowRender,
    line: usize,
    g: &CellGeom,
    fg_is_rgb: &dyn Fn(u16) -> bool,
    out: &mut Vec<PaintOp>,
) {
    let y = g.origin_y + g.cell_h * f64::from(u32::try_from(line).unwrap_or(u32::MAX));
    let x_of = |col: u16| g.origin_x + g.cell_w * f64::from(col);
    for &(col, cells, rgb) in &row.bg_runs {
        out.push(PaintOp::Rect {
            x: x_of(col),
            y,
            w: g.cell_w * f64::from(cells),
            h: g.cell_h,
            rgb,
        });
    }
    let metrics = CellMetrics {
        cell_w: g.cell_w * g.dpr,
        cell_h: g.cell_h * g.dpr,
        dpr: g.dpr,
        font_size: g.font_size,
    };
    for run in &row.runs {
        if run.style.hidden {
            continue;
        }
        // xterm render runs merge equal resolved styles. VTE dim depends on
        // color provenance, so equal RGB/indexed cells must be separated here.
        let mut segments: Vec<(u16, u16, String, bool)> = Vec::new();
        if run.style.dim && run.kind == RunKind::Text && run.cells > 1 {
            for (col, text) in run.cell_texts() {
                let rgb = fg_is_rgb(col);
                if let Some(last) = segments
                    .last_mut()
                    .filter(|s| s.3 == rgb && s.0.saturating_add(s.1) == col)
                {
                    last.1 = last.1.saturating_add(1);
                    last.2.push_str(text);
                } else {
                    segments.push((col, 1, text.to_owned(), rgb));
                }
            }
        } else {
            segments.push((run.col, run.cells, run.text.clone(), fg_is_rgb(run.col)));
        }
        for (col, cells, text, direct_rgb) in segments {
            let base = run.style.dim_fg.unwrap_or(run.style.fg);
            let rgb = if run.style.dim && !direct_rgb {
                vte_dim(base)
            } else if run.style.dim {
                base
            } else {
                run.style.fg
            };
            let mut glyph = false;
            if let RunKind::Box(c) = run.kind {
                let mut ops = Vec::new();
                if draw_ops(c, &metrics, &mut ops) {
                    out.push(PaintOp::Glyph {
                        x: x_of(col),
                        y,
                        ops,
                        rgb,
                    });
                    glyph = true;
                }
            }
            if !glyph {
                out.push(PaintOp::Text {
                    col,
                    x: x_of(col),
                    y,
                    cells,
                    text,
                    rgb,
                    bold: run.style.bold,
                    italic: run.style.italic,
                });
            }
            let w = g.cell_w * f64::from(cells);
            let line_rgb = run.style.underline_color.unwrap_or(rgb);
            if run.style.underline != Underline::None {
                out.push(PaintOp::Line {
                    x: x_of(col),
                    y,
                    w,
                    rgb: line_rgb,
                    kind: LineKind::Under(run.style.underline),
                });
            }
            if run.style.strike {
                out.push(PaintOp::Line {
                    x: x_of(col),
                    y,
                    w,
                    rgb,
                    kind: LineKind::Strike,
                });
            }
        }
    }
}
