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

#[derive(Clone, Copy)]
struct Note {
    at: f32,
    semitone: f32,
    length: f32,
    gain: f32,
    glide: f32,
}

#[derive(Clone, Copy)]
struct Cue {
    duration: f32,
    base_midi: f32,
    noise: f32,
    default_volume: f32,
    notes: &'static [Note],
}

const OPEN_NOTES: &[Note] = &[
    Note {
        at: 0.0,
        semitone: -2.0,
        length: 0.28,
        gain: 1.0,
        glide: 9.0,
    },
    Note {
        at: 0.18,
        semitone: 12.0,
        length: 0.11,
        gain: 0.3,
        glide: 0.0,
    },
];
const CLOSE_NOTES: &[Note] = &[
    Note {
        at: 0.0,
        semitone: 12.0,
        length: 0.1,
        gain: 0.34,
        glide: 0.0,
    },
    Note {
        at: 0.055,
        semitone: 5.0,
        length: 0.24,
        gain: 1.0,
        glide: -8.0,
    },
];
const COMPLETE_NOTES: &[Note] = &[
    Note {
        at: 0.0,
        semitone: 0.0,
        length: 0.22,
        gain: 1.0,
        glide: 0.0,
    },
    Note {
        at: 0.22,
        semitone: 7.0,
        length: 0.25,
        gain: 1.0,
        glide: 0.0,
    },
    Note {
        at: 0.46,
        semitone: 12.0,
        length: 0.27,
        gain: 1.0,
        glide: 0.0,
    },
];
const SUCCESS_NOTES: &[Note] = &[
    Note {
        at: 0.0,
        semitone: 0.0,
        length: 0.3,
        gain: 1.0,
        glide: 0.0,
    },
    Note {
        at: 0.16,
        semitone: 4.0,
        length: 0.32,
        gain: 1.0,
        glide: 0.0,
    },
    Note {
        at: 0.33,
        semitone: 7.0,
        length: 0.33,
        gain: 1.0,
        glide: 0.0,
    },
];
const LEVEL_UP_NOTES: &[Note] = &[
    Note {
        at: 0.0,
        semitone: 0.0,
        length: 0.24,
        gain: 1.0,
        glide: 0.0,
    },
    Note {
        at: 0.17,
        semitone: 4.0,
        length: 0.25,
        gain: 1.0,
        glide: 0.0,
    },
    Note {
        at: 0.34,
        semitone: 7.0,
        length: 0.26,
        gain: 1.0,
        glide: 0.0,
    },
    Note {
        at: 0.51,
        semitone: 12.0,
        length: 0.32,
        gain: 1.0,
        glide: 0.0,
    },
];
const NOTIFICATION_NOTES: &[Note] = &[
    Note {
        at: 0.0,
        semitone: 0.0,
        length: 0.26,
        gain: 1.0,
        glide: 0.0,
    },
    Note {
        at: 0.19,
        semitone: 5.0,
        length: 0.3,
        gain: 1.0,
        glide: 0.0,
    },
];
const ERROR_NOTES: &[Note] = &[
    Note {
        at: 0.0,
        semitone: 6.0,
        length: 0.28,
        gain: 1.0,
        glide: 0.0,
    },
    Note {
        at: 0.22,
        semitone: 0.0,
        length: 0.32,
        gain: 1.0,
        glide: -2.0,
    },
];
const WARNING_NOTES: &[Note] = &[
    Note {
        at: 0.0,
        semitone: 0.0,
        length: 0.22,
        gain: 1.0,
        glide: 0.0,
    },
    Note {
        at: 0.28,
        semitone: 0.0,
        length: 0.3,
        gain: 1.0,
        glide: 0.0,
    },
];

fn cue(name: &str) -> Option<Cue> {
    match name {
        "open" => Some(Cue {
            duration: 0.37,
            base_midi: 64.0,
            noise: 0.05,
            default_volume: 0.18,
            notes: OPEN_NOTES,
        }),
        "close" => Some(Cue {
            duration: 0.34,
            base_midi: 64.0,
            noise: 0.04,
            default_volume: 0.17,
            notes: CLOSE_NOTES,
        }),
        "complete" => Some(Cue {
            duration: 0.8,
            base_midi: 65.0,
            noise: 0.0,
            default_volume: 0.24,
            notes: COMPLETE_NOTES,
        }),
        "success" => Some(Cue {
            duration: 0.72,
            base_midi: 67.0,
            noise: 0.0,
            default_volume: 0.23,
            notes: SUCCESS_NOTES,
        }),
        "level-up" => Some(Cue {
            duration: 0.92,
            base_midi: 64.0,
            noise: 0.0,
            default_volume: 0.25,
            notes: LEVEL_UP_NOTES,
        }),
        "notification" => Some(Cue {
            duration: 0.58,
            base_midi: 72.0,
            noise: 0.0,
            default_volume: 0.20,
            notes: NOTIFICATION_NOTES,
        }),
        "error" => Some(Cue {
            duration: 0.62,
            base_midi: 62.0,
            noise: 0.10,
            default_volume: 0.22,
            notes: ERROR_NOTES,
        }),
        "warning" => Some(Cue {
            duration: 0.68,
            base_midi: 65.0,
            noise: 0.0,
            default_volume: 0.22,
            notes: WARNING_NOTES,
        }),
        _ => None,
    }
}

fn resolve(name: &str) -> &str {
    CATALOG
        .iter()
        .find_map(|(public, engine)| (*public == name).then_some(*engine))
        .unwrap_or(name)
}

fn midi_to_hz(midi: f32) -> f32 {
    440.0 * 2.0_f32.powf((midi - 69.0) / 12.0)
}

fn env(t: f32, len: f32) -> f32 {
    if !(0.0..=len).contains(&t) {
        return 0.0;
    }
    let attack = 0.002_f32.max(len * 0.02);
    if t < attack {
        return t / attack;
    }
    let tail = 1.0 - ((t - attack) / (len - attack).max(0.001));
    tail.max(0.0).powf(1.1)
}

fn noise(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    ((*seed >> 8) as f32 / 16_777_215.0) * 2.0 - 1.0
}

pub fn render_cue(name: &str, sample_rate: f32) -> Vec<f32> {
    let resolved = resolve(name);
    if LOOP_CUES.contains(&resolved) || sample_rate <= 0.0 {
        return Vec::new();
    }
    let Some(cue) = cue(resolved) else {
        return Vec::new();
    };
    let len = ((cue.duration + 0.06) * sample_rate).round().max(1.0) as usize;
    let mut out = vec![0.0; len];
    let mut seed = 2_166_136_261u32;
    for (i, sample) in out.iter_mut().enumerate() {
        let t = i as f32 / sample_rate;
        let mut v = 0.0;
        for note in cue.notes {
            let local = t - note.at;
            let amp = env(local, note.length);
            if amp <= 0.0 {
                continue;
            }
            let ratio = (local / note.length.max(0.001)).clamp(0.0, 1.0);
            let midi = cue.base_midi + note.semitone + note.glide * ratio;
            let phase = core::f32::consts::TAU * midi_to_hz(midi) * local;
            v += phase.sin() * amp * note.gain;
        }
        if cue.noise > 0.0 {
            v += noise(&mut seed) * cue.noise * 0.2;
        }
        *sample = (v * cue.default_volume).clamp(-1.0, 1.0);
    }
    let peak = out.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
    if peak > 0.42 {
        for sample in &mut out {
            *sample *= 0.42 / peak;
        }
    }
    out
}
