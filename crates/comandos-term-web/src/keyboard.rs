//! Teclado, IME y teclados en pantalla: el `<textarea>` oculto de xterm.js
//! y lo que hace con sus eventos.
//!
//! - `keydown` → [`comandos_term::input::encode_key`] (salvo durante una
//!   composición o con keyCode 229); `preventDefault` solo con bytes o
//!   desplazamiento, para que el navegador no repita la tecla por `input`.
//! - Composición (IME) y `beforeinput`/`input` (GBoard, que no manda
//!   `keydown`) pasan por [`comandos_term::input::Ime`]: el texto compuesto
//!   sale una sola vez.
//! - Tecla muerta: como `_unprocessedDeadKey` de xterm.js, la tecla
//!   siguiente no se codifica y el carácter compuesto llega por `input`.
//! - Ctrl/Meta+C (sin Alt) con selección copia en vez de mandar `^C`
//!   (`handleTerminalKeyEvent` de `dash/term.html`).
//!
//! La decisión es pura ([`Keys`], probada en host); el cableado con el DOM
//! está en `lib.rs`.
use comandos_term::{
    engine::Modes,
    input::{Ime, KeyAction, KeyInput, encode_key},
};

/// Lo que importa de un `KeyboardEvent`.
#[derive(Debug, Clone, Copy)]
pub struct KeyEvent<'a> {
    pub key: &'a str,
    pub code: &'a str,
    pub key_code: u32,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub is_composing: bool,
}

/// Qué hacer con un `keydown`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOutcome {
    /// No tocar el evento (el navegador o el IME se encargan).
    Ignore,
    /// Copiar la selección; `preventDefault`.
    Copy,
    /// Bytes para el PTY; `preventDefault`. Con Esc o Enter el `<textarea>`
    /// se vacía (como xterm.js).
    Send {
        bytes: Vec<u8>,
        clear_textarea: bool,
    },
    /// Desplazar la vista una página (−1 arriba); `preventDefault`.
    ScrollPage(i32),
}

/// Estado del teclado de una terminal.
#[derive(Debug, Clone, Default)]
pub struct Keys {
    ime: Ime,
    /// Se pulsó una tecla muerta: la próxima tecla con bytes no se codifica.
    dead_pending: bool,
    /// Llegó `beforeinput` para la edición en curso: su `input` ya está
    /// resuelto (si no, el eco del IME se mandaría dos veces).
    saw_before: bool,
}

impl Keys {
    /// Un `keydown`. `has_selection` decide si Ctrl+C copia.
    pub fn keydown(&mut self, e: &KeyEvent, m: &Modes, has_selection: bool) -> KeyOutcome {
        // `attachCustomKeyEventHandler` de `term.html` va antes que nada.
        let copy = !e.alt && (e.ctrl || e.meta) && e.key.eq_ignore_ascii_case("c");
        if copy && has_selection {
            return KeyOutcome::Copy;
        }
        if e.is_composing || self.ime.keydown_ignored(e.key_code) {
            return KeyOutcome::Ignore;
        }
        if e.key == "Dead" || e.key == "AltGraph" {
            self.dead_pending = true;
        }
        let input = KeyInput {
            key: e.key,
            code: e.code,
            key_code: e.key_code,
            ctrl: e.ctrl,
            alt: e.alt,
            shift: e.shift,
            meta: e.meta,
        };
        match encode_key(&input, m) {
            KeyAction::None => KeyOutcome::Ignore,
            KeyAction::ScrollPage(p) => KeyOutcome::ScrollPage(p),
            KeyAction::Send(bytes) => {
                if self.dead_pending {
                    // Tecla tras una muerta: el navegador compone el
                    // carácter y llega por `input`.
                    self.dead_pending = false;
                    return KeyOutcome::Ignore;
                }
                self.ime.forget_commit();
                let clear_textarea = bytes == [0x1b] || bytes == [b'\r'];
                KeyOutcome::Send {
                    bytes,
                    clear_textarea,
                }
            }
        }
    }

    pub fn composition_start(&mut self) {
        self.ime.start();
    }

    /// `compositionend`: el texto compuesto, una vez.
    pub fn composition_end(&mut self, data: &str) -> Option<Vec<u8>> {
        self.dead_pending = false;
        self.ime.end(data)
    }

    /// `beforeinput`: bytes si hay que mandarlos (y `preventDefault`).
    pub fn before_input(
        &mut self,
        input_type: &str,
        data: Option<&str>,
        is_composing: bool,
    ) -> Option<Vec<u8>> {
        self.saw_before = true;
        let out = self.ime.input(input_type, data, is_composing);
        if out.is_some() {
            self.dead_pending = false;
        }
        out
    }

    /// `input`: solo cuenta si no hubo `beforeinput` para esta edición
    /// (navegadores sin `beforeinput`).
    pub fn input(
        &mut self,
        input_type: &str,
        data: Option<&str>,
        is_composing: bool,
    ) -> Option<Vec<u8>> {
        if std::mem::take(&mut self.saw_before) {
            return None;
        }
        let out = self.ime.input(input_type, data, is_composing);
        if out.is_some() {
            self.dead_pending = false;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: &'static str, key_code: u32) -> KeyEvent<'static> {
        KeyEvent {
            key,
            code: "",
            key_code,
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
            is_composing: false,
        }
    }

    fn send(bytes: &[u8]) -> KeyOutcome {
        KeyOutcome::Send {
            bytes: bytes.to_vec(),
            clear_textarea: false,
        }
    }

    #[test]
    fn keys_are_encoded_and_enter_clears_the_textarea() {
        let mut k = Keys::default();
        let m = Modes::default();
        assert_eq!(k.keydown(&key("a", 65), &m, false), send(b"a"));
        assert_eq!(
            k.keydown(&key("Enter", 13), &m, false),
            KeyOutcome::Send {
                bytes: b"\r".to_vec(),
                clear_textarea: true
            }
        );
        assert_eq!(
            k.keydown(&key("Escape", 27), &m, false),
            KeyOutcome::Send {
                bytes: b"\x1b".to_vec(),
                clear_textarea: true
            }
        );
        assert_eq!(k.keydown(&key("Shift", 16), &m, false), KeyOutcome::Ignore);
        let page_up = KeyEvent {
            shift: true,
            ..key("PageUp", 33)
        };
        assert_eq!(k.keydown(&page_up, &m, false), KeyOutcome::ScrollPage(-1));
    }

    #[test]
    fn ctrl_c_copies_only_with_a_selection() {
        let mut k = Keys::default();
        let m = Modes::default();
        let ctrl_c = KeyEvent {
            ctrl: true,
            ..key("c", 67)
        };
        assert_eq!(k.keydown(&ctrl_c, &m, false), send(b"\x03"));
        assert_eq!(k.keydown(&ctrl_c, &m, true), KeyOutcome::Copy);
        let ctrl_shift_c = KeyEvent {
            shift: true,
            ..KeyEvent { key: "C", ..ctrl_c }
        };
        assert_eq!(k.keydown(&ctrl_shift_c, &m, true), KeyOutcome::Copy);
        let meta_c = KeyEvent {
            meta: true,
            ..key("c", 67)
        };
        assert_eq!(k.keydown(&meta_c, &m, true), KeyOutcome::Copy);
        let alt_ctrl_c = KeyEvent {
            alt: true,
            ..ctrl_c
        };
        assert_ne!(k.keydown(&alt_ctrl_c, &m, true), KeyOutcome::Copy);
    }

    #[test]
    fn composition_is_left_to_the_ime_and_sent_once() {
        let mut k = Keys::default();
        let m = Modes::default();
        k.composition_start();
        assert_eq!(
            k.keydown(&key("Process", 229), &m, false),
            KeyOutcome::Ignore
        );
        let composing = KeyEvent {
            is_composing: true,
            ..key("a", 65)
        };
        assert_eq!(k.keydown(&composing, &m, false), KeyOutcome::Ignore);
        assert_eq!(
            k.before_input("insertCompositionText", Some("漢"), true),
            None
        );
        assert_eq!(k.input("insertCompositionText", Some("漢"), true), None);
        assert_eq!(k.composition_end("漢字"), Some("漢字".as_bytes().to_vec()));
        // Eco de Chrome: `beforeinput` + `input` con el mismo texto.
        assert_eq!(k.before_input("insertText", Some("漢字"), false), None);
        assert_eq!(
            k.input("insertText", Some("漢字"), false),
            None,
            "nunca dos veces"
        );
        assert_eq!(
            k.before_input("insertText", Some("x"), false),
            Some(b"x".to_vec())
        );
    }

    #[test]
    fn gboard_edits_arrive_by_beforeinput_or_input() {
        let mut k = Keys::default();
        assert_eq!(
            k.before_input("deleteContentBackward", None, false),
            Some(vec![0x7f])
        );
        assert_eq!(
            k.input("deleteContentBackward", None, false),
            None,
            "ya resuelto"
        );
        // Sin `beforeinput` (navegador viejo), `input` manda.
        assert_eq!(
            k.input("insertText", Some("ñ"), false),
            Some("ñ".as_bytes().to_vec())
        );
    }

    #[test]
    fn a_sent_key_forgets_the_commit_echo() {
        let mut k = Keys::default();
        let m = Modes::default();
        k.composition_start();
        assert!(k.composition_end("a").is_some());
        assert_eq!(k.keydown(&key("b", 66), &m, false), send(b"b"));
        // El usuario escribe «a» por un teclado en pantalla: no es eco.
        assert_eq!(
            k.before_input("insertText", Some("a"), false),
            Some(b"a".to_vec())
        );
    }

    #[test]
    fn the_key_after_a_dead_key_is_composed_by_the_browser() {
        let mut k = Keys::default();
        let m = Modes::default();
        assert_eq!(k.keydown(&key("Dead", 222), &m, false), KeyOutcome::Ignore);
        assert_eq!(k.keydown(&key("é", 69), &m, false), KeyOutcome::Ignore);
        assert_eq!(
            k.before_input("insertText", Some("é"), false),
            Some("é".as_bytes().to_vec())
        );
        assert_eq!(
            k.keydown(&key("e", 69), &m, false),
            send(b"e"),
            "solo una tecla"
        );
    }
}

// --- cableado con el DOM -----------------------------------------------------

mod web {
    use super::KeyOutcome;
    use crate::{Inner, set_styles};
    use comandos_term::input::encode_paste;
    use wasm_bindgen::JsCast;
    use web_sys::{ClipboardEvent, CompositionEvent, Event, InputEvent, KeyboardEvent};

    /// `cancel(e, true)` de xterm.js.
    fn cancel(e: &Event) {
        e.prevent_default();
        e.stop_propagation();
    }

    impl Inner {
        pub(crate) fn on_keydown(&mut self, e: &Event) {
            let Some(ke) = e.dyn_ref::<KeyboardEvent>() else {
                return;
            };
            let (key, code) = (ke.key(), ke.code());
            let ev = super::KeyEvent {
                key: &key,
                code: &code,
                key_code: ke.key_code(),
                ctrl: ke.ctrl_key(),
                alt: ke.alt_key(),
                shift: ke.shift_key(),
                meta: ke.meta_key(),
                is_composing: ke.is_composing(),
            };
            let modes = self.engine.modes();
            let has_selection = self.has_selection();
            match self.keys.keydown(&ev, &modes, has_selection) {
                KeyOutcome::Ignore => {}
                KeyOutcome::Copy => {
                    cancel(e);
                    self.outbox.copy = Some(self.selection_text());
                }
                KeyOutcome::Send {
                    bytes,
                    clear_textarea,
                } => {
                    cancel(e);
                    if clear_textarea {
                        self.dom.textarea.set_value("");
                    }
                    self.send(bytes, true);
                }
                KeyOutcome::ScrollPage(pages) => {
                    cancel(e);
                    // Una página = filas − 1 (`scrollLines(±(rows − 1))`).
                    let page = i32::from(self.size.rows).saturating_sub(1);
                    self.scroll_lines(pages.saturating_mul(page).saturating_neg());
                }
            }
        }

        pub(crate) fn on_composition_start(&mut self, _e: &Event) {
            self.keys.composition_start();
            self.composing = true;
            self.dom.composition.set_text_content(Some(""));
            set_styles(&self.dom.composition, &[("display", "block")]);
            self.place_composition();
        }

        pub(crate) fn on_composition_update(&mut self, e: &Event) {
            let data = e
                .dyn_ref::<CompositionEvent>()
                .and_then(CompositionEvent::data)
                .unwrap_or_default();
            self.dom.composition.set_text_content(Some(&data));
            self.place_composition();
        }

        pub(crate) fn on_composition_end(&mut self, e: &Event) {
            self.composing = false;
            set_styles(&self.dom.composition, &[("display", "none")]);
            self.textarea_at = None;
            let data = e
                .dyn_ref::<CompositionEvent>()
                .and_then(CompositionEvent::data)
                .unwrap_or_default();
            if let Some(bytes) = self.keys.composition_end(&data) {
                self.send(bytes, true);
            }
            self.request_frame();
        }

        /// `updateCompositionElements`: la vista de composición y el
        /// `<textarea>` sobre la celda del cursor.
        fn place_composition(&mut self) {
            let c = comandos_term::render::cursor(&self.engine);
            if c.line >= usize::from(self.size.rows) {
                return;
            }
            let Some((cw, ch)) = self.metrics.css_cell(self.size.cols, self.size.rows) else {
                return;
            };
            let col = c.col.min(self.size.cols.saturating_sub(1));
            let px = |v: f64| format!("{v}px");
            let (left, top) = (px(f64::from(col) * cw), px(c.line as f64 * ch));
            let font = px(self.opts.font_size);
            set_styles(
                &self.dom.composition,
                &[
                    ("left", &left),
                    ("top", &top),
                    ("height", &px(ch)),
                    ("line-height", &px(ch)),
                    ("font-family", &self.opts.font_family),
                    ("font-size", &font),
                ],
            );
            let rect = self.dom.composition.get_bounding_client_rect();
            set_styles(
                &self.dom.textarea,
                &[
                    ("left", &left),
                    ("top", &top),
                    ("width", &px(rect.width().max(1.0))),
                    ("height", &px(rect.height().max(1.0))),
                    ("line-height", &px(rect.height())),
                ],
            );
        }

        pub(crate) fn on_before_input(&mut self, e: &Event) {
            let Some(ie) = e.dyn_ref::<InputEvent>() else {
                return;
            };
            let data = ie.data();
            let bytes =
                self.keys
                    .before_input(&ie.input_type(), data.as_deref(), ie.is_composing());
            if let Some(bytes) = bytes {
                e.prevent_default();
                self.send(bytes, true);
            }
        }

        pub(crate) fn on_input(&mut self, e: &Event) {
            let Some(ie) = e.dyn_ref::<InputEvent>() else {
                return;
            };
            let data = ie.data();
            if let Some(bytes) =
                self.keys
                    .input(&ie.input_type(), data.as_deref(), ie.is_composing())
            {
                self.send(bytes, true);
            }
        }

        /// `handlePasteEvent`: el texto plano del portapapeles.
        pub(crate) fn on_paste(&mut self, e: &Event) {
            let Some(ce) = e.dyn_ref::<ClipboardEvent>() else {
                return;
            };
            e.stop_propagation();
            e.prevent_default();
            let text = ce
                .clipboard_data()
                .and_then(|d| d.get_data("text/plain").ok())
                .unwrap_or_default();
            self.paste(&text);
        }

        /// `paste()` de xterm.js: saltos de línea a `\r` y corchetes de
        /// pegado si la aplicación los pidió.
        pub(crate) fn paste(&mut self, text: &str) {
            let bytes = encode_paste(text, &self.engine.modes());
            self.dom.textarea.set_value("");
            self.send(bytes, true);
        }

        pub(crate) fn on_focus(&mut self, _e: &Event) {
            self.set_focused(true);
        }

        pub(crate) fn on_blur(&mut self, _e: &Event) {
            self.dom.textarea.set_value("");
            self.set_focused(false);
        }

        /// `copyHandler`: «Copiar» del navegador con una selección.
        pub(crate) fn on_copy(&mut self, e: &Event) {
            if !self.has_selection() {
                return;
            }
            let Some(data) = e
                .dyn_ref::<ClipboardEvent>()
                .and_then(ClipboardEvent::clipboard_data)
            else {
                return;
            };
            let _ = data.set_data("text/plain", &self.selection_text());
            e.prevent_default();
        }
    }
}
