//! Portable rendering for human marks and AI status dots. Callers supply time;
//! this module never schedules animation or changes the stored human mark.
use crate::json::{python_eq, truthy};
use serde_json::Value;

pub const MARKS: [&str; 4] = ["none", "resolved", "frozen", "awaiting_reply"];

pub const FRAME_SECONDS: f64 = 0.08;
pub const MAX_FRAMES: usize = 48;
pub const AI_DOT_PIXELS: u32 = 12;
pub const AI_PULSE_SECONDS: f64 = 1.4;
pub const AI_PULSE_FRAMES: usize = 14;

pub fn label(name: &str, english: bool) -> Option<&'static str> {
    let pair = match name {
        "none" => ("Sin marca", "No mark"),
        "resolved" => ("Resuelto", "Resolved"),
        "frozen" => ("Congelado", "Frozen"),
        "awaiting_reply" => ("Esperando respuesta", "Awaiting reply"),
        "favorite" => ("Favorito", "Favorite"),
        "working" => ("Trabajando", "Working"),
        _ => return None,
    };
    Some(if english { pair.1 } else { pair.0 })
}

pub fn icon_color(name: &str) -> &'static str {
    match name {
        "resolved" => "#2EE59D",
        "frozen" => "#7CC4FF",
        "awaiting_reply" | "favorite" => "#FFAE1A",
        "working" => "#7AA5FF",
        _ => "#5E6980",
    }
}

fn icon_body(name: &str) -> &'static str {
    match name {
        "resolved" => {
            r#"<circle cx="12" cy="12" r="9"/><path class="wm-draw" pathLength="1" d="m7.5 12.4 3 3 6-6.6"/>"#
        }
        "frozen" => {
            r#"<g class="wm-spin"><path d="M12 3v18M4.2 7.5l15.6 9M4.2 16.5l15.6-9"/><path d="m9.5 4.5 2.5 2 2.5-2M9.5 19.5l2.5-2 2.5 2"/></g>"#
        }
        "awaiting_reply" => {
            r#"<path d="M4 5.5h16v10H10l-4.5 3.8V15.5H4z"/><circle class="wm-dot wm-d1" cx="8.5" cy="10.5" r="1.1"/><circle class="wm-dot wm-d2" cx="12" cy="10.5" r="1.1"/><circle class="wm-dot wm-d3" cx="15.5" cy="10.5" r="1.1"/>"#
        }
        "working" => {
            r#"<circle class="wm-track" cx="12" cy="12" r="8"/><path class="wm-spin" d="M12 4a8 8 0 0 1 8 8"/>"#
        }
        "favorite" => {
            r#"<path class="wm-twinkle" d="m12 3.6 2.6 5.3 5.8.8-4.2 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.2-4.1 5.8-.8z"/>"#
        }
        _ => r#"<circle class="wm-ring" cx="12" cy="12" r="6.5"/>"#,
    }
}

pub fn cycle(name: &str) -> f64 {
    match name {
        "working" => 1.4,
        "frozen" => 9.0,
        "resolved" => 2.6,
        "awaiting_reply" => 1.5,
        "favorite" => 2.4,
        _ => 0.0,
    }
}

pub fn frame_count(name: &str) -> usize {
    let seconds = cycle(name);
    if seconds == 0.0 {
        1
    } else {
        ((seconds / FRAME_SECONDS).round_ties_even() as usize).clamp(8, MAX_FRAMES)
    }
}

// Python's remainder has the divisor's sign (including positive zero). The
// rounded result may equal the divisor for a very small negative numerator.
fn remainder(value: f64, divisor: f64) -> f64 {
    let result = value % divisor;
    if result < 0.0 {
        result + divisor
    } else if result == 0.0 {
        0.0
    } else {
        result
    }
}

fn clock_frame(seconds: f64, cycle: f64, frames: usize) -> Result<usize, &'static str> {
    if cycle == 0.0 {
        return Ok(0);
    }
    if !seconds.is_finite() {
        return Err("nonfinite animation clock");
    }
    Ok(((remainder(seconds, cycle) / cycle * frames as f64) as usize) % frames)
}

pub fn frame_index(name: &str, seconds: f64) -> Result<usize, &'static str> {
    clock_frame(seconds, cycle(name), frame_count(name))
}

fn ease(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn fixed(value: f64, digits: usize) -> String {
    // Python formats nonfinite phase results in lower case.
    if value.is_nan() {
        "nan".into()
    } else {
        format!("{value:.digits$}")
    }
}

fn phase_body(name: &str, body: &str, phase: f64) -> String {
    let p = remainder(phase, 1.0);
    match name {
        "working" | "frozen" => body.replacen(
            "class=\"wm-spin\"",
            &format!("transform=\"rotate({} 12 12)\"", fixed(p * 360.0, 1)),
            1,
        ),
        "resolved" => {
            let off = if p < 0.4 {
                1.0 - ease(p / 0.4)
            } else if p < 0.8 {
                0.0
            } else {
                -ease((p - 0.8) / 0.2)
            };
            body.replacen(
                "class=\"wm-draw\"",
                &format!(
                    "stroke-dasharray=\"1\" stroke-dashoffset=\"{}\"",
                    fixed(off, 3)
                ),
                1,
            )
        }
        "awaiting_reply" => {
            let mut out = body.to_owned();
            for i in 0..3 {
                let q = remainder(p - i as f64 * 0.2 / cycle(name), 1.0);
                let alpha = 0.25
                    + 0.75
                        * if q < 0.4 {
                            ease(q / 0.4)
                        } else {
                            1.0 - ease((q - 0.4) / 0.6)
                        };
                out = out.replacen(
                    &format!("class=\"wm-dot wm-d{}\"", i + 1),
                    &format!("opacity=\"{}\"", fixed(alpha, 3)),
                    1,
                );
            }
            out
        }
        "favorite" => {
            let k = ease(if p < 0.5 { p * 2.0 } else { 2.0 - p * 2.0 });
            let (scale, alpha) = (1.0 - 0.18 * k, 1.0 - 0.3 * k);
            body.replacen(
                "class=\"wm-twinkle\"",
                &format!(
                    "transform=\"translate(12 12) scale({}) translate(-12 -12)\" opacity=\"{}\"",
                    fixed(scale, 3),
                    fixed(alpha, 3)
                ),
                1,
            )
        }
        _ => body.to_owned(),
    }
}

/// Exact standalone markup for the integer pixel sizes used by the UI.
/// An absent phase is the resting frame; the caller controls animation timing.
pub fn icon_svg(name: &str, color: Option<&str>, size: u32, phase: Option<f64>) -> String {
    let body = match phase {
        Some(p) if cycle(name) > 0.0 => phase_body(name, icon_body(name), p),
        _ => icon_body(name).to_owned(),
    };
    let color = color
        .filter(|color| !color.is_empty())
        .unwrap_or_else(|| icon_color(name));
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 24 24" fill="none" stroke="{color}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#
    )
}

pub fn sticker(name: &str) -> Option<&'static str> {
    match name {
        "frozen" => Some("Aparcado"),
        "awaiting_reply" => Some("Esperando"),
        "resolved" => Some("Hecho"),
        _ => None,
    }
}

pub fn ai_state(name: &str) -> &'static str {
    match name {
        "work" => "work",
        "need" => "need",
        "done" => "done",
        "error" => "error",
        _ => "idle",
    }
}

pub fn ai_status(status: &str) -> &'static str {
    match status {
        "working" => "work",
        "awaiting_permission" | "awaiting_input" | "waiting" => "need",
        "completed" | "done" => "done",
        "failed" | "error" => "error",
        _ => "idle",
    }
}

pub fn ai_color(name: &str) -> &'static str {
    match ai_state(name) {
        "work" => "#4ade80",
        "need" => "#f5b83d",
        "done" => "#60a5fa",
        "error" => "#f87171",
        _ => "#5d6b7e",
    }
}

pub fn ai_label(name: &str, english: bool) -> &'static str {
    let pair = match ai_state(name) {
        "work" => ("Trabajando", "Working"),
        "need" => ("Te necesita", "Needs you"),
        "done" => ("Terminó", "Finished"),
        "error" => ("Error", "Error"),
        _ => ("Quieta", "Idle"),
    };
    if english { pair.1 } else { pair.0 }
}

pub fn ai_cycle(name: &str) -> f64 {
    if ai_state(name) == "need" {
        AI_PULSE_SECONDS
    } else {
        0.0
    }
}

pub fn ai_frame_index(name: &str, seconds: f64) -> Result<usize, &'static str> {
    clock_frame(seconds, ai_cycle(name), AI_PULSE_FRAMES)
}

// Coordinates derive only from a u32 divided by 2 or 3: their range never
// needs Python's scientific notation. The shared JSON codec provides its
// existing nearest/even float formatting and retains the trailing .0.
fn coordinate(value: f64) -> String {
    crate::json::workspace_dumps(&Value::from(value)).expect("finite bounded coordinate")
}

pub fn ai_dot_svg(name: &str, size: u32, phase: Option<f64>) -> String {
    let name = ai_state(name);
    let color = ai_color(name);
    let c = size as f64 / 2.0;
    let r = size as f64 / 3.0;
    let halo = if let Some(phase) = phase.filter(|_| name == "need") {
        let t = remainder(phase, 1.0) * 2.0;
        let t = if t <= 1.0 { t } else { 2.0 - t };
        format!(
            r#"<circle cx="{}" cy="{}" r="{}" fill="{color}" fill-opacity="{}"/>"#,
            coordinate(c),
            coordinate(c),
            fixed(r + (size as f64 / 2.0 - r) * t, 2),
            fixed(0.3 * (1.0 - t), 2)
        )
    } else {
        String::new()
    };
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">{halo}<circle cx="{}" cy="{}" r="{}" fill="{color}"/></svg>"#,
        coordinate(c),
        coordinate(c),
        coordinate(r)
    )
}

pub fn ai_icon_html(name: &str, size: Option<u32>) -> String {
    let name = ai_state(name);
    let size = size.filter(|v| *v != 0).unwrap_or(AI_DOT_PIXELS);
    let color = ai_color(name);
    let label = ai_label(name, false);
    format!(
        r#"<span class="ai-icon ai-dot ai-{name}" style="--ai-c:{color};width:{size}px;height:{size}px" role="img" aria-label="{label}"></span>"#
    )
}

/// Select an identity only when exactly one pane matches. Missing JSON fields
/// compare as null, following the reference dictionary lookup.
pub fn pane_key_for(panes: Option<&[Value]>, session: &Value, pane_id: &Value) -> Option<Value> {
    let mut matches = panes?.iter().filter(|pane| {
        pane.is_object()
            && python_eq(&pane["session"], session)
            && python_eq(&pane["paneId"], pane_id)
    });
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let key = &first["paneKey"];
    truthy(key).then(|| key.clone())
}
