//! Actual Node-WASM collector refusal still leaves copy/title/bell unchanged.
#![cfg(target_arch = "wasm32")]
use comandos_term::engine::{Engine, GridSize, Palette};
use wasm_bindgen_test::wasm_bindgen_test;
#[wasm_bindgen_test]
fn clipboard_queries_remain_unanswered_and_following_effects_survive() {
    let mut engine = Engine::new(
        GridSize { cols: 4, rows: 1 },
        0,
        Palette::xterm_default([255; 3], [0; 3], [255; 3], [0; 3], [127; 3]),
    );
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
