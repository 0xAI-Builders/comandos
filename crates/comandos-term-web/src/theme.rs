//! Tema con los nombres de `ITheme` de xterm.js y su conversión a la paleta
//! del motor. El análisis de colores es puro (pruebas en host).
use comandos_term::engine::Palette;

/// Colores por omisión de xterm.js 5.5 (`ThemeService`).
pub const DEFAULT_FG: [u8; 3] = [0xFF, 0xFF, 0xFF];
pub const DEFAULT_BG: [u8; 3] = [0x00, 0x00, 0x00];
pub const DEFAULT_CURSOR: [u8; 3] = [0xFF, 0xFF, 0xFF];
pub const DEFAULT_CURSOR_ACCENT: [u8; 3] = [0x00, 0x00, 0x00];
/// Selección por omisión: blanco al 30 % sobre el fondo negro.
pub const DEFAULT_SELECTION: [u8; 3] = [0x4D, 0x4D, 0x4D];

/// Nombres de los 16 colores ANSI en `ITheme`.
pub const ANSI_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "brightBlack",
    "brightRed",
    "brightGreen",
    "brightYellow",
    "brightBlue",
    "brightMagenta",
    "brightCyan",
    "brightWhite",
];

/// Un tema ya leído: lo que falta queda en `None` y toma el valor de xterm.js.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Theme {
    pub foreground: Option<[u8; 3]>,
    pub background: Option<[u8; 3]>,
    pub cursor: Option<[u8; 3]>,
    pub cursor_accent: Option<[u8; 3]>,
    pub selection: Option<[u8; 3]>,
    pub ansi: [Option<[u8; 3]>; 16],
}

impl Theme {
    /// La paleta del motor: la de xterm.js con los colores del tema encima.
    /// Sin `cursor` ni `cursorAccent`, los de `ThemeService` (`#ffffff` y
    /// `#000000`), sea cual sea el texto o el fondo.
    pub fn palette(&self) -> Palette {
        let fg = self.foreground.unwrap_or(DEFAULT_FG);
        let bg = self.background.unwrap_or(DEFAULT_BG);
        let mut p = Palette::xterm_default(
            fg,
            bg,
            self.cursor.unwrap_or(DEFAULT_CURSOR),
            self.cursor_accent.unwrap_or(DEFAULT_CURSOR_ACCENT),
            self.selection.unwrap_or(DEFAULT_SELECTION),
        );
        for (slot, color) in p.ansi.iter_mut().zip(self.ansi.iter()) {
            if let Some(c) = color {
                *slot = *c;
            }
        }
        p
    }
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Color CSS de un tema: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa` (el alfa se
/// ignora: la terminal es opaca) o `rgb(r, g, b)` / `rgba(r, g, b, a)`.
pub fn parse_color(css: &str) -> Option<[u8; 3]> {
    let css = css.trim();
    if let Some(hex) = css.strip_prefix('#') {
        let b = hex.as_bytes();
        let nibble = |i: usize| b.get(i).copied().and_then(hex_digit);
        return match b.len() {
            3 | 4 => Some([nibble(0)? * 17, nibble(1)? * 17, nibble(2)? * 17]),
            6 | 8 => Some([
                nibble(0)? * 16 + nibble(1)?,
                nibble(2)? * 16 + nibble(3)?,
                nibble(4)? * 16 + nibble(5)?,
            ]),
            _ => None,
        };
    }
    let inner = css
        .strip_prefix("rgba(")
        .or_else(|| css.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let mut parts = inner.split(',').map(|p| p.trim().parse::<f64>().ok());
    let mut channel = || -> Option<u8> {
        let v = parts.next()??;
        // `round` de un valor ya acotado a 0..=255 cabe en u8.
        v.is_finite().then(|| v.round().clamp(0.0, 255.0) as u8)
    };
    Some([channel()?, channel()?, channel()?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_theme_colors() {
        assert_eq!(parse_color("#0A0D13"), Some([0x0A, 0x0D, 0x13]));
        assert_eq!(parse_color("#fff"), Some([255, 255, 255]));
        assert_eq!(parse_color("#11223380"), Some([0x11, 0x22, 0x33]));
        assert_eq!(parse_color("rgb(1, 2, 3)"), Some([1, 2, 3]));
        assert_eq!(parse_color("rgba(10,20,30,0.5)"), Some([10, 20, 30]));
        assert_eq!(parse_color("#12"), None);
        assert_eq!(parse_color("#zzzzzz"), None);
        assert_eq!(parse_color("red"), None);
        assert_eq!(parse_color("rgb(1,2)"), None);
    }

    #[test]
    fn palette_takes_theme_colors_and_xterm_defaults() {
        let mut t = Theme {
            foreground: Some([1, 2, 3]),
            background: Some([4, 5, 6]),
            ..Theme::default()
        };
        t.ansi[1] = Some([0xDA, 0x46, 0x2F]);
        let p = t.palette();
        assert_eq!(
            (p.fg, p.bg, p.cursor, p.cursor_accent),
            ([1, 2, 3], [4, 5, 6], DEFAULT_CURSOR, DEFAULT_CURSOR_ACCENT)
        );
        assert_eq!(p.ansi[1], [0xDA, 0x46, 0x2F]);
        // Lo que el tema no da es la paleta de xterm.js.
        assert_eq!(p.ansi[2], [0x4E, 0x9A, 0x06]);
        assert_eq!(p.ansi[196], [255, 0, 0]);
        assert_eq!(Theme::default().palette().bg, DEFAULT_BG);
    }
}
