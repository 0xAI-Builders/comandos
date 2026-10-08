//! Reloj de sprites compartido con el tablero, con tiempo explícito.
use serde_json::Value;
pub fn hourglass_sheet_frames(width: u32, height: u32, _scale: f64) -> Vec<(u32, u32, u32, u32)> {
    (0..(width / 32).max(1))
        .map(|i| (i * 32, 0, 32, height.min(32)))
        .collect()
}
pub fn hourglass_frame(_block: &Value, now_ms: f64, flip_start_ms: Option<f64>) -> usize {
    if let Some(start) = flip_start_ms {
        let dt = now_ms - start;
        if (0.0..660.0).contains(&dt) {
            return 21 + (dt / 110.).floor() as usize;
        }
    }
    let t = now_ms.rem_euclid(8010.);
    if t < 7350. {
        (t / 350.).floor() as usize
    } else {
        21 + ((t - 7350.) / 110.).floor() as usize
    }
}
#[derive(Default, Debug)]
pub struct Hourglass {
    pub block: Value,
    pub offset: f64,
    pub flip: Option<f64>,
    seen: Option<Value>,
}
impl Hourglass {
    pub fn adopt(&mut self, data: &Value, now: f64) {
        let block = data
            .get("block")
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or(Value::Null);
        self.offset = data
            .get("serverNowMs")
            .and_then(Value::as_f64)
            .filter(|v| *v != 0.)
            .unwrap_or(now)
            .trunc()
            - now.trunc();
        if comandos_core::json::truthy(&block) {
            let id = block.get("blockId").cloned().unwrap_or(Value::Null);
            if block.get("status").and_then(Value::as_str) == Some("completed") {
                if self.seen.as_ref().is_some_and(|old| old != &id) {
                    self.flip = Some(now.trunc() + self.offset);
                }
                self.seen = Some(id);
            } else if self.seen.is_none() {
                self.seen = Some(Value::String(String::new()));
            }
        }
        self.block = block;
    }
}

/// Un único juego de cuadros por DPR, reemplazado al cambiar de pantalla.
#[derive(Default)]
pub struct Sprites {
    scale: i32,
    frames: Vec<gdk_pixbuf::Pixbuf>,
    dim: Vec<gdk_pixbuf::Pixbuf>,
}
impl Sprites {
    pub fn len(&self) -> usize {
        self.frames.len()
    }
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
    pub fn load(&mut self, path: &str, scale: i32) -> Result<(), String> {
        let scale = scale.max(1);
        if self.scale == scale && !self.frames.is_empty() {
            return Ok(());
        }
        let sheet = gdk_pixbuf::Pixbuf::from_file(path).map_err(|e| e.to_string())?;
        let mut frames = Vec::new();
        let mut dim = Vec::new();
        let px = 24 * scale;
        for (x, y, w, h) in
            hourglass_sheet_frames(sheet.width() as u32, sheet.height() as u32, 0.75)
        {
            if x + w > sheet.width() as u32 || h == 0 {
                return Err("Spritesheet incompleto".into());
            }
            let pb = sheet
                .new_subpixbuf(x as i32, y as i32, w as i32, h as i32)
                .scale_simple(px, px, gdk_pixbuf::InterpType::Nearest)
                .ok_or("No se pudo escalar el reloj")?;
            let faded = pb.copy().ok_or("No se pudo copiar el reloj")?;
            pb.saturate_and_pixelate(&faded, 0.45, false);
            frames.push(pb);
            dim.push(faded);
        }
        self.frames = frames;
        self.dim = dim;
        self.scale = scale;
        Ok(())
    }
    pub fn frame(&self, index: usize, paused: bool) -> Option<gdk_pixbuf::Pixbuf> {
        let frames = if paused { &self.dim } else { &self.frames };
        frames
            .get(index.min(frames.len().saturating_sub(1)))
            .cloned()
    }
}
