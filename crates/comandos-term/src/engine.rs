//! Motor VT: `alacritty_terminal` con el reloj inyectado de `clock`, las
//! respuestas DA de xterm.js 5.5.0 y una vista estable de modos, daños y
//! eventos para el puente y el renderizador. No abre PTY ni toca tmux.
//!
//! Superconjunto deliberado de xterm.js 5.5.0: el modo 2026 (actualización
//! sincronizada) existe y DECRQM `?2026` contesta `;2$y`; ver la
//! documentación del crate.
use crate::{
    clock::{ClockSync, set_now},
    osc::OscLimiter,
};
use alacritty_terminal::{
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    term::{
        ClipboardType, Config, MIN_COLUMNS, MIN_SCREEN_LINES, Term, TermDamage, TermMode,
        color::COUNT as COLOR_COUNT,
    },
    vte::ansi::{Processor, Rgb},
};
use std::{cell::Cell, rc::Rc, sync::Arc};

/// Respuesta DA1 de xterm.js 5.5.0 (`CSI ? 1 ; 2 c`).
pub const DA1_REPLY: &[u8] = b"\x1b[?1;2c";
/// Respuesta DA2 de xterm.js 5.5.0; `config/terminal-replies.conf` la traga.
pub const DA2_REPLY: &[u8] = b"\x1b[>0;276;0c";

/// Tope de bytes de respuestas al PTY entre dos `drain`. Una respuesta
/// normal (DA, DSR, CPR, color) mide menos de 40 bytes: caben miles.
pub const MAX_REPLY_BYTES: usize = 64 * 1024;

/// Coste fijo de una consulta de color o de tamaño (su respuesta real es
/// menor; se cobra antes de formatearla).
const REQUEST_REPLY_COST: usize = 64;

/// Tope de un título (OSC 0/2): se recorta sin partir un carácter. La
/// cabecera de un pane muestra unas decenas de caracteres.
pub const MAX_TITLE_BYTES: usize = 4 * 1024;

/// Tope del texto copiado por OSC 52. Cubre copiar una respuesta larga de un
/// agente o un archivo mediano; lo que lo supere se descarta entero. Es
/// coherente con [`crate::osc::MAX_OSC_BYTES`], que admite su base64.
pub const MAX_CLIPBOARD_BYTES: usize = 1024 * 1024;

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
    /// Destino de `clipboard` (es `Some` exactamente cuando `clipboard` lo es).
    pub clipboard_target: Option<ClipboardTarget>,
    /// Sonó la campana al menos una vez.
    pub bell: bool,
}

/// Destino de un OSC 52. alacritty junta `p` (primaria) y `s` (selección) en
/// `Selection`; `c` es el portapapeles normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardTarget {
    Clipboard,
    Selection,
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
    /// Índices de línea tal como los da alacritty: línea de pantalla **más**
    /// `display_offset`, es decir, la fila de la vista donde se ve esa línea
    /// (alacritty ya quita las que caen debajo de la vista; quien pinta solo
    /// recorta a la pantalla). Desplazar la vista llega como `Full`.
    Lines(Vec<usize>),
}

/// Formateador que alacritty adjunta a una consulta de color.
type ColorFormat = Arc<dyn Fn(Rgb) -> String + Sync + Send + 'static>;

/// Respuesta pendiente; las de color y tamaño se arman en `drain`, que
/// conoce la paleta, los colores cambiados por OSC y la celda en píxeles.
enum Reply {
    /// Ya reescrita (DA de xterm.js), para que su coste sea el real.
    Bytes(Vec<u8>),
    Color(usize, ColorFormat),
    /// CSI 14 t. Se arma aquí en `u32`: el formateador de alacritty
    /// multiplica en `u16` y se desborda con rejillas o celdas grandes.
    TextAreaPixels,
}

impl Reply {
    /// Bytes que cuenta contra [`MAX_REPLY_BYTES`].
    fn cost(&self) -> usize {
        match self {
            Reply::Bytes(bytes) => bytes.len(),
            Reply::Color(..) | Reply::TextAreaPixels => REQUEST_REPLY_COST,
        }
    }
}

/// Lo acumulado entre dos `drain`, ya acotado: las respuestas por bytes,
/// y de título, portapapeles y campana solo el último estado.
#[derive(Default)]
struct Pending {
    replies: Vec<Reply>,
    reply_bytes: usize,
    title: Option<String>,
    clipboard: Option<(ClipboardTarget, String)>,
    bell: bool,
}

/// Recolector de eventos: `send_event(&self)` exige mutabilidad interior;
/// `Cell` con `take`/`set` no puede entrar en pánico (`RefCell` sí).
///
/// Nada barato puede desplazar una respuesta: los eventos que `drain` no usa
/// se ignoran al llegar, título/portapapeles/campana se sobrescriben y solo
/// las respuestas al PTY ocupan cola, acotada por bytes. Si se llena, se
/// conservan las primeras (la aplicación espera las respuestas en orden).
#[derive(Clone, Default)]
pub struct Collector(Rc<Cell<Pending>>);

impl Collector {
    fn take(&self) -> Pending {
        self.0.take()
    }
}

impl EventListener for Collector {
    fn send_event(&self, event: Event) {
        let mut pending = self.0.take();
        match event {
            Event::PtyWrite(text) => {
                pending.push_reply(Reply::Bytes(rewrite_reply(text.as_bytes()).to_vec()))
            }
            Event::ColorRequest(index, format) => pending.push_reply(Reply::Color(index, format)),
            // En alacritty 0.26 solo CSI 14 t emite este evento.
            Event::TextAreaSizeRequest(_) => pending.push_reply(Reply::TextAreaPixels),
            Event::Title(title) => pending.title = Some(bounded_title(title)),
            Event::ResetTitle => pending.title = Some(String::new()),
            // Un texto mayor que el tope se descarta entero: copiar media
            // salida sería peor que no copiar.
            Event::ClipboardStore(kind, text) if text.len() <= MAX_CLIPBOARD_BYTES => {
                let target = match kind {
                    ClipboardType::Clipboard => ClipboardTarget::Clipboard,
                    ClipboardType::Selection => ClipboardTarget::Selection,
                };
                pending.clipboard = Some((target, text));
            }
            Event::Bell => pending.bell = true,
            _ => {}
        }
        self.0.set(pending);
    }
}

impl Pending {
    fn push_reply(&mut self, reply: Reply) {
        let cost = reply.cost();
        if self.reply_bytes.saturating_add(cost) <= MAX_REPLY_BYTES {
            self.reply_bytes += cost;
            self.replies.push(reply);
        }
    }
}

/// Título acotado a [`MAX_TITLE_BYTES`] sin partir un carácter.
fn bounded_title(mut title: String) -> String {
    if title.len() > MAX_TITLE_BYTES {
        let cut = title.floor_char_boundary(MAX_TITLE_BYTES);
        title.truncate(cut);
    }
    title
}

/// Emulador de una terminal: bytes del PTY dentro, rejilla y eventos fuera.
pub struct Engine {
    term: Term<Collector>,
    parser: Processor<ClockSync>,
    osc: OscLimiter,
    events: Collector,
    palette: Palette,
    cell_px: (u16, u16),
    size: GridSize,
}

impl Engine {
    /// Terminal vacía de `size` celdas con `scrollback` líneas de historia.
    pub fn new(size: GridSize, scrollback: usize, palette: Palette) -> Engine {
        Engine::with_cursor_blink(size, scrollback, palette, false)
    }

    /// Como [`Engine::new`], con el parpadeo del cursor por omisión
    /// (`cursorBlink` de la página). DECSCUSR y el modo 12 lo cambian; un
    /// DECSCUSR 0 vuelve a este valor.
    pub fn with_cursor_blink(
        size: GridSize,
        scrollback: usize,
        palette: Palette,
        blink: bool,
    ) -> Engine {
        let size = size.clamped();
        let events = Collector::default();
        let mut config = Config {
            scrolling_history: scrollback,
            ..Config::default()
        };
        config.default_cursor_style.blinking = blink;
        let term = Term::new(config, &size, events.clone());
        Engine {
            term,
            parser: Processor::new(),
            osc: OscLimiter::default(),
            events,
            palette,
            cell_px: DEFAULT_CELL_PX,
            size,
        }
    }

    /// Procesa bytes del PTY; `now_ms` es el reloj del llamador.
    pub fn advance(&mut self, bytes: &[u8], now_ms: f64) {
        let now_ms = set_now(now_ms);
        // Si una actualización sincronizada venció sin `tick`, se vuelca antes:
        // si no, los bytes nuevos se pintarían antes que los retenidos.
        if self.sync_expired(now_ms) {
            self.parser.stop_sync(&mut self.term);
        }
        let (parser, term) = (&mut self.parser, &mut self.term);
        self.osc.filter(bytes, |chunk| parser.advance(term, chunk));
    }

    /// Instante (ms) en que vence la actualización sincronizada abierta.
    pub fn next_deadline_ms(&self) -> Option<f64> {
        self.parser.sync_timeout().deadline_ms()
    }

    /// Cierra una actualización sincronizada vencida; `true` si volcó algo.
    pub fn tick(&mut self, now_ms: f64) -> bool {
        let now_ms = set_now(now_ms);
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

    /// Cambia la paleta del tema (la que contesta a OSC 4/10/11/12).
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    /// Recoge respuestas y eventos pendientes desde el último `drain`.
    pub fn drain(&mut self) -> Drained {
        let pending = self.events.take();
        let mut out = Drained {
            replies: Vec::with_capacity(pending.reply_bytes),
            title: pending.title,
            clipboard_target: pending.clipboard.as_ref().map(|(target, _)| *target),
            clipboard: pending.clipboard.map(|(_, text)| text),
            bell: pending.bell,
        };
        for reply in pending.replies {
            match reply {
                Reply::Bytes(bytes) => out.replies.extend_from_slice(&bytes),
                Reply::Color(index, format) => {
                    let color = self.color_for_request(index);
                    out.replies.extend_from_slice(format(color).as_bytes());
                }
                Reply::TextAreaPixels => {
                    let height = u32::from(self.size.rows) * u32::from(self.cell_px.1);
                    let width = u32::from(self.size.cols) * u32::from(self.cell_px.0);
                    out.replies
                        .extend_from_slice(format!("\x1b[4;{height};{width}t").as_bytes());
                }
            }
        }
        out
    }

    /// Color que se contesta a OSC 4/10/11/12 `?`: primero el que la
    /// aplicación cambió por OSC (`Term::colors`), luego la paleta del tema.
    fn color_for_request(&self, index: usize) -> Rgb {
        // `Colors` solo ofrece `Index`; el índice se comprueba antes.
        let changed = if index < COLOR_COUNT {
            self.term.colors()[index]
        } else {
            None
        };
        changed.unwrap_or_else(|| {
            let [r, g, b] = self.palette.color_for_request(index);
            Rgb { r, g, b }
        })
    }

    /// Líneas que cambiaron desde la última llamada (y las reinicia).
    pub fn take_damage(&mut self) -> Damage {
        let mut lines = Vec::new();
        if self.take_damage_into(&mut lines) {
            Damage::Full
        } else {
            Damage::Lines(lines)
        }
    }

    /// Como [`Engine::take_damage`] sin asignar: vacía `lines`, la llena con
    /// los índices dañados (mismo significado que [`Damage::Lines`]) y
    /// devuelve `true` si está todo dañado (entonces `lines` queda vacía).
    /// Ojo: alacritty daña siempre la línea del cursor.
    pub fn take_damage_into(&mut self, lines: &mut Vec<usize>) -> bool {
        lines.clear();
        let full = match self.term.damage() {
            TermDamage::Full => true,
            TermDamage::Partial(damaged) => {
                lines.extend(damaged.map(|bounds| bounds.line));
                false
            }
        };
        self.term.reset_damage();
        full
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
