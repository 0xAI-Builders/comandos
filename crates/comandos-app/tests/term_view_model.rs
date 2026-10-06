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
