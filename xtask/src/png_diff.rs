//! Comparación de capturas: sustituye tools/png_diff.py con la misma regla
//! (gris de la diferencia por canal > umbral) y recortes por componente.
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Imagen RGBA de 8 bits por canal, filas contiguas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffStats {
    pub differing: u64,
    pub total: u64,
}

impl DiffStats {
    pub fn ratio(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.differing as f64 / self.total as f64
        }
    }
}

/// Decodifica un PNG a RGBA 8 bits; RGB, gris y paleta se expanden con alfa
/// opaco (las capturas de Chrome llegan en RGB).
pub fn decode(bytes: &[u8]) -> Result<Rgba, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().map_err(|e| format!("png: {e}"))?;
    let size = reader
        .output_buffer_size()
        .ok_or("png: tamaño de salida desconocido")?;
    let mut buf = vec![0; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("png: {e}"))?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "png: formato {:?}/{:?} no soportado",
            info.color_type, info.bit_depth
        ));
    }
    buf.truncate(info.buffer_size());
    Ok(Rgba {
        width: info.width,
        height: info.height,
        pixels: buf,
    })
}

/// Codifica RGBA 8 bits a PNG.
pub fn encode(img: &Rgba) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.width, img.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| format!("png: {e}"))?;
        w.write_image_data(&img.pixels)
            .map_err(|e| format!("png: {e}"))?;
    }
    Ok(out)
}

/// Escribe `img` como PNG en `out`.
pub fn write_png(img: &Rgba, out: &Path) -> Result<(), String> {
    let bytes = encode(img)?;
    std::fs::write(out, bytes).map_err(|e| format!("{}: {e}", out.display()))
}

fn offset(img: &Rgba, x: u32, y: u32) -> usize {
    ((y as usize) * (img.width as usize) + x as usize) * 4
}

/// Recorte exacto; error si el rectángulo se sale de la imagen.
pub fn crop(img: &Rgba, r: Rect) -> Result<Rgba, String> {
    if r.x.saturating_add(r.w) > img.width || r.y.saturating_add(r.h) > img.height {
        return Err(format!(
            "recorte {r:?} fuera de {}x{}",
            img.width, img.height
        ));
    }
    let row_len = (r.w as usize) * 4;
    let mut pixels = Vec::with_capacity(row_len * r.h as usize);
    for y in r.y..r.y + r.h {
        let start = offset(img, r.x, y);
        let row = img
            .pixels
            .get(start..start + row_len)
            .ok_or("recorte: fila corta")?;
        pixels.extend_from_slice(row);
    }
    Ok(Rgba {
        width: r.w,
        height: r.h,
        pixels,
    })
}

/// Pinta el rectángulo de magenta opaco: regiones volátiles (relojes,
/// contadores). Lo que cae fuera de la imagen se ignora.
pub fn mask(img: &mut Rgba, r: Rect) {
    let x_end = r.x.saturating_add(r.w).min(img.width);
    let y_end = r.y.saturating_add(r.h).min(img.height);
    for y in r.y.min(y_end)..y_end {
        for x in r.x.min(x_end)..x_end {
            let at = offset(img, x, y);
            if let Some(px) = img.pixels.get_mut(at..at + 4) {
                px.copy_from_slice(&[255, 0, 255, 255]);
            }
        }
    }
}

/// `ImageChops.difference(...).convert("L")` de PIL: luma ITU-R 601-2 en
/// punto fijo con redondeo, sobre |Δ| por canal (el alfa no cuenta).
fn gray_delta(a: &[u8], b: &[u8]) -> u32 {
    let d = |i: usize| {
        u32::from(
            a.get(i)
                .copied()
                .unwrap_or(0)
                .abs_diff(b.get(i).copied().unwrap_or(0)),
        )
    };
    (d(0) * 19595 + d(1) * 38470 + d(2) * 7471 + 0x8000) >> 16
}

/// Cuenta los píxeles cuyo gris de la diferencia supera `channel`.
pub fn diff(a: &Rgba, b: &Rgba, channel: u8) -> Result<DiffStats, String> {
    if (a.width, a.height) != (b.width, b.height) {
        return Err(format!(
            "tamaños distintos: {}x{} vs {}x{}",
            a.width, a.height, b.width, b.height
        ));
    }
    let differing = a
        .pixels
        .chunks_exact(4)
        .zip(b.pixels.chunks_exact(4))
        .filter(|(pa, pb)| gray_delta(pa, pb) > u32::from(channel))
        .count() as u64;
    Ok(DiffStats {
        differing,
        total: u64::from(a.width) * u64::from(a.height),
    })
}

/// Copia de `a` con los píxeles distintos en rojo, como png_diff.py.
pub fn write_diff_png(a: &Rgba, b: &Rgba, channel: u8, out: &Path) -> Result<(), String> {
    diff(a, b, channel)?;
    let mut pixels = a.pixels.clone();
    for (i, (pa, pb)) in a
        .pixels
        .chunks_exact(4)
        .zip(b.pixels.chunks_exact(4))
        .enumerate()
    {
        if gray_delta(pa, pb) > u32::from(channel)
            && let Some(px) = pixels.get_mut(i * 4..i * 4 + 4)
        {
            px.copy_from_slice(&[255, 0, 0, 255]);
        }
    }
    write_png(
        &Rgba {
            width: a.width,
            height: a.height,
            pixels,
        },
        out,
    )
}
