use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line, Point},
    term::cell::Flags,
    vte::ansi::{Color, Rgb},
};
use comandos_term::engine::{DA1_REPLY, DA2_REPLY, Damage, Engine, GridSize, MouseMode, Palette};

fn engine(cols: u16, rows: u16) -> Engine {
    let p = Palette::xterm_default(
        [0xEA, 0xF0, 0xFB],
        [0x0A, 0x0D, 0x13],
        [0xFF, 0xAE, 0x1A],
        [0x0A, 0x0D, 0x13],
        [0x2E, 0x38, 0x52],
    );
    Engine::new(GridSize { cols, rows }, 10_000, p)
}

fn cell(e: &Engine, line: i32, col: usize) -> alacritty_terminal::term::cell::Cell {
    e.term().grid()[Point::new(Line(line), Column(col))].clone()
}

#[test]
fn truecolor_bold_text_lands_in_the_grid() {
    let mut e = engine(20, 3);
    e.advance(b"\x1b[1;38;2;255;0;0mA", 0.0);
    let c = cell(&e, 0, 0);
    assert_eq!(c.c, 'A');
    assert_eq!(c.fg, Color::Spec(Rgb { r: 255, g: 0, b: 0 }));
    assert!(c.flags.contains(Flags::BOLD));
}

#[test]
fn cjk_takes_two_cells() {
    let mut e = engine(20, 3);
    e.advance("漢x".as_bytes(), 0.0);
    assert!(cell(&e, 0, 0).flags.contains(Flags::WIDE_CHAR));
    assert!(cell(&e, 0, 1).flags.contains(Flags::WIDE_CHAR_SPACER));
    assert_eq!(cell(&e, 0, 2).c, 'x');
}

#[test]
fn da1_and_da2_replies_match_xterm_js() {
    // Review Focus 5: tmux 3.2a + config/terminal-replies.conf esperan la DA2 de xterm.js.
    let mut e = engine(20, 3);
    e.advance(b"\x1b[c\x1b[>c", 0.0);
    let mut want = DA1_REPLY.to_vec();
    want.extend_from_slice(DA2_REPLY);
    assert_eq!(e.drain().replies, want);
    assert_eq!(DA2_REPLY, b"\x1b[>0;276;0c");
}

#[test]
fn osc_title_and_osc52_are_events() {
    let mut e = engine(20, 3);
    e.advance(b"\x1b]0;hola\x07\x1b]52;c;aG9sYQ==\x07", 0.0);
    let d = e.drain();
    assert_eq!(d.title.as_deref(), Some("hola"));
    assert_eq!(d.clipboard.as_deref(), Some("hola"));
}

#[test]
fn synchronized_update_waits_for_end_or_deadline() {
    // Claude Code emite CSI ? 2026 h; sin reloj inyectado, vte llamaría a Instant::now (pánico en wasm).
    let mut e = engine(20, 3);
    let _ = e.take_damage();
    e.advance(b"\x1b[?2026hX", 0.0);
    assert_eq!(
        cell(&e, 0, 0).c,
        ' ',
        "retenido durante la actualización sincronizada"
    );
    assert!(e.next_deadline_ms().is_some());
    assert!(!e.tick(50.0));
    assert!(e.tick(1_000.0), "el plazo vencido vuelca lo retenido");
    assert_eq!(cell(&e, 0, 0).c, 'X');
    e.advance(b"\x1b[?2026hY\x1b[?2026l", 2_000.0);
    assert_eq!(cell(&e, 0, 1).c, 'Y', "el fin explícito vuelca sin esperar");
}

#[test]
fn scrollback_is_bounded_to_ten_thousand_lines() {
    let mut e = engine(10, 5);
    let lines: String = (0..10_100).map(|i| format!("{i}\r\n")).collect();
    e.advance(lines.as_bytes(), 0.0);
    assert_eq!(e.history_len(), 10_000);
}

#[test]
fn modes_follow_dec_private_sequences() {
    let mut e = engine(10, 5);
    e.advance(
        b"\x1b[?1h\x1b[?2004h\x1b[?1002h\x1b[?1006h\x1b[?1049h\x1b[?1004h",
        0.0,
    );
    let m = e.modes();
    assert!(m.app_cursor && m.bracketed_paste && m.sgr_mouse && m.alt_screen && m.focus_events);
    assert_eq!(m.mouse, MouseMode::Drag);
}

#[test]
fn damage_reports_only_touched_lines() {
    // Mover el cursor daña también la línea que deja (hay que borrar su dibujo):
    // se lleva primero a la fila 3 y se descarta ese daño antes de escribir.
    let mut e = engine(10, 5);
    e.advance(b"\x1b[3;1H", 0.0);
    let _ = e.take_damage();
    e.advance(b"z", 0.0);
    match e.take_damage() {
        Damage::Lines(lines) => assert!(lines.contains(&2) && !lines.contains(&0), "{lines:?}"),
        Damage::Full => panic!("se esperaba daño parcial"),
    }
}

#[test]
fn expired_sync_flushes_before_new_bytes() {
    // Sin `tick` entre medias, lo retenido se vuelca antes que lo nuevo.
    let mut e = engine(20, 3);
    e.advance(b"\x1b[?2026hA", 0.0);
    e.advance(b"B", 1_000.0);
    assert_eq!(cell(&e, 0, 0).c, 'A');
    assert_eq!(cell(&e, 0, 1).c, 'B');
    assert!(e.next_deadline_ms().is_none());
}

#[test]
fn palette_matches_xterm_js() {
    let e = Palette::xterm_default([1, 2, 3], [4, 5, 6], [7, 8, 9], [0, 0, 0], [0, 0, 0]);
    assert_eq!(e.ansi[1], [0xCC, 0x00, 0x00]);
    assert_eq!(e.ansi[15], [0xEE, 0xEE, 0xEC]);
    assert_eq!(e.ansi[16], [0, 0, 0]);
    assert_eq!(e.ansi[196], [255, 0, 0]);
    assert_eq!(e.ansi[231], [255, 255, 255]);
    assert_eq!(e.ansi[232], [8, 8, 8]);
    assert_eq!(e.ansi[255], [238, 238, 238]);
    assert_eq!(e.color_for_request(256), [1, 2, 3]);
    assert_eq!(e.color_for_request(257), [4, 5, 6]);
    assert_eq!(e.color_for_request(258), [7, 8, 9]);
    assert_eq!(e.color_for_request(9_999), [1, 2, 3]);
}

#[test]
fn color_and_size_queries_answer_from_the_theme() {
    let mut e = engine(20, 3);
    e.resize(GridSize { cols: 30, rows: 4 }, (7, 15));
    e.advance(b"\x1b]10;?\x07\x1b[14t", 0.0);
    let replies = String::from_utf8(e.drain().replies).unwrap();
    assert!(replies.contains("rgb:eaea/f0f0/fbfb"), "{replies:?}");
    assert!(replies.contains("\x1b[4;60;210t"), "{replies:?}");
}

#[test]
fn zero_size_is_clamped_instead_of_panicking() {
    let mut e = engine(0, 0);
    e.advance(b"abc\r\nd", 0.0);
    assert_eq!(e.term().grid().columns(), 2);
    e.resize(GridSize { cols: 0, rows: 0 }, (8, 16));
    assert_eq!(e.term().grid().screen_lines(), 1);
}
