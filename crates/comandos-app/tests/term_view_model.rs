#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::term::{
    engine::{Ime, KeyAction, Modes},
    keys::key_action,
    links::{normalize_wrapped_url_text, url_from_wrapped_text},
    schedule::PaintSchedule,
};
#[test]
fn wrapped_links_keep_utf8_and_point_coordinates() {
    assert_eq!(
        normalize_wrapped_url_text("https://example.org/ñandú"),
        "https://example.org/ñandú"
    );
    assert_eq!(
        normalize_wrapped_url_text("https://example.org/\n  ñandú"),
        "https://example.org/ñandú"
    );
    assert_eq!(
        url_from_wrapped_text("one https://example.org/\nñandú two", "", Some(1), Some(1)),
        Some("https://example.org/ñandú".into())
    );
    assert_eq!(url_from_wrapped_text("/model", "", None, None), None);
    assert_eq!(
        url_from_wrapped_text("/home/test/\nfile.rs:22", "", None, None),
        Some("/home/test/file.rs:22".into())
    );
}

#[test]
fn padded_terminal_wrap_returns_the_entire_link_from_either_row() {
    use comandos_app::term::{
        engine::{Palette, TermEngine},
        links::link_at,
    };
    let now = std::time::Instant::now();
    let palette = Palette::xterm_default([255; 3], [0; 3], [255; 3], [0; 3], [255; 3]);
    let mut engine = TermEngine::new(60, 4, 10, palette, false, now);
    engine.feed(
        b"https://example.invalid/very/long/\r\n  resource?id=1",
        now,
    );
    for (row, col) in [(0, 8), (1, 5)] {
        assert_eq!(
            link_at(&engine, (i32::from(row), col), row, col),
            Some("https://example.invalid/very/long/resource?id=1".into())
        );
    }
}

#[test]
fn link_clicks_and_drags_are_owned_by_the_menu_even_with_mouse_reporting() {
    use comandos_app::term::links::{PrimaryPress, report_mouse};
    assert!(
        report_mouse(false, false),
        "ordinary mouse events still reach tmux"
    );
    assert!(
        !report_mouse(true, false),
        "Shift retains terminal selection"
    );
    assert!(
        !report_mouse(false, true),
        "a link gesture must reach the native menu"
    );
    let press = PrimaryPress::new((10., 20.), (1, 2), Some("https://example.invalid/a".into()));
    assert!(
        press.clone().release((30., 20.), 1).is_none(),
        "dragging never opens the menu"
    );
    let clicked = press.release((10., 20.), 1).unwrap();
    assert_eq!(clicked.link.as_deref(), Some("https://example.invalid/a"));
    let rows = comandos_app::ui::menu::link_rows(false, false);
    assert_eq!(
        rows.iter()
            .map(|(_, _, action)| action.is_some())
            .collect::<Vec<_>>(),
        [true, false]
    );
}
#[test]
fn damage_coalesces_and_blink_stops_without_focus() {
    let mut schedule = PaintSchedule::default();
    schedule.damage(0, &[1, 2]);
    schedule.damage(1, &[2, 3]);
    assert_eq!(schedule.pending_lines(), vec![1, 2, 3]);
    assert_eq!(schedule.next_delay_ms(2), Some(0));
    schedule.painted(2);
    assert!(schedule.pending_lines().is_empty());
    schedule.set_focused(false, 3);
    assert!(!schedule.blink_due(1000));
    schedule.set_focused(true, 1000);
    assert!(schedule.blink_due(1600));
}
#[test]
fn gdk_keys_and_ime_do_not_duplicate_utf8() {
    assert_eq!(
        key_action(
            gdk::keys::constants::Up,
            gdk::ModifierType::empty(),
            &Modes::default()
        ),
        KeyAction::Send(b"\x1b[A".to_vec())
    );
    let mut ime = Ime::default();
    ime.start();
    assert!(ime.keydown_ignored(0));
    assert_eq!(ime.input("insertText", Some("漢"), true), None);
    assert_eq!(ime.end("漢字"), Some("漢字".as_bytes().to_vec()));
    assert_eq!(ime.input("insertText", Some("漢字"), false), None);
}
#[test]
fn frame_cache_preserves_pixels_during_sync_and_flushes_at_deadline() {
    use comandos_app::term::{
        engine::{Palette, TermEngine},
        paint::{CellGeom, FrameCache},
    };
    use std::time::{Duration, Instant};
    let epoch = Instant::now();
    let p = Palette::xterm_default([255; 3], [0; 3], [255; 3], [0; 3], [255; 3]);
    let mut t = TermEngine::new(20, 2, 10, p, false, epoch);
    let geom = CellGeom {
        cell_w: 8.,
        cell_h: 19.,
        origin_x: 0.,
        origin_y: 0.,
        dpr: 1.,
        font_size: 13.,
    };
    let mut frame = FrameCache::default();
    t.feed(b"before", epoch);
    assert!(frame.refresh(&t, &geom));
    let old = frame.ops().to_vec();
    t.feed(b"\x1b[?2026h\rAFTER", epoch + Duration::from_millis(1));
    assert!(!frame.refresh(&t, &geom));
    assert_eq!(frame.ops(), old);
    t.tick(epoch + Duration::from_secs(3));
    assert!(frame.refresh(&t, &geom));
    assert_ne!(frame.ops(), old);
}
#[test]
fn punctuation_digits_and_tab_map_to_terminal_codes() {
    use gdk::{ModifierType as Mod, keys::constants as key};
    let modes = Modes::default();
    assert_eq!(
        key_action(key::bracketleft, Mod::CONTROL_MASK, &modes),
        KeyAction::Send(vec![27])
    );
    assert_eq!(
        key_action(key::space, Mod::CONTROL_MASK, &modes),
        KeyAction::Send(vec![0])
    );
    assert_eq!(
        key_action(key::_1, Mod::MOD1_MASK, &modes),
        KeyAction::Send(b"\x1b1".to_vec())
    );
    assert_eq!(
        key_action(key::ISO_Left_Tab, Mod::empty(), &modes),
        KeyAction::Send(b"\x1b[Z".to_vec())
    );
}
#[test]
fn three_mouse_buttons_keep_identity_in_sgr_press_release_and_drag() {
    use comandos_app::term::{
        engine::{Button, MouseKind, MouseMode, encode_mouse},
        keys::{drag_button, mouse_button},
    };
    let modes = Modes {
        mouse: MouseMode::Drag,
        sgr_mouse: true,
        ..Modes::default()
    };
    for (number, button, state, code) in [
        (1, Button::Left, gdk::ModifierType::BUTTON1_MASK, 0),
        (2, Button::Middle, gdk::ModifierType::BUTTON2_MASK, 1),
        (3, Button::Right, gdk::ModifierType::BUTTON3_MASK, 2),
    ] {
        assert_eq!(mouse_button(number), button);
        assert_eq!(drag_button(state), button);
        assert_eq!(
            encode_mouse(
                mouse_button(number),
                MouseKind::Press,
                4,
                5,
                (false, false, false),
                &modes
            ),
            Some(format!("\x1b[<{code};5;6M").into_bytes())
        );
        assert_eq!(
            encode_mouse(
                mouse_button(number),
                MouseKind::Release,
                4,
                5,
                (false, false, false),
                &modes
            ),
            Some(format!("\x1b[<{code};5;6m").into_bytes())
        );
        assert_eq!(
            encode_mouse(
                drag_button(state),
                MouseKind::Move,
                4,
                5,
                (false, false, false),
                &modes
            ),
            Some(format!("\x1b[<{};5;6M", code + 32).into_bytes())
        );
    }
}
#[test]
fn synchronized_update_freezes_blink_until_end() {
    let mut schedule = PaintSchedule::default();
    schedule.set_focused(true, 0);
    schedule.set_synchronized(true);
    assert!(!schedule.blink_due(800));
    schedule.set_synchronized(false);
    assert!(schedule.blink_due(800));
}
