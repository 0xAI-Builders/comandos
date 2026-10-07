//! Fixed key equality and bounded FIFO remain those of the original tuple.
#[path = "../src/canvas/pattern_key.rs"]
mod pattern_key;
use comandos_term_web::canvas::Bounded;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn identity_color_and_fifo_match_the_original_tuple() {
    let masks = [0, 1, 2, usize::MAX, usize::MAX - 1, 0x12345678, 0x80000000];
    let colors = [
        [0; 3],
        [255; 3],
        [0, 0, 1],
        [0, 1, 0],
        [1, 0, 0],
        [127, 128, 255],
    ];
    for a in masks {
        for ca in colors {
            for b in masks {
                for cb in colors {
                    assert_eq!(
                        pattern_key::key(a, ca) == pattern_key::key(b, cb),
                        (a, ca) == (b, cb)
                    );
                }
            }
        }
    }
    let mut old = Bounded::new(64);
    let mut current = Bounded::new(64);
    let mut seed = 0x12a5_8877_6644_e321u64;
    let mut keys = Vec::new();
    for n in 0..4096u64 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let bytes = seed.to_le_bytes();
        let mask = masks[n as usize % masks.len()];
        let color = [bytes[0], bytes[1], bytes[2]];
        old.insert((mask, color), n);
        current.insert(pattern_key::key(mask, color), n);
        keys.push((mask, color));
        // Updating an existing key must not move its FIFO eviction order.
        if n % 3 == 0 {
            let (m, c) = keys[n as usize / 2];
            old.insert((m, c), n + 1);
            current.insert(pattern_key::key(m, c), n + 1);
        }
        for &(m, c) in &keys {
            assert_eq!(current.get(&pattern_key::key(m, c)), old.get(&(m, c)));
        }
        assert_eq!(current.len(), old.len());
        assert_eq!(current.is_empty(), old.is_empty());
    }
    current.clear();
    old.clear();
    assert_eq!(current.len(), old.len());
}
