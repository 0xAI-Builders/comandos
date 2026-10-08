//! Existing event consumers retain OSC52 callbacks; Collector keeps its refusal.
use alacritty_terminal::{
    event::{Event, EventListener},
    term::{ClipboardType, Config, Osc52, Term},
    vte::ansi::Handler,
};
use comandos_term::engine::{Collector, Engine, GridSize, Palette};
use std::{cell::RefCell, rc::Rc};
#[derive(Clone, Default)]
struct Listener(Rc<RefCell<Vec<Event>>>);
impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        self.0.borrow_mut().push(event);
    }
}
#[derive(Clone, Default)]
struct Declining(Listener);
impl EventListener for Declining {
    fn accepts_clipboard_load(&self) -> bool {
        false
    }
    fn send_event(&self, event: Event) {
        self.0.send_event(event);
    }
}
fn palette() -> Palette {
    Palette::xterm_default([255; 3], [0; 3], [255; 3], [0; 3], [127; 3])
}
#[test]
fn inherited_admission_keeps_original_clipboard_load_targets_and_reply_bytes() {
    let listener = Listener::default();
    assert!(listener.accepts_clipboard_load());
    let mut term = Term::new(
        Config {
            osc52: Osc52::CopyPaste,
            ..Config::default()
        },
        &GridSize { cols: 2, rows: 1 },
        listener.clone(),
    );
    for clipboard in [b'c', b'p', b's'] {
        for terminator in ["\x07", "\x1b\\"] {
            term.clipboard_load(clipboard, terminator);
            let event = listener.0.borrow_mut().pop().unwrap();
            match event {
                Event::ClipboardLoad(target, reply) => {
                    assert_eq!(
                        target,
                        if clipboard == b'c' {
                            ClipboardType::Clipboard
                        } else {
                            ClipboardType::Selection
                        }
                    );
                    for (text, base64) in [
                        ("", ""),
                        ("hola", "aG9sYQ=="),
                        ("a\0b", "YQBi"),
                        ("😀", "8J+YgA=="),
                        ("a", "YQ=="),
                        ("ab", "YWI="),
                    ] {
                        assert_eq!(
                            reply(text),
                            format!("\x1b]52;{};{}{}", char::from(clipboard), base64, terminator)
                        );
                    }
                }
                _ => panic!("original ClipboardLoad event missing"),
            }
        }
    }
    term.clipboard_load(b'x', "\x07");
    assert!(listener.0.borrow().is_empty());
}
#[test]
fn declined_load_has_no_event_and_preserves_standard_store_decoder() {
    let listener = Declining::default();
    let mut term = Term::new(
        Config {
            osc52: Osc52::CopyPaste,
            ..Config::default()
        },
        &GridSize { cols: 2, rows: 1 },
        listener.clone(),
    );
    term.clipboard_load(b'c', "\x07");
    assert!(listener.0.0.borrow().is_empty());
    term.clipboard_store(b'c', b"8J+YgA==");
    assert!(
        matches!(listener.0.0.borrow_mut().pop(),Some(Event::ClipboardStore(ClipboardType::Clipboard,text))if text=="😀")
    );
    for invalid in [
        b"YQ".as_slice(),
        b"YR==",
        b"YQ===",
        b"Y Q=",
        b"_w==",
        b"/w==",
    ] {
        term.clipboard_store(b'c', invalid);
        assert!(listener.0.0.borrow().is_empty());
    }
}
#[test]
fn collector_keeps_documented_refusal_and_ordinary_terminal_effects() {
    assert!(!Collector::default().accepts_clipboard_load());
    let mut engine = Engine::new(GridSize { cols: 4, rows: 1 }, 0, palette());
    engine.advance(b"\x1b]52;c;?\x07\x1b]52;p;?\x1b\\\x1b]52;s;?\x07", 0.);
    let effects = engine.drain();
    assert!(effects.replies.is_empty());
    assert_eq!(effects.clipboard, None);
    assert_eq!(effects.clipboard_target, None);
    engine.advance(b"\x1b]52;c;aG9sYQ==\x07\x1b]0;private\x07\x07X", 0.);
    let effects = engine.drain();
    assert_eq!(effects.clipboard.as_deref(), Some("hola"));
    assert_eq!(effects.title.as_deref(), Some("private"));
    assert!(effects.bell);
    assert!(effects.replies.is_empty());
}
