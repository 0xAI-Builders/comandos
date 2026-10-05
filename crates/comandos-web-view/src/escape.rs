//! Escapes del JS heredado, byte a byte.
//!
//! - [`text`]: el `esc()` de los scripts (`dash/analytics.js`,
//!   `command-sidebar.js`, `notifications.js`, …):
//!   `String(s).replace(/[&<>"']/g, …)` → `&amp; &lt; &gt; &quot; &#39;`.
//! - [`md_esc`]: `mdEsc` de `index.html` (región «helpers»): solo `& < >`.
//! - [`attr_esc`]: `attrEsc` de `index.html`: `mdEsc` y después `"` → `&quot;`.
//!
//! Las variantes con extras (el `esc` de `analytics-render.js` también pasa 🍅
//! a `&#127813;`) se portan con su componente, no aquí.

/// Recorre `s` una vez y sustituye los bytes que `map` reconoce. Los bytes
/// escapados son todos ASCII, así que nunca parten un carácter UTF-8.
fn replace_ascii(s: &str, map: impl Fn(u8) -> Option<&'static str>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = 0;
    for (i, b) in s.bytes().enumerate() {
        if let Some(rep) = map(b) {
            out.push_str(s.get(start..i).unwrap_or_default());
            out.push_str(rep);
            start = i + 1;
        }
    }
    out.push_str(s.get(start..).unwrap_or_default());
    out
}

/// `esc()` del tablero: `& < > " '`.
pub fn text(s: &str) -> String {
    replace_ascii(s, |b| match b {
        b'&' => Some("&amp;"),
        b'<' => Some("&lt;"),
        b'>' => Some("&gt;"),
        b'"' => Some("&quot;"),
        b'\'' => Some("&#39;"),
        _ => None,
    })
}

/// `mdEsc` de `index.html`: `& < >` (las comillas quedan tal cual).
pub fn md_esc(s: &str) -> String {
    replace_ascii(s, |b| match b {
        b'&' => Some("&amp;"),
        b'<' => Some("&lt;"),
        b'>' => Some("&gt;"),
        _ => None,
    })
}

/// `attrEsc` de `index.html`: `mdEsc` más `"` → `&quot;` (`'` sin tocar).
pub fn attr_esc(s: &str) -> String {
    replace_ascii(s, |b| match b {
        b'&' => Some("&amp;"),
        b'<' => Some("&lt;"),
        b'>' => Some("&gt;"),
        b'"' => Some("&quot;"),
        _ => None,
    })
}
