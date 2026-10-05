//! Motor VT: `alacritty_terminal` con el reloj inyectado de `clock`, las
//! respuestas DA de xterm.js 5.5.0 y una vista estable de modos, daños y
//! eventos para el puente y el renderizador. No abre PTY ni toca tmux.
use crate::clock::{ClockSync, set_now};
use alacritty_terminal::{
    event::{Event, EventListener, WindowSize},
    grid::{Dimensions, Scroll},
    term::{Config, MIN_COLUMNS, MIN_SCREEN_LINES, Term, TermDamage, TermMode},
    vte::ansi::{Processor, Rgb},
};
use std::{cell::Cell, rc::Rc};

/// Respuesta DA1 de xterm.js 5.5.0 (`CSI ? 1 ; 2 c`).
pub const DA1_REPLY: &[u8] = b"\x1b[?1;2c";
/// Respuesta DA2 de xterm.js 5.5.0; `config/terminal-replies.conf` la traga.
pub const DA2_REPLY: &[u8] = b"\x1b[>0;276;0c";

/// Máximo de eventos pendientes entre dos `drain`: un flujo hostil no hace
/// crecer la cola sin límite (lo que exceda se descarta).
const MAX_PENDING_EVENTS: usize = 4096;

/// Celda en píxeles por omisión hasta el primer `resize` (la letra de 11 px).
const DEFAULT_CELL_PX: (u16, u16) = (8, 16);

/// Los 16 colores de `DEFAULT_ANSI_COLORS` de xterm.js 5.5.
const XTERM_ANSI_16: [[u8; 3]; 16] = [
    [0x2E, 0x34, 0x36],
    [0xCC, 0x00, 0x00],
    [0x4E, 0x9A, 0x06],
    [0xC4, 0xA0, 0x00],
    [0x34, 0x65, 0xA4],
    [0x75, 0x50, 0x7B],
    [0x06, 0x98, 0x9A],
    [0xD3, 0xD7, 0xCF],
    [0x55, 0x57, 0x53],
    [0xEF, 0x29, 0x29],
    [0x8A, 0xE2, 0x34],
    [0xFC, 0xE9, 0x4F],
    [0x72, 0x9F, 0xCF],
    [0xAD, 0x7F, 0xA8],
    [0x34, 0xE2, 0xE2],
    [0xEE, 0xEE, 0xEC],
];

/// Niveles del cubo 6×6×6 de xterm.js (índices 16–231).
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Índices de los colores con nombre en `ColorRequest` (los de alacritty).
const REQUEST_FOREGROUND: usize = 256;
const REQUEST_BACKGROUND: usize = 257;
const REQUEST_CURSOR: usize = 258;

/// Tamaño de la rejilla en celdas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
}

impl GridSize {
    /// El mismo tamaño con los mínimos de alacritty (2 columnas, 1 fila):
    /// una rejilla vacía haría fallar sus índices internos.
    fn clamped(self) -> GridSize {
        GridSize {
            cols: self.cols.max(u16::try_from(MIN_COLUMNS).unwrap_or(2)),
            rows: self.rows.max(u16::try_from(MIN_SCREEN_LINES).unwrap_or(1)),
        }
    }
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows).max(MIN_SCREEN_LINES)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols).max(MIN_COLUMNS)
    }
}

/// Colores del tema; `ansi` cubre los 256 índices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    pub cursor: [u8; 3],
    pub cursor_accent: [u8; 3],
    pub selection: [u8; 3],
    pub ansi: [[u8; 3]; 256],
}

impl Palette {
    /// Paleta de xterm.js 5.5: 16 colores base, cubo 6×6×6 y 24 grises.
    pub fn xterm_default(
        fg: [u8; 3],
        bg: [u8; 3],
        cursor: [u8; 3],
        cursor_accent: [u8; 3],
        selection: [u8; 3],
    ) -> Palette {
        let mut ansi = [[0_u8; 3]; 256];
        for (index, slot) in ansi.iter_mut().enumerate() {
            *slot = xterm_color(index);
        }
        Palette {
            fg,
            bg,
            cursor,
            cursor_accent,
            selection,
            ansi,
        }
    }

    /// Color que se contesta a una consulta OSC 4/10/11/12: 0–255 → `ansi`,
    /// 256 → `fg`, 257 → `bg`, 258 → `cursor`; cualquier otro → `fg`.
    pub fn color_for_request(&self, index: usize) -> [u8; 3] {
        match index {
            REQUEST_FOREGROUND => self.fg,
            REQUEST_BACKGROUND => self.bg,
            REQUEST_CURSOR => self.cursor,
            _ => self.ansi.get(index).copied().unwrap_or(self.fg),
        }
    }
}

/// Color `index` (0–255) de la tabla de xterm.js.
fn xterm_color(index: usize) -> [u8; 3] {
    if let Some(color) = XTERM_ANSI_16.get(index) {
        return *color;
    }
    if (16..232).contains(&index) {
        let n = index - 16;
        let level = |i: usize| CUBE_LEVELS.get(i).copied().unwrap_or(0);
        return [level(n / 36), level((n / 6) % 6), level(n % 6)];
    }
    // Grises 232–255: 8 + 10·i (el máximo, 238, cabe en u8).
    let step = u8::try_from(index.saturating_sub(232)).unwrap_or(0).min(23);
    let gray = 8 + 10 * step;
    [gray, gray, gray]
}

/// Lo que el motor produjo desde el último `drain`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Drained {
    /// Bytes que hay que devolver al PTY (DA, DSR, consultas de color…).
    pub replies: Vec<u8>,
    /// Último título pedido (`Some("")` = volver al título por omisión).
    pub title: Option<String>,
    /// Último texto que la aplicación quiso copiar (OSC 52).
    pub clipboard: Option<String>,
    /// Sonó la campana al menos una vez.
    pub bell: bool,
}

/// Seguimiento del ratón que pidió la aplicación.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MouseMode {
    #[default]
    Off,
    Click,
    Drag,
    Motion,
}

/// Modos de terminal que necesitan la entrada y el render.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modes {
    pub app_cursor: bool,
    pub app_keypad: bool,
    pub bracketed_paste: bool,
    pub mouse: MouseMode,
    pub sgr_mouse: bool,
    pub alt_screen: bool,
    pub alternate_scroll: bool,
    pub focus_events: bool,
    pub cursor_visible: bool,
}

/// Qué hay que repintar desde el último `take_damage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Damage {
    Full,
    /// Índices de línea de pantalla (0 = arriba del todo de lo visible).
    Lines(Vec<usize>),
}

/// Recolector de eventos: `send_event(&self)` exige mutabilidad interior;
/// `Cell<Vec<_>>` con `take`/`set` no puede entrar en pánico (`RefCell` sí).
#[derive(Clone, Default)]
pub struct Collector(Rc<Cell<Vec<Event>>>);

impl EventListener for Collector {
    fn send_event(&self, event: Event) {
        let mut pending = self.0.take();
        if pending.len() < MAX_PENDING_EVENTS {
            pending.push(event);
        }
        self.0.set(pending);
    }
}

/// Emulador de una terminal: bytes del PTY dentro, rejilla y eventos fuera.
pub struct Engine {
    term: Term<Collector>,
    parser: Processor<ClockSync>,
    events: Collector,
    palette: Palette,
    cell_px: (u16, u16),
    size: GridSize,
}

impl Engine {
    /// Terminal vacía de `size` celdas con `scrollback` líneas de historia.
    pub fn new(size: GridSize, scrollback: usize, palette: Palette) -> Engine {
        let size = size.clamped();
        let events = Collector::default();
        let config = Config {
            scrolling_history: scrollback,
            ..Config::default()
        };
        let term = Term::new(config, &size, events.clone());
        Engine {
            term,
            parser: Processor::new(),
            events,
            palette,
            cell_px: DEFAULT_CELL_PX,
            size,
        }
    }

    /// Procesa bytes del PTY; `now_ms` es el reloj del llamador.
    pub fn advance(&mut self, bytes: &[u8], now_ms: f64) {
        set_now(now_ms);
        // Si una actualización sincronizada venció sin `tick`, se vuelca antes:
        // si no, los bytes nuevos se pintarían antes que los retenidos.
        if self.sync_expired(now_ms) {
            self.parser.stop_sync(&mut self.term);
        }
        self.parser.advance(&mut self.term, bytes);
    }

    /// Instante (ms) en que vence la actualización sincronizada abierta.
    pub fn next_deadline_ms(&self) -> Option<f64> {
        self.parser.sync_timeout().deadline_ms()
    }

    /// Cierra una actualización sincronizada vencida; `true` si volcó algo.
    pub fn tick(&mut self, now_ms: f64) -> bool {
        set_now(now_ms);
        if !self.sync_expired(now_ms) {
            return false;
        }
        self.parser.stop_sync(&mut self.term);
        true
    }

    fn sync_expired(&self, now_ms: f64) -> bool {
        self.next_deadline_ms()
            .is_some_and(|deadline| now_ms >= deadline)
    }

    /// Cambia el tamaño en celdas y el de la celda en píxeles (para CSI 14 t).
    pub fn resize(&mut self, size: GridSize, cell_px: (u16, u16)) {
        self.size = size.clamped();
        self.cell_px = cell_px;
        self.term.resize(self.size);
    }

    /// Recoge respuestas y eventos pendientes desde el último `drain`.
    pub fn drain(&mut self) -> Drained {
        let mut out = Drained::default();
        for event in self.events.0.take() {
            match event {
                Event::PtyWrite(text) => {
                    out.replies
                        .extend_from_slice(rewrite_reply(text.as_bytes()));
                }
                Event::ColorRequest(index, format) => {
                    let [r, g, b] = self.palette.color_for_request(index);
                    out.replies
                        .extend_from_slice(format(Rgb { r, g, b }).as_bytes());
                }
                Event::TextAreaSizeRequest(format) => {
                    let size = WindowSize {
                        num_lines: self.size.rows,
                        num_cols: self.size.cols,
                        cell_width: self.cell_px.0,
                        cell_height: self.cell_px.1,
                    };
                    out.replies.extend_from_slice(format(size).as_bytes());
                }
                Event::Title(title) => out.title = Some(title),
                Event::ResetTitle => out.title = Some(String::new()),
                Event::ClipboardStore(_, text) => out.clipboard = Some(text),
                Event::Bell => out.bell = true,
                _ => {}
            }
        }
        out
    }

    /// Líneas que cambiaron desde la última llamada (y las reinicia).
    pub fn take_damage(&mut self) -> Damage {
        let damage = match self.term.damage() {
            TermDamage::Full => Damage::Full,
            TermDamage::Partial(lines) => Damage::Lines(lines.map(|bounds| bounds.line).collect()),
        };
        self.term.reset_damage();
        damage
    }

    /// Desplaza la vista por la historia (positivo = hacia atrás).
    pub fn scroll_display(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    /// Líneas de historia por encima de la vista actual.
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// Líneas guardadas en la historia.
    pub fn history_len(&self) -> usize {
        self.term.grid().history_size()
    }

    /// Acceso de lectura para el render y la selección.
    pub fn term(&self) -> &Term<Collector> {
        &self.term
    }

    /// Modos activos de la aplicación.
    pub fn modes(&self) -> Modes {
        let m = *self.term.mode();
        let mouse = if m.contains(TermMode::MOUSE_MOTION) {
            MouseMode::Motion
        } else if m.contains(TermMode::MOUSE_DRAG) {
            MouseMode::Drag
        } else if m.contains(TermMode::MOUSE_REPORT_CLICK) {
            MouseMode::Click
        } else {
            MouseMode::Off
        };
        Modes {
            app_cursor: m.contains(TermMode::APP_CURSOR),
            app_keypad: m.contains(TermMode::APP_KEYPAD),
            bracketed_paste: m.contains(TermMode::BRACKETED_PASTE),
            mouse,
            sgr_mouse: m.contains(TermMode::SGR_MOUSE),
            alt_screen: m.contains(TermMode::ALT_SCREEN),
            alternate_scroll: m.contains(TermMode::ALTERNATE_SCROLL),
            focus_events: m.contains(TermMode::FOCUS_IN_OUT),
            cursor_visible: m.contains(TermMode::SHOW_CURSOR),
        }
    }
}

/// alacritty contesta DA1 `CSI ? 6 c` y DA2 `CSI > 0 ; <versión> ; 1 c`;
/// tmux y `config/terminal-replies.conf` esperan las de xterm.js
/// (Review Focus 5), así que se reescriben aquí.
fn rewrite_reply(reply: &[u8]) -> &[u8] {
    if reply == b"\x1b[?6c" {
        return DA1_REPLY;
    }
    if reply.starts_with(b"\x1b[>0;") && reply.ends_with(b";1c") {
        return DA2_REPLY;
    }
    reply
}
