//! Pruebas en navegador de `WebTerm` (`wasm-bindgen-test`). No corren en
//! host ni con un navegador local: las ejecuta A12 en el Chrome del Mac
//! (`cargo run -p xtask -- term-bench --wasm-tests`).
#![cfg(target_arch = "wasm32")]

use comandos_term_web::WebTerm;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlElement};

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window()
        .and_then(|w| w.document())
        .unwrap_or_else(|| panic!("sin document"))
}

/// Un contenedor de `w × h` píxeles CSS en el cuerpo de la página.
fn host(w: u32, h: u32) -> HtmlElement {
    let doc = document();
    let el: HtmlElement = doc
        .create_element("div")
        .ok()
        .and_then(|e| e.dyn_into().ok())
        .unwrap_or_else(|| panic!("sin div"));
    let _ = el.style().set_property("width", &format!("{w}px"));
    let _ = el.style().set_property("height", &format!("{h}px"));
    let _ = doc.body().map(|b| b.append_child(&el));
    el
}

fn options() -> JsValue {
    let theme = js_sys::Object::new();
    let set = |o: &js_sys::Object, k: &str, v: JsValue| {
        let _ = js_sys::Reflect::set(o, &k.into(), &v);
    };
    set(&theme, "background", "#0A0D13".into());
    set(&theme, "foreground", "#EAF0FB".into());
    set(&theme, "cursor", "#FFAE1A".into());
    set(&theme, "cursorAccent", "#0A0D13".into());
    let o = js_sys::Object::new();
    set(&o, "fontFamily", "monospace".into());
    set(&o, "fontSize", 14.0.into());
    set(&o, "lineHeight", 1.2.into());
    set(&o, "cursorBlink", true.into());
    set(&o, "scrollback", 10_000.0.into());
    set(&o, "theme", theme.into());
    o.into()
}

/// Espera al siguiente `requestAnimationFrame`.
async fn next_frame() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let cb = Closure::once_into_js(move |_t: f64| {
            let _ = resolve.call0(&JsValue::NULL);
        });
        let _ = web_sys::window().map(|w| w.request_animation_frame(cb.unchecked_ref()));
    });
    let _ = JsFuture::from(promise).await;
}

fn num(obj: &JsValue, key: &str) -> f64 {
    js_sys::Reflect::get(obj, &key.into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN)
}

fn canvas_of(host: &HtmlElement) -> HtmlCanvasElement {
    host.query_selector(".xterm-screen canvas")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into().ok())
        .unwrap_or_else(|| panic!("sin canvas"))
}

/// Color del píxel de dispositivo `(x, y)` del canvas.
fn pixel(canvas: &HtmlCanvasElement, x: f64, y: f64) -> [u8; 3] {
    let ctx: CanvasRenderingContext2d = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|c| c.dyn_into().ok())
        .unwrap_or_else(|| panic!("sin contexto"));
    let data = ctx
        .get_image_data(x, y, 1.0, 1.0)
        .map(|d| d.data().0)
        .unwrap_or_default();
    [
        data.first().copied().unwrap_or(0),
        data.get(1).copied().unwrap_or(0),
        data.get(2).copied().unwrap_or(0),
    ]
}

/// Las capas de `.xterm-screen` compuestas en orden del DOM (lo que se ve).
fn composite(host: &HtmlElement) -> HtmlCanvasElement {
    let main = canvas_of(host);
    let doc = document();
    let out: HtmlCanvasElement = doc
        .create_element("canvas")
        .ok()
        .and_then(|e| e.dyn_into().ok())
        .unwrap_or_else(|| panic!("sin canvas"));
    out.set_width(main.width());
    out.set_height(main.height());
    let ctx: CanvasRenderingContext2d = out
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|c| c.dyn_into().ok())
        .unwrap_or_else(|| panic!("sin contexto"));
    let layers = host
        .query_selector_all(".xterm-screen canvas")
        .unwrap_or_else(|e| panic!("{e:?}"));
    for i in 0..layers.length() {
        if let Some(layer) = layers
            .get(i)
            .and_then(|n| n.dyn_into::<HtmlCanvasElement>().ok())
            && layer.width() == main.width()
        {
            let _ = ctx.draw_image_with_html_canvas_element(&layer, 0.0, 0.0);
        }
    }
    out
}

#[wasm_bindgen_test]
async fn mounts_with_xterm_dom_and_fits_like_fitaddon() {
    let h = host(800, 450);
    let mut term = WebTerm::new(h.clone(), options()).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(
        h.query_selector(".xterm .xterm-viewport .xterm-scroll-area")
            .ok()
            .flatten()
            .is_some()
    );
    // FitAddon mide con la celda CSS de la rejilla actual (antes del ajuste).
    let before = term.dimensions();
    let (cw, ch) = (num(&before, "cssCellWidth"), num(&before, "cssCellHeight"));
    assert!(cw > 0.0 && ch > 0.0);
    let size = term.resize_to_fit();
    let dims = term.dimensions();
    let cols = num(&size, "cols");
    let rows = num(&size, "rows");
    // Columnas: `floor((800 − barra) / cssCellWidth)`, con barra ≥ 0.
    assert!(cols >= (785.0 / cw).floor());
    assert!(cols <= (800.0 / cw).floor());
    assert_eq!(rows, (450.0 / ch).floor().max(1.0));
    let canvas = canvas_of(&h);
    assert_eq!(
        f64::from(canvas.width()),
        cols * num(&dims, "deviceCellWidth")
    );
    h.remove();
}

#[wasm_bindgen_test]
async fn paints_backgrounds_and_goes_idle() {
    let h = host(400, 200);
    let mut term = WebTerm::new(h.clone(), options()).unwrap_or_else(|e| panic!("{e:?}"));
    term.resize_to_fit();
    // Fondo rojo (paleta xterm.js: #cc0000) en las dos primeras celdas.
    term.write(b"\x1b[41m  \x1b[0m");
    next_frame().await;
    next_frame().await;
    let dims = term.dimensions();
    let (cw, ch) = (
        num(&dims, "deviceCellWidth"),
        num(&dims, "deviceCellHeight"),
    );
    let canvas = canvas_of(&h);
    assert_eq!(pixel(&canvas, cw * 0.5, ch * 0.5), [0xCC, 0x00, 0x00]);
    // Fondo del tema fuera del texto.
    assert_eq!(pixel(&canvas, cw * 10.5, ch * 1.5), [0x0A, 0x0D, 0x13]);
    // Sin foco no parpadea: nada pendiente tras pintar.
    assert!(term.is_idle());
    assert!(term.take_paint_error().is_null());
    h.remove();
}

#[wasm_bindgen_test]
async fn block_cursor_and_outline_when_blurred() {
    let h = host(400, 200);
    let mut term = WebTerm::new(h.clone(), options()).unwrap_or_else(|e| panic!("{e:?}"));
    term.resize_to_fit();
    term.set_focus(true);
    next_frame().await;
    next_frame().await;
    let dims = term.dimensions();
    let (cw, ch) = (
        num(&dims, "deviceCellWidth"),
        num(&dims, "deviceCellHeight"),
    );
    // Bloque del color del cursor en la celda (0, 0), en su propia capa
    // encima de la selección: el canvas del texto solo tiene el fondo.
    assert_eq!(
        pixel(&composite(&h), cw * 0.5, ch * 0.5),
        [0xFF, 0xAE, 0x1A]
    );
    assert_eq!(
        pixel(&canvas_of(&h), cw * 0.5, ch * 0.5),
        [0x0A, 0x0D, 0x13]
    );
    // Con foco y parpadeo hay un temporizador, pero ningún cuadro en vuelo.
    term.set_focus(false);
    next_frame().await;
    next_frame().await;
    // Sin foco: contorno; el centro de la celda vuelve al fondo.
    let seen = composite(&h);
    assert_eq!(pixel(&seen, cw * 0.5, ch * 0.5), [0x0A, 0x0D, 0x13]);
    assert_eq!(pixel(&seen, 0.0, ch * 0.5), [0xFF, 0xAE, 0x1A]);
    assert!(term.is_idle());
    h.remove();
}

#[wasm_bindgen_test]
async fn theme_change_repaints_everything() {
    let h = host(400, 200);
    let mut term = WebTerm::new(h.clone(), options()).unwrap_or_else(|e| panic!("{e:?}"));
    term.resize_to_fit();
    next_frame().await;
    let theme = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&theme, &"background".into(), &"#300A24".into());
    term.set_theme(theme.into());
    next_frame().await;
    next_frame().await;
    let dims = term.dimensions();
    let (cw, ch) = (
        num(&dims, "deviceCellWidth"),
        num(&dims, "deviceCellHeight"),
    );
    assert_eq!(
        pixel(&canvas_of(&h), cw * 5.5, ch * 2.5),
        [0x30, 0x0A, 0x24]
    );
    h.remove();
}

#[wasm_bindgen_test]
fn replies_reach_the_page() {
    let h = host(400, 200);
    let mut term = WebTerm::new(h.clone(), options()).unwrap_or_else(|e| panic!("{e:?}"));
    term.write(b"\x1b[c");
    assert_eq!(term.take_replies(), b"\x1b[?1;2c".to_vec());
    assert!(term.take_replies().is_empty());
    term.write(b"\x1b]0;hola\x07\x07");
    assert_eq!(term.take_title().as_deref(), Some("hola"));
    assert!(term.take_bell());
    h.remove();
}
