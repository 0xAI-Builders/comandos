use comandos_web_dom::log::{BATCH, Buffer, FLUSH_AT, FLUSH_EVERY_MS, TEXT_MAX, js_round};

#[test]
fn constants_match_the_inline_registry() {
    // `ulog`: flush con 40; `ulogFlush`: `splice(0, 200)`; `setInterval(ulogFlush, 5000)`;
    // `String(n).slice(0, 80)`.
    assert_eq!(
        (FLUSH_AT, BATCH, FLUSH_EVERY_MS, TEXT_MAX),
        (40, 200, 5000, 80)
    );
}

#[test]
fn buffer_asks_for_a_flush_at_forty_and_drains_two_hundred() {
    let mut b = Buffer::default();
    for i in 0..39 {
        assert!(!b.push(i), "{i}");
    }
    assert!(b.push(39));
    assert!(b.push(40), "sigue pidiendo mientras haya ≥ 40");
    for i in 41..250 {
        b.push(i);
    }
    let first = b.take_batch();
    assert_eq!(first.len(), 200);
    assert_eq!(first.first(), Some(&0));
    assert_eq!(b.take_batch(), (200..250).collect::<Vec<_>>());
    assert!(b.take_batch().is_empty());
}

#[test]
fn js_round_is_math_round() {
    // Empates hacia +∞, no lejos del cero (`f64::round` daría -3 y 1).
    assert_eq!(js_round(2.5), 3.0);
    assert_eq!(js_round(-2.5), -2.0);
    assert_eq!(js_round(0.499_999_999_999_999_94), 0.0);
    assert_eq!(js_round(1.4), 1.0);
    assert_eq!(js_round(-0.2), 0.0);
    assert_eq!(js_round(f64::NAN), 0.0, "`d || 0`: NaN pasa a 0");
}
