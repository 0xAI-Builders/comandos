use comandos_term::{
    engine::{Engine, GridSize, Palette},
    select::*,
};

fn eng(cols: u16) -> Engine {
    Engine::new(
        GridSize { cols, rows: 5 },
        100,
        Palette::xterm_default([255; 3], [0; 3], [255; 3], [0; 3], [80; 3]),
    )
}

#[test]
fn wrapped_rows_copy_without_newline_and_trailing_spaces_are_trimmed() {
    let mut e = eng(10);
    e.advance(b"0123456789abc   \r\nfin", 0.0);
    let s = Selection {
        anchor: (0, 0),
        head: (2, 2),
        mode: SelectMode::Simple,
    };
    assert_eq!(selected_text(&e, &s), "0123456789abc\nfin");
}

#[test]
fn url_split_by_tmux_hard_wrap_is_one_link() {
    let mut e = eng(20);
    e.advance(b"ver https://ejemplo.mx/un/camino/largo ok", 0.0);
    let urls = find_urls(&e, 1);
    assert_eq!(urls.len(), 1);
    assert_eq!(urls[0].url, "https://ejemplo.mx/un/camino/largo");
    assert_eq!((urls[0].start.0, urls[0].end.0), (0, 1));
}

#[test]
fn ligature_candidates_are_greedy() {
    assert_eq!(ligature_runs("a <=> b != c"), vec![(2, 5), (8, 10)]);
    assert_eq!(ligature_runs("===>"), vec![(0, 4)]);
}

fn sel(a: (i32, u16), h: (i32, u16), mode: SelectMode) -> Selection {
    Selection {
        anchor: a,
        head: h,
        mode,
    }
}

#[test]
fn reversed_anchor_and_head_give_the_same_text() {
    let mut e = eng(20);
    e.advance(b"hola mundo\r\nsegunda", 0.0);
    let fwd = selected_text(&e, &sel((0, 5), (1, 2), SelectMode::Simple));
    let rev = selected_text(&e, &sel((1, 2), (0, 5), SelectMode::Simple));
    assert_eq!(fwd, "mundo\nseg");
    assert_eq!(rev, fwd);
}

#[test]
fn word_mode_uses_xterm_separators_including_comma_and_backtick() {
    let mut e = eng(30);
    e.advance(b"uno,dos `tres` (cuatro)", 0.0);
    // "dos" entre la coma y el espacio.
    assert_eq!(
        selected_text(&e, &sel((0, 5), (0, 5), SelectMode::Word)),
        "dos"
    );
    assert_eq!(
        selected_text(&e, &sel((0, 10), (0, 10), SelectMode::Word)),
        "tres"
    );
    assert_eq!(
        selected_text(&e, &sel((0, 17), (0, 17), SelectMode::Word)),
        "cuatro"
    );
    // Arrastrar de una palabra a otra las incluye enteras.
    assert_eq!(
        selected_text(&e, &sel((0, 1), (0, 5), SelectMode::Word)),
        "uno,dos"
    );
}

#[test]
fn word_at_is_none_on_blank_run_and_follows_wrapped_rows() {
    let mut e = eng(10);
    e.advance(b"ab         cdefghijklmno", 0.0);
    assert!(word_at(&e, (0, 5)).is_none());
    // "cdefghijklmno" (13) arranca en la columna 11 → fila 1, cols 1..
    let (start, end) = word_at(&e, (1, 3)).expect("palabra");
    assert_eq!((start, end), ((1, 1), (2, 3)));
}

#[test]
fn line_mode_selects_whole_wrapped_logical_lines() {
    let mut e = eng(10);
    e.advance(b"0123456789abcde\r\notra", 0.0);
    assert_eq!(
        selected_text(&e, &sel((1, 2), (1, 2), SelectMode::Line)),
        "0123456789abcde"
    );
    assert_eq!(
        selected_text(&e, &sel((0, 0), (2, 0), SelectMode::Line)),
        "0123456789abcde\notra"
    );
}

#[test]
fn history_rows_are_negative_and_nbsp_copies_as_space() {
    let mut e = eng(10);
    e.advance(b"h0\r\nh1\r\nh2\r\nh3\r\nh4\r\nh5\r\na\xc2\xa0b", 0.0);
    // Cinco filas de pantalla: «h0» y «h1» pasaron a la historia.
    assert_eq!(e.history_len(), 2);
    assert_eq!(
        selected_text(&e, &sel((-2, 0), (-1, 1), SelectMode::Simple)),
        "h0\nh1"
    );
    assert_eq!(
        selected_text(&e, &sel((4, 0), (4, 2), SelectMode::Simple)),
        "a b"
    );
}

#[test]
fn wide_chars_are_copied_once() {
    let mut e = eng(10);
    e.advance("日本x".as_bytes(), 0.0);
    assert_eq!(
        selected_text(&e, &sel((0, 0), (0, 5), SelectMode::Simple)),
        "日本x"
    );
}

#[test]
fn single_row_url_trailing_punctuation_is_left_out() {
    let mut e = eng(60);
    e.advance(b"mira (https://a.mx/p?q=1). y http://b.mx", 0.0);
    let urls = find_urls(&e, 0);
    let list: Vec<&str> = urls.iter().map(|u| u.url.as_str()).collect();
    assert_eq!(list, vec!["https://a.mx/p?q=1", "http://b.mx"]);
    assert_eq!(urls[0].start, (0, 6));
    assert_eq!(urls[0].end, (0, 23));
}

#[test]
fn osc8_hyperlinks_are_found_per_row() {
    let mut e = eng(40);
    e.advance(
        b"ir a \x1b]8;;https://osc.mx/x\x1b\\este sitio\x1b]8;;\x1b\\ ya",
        0.0,
    );
    let urls = find_urls(&e, 0);
    assert_eq!(urls.len(), 1);
    assert_eq!(urls[0].url, "https://osc.mx/x");
    assert_eq!((urls[0].start, urls[0].end), ((0, 5), (0, 14)));
    // Solo http/https.
    let mut e = eng(40);
    e.advance(b"\x1b]8;;file:///etc/passwd\x1b\\x\x1b]8;;\x1b\\", 0.0);
    assert!(find_urls(&e, 0).is_empty());
}

#[test]
fn hard_wrapped_url_is_found_from_either_row_and_not_from_others() {
    let mut e = eng(20);
    e.advance(b"ver https://ejemplo.mx/un/camino/largo ok\r\nfin", 0.0);
    assert_eq!(find_urls(&e, 0).len(), 1);
    assert_eq!(find_urls(&e, 1).len(), 1);
    assert!(find_urls(&e, 3).is_empty());
}

#[test]
fn ligature_runs_misc() {
    assert!(ligature_runs("").is_empty());
    assert!(ligature_runs("abc def").is_empty());
    assert_eq!(ligature_runs("<===>"), vec![(0, 5)]);
    assert_eq!(ligature_runs("é->é"), vec![(1, 3)]);
    assert_eq!(ligature_runs("->->"), vec![(0, 2), (2, 4)]);
    assert_eq!(ligature_runs("a\\/b"), vec![(1, 3)]);
}
