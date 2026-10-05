use xtask::png_diff::{Rect, Rgba, crop, decode, diff, encode, mask, write_diff_png};

fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Rgba {
    Rgba {
        width: w,
        height: h,
        pixels: rgba.repeat((w * h) as usize),
    }
}

#[test]
fn identical_images_have_zero_ratio() {
    let a = solid(4, 4, [10, 20, 30, 255]);
    let stats = diff(&a, &a.clone(), 24).unwrap();
    assert_eq!((stats.differing, stats.total), (0, 16));
    assert_eq!(stats.ratio(), 0.0);
}

#[test]
fn channel_threshold_matches_png_diff_py() {
    // tools/png_diff.py: diferencia por canal convertida a gris, > 24 cuenta.
    let a = solid(2, 1, [100, 100, 100, 255]);
    let mut b = a.clone();
    b.pixels[0] = 124; // Δ=24 en R → gris ≈ 7: no cuenta
    b.pixels[4] = 220; // Δ=120 en R del segundo píxel → gris ≈ 36: cuenta
    let stats = diff(&a, &b, 24).unwrap();
    assert_eq!(stats.differing, 1);
    assert_eq!(stats.ratio(), 0.5);
}

#[test]
fn gray_rounds_like_pil() {
    // PIL "L": (R*19595 + G*38470 + B*7471 + 0x8000) >> 16. Δ=(0,43,0) → 25.24 → 25 > 24.
    let a = solid(1, 1, [0, 0, 0, 255]);
    let b = solid(1, 1, [0, 43, 0, 255]);
    assert_eq!(diff(&a, &b, 24).unwrap().differing, 1);
    // Δ=(0,42,0) → 24.65 → redondea a 25 en PIL (no trunca a 24).
    let c = solid(1, 1, [0, 42, 0, 255]);
    assert_eq!(diff(&a, &c, 24).unwrap().differing, 1);
    // Δ=(0,41,0) → 24.07 → 24: no cuenta.
    let d = solid(1, 1, [0, 41, 0, 255]);
    assert_eq!(diff(&a, &d, 24).unwrap().differing, 0);
}

#[test]
fn crop_and_mask_bound_the_comparison() {
    let mut a = solid(10, 10, [0, 0, 0, 255]);
    let mut b = a.clone();
    b.pixels[0] = 255; // (0,0) distinto, fuera del recorte
    let r = Rect {
        x: 2,
        y: 2,
        w: 4,
        h: 4,
    };
    assert_eq!(
        diff(&crop(&a, r).unwrap(), &crop(&b, r).unwrap(), 24)
            .unwrap()
            .differing,
        0
    );
    let full = Rect {
        x: 0,
        y: 0,
        w: 10,
        h: 10,
    };
    mask(
        &mut a,
        Rect {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        },
    );
    mask(
        &mut b,
        Rect {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        },
    );
    assert_eq!(
        diff(&crop(&a, full).unwrap(), &crop(&b, full).unwrap(), 24)
            .unwrap()
            .differing,
        0
    );
    assert!(
        crop(
            &a,
            Rect {
                x: 8,
                y: 8,
                w: 4,
                h: 4
            }
        )
        .is_err()
    );
}

#[test]
fn mask_out_of_bounds_is_clipped() {
    let mut a = solid(3, 3, [0, 0, 0, 255]);
    mask(
        &mut a,
        Rect {
            x: 2,
            y: 2,
            w: 50,
            h: 50,
        },
    );
    assert_eq!(&a.pixels[(2 * 3 + 2) * 4..], &[255, 0, 255, 255]);
    assert_eq!(&a.pixels[..4], &[0, 0, 0, 255]);
}

#[test]
fn size_mismatch_is_an_error() {
    assert!(diff(&solid(2, 2, [0; 4]), &solid(3, 2, [0; 4]), 24).is_err());
}

#[test]
fn encode_decode_round_trip_and_rgb_gets_alpha() {
    let img = Rgba {
        width: 2,
        height: 1,
        pixels: vec![1, 2, 3, 255, 4, 5, 6, 128],
    };
    assert_eq!(decode(&encode(&img).unwrap()).unwrap(), img);
    // Las capturas de Chrome llegan en RGB sin alfa: decode las expande a RGBA opaco.
    let mut rgb = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut rgb, 1, 1);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[9, 8, 7]).unwrap();
    }
    assert_eq!(decode(&rgb).unwrap().pixels, vec![9, 8, 7, 255]);
}

#[test]
fn diff_png_marks_differences_in_red() {
    let a = solid(2, 1, [0, 0, 0, 255]);
    let mut b = a.clone();
    b.pixels[4] = 255;
    let out = std::env::temp_dir().join(format!("xtask-png-diff-{}.png", std::process::id()));
    write_diff_png(&a, &b, 24, &out).unwrap();
    let got = decode(&std::fs::read(&out).unwrap()).unwrap();
    let _ = std::fs::remove_file(&out);
    assert_eq!(got.pixels, vec![0, 0, 0, 255, 255, 0, 0, 255]);
}

fn png_bytes(w: u32, h: u32, color: png::ColorType, depth: png::BitDepth, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(color);
        enc.set_depth(depth);
        let mut wr = enc.write_header().unwrap();
        wr.write_image_data(data).unwrap();
    }
    out
}

#[test]
fn grey_pngs_become_rgba_like_pillow_convert_rgb() {
    let g = png_bytes(
        2,
        1,
        png::ColorType::Grayscale,
        png::BitDepth::Eight,
        &[10, 200],
    );
    assert_eq!(
        decode(&g).unwrap().pixels,
        vec![10, 10, 10, 255, 200, 200, 200, 255]
    );
    let ga = png_bytes(
        1,
        1,
        png::ColorType::GrayscaleAlpha,
        png::BitDepth::Eight,
        &[7, 9],
    );
    assert_eq!(decode(&ga).unwrap().pixels, vec![7, 7, 7, 9]);
}

#[test]
fn sixteen_bit_pngs_keep_the_high_byte_like_pillow() {
    // Pillow abre RGB de 16 bits como "RGB" con el byte alto (rawmode RGB;16B).
    let rgb16 = png_bytes(
        1,
        1,
        png::ColorType::Rgb,
        png::BitDepth::Sixteen,
        &[0x12, 0x34, 0xab, 0xcd, 0xff, 0x00],
    );
    assert_eq!(decode(&rgb16).unwrap().pixels, vec![0x12, 0xab, 0xff, 255]);
    let rgba16 = png_bytes(
        1,
        1,
        png::ColorType::Rgba,
        png::BitDepth::Sixteen,
        &[1, 0, 2, 0, 3, 0, 4, 0],
    );
    assert_eq!(decode(&rgba16).unwrap().pixels, vec![1, 2, 3, 4]);
    // Gris de 16 bits: Pillow 9 lo abre en modo "I" y convert("RGB") recorta a 0..=255
    // (comprobado con Pillow 9.0.1: 0x0080 → 128, 0x1234 → 255).
    let g16 = png_bytes(
        2,
        1,
        png::ColorType::Grayscale,
        png::BitDepth::Sixteen,
        &[0x00, 0x80, 0x12, 0x34],
    );
    assert_eq!(
        decode(&g16).unwrap().pixels,
        vec![128, 128, 128, 255, 255, 255, 255, 255]
    );
}
