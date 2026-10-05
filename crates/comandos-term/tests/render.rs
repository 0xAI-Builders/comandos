use comandos_term::{
    engine::{Engine, GridSize, Palette},
    glyphs::{BoxOp, box_ops},
    render::*,
};

fn eng() -> (Engine, Palette) {
    let p = Palette::xterm_default(
        [0xEA, 0xF0, 0xFB],
        [0x0A, 0x0D, 0x13],
        [0xFF, 0xAE, 0x1A],
        [0x0A, 0x0D, 0x13],
        [0x2E, 0x38, 0x52],
    );
    (
        Engine::new(GridSize { cols: 20, rows: 4 }, 100, p.clone()),
        p,
    )
}
const O: RenderOpts = RenderOpts {
    bold_is_bright: true,
    min_contrast: 1.0,
};

#[test]
fn same_style_ascii_is_one_run() {
    let (mut e, p) = eng();
    e.advance(b"abc\x1b[31mde", 0.0);
    let r = render_row(&e, 0, &p, &O);
    assert_eq!(
        r.runs.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(),
        ["abc", "de"]
    );
    assert_eq!(r.runs[1].style.fg, p.ansi[1]);
}

#[test]
fn bold_named_color_becomes_bright() {
    let (mut e, p) = eng();
    e.advance(b"\x1b[1;34mX", 0.0);
    assert_eq!(render_row(&e, 0, &p, &O).runs[0].style.fg, p.ansi[12]);
}

#[test]
fn inverse_swaps_and_default_bg_is_omitted() {
    let (mut e, p) = eng();
    e.advance(b"a\x1b[7mb", 0.0);
    let r = render_row(&e, 0, &p, &O);
    assert_eq!(r.runs[1].style.fg, p.bg);
    assert_eq!(
        r.bg_runs,
        vec![(1, 1, p.fg)],
        "solo se pinta el fondo que no es el de la terminal"
    );
}

#[test]
fn wide_glyphs_are_their_own_two_cell_run() {
    let (mut e, p) = eng();
    e.advance("a漢b".as_bytes(), 0.0);
    let r = render_row(&e, 0, &p, &O);
    let wide = r.runs.iter().find(|x| x.text == "漢").unwrap();
    assert_eq!((wide.col, wide.cells, &wide.kind), (1, 2, &RunKind::Wide));
}

#[test]
fn box_drawing_is_drawn_not_typed() {
    let (mut e, p) = eng();
    e.advance("─│█".as_bytes(), 0.0);
    let r = render_row(&e, 0, &p, &O);
    assert!(r.runs.iter().all(|x| matches!(x.kind, RunKind::Box(_))));
    assert!(matches!(box_ops('─').unwrap(), [BoxOp::HLine { y, .. }] if (*y - 0.5).abs() < 1e-6));
    assert!(matches!(box_ops('█').unwrap(), [BoxOp::Rect { w, h, .. }] if *w == 1.0 && *h == 1.0));
    assert!(box_ops('a').is_none());
}

#[test]
fn osc8_links_are_attached_to_runs() {
    let (mut e, p) = eng();
    e.advance(b"\x1b]8;;https://ej.mx\x1b\\ver\x1b]8;;\x1b\\ fin", 0.0);
    let r = render_row(&e, 0, &p, &O);
    let link = r
        .runs
        .iter()
        .find(|x| x.text == "ver")
        .unwrap()
        .link
        .unwrap();
    assert_eq!(r.links[link as usize], "https://ej.mx");
}

#[test]
fn cursor_reports_shape_and_position() {
    let (mut e, _) = eng();
    e.advance(b"ab\x1b[5 q", 0.0);
    let c = cursor(&e);
    assert_eq!((c.line, c.col, c.visible), (0, 2, true));
    assert_eq!(c.shape, CursorShape::Beam);
}

// --- Pruebas propias de A5 -------------------------------------------------

/// Mezcla de xterm.js para el atenuado: el texto se pinta con opacidad
/// 128/255 sobre el fondo de la celda.
fn dim_of(fg: [u8; 3], bg: [u8; 3]) -> [u8; 3] {
    let mix = |f: u8, b: u8| ((u32::from(f) * 128 + u32::from(b) * 127 + 127) / 255) as u8;
    [mix(fg[0], bg[0]), mix(fg[1], bg[1]), mix(fg[2], bg[2])]
}

#[test]
fn spaces_inside_a_run_are_kept_and_trailing_blanks_are_not() {
    let (mut e, p) = eng();
    e.advance(b"a b  c\x1b[31m  \x1b[0m", 0.0);
    let r = render_row(&e, 0, &p, &O);
    assert_eq!(
        r.runs
            .iter()
            .map(|x| (x.col, x.cells, x.text.as_str()))
            .collect::<Vec<_>>(),
        [(0, 6, "a b  c")]
    );
    assert!(r.bg_runs.is_empty());
}

#[test]
fn dim_blends_half_way_toward_the_cell_background() {
    let (mut e, p) = eng();
    e.advance(b"\x1b[2mx\x1b[0;2;41my", 0.0);
    let r = render_row(&e, 0, &p, &O);
    assert_eq!(r.runs[0].style.fg, dim_of(p.fg, p.bg));
    assert!(r.runs[0].style.dim);
    assert_eq!(r.runs[1].style.fg, dim_of(p.fg, p.ansi[1]));
    assert_eq!(r.runs[1].style.bg, p.ansi[1]);
}

#[test]
fn bold_brightens_only_the_first_eight_palette_entries() {
    let (mut e, p) = eng();
    e.advance(
        b"\x1b[1;38;5;2ma\x1b[38;5;9mb\x1b[38;5;100mc\x1b[38;2;1;2;3md\x1b[39me",
        0.0,
    );
    let fgs: Vec<_> = render_row(&e, 0, &p, &O)
        .runs
        .iter()
        .map(|x| x.style.fg)
        .collect();
    assert_eq!(fgs, [p.ansi[10], p.ansi[9], p.ansi[100], [1, 2, 3], p.fg]);

    let plain = RenderOpts {
        bold_is_bright: false,
        min_contrast: 1.0,
    };
    let (mut e, p) = eng();
    e.advance(b"\x1b[1;31mX", 0.0);
    assert_eq!(render_row(&e, 0, &p, &plain).runs[0].style.fg, p.ansi[1]);
}

#[test]
fn inverse_brightens_the_new_foreground_but_not_the_painted_background() {
    // xterm.js: con inverso, el color de texto es el antiguo fondo (y el
    // negrita lo aclara); el fondo pintado es el antiguo texto, sin aclarar.
    let (mut e, p) = eng();
    e.advance(b"\x1b[1;7;31;44mX", 0.0);
    let r = render_row(&e, 0, &p, &O);
    assert_eq!(r.runs[0].style.fg, p.ansi[12]);
    assert_eq!(r.runs[0].style.bg, p.ansi[1]);
    assert_eq!(r.bg_runs, vec![(0, 1, p.ansi[1])]);
}

#[test]
fn hidden_text_takes_the_background_color() {
    let (mut e, p) = eng();
    e.advance(b"\x1b[8;31;42mX", 0.0);
    let s = render_row(&e, 0, &p, &O).runs[0].style;
    assert!(s.hidden);
    assert_eq!((s.fg, s.bg), (p.ansi[2], p.ansi[2]));
}

#[test]
fn underline_variants_and_attributes() {
    let (mut e, p) = eng();
    e.advance(
        b"\x1b[4ma\x1b[4:2mb\x1b[4:3mc\x1b[4:4md\x1b[4:5me\x1b[0;3;9mf",
        0.0,
    );
    let r = render_row(&e, 0, &p, &O);
    let lines: Vec<_> = r.runs.iter().map(|x| x.style.underline).collect();
    assert_eq!(
        lines,
        [
            Underline::Single,
            Underline::Double,
            Underline::Curly,
            Underline::Dotted,
            Underline::Dashed,
            Underline::None
        ]
    );
    let last = r.runs[5].style;
    assert!(last.italic && last.strike && !last.bold);
}

#[test]
fn background_runs_group_equal_colors_and_cover_wide_spacers() {
    let (mut e, p) = eng();
    e.advance("\x1b[41m  \x1b[42m 漢\x1b[0m z".as_bytes(), 0.0);
    let r = render_row(&e, 0, &p, &O);
    assert_eq!(r.bg_runs, vec![(0, 2, p.ansi[1]), (2, 3, p.ansi[2])]);
}

#[test]
fn combining_marks_get_their_own_run() {
    let (mut e, p) = eng();
    e.advance("ae\u{301}b".as_bytes(), 0.0);
    let r = render_row(&e, 0, &p, &O);
    assert_eq!(
        r.runs
            .iter()
            .map(|x| (x.col, x.cells, x.text.as_str(), x.kind))
            .collect::<Vec<_>>(),
        [
            (0, 1, "a", RunKind::Text),
            (1, 1, "e\u{301}", RunKind::Text),
            (2, 1, "b", RunKind::Text)
        ]
    );
}

#[test]
fn non_latin_narrow_glyphs_are_drawn_one_cell_at_a_time() {
    let (mut e, p) = eng();
    e.advance("ñá αβ".as_bytes(), 0.0);
    let r = render_row(&e, 0, &p, &O);
    let texts: Vec<_> = r.runs.iter().map(|x| x.text.as_str()).collect();
    assert_eq!(texts, ["ñá", "α", "β"]);
}

#[test]
fn links_are_deduplicated_per_row() {
    let (mut e, p) = eng();
    e.advance(
        b"\x1b]8;;https://a.mx\x1b\\uno\x1b]8;;\x1b\\ \x1b]8;;https://b.mx\x1b\\dos\x1b]8;;\x1b\\ \x1b]8;;https://a.mx\x1b\\tres\x1b]8;;\x1b\\",
        0.0,
    );
    let r = render_row(&e, 0, &p, &O);
    assert_eq!(r.links, ["https://a.mx", "https://b.mx"]);
    let ids: Vec<_> = r.runs.iter().map(|x| (x.text.as_str(), x.link)).collect();
    assert_eq!(ids, [("uno", Some(0)), ("dos", Some(1)), ("tres", Some(0))]);
}

#[test]
fn osc4_palette_changes_are_rendered() {
    let (mut e, p) = eng();
    e.advance(b"\x1b]4;1;rgb:12/34/56\x1b\\\x1b[31mX", 0.0);
    assert_eq!(
        render_row(&e, 0, &p, &O).runs[0].style.fg,
        [0x12, 0x34, 0x56]
    );
}

#[test]
fn rows_outside_the_screen_are_empty_and_history_is_reachable() {
    let (mut e, p) = eng();
    e.advance(b"uno\r\ndos\r\ntres\r\ncuatro\r\ncinco", 0.0);
    assert_eq!(render_row(&e, 0, &p, &O).runs[0].text, "dos");
    let off = render_row(&e, 4, &p, &O);
    assert_eq!((off.line, off.runs.len(), off.bg_runs.len()), (4, 0, 0));
    e.scroll_display(1);
    assert_eq!(e.display_offset(), 1);
    assert_eq!(render_row(&e, 0, &p, &O).runs[0].text, "uno");
    assert_eq!(render_row(&e, 3, &p, &O).runs[0].text, "cuatro");
}

#[test]
fn render_into_reuses_buffers_and_matches_render_row() {
    let (mut e, p) = eng();
    e.advance(b"\x1b[41mab\x1b[0m cd \x1b[32mef", 0.0);
    let mut out = RowRender::default();
    render_row_into(&e, 0, &p, &O, &mut out);
    let cap = (out.runs.capacity(), out.bg_runs.capacity());
    let first = out.clone();
    render_row_into(&e, 0, &p, &O, &mut out);
    assert_eq!(out, first);
    assert_eq!(out, render_row(&e, 0, &p, &O));
    assert_eq!((out.runs.capacity(), out.bg_runs.capacity()), cap);
}

/// Contraste WCAG como lo calcula xterm.js (`rgb.relativeLuminance2`).
fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let lum = |c: [u8; 3]| {
        let ch = |v: u8| {
            let s = f64::from(v) / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2])
    };
    let (x, y) = (lum(a), lum(b));
    if x < y {
        (y + 0.05) / (x + 0.05)
    } else {
        (x + 0.05) / (y + 0.05)
    }
}

#[test]
fn minimum_contrast_lifts_text_but_not_drawing_glyphs() {
    let (mut e, p) = eng();
    e.advance("\x1b[38;2;20;20;30mA─".as_bytes(), 0.0);
    let opts = RenderOpts {
        bold_is_bright: true,
        min_contrast: 4.5,
    };
    let r = render_row(&e, 0, &p, &opts);
    assert!(contrast(r.runs[0].style.fg, p.bg) >= 4.5);
    assert_eq!(
        r.runs[1].style.fg,
        [20, 20, 30],
        "los glifos de dibujo no se ajustan"
    );
    // Con 1.0 (xterm.js por omisión) no se toca nada.
    assert_eq!(render_row(&e, 0, &p, &O).runs[0].style.fg, [20, 20, 30]);
}

#[test]
fn cursor_tracks_wide_cells_visibility_and_scrollback() {
    let (mut e, _) = eng();
    e.advance("漢\x1b[D".as_bytes(), 0.0);
    let c = cursor(&e);
    assert_eq!((c.col, c.wide, c.shape), (0, true, CursorShape::Block));

    e.advance(b"\x1b[?25l", 0.0);
    assert!(!cursor(&e).visible);
    e.advance(b"\x1b[?25h\x1b[2 q", 0.0);
    assert!(cursor(&e).visible);

    e.advance(b"\r\n1\r\n2\r\n3\r\n4\r\n5", 0.0);
    e.scroll_display(1);
    let c = cursor(&e);
    assert_eq!(
        (c.line, c.visible),
        (4, false),
        "la vista desplazada deja el cursor fuera"
    );
}

#[test]
fn every_xterm_custom_glyph_has_ops() {
    for c in ('\u{2500}'..='\u{259F}').chain('\u{E0B0}'..='\u{E0B3}') {
        let ops = box_ops(c).unwrap_or_else(|| panic!("falta {c:?}"));
        assert!(!ops.is_empty(), "{c:?} sin operaciones");
        for op in ops {
            let coords: Vec<f32> = match *op {
                BoxOp::HLine { y, x0, x1, w } => vec![y, x0, x1, w],
                BoxOp::VLine { x, y0, y1, w } => vec![x, y0, y1, w],
                BoxOp::Rect { x, y, w, h, alpha } => vec![x, y, w, h, alpha],
                BoxOp::Tri { pts } => pts.iter().flat_map(|&(a, b)| [a, b]).collect(),
            };
            assert!(
                coords
                    .iter()
                    .all(|v| v.is_finite() && (-0.6..=1.6).contains(v)),
                "{c:?} fuera de la celda: {op:?}"
            );
        }
    }
    assert!(box_ops('\u{E0B4}').is_none());
    assert!(box_ops('\u{24FF}').is_none());
    assert!(box_ops('\u{25A0}').is_none());
}

#[test]
fn lines_joints_blocks_and_shades_match_xterm_geometry() {
    // Línea fina y gruesa.
    let thin = match box_ops('─').unwrap() {
        [BoxOp::HLine { x0, x1, w, .. }] => {
            assert_eq!((*x0, *x1), (0.0, 1.0));
            *w
        }
        other => panic!("{other:?}"),
    };
    let thick = match box_ops('━').unwrap() {
        [BoxOp::HLine { w, .. }] => *w,
        other => panic!("{other:?}"),
    };
    assert!((thin - 1.0 / 12.0).abs() < 1e-6 && (thick - 1.0 / 6.0).abs() < 1e-6);

    // Una esquina se prolonga medio grosor en la unión (como el «miter» de
    // xterm.js) para no dejar muesca.
    let corner = box_ops('┌').unwrap();
    assert!(corner.iter().any(|op| matches!(*op,
        BoxOp::VLine { x, y0, y1, .. } if (x - 0.5).abs() < 1e-6 && y0 < 0.5 && y1 == 1.0)));
    assert!(corner.iter().any(|op| matches!(*op,
        BoxOp::HLine { y, x0, x1, .. } if (y - 0.5).abs() < 1e-6 && x0 < 0.5 && x1 == 1.0)));

    // Doble línea: dos trazos simétricos alrededor del centro.
    let ys: Vec<f32> = box_ops('═')
        .unwrap()
        .iter()
        .filter_map(|op| match *op {
            BoxOp::HLine { y, .. } => Some(y),
            _ => None,
        })
        .collect();
    assert_eq!(ys.len(), 2);
    assert!(((ys[0] + ys[1]) / 2.0 - 0.5).abs() < 1e-6 && (ys[0] - ys[1]).abs() > 0.1);

    // Bloques en octavos y sombreados como rectángulo traslúcido.
    assert!(matches!(box_ops('▄').unwrap(),
        [BoxOp::Rect { x, y, w, h, alpha }] if (*x, *y, *w, *h, *alpha) == (0.0, 0.5, 1.0, 0.5, 1.0)));
    assert!(matches!(box_ops('▕').unwrap(),
        [BoxOp::Rect { x, w, .. }] if (*x, *w) == (0.875, 0.125)));
    for (c, a) in [('░', 0.25), ('▒', 0.5), ('▓', 0.75)] {
        assert!(matches!(box_ops(c).unwrap(),
            [BoxOp::Rect { w, h, alpha, .. }] if (*w, *h, *alpha) == (1.0, 1.0, a)));
    }
}

#[test]
fn curves_diagonals_and_powerline_are_consistent_triangles() {
    // A6 rellena todos los `Tri` de un glifo en un solo trazado (regla
    // nonzero): tienen que tener la misma orientación y área positiva.
    for c in [
        '╭', '╮', '╯', '╰', '╱', '╲', '╳', '\u{E0B0}', '\u{E0B1}', '\u{E0B2}', '\u{E0B3}',
    ] {
        let ops = box_ops(c).unwrap();
        assert!(
            ops.iter().all(|op| matches!(op, BoxOp::Tri { .. })),
            "{c:?}"
        );
        for op in ops {
            if let BoxOp::Tri { pts: [a, b, d] } = *op {
                let area = (b.0 - a.0) * (d.1 - a.1) - (d.0 - a.0) * (b.1 - a.1);
                assert!(area > 0.0, "{c:?}: {op:?}");
            }
        }
    }
    // El triángulo relleno de powerline apunta a la derecha y cubre el alto.
    let ops = box_ops('\u{E0B0}').unwrap();
    assert_eq!(ops.len(), 1);
    if let [BoxOp::Tri { pts }] = ops {
        let max_x = pts.iter().map(|p| p.0).fold(0.0_f32, f32::max);
        assert!(max_x > 0.8 && max_x <= 1.0);
        assert!(pts.contains(&(0.0, 0.0)) && pts.contains(&(0.0, 1.0)));
    }
    // La esquina redondeada empieza en el borde inferior y acaba en el derecho.
    let pts: Vec<(f32, f32)> = box_ops('╭')
        .unwrap()
        .iter()
        .flat_map(|op| match *op {
            BoxOp::Tri { pts } => pts.to_vec(),
            _ => Vec::new(),
        })
        .collect();
    assert!(pts.iter().any(|p| (p.1 - 1.0).abs() < 1e-6));
    assert!(pts.iter().any(|p| (p.0 - 1.0).abs() < 1e-6));
    assert!(pts.iter().all(|p| p.0 > 0.3 && p.1 > 0.3));
}
