//! Tema del tablero (`theme_tokens`, `build_css` y el estilo del botón «Abrir»
//! de `bin/cc-notifyd`). Funciones puras: la lectura de `/prefs` y la caché de
//! 30 s viven en `dash.rs`, fuera del hilo de GTK.
use comandos_core::pomodoro::python_str;
use serde_json::{Map, Value};

/// Tokens de un tema: `dict(base, **tema)` más `name`.
pub type Tokens = Map<String, Value>;

/// Tokens base literales del Python (se sobrescriben con los del tema).
const BASE: [(&str, &str); 12] = [
    ("bg", "#0A0D13"),
    ("panel", "#121722"),
    ("panel2", "#161C29"),
    ("line", "#222A3A"),
    ("line2", "#2E3852"),
    ("text", "#EAF0FB"),
    ("dim", "#9AA6BF"),
    ("faint", "#5E6980"),
    ("brand", "#8B7CFF"),
    ("waiting", "#FFAE1A"),
    ("done", "#2EE59D"),
    ("working", "#7AA5FF"),
];

/// Verdad de Python (`bool(valor)`) para un valor JSON.
pub fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_none_or(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `json.load(f).get("themes") or {}` dentro del `try`: cualquier fallo de
/// lectura o un JSON que no es objeto deja `{}`.
pub fn themes_from_file(raw: Option<&[u8]>) -> Value {
    let parsed = raw.and_then(comandos_core::json::workspace_loads_bytes);
    match parsed {
        Some(Value::Object(top)) => match top.get("themes") {
            Some(themes) if py_truthy(themes) => themes.clone(),
            _ => Value::Object(Map::new()),
        },
        _ => Value::Object(Map::new()),
    }
}

/// `dash_get("/prefs").get("theme") or "noche"`; `None` si `/prefs` falló.
pub fn theme_name(prefs: Option<&Value>) -> Value {
    match prefs
        .and_then(Value::as_object)
        .and_then(|p| p.get("theme"))
    {
        Some(name) if py_truthy(name) => name.clone(),
        _ => Value::String("noche".into()),
    }
}

/// `themes.get(nombre)`: `Err` donde el Python lanzaría (`themes` no es un
/// `dict`, o el nombre no es *hashable*).
fn theme_get<'a>(themes: &'a Map<String, Value>, name: &Value) -> Result<Option<&'a Value>, ()> {
    match name {
        Value::String(key) => Ok(themes.get(key)),
        Value::Array(_) | Value::Object(_) => Err(()),
        _ => Ok(None),
    }
}

/// `theme_tokens()` sin la caché: `None` donde el Python lanza (`themes` que
/// no es objeto, tema que no es objeto o nombre de tema no *hashable*).
pub fn tokens(themes: &Value, name: &Value) -> Option<Tokens> {
    let themes = themes.as_object()?;
    let chosen = theme_get(themes, name).ok()?.filter(|v| py_truthy(v));
    let fallback = || themes.get("noche").filter(|v| py_truthy(v));
    let overrides = match chosen.or_else(fallback) {
        Some(Value::Object(theme)) => theme.clone(),
        Some(_) => return None,
        None => Map::new(),
    };
    let mut out: Tokens = BASE
        .iter()
        .map(|(k, v)| ((*k).to_string(), Value::String((*v).to_string())))
        .collect();
    for (key, value) in overrides {
        out.insert(key, value);
    }
    out.insert("name".into(), name.clone());
    Some(out)
}

/// `str(t[clave])`; los tokens base garantizan las claves que usa el CSS.
fn token(t: &Tokens, key: &str) -> String {
    t.get(key).map(python_str).unwrap_or_default()
}

/// `build_css(t)`: el CSS de los popups, byte a byte igual que el Python.
pub fn build_css(t: &Tokens) -> String {
    let panel = token(t, "panel");
    let line2 = token(t, "line2");
    let text = token(t, "text");
    let dim = token(t, "dim");
    let brand = token(t, "brand");
    let bg = token(t, "bg");
    format!(
        "
window.ccpop {{ background-color: rgba(0,0,0,0); }}
.card {{ background-color: {panel}; border: 1px solid {line2};
  border-radius: 14px; box-shadow: 0 8px 26px rgba(0,0,0,0.45); }}
.hd {{ background-color: rgba(0,0,0,0); border-radius: 14px; }}
.kbadge {{ background: none; min-width: 22px; min-height: 22px; padding: 0px; }}
.kbadge.waiting {{ background: none; }}
.proj {{ color: {text}; font-family: 'JetBrainsMono Nerd Font', monospace; font-weight: 800; font-size: 13px; letter-spacing: 0.2px; }}
.inprev {{ color: mix({text}, {dim}, 0.35); font-size: 12.5px; }}
.when {{ color: alpha({dim}, 0.9); font-family: 'JetBrainsMono Nerd Font', monospace; font-size: 11px; }}
.body {{ color: {text}; font-size: 13.5px; }}
.body selection {{ background-color: {brand}; color: {bg}; }}
.vermas {{ color: {dim}; font-size: 11.5px; padding: 4px 10px; background: none; border: none;
  font-family: 'JetBrainsMono Nerd Font', monospace; font-weight: 700; border-radius: 7px; }}
.vermas:hover {{ color: {text}; background-color: alpha({dim}, 0.15); }}
.btn {{ background-color: transparent; color: {text}; border: 1px solid alpha({dim}, 0.5);
  border-radius: 9px; padding: 5px 16px; font-family: 'JetBrainsMono Nerd Font', monospace;
  font-size: 12px; font-weight: 800; min-height: 30px; }}
.btn:hover {{ border-color: {brand}; color: {brand}; }}
.btn.primary {{ background-color: {brand}; color: {bg}; border-color: {brand}; }}
.btn.primary:hover {{ background-color: mix({brand}, {text}, 0.18); color: {bg}; }}
.close {{ background: none; border: none; color: {dim}; padding: 2px 8px; font-size: 15px;
  min-width: 26px; min-height: 26px; border-radius: 8px; }}
.close:hover, .close.hover {{ color: {text}; background-color: alpha({dim}, 0.18); }}
.clearall {{ background-color: {panel}; color: {text}; border: 1px solid {line2};
  border-radius: 999px; padding: 4px 16px; font-family: 'Inter', sans-serif; font-size: 12px; font-weight: 700;
  min-height: 30px; box-shadow: 0 6px 18px rgba(0,0,0,0.4); }}
.clearall:hover {{ border-color: {brand}; color: {brand}; }}
.entry {{ background-color: {bg}; color: {text}; border: 1px solid {line2};
  border-radius: 8px; padding: 6px 10px; font-family: 'JetBrainsMono Nerd Font', monospace; font-size: 12px; }}
.entry:focus {{ border-color: {brand}; }}
"
    )
}

/// CSS clavado al botón «Abrir» (prioridad USER: sin foco, `:backdrop` lo desaturaba).
pub fn open_button_css(t: &Tokens) -> String {
    let brand = token(t, "brand");
    let bg = token(t, "bg");
    let text = t.get("text").map_or_else(|| "#fff".to_string(), python_str);
    format!(
        "button, button:backdrop {{ background-image: none; background-color: {brand}; \
color: {bg}; border: 1px solid {brand}; border-radius: 8px; font-weight: 800; \
padding: 2px 11px; min-height: 22px; font-family: 'JetBrainsMono Nerd Font', monospace; font-size: 10.5px; }} \
button label, button:backdrop label {{ color: {bg}; }} \
button:hover {{ background-color: {text}; }}"
    )
}

/// Icono y color del distintivo de estado: `bell` con `waiting` o `check` con
/// `done` (`tk.get(...) or "#…"`). Color `None` si el token no es texto: en el
/// Python `color.encode()` falla y el icono cae a la viñeta `•`.
pub fn kind_icon(kind: &str, t: &Tokens) -> (&'static str, Option<String>) {
    let (name, key, fallback) = if kind == "waiting" {
        ("bell", "waiting", "#F5B14C")
    } else {
        ("check", "done", "#34D399")
    };
    let color = match t.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(value) if py_truthy(value) => None,
        _ => Some(fallback.to_string()),
    };
    (name, color)
}

/// SVG de Lucide con `currentColor` sustituido por el color.
pub fn recolor_svg(raw: &[u8], color: &str) -> Vec<u8> {
    let needle = b"currentColor";
    let mut out = Vec::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.windows(needle.len()).position(|w| w == needle) {
        out.extend_from_slice(rest.get(..at).unwrap_or_default());
        out.extend_from_slice(color.as_bytes());
        rest = rest.get(at + needle.len()..).unwrap_or_default();
    }
    out.extend_from_slice(rest);
    out
}
