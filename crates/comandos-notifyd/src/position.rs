//! Posición de la pila: la aritmética de `reposition` y `_sync_clear_all`, el
//! ancla del arrastre (`_on_configure`, `_release`) y `notifyd-pos.json`
//! (`_anchor_load`, `_anchor_save`).
use crate::markup::py_strip;
use crate::stack::{MARGIN, TOP, WIDTH};
use serde_json::Value;
use std::io;
use std::path::Path;

/// Separación vertical entre popups de la pila.
pub const GAP: i64 = 10;
/// Altura mínima que se reserva para «Cerrar todas».
pub const CLEAR_ALL_MIN_HEIGHT: i64 = 38;
/// Segundos tras un arrastre en los que un `configure` cuenta como del usuario.
pub const DRAG_WINDOW_SECS: f64 = 6.0;
/// Reasientos máximos por ventana cuando el gestor la recoloca.
pub const MAX_FIXES: u32 = 5;

/// Geometría del monitor (`mon.get_geometry()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

/// Resultado de `reposition`: dónde va cada popup y desde dónde sigue la pila
/// (`x`, `y`, `up` que recibe `_sync_clear_all`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub positions: Vec<(i64, i64)>,
    pub end: (i64, i64),
    pub up: bool,
}

/// La parte aritmética de `reposition()`. `mode` es `notif_pos` de `/prefs`
/// (`tl`, `tr`, `bl`, `br`; cualquier otra cosa es «libre»), `anchor` el
/// último arrastre y `heights` la altura asignada de cada popup, en orden.
pub fn layout(
    geo: Geometry,
    mode: Option<&str>,
    anchor: Option<(i64, i64)>,
    heights: &[i64],
) -> Layout {
    let width = i64::from(WIDTH);
    let corner = mode.filter(|m| matches!(*m, "tl" | "tr" | "bl" | "br"));
    let mut up = false;
    let (x, mut y) = match (corner, anchor) {
        (Some(m), _) => {
            let x = if m.ends_with('l') {
                geo.x + MARGIN
            } else {
                geo.x + geo.width - width - MARGIN
            };
            let y = if m.starts_with('t') {
                geo.y + TOP
            } else {
                up = true;
                // borde inferior EXACTO; el bucle resta cada altura
                geo.y + geo.height - MARGIN
            };
            (x, y)
        }
        // «Libre»: la pila nace donde el usuario dejó el último arrastre.
        (None, Some((ax, ay))) => (
            geo.x.max(ax.min(geo.x + geo.width - width - 4)),
            geo.y.max(ay.min(geo.y + geo.height - 120)),
        ),
        (None, None) => (geo.x + geo.width - width - MARGIN, geo.y + TOP),
    };
    let mut positions = Vec::with_capacity(heights.len());
    for &h in heights {
        if up {
            y -= h;
        }
        positions.push((x, y));
        y = if up { y - GAP } else { y + h + GAP };
    }
    Layout {
        positions,
        end: (x, y),
        up,
    }
}

/// `_sync_clear_all`: la pastilla, pegada al final de la pila (arriba si crece
/// hacia arriba). `height` es su altura asignada (mínimo 38).
pub fn clear_all_position(end: (i64, i64), up: bool, height: i64) -> (i64, i64) {
    let h = height.max(CLEAR_ALL_MIN_HEIGHT);
    (end.0, if up { end.1 - h } else { end.1 })
}

/// Nueva ancla tras un arrastre: la posición de la ventana menos su desplazamiento
/// en la pila (alturas de las de arriba + 10 cada una).
pub fn anchor_from(position: (i64, i64), heights_above: &[i64]) -> (i64, i64) {
    let offset: i64 = heights_above.iter().map(|h| h + GAP).sum();
    (position.0, position.1 - offset)
}

/// Qué hacer con un `configure-event` (`_on_configure`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Configure {
    /// Nada que hacer.
    Ignore,
    /// El gestor movió la ventana (mapa inicial, restack): re-asentar en 60 ms.
    Reassert,
    /// Arrastre del usuario: nueva ancla desde esta posición, guardarla y re-asentar.
    Anchor,
}

/// `_on_configure`: `want` es donde la pusimos, `dragging` si hay arrastre en
/// curso, `since_drag` segundos desde el último movimiento de arrastre
/// (`None` si nunca), `event` la posición del evento y `fixes` los reasientos ya hechos.
pub fn on_configure(
    want: Option<(i64, i64)>,
    dragging: bool,
    since_drag: Option<f64>,
    event: (i64, i64),
    fixes: u32,
) -> Configure {
    let Some(want) = want else {
        return Configure::Ignore;
    };
    if dragging {
        return Configure::Ignore;
    }
    let moved = (event.0 - want.0).abs() > 2 || (event.1 - want.1).abs() > 2;
    if since_drag.is_none_or(|s| s > DRAG_WINDOW_SECS) {
        if moved && fixes < MAX_FIXES {
            return Configure::Reassert;
        }
        return Configure::Ignore;
    }
    if moved {
        Configure::Anchor
    } else {
        Configure::Ignore
    }
}

/// `int(valor)` de Python para lo que guarda `notifyd-pos.json`: enteros,
/// flotantes finitos (truncados), booleanos y cadenas decimales ASCII.
fn py_int(value: &Value) -> Option<i64> {
    match value {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_u64().map(|_| i64::MAX))
            .or_else(|| {
                n.as_f64()
                    .filter(|f| f.is_finite())
                    .map(|f| f.trunc().clamp(i64::MIN as f64, i64::MAX as f64) as i64)
            }),
        Value::String(s) => py_int_str(s),
        _ => None,
    }
}

/// `int(cadena)`: espacios a los lados, signo y guiones bajos entre dígitos.
fn py_int_str(text: &str) -> Option<i64> {
    let text = py_strip(text);
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, text.get(1..)?),
        Some(b'+') => (false, text.get(1..)?),
        _ => (false, text),
    };
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || !digits.bytes().all(|b| b.is_ascii_digit() || b == b'_')
    {
        return None;
    }
    let mut value: i64 = 0;
    for b in digits.bytes().filter(u8::is_ascii_digit) {
        value = value.saturating_mul(10).saturating_add(i64::from(b - b'0'));
    }
    Some(if negative { -value } else { value })
}

/// `_anchor_load()`: `(int(d["x"]), int(d["y"]))` o `None` ante cualquier fallo.
pub fn load_anchor(path: &Path) -> Option<(i64, i64)> {
    let raw = std::fs::read(path).ok()?;
    let value = comandos_core::json::workspace_loads_bytes(&raw)?;
    let object = value.as_object()?;
    Some((py_int(object.get("x")?)?, py_int(object.get("y")?)?))
}

/// `_anchor_save(x, y)`: `{"x": x, "y": y}` en `<ruta>.tmp` y `rename`.
/// Se llama fuera del hilo de GTK.
pub fn save_anchor(path: &Path, anchor: (i64, i64)) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(
        &tmp,
        format!("{{\"x\": {}, \"y\": {}}}", anchor.0, anchor.1),
    )?;
    std::fs::rename(&tmp, path)
}
