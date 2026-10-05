//! Ratón, rueda y selección con el ratón, como xterm.js 5.5.0
//! (`MouseService.getCoords`, `bindMouse`, `CoreMouseService`,
//! `Viewport.getLinesScrolled`/`handleWheel` y `SelectionService`).
//!
//! La parte pura (coordenadas, rueda, deduplicación de movimientos y modelo
//! de selección) se prueba en host; el cableado con el DOM está al final.
use comandos_term::{
    engine::{Modes, MouseMode},
    input::{Button, MouseKind, encode_mouse},
    select::{Point, SelectMode, Selection},
};

/// `Math.round` de JavaScript.
fn js_round(v: f64) -> f64 {
    crate::metrics::js_round(v)
}

/// Celda de un informe de ratón: `getCoords(…, isSelection = false)` de
/// xterm.js (`ceil(x / ancho)` acotado a `[1, cols]`, en base 0). `x` e `y`
/// son píxeles CSS relativos a `.xterm-screen`; `None` sin celda medida.
pub fn report_cell(x: f64, y: f64, cell: (f64, f64), cols: u16, rows: u16) -> Option<(u16, u16)> {
    let (w, h) = cell;
    if !(w > 0.0 && h > 0.0 && x.is_finite() && y.is_finite()) || cols == 0 || rows == 0 {
        return None;
    }
    let col = (x / w).ceil().clamp(1.0, f64::from(cols));
    let row = (y / h).ceil().clamp(1.0, f64::from(rows));
    Some((col as u16 - 1, row as u16 - 1))
}

/// Punto de selección: `getCoords(…, isSelection = true)`, el borde de celda
/// más cercano (`ceil((x + ancho/2) / ancho)` acotado a `[1, cols + 1]`), en
/// base 0: la columna va de 0 a `cols` y la fila es la de la vista.
pub fn selection_point(
    x: f64,
    y: f64,
    cell: (f64, f64),
    cols: u16,
    rows: u16,
) -> Option<(u16, u16)> {
    let (w, h) = cell;
    if !(w > 0.0 && h > 0.0 && x.is_finite() && y.is_finite()) || cols == 0 || rows == 0 {
        return None;
    }
    let col = ((x + w / 2.0) / w).ceil().clamp(1.0, f64::from(cols) + 1.0);
    let row = (y / h).ceil().clamp(1.0, f64::from(rows));
    Some((col as u16 - 1, row as u16 - 1))
}

/// Botón de `mousedown`/`mouseup` (`MouseEvent.button`): xterm.js descarta
/// los que no son 0, 1 o 2.
pub fn pressed_button(button: i16) -> Option<Button> {
    match button {
        0 => Some(Button::Left),
        1 => Some(Button::Middle),
        2 => Some(Button::Right),
        _ => None,
    }
}

/// Botón de un `mousemove` según `MouseEvent.buttons` (izquierdo, central,
/// derecho, en ese orden, como `bindMouse`).
pub fn moving_button(buttons: u16) -> Button {
    if buttons & 1 != 0 {
        Button::Left
    } else if buttons & 4 != 0 {
        Button::Middle
    } else if buttons & 2 != 0 {
        Button::Right
    } else {
        Button::None
    }
}

/// Un informe de ratón antes de codificarlo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub button: Button,
    pub kind: MouseKind,
    pub col: u16,
    pub row: u16,
    /// `(ctrl, alt, shift)`.
    pub mods: (bool, bool, bool),
}

/// Codifica informes y descarta el movimiento repetido, como
/// `CoreMouseService._lastEvent`.
#[derive(Debug, Clone, Default)]
pub struct MouseReporter {
    last: Option<Report>,
}

impl MouseReporter {
    /// Bytes del informe, o `None` si el modo no lo pide o repite el último
    /// movimiento (misma celda, botón y modificadores).
    pub fn report(&mut self, r: Report, m: &Modes) -> Option<Vec<u8>> {
        if r.kind == MouseKind::Move && self.last == Some(r) {
            return None;
        }
        let bytes = encode_mouse(r.button, r.kind, r.col, r.row, r.mods, m)?;
        self.last = Some(r);
        Some(bytes)
    }

    /// Un informe de rueda ya enviado por [`comandos_term::input::wheel`]
    /// también cuenta como el último evento.
    pub fn note(&mut self, r: Report) {
        self.last = Some(r);
    }
}

/// `WheelEvent.DOM_DELTA_PIXEL`, `DOM_DELTA_LINE` y `DOM_DELTA_PAGE`.
pub const DELTA_PIXEL: u32 = 0;
pub const DELTA_LINE: u32 = 1;
pub const DELTA_PAGE: u32 = 2;

/// `fastScrollSensitivity` por omisión: con Alt la rueda va 5 veces más
/// rápido (`fastScrollModifier: 'alt'`).
const FAST_SCROLL: f64 = 5.0;

/// Lo que importa de un `WheelEvent`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelInput {
    pub delta_y: f64,
    pub delta_mode: u32,
    pub shift: bool,
    pub alt: bool,
}

impl WheelInput {
    /// `_applyScrollModifier`: Alt multiplica por 5; `None` con Shift o
    /// sin desplazamiento vertical (xterm.js devuelve 0).
    fn scaled(&self) -> Option<f64> {
        if self.delta_y == 0.0 || self.shift || !self.delta_y.is_finite() {
            return None;
        }
        Some(if self.alt {
            self.delta_y * FAST_SCROLL
        } else {
            self.delta_y
        })
    }
}

/// Estado de la rueda y del desplazamiento local.
#[derive(Debug, Clone, Default)]
pub struct Wheel {
    /// `_wheelPartialScroll`: fracción de línea pendiente.
    partial: f64,
    /// Posición en píxeles del área de desplazamiento (`scrollTop`) y la
    /// fila superior (`ydisp`) a la que corresponde.
    pos: Option<(usize, f64)>,
}

impl Wheel {
    /// `Viewport.getLinesScrolled`: líneas para el ratón de la aplicación o
    /// para las flechas de la pantalla alternativa. En píxeles acumula la
    /// fracción entre eventos.
    pub fn lines(&mut self, e: &WheelInput, row_h: f64, rows: u16) -> i32 {
        let Some(mut t) = e.scaled() else {
            return 0;
        };
        match e.delta_mode {
            DELTA_PIXEL => {
                if row_h.is_nan() || row_h <= 0.0 {
                    return 0;
                }
                self.partial += t / row_h;
                let sign = if self.partial > 0.0 { 1.0 } else { -1.0 };
                t = self.partial.abs().floor() * sign;
                // `%` de JavaScript conserva el signo, como `%` de Rust.
                self.partial %= 1.0;
            }
            DELTA_PAGE => t *= f64::from(rows),
            _ => {}
        }
        // Las flechas se repiten `Math.abs(t)` veces (hacia arriba si hay
        // fracción): se redondea alejándose de cero.
        let n = t.abs().ceil().min(f64::from(i32::MAX));
        if t < 0.0 { -(n as i32) } else { n as i32 }
    }

    /// `Viewport._getPixelsScrolled`: píxeles para el desplazamiento local.
    pub fn pixels(e: &WheelInput, row_h: f64, rows: u16) -> f64 {
        let Some(t) = e.scaled() else {
            return 0.0;
        };
        match e.delta_mode {
            DELTA_LINE => t * row_h,
            DELTA_PAGE => t * row_h * f64::from(rows),
            _ => t,
        }
    }

    /// Desplazamiento local por píxeles (rueda o dedo): como
    /// `scrollTop += px` seguido de `_handleScroll` (fila =
    /// `round(scrollTop / alto)`). Si la fila cambia, la posición se ajusta a
    /// ella, como `_innerRefresh`; si no, la fracción se conserva para el
    /// próximo evento. Devuelve la nueva fila superior (`ydisp`, 0 = lo más
    /// antiguo de la historia, `max_ydisp` = abajo del todo).
    pub fn scroll_px(&mut self, px: f64, ydisp: usize, max_ydisp: usize, row_h: f64) -> usize {
        if row_h.is_nan() || row_h <= 0.0 || !px.is_finite() {
            return ydisp;
        }
        let snapped = ydisp as f64 * row_h;
        let pos = match self.pos {
            Some((y, p)) if y == ydisp => p,
            _ => snapped,
        };
        let pos = (pos + px).clamp(0.0, max_ydisp as f64 * row_h);
        let next = (js_round(pos / row_h).max(0.0) as usize).min(max_ydisp);
        let pos = if next == ydisp {
            pos
        } else {
            next as f64 * row_h
        };
        self.pos = Some((next, pos));
        next
    }
}

/// Fila superior pedida por la barra de desplazamiento: `_handleScroll`
/// (`round(scrollTop / alto)`), acotada a la historia.
pub fn scrollbar_ydisp(scroll_top: f64, row_h: f64, max_ydisp: usize) -> Option<usize> {
    if row_h.is_nan() || row_h <= 0.0 || !scroll_top.is_finite() {
        return None;
    }
    Some((js_round(scroll_top / row_h).max(0.0) as usize).min(max_ydisp))
}

/// `_getMouseEventScrollAmount`: líneas por paso (cada 50 ms) al arrastrar
/// la selección por encima o por debajo de la terminal; `y` es la posición
/// relativa a `.xterm-screen` y `height` su alto CSS.
pub fn drag_scroll_amount(y: f64, height: f64) -> i32 {
    if !y.is_finite() || (y >= 0.0 && y <= height) {
        return 0;
    }
    let mut t = y;
    if t > height {
        t -= height;
    }
    let t = t.clamp(-50.0, 50.0) / 50.0;
    let sign = if t < 0.0 { -1.0 } else { 1.0 };
    (sign + js_round(14.0 * t)) as i32
}

/// Selección hecha con el ratón (el modelo de `SelectionService`).
///
/// En modo [`SelectMode::Simple`] los puntos son **bordes** de celda
/// (columna de 0 a `cols`, el final excluido, como xterm.js); en
/// [`SelectMode::Word`] y [`SelectMode::Line`] son celdas y la expansión la
/// hace [`comandos_term::select::selection_bounds`]. Las filas son absolutas
/// (historia negativa).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SelectModel {
    anchor: Option<Point>,
    head: Option<Point>,
    mode: Option<SelectMode>,
}

impl SelectModel {
    /// Empieza una selección (clic, doble o triple clic). Con un clic simple
    /// aún no hay nada seleccionado.
    pub fn start(&mut self, mode: SelectMode, p: Point) {
        self.mode = Some(mode);
        self.anchor = Some(p);
        self.head = match mode {
            SelectMode::Simple => None,
            SelectMode::Word | SelectMode::Line => Some(p),
        };
    }

    /// Mueve el extremo (arrastre o Shift+clic).
    pub fn extend(&mut self, p: Point) {
        if self.anchor.is_some() {
            self.head = Some(p);
        }
    }

    /// Hay un ancla (un arrastre puede extenderla).
    pub fn is_started(&self) -> bool {
        self.anchor.is_some()
    }

    pub fn mode(&self) -> Option<SelectMode> {
        self.mode
    }

    pub fn clear(&mut self) {
        *self = SelectModel::default();
    }

    /// El texto subió `lines` filas absolutas (salida nueva que empuja la
    /// historia): la selección lo sigue, como las coordenadas de búfer de
    /// xterm.js. Se borra si sale por arriba de `first_row`.
    pub fn shift_up(&mut self, lines: u64, first_row: i32) {
        if lines == 0 || self.anchor.is_none() {
            return;
        }
        let lines = i32::try_from(lines).unwrap_or(i32::MAX);
        let up = |p: Point| (p.0.saturating_sub(lines), p.1);
        self.anchor = self.anchor.map(up);
        self.head = self.head.map(up);
        let gone = |p: Option<Point>| p.is_some_and(|(row, _)| row < first_row);
        if gone(self.anchor) || gone(self.head) {
            self.clear();
        }
    }

    /// La selección para `comandos_term::select`, con la cabeza inclusiva;
    /// `None` si no hay nada seleccionado (un clic sin arrastre).
    pub fn selection(&self, cols: u16) -> Option<Selection> {
        let (anchor, head, mode) = (self.anchor?, self.head?, self.mode?);
        if mode != SelectMode::Simple {
            return Some(Selection { anchor, head, mode });
        }
        let (lo, hi) = if anchor <= head {
            (anchor, head)
        } else {
            (head, anchor)
        };
        if lo == hi || cols == 0 {
            return None;
        }
        // El borde `cols` de una fila es el principio de la siguiente.
        let lo = if lo.1 >= cols {
            (lo.0.saturating_add(1), 0)
        } else {
            lo
        };
        // Final excluido → última celda incluida.
        let hi = if hi.1 == 0 {
            (hi.0.saturating_sub(1), cols - 1)
        } else {
            (hi.0, (hi.1 - 1).min(cols - 1))
        };
        (lo <= hi).then_some(Selection {
            anchor: lo,
            head: hi,
            mode,
        })
    }
}

/// Lo que hace un `mousedown` del botón izquierdo según los modos y Shift
/// (`bindMouse` + `SelectionService.handleMouseDown`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// Informe para la aplicación (ratón de tmux activo, sin Shift).
    Report,
    /// Selección nueva según el número de clics.
    Select(SelectMode),
    /// Shift+clic sin ratón de la aplicación: extiende la selección.
    Extend,
}

/// Decide qué hace una pulsación. `button` es `MouseEvent.button` y
/// `detail` el número de clics.
pub fn classify_press(button: i16, detail: i32, shift: bool, m: &Modes) -> Option<Press> {
    let mouse_on = m.mouse != MouseMode::Off;
    if mouse_on && !shift {
        return pressed_button(button).map(|_| Press::Report);
    }
    // La selección solo empieza con el botón izquierdo.
    if button != 0 {
        return None;
    }
    if !mouse_on && shift {
        return Some(Press::Extend);
    }
    match detail {
        2 => Some(Press::Select(SelectMode::Word)),
        3 => Some(Press::Select(SelectMode::Line)),
        // Más de tres clics no hacen nada en xterm.js.
        d if d > 3 => None,
        _ => Some(Press::Select(SelectMode::Simple)),
    }
}

// --- cableado con el DOM -----------------------------------------------------

mod web {
    use super::{
        Press, Report, Wheel, WheelInput, classify_press, drag_scroll_amount, moving_button,
        pressed_button, report_cell, scrollbar_ydisp, selection_point,
    };
    use crate::{Drag, DragKind, Focus, Inner, links, listen, set_styles};
    use comandos_term::{
        engine::MouseMode,
        input::{Button, MouseKind, WheelAction, wheel},
        select::{Point, SelectMode},
    };
    use wasm_bindgen::{JsCast, closure::Closure};
    use web_sys::{Event, EventTarget, MouseEvent, TouchEvent, WheelEvent};

    /// `cancel(e, true)` de xterm.js.
    fn cancel(e: &Event) {
        e.prevent_default();
        e.stop_propagation();
    }

    fn mods(me: &MouseEvent) -> (bool, bool, bool) {
        (me.ctrl_key(), me.alt_key(), me.shift_key())
    }

    impl Inner {
        /// Posición del ratón en píxeles CSS relativa a `.xterm-screen`.
        fn screen_xy(&self, me: &MouseEvent) -> (f64, f64) {
            let rect = self.dom.screen.get_bounding_client_rect();
            (
                f64::from(me.client_x()) - rect.left(),
                f64::from(me.client_y()) - rect.top(),
            )
        }

        /// Celda CSS de addon-canvas (la de `MouseService`).
        fn css_cell_size(&self) -> (f64, f64) {
            self.metrics
                .css_cell(self.size.cols, self.size.rows)
                .unwrap_or((0.0, 0.0))
        }

        /// Fila de la vista → fila absoluta.
        fn absolute(&self, (col, row): (u16, u16)) -> Point {
            let offset = i32::try_from(self.engine.display_offset()).unwrap_or(i32::MAX);
            (i32::from(row) - offset, col)
        }

        fn cell_at(&self, x: f64, y: f64) -> Option<(u16, u16)> {
            report_cell(x, y, self.css_cell_size(), self.size.cols, self.size.rows)
        }

        /// Punto de selección según el modo: borde de celda en `Simple`,
        /// celda en `Word` y `Line`.
        fn select_point(&self, mode: SelectMode, x: f64, y: f64) -> Option<Point> {
            let cell = self.css_cell_size();
            let p = match mode {
                SelectMode::Simple => selection_point(x, y, cell, self.size.cols, self.size.rows),
                SelectMode::Word | SelectMode::Line => self.cell_at(x, y),
            }?;
            Some(self.absolute(p))
        }

        fn report(&mut self, r: Report) {
            let m = self.engine.modes();
            if let Some(bytes) = self.reporter.report(r, &m) {
                // SGR va como datos del usuario; X10 como binario (no).
                self.send(bytes, m.sgr_mouse);
            }
        }

        /// Enlace bajo `(x, y)`, calculado una vez por fila y generación.
        fn link_under(&mut self, x: f64, y: f64) -> Option<comandos_term::select::UrlSpan> {
            let p = self.absolute(self.cell_at(x, y)?);
            let engine = &self.engine;
            self.links
                .link_at(self.link_gen, p, |row| {
                    comandos_term::select::find_urls(engine, row)
                })
                .cloned()
        }

        fn update_hover(&mut self, x: f64, y: f64) {
            let found = self.link_under(x, y);
            if found != self.hover {
                self.set_pointer(found.is_some());
                self.hover = found;
                self.overlay.dirty = true;
                self.request_frame();
            }
        }

        /// Escuchas de `document` mientras dura el arrastre (y el paso de
        /// 50 ms del desplazamiento al seleccionar, como xterm.js).
        fn begin_drag(&mut self, kind: DragKind) {
            self.drag = None;
            let document: EventTarget = self.document.clone().into();
            let mut listeners = Vec::new();
            for (event, handler) in [
                ("mousemove", Inner::on_drag_move as fn(&mut Inner, &Event)),
                ("mouseup", Inner::on_drag_up),
            ] {
                if let Some(l) = listen(&self.weak, &document, event, None, handler) {
                    listeners.push(l);
                }
            }
            let (mut interval, mut interval_cb) = (None, None);
            if kind == DragKind::Select {
                let weak = self.weak.clone();
                let cb = Closure::<dyn FnMut()>::new(move || {
                    crate::dispatch(&weak, Inner::on_drag_tick);
                });
                interval = self
                    .window
                    .set_interval_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(),
                        50,
                    )
                    .ok();
                interval_cb = Some(cb);
            }
            self.drag = Some(Drag {
                kind,
                scroll: 0,
                window: self.window.clone(),
                interval,
                _interval_cb: interval_cb,
                _listeners: listeners,
            });
        }

        pub(crate) fn on_mouse_down(&mut self, e: &Event) {
            let Some(me) = e.dyn_ref::<MouseEvent>() else {
                return;
            };
            // `bindMouse`: sin selección nativa y con el foco en la terminal.
            e.prevent_default();
            self.outbox.focus = Some(Focus::Plain);
            let (x, y) = self.screen_xy(me);
            let button = me.button();
            if button == 0 && links::activates(me.ctrl_key(), me.meta_key()) {
                self.update_hover(x, y);
                if let Some(span) = self.hover.clone() {
                    self.link_down = Some(span);
                    return;
                }
            }
            let m = self.engine.modes();
            match classify_press(button, me.detail(), me.shift_key(), &m) {
                None => {}
                Some(Press::Report) => {
                    if let (Some(b), Some((col, row))) =
                        (pressed_button(button), self.cell_at(x, y))
                    {
                        self.report(Report {
                            button: b,
                            kind: MouseKind::Press,
                            col,
                            row,
                            mods: mods(me),
                        });
                    }
                    self.begin_drag(DragKind::Report);
                }
                Some(Press::Select(mode)) => {
                    if m.mouse != MouseMode::Off {
                        // Shift fuerza la selección: tmux no lo ve.
                        e.stop_propagation();
                    }
                    if let Some(p) = self.select_point(mode, x, y) {
                        self.select.start(mode, p);
                        self.overlay.dirty = true;
                    }
                    self.begin_drag(DragKind::Select);
                }
                Some(Press::Extend) => {
                    if let Some(mode) = self.select.mode()
                        && let Some(p) = self.select_point(mode, x, y)
                    {
                        self.select.extend(p);
                        self.overlay.dirty = true;
                    }
                    self.begin_drag(DragKind::Select);
                }
            }
            self.request_frame();
        }

        pub(crate) fn on_mouse_move(&mut self, e: &Event) {
            let Some(me) = e.dyn_ref::<MouseEvent>() else {
                return;
            };
            if self.drag.is_some() {
                return;
            }
            let (x, y) = self.screen_xy(me);
            self.update_hover(x, y);
            // Movimiento sin botón: solo con `MouseMode::Motion` (1003).
            if me.buttons() == 0
                && self.engine.modes().mouse == MouseMode::Motion
                && let Some((col, row)) = self.cell_at(x, y)
            {
                self.report(Report {
                    button: Button::None,
                    kind: MouseKind::Move,
                    col,
                    row,
                    mods: mods(me),
                });
            }
        }

        /// Ctrl+clic: el enlace se abre al soltar sobre el mismo enlace.
        pub(crate) fn on_mouse_up(&mut self, e: &Event) {
            let Some(me) = e.dyn_ref::<MouseEvent>() else {
                return;
            };
            let Some(down) = self.link_down.take() else {
                return;
            };
            let (x, y) = self.screen_xy(me);
            if me.button() == 0
                && self.link_under(x, y).as_ref() == Some(&down)
                && links::is_web_url(&down.url)
            {
                self.outbox.open = Some(down.url);
            }
        }

        pub(crate) fn on_mouse_leave(&mut self, _e: &Event) {
            if self.drag.is_none() {
                self.drop_hover();
                self.request_frame();
            }
        }

        pub(crate) fn on_drag_move(&mut self, e: &Event) {
            let Some(me) = e.dyn_ref::<MouseEvent>() else {
                return;
            };
            let Some(kind) = self.drag.as_ref().map(|d| d.kind) else {
                return;
            };
            let (x, y) = self.screen_xy(me);
            match kind {
                DragKind::Report => {
                    if me.buttons() != 0
                        && let Some((col, row)) = self.cell_at(x, y)
                    {
                        self.report(Report {
                            button: moving_button(me.buttons()),
                            kind: MouseKind::Move,
                            col,
                            row,
                            mods: mods(me),
                        });
                    }
                }
                DragKind::Select => {
                    e.stop_immediate_propagation();
                    let (_, css_h) = self.metrics.css_canvas(self.size.cols, self.size.rows);
                    let amount = drag_scroll_amount(y, css_h);
                    if let Some(d) = self.drag.as_mut() {
                        d.scroll = amount;
                    }
                    let Some(mode) = self.select.mode() else {
                        return;
                    };
                    if let Some(mut p) = self.select_point(mode, x, y) {
                        // Fuera de la terminal la selección llega al borde.
                        if amount > 0 {
                            p.1 = self.end_col(mode);
                        } else if amount < 0 {
                            p.1 = 0;
                        }
                        self.select.extend(p);
                        self.overlay.dirty = true;
                        self.request_frame();
                    }
                }
            }
        }

        /// Última columna según el modo (borde en `Simple`, celda si no).
        fn end_col(&self, mode: SelectMode) -> u16 {
            match mode {
                SelectMode::Simple => self.size.cols,
                SelectMode::Word | SelectMode::Line => self.size.cols.saturating_sub(1),
            }
        }

        pub(crate) fn on_drag_up(&mut self, e: &Event) {
            let Some(me) = e.dyn_ref::<MouseEvent>() else {
                return;
            };
            let Some(kind) = self.drag.as_ref().map(|d| d.kind) else {
                return;
            };
            match kind {
                DragKind::Report => {
                    let (x, y) = self.screen_xy(me);
                    if let (Some(b), Some((col, row))) =
                        (pressed_button(me.button()), self.cell_at(x, y))
                    {
                        self.report(Report {
                            button: b,
                            kind: MouseKind::Release,
                            col,
                            row,
                            mods: mods(me),
                        });
                    }
                    if me.buttons() == 0 {
                        self.drag = None;
                    }
                }
                DragKind::Select => {
                    self.drag = None;
                    // Selección primaria de Linux: xterm.js deja el texto
                    // seleccionado en el `<textarea>`.
                    if self.is_linux && self.has_selection() {
                        self.outbox.focus = Some(Focus::Select(self.selection_text()));
                        self.textarea_at = None;
                    }
                }
            }
        }

        /// `_dragScroll`: cada 50 ms con el ratón fuera de la terminal.
        pub(crate) fn on_drag_tick(&mut self) {
            let Some(amount) = self.drag.as_ref().map(|d| d.scroll) else {
                return;
            };
            let Some(mode) = self.select.mode() else {
                return;
            };
            if amount == 0 {
                return;
            }
            self.scroll_lines(amount.saturating_neg());
            let offset = i32::try_from(self.engine.display_offset()).unwrap_or(i32::MAX);
            let rows = i32::from(self.size.rows);
            let p = if amount > 0 {
                ((rows - offset).min(rows - 1), self.end_col(mode))
            } else {
                (-offset, 0)
            };
            self.select.extend(p);
            self.overlay.dirty = true;
            self.request_frame();
        }

        /// `moveTextAreaUnderMouseCursor`: el `<textarea>` bajo el ratón,
        /// para que el menú del navegador (copiar/pegar) y el pegado con el
        /// botón central caigan en él.
        fn textarea_under(&mut self, me: &MouseEvent) {
            let (x, y) = self.screen_xy(me);
            let px = |v: f64| format!("{v}px");
            set_styles(
                &self.dom.textarea,
                &[
                    ("width", "20px"),
                    ("height", "20px"),
                    ("left", &px(x - 10.0)),
                    ("top", &px(y - 10.0)),
                    ("z-index", "1000"),
                ],
            );
            self.textarea_at = None;
        }

        /// `rightClickHandler`: el menú contextual ofrece «Copiar».
        pub(crate) fn on_context_menu(&mut self, e: &Event) {
            let Some(me) = e.dyn_ref::<MouseEvent>() else {
                return;
            };
            self.textarea_under(me);
            self.outbox.focus = Some(Focus::Select(self.selection_text()));
        }

        /// Linux: el botón central pega la selección primaria en el
        /// `<textarea>`.
        pub(crate) fn on_aux_click(&mut self, e: &Event) {
            let Some(me) = e.dyn_ref::<MouseEvent>() else {
                return;
            };
            if self.is_linux && me.button() == 1 {
                self.textarea_under(me);
                self.outbox.focus = Some(Focus::Plain);
            }
        }

        pub(crate) fn on_wheel(&mut self, e: &Event) {
            let Some(we) = e.dyn_ref::<WheelEvent>() else {
                return;
            };
            let input = WheelInput {
                delta_y: we.delta_y(),
                delta_mode: we.delta_mode(),
                shift: we.shift_key(),
                alt: we.alt_key(),
            };
            let m = self.engine.modes();
            let (rows, row_h) = (self.size.rows, self.row_h());
            let mouse_on = m.mouse != MouseMode::Off;
            if mouse_on || m.alt_screen {
                let lines = self.wheel.lines(&input, row_h, rows);
                if mouse_on {
                    // xterm.js cancela siempre la rueda con ratón activo.
                    cancel(e);
                }
                if lines == 0 {
                    return;
                }
                let (x, y) = self.screen_xy(we);
                let Some((col, row)) = self.cell_at(x, y) else {
                    return;
                };
                let mods = mods(we);
                match wheel(lines, col, row, mods, &m) {
                    WheelAction::Mouse(bytes) => {
                        self.reporter.note(Report {
                            button: if lines < 0 {
                                Button::WheelUp
                            } else {
                                Button::WheelDown
                            },
                            kind: MouseKind::Press,
                            col,
                            row,
                            mods,
                        });
                        self.send(bytes, m.sgr_mouse);
                    }
                    WheelAction::Arrows(bytes) => {
                        cancel(e);
                        self.send(bytes, true);
                    }
                    WheelAction::Scroll(_) => {}
                }
                return;
            }
            let px = Wheel::pixels(&input, row_h, rows);
            self.local_scroll(e, px);
        }

        /// `handleWheel`/`handleTouchMove` + `_bubbleScroll`: desplaza la
        /// vista y cancela el evento salvo en el tope (así la página
        /// desplaza).
        fn local_scroll(&mut self, e: &Event, px: f64) {
            if px == 0.0 || !px.is_finite() {
                return;
            }
            let (ydisp, max) = self.ydisp();
            let can = (px < 0.0 && ydisp > 0) || (px > 0.0 && ydisp < max);
            if can && e.cancelable() {
                e.prevent_default();
            }
            let row_h = self.row_h();
            let next = self.wheel.scroll_px(px, ydisp, max, row_h);
            if next != ydisp {
                let delta = i64::try_from(ydisp).unwrap_or(i64::MAX)
                    - i64::try_from(next).unwrap_or(i64::MAX);
                self.scroll_lines(i32::try_from(delta).unwrap_or(0));
            }
        }

        pub(crate) fn on_touch_start(&mut self, e: &Event) {
            let Some(te) = e.dyn_ref::<TouchEvent>() else {
                return;
            };
            if self.engine.modes().mouse != MouseMode::Off {
                return;
            }
            self.touch_y = te.touches().get(0).map(|t| f64::from(t.page_y()));
        }

        pub(crate) fn on_touch_move(&mut self, e: &Event) {
            let Some(te) = e.dyn_ref::<TouchEvent>() else {
                return;
            };
            if self.engine.modes().mouse != MouseMode::Off {
                return;
            }
            let Some(y) = te.touches().get(0).map(|t| f64::from(t.page_y())) else {
                return;
            };
            let Some(last) = self.touch_y.replace(y) else {
                return;
            };
            self.local_scroll(e, last - y);
        }

        /// La barra de desplazamiento: `_handleScroll`.
        pub(crate) fn on_viewport_scroll(&mut self, _e: &Event) {
            if std::mem::take(&mut self.ignore_scroll) {
                return;
            }
            if self.dom.viewport.offset_parent().is_none() {
                return;
            }
            let (ydisp, max) = self.ydisp();
            let top = f64::from(self.dom.viewport.scroll_top());
            let Some(next) = scrollbar_ydisp(top, self.row_h(), max) else {
                return;
            };
            if next != ydisp {
                let delta = i64::try_from(ydisp).unwrap_or(i64::MAX)
                    - i64::try_from(next).unwrap_or(i64::MAX);
                self.scroll_lines(i32::try_from(delta).unwrap_or(0));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: (f64, f64) = (8.4, 16.8);

    #[test]
    fn report_cell_uses_ceil_like_get_coords() {
        assert_eq!(report_cell(0.0, 0.0, CELL, 80, 24), Some((0, 0)));
        assert_eq!(
            report_cell(8.4, 16.8, CELL, 80, 24),
            Some((0, 0)),
            "borde exacto = celda anterior"
        );
        assert_eq!(report_cell(8.5, 17.0, CELL, 80, 24), Some((1, 1)));
        assert_eq!(
            report_cell(-5.0, 9999.0, CELL, 80, 24),
            Some((0, 23)),
            "acotado"
        );
        assert_eq!(report_cell(9999.0, 1.0, CELL, 80, 24), Some((79, 0)));
        assert_eq!(report_cell(1.0, 1.0, (0.0, 16.8), 80, 24), None);
        assert_eq!(report_cell(f64::NAN, 1.0, CELL, 80, 24), None);
    }

    #[test]
    fn selection_point_snaps_to_the_nearest_border() {
        assert_eq!(selection_point(0.0, 1.0, CELL, 80, 24), Some((0, 0)));
        assert_eq!(selection_point(4.1, 1.0, CELL, 80, 24), Some((0, 0)));
        assert_eq!(selection_point(4.3, 1.0, CELL, 80, 24), Some((1, 0)));
        assert_eq!(
            selection_point(9999.0, 1.0, CELL, 80, 24),
            Some((80, 0)),
            "borde final"
        );
    }

    #[test]
    fn buttons_follow_bind_mouse() {
        assert_eq!(pressed_button(0), Some(Button::Left));
        assert_eq!(pressed_button(2), Some(Button::Right));
        assert_eq!(pressed_button(3), None);
        assert_eq!(moving_button(1 | 2), Button::Left);
        assert_eq!(moving_button(4 | 2), Button::Middle);
        assert_eq!(moving_button(2), Button::Right);
        assert_eq!(moving_button(0), Button::None);
    }

    #[test]
    fn repeated_moves_are_dropped() {
        let m = Modes {
            mouse: MouseMode::Motion,
            sgr_mouse: true,
            ..Modes::default()
        };
        let mut r = MouseReporter::default();
        let mv = |col| Report {
            button: Button::None,
            kind: MouseKind::Move,
            col,
            row: 0,
            mods: (false, false, false),
        };
        assert!(r.report(mv(1), &m).is_some());
        assert!(r.report(mv(1), &m).is_none(), "misma celda");
        assert!(r.report(mv(2), &m).is_some());
        let press = Report {
            button: Button::Left,
            kind: MouseKind::Press,
            ..mv(2)
        };
        assert!(r.report(press, &m).is_some());
        assert!(
            r.report(press, &m).is_some(),
            "solo se descarta el movimiento"
        );
        r.note(mv(2));
        assert!(
            r.report(mv(2), &m).is_none(),
            "la rueda también es el último evento"
        );
        let click = Modes {
            mouse: MouseMode::Click,
            sgr_mouse: true,
            ..Modes::default()
        };
        let mut r = MouseReporter::default();
        assert!(
            r.report(mv(5), &click).is_none(),
            "el modo no informa movimiento"
        );
        assert!(r.report(mv(5), &m).is_some(), "y no cuenta como enviado");
    }

    fn wheel(delta_y: f64, delta_mode: u32) -> WheelInput {
        WheelInput {
            delta_y,
            delta_mode,
            shift: false,
            alt: false,
        }
    }

    #[test]
    fn wheel_lines_accumulate_pixel_fractions() {
        let mut w = Wheel::default();
        assert_eq!(w.lines(&wheel(10.0, DELTA_PIXEL), 20.0, 24), 0);
        assert_eq!(w.lines(&wheel(10.0, DELTA_PIXEL), 20.0, 24), 1);
        assert_eq!(w.lines(&wheel(-50.0, DELTA_PIXEL), 20.0, 24), -2);
        assert_eq!(w.lines(&wheel(3.0, DELTA_LINE), 20.0, 24), 3);
        assert_eq!(w.lines(&wheel(1.0, DELTA_PAGE), 20.0, 24), 24);
        assert_eq!(
            w.lines(
                &WheelInput {
                    alt: true,
                    ..wheel(1.0, DELTA_LINE)
                },
                20.0,
                24
            ),
            5,
            "Alt acelera"
        );
        assert_eq!(
            w.lines(
                &WheelInput {
                    shift: true,
                    ..wheel(100.0, DELTA_PIXEL)
                },
                20.0,
                24
            ),
            0,
            "Shift no desplaza"
        );
    }

    #[test]
    fn wheel_pixels_follow_get_pixels_scrolled() {
        assert_eq!(Wheel::pixels(&wheel(53.0, DELTA_PIXEL), 17.0, 24), 53.0);
        assert_eq!(Wheel::pixels(&wheel(3.0, DELTA_LINE), 17.0, 24), 51.0);
        assert_eq!(Wheel::pixels(&wheel(-1.0, DELTA_PAGE), 17.0, 24), -408.0);
    }

    #[test]
    fn local_scroll_keeps_fractions_and_snaps_on_change() {
        let mut w = Wheel::default();
        // 100 filas de historia, abajo del todo.
        assert_eq!(w.scroll_px(-4.0, 100, 100, 17.0), 100);
        assert_eq!(w.scroll_px(-4.0, 100, 100, 17.0), 100);
        // 12 px acumulados: round(1688/17) = 99.
        assert_eq!(w.scroll_px(-4.0, 100, 100, 17.0), 99);
        // Tras cambiar de fila se parte de 99·17 exacto.
        assert_eq!(w.scroll_px(-8.0, 99, 100, 17.0), 99);
        assert_eq!(w.scroll_px(10_000.0, 99, 100, 17.0), 100, "acotado abajo");
        assert_eq!(w.scroll_px(-1e9, 100, 100, 17.0), 0, "acotado arriba");
        // Si la fila cambió por otra vía (salida nueva), se vuelve a partir de ella.
        assert_eq!(w.scroll_px(-9.0, 50, 100, 17.0), 49);
        assert_eq!(w.scroll_px(f64::NAN, 49, 100, 17.0), 49);
    }

    #[test]
    fn scrollbar_maps_scroll_top_to_rows() {
        assert_eq!(scrollbar_ydisp(0.0, 17.0, 100), Some(0));
        assert_eq!(scrollbar_ydisp(17.0 * 40.4, 17.0, 100), Some(40));
        assert_eq!(scrollbar_ydisp(17.0 * 40.5, 17.0, 100), Some(41));
        assert_eq!(scrollbar_ydisp(1e9, 17.0, 100), Some(100));
        assert_eq!(scrollbar_ydisp(5.0, 0.0, 100), None);
    }

    #[test]
    fn drag_scroll_speed_follows_xterm() {
        assert_eq!(drag_scroll_amount(10.0, 400.0), 0);
        assert_eq!(drag_scroll_amount(-1.0, 400.0), -1);
        assert_eq!(drag_scroll_amount(-25.0, 400.0), -1 - 7);
        assert_eq!(drag_scroll_amount(-500.0, 400.0), -15);
        assert_eq!(drag_scroll_amount(401.0, 400.0), 1);
        assert_eq!(drag_scroll_amount(475.0, 400.0), 15);
    }

    #[test]
    fn simple_selection_turns_borders_into_inclusive_cells() {
        let mut s = SelectModel::default();
        s.start(SelectMode::Simple, (2, 3));
        assert_eq!(s.selection(10), None, "clic sin arrastre");
        s.extend((2, 3));
        assert_eq!(s.selection(10), None, "sin ancho");
        s.extend((2, 7));
        assert_eq!(
            s.selection(10),
            Some(Selection {
                anchor: (2, 3),
                head: (2, 6),
                mode: SelectMode::Simple
            })
        );
        // Hacia atrás y hasta el principio de una fila.
        s.extend((0, 0));
        assert_eq!(
            s.selection(10),
            Some(Selection {
                anchor: (0, 0),
                head: (2, 2),
                mode: SelectMode::Simple
            })
        );
        // Desde el borde final de una fila hasta el principio de la siguiente.
        s.start(SelectMode::Simple, (4, 10));
        s.extend((5, 0));
        assert_eq!(s.selection(10), None);
        s.extend((5, 2));
        assert_eq!(
            s.selection(10),
            Some(Selection {
                anchor: (5, 0),
                head: (5, 1),
                mode: SelectMode::Simple
            })
        );
    }

    #[test]
    fn word_and_line_selections_exist_from_the_click() {
        let mut s = SelectModel::default();
        s.start(SelectMode::Word, (-3, 4));
        assert_eq!(
            s.selection(10),
            Some(Selection {
                anchor: (-3, 4),
                head: (-3, 4),
                mode: SelectMode::Word
            })
        );
        s.extend((1, 0));
        assert_eq!(s.selection(10).map(|x| x.head), Some((1, 0)));
    }

    #[test]
    fn selection_follows_text_pushed_into_history() {
        let mut s = SelectModel::default();
        s.start(SelectMode::Simple, (2, 0));
        s.extend((3, 4));
        s.shift_up(5, -100);
        assert_eq!(
            s.selection(10).map(|x| (x.anchor, x.head)),
            Some(((-3, 0), (-2, 3)))
        );
        s.shift_up(98, -100);
        assert!(!s.is_started(), "salió de la historia");
    }

    #[test]
    fn presses_follow_bind_mouse_and_shift_forces_selection() {
        let off = Modes::default();
        let tmux = Modes {
            mouse: MouseMode::Drag,
            sgr_mouse: true,
            ..Modes::default()
        };
        assert_eq!(
            classify_press(0, 1, false, &off),
            Some(Press::Select(SelectMode::Simple))
        );
        assert_eq!(
            classify_press(0, 2, false, &off),
            Some(Press::Select(SelectMode::Word))
        );
        assert_eq!(
            classify_press(0, 3, false, &off),
            Some(Press::Select(SelectMode::Line))
        );
        assert_eq!(classify_press(0, 4, false, &off), None);
        assert_eq!(classify_press(0, 1, true, &off), Some(Press::Extend));
        assert_eq!(classify_press(2, 1, false, &off), None);
        assert_eq!(classify_press(0, 1, false, &tmux), Some(Press::Report));
        assert_eq!(classify_press(2, 1, false, &tmux), Some(Press::Report));
        assert_eq!(classify_press(5, 1, false, &tmux), None);
        assert_eq!(
            classify_press(0, 2, true, &tmux),
            Some(Press::Select(SelectMode::Word)),
            "Shift fuerza la selección con el ratón de tmux"
        );
    }
}
