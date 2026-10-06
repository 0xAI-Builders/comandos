//! Única puerta a `comandos-term` (F22): el resto del crate importa de aquí. Si la
//! Fase 3 cambia una firma, solo cambia este archivo y quien use lo cambiado.
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Line;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::{Color, NamedColor};
pub use comandos_term::engine::{
    ClipboardTarget, Damage, Drained, Engine, GridSize, Modes, MouseMode, Palette,
};
pub use comandos_term::glyphs::{CellMetrics, DrawOp, draw_ops};
pub use comandos_term::input::{
    Button, Ime, KeyAction, KeyInput, MouseKind, WheelAction, encode_key, encode_mouse,
    encode_paste, focus, wheel,
};
pub use comandos_term::render::{
    CursorShape, CursorView, RenderOpts, RowRender, Run, RunKind, Style, Underline, cursor,
    render_row_into,
};
pub use comandos_term::select::{
    Point, SelectMode, Selection, UrlSpan, find_urls, selected_text, selection_bounds, word_at,
};
use std::time::{Duration, Instant};

/// `set_scrollback_lines(10000)` de `make_term` (795).
pub const SCROLLBACK: usize = 10_000;
/// Celdas del mosaico (`_mosaic_term`).
pub const MOSAIC_SCROLLBACK: usize = 400;
/// VTE: la negrita no aclara el color; sin ajuste de contraste.
pub const VTE_OPTS: RenderOpts = RenderOpts {
    bold_is_bright: false,
    min_contrast: 1.0,
};

pub struct TermEngine {
    engine: Engine,
    epoch: Instant,
    palette: Palette,
    cols: u16,
    rows: u16,
}

impl TermEngine {
    pub fn new(
        cols: u16,
        rows: u16,
        scrollback: usize,
        palette: Palette,
        blink: bool,
        epoch: Instant,
    ) -> TermEngine {
        let (cols, rows) = (cols.max(2), rows.max(1));
        let engine =
            Engine::with_cursor_blink(GridSize { cols, rows }, scrollback, palette.clone(), blink);
        TermEngine {
            engine,
            epoch,
            palette,
            cols,
            rows,
        }
    }

    fn ms(&self, now: Instant) -> f64 {
        now.saturating_duration_since(self.epoch).as_secs_f64() * 1000.0
    }

    pub fn feed(&mut self, bytes: &[u8], now: Instant) {
        let ms = self.ms(now);
        self.engine.advance(bytes, ms);
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        let ms = self.ms(now);
        self.engine.tick(ms)
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        let ms = self.engine.next_deadline_ms()?;
        Some(self.epoch + Duration::from_secs_f64((ms / 1000.0).max(0.0)))
    }

    pub fn take_dirty(&mut self, lines: &mut Vec<usize>) -> bool {
        if self.engine.next_deadline_ms().is_some() {
            lines.clear();
            return false;
        }
        self.engine.take_damage_into(lines)
    }

    pub fn render_line(&self, line: usize, out: &mut RowRender) {
        render_row_into(&self.engine, line, &self.palette, &VTE_OPTS, out);
        // xterm drops backgrounds equal to the theme. Desktop opacity still
        // requires an opaque rectangle for explicit colors, including spaces.
        let Some(cells) = self.row_cells(line) else {
            return;
        };
        let mut covered = vec![false; cells.len()];
        for (start, count, _) in &out.bg_runs {
            let start = usize::from(*start);
            let end = start.saturating_add(usize::from(*count)).min(covered.len());
            if let Some(span) = covered.get_mut(start..end) {
                span.fill(true);
            }
        }
        for (column, cell) in cells.iter().enumerate() {
            let Ok(col) = u16::try_from(column) else {
                continue;
            };
            let explicit = cell.bg != Color::Named(NamedColor::Background)
                || cell.flags.contains(Flags::INVERSE);
            if explicit && !covered.get(column).copied().unwrap_or(false) {
                out.bg_runs.push((col, 1, self.palette.bg));
            }
        }
        out.bg_runs.sort_by_key(|run| run.0);
        out.bg_runs.dedup_by(|next, previous| {
            if previous.2 == next.2 && previous.0.saturating_add(previous.1) == next.0 {
                previous.1 = previous.1.saturating_add(next.1);
                true
            } else {
                false
            }
        });
    }

    pub fn cursor(&self) -> CursorView {
        cursor(&self.engine)
    }

    pub fn modes(&self) -> Modes {
        self.engine.modes()
    }

    pub fn drain(&mut self) -> Drained {
        self.engine.drain()
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell_px: (u16, u16)) {
        let (cols, rows) = (cols.max(2), rows.max(1));
        self.cols = cols;
        self.rows = rows;
        self.engine.resize(GridSize { cols, rows }, cell_px);
    }

    pub fn set_palette(&mut self, palette: Palette) {
        self.engine.set_palette(palette.clone());
        self.palette = palette;
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    pub fn scroll(&mut self, lines: i32) {
        self.engine.scroll_display(lines);
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    fn visible_line(&self, line: usize) -> Option<Line> {
        let grid = self.engine.term().grid();
        if line >= grid.screen_lines() {
            return None;
        }
        let line = i32::try_from(line).ok()?;
        let offset = i32::try_from(self.engine.display_offset()).ok()?;
        Some(Line(line.checked_sub(offset)?))
    }
    // Grid exposes Index only. Validate both viewport and storage bounds before
    // accessing one row; callers then use checked slice access for columns.
    #[allow(clippy::indexing_slicing)]
    fn row_cells(&self, line: usize) -> Option<&[Cell]> {
        let grid = self.engine.term().grid();
        let point = self.visible_line(line)?;
        if point < grid.topmost_line() || point > grid.bottommost_line() {
            return None;
        }
        Some(&grid[point][..])
    }

    /// El color de primer plano de la celda es RGB directo (VTE no lo atenúa).
    pub fn fg_is_rgb(&self, line: usize, col: u16) -> bool {
        self.row_cells(line)
            .and_then(|cells| cells.get(usize::from(col)))
            .is_some_and(|cell| {
                let color = if cell.flags.contains(Flags::INVERSE) {
                    cell.bg
                } else {
                    cell.fg
                };
                matches!(color, Color::Spec(_))
            })
    }
}
