const LOOP_CUES: &[&str] = &[
    "loading",
    "processing",
    "recording",
    "connecting",
    "scanning",
    "streaming",
];

pub const CATALOG: &[(&str, &str)] = &[
    ("focus-start", "open"),
    ("focus-resume", "open"),
    ("focus-pause", "close"),
    ("focus-complete", "complete"),
    ("break-complete", "success"),
    ("level-up", "level-up"),
    ("attention", "notification"),
    ("permission", "notification"),
    ("error", "error"),
    ("warning", "warning"),
    ("success", "success"),
    ("complete", "complete"),
];

// UISFX 0.4.0 createRecipe("arcade", cue), pinned from the vendored source.
// Float32 writes deliberately round at the same points as renderRecipe.
use serde_json::Value;
fn number(v: &Value, k: &str, default: f64) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(default)
}
fn hash(s: &str) -> u32 {
    s.encode_utf16().fold(2_166_136_261, |a, c| {
        (a ^ u32::from(c)).wrapping_mul(16_777_619)
    })
}
fn random(seed: &mut u32) -> f64 {
    *seed = seed.wrapping_add(1_831_565_813);
    let mut n = *seed;
    n = (n ^ (n >> 15)).wrapping_mul(n | 1);
    n ^= n.wrapping_add((n ^ (n >> 7)).wrapping_mul(n | 61));
    f64::from(n ^ (n >> 14)) / 4_294_967_296.0
}
fn blep(phase: f64, step: f64) -> f64 {
    if step <= 0.0 || step >= 0.5 {
        return 0.0;
    }
    if phase < step {
        let x = phase / step;
        return x + x - x * x - 1.0;
    }
    if phase > 1.0 - step {
        let x = (phase - 1.0) / step;
        return x * x + x + x + 1.0;
    }
    0.0
}
fn square(phase: f64, step: f64) -> f64 {
    let cycle = phase / std::f64::consts::TAU;
    let p = cycle - cycle.floor();
    (if p < 0.5 { 1.0 } else { -1.0 }) + blep(p, step) - blep((p + 0.5) % 1.0, step)
}
fn envelope(t: f64, length: f64, attack: f64, decay: f64) -> f64 {
    if t < 0.0 || t > length {
        return 0.0;
    }
    if t < attack {
        return t / attack.max(0.0001);
    }
    (1.0 - (t - attack) / (length - attack).max(0.0001))
        .max(0.0)
        .powf(decay)
}
pub fn recipe(name: &str) -> Option<Value> {
    pack_recipe("arcade", name)
}
fn recipes() -> &'static Value {
    static RECIPES: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    RECIPES.get_or_init(|| {
        serde_json::from_str(include_str!("audio_recipes.json")).unwrap_or(Value::Null)
    })
}
pub fn pack_recipe(pack: &str, name: &str) -> Option<Value> {
    let resolved = CATALOG
        .iter()
        .find_map(|(public, engine)| (*public == name).then_some(*engine))
        .unwrap_or(name);
    if LOOP_CUES.contains(&resolved) {
        return None;
    }
    recipes().get("packs")?.get(pack)?.get(resolved).cloned()
}
pub fn cue_names() -> Vec<String> {
    recipes()
        .get("packs")
        .and_then(|v| v.get("arcade"))
        .and_then(Value::as_object)
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}
pub fn render_cue(name: &str, sample_rate: f32) -> Vec<f32> {
    render_pack_cue("arcade", name, sample_rate).0
}
pub fn render_pack_cue(pack: &str, name: &str, sample_rate: f32) -> (Vec<f32>, Vec<f32>) {
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return (Vec::new(), Vec::new());
    }
    let Some(recipe) = pack_recipe(pack, name) else {
        return (Vec::new(), Vec::new());
    };
    render_stereo_recipe(&recipe, f64::from(sample_rate))
}
pub fn render_recipe(r: &Value, rate: f64) -> Vec<f32> {
    render_stereo_recipe(r, rate).0
}
fn wave(phase: f64, name: &str, step: f64) -> f64 {
    let cycle = phase / std::f64::consts::TAU;
    let p = cycle - cycle.floor();
    match name {
        "square" => square(phase, step),
        "saw" => 2.0 * p - 1.0 - blep(p, step),
        "triangle" => 2.0 * (2.0 * (cycle - (cycle + 0.5).floor())).abs() - 1.0,
        _ => phase.sin(),
    }
}
/// Literal UISFX 0.4.0 renderRecipe one-shot algorithm, stereo and all packs.
/// Float32 channel writes deliberately round at the same points as JS.
pub fn render_stereo_recipe(r: &Value, rate: f64) -> (Vec<f32>, Vec<f32>) {
    if !rate.is_finite() || rate <= 0.0 {
        return (Vec::new(), Vec::new());
    }
    let Some(notes) = r.get("notes").and_then(Value::as_array) else {
        return (Vec::new(), Vec::new());
    };
    struct Note {
        at: f64,
        length: f64,
        frequency: f64,
        end_frequency: f64,
        gain: f64,
    }
    let notes = notes
        .iter()
        .map(|n| Note {
            at: number(n, "at", 0.0),
            length: number(n, "length", 0.0),
            frequency: number(n, "frequency", 0.0),
            end_frequency: number(n, "endFrequency", 0.0),
            gain: number(n, "gain", 1.0),
        })
        .collect::<Vec<_>>();
    let end = notes.iter().fold(0.0_f64, |m, n| m.max(n.at + n.length));
    let cue = r.get("cue").and_then(Value::as_str).unwrap_or("");
    let pack = r.get("pack").and_then(Value::as_str).unwrap_or("arcade");
    let zen = pack == "zen";
    let echo = number(r, "echo", 0.0);
    let duration = number(r, "duration", 0.0);
    let padding = if cue == "typing" { 0.048 } else { 0.024 };
    let tail = if zen { 0.12 } else { 0.06 };
    let length = ((duration.min(end + tail) + padding + echo * 1.6) * rate)
        .round()
        .max(1.0) as usize;
    let mut left = vec![0.0_f32; length];
    let mut right = vec![0.0_f32; length];
    let mut phases = vec![0.0_f64; notes.len()];
    let mut seed = hash(&format!("{pack}:{cue}"));
    let mut materials_seed = hash(&format!("{pack}:{cue}:materials"));
    let mut low = 0.0;
    let mut material_x = 0.0;
    let mut material_w = 0.0;
    let mut material_b = 0.0;
    let attack = number(r, "attack", 0.002);
    let decay = number(r, "decay", 1.1);
    let bright = number(r, "brightness", 0.72);
    let filter = 1.0 - (-std::f64::consts::TAU * (520.0 + bright * 4800.0) / rate).exp();
    let waveform = r.get("waveform").and_then(Value::as_str).unwrap_or("sine");
    let harmonics = r
        .get("harmonics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let r_elasticity = number(r, "elasticity", 0.0);
    let r_fmdepth = number(r, "fmDepth", 0.0);
    let r_fmratio = number(r, "fmRatio", 1.0);
    let r_paper = number(r, "paper", 0.0);
    let r_brush = number(r, "brush", 0.0);
    let r_wood = number(r, "wood", 0.0);
    let r_chime = number(r, "chime", 0.0);
    let r_noise = number(r, "noise", 0.0);
    let r_transient = number(r, "transient", 0.0);
    let r_bitdepth = number(r, "bitDepth", 16.0);
    let r_panfrom = number(r, "panFrom", 0.0);
    let r_panto = number(r, "panTo", 0.0);
    let harmonics = harmonics
        .iter()
        .map(|h| {
            (
                h.get(0).and_then(Value::as_f64).unwrap_or(1.0),
                h.get(1).and_then(Value::as_f64).unwrap_or(1.0),
            )
        })
        .collect::<Vec<_>>();
    for (i, (lft, rgt)) in left.iter_mut().zip(&mut right).enumerate() {
        let t = i as f64 / rate;
        let mut tone = 0.0;
        let mut noise_env = 0.0;
        let mut transient_env = 0.0;
        let mut material = 0.0;
        for (n, phase) in notes.iter().zip(&mut phases) {
            let local = t - n.at;
            let len = n.length;
            if local < 0.0 || local > len {
                continue;
            }
            let freq = n.frequency * (n.end_frequency / n.frequency).powf(local / len);
            let elastic =
                r_elasticity * (-local * 19.0).exp() * (std::f64::consts::TAU * 12.5 * local).cos();
            let freq = freq * 2.0_f64.powf(elastic / 12.0);
            *phase += std::f64::consts::TAU * freq / rate;
            let fm = if r_fmdepth > 0.0 {
                (*phase * r_fmratio).sin() * r_fmdepth * (-local * 7.5).exp()
            } else {
                0.0
            };
            let mut harmonic = 0.0;
            for h in &harmonics {
                let (ratio, gain) = *h;
                harmonic += wave(
                    *phase * ratio + fm,
                    waveform,
                    (freq * ratio / rate).min(0.49),
                ) * gain;
            }
            let gain = n.gain;
            tone += harmonic * envelope(local, len, attack, decay) * gain;
            noise_env += envelope(local, len, attack.min(0.004), 2.35_f64.max(decay * 1.25)) * gain;
            transient_env +=
                (1.0 - (-local * 800.0).exp()) * (-local * (105.0 + bright * 170.0)).exp() * gain;
        }
        let noise = random(&mut seed) * 2.0 - 1.0;
        let m = random(&mut materials_seed) * 2.0 - 1.0;
        material_x += (m - material_x) * 0.08;
        material_w += (m - material_w) * 0.018;
        material_b += (m - material_b) * 0.004;
        let paper_band = material_x - material_w;
        let brush_band = material_w - material_b;
        for n in &notes {
            let local = t - n.at;
            if local < 0.0 {
                continue;
            }
            let len = n.length;
            let freq = n.frequency;
            let paper = r_paper;
            if paper > 0.0 {
                let h = len.min(0.075).min(0.028_f64.max(len * 0.62));
                if local <= h {
                    let v = local / h;
                    let env = (std::f64::consts::PI * v).sin().powf(1.1) * (1.0 - v * 0.48);
                    let q = (-((local - h * 0.24) / 0.003).powi(2)).exp();
                    let g = (-((local - h * 0.64) / 0.005).powi(2)).exp();
                    material += paper_band * paper * env * (0.16 + q * 0.52 + g * 0.24);
                }
            }
            let brush = r_brush;
            if brush > 0.0 {
                let h = len.min(0.11).min(0.045_f64.max(len * 0.95));
                if local <= h {
                    let v = local / h;
                    material += brush_band
                        * brush
                        * (std::f64::consts::PI * v).sin().powf(1.3)
                        * (0.42 + v * 0.08);
                }
            }
            let wood = r_wood;
            if wood > 0.0 {
                let h = len.min(0.22).min(0.09_f64.max(len * 0.8));
                if local <= h {
                    let v = 1.0 - (-local * 480.0).exp();
                    let d = (-local * 18.0).exp();
                    let q = (std::f64::consts::TAU * freq * 0.31 * local).sin();
                    let g = (std::f64::consts::TAU * freq * 0.47 * local + 0.3).sin() * 0.34;
                    material += (q + g) * wood * v * d;
                }
            }
            let chime = r_chime;
            if chime > 0.0 {
                let h = len.min(0.42).min(0.16_f64.max(len * 1.25));
                if local <= h {
                    let v = 1.0 - (-local * 520.0).exp();
                    let d = (-local * 7.4).exp();
                    let q = (std::f64::consts::TAU * freq * 2.01 * local).sin()
                        + (std::f64::consts::TAU * freq * 3.87 * local + 0.4).sin() * 0.28;
                    material += q * chime * v * d;
                }
            }
        }
        low += (noise - low) * filter;
        let high = noise - low;
        let hiss = (high * (0.52 + bright * 0.28) + low * (0.18 - bright * 0.08))
            * r_noise
            * noise_env.min(1.4);
        let transient = high * r_transient * transient_env.min(1.35);
        let mut mixed = tone * (if zen { 0.58 } else { 0.62 })
            + hiss * (if zen { 0.1 } else { 0.32 })
            + transient * (if zen { 0.08 } else { 0.3 })
            + material * (if zen { 0.34 } else { 0.0 });
        let bits = r_bitdepth;
        if bits < 16.0 {
            let p = 2.0_f64.powf(bits);
            mixed = (mixed * p + 0.5).floor() / p;
        }
        let pan = (r_panfrom + (r_panto - r_panfrom) * (t / duration.max(0.001)).min(1.0) + 1.0)
            * std::f64::consts::FRAC_PI_4;
        let shaped = if zen {
            mixed
        } else {
            (mixed * 1.2).tanh() / 1.2_f64.tanh()
        };
        *lft = (shaped * pan.cos()) as f32;
        *rgt = (shaped * pan.sin()) as f32;
    }
    if echo > 0.0 {
        let delay = ((0.035 + echo * 0.38) * rate).floor() as usize;
        let gain = 0.22_f64.min(echo * 1.65);
        for channel in [&mut left, &mut right] {
            for i in delay..length {
                let prev = channel.get(i - delay).copied().unwrap_or(0.0);
                if let Some(s) = channel.get_mut(i) {
                    *s = (f64::from(*s) + f64::from(prev) * gain) as f32;
                }
            }
        }
    }
    let fade = length.min((0.028 * rate).round() as usize);
    for channel in [&mut left, &mut right] {
        for (i, s) in channel.iter_mut().enumerate().skip(length - fade) {
            let x = (length - 1 - i) as f64 / fade.saturating_sub(1).max(1) as f64;
            *s = (f64::from(*s) * (x * std::f64::consts::FRAC_PI_2).sin().powi(2)) as f32;
        }
    }
    let peak = left
        .iter()
        .chain(&right)
        .fold(0.0_f64, |p, s| p.max(f64::from(s.abs())));
    let limit = if cue == "typing" {
        if zen { 0.15 } else { 0.28 }
    } else if zen {
        if cue == "hover" { 0.15 } else { 0.28 }
    } else if cue == "hover" {
        0.3
    } else {
        0.42
    };
    let gain = if peak > 0.0 {
        2.2_f64.min(limit / peak)
    } else {
        1.0
    };
    for s in left.iter_mut().chain(&mut right) {
        *s = (f64::from(*s) * gain) as f32;
    }
    (left, right)
}
