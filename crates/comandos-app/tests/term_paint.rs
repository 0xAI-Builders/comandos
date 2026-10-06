#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::term::engine::{RowRender, TermEngine, Underline};
use comandos_app::term::paint::{
    CellGeom, LineKind, PaintOp, palette_from_theme, plan_row, vte_dim,
};
use std::time::{Duration, Instant};

const PAL: [&str; 16] = [
    "#232B3E", "#FF6B6B", "#2EE59D", "#FFAE1A", "#7AA5FF", "#B49CFF", "#4DD0E1", "#EAF0FB",
    "#5E6980", "#FF8F8F", "#6FF0BC", "#FFC55C", "#A3C0FF", "#CDBBFF", "#86E3F0", "#FFFFFF",
];

fn engine(cols: u16, rows: u16) -> (TermEngine, Instant) {
    let epoch = Instant::now();
    let pal = palette_from_theme("#EAF0FB", "#0A0D13", "#FFAE1A", &PAL).unwrap();
    (TermEngine::new(cols, rows, 100, pal, false, epoch), epoch)
}

fn geom() -> CellGeom {
    CellGeom {
        cell_w: 8.0,
        cell_h: 19.0,
        origin_x: 10.0,
        origin_y: 22.0,
        dpr: 1.0,
        font_size: 13.0,
    }
}

fn plan(t: &TermEngine, line: usize) -> Vec<PaintOp> {
    let mut row = RowRender::default();
    t.render_line(line, &mut row);
    let mut ops = Vec::new();
    plan_row(&row, line, &geom(), &|col| t.fg_is_rgb(line, col), &mut ops);
    ops
}

fn texts(ops: &[PaintOp]) -> Vec<(String, u16, u16, [u8; 3], bool)> {
    ops.iter()
        .filter_map(|o| match o {
            PaintOp::Text {
                col,
                cells,
                text,
                rgb,
                bold,
                ..
            } => Some((text.clone(), *col, *cells, *rgb, *bold)),
            _ => None,
        })
        .collect()
}

#[test]
fn palette_from_theme_hex() {
    let p = palette_from_theme("#EAF0FB", "#0A0D13", "#FFAE1A", &PAL).unwrap();
    assert_eq!(p.ansi[1], [0xFF, 0x6B, 0x6B]);
    assert_eq!(p.ansi[16], [0, 0, 0]);
    assert_eq!(p.ansi[231], [255, 255, 255]);
    assert_eq!(p.ansi[232], [8, 8, 8]);
    assert_eq!(
        (p.fg, p.bg, p.cursor, p.cursor_accent),
        (
            [0xEA, 0xF0, 0xFB],
            [0x0A, 0x0D, 0x13],
            [0xFF, 0xAE, 0x1A],
            [0x0A, 0x0D, 0x13]
        )
    );
    assert!(palette_from_theme("#EAF0FB", "nope", "#FFAE1A", &PAL).is_none());
}

#[test]
fn ascii_runs_merge() {
    let (mut t, e) = engine(20, 2);
    t.feed(b"hola mundo", e);
    let got = texts(&plan(&t, 0));
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!((got[0].0.trim_end(), got[0].1), ("hola mundo", 0));
}

#[test]
fn wide_cells_paint_once() {
    let (mut t, e) = engine(20, 2);
    t.feed("a漢b🚀".as_bytes(), e);
    let got = texts(&plan(&t, 0));
    let wide: Vec<_> = got.iter().filter(|(s, ..)| s.contains('漢')).collect();
    assert_eq!(wide.len(), 1, "{got:?}");
    assert_eq!((wide[0].1, wide[0].2), (1, 2), "columna 1, dos celdas");
    let rocket: Vec<_> = got.iter().filter(|(s, ..)| s.contains('🚀')).collect();
    assert_eq!((rocket.len(), rocket[0].2), (1, 2));
    assert!(
        got.iter()
            .any(|(s, col, ..)| s.starts_with('b') && *col == 3)
    );
}

#[test]
fn box_chars_become_draw_ops() {
    let (mut t, e) = engine(10, 1);
    t.feed("─│┼".as_bytes(), e);
    let ops = plan(&t, 0);
    let glyphs = ops
        .iter()
        .filter(|o| matches!(o, PaintOp::Glyph { .. }))
        .count();
    assert_eq!(glyphs, 3);
    assert!(
        texts(&ops)
            .iter()
            .all(|(s, ..)| !s.contains(['─', '│', '┼']))
    );
}

#[test]
fn bg_runs_become_rects_but_default_bg_does_not() {
    let (mut t, e) = engine(10, 1);
    t.feed(b"\x1b[41m  \x1b[0m  ", e);
    let rects: Vec<_> = plan(&t, 0)
        .into_iter()
        .filter_map(|o| match o {
            PaintOp::Rect { x, y, w, h, rgb } => Some((x, y, w, h, rgb)),
            _ => None,
        })
        .collect();
    assert_eq!(rects, [(10.0, 22.0, 16.0, 19.0, [0xFF, 0x6B, 0x6B])]);
}

#[test]
fn dim_uses_vte_two_thirds_except_rgb() {
    let (mut t, e) = engine(10, 2);
    t.feed(b"\x1b[2mx\x1b[0m\r\n\x1b[2;38;2;200;100;50my", e);
    assert_eq!(texts(&plan(&t, 0))[0].3, vte_dim([0xEA, 0xF0, 0xFB]));
    assert_eq!(vte_dim([0xEA, 0xF0, 0xFB]), [0x9C, 0xA0, 0xA7]);
    assert_eq!(texts(&plan(&t, 1))[0].3, [200, 100, 50]);
}

#[test]
fn bold_is_not_bright_like_vte() {
    let (mut t, e) = engine(10, 1);
    t.feed(b"\x1b[1;31mx", e);
    let got = texts(&plan(&t, 0));
    assert_eq!((got[0].3, got[0].4), ([0xFF, 0x6B, 0x6B], true));
}

#[test]
fn underline_and_strike_become_lines() {
    let (mut t, e) = engine(10, 1);
    t.feed(b"\x1b[4mu\x1b[24;9ms", e);
    let kinds: Vec<LineKind> = plan(&t, 0)
        .into_iter()
        .filter_map(|o| match o {
            PaintOp::Line { kind, .. } => Some(kind),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        [LineKind::Under(Underline::Single), LineKind::Strike]
    );
}

#[test]
fn sync_update_defers_damage() {
    let (mut t, e) = engine(10, 2);
    let mut lines = Vec::new();
    let _ = t.take_dirty(&mut lines);
    lines.clear();
    t.feed(b"\x1b[?2026habc", e);
    assert!(t.next_deadline().is_some());
    assert!(
        !t.take_dirty(&mut lines) && lines.is_empty(),
        "retenido: nada que pintar"
    );
    t.feed(b"\x1b[?2026l", e + Duration::from_millis(5));
    assert!(t.take_dirty(&mut lines) || lines.contains(&0));
    assert!(t.next_deadline().is_none());
}

#[test]
fn strict_hex_and_exact_palette_shape() {
    use comandos_app::term::paint::hex_rgb;
    assert_eq!(hex_rgb("#aBcD09"), Some([0xAB, 0xCD, 9]));
    for value in [
        "#+12345",
        "#12345",
        "#1234567",
        "123456",
        "#１２３４５６",
        "#12GG00",
    ] {
        assert_eq!(hex_rgb(value), None, "{value}");
    }
    assert!(palette_from_theme("#EAF0FB", "#0A0D13", "#FFAE1A", &PAL[..15]).is_none());
}
#[test]
fn box_glyphs_keep_underline_and_strike() {
    let (mut t, e) = engine(10, 1);
    t.feed("\x1b[4;9m─".as_bytes(), e);
    let ops = plan(&t, 0);
    assert_eq!(
        ops.iter()
            .filter(|o| matches!(o, PaintOp::Glyph { .. }))
            .count(),
        1
    );
    assert_eq!(
        ops.iter()
            .filter(|o| matches!(o, PaintOp::Line { .. }))
            .count(),
        2
    );
}
#[test]
fn dim_run_splits_when_equal_rgb_has_different_provenance() {
    let (mut t, e) = engine(10, 1);
    t.feed(b"\x1b[2mA\x1b[38;2;234;240;251mB\x1b[39mC", e);
    let got = texts(&plan(&t, 0));
    assert_eq!(
        got.iter().map(|t| (&*t.0, t.1, t.3)).collect::<Vec<_>>(),
        [
            ("A", 0, vte_dim([234, 240, 251])),
            ("B", 1, [234, 240, 251]),
            ("C", 2, vte_dim([234, 240, 251]))
        ]
    );
}
#[test]
fn rgb_lookup_tracks_scrollback_and_selection_reexports() {
    use comandos_app::term::engine::{SelectMode, Selection, selected_text};
    let (mut t, e) = engine(10, 2);
    t.feed(b"\x1b[38;2;200;100;50mR\r\n\x1b[39mI\r\nJ", e);
    t.scroll(1);
    assert!(t.fg_is_rgb(0, 0));
    assert!(!t.fg_is_rgb(1, 0));
    assert!(!t.fg_is_rgb(2, 0));
    assert!(!t.fg_is_rgb(0, 10));
    assert_eq!(
        selected_text(
            t.engine(),
            &Selection {
                anchor: (-1, 0),
                head: (-1, 0),
                mode: SelectMode::Simple
            }
        ),
        "R"
    );
}
#[test]
fn inverse_dim_uses_effective_foreground_provenance() {
    let (mut t, e) = engine(10, 1);
    t.feed(b"\x1b[2;7;38;2;200;100;50mI", e);
    assert!(!t.fg_is_rgb(0, 0));
    assert_eq!(texts(&plan(&t, 0))[0].3, vte_dim([10, 13, 19]));
}
#[test]
fn adapter_reports_engine_clamped_size() {
    let (mut t, _) = engine(0, 0);
    assert_eq!(t.size(), (2, 1));
    t.resize(0, 0, (8, 19));
    assert_eq!(t.size(), (2, 1));
}

#[test]
fn explicit_background_equal_to_theme_stays_opaque_even_on_spaces() {
    let (mut t, e) = engine(10, 1);
    t.feed(b"\x1b[48;2;10;13;19m  \x1b[0m", e);
    let rects: Vec<_> = plan(&t, 0)
        .into_iter()
        .filter_map(|o| {
            if let PaintOp::Rect { x, y, w, h, rgb } = o {
                Some((x, y, w, h, rgb))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(rects, [(10.0, 22.0, 16.0, 19.0, [10, 13, 19])]);
}
#[test]
fn synchronized_update_timeout_releases_damage() {
    let (mut t, e) = engine(10, 1);
    let mut lines = Vec::new();
    t.take_dirty(&mut lines);
    t.feed(b"\x1b[?2026habc", e);
    let deadline = t.next_deadline().unwrap();
    assert!(!t.take_dirty(&mut lines));
    assert!(t.tick(deadline + Duration::from_millis(1)));
    assert!(t.take_dirty(&mut lines) || lines.contains(&0));
    assert!(t.next_deadline().is_none());
}

#[test]
fn bounded_multicolor_frame_cost() {
    let (mut t, epoch) = engine(120, 40);
    let mut input = String::new();
    for line in 0..40 {
        input.push_str(&format!("\x1b[{};1H\x1b[2m", line + 1));
        for col in 0..120 {
            input.push_str(if col % 2 == 0 {
                "\x1b[38;2;255;0;0m"
            } else {
                "\x1b[31m"
            });
            input.push('x');
        }
    }
    t.feed(input.as_bytes(), epoch);
    let started = Instant::now();
    let mut total = 0;
    for _ in 0..5 {
        for line in 0..40 {
            total += plan(&t, line).len();
        }
    }
    assert!(total >= 24_000);
    eprintln!(
        "five 120x40 mixed DIM frames: {:?}; ops={total}",
        started.elapsed()
    );
    assert!(!t.fg_is_rgb(40, 0));
    assert!(!t.fg_is_rgb(0, 120));
}
