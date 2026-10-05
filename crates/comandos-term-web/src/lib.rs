//! `comandos-term-web`: la terminal de ComandOS en el navegador. Sustituye a
//! xterm.js 5.5.0 + `@xterm/addon-canvas` 0.7.0 + `@xterm/addon-fit` 0.10 en
//! `dash/term.html` (la página la monta A9).
//!
//! - [`metrics`]: aritmética de celdas y de ajuste (pura) y medición.
//! - [`paint`]: qué filas repintar y cuándo (puro, con [`paint::Painter`]).
//! - [`canvas`]: el `<canvas>` que pinta como addon-canvas.
//! - [`theme`]: el tema con los nombres de xterm.js.
//! - [`WebTerm`]: une motor, planificador y canvas para JavaScript.
//!
//! ## Ligero
//!
//! Sin daños no se pide ningún cuadro; como mucho hay uno en vuelo. Un
//! temporizador único despierta para el parpadeo del cursor (solo con foco,
//! como xterm.js) o para vencer una actualización sincronizada. Los
//! búferes, cadenas de color y glifos se reutilizan entre cuadros.
//!
//! ## Puntos de enganche para A7/A8
//!
//! El teclado y el ratón (A7) llaman a [`WebTerm::set_focus`],
//! [`WebTerm::scroll_lines`] y leen [`WebTerm::dimensions`]; los enlaces
//! (A8) usarán el elemento [`WebTerm::screen`].
pub mod canvas;
pub mod metrics;
pub mod paint;
pub mod theme;

use canvas::{Canvas2d, CanvasTheme};
use comandos_term::{
    engine::{ClipboardTarget, Engine, GridSize, Palette},
    render::RenderOpts,
};
use metrics::CellMetrics;
use paint::{Blink, CursorInput, Painter, Scheduler};
use std::{cell::RefCell, fmt::Write as _, rc::Rc};
use theme::{ANSI_NAMES, Theme};
use wasm_bindgen::{JsCast, JsValue, closure::Closure, prelude::wasm_bindgen};
use web_sys::{
    Document, HtmlElement, Performance, ResizeObserver, ResizeObserverBoxOptions,
    ResizeObserverOptions, Window,
};

#[wasm_bindgen]
extern "C" {
    /// `promise.then(ok, err)` con funciones ya creadas.
    type PromiseThen;
    #[wasm_bindgen(method, js_name = then)]
    fn then2(this: &PromiseThen, ok: &JsValue, err: &JsValue);

    /// `observer.observe(target, options)` capturando la excepción de los
    /// navegadores sin `device-pixel-content-box`.
    type ObserveCatch;
    #[wasm_bindgen(method, catch, js_name = observe)]
    fn observe_catch(this: &ObserveCatch, target: &JsValue, opts: &JsValue) -> Result<(), JsValue>;
}

/// Opciones de `new Terminal({...})` que usa la terminal (las de xterm.js
/// 5.5 por omisión).
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub font_family: String,
    pub font_size: f64,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub cursor_blink: bool,
    pub scrollback: usize,
    pub draw_bold_text_in_bright_colors: bool,
    pub minimum_contrast_ratio: f32,
    pub theme: Theme,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            font_family: "courier-new, courier, monospace".to_string(),
            font_size: 15.0,
            line_height: 1.0,
            letter_spacing: 0.0,
            cursor_blink: false,
            scrollback: 1000,
            draw_bold_text_in_bright_colors: true,
            minimum_contrast_ratio: 1.0,
            theme: Theme::default(),
        }
    }
}

fn get(obj: &JsValue, key: &str) -> Option<JsValue> {
    if !obj.is_object() {
        return None;
    }
    js_sys::Reflect::get(obj, &JsValue::from_str(key))
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null())
}

fn get_f64(obj: &JsValue, key: &str) -> Option<f64> {
    get(obj, key)
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite())
}

/// Lee un objeto `ITheme` de xterm.js.
pub fn read_theme(obj: &JsValue) -> Theme {
    let color = |key: &str| {
        get(obj, key)
            .and_then(|v| v.as_string())
            .and_then(|s| theme::parse_color(&s))
    };
    let mut theme = Theme {
        foreground: color("foreground"),
        background: color("background"),
        cursor: color("cursor"),
        cursor_accent: color("cursorAccent"),
        selection: color("selectionBackground"),
        ..Theme::default()
    };
    for (slot, name) in theme.ansi.iter_mut().zip(ANSI_NAMES) {
        *slot = color(name);
    }
    theme
}

/// Tamaño de letra aceptable: finito y positivo, acotado a 1–512 px (uno
/// absurdo desbordaría las cuentas de los canvases).
pub fn font_size_in_range(size: f64) -> Option<f64> {
    (size.is_finite() && size > 0.0).then(|| size.clamp(1.0, 512.0))
}

/// Lee las opciones de `new Terminal({...})`.
pub fn read_options(obj: &JsValue) -> Options {
    let d = Options::default();
    Options {
        font_family: get(obj, "fontFamily")
            .and_then(|v| v.as_string())
            .unwrap_or(d.font_family),
        font_size: get_f64(obj, "fontSize")
            .and_then(font_size_in_range)
            .unwrap_or(d.font_size),
        // xterm.js exige `lineHeight ≥ 1`; el tope evita celdas absurdas.
        line_height: get_f64(obj, "lineHeight")
            .filter(|v| *v >= 1.0)
            .map(|v| v.min(10.0))
            .unwrap_or(d.line_height),
        letter_spacing: get_f64(obj, "letterSpacing")
            .map(|v| v.clamp(-100.0, 100.0))
            .unwrap_or(d.letter_spacing),
        cursor_blink: get(obj, "cursorBlink")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.cursor_blink),
        scrollback: get_f64(obj, "scrollback")
            .filter(|v| *v >= 0.0)
            // Acotado antes de convertir: la conversión satura.
            .map(|v| v.min(1e7) as usize)
            .unwrap_or(d.scrollback),
        draw_bold_text_in_bright_colors: get(obj, "drawBoldTextInBrightColors")
            .and_then(|v| v.as_bool())
            .unwrap_or(d.draw_bold_text_in_bright_colors),
        minimum_contrast_ratio: get_f64(obj, "minimumContrastRatio")
            .map(|v| v.clamp(1.0, 21.0) as f32)
            .unwrap_or(d.minimum_contrast_ratio),
        theme: get(obj, "theme")
            .map(|t| read_theme(&t))
            .unwrap_or_default(),
    }
}

fn canvas_theme(p: &Palette) -> CanvasTheme {
    CanvasTheme {
        bg: p.bg,
        cursor: p.cursor,
        cursor_accent: p.cursor_accent,
    }
}

/// Elementos del DOM, con las clases de xterm.js para que el CSS de
/// `xterm.css` y de `term.html` (barra de desplazamiento) aplique igual.
struct Dom {
    root: HtmlElement,
    viewport: HtmlElement,
    scroll_area: HtmlElement,
    screen: HtmlElement,
}

impl Dom {
    fn new(document: &Document, host: &HtmlElement) -> Result<Dom, JsValue> {
        let div = |class: &str| -> Result<HtmlElement, JsValue> {
            let el: HtmlElement = document.create_element("div")?.dyn_into()?;
            el.set_class_name(class);
            Ok(el)
        };
        let root = div("terminal xterm")?;
        root.set_dir("ltr");
        let viewport = div("xterm-viewport")?;
        let scroll_area = div("xterm-scroll-area")?;
        let screen = div("xterm-screen")?;
        viewport.append_child(&scroll_area)?;
        root.append_child(&viewport)?;
        root.append_child(&screen)?;
        host.append_child(&root)?;
        Ok(Dom {
            root,
            viewport,
            scroll_area,
            screen,
        })
    }
}

/// Lo último que se escribió en la barra (`Viewport._innerRefresh`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct ScrollSync {
    /// `viewport.offsetHeight`, leído una vez por cambio de tamaño (leerlo
    /// fuerza un cálculo de estilo y diseño).
    viewport_h: Option<f64>,
    buffer_len: usize,
    css_canvas_h: f64,
    dev_cell_h: u32,
    scroll_top: f64,
}

struct Inner {
    window: Window,
    document: Document,
    perf: Option<Performance>,
    host: HtmlElement,
    dom: Dom,
    engine: Engine,
    palette: Palette,
    render_opts: RenderOpts,
    scheduler: Scheduler,
    painter: Canvas2d,
    metrics: CellMetrics,
    opts: Options,
    size: GridSize,
    scrollbar_w: f64,
    focused: bool,
    blink: Blink,
    /// Fase visible del parpadeo, fijada por el temporizador.
    blink_on: bool,
    frame_cb: Option<Closure<dyn FnMut(f64)>>,
    frame_id: Option<i32>,
    timer_cb: Option<Closure<dyn FnMut()>>,
    /// `devicePixelContentBox` del canvas (si el navegador lo da).
    observer: Option<ResizeObserver>,
    observer_cb: Option<Closure<dyn FnMut(js_sys::Array)>>,
    /// `contextrestored` del canvas (tras perderse la GPU).
    restored_cb: Option<Closure<dyn FnMut()>>,
    /// Texto reutilizado para la altura del área de desplazamiento.
    css_buf: String,
    /// Temporizador armado y para cuándo (ms de `performance.now()`).
    timer: Option<(i32, f64)>,
    replies: Vec<u8>,
    title: Option<String>,
    bell: bool,
    clipboard: Option<(ClipboardTarget, String)>,
    scroll_sync: ScrollSync,
}

impl Inner {
    fn now(&self) -> f64 {
        self.perf.as_ref().map_or(0.0, Performance::now)
    }

    fn dpr(&self) -> f64 {
        let dpr = self.window.device_pixel_ratio();
        if dpr.is_finite() && dpr > 0.0 {
            dpr
        } else {
            1.0
        }
    }

    fn measure(&self) -> Result<CellMetrics, JsValue> {
        let (w, h) = metrics::measure(
            &self.document,
            &self.dom.root,
            &self.opts.font_family,
            self.opts.font_size,
        )?;
        Ok(metrics::from_measure(
            w,
            h,
            self.dpr(),
            self.opts.line_height,
            self.opts.letter_spacing,
        ))
    }

    /// Estado del cursor para pintar. La fase del parpadeo se fija en el
    /// temporizador (como el `setInterval` de xterm.js), no al pintar: un
    /// cuadro retrasado no se salta una fase.
    fn cursor_input(&mut self) -> CursorInput {
        let enabled = paint::blink_enabled(&self.engine, self.focused);
        if self.blink.set_enabled(enabled, self.now()) {
            // Vuelve a parpadear (cursor de nuevo en la vista, foco…): como
            // `restartBlinkAnimation`, empieza visible.
            self.blink_on = true;
        }
        CursorInput {
            focused: self.focused,
            blink_on: !self.blink.enabled || self.blink_on,
        }
    }

    /// `restartBlinkAnimation`: el cursor vuelve a verse y el ciclo empieza.
    fn restart_blink(&mut self, now: f64) {
        self.blink.restart(now);
        self.blink_on = true;
    }

    /// Junta lo que dejó el motor (respuestas, título, campana…).
    fn drain_engine(&mut self) {
        let d = self.engine.drain();
        self.replies.extend_from_slice(&d.replies);
        if d.title.is_some() {
            self.title = d.title;
        }
        self.bell |= d.bell;
        if let (Some(target), Some(text)) = (d.clipboard_target, d.clipboard) {
            self.clipboard = Some((target, text));
        }
    }

    /// Recoge daños y pide cuadro si hace falta.
    fn touch(&mut self) {
        let input = self.cursor_input();
        self.scheduler.absorb(&mut self.engine, input);
        self.after_touch();
    }

    /// Como [`Inner::touch`] cuando el motor no cambió (foco, parpadeo):
    /// solo cuenta el cursor, no el daño que alacritty pone siempre en su
    /// fila.
    fn touch_cursor(&mut self) {
        let input = self.cursor_input();
        self.scheduler.absorb_cursor(&self.engine, input);
        self.after_touch();
    }

    fn after_touch(&mut self) {
        let now = self.now();
        self.request_frame();
        self.arm_timer(now);
    }

    fn request_frame(&mut self) {
        if !self.scheduler.wants_frame() {
            return;
        }
        let Some(cb) = self.frame_cb.as_ref() else {
            self.scheduler.frame_cancelled();
            return;
        };
        match self
            .window
            .request_animation_frame(cb.as_ref().unchecked_ref())
        {
            Ok(id) => self.frame_id = Some(id),
            Err(_) => self.scheduler.frame_cancelled(),
        }
    }

    fn on_frame(&mut self) {
        self.frame_id = None;
        let now = self.now();
        let input = self.cursor_input();
        self.scheduler.frame(
            &mut self.engine,
            &self.palette,
            &self.render_opts,
            input,
            &self.metrics,
            &mut self.painter,
        );
        self.sync_scroll_area();
        self.arm_timer(now);
    }

    /// Un solo temporizador: el próximo cambio del parpadeo o el
    /// vencimiento de una actualización sincronizada, lo que llegue antes.
    fn arm_timer(&mut self, now: f64) {
        let blink = self.blink.next_toggle(now);
        let sync = self.engine.next_deadline_ms();
        let wake = match (blink, sync) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let Some(wake) = wake else {
            if let Some((id, _)) = self.timer.take() {
                self.window.clear_timeout_with_handle(id);
            }
            return;
        };
        if let Some((id, at)) = self.timer {
            if at <= wake {
                return;
            }
            self.window.clear_timeout_with_handle(id);
            self.timer = None;
        }
        let Some(cb) = self.timer_cb.as_ref() else {
            return;
        };
        // Un temporizador nunca es negativo; un retardo absurdo se acota.
        let delay = (wake - now).clamp(0.0, 1e9).ceil() as i32;
        if let Ok(id) = self
            .window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                delay,
            )
        {
            self.timer = Some((id, wake));
        }
    }

    fn on_timer(&mut self) {
        self.timer = None;
        let now = self.now();
        let flushed = self.engine.tick(now);
        if flushed {
            self.drain_engine();
        }
        self.blink_on = self.blink.visible(now);
        if flushed {
            self.touch();
        } else {
            self.touch_cursor();
        }
    }

    /// `Viewport._innerRefresh`: alto del área y posición de la barra.
    fn sync_scroll_area(&mut self) {
        let rows = usize::from(self.size.rows);
        let history = self.engine.history_len();
        let buffer_len = history + rows;
        let row_h = f64::from(self.metrics.dev_h) / self.metrics.dpr;
        let (_, css_canvas_h) = self.metrics.css_canvas(self.size.cols, self.size.rows);
        let ydisp = history.saturating_sub(self.engine.display_offset());
        let scroll_top = ydisp as f64 * row_h;
        let viewport_h = self
            .scroll_sync
            .viewport_h
            .unwrap_or_else(|| f64::from(self.dom.viewport.offset_height()));
        let next = ScrollSync {
            viewport_h: Some(viewport_h),
            buffer_len,
            css_canvas_h,
            dev_cell_h: self.metrics.dev_h,
            scroll_top,
        };
        if next == self.scroll_sync {
            return;
        }
        let layout_changed = (
            next.viewport_h,
            next.buffer_len,
            next.css_canvas_h,
            next.dev_cell_h,
        ) != (
            self.scroll_sync.viewport_h,
            self.scroll_sync.buffer_len,
            self.scroll_sync.css_canvas_h,
            self.scroll_sync.dev_cell_h,
        );
        if layout_changed {
            let height = metrics::js_round(row_h * buffer_len as f64) + (viewport_h - css_canvas_h);
            self.css_buf.clear();
            let _ = write!(self.css_buf, "{height}px");
            let _ = self
                .dom
                .scroll_area
                .style()
                .set_property("height", &self.css_buf);
        }
        self.dom.viewport.set_scroll_top(scroll_top.round() as i32);
        self.scroll_sync = next;
    }

    /// Aplica medidas y tamaño al canvas y al DOM y repinta todo.
    fn relayout(&mut self, size: GridSize) {
        let changed = size != self.size;
        self.size = size;
        if changed {
            let (cw, ch) = self
                .metrics
                .css_cell(size.cols, size.rows)
                .unwrap_or((self.metrics.css_w, self.metrics.css_h));
            let cell = (
                metrics::js_round(cw).clamp(0.0, 65535.0) as u16,
                metrics::js_round(ch).clamp(0.0, 65535.0) as u16,
            );
            self.engine.resize(size, cell);
        }
        self.painter.resize(size.cols, size.rows, &self.metrics);
        self.scheduler.resize(size.rows);
        let (w, h) = self.metrics.css_canvas(size.cols, size.rows);
        let style = self.dom.screen.style();
        let _ = style.set_property("width", &format!("{w}px"));
        let _ = style.set_property("height", &format!("{h}px"));
        self.scroll_sync = ScrollSync::default();
        self.touch();
    }

    fn on_device_pixels(&mut self, entries: &js_sys::Array) {
        let canvas: &JsValue = self.painter.element();
        let entry = entries
            .iter()
            .find(|e| js_sys::Reflect::get(e, &"target".into()).is_ok_and(|t| t == *canvas));
        let Some(entry) = entry else {
            return;
        };
        let size = js_sys::Reflect::get(&entry, &"devicePixelContentBoxSize".into())
            .ok()
            .filter(|v| !v.is_undefined())
            .and_then(|list| js_sys::Reflect::get_u32(&list, 0).ok());
        let Some(size) = size.filter(|v| !v.is_undefined()) else {
            // Sin la medida en píxeles de dispositivo no hay nada que seguir.
            if let Some(observer) = self.observer.take() {
                observer.disconnect();
            }
            return;
        };
        let dim = |key: &str| {
            js_sys::Reflect::get(&size, &key.into())
                .ok()
                .and_then(|v| v.as_f64())
                .filter(|v| v.is_finite() && *v > 0.0)
                .map(|v| v.min(f64::from(u32::MAX)) as u32)
        };
        let (Some(w), Some(h)) = (dim("inlineSize"), dim("blockSize")) else {
            return;
        };
        // Otro monitor con otro `devicePixelRatio` sin cambio de tamaño CSS:
        // hay que volver a medir, si no se pinta con la celda vieja.
        if self.dpr() != self.metrics.dpr && matches!(self.remeasure(), Ok(true)) {
            let grid = self.size;
            self.painter.set_device_size(w, h);
            self.relayout(grid);
            return;
        }
        if self.painter.set_device_size(w, h) {
            self.scheduler.resize(self.size.rows);
            self.touch();
        }
    }

    fn remeasure(&mut self) -> Result<bool, JsValue> {
        let m = self.measure()?;
        if !m.is_valid() || m == self.metrics {
            return Ok(false);
        }
        self.metrics = m;
        Ok(true)
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(cb) = self.restored_cb.take() {
            let _ = self.painter.element().remove_event_listener_with_callback(
                "contextrestored",
                cb.as_ref().unchecked_ref(),
            );
        }
        if let Some(observer) = self.observer.take() {
            observer.disconnect();
        }
        if let Some(id) = self.frame_id.take() {
            let _ = self.window.cancel_animation_frame(id);
        }
        if let Some((id, _)) = self.timer.take() {
            self.window.clear_timeout_with_handle(id);
        }
        self.dom.root.remove();
    }
}

/// Terminal en el navegador: motor, planificador y canvas.
#[wasm_bindgen]
pub struct WebTerm {
    inner: Rc<RefCell<Inner>>,
}

/// Ejecuta `f` sobre el estado si sigue vivo y nadie lo tiene prestado.
fn with_weak(weak: &std::rc::Weak<RefCell<Inner>>, f: impl FnOnce(&mut Inner)) {
    if let Some(rc) = weak.upgrade()
        && let Ok(mut inner) = rc.try_borrow_mut()
    {
        f(&mut inner);
    }
}

#[wasm_bindgen]
impl WebTerm {
    /// Monta la terminal dentro de `host` con las opciones de
    /// `new Terminal({...})` de xterm.js (`fontFamily`, `fontSize`,
    /// `lineHeight`, `letterSpacing`, `cursorBlink`, `scrollback`, `theme`).
    /// Empieza con 80×24; el tamaño real llega con [`WebTerm::resize_to_fit`].
    #[wasm_bindgen(constructor)]
    pub fn new(host: HtmlElement, opts: JsValue) -> Result<WebTerm, JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("sin window"))?;
        let document = window
            .document()
            .ok_or_else(|| JsValue::from_str("sin document"))?;
        let opts = read_options(&opts);
        let palette = opts.theme.palette();
        let dom = Dom::new(&document, &host)?;
        let _ = dom
            .viewport
            .style()
            .set_property("background-color", &canvas::hex(palette.bg));
        let size = GridSize { cols: 80, rows: 24 };
        let dpr = {
            let d = window.device_pixel_ratio();
            if d.is_finite() && d > 0.0 { d } else { 1.0 }
        };
        let (cw, ch) = metrics::measure(&document, &dom.root, &opts.font_family, opts.font_size)?;
        let m = metrics::from_measure(cw, ch, dpr, opts.line_height, opts.letter_spacing);
        let painter = Canvas2d::new(
            &window,
            &document,
            canvas_theme(&palette),
            &opts.font_family,
            opts.font_size,
            m,
        )?;
        dom.screen.append_child(painter.element())?;
        let scrollbar_w = metrics::scrollbar_width(&dom.viewport, &dom.scroll_area);
        let engine =
            Engine::with_cursor_blink(size, opts.scrollback, palette.clone(), opts.cursor_blink);
        let render_opts = RenderOpts {
            bold_is_bright: opts.draw_bold_text_in_bright_colors,
            min_contrast: opts.minimum_contrast_ratio,
        };
        let perf = window.performance();
        let now = perf.as_ref().map_or(0.0, Performance::now);
        let inner = Rc::new(RefCell::new(Inner {
            window,
            document,
            perf,
            host,
            dom,
            engine,
            palette,
            render_opts,
            scheduler: Scheduler::new(size.rows),
            painter,
            metrics: m,
            opts,
            size,
            scrollbar_w,
            focused: false,
            blink: Blink::new(false, now),
            blink_on: true,
            frame_cb: None,
            frame_id: None,
            timer_cb: None,
            observer: None,
            observer_cb: None,
            restored_cb: None,
            css_buf: String::new(),
            timer: None,
            replies: Vec::new(),
            title: None,
            bell: false,
            clipboard: None,
            scroll_sync: ScrollSync::default(),
        }));
        let weak = Rc::downgrade(&inner);
        let frame_cb = Closure::<dyn FnMut(f64)>::new(move |_t: f64| {
            with_weak(&weak, Inner::on_frame);
        });
        let weak = Rc::downgrade(&inner);
        let timer_cb = Closure::<dyn FnMut()>::new(move || {
            with_weak(&weak, Inner::on_timer);
        });
        {
            let mut i = inner
                .try_borrow_mut()
                .map_err(|_| JsValue::from_str("estado ocupado"))?;
            i.frame_cb = Some(frame_cb);
            i.timer_cb = Some(timer_cb);
            i.relayout(size);
        }
        let term = WebTerm { inner };
        term.watch_font();
        term.observe_device_pixels();
        term.watch_context();
        Ok(term)
    }

    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> Option<T> {
        self.inner.try_borrow_mut().ok().map(|mut i| f(&mut i))
    }

    /// Cuando la fuente termine de cargar (`document.fonts.load`), vuelve a
    /// medir y repinta si la celda cambió.
    fn watch_font(&self) {
        let Some((promise, weak)) = self.with(|i| {
            let px = i.opts.font_size;
            let spec = format!("{px}px {}", i.opts.font_family);
            (i.document.fonts().load(&spec), Rc::downgrade(&self.inner))
        }) else {
            return;
        };
        // Un solo cierre para los dos casos: la promesa llama exactamente a
        // uno, y `once_into_js` lo libera tras esa llamada (dos cierres
        // dejarían siempre uno sin liberar).
        let settle = Closure::<dyn FnMut(JsValue)>::once_into_js(move |_v: JsValue| {
            with_weak(&weak, |i| {
                // Aunque la celda mida igual, lo rasterizado con la fuente de
                // respaldo sobra.
                let (family, size) = (i.opts.font_family.clone(), i.opts.font_size);
                i.painter.set_font(&family, size);
                let _ = i.remeasure();
                let grid = i.size;
                i.relayout(grid);
            });
        });
        promise
            .unchecked_ref::<PromiseThen>()
            .then2(&settle, &settle);
    }

    /// Si el navegador pierde y restaura el contexto del canvas (reinicio de
    /// la GPU), las páginas del atlas pueden haber quedado vacías: se vacía
    /// todo y se repinta. xterm.js 0.7 no lo trata.
    fn watch_context(&self) {
        let weak = Rc::downgrade(&self.inner);
        let cb = Closure::<dyn FnMut()>::new(move || {
            with_weak(&weak, |i| {
                i.painter.reset_caches();
                let grid = i.size;
                i.relayout(grid);
            });
        });
        self.with(|i| {
            let added = i
                .painter
                .element()
                .add_event_listener_with_callback("contextrestored", cb.as_ref().unchecked_ref());
            if added.is_ok() {
                i.restored_cb = Some(cb);
            }
        });
    }

    /// `observeDevicePixelDimensions` de xterm.js: el almacén del canvas
    /// sigue a los píxeles de dispositivo que el navegador le da.
    fn observe_device_pixels(&self) {
        let weak = Rc::downgrade(&self.inner);
        let cb = Closure::<dyn FnMut(js_sys::Array)>::new(move |entries: js_sys::Array| {
            with_weak(&weak, |i| i.on_device_pixels(&entries));
        });
        let Ok(observer) = ResizeObserver::new(cb.as_ref().unchecked_ref()) else {
            return;
        };
        let opts = ResizeObserverOptions::new();
        opts.set_box(ResizeObserverBoxOptions::DevicePixelContentBox);
        self.with(|i| {
            // Sin `device-pixel-content-box` (navegadores viejos) se lanza:
            // se queda el tamaño calculado, como en xterm.js.
            let target: &JsValue = i.painter.element();
            if observer
                .unchecked_ref::<ObserveCatch>()
                .observe_catch(target, &opts)
                .is_ok()
            {
                i.observer = Some(observer);
                i.observer_cb = Some(cb);
            } else {
                observer.disconnect();
            }
        });
    }

    /// Bytes del PTY.
    pub fn write(&mut self, bytes: &[u8]) {
        self.with(|i| {
            let now = i.now();
            i.engine.advance(bytes, now);
            i.drain_engine();
            // `handleGridChanged`: cada cambio deja el cursor visible.
            i.restart_blink(now);
            i.touch();
        });
    }

    /// Ajusta filas y columnas al contenedor como `FitAddon.fit()` y
    /// devuelve `{cols, rows}`.
    pub fn resize_to_fit(&mut self) -> JsValue {
        let size = self.with(|i| {
            if i.dpr() != i.metrics.dpr {
                let _ = i.remeasure();
            }
            let style = i.window.get_computed_style(&i.host).ok().flatten();
            let prop = |name: &str| {
                style
                    .as_ref()
                    .and_then(|s| s.get_property_value(name).ok())
                    .map(|v| metrics::parse_css_int(&v))
                    .unwrap_or(f64::NAN)
            };
            // `Math.max(0, NaN)` es NaN: `f64::max` daría 0.
            let width = prop("width");
            let width = if width.is_nan() {
                width
            } else {
                width.max(0.0)
            };
            let height = prop("height");
            // El elemento `.xterm` no tiene relleno propio (xterm.css).
            let scrollbar = if i.opts.scrollback == 0 {
                0.0
            } else {
                i.scrollbar_w
            };
            let current = (i.size.cols, i.size.rows);
            let Some((cols, rows)) = metrics::fit(width, height, scrollbar, &i.metrics, current)
            else {
                // Sin celda medida o sin tamaño numérico FitAddon no toca
                // la terminal.
                return i.size;
            };
            let size = GridSize { cols, rows };
            if size != i.size || i.painter_needs_layout() {
                i.relayout(size);
            } else {
                // El contenedor cambió sin cambiar la rejilla: la barra
                // depende del alto del viewport.
                i.scroll_sync.viewport_h = None;
                i.sync_scroll_area();
            }
            size
        });
        let size = size.unwrap_or(GridSize { cols: 0, rows: 0 });
        size_object(size.cols, size.rows)
    }

    /// Cambia el tema (objeto `ITheme` de xterm.js) y repinta.
    pub fn set_theme(&mut self, theme: JsValue) {
        self.with(|i| {
            let parsed = read_theme(&theme);
            i.opts.theme = parsed;
            i.palette = i.opts.theme.palette();
            i.engine.set_palette(i.palette.clone());
            i.painter.set_theme(canvas_theme(&i.palette));
            let _ = i
                .dom
                .viewport
                .style()
                .set_property("background-color", &canvas::hex(i.palette.bg));
            let size = i.size;
            i.relayout(size);
        });
    }

    /// Cambia la fuente; las columnas y filas se ajustan con
    /// [`WebTerm::resize_to_fit`].
    pub fn set_font(&mut self, family: &str, size: f64) {
        let changed = self.with(|i| {
            let Some(size) = font_size_in_range(size) else {
                return false;
            };
            i.opts.font_family = family.to_string();
            i.opts.font_size = size;
            i.painter.set_font(family, size);
            let _ = i.remeasure();
            let grid = i.size;
            i.relayout(grid);
            true
        });
        if changed == Some(true) {
            self.watch_font();
        }
    }

    /// Respuestas para el PTY (DA, DSR, colores…) desde la última llamada.
    pub fn take_replies(&mut self) -> Vec<u8> {
        self.with(|i| std::mem::take(&mut i.replies))
            .unwrap_or_default()
    }

    /// Último título pedido por la aplicación (`""` = el de por omisión).
    pub fn take_title(&mut self) -> Option<String> {
        self.with(|i| i.title.take()).flatten()
    }

    /// Sonó la campana desde la última llamada.
    pub fn take_bell(&mut self) -> bool {
        self.with(|i| std::mem::take(&mut i.bell)).unwrap_or(false)
    }

    /// Texto que la aplicación quiso copiar (OSC 52); la página decide si
    /// lo lleva al portapapeles.
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.with(|i| i.clipboard.take().map(|(_, text)| text))
            .flatten()
    }

    /// Foco de la terminal (lo da el teclado, A7): con foco el cursor
    /// parpadea; sin foco se dibuja su contorno.
    pub fn set_focus(&mut self, focused: bool) {
        self.with(|i| {
            if i.focused != focused {
                i.focused = focused;
                let now = i.now();
                i.restart_blink(now);
                i.touch_cursor();
            }
        });
    }

    /// Desplaza la vista por la historia (positivo = hacia atrás).
    pub fn scroll_lines(&mut self, lines: i32) {
        self.with(|i| {
            i.engine.scroll_display(lines);
            // `handleGridChanged` al repintar las filas.
            let now = i.now();
            i.restart_blink(now);
            i.touch();
        });
    }

    /// Elemento `.xterm-screen` (para la entrada y los enlaces).
    pub fn screen(&self) -> Option<HtmlElement> {
        self.inner.try_borrow().ok().map(|i| i.dom.screen.clone())
    }

    /// Medidas actuales: `{cols, rows, cssCellWidth, cssCellHeight,
    /// deviceCellWidth, deviceCellHeight, dpr, cssCanvasWidth, cssCanvasHeight}`.
    pub fn dimensions(&self) -> JsValue {
        let Ok(i) = self.inner.try_borrow() else {
            return JsValue::NULL;
        };
        let obj = size_object(i.size.cols, i.size.rows);
        let m = &i.metrics;
        let (cw, ch) = m.css_canvas(i.size.cols, i.size.rows);
        // La celda CSS de addon-canvas (la que usan FitAddon y el ratón).
        let (cell_w, cell_h) = m.css_cell(i.size.cols, i.size.rows).unwrap_or((0.0, 0.0));
        for (key, value) in [
            ("cssCellWidth", cell_w),
            ("cssCellHeight", cell_h),
            ("deviceCellWidth", f64::from(m.dev_w)),
            ("deviceCellHeight", f64::from(m.dev_h)),
            ("dpr", m.dpr),
            ("cssCanvasWidth", cw),
            ("cssCanvasHeight", ch),
        ] {
            let _ = js_sys::Reflect::set(&obj, &key.into(), &JsValue::from_f64(value));
        }
        obj
    }

    /// No hay cuadro pedido ni nada pendiente de pintar (para las pruebas
    /// del presupuesto de CPU en reposo).
    pub fn is_idle(&self) -> bool {
        self.inner
            .try_borrow()
            .is_ok_and(|i| i.frame_id.is_none() && !i.scheduler.has_work())
    }

    /// Primer error de pintado desde la última consulta (`null` si no hubo).
    pub fn take_paint_error(&mut self) -> JsValue {
        self.with(|i| i.painter.take_error())
            .flatten()
            .unwrap_or(JsValue::NULL)
    }
}

impl Inner {
    /// El canvas aún no tiene tamaño (primer ajuste tras medir).
    fn painter_needs_layout(&self) -> bool {
        let (w, h) = self.metrics.device_canvas(self.size.cols, self.size.rows);
        let canvas = self.painter.element();
        canvas.width() != w || canvas.height() != h
    }
}

fn size_object(cols: u16, rows: u16) -> JsValue {
    let obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&obj, &"cols".into(), &JsValue::from(cols));
    let _ = js_sys::Reflect::set(&obj, &"rows".into(), &JsValue::from(rows));
    obj.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_xterm_defaults() {
        let o = Options::default();
        assert_eq!(o.font_size, 15.0);
        assert_eq!(o.line_height, 1.0);
        assert!(!o.cursor_blink);
        assert_eq!(o.scrollback, 1000);
        assert!(o.draw_bold_text_in_bright_colors);
    }

    #[test]
    fn canvas_theme_takes_palette_colors() {
        let p = Palette::xterm_default([1; 3], [2; 3], [3; 3], [4; 3], [5; 3]);
        let t = canvas_theme(&p);
        assert_eq!((t.bg, t.cursor, t.cursor_accent), ([2; 3], [3; 3], [4; 3]));
    }

    #[test]
    fn absurd_font_sizes_are_rejected_or_clamped() {
        assert_eq!(font_size_in_range(14.0), Some(14.0));
        assert_eq!(font_size_in_range(0.25), Some(1.0));
        assert_eq!(font_size_in_range(1e9), Some(512.0));
        for bad in [0.0, -3.0, f64::NAN, f64::INFINITY] {
            assert_eq!(font_size_in_range(bad), None);
        }
    }
}
