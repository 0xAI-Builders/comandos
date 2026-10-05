//! Pruebas en navegador de la entrada, la selección y los enlaces de
//! `WebTerm` (A7/A8). No corren en host ni con un navegador local: las
//! ejecuta A12 en el Chrome del Mac (`xtask term-bench --wasm-tests`).
#![cfg(target_arch = "wasm32")]

use comandos_term_web::WebTerm;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{
    ClipboardEvent, ClipboardEventInit, CompositionEvent, CompositionEventInit, DataTransfer,
    Event, EventTarget, HtmlElement, InputEvent, InputEventInit, KeyboardEvent, KeyboardEventInit,
    MouseEvent, MouseEventInit, WheelEvent, WheelEventInit,
};

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window()
        .and_then(|w| w.document())
        .unwrap_or_else(|| panic!("sin document"))
}

/// Lo mínimo de `xterm.css` que necesita la barra de desplazamiento (la
/// página nueva trae su propio estilo; aquí no hay ninguno).
fn style_once() {
    let doc = document();
    if doc.get_element_by_id("web-input-css").is_some() {
        return;
    }
    let Ok(style) = doc.create_element("style") else {
        return;
    };
    style.set_id("web-input-css");
    style.set_text_content(Some(
        ".xterm{position:relative;height:100%}\
         .xterm .xterm-viewport{overflow-y:scroll;position:absolute;top:0;bottom:0;left:0;right:0}\
         .xterm .xterm-screen{position:relative}",
    ));
    let _ = doc.body().map(|b| b.append_child(&style));
}

fn host(w: u32, h: u32) -> HtmlElement {
    style_once();
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
    let o = js_sys::Object::new();
    let set = |k: &str, v: JsValue| {
        let _ = js_sys::Reflect::set(&o, &k.into(), &v);
    };
    set("fontFamily", "monospace".into());
    set("fontSize", 14.0.into());
    set("lineHeight", 1.2.into());
    set("scrollback", 1000.0.into());
    set("ligatures", false.into());
    o.into()
}

async fn next_frame() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let cb = Closure::once_into_js(move |_t: f64| {
            let _ = resolve.call0(&JsValue::NULL);
        });
        let _ = web_sys::window().map(|w| w.request_animation_frame(cb.unchecked_ref()));
    });
    let _ = JsFuture::from(promise).await;
}

/// Terminal montada y ajustada, con los bytes que manda acumulados.
fn term(h: &HtmlElement) -> (WebTerm, Rc<RefCell<Vec<Vec<u8>>>>) {
    let mut t = WebTerm::new(h.clone(), options()).unwrap_or_else(|e| panic!("{e:?}"));
    t.resize_to_fit();
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sink = sent.clone();
    let cb = Closure::<dyn FnMut(js_sys::Uint8Array)>::new(move |b: js_sys::Uint8Array| {
        sink.borrow_mut().push(b.to_vec());
    });
    t.set_on_data(Some(
        cb.as_ref().unchecked_ref::<js_sys::Function>().clone(),
    ));
    cb.forget();
    (t, sent)
}

fn take(sent: &Rc<RefCell<Vec<Vec<u8>>>>) -> Vec<u8> {
    sent.borrow_mut().drain(..).flatten().collect()
}

fn textarea(t: &WebTerm) -> EventTarget {
    t.textarea()
        .unwrap_or_else(|| panic!("sin textarea"))
        .into()
}

fn screen(t: &WebTerm) -> HtmlElement {
    t.screen().unwrap_or_else(|| panic!("sin screen"))
}

fn num(obj: &JsValue, key: &str) -> f64 {
    js_sys::Reflect::get(obj, &key.into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN)
}

fn key(target: &EventTarget, key: &str, key_code: u32, ctrl: bool) -> Event {
    let init = KeyboardEventInit::new();
    init.set_key(key);
    init.set_key_code(key_code);
    init.set_ctrl_key(ctrl);
    init.set_bubbles(true);
    init.set_cancelable(true);
    let e: Event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .into();
    let _ = target.dispatch_event(&e);
    e
}

/// Evento de ratón en la celda `(col, row)` (centro) de la terminal.
fn mouse(t: &WebTerm, target: &EventTarget, kind: &str, col: f64, row: f64, detail: i32) -> Event {
    let dims = t.dimensions();
    let rect = screen(t).get_bounding_client_rect();
    let (cw, ch) = (num(&dims, "cssCellWidth"), num(&dims, "cssCellHeight"));
    let init = MouseEventInit::new();
    init.set_client_x((rect.left() + (col + 0.5) * cw) as i32);
    init.set_client_y((rect.top() + (row + 0.5) * ch) as i32);
    init.set_button(0);
    init.set_buttons(if kind == "mouseup" { 0 } else { 1 });
    init.set_detail(detail);
    init.set_bubbles(true);
    init.set_cancelable(true);
    let e: Event = MouseEvent::new_with_mouse_event_init_dict(kind, &init)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .into();
    let _ = target.dispatch_event(&e);
    e
}

#[wasm_bindgen_test]
async fn keys_go_out_through_on_data_and_are_cancelled() {
    let h = host(400, 200);
    let (_t, sent) = term(&h);
    let ta = textarea(&_t);
    let e = key(&ta, "a", 65, false);
    assert!(e.default_prevented());
    let e = key(&ta, "ArrowUp", 38, false);
    assert!(e.default_prevented());
    let e = key(&ta, "c", 67, true);
    assert!(e.default_prevented());
    assert_eq!(take(&sent), b"a\x1b[A\x03");
    // Una tecla sin bytes no se toca.
    let e = key(&ta, "Shift", 16, false);
    assert!(!e.default_prevented());
    assert!(take(&sent).is_empty());
    h.remove();
}

#[wasm_bindgen_test]
async fn ime_text_is_sent_once_and_gboard_backspace_works() {
    let h = host(400, 200);
    let (t, sent) = term(&h);
    let ta = textarea(&t);
    let comp = |kind: &str, data: &str| {
        let init = CompositionEventInit::new();
        init.set_data(data);
        let e = CompositionEvent::new_with_event_init_dict(kind, &init)
            .unwrap_or_else(|e| panic!("{e:?}"));
        let _ = ta.dispatch_event(&e);
    };
    let input = |kind: &str, ty: &str, data: Option<&str>| -> Event {
        let init = InputEventInit::new();
        init.set_input_type(ty);
        if let Some(d) = data {
            init.set_data(Some(d));
        }
        init.set_cancelable(true);
        let e: Event = InputEvent::new_with_event_init_dict(kind, &init)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .into();
        let _ = ta.dispatch_event(&e);
        e
    };
    comp("compositionstart", "");
    comp("compositionupdate", "漢");
    comp("compositionend", "漢字");
    let _ = input("beforeinput", "insertText", Some("漢字"));
    let _ = input("input", "insertText", Some("漢字"));
    assert_eq!(take(&sent), "漢字".as_bytes());
    let e = input("beforeinput", "deleteContentBackward", None);
    assert!(e.default_prevented());
    assert_eq!(take(&sent), b"\x7f");
    h.remove();
}

#[wasm_bindgen_test]
async fn paste_uses_brackets_when_asked() {
    let h = host(400, 200);
    let (mut t, sent) = term(&h);
    t.write(b"\x1b[?2004h");
    let data = DataTransfer::new().unwrap_or_else(|e| panic!("{e:?}"));
    let _ = data.set_data("text/plain", "a\nb");
    let init = ClipboardEventInit::new();
    init.set_clipboard_data(Some(&data));
    init.set_cancelable(true);
    let e = ClipboardEvent::new_with_event_init_dict("paste", &init)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let _ = textarea(&t).dispatch_event(&e);
    assert!(e.default_prevented());
    assert_eq!(take(&sent), b"\x1b[200~a\rb\x1b[201~");
    t.paste("x");
    assert_eq!(take(&sent), b"\x1b[200~x\x1b[201~");
    h.remove();
}

#[wasm_bindgen_test]
async fn drag_selects_and_ctrl_c_copies_instead_of_sending() {
    let h = host(400, 200);
    let (mut t, sent) = term(&h);
    t.write(b"hola mundo\r\nadios");
    let changes = Rc::new(RefCell::new(0));
    let counter = changes.clone();
    let cb = Closure::<dyn FnMut()>::new(move || *counter.borrow_mut() += 1);
    t.set_on_selection_change(Some(
        cb.as_ref().unchecked_ref::<js_sys::Function>().clone(),
    ));
    cb.forget();
    let sc: EventTarget = screen(&t).into();
    let doc: EventTarget = document().into();
    let _ = mouse(&t, &sc, "mousedown", -0.5, 0.0, 1);
    let _ = mouse(&t, &doc, "mousemove", 3.5, 1.0, 1);
    let _ = mouse(&t, &doc, "mouseup", 3.5, 1.0, 1);
    assert!(t.has_selection());
    assert_eq!(t.get_selection(), "hola mundo\nadios");
    assert_eq!(*changes.borrow(), 1);
    let e = key(&textarea(&t), "c", 67, true);
    assert!(e.default_prevented());
    assert!(take(&sent).is_empty(), "Ctrl+C copia y no manda ^C");
    // Escribir quita la selección.
    let _ = key(&textarea(&t), "x", 88, false);
    assert!(!t.has_selection());
    // Doble clic: palabra.
    let _ = mouse(&t, &sc, "mousedown", 6.0, 0.0, 2);
    let _ = mouse(&t, &doc, "mouseup", 6.0, 0.0, 2);
    assert_eq!(t.get_selection(), "mundo");
    h.remove();
}

#[wasm_bindgen_test]
async fn mouse_reports_go_to_the_application_and_shift_selects() {
    let h = host(400, 200);
    let (mut t, sent) = term(&h);
    t.write(b"texto\x1b[?1002h\x1b[?1006h");
    let sc: EventTarget = screen(&t).into();
    let doc: EventTarget = document().into();
    let _ = mouse(&t, &sc, "mousedown", 2.0, 1.0, 1);
    let _ = mouse(&t, &doc, "mousemove", 3.0, 1.0, 1);
    let _ = mouse(&t, &doc, "mousemove", 3.0, 1.0, 1);
    let _ = mouse(&t, &doc, "mouseup", 3.0, 1.0, 1);
    assert_eq!(
        take(&sent),
        b"\x1b[<0;3;2M\x1b[<32;4;2M\x1b[<0;4;2m",
        "movimiento repetido descartado"
    );
    assert!(!t.has_selection());
    h.remove();
}

#[wasm_bindgen_test]
async fn wheel_scrolls_history_or_reports() {
    let h = host(400, 200);
    let (mut t, sent) = term(&h);
    for i in 0..100 {
        t.write(format!("linea {i}\r\n").as_bytes());
    }
    next_frame().await;
    let root: EventTarget = h
        .query_selector(".xterm")
        .ok()
        .flatten()
        .unwrap_or_else(|| panic!("sin .xterm"))
        .into();
    let wheel = |delta: f64| -> Event {
        let init = WheelEventInit::new();
        init.set_delta_y(delta);
        init.set_delta_mode(1);
        init.set_cancelable(true);
        init.set_bubbles(true);
        let e: Event = WheelEvent::new_with_event_init_dict("wheel", &init)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .into();
        let _ = root.dispatch_event(&e);
        e
    };
    let viewport: HtmlElement = h
        .query_selector(".xterm-viewport")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into().ok())
        .unwrap_or_else(|| panic!("sin viewport"));
    let bottom = viewport.scroll_top();
    let e = wheel(-3.0);
    assert!(e.default_prevented());
    next_frame().await;
    assert!(viewport.scroll_top() < bottom, "subió por la historia");
    assert!(take(&sent).is_empty());
    // Con ratón de la aplicación: un informe de rueda.
    t.write(b"\x1b[?1000h\x1b[?1006h");
    let _ = wheel(3.0);
    let out = take(&sent);
    assert!(out.starts_with(b"\x1b[<65;"), "{out:?}");
    h.remove();
}

#[wasm_bindgen_test]
async fn links_show_a_pointer_and_osc52_never_reaches_the_clipboard() {
    let h = host(600, 200);
    let (mut t, sent) = term(&h);
    t.write(b"ver https://ejemplo.mx/a ok\r\n\x1b]52;c;aG9sYQ==\x07");
    let sc: EventTarget = screen(&t).into();
    let _ = mouse(&t, &sc, "mousemove", 8.0, 0.0, 0);
    assert_eq!(
        screen(&t)
            .style()
            .get_property_value("cursor")
            .unwrap_or_default(),
        "pointer"
    );
    let _ = mouse(&t, &sc, "mousemove", 1.0, 1.0, 0);
    assert_eq!(
        screen(&t)
            .style()
            .get_property_value("cursor")
            .unwrap_or_default(),
        ""
    );
    // OSC 52 se queda en la terminal: la página decide (hoy lo ignora).
    assert_eq!(t.take_clipboard().as_deref(), Some("hola"));
    assert!(take(&sent).is_empty());
    h.remove();
}
