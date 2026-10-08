//! One renderer hash-table representation versus the original three tables.
use comandos_term_web::canvas::{GlyphIndex, MAX_ENTRIES, MAX_KEY_BYTES};
#[path = "fixtures/glyph_index_original.rs"]
mod original;
use original::OriginalIndex;
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;
#[derive(Clone, Copy, PartialEq, Eq)]
struct Collision(u32);
impl Hash for Collision {
    fn hash<H: Hasher>(&self, state: &mut H) {
        0u8.hash(state);
    }
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn borrowed_lookup_replacement_unicode_collision_and_caps_match_original() {
    let mut current = GlyphIndex::<Collision, u32>::default();
    let mut previous = OriginalIndex::<Collision, u32>::default();
    let texts = ["", "a", "\0", "😀", "e\u{301}", "👩‍💻", "ab", "a\0"];
    for round in 0..2 {
        for (n, text) in texts.iter().enumerate() {
            for key in 0..20 {
                let slot = round * 10000 + n as u32 * 100 + key;
                current.insert(text, Collision(key), slot);
                previous.insert(text, Collision(key), slot);
                assert_eq!(current.len(), previous.len());
                assert_eq!(current.key_bytes(), previous.key_bytes());
                for candidate in texts {
                    for style in 0..21 {
                        assert_eq!(
                            current.lookup(candidate, &Collision(style)),
                            previous.lookup(candidate, &Collision(style))
                        );
                    }
                }
            }
        }
    }
    let mut a = BTreeMap::new();
    let mut b = BTreeMap::new();
    current.for_each(|t, k, s| {
        a.insert((t.to_owned(), k.0), *s);
    });
    previous.for_each(|t, k, s| {
        b.insert((t.to_owned(), k.0), *s);
    });
    assert_eq!(a, b);
    assert_eq!(current.is_empty(), previous.is_empty());
    current.clear();
    previous.clear();
    assert_eq!(
        (
            current.len(),
            current.key_bytes(),
            current.generation(),
            current.is_empty()
        ),
        (
            previous.len(),
            previous.key_bytes(),
            previous.generation(),
            previous.is_empty()
        )
    );
    for i in 0..MAX_ENTRIES {
        current.insert("a", Collision(i as u32), i as u32);
        previous.insert("a", Collision(i as u32), i as u32);
    }
    assert!(current.needs_room("a"));
    assert_eq!(current.needs_room("a"), previous.needs_room("a"));
    current.clear();
    previous.clear();
    let long = "x".repeat(MAX_KEY_BYTES);
    assert_eq!(current.needs_room(&long), previous.needs_room(&long));
    current.insert(&long, Collision(0), 1);
    previous.insert(&long, Collision(0), 1);
    assert_eq!(
        current.lookup(&long, &Collision(0)),
        previous.lookup(&long, &Collision(0))
    );
    assert!(current.needs_room("a"));
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[ignore = "diagnostic: full-cardinality renderer lookup timing, no product threshold"]
fn full_cardinality_hot_lookup_timing() {
    let mut current = GlyphIndex::<[u32; 6], u32>::default();
    let mut previous = OriginalIndex::<[u32; 6], u32>::default();
    let keys = (0..MAX_ENTRIES)
        .map(|n| [n as u32, 0xabcdef, 0xff, 1, 0x123456, 2])
        .collect::<Vec<_>>();
    for (n, key) in keys.iter().enumerate() {
        let text = if n % 2 == 0 { "a" } else { "e\u{301}" };
        current.insert(text, *key, n as u32);
        previous.insert(text, *key, n as u32);
    }
    for text in ["a", "e\u{301}"] {
        let mut old = Vec::new();
        let mut new = Vec::new();
        for round in 0..9 {
            for new_first in [round % 2 == 0, round % 2 != 0] {
                let start = now();
                let mut sum = 0u64;
                for n in 0..1_000_000 {
                    let key = &keys[(n * 2 + usize::from(text != "a")) % keys.len()];
                    sum += u64::from(
                        if new_first {
                            current.lookup(std::hint::black_box(text), std::hint::black_box(key))
                        } else {
                            previous.lookup(std::hint::black_box(text), std::hint::black_box(key))
                        }
                        .unwrap(),
                    );
                }
                std::hint::black_box(sum);
                let elapsed = now() - start;
                if new_first {
                    new.push(elapsed);
                } else {
                    old.push(elapsed);
                }
            }
        }
        old.sort_by(f64::total_cmp);
        new.sort_by(f64::total_cmp);
        let line = format!(
            "full8192 lookup1000000 text={text:?} original_ms={} shared_ms={} ratio={}",
            old[4],
            new[4],
            new[4] / old[4]
        );
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_test::console_log!("{}", line);
        #[cfg(not(target_arch = "wasm32"))]
        println!("{}", line);
    }
}
#[cfg(target_arch = "wasm32")]
fn now() -> f64 {
    js_sys::Date::now()
}
#[cfg(not(target_arch = "wasm32"))]
fn now() -> f64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
        * 1000.
}
