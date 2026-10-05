use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line, Point},
    term::cell::Flags,
    vte::ansi::{Color, Rgb},
};
use comandos_term::engine::{
    ClipboardTarget, DA1_REPLY, DA2_REPLY, Damage, Engine, GridSize, MAX_CLIPBOARD_BYTES,
    MAX_REPLY_BYTES, MAX_TITLE_BYTES, MouseMode, Palette,
};

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

#[test]
fn flood_of_cheap_events_never_loses_a_da_reply() {
    // Campanas, títulos y cambios de parpadeo (que `drain` descarta) no ocupan
    // cola: la DA del final llega aunque vengan cientos de miles antes.
    let mut e = engine(20, 3);
    let mut flood = Vec::new();
    for i in 0..20_000 {
        flood.extend_from_slice(b"\x07\x1b[?12h\x1b[?12l");
        flood.extend_from_slice(format!("\x1b]0;t{i}\x07").as_bytes());
    }
    flood.extend_from_slice(b"\x1b[c");
    e.advance(&flood, 0.0);
    let d = e.drain();
    assert_eq!(d.replies, DA1_REPLY);
    assert!(d.bell);
    assert_eq!(d.title.as_deref(), Some("t19999"));
}

#[test]
fn replies_are_bounded_by_bytes_and_keep_the_first_ones() {
    let mut e = engine(20, 3);
    e.advance(&b"\x1b[c".repeat(20_000), 0.0);
    let replies = e.drain().replies;
    assert!(replies.len() <= MAX_REPLY_BYTES, "{}", replies.len());
    assert!(replies.starts_with(DA1_REPLY));
    assert_eq!(
        replies.len() % DA1_REPLY.len(),
        0,
        "no se parte ninguna respuesta"
    );
    // Tras el `drain` vuelve a haber sitio.
    e.advance(b"\x1b[>c", 0.0);
    assert_eq!(e.drain().replies, DA2_REPLY);
}

#[test]
fn long_titles_are_truncated_on_a_char_boundary() {
    let mut e = engine(20, 3);
    let title = "ñ".repeat(5_000);
    e.advance(format!("\x1b]2;{title}\x07").as_bytes(), 0.0);
    let got = e.drain().title.unwrap();
    assert!(got.len() <= MAX_TITLE_BYTES && got.len() > MAX_TITLE_BYTES - 2);
    assert!(got.chars().all(|c| c == 'ñ'));
}

#[test]
fn huge_osc52_is_dropped_whole_and_following_text_survives() {
    // 2 MiB de base64 en trozos de 64 KiB: el motor no lo retiene entero ni
    // copia un trozo; el texto posterior se pinta normal.
    let mut e = engine(20, 3);
    e.advance(b"\x1b]52;c;", 0.0);
    let chunk = b"QUFB".repeat(16 * 1024);
    for _ in 0..32 {
        e.advance(&chunk, 0.0);
    }
    e.advance(b"\x07ok", 0.0);
    let d = e.drain();
    assert_eq!(d.clipboard, None);
    assert_eq!(cell(&e, 0, 0).c, 'o');
    assert_eq!(cell(&e, 0, 1).c, 'k');
}

#[test]
fn osc52_just_over_the_text_cap_is_dropped() {
    let mut e = engine(20, 3);
    let text = vec![b'a'; MAX_CLIPBOARD_BYTES + 3];
    let b64 = base64_encode(&text);
    e.advance(format!("\x1b]52;c;{b64}\x07").as_bytes(), 0.0);
    assert_eq!(e.drain().clipboard, None);
    let ok = base64_encode(&vec![b'a'; MAX_CLIPBOARD_BYTES]);
    e.advance(format!("\x1b]52;c;{ok}\x07").as_bytes(), 0.0);
    assert_eq!(
        e.drain().clipboard.map(|c| c.len()),
        Some(MAX_CLIPBOARD_BYTES)
    );
}

/// base64 estándar mínimo para las pruebas (sin dependencias nuevas).
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn color_query_answers_osc_overrides_before_the_palette() {
    let mut e = engine(20, 3);
    e.advance(b"\x1b]10;rgb:12/34/56\x07\x1b]4;1;rgb:ab/cd/ef\x07", 0.0);
    e.advance(b"\x1b]10;?\x07\x1b]4;1;?\x07\x1b]11;?\x07", 0.0);
    let replies = String::from_utf8(e.drain().replies).unwrap();
    assert!(replies.contains("rgb:1212/3434/5656"), "{replies:?}");
    assert!(replies.contains("rgb:abab/cdcd/efef"), "{replies:?}");
    assert!(replies.contains("rgb:0a0a/0d0d/1313"), "{replies:?}");
}

#[test]
fn osc52_keeps_the_clipboard_target() {
    let mut e = engine(20, 3);
    e.advance(b"\x1b]52;p;aG9sYQ==\x07", 0.0);
    let d = e.drain();
    assert_eq!(d.clipboard.as_deref(), Some("hola"));
    assert_eq!(d.clipboard_target, Some(ClipboardTarget::Selection));
    e.advance(b"\x1b]52;c;aG9sYQ==\x07", 0.0);
    assert_eq!(e.drain().clipboard_target, Some(ClipboardTarget::Clipboard));
    assert_eq!(e.drain().clipboard_target, None);
}

#[test]
fn text_area_pixels_do_not_overflow_u16() {
    let mut e = engine(20, 3);
    e.resize(
        GridSize {
            cols: 1_000,
            rows: 500,
        },
        (200, 300),
    );
    e.advance(b"\x1b[14t", 0.0);
    assert_eq!(e.drain().replies, b"\x1b[4;150000;200000t");
}

#[test]
fn non_finite_clock_is_ignored() {
    let mut e = engine(20, 3);
    e.advance(b"\x1b[?2026hX", 0.0);
    assert!(!e.tick(f64::NAN));
    assert!(!e.tick(f64::INFINITY), "∞ no vence el plazo");
    e.advance(b"Y", f64::NEG_INFINITY);
    assert_eq!(cell(&e, 0, 0).c, ' ', "sigue retenido");
    assert!(e.next_deadline_ms().is_some_and(f64::is_finite));
    assert!(e.tick(1_000.0));
    assert_eq!(cell(&e, 0, 0).c, 'X');
    assert_eq!(cell(&e, 0, 1).c, 'Y');
}

#[test]
fn decrqm_reports_sync_update_as_supported() {
    // Superconjunto deliberado de xterm.js 5.5.0, que contesta `;0$y` (desconocido).
    let mut e = engine(20, 3);
    e.advance(b"\x1b[?2026$p", 0.0);
    assert_eq!(e.drain().replies, b"\x1b[?2026;2$y");
}

#[test]
fn default_cursor_blink_follows_the_page_option_until_decscusr() {
    let p = Palette::xterm_default([255; 3], [0; 3], [255; 3], [0; 3], [9; 3]);
    let mut e = Engine::with_cursor_blink(GridSize { cols: 10, rows: 2 }, 100, p, true);
    assert!(e.term().cursor_style().blinking);
    // DECSCUSR 2 = bloque fijo, como `setCursorStyle` de xterm.js.
    e.advance(b"\x1b[2 q", 0.0);
    assert!(!e.term().cursor_style().blinking);
    e.advance(b"\x1b[5 q", 0.0);
    assert!(e.term().cursor_style().blinking);
    assert!(!engine(10, 2).term().cursor_style().blinking);
}

#[test]
fn set_palette_changes_color_replies() {
    let mut e = engine(10, 2);
    let mut p = Palette::xterm_default([1, 2, 3], [4, 5, 6], [7, 8, 9], [0; 3], [0; 3]);
    p.ansi[1] = [0xAB, 0xCD, 0xEF];
    e.set_palette(p);
    e.advance(b"\x1b]11;?\x07\x1b]4;1;?\x07", 0.0);
    let replies = String::from_utf8(e.drain().replies).unwrap_or_default();
    assert!(replies.contains("rgb:0404/0505/0606"), "{replies}");
    assert!(replies.contains("rgb:abab/cdcd/efef"), "{replies}");
}

#[test]
fn take_damage_into_reuses_the_buffer_and_matches_take_damage() {
    let mut e = engine(10, 3);
    let mut lines = vec![99, 98];
    assert!(
        e.take_damage_into(&mut lines),
        "un motor nuevo está todo dañado"
    );
    assert!(lines.is_empty());
    e.advance(b"\x1b[3;1Hx", 0.0);
    assert!(!e.take_damage_into(&mut lines));
    // La línea escrita y, como siempre en alacritty, las del cursor.
    assert!(lines.contains(&2), "{lines:?}");
    e.advance(b"\x1b[2;1Hy", 0.0);
    assert!(matches!(e.take_damage(), Damage::Lines(l) if l.contains(&1)));
}

/// Las líneas que suben a la historia desplazan las filas absolutas (lo usa
/// la selección web para seguir a su texto); la pantalla alternativa y una
/// región que no empieza arriba no cuentan.
#[test]
fn scrolled_up_counts_lines_pushed_into_history() {
    let mut e = engine(10, 3);
    assert_eq!(e.scrolled_up(), 0);
    e.advance(b"a\r\nb\r\nc\r\nd\r\ne", 0.0);
    assert_eq!(e.scrolled_up(), 2);
    e.advance(b"\x1b[?1049h\r\n\r\n\r\n\r\n\x1b[?1049l", 0.0);
    assert_eq!(e.scrolled_up(), 2, "pantalla alternativa");
    e.advance(b"\x1b[2;3r\x1b[3;1H\n\n", 0.0);
    assert_eq!(e.scrolled_up(), 2, "región que no empieza arriba");
    e.advance(b"\x1b[r\x1b[3;1H\n", 0.0);
    assert_eq!(e.scrolled_up(), 3);
    // Desplazar la vista no mueve el contenido.
    e.scroll_display(1);
    assert_eq!(e.scrolled_up(), 3);
}
