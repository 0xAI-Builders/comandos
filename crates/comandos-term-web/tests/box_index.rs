//! Finite cache keys cover every supported Unicode scalar and original op.
#[path = "../src/canvas/box_index.rs"]
mod box_index;
use box_index::BoxIndex;
use comandos_term::glyphs::{self, DrawOp};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn all_supported_glyphs_keep_operations_reuse_and_geometry_reset() {
    for (width, height, dpr, font) in [
        (8., 16., 1., 14.),
        (13., 29., 1.25, 15.),
        (30., 64., 2., 28.),
    ] {
        let metrics = glyphs::CellMetrics {
            cell_w: width,
            cell_h: height,
            dpr,
            font_size: font,
        };
        let mut index = BoxIndex::<Vec<DrawOp>>::default();
        let mut cached = 0;
        for cp in 0..=0x10ffff {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            if glyphs::box_ops(c).is_none() {
                continue;
            }
            let mut expected = Vec::new();
            glyphs::draw_ops(c, &metrics, &mut expected);
            assert!(!expected.is_empty());
            let current = index.entry(c, || {
                cached += 1;
                expected.clone()
            });
            assert_eq!(current, &expected, "glyph {cp:x}");
            assert_eq!(
                index.entry(c, || panic!("cached glyph recomputed")),
                &expected
            );
        }
        assert_eq!(cached, 207);
        for absent in ['a', '😀', '\0', '\u{1fb8c}'] {
            let mut expected = Vec::new();
            glyphs::draw_ops(absent, &metrics, &mut expected);
            assert_eq!(index.entry(absent, || expected.clone()), &expected);
        }
    }
}
