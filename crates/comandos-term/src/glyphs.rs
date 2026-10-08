//! Glifos que xterm.js 5.5.0 dibuja a mano con `customGlyphs: true` (por
//! omisión) en vez de pedírselos a la fuente, copiados de las tablas de
//! `@xterm/addon-canvas` 0.7.0 (el renderizador que carga `dash/term.html`;
//! leídas del bundle `assets/xterm/addon-canvas.js`):
//!
//! - `boxDrawingDefinitions`: líneas de caja U+2500–U+257F, como trazos SVG
//!   (`M`, `L`, `C`) con su peso en píxeles; las cadenas son las del bundle,
//!   con las variables de las funciones de xterm.js escritas `{.5-t}`.
//! - `blockElementDefinitions`: bloques U+2580–U+259F (salvo ░▒▓) y los de
//!   *legacy computing* U+1FB70–U+1FB8B y U+1FB95–U+1FB97, en octavos.
//! - Tramas ░▒▓: máscaras por píxel de dispositivo.
//! - `powerlineDefinitions`: U+E0B0–U+E0BF (U+E0BB y U+E0BF son alias de
//!   U+E0BD y U+E0B9).
//!
//! [`box_ops`] da la definición, independiente del tamaño de la celda.
//! [`draw_ops`] la convierte en órdenes de canvas en píxeles de dispositivo
//! con las mismas cuentas que `tryDrawCustomChar` (escala, ajuste a medio
//! píxel, márgenes de powerline, grosores), para que quien pinta (A6) las
//! repita una a una sobre un `CanvasRenderingContext2D`.
//!
//! ## Contrato con quien pinta (A6)
//!
//! - Las coordenadas de [`DrawOp`] son píxeles de dispositivo relativos a la
//!   esquina superior izquierda de la celda; A6 suma el origen entero de la
//!   celda (`col · cell_w`, `fila · cell_h`).
//! - Color: el de texto de la tira (`Style::fg`); `strokeStyle = fillStyle`.
//! - Las órdenes de un glifo van entre `save()`/`restore()`: [`DrawOp::ClipCell`]
//!   recorta a la celda y no se deshace dentro del glifo.
//! - [`DrawOp::FillPattern`] llena la celda con la máscara repetida y anclada
//!   en la esquina de la celda: el píxel `(x, y)` de la celda se pinta si
//!   `mask[y % filas][x % columnas] == 1`.
//! - [`CellMetrics`] lleva el tamaño de celda **en píxeles de dispositivo**
//!   tal como lo calcula xterm.js (`deviceCellWidth`, `deviceCellHeight`),
//!   el `devicePixelRatio` y el `fontSize` en píxeles CSS.

/// Definición de un glifo, en las unidades de las tablas de xterm.js.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoxOp {
    /// Rectángulo relleno (`fillRect`), en fracción de la celda; los valores
    /// son octavos exactos.
    Rect { x: f32, y: f32, w: f32, h: f32 },
    /// Trama de la celda: máscara 0/1 por píxel de dispositivo.
    Pattern(&'static [&'static [u8]]),
    /// Trazo de caja: `lineWidth = dpr · weight` (1 fina, 3 gruesa) y
    /// coordenadas ajustadas al centro del píxel.
    Stroke { weight: u8, d: &'static str },
    /// Flecha de powerline recortada a la celda: relleno (`stroke = false`)
    /// o contorno de `fontSize / 12` px; márgenes en medios de ese grosor.
    Powerline {
        d: &'static str,
        stroke: bool,
        left_pad: u8,
        right_pad: u8,
    },
}

/// Tamaño de la celda para resolver un glifo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    /// `deviceCellWidth` de xterm.js (píxeles de dispositivo, entero).
    pub cell_w: f64,
    /// `deviceCellHeight` de xterm.js (píxeles de dispositivo, entero).
    pub cell_h: f64,
    /// `devicePixelRatio`.
    pub dpr: f64,
    /// `fontSize` del terminal, en píxeles CSS.
    pub font_size: f64,
}

/// Una orden de canvas, en píxeles de dispositivo relativos a la celda.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DrawOp {
    FillRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    FillPattern {
        mask: &'static [&'static [u8]],
    },
    /// `ctx.clip()` al rectángulo de la celda.
    ClipCell,
    BeginPath,
    MoveTo {
        x: f64,
        y: f64,
    },
    LineTo {
        x: f64,
        y: f64,
    },
    /// `bezierCurveTo(x1, y1, x2, y2, x, y)`.
    CurveTo {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x: f64,
        y: f64,
    },
    /// `ctx.lineWidth = line_width; ctx.stroke()`.
    Stroke {
        line_width: f64,
    },
    /// `ctx.fill()` (regla `nonzero`).
    Fill,
}

const fn stroke(weight: u8, d: &'static str) -> BoxOp {
    BoxOp::Stroke { weight, d }
}

const fn rect(x: u8, y: u8, w: u8, h: u8) -> BoxOp {
    BoxOp::Rect {
        x: x as f32 / 8.0,
        y: y as f32 / 8.0,
        w: w as f32 / 8.0,
        h: h as f32 / 8.0,
    }
}

const fn powerline(d: &'static str, stroke: bool, left_pad: u8, right_pad: u8) -> BoxOp {
    BoxOp::Powerline {
        d,
        stroke,
        left_pad,
        right_pad,
    }
}

/// Lista estática de operaciones (las llamadas `const fn` necesitan un
/// contexto constante para vivir en `'static`).
macro_rules! ops {
    ($($op:expr),+ $(,)?) => {{
        const OPS: &[BoxOp] = &[$($op),+];
        OPS
    }};
}

/// Máscaras de `patternCharacterDefinitions` (cobertura 12,5 %, 25 %, 75 %).
const SHADE_LIGHT: &[&[u8]] = &[&[1, 0, 0, 0], &[0, 0, 0, 0], &[0, 0, 1, 0], &[0, 0, 0, 0]];
const SHADE_MEDIUM: &[&[u8]] = &[&[1, 0], &[0, 0], &[0, 1], &[0, 0]];
const SHADE_DARK: &[&[u8]] = &[&[0, 1], &[1, 1], &[1, 0], &[1, 1]];

/// Cómo dibuja xterm.js 5.5.0 el carácter `c` con `customGlyphs`, o `None`
/// si lo pinta la fuente. Mismo orden de búsqueda que `tryDrawCustomChar`:
/// bloques, tramas, cajas, powerline.
pub fn box_ops(c: char) -> Option<&'static [BoxOp]> {
    let ops: &'static [BoxOp] = match c {
        // Tramas.
        '░' => ops![BoxOp::Pattern(SHADE_LIGHT)],
        '▒' => ops![BoxOp::Pattern(SHADE_MEDIUM)],
        '▓' => ops![BoxOp::Pattern(SHADE_DARK)],
        // Alias de `powerlineDefinitions`.
        '\u{E0BB}' => return box_ops('\u{E0BD}'),
        '\u{E0BF}' => return box_ops('\u{E0B9}'),
        // Tablas copiadas del bundle.
        '─' => ops![stroke(1, "M0,.5 L1,.5")],
        '━' => ops![stroke(3, "M0,.5 L1,.5")],
        '│' => ops![stroke(1, "M.5,0 L.5,1")],
        '┃' => ops![stroke(3, "M.5,0 L.5,1")],
        '┌' => ops![stroke(1, "M0.5,1 L.5,.5 L1,.5")],
        '┏' => ops![stroke(3, "M0.5,1 L.5,.5 L1,.5")],
        '┐' => ops![stroke(1, "M0,.5 L.5,.5 L.5,1")],
        '┓' => ops![stroke(3, "M0,.5 L.5,.5 L.5,1")],
        '└' => ops![stroke(1, "M.5,0 L.5,.5 L1,.5")],
        '┗' => ops![stroke(3, "M.5,0 L.5,.5 L1,.5")],
        '┘' => ops![stroke(1, "M.5,0 L.5,.5 L0,.5")],
        '┛' => ops![stroke(3, "M.5,0 L.5,.5 L0,.5")],
        '├' => ops![stroke(1, "M.5,0 L.5,1 M.5,.5 L1,.5")],
        '┣' => ops![stroke(3, "M.5,0 L.5,1 M.5,.5 L1,.5")],
        '┤' => ops![stroke(1, "M.5,0 L.5,1 M.5,.5 L0,.5")],
        '┫' => ops![stroke(3, "M.5,0 L.5,1 M.5,.5 L0,.5")],
        '┬' => ops![stroke(1, "M0,.5 L1,.5 M.5,.5 L.5,1")],
        '┳' => ops![stroke(3, "M0,.5 L1,.5 M.5,.5 L.5,1")],
        '┴' => ops![stroke(1, "M0,.5 L1,.5 M.5,.5 L.5,0")],
        '┻' => ops![stroke(3, "M0,.5 L1,.5 M.5,.5 L.5,0")],
        '┼' => ops![stroke(1, "M0,.5 L1,.5 M.5,0 L.5,1")],
        '╋' => ops![stroke(3, "M0,.5 L1,.5 M.5,0 L.5,1")],
        '╴' => ops![stroke(1, "M.5,.5 L0,.5")],
        '╸' => ops![stroke(3, "M.5,.5 L0,.5")],
        '╵' => ops![stroke(1, "M.5,.5 L.5,0")],
        '╹' => ops![stroke(3, "M.5,.5 L.5,0")],
        '╶' => ops![stroke(1, "M.5,.5 L1,.5")],
        '╺' => ops![stroke(3, "M.5,.5 L1,.5")],
        '╷' => ops![stroke(1, "M.5,.5 L.5,1")],
        '╻' => ops![stroke(3, "M.5,.5 L.5,1")],
        '═' => ops![stroke(1, "M0,{.5-t} L1,{.5-t} M0,{.5+t} L1,{.5+t}")],
        '║' => ops![stroke(1, "M{.5-e},0 L{.5-e},1 M{.5+e},0 L{.5+e},1")],
        '╒' => ops![stroke(1, "M.5,1 L.5,{.5-t} L1,{.5-t} M.5,{.5+t} L1,{.5+t}")],
        '╓' => ops![stroke(1, "M{.5-e},1 L{.5-e},.5 L1,.5 M{.5+e},.5 L{.5+e},1")],
        '╔' => ops![stroke(
            1,
            "M1,{.5-t} L{.5-e},{.5-t} L{.5-e},1 M1,{.5+t} L{.5+e},{.5+t} L{.5+e},1"
        )],
        '╕' => ops![stroke(1, "M0,{.5-t} L.5,{.5-t} L.5,1 M0,{.5+t} L.5,{.5+t}")],
        '╖' => ops![stroke(1, "M{.5+e},1 L{.5+e},.5 L0,.5 M{.5-e},.5 L{.5-e},1")],
        '╗' => ops![stroke(
            1,
            "M0,{.5+t} L{.5-e},{.5+t} L{.5-e},1 M0,{.5-t} L{.5+e},{.5-t} L{.5+e},1"
        )],
        '╘' => ops![stroke(1, "M.5,0 L.5,{.5+t} L1,{.5+t} M.5,{.5-t} L1,{.5-t}")],
        '╙' => ops![stroke(1, "M1,.5 L{.5-e},.5 L{.5-e},0 M{.5+e},.5 L{.5+e},0")],
        '╚' => ops![stroke(
            1,
            "M1,{.5-t} L{.5+e},{.5-t} L{.5+e},0 M1,{.5+t} L{.5-e},{.5+t} L{.5-e},0"
        )],
        '╛' => ops![stroke(1, "M0,{.5+t} L.5,{.5+t} L.5,0 M0,{.5-t} L.5,{.5-t}")],
        '╜' => ops![stroke(1, "M0,.5 L{.5+e},.5 L{.5+e},0 M{.5-e},.5 L{.5-e},0")],
        '╝' => ops![stroke(
            1,
            "M0,{.5-t} L{.5-e},{.5-t} L{.5-e},0 M0,{.5+t} L{.5+e},{.5+t} L{.5+e},0"
        )],
        '╞' => ops![stroke(
            1,
            "M.5,0 L.5,1 M.5,{.5-t} L1,{.5-t} M.5,{.5+t} L1,{.5+t}"
        )],
        '╟' => ops![stroke(
            1,
            "M{.5-e},0 L{.5-e},1 M{.5+e},0 L{.5+e},1 M{.5+e},.5 L1,.5"
        )],
        '╠' => ops![stroke(
            1,
            "M{.5-e},0 L{.5-e},1 M1,{.5+t} L{.5+e},{.5+t} L{.5+e},1 M1,{.5-t} L{.5+e},{.5-t} L{.5+e},0"
        )],
        '╡' => ops![stroke(
            1,
            "M.5,0 L.5,1 M0,{.5-t} L.5,{.5-t} M0,{.5+t} L.5,{.5+t}"
        )],
        '╢' => ops![stroke(
            1,
            "M0,.5 L{.5-e},.5 M{.5-e},0 L{.5-e},1 M{.5+e},0 L{.5+e},1"
        )],
        '╣' => ops![stroke(
            1,
            "M{.5+e},0 L{.5+e},1 M0,{.5+t} L{.5-e},{.5+t} L{.5-e},1 M0,{.5-t} L{.5-e},{.5-t} L{.5-e},0"
        )],
        '╤' => ops![stroke(
            1,
            "M0,{.5-t} L1,{.5-t} M0,{.5+t} L1,{.5+t} M.5,{.5+t} L.5,1"
        )],
        '╥' => ops![stroke(
            1,
            "M0,.5 L1,.5 M{.5-e},.5 L{.5-e},1 M{.5+e},.5 L{.5+e},1"
        )],
        '╦' => ops![stroke(
            1,
            "M0,{.5-t} L1,{.5-t} M0,{.5+t} L{.5-e},{.5+t} L{.5-e},1 M1,{.5+t} L{.5+e},{.5+t} L{.5+e},1"
        )],
        '╧' => ops![stroke(
            1,
            "M.5,0 L.5,{.5-t} M0,{.5-t} L1,{.5-t} M0,{.5+t} L1,{.5+t}"
        )],
        '╨' => ops![stroke(
            1,
            "M0,.5 L1,.5 M{.5-e},.5 L{.5-e},0 M{.5+e},.5 L{.5+e},0"
        )],
        '╩' => ops![stroke(
            1,
            "M0,{.5+t} L1,{.5+t} M0,{.5-t} L{.5-e},{.5-t} L{.5-e},0 M1,{.5-t} L{.5+e},{.5-t} L{.5+e},0"
        )],
        '╪' => ops![stroke(
            1,
            "M.5,0 L.5,1 M0,{.5-t} L1,{.5-t} M0,{.5+t} L1,{.5+t}"
        )],
        '╫' => ops![stroke(
            1,
            "M0,.5 L1,.5 M{.5-e},0 L{.5-e},1 M{.5+e},0 L{.5+e},1"
        )],
        '╬' => ops![stroke(
            1,
            "M0,{.5+t} L{.5-e},{.5+t} L{.5-e},1 M1,{.5+t} L{.5+e},{.5+t} L{.5+e},1 M0,{.5-t} L{.5-e},{.5-t} L{.5-e},0 M1,{.5-t} L{.5+e},{.5-t} L{.5+e},0"
        )],
        '╱' => ops![stroke(1, "M1,0 L0,1")],
        '╲' => ops![stroke(1, "M0,0 L1,1")],
        '╳' => ops![stroke(1, "M1,0 L0,1 M0,0 L1,1")],
        '╼' => ops![stroke(1, "M.5,.5 L0,.5"), stroke(3, "M.5,.5 L1,.5")],
        '╽' => ops![stroke(1, "M.5,.5 L.5,0"), stroke(3, "M.5,.5 L.5,1")],
        '╾' => ops![stroke(1, "M.5,.5 L1,.5"), stroke(3, "M.5,.5 L0,.5")],
        '╿' => ops![stroke(1, "M.5,.5 L.5,1"), stroke(3, "M.5,.5 L.5,0")],
        '┍' => ops![stroke(1, "M.5,.5 L.5,1"), stroke(3, "M.5,.5 L1,.5")],
        '┎' => ops![stroke(1, "M.5,.5 L1,.5"), stroke(3, "M.5,.5 L.5,1")],
        '┑' => ops![stroke(1, "M.5,.5 L.5,1"), stroke(3, "M.5,.5 L0,.5")],
        '┒' => ops![stroke(1, "M.5,.5 L0,.5"), stroke(3, "M.5,.5 L.5,1")],
        '┕' => ops![stroke(1, "M.5,.5 L.5,0"), stroke(3, "M.5,.5 L1,.5")],
        '┖' => ops![stroke(1, "M.5,.5 L1,.5"), stroke(3, "M.5,.5 L.5,0")],
        '┙' => ops![stroke(1, "M.5,.5 L.5,0"), stroke(3, "M.5,.5 L0,.5")],
        '┚' => ops![stroke(1, "M.5,.5 L0,.5"), stroke(3, "M.5,.5 L.5,0")],
        '┝' => ops![stroke(1, "M.5,0 L.5,1"), stroke(3, "M.5,.5 L1,.5")],
        '┞' => ops![stroke(1, "M0.5,1 L.5,.5 L1,.5"), stroke(3, "M.5,.5 L.5,0")],
        '┟' => ops![stroke(1, "M.5,0 L.5,.5 L1,.5"), stroke(3, "M.5,.5 L.5,1")],
        '┠' => ops![stroke(1, "M.5,.5 L1,.5"), stroke(3, "M.5,0 L.5,1")],
        '┡' => ops![stroke(1, "M.5,.5 L.5,1"), stroke(3, "M.5,0 L.5,.5 L1,.5")],
        '┢' => ops![stroke(1, "M.5,.5 L.5,0"), stroke(3, "M0.5,1 L.5,.5 L1,.5")],
        '┥' => ops![stroke(1, "M.5,0 L.5,1"), stroke(3, "M.5,.5 L0,.5")],
        '┦' => ops![stroke(1, "M0,.5 L.5,.5 L.5,1"), stroke(3, "M.5,.5 L.5,0")],
        '┧' => ops![stroke(1, "M.5,0 L.5,.5 L0,.5"), stroke(3, "M.5,.5 L.5,1")],
        '┨' => ops![stroke(1, "M.5,.5 L0,.5"), stroke(3, "M.5,0 L.5,1")],
        '┩' => ops![stroke(1, "M.5,.5 L.5,1"), stroke(3, "M.5,0 L.5,.5 L0,.5")],
        '┪' => ops![stroke(1, "M.5,.5 L.5,0"), stroke(3, "M0,.5 L.5,.5 L.5,1")],
        '┭' => ops![stroke(1, "M0.5,1 L.5,.5 L1,.5"), stroke(3, "M.5,.5 L0,.5")],
        '┮' => ops![stroke(1, "M0,.5 L.5,.5 L.5,1"), stroke(3, "M.5,.5 L1,.5")],
        '┯' => ops![stroke(1, "M.5,.5 L.5,1"), stroke(3, "M0,.5 L1,.5")],
        '┰' => ops![stroke(1, "M0,.5 L1,.5"), stroke(3, "M.5,.5 L.5,1")],
        '┱' => ops![stroke(1, "M.5,.5 L1,.5"), stroke(3, "M0,.5 L.5,.5 L.5,1")],
        '┲' => ops![stroke(1, "M.5,.5 L0,.5"), stroke(3, "M0.5,1 L.5,.5 L1,.5")],
        '┵' => ops![stroke(1, "M.5,0 L.5,.5 L1,.5"), stroke(3, "M.5,.5 L0,.5")],
        '┶' => ops![stroke(1, "M.5,0 L.5,.5 L0,.5"), stroke(3, "M.5,.5 L1,.5")],
        '┷' => ops![stroke(1, "M.5,.5 L.5,0"), stroke(3, "M0,.5 L1,.5")],
        '┸' => ops![stroke(1, "M0,.5 L1,.5"), stroke(3, "M.5,.5 L.5,0")],
        '┹' => ops![stroke(1, "M.5,.5 L1,.5"), stroke(3, "M.5,0 L.5,.5 L0,.5")],
        '┺' => ops![stroke(1, "M.5,.5 L0,.5"), stroke(3, "M.5,0 L.5,.5 L1,.5")],
        '┽' => ops![
            stroke(1, "M.5,0 L.5,1 M.5,.5 L1,.5"),
            stroke(3, "M.5,.5 L0,.5")
        ],
        '┾' => ops![
            stroke(1, "M.5,0 L.5,1 M.5,.5 L0,.5"),
            stroke(3, "M.5,.5 L1,.5")
        ],
        '┿' => ops![stroke(1, "M.5,0 L.5,1"), stroke(3, "M0,.5 L1,.5")],
        '╀' => ops![
            stroke(1, "M0,.5 L1,.5 M.5,.5 L.5,1"),
            stroke(3, "M.5,.5 L.5,0")
        ],
        '╁' => ops![
            stroke(1, "M.5,.5 L.5,0 M0,.5 L1,.5"),
            stroke(3, "M.5,.5 L.5,1")
        ],
        '╂' => ops![stroke(1, "M0,.5 L1,.5"), stroke(3, "M.5,0 L.5,1")],
        '╃' => ops![
            stroke(1, "M0.5,1 L.5,.5 L1,.5"),
            stroke(3, "M.5,0 L.5,.5 L0,.5")
        ],
        '╄' => ops![
            stroke(1, "M0,.5 L.5,.5 L.5,1"),
            stroke(3, "M.5,0 L.5,.5 L1,.5")
        ],
        '╅' => ops![
            stroke(1, "M.5,0 L.5,.5 L1,.5"),
            stroke(3, "M0,.5 L.5,.5 L.5,1")
        ],
        '╆' => ops![
            stroke(1, "M.5,0 L.5,.5 L0,.5"),
            stroke(3, "M0.5,1 L.5,.5 L1,.5")
        ],
        '╇' => ops![
            stroke(1, "M.5,.5 L.5,1"),
            stroke(3, "M.5,.5 L.5,0 M0,.5 L1,.5")
        ],
        '╈' => ops![
            stroke(1, "M.5,.5 L.5,0"),
            stroke(3, "M0,.5 L1,.5 M.5,.5 L.5,1")
        ],
        '╉' => ops![
            stroke(1, "M.5,.5 L1,.5"),
            stroke(3, "M.5,0 L.5,1 M.5,.5 L0,.5")
        ],
        '╊' => ops![
            stroke(1, "M.5,.5 L0,.5"),
            stroke(3, "M.5,0 L.5,1 M.5,.5 L1,.5")
        ],
        '╌' => ops![stroke(1, "M.1,.5 L.4,.5 M.6,.5 L.9,.5")],
        '╍' => ops![stroke(3, "M.1,.5 L.4,.5 M.6,.5 L.9,.5")],
        '┄' => ops![stroke(
            1,
            "M.0667,.5 L.2667,.5 M.4,.5 L.6,.5 M.7333,.5 L.9333,.5"
        )],
        '┅' => ops![stroke(
            3,
            "M.0667,.5 L.2667,.5 M.4,.5 L.6,.5 M.7333,.5 L.9333,.5"
        )],
        '┈' => ops![stroke(
            1,
            "M.05,.5 L.2,.5 M.3,.5 L.45,.5 M.55,.5 L.7,.5 M.8,.5 L.95,.5"
        )],
        '┉' => ops![stroke(
            3,
            "M.05,.5 L.2,.5 M.3,.5 L.45,.5 M.55,.5 L.7,.5 M.8,.5 L.95,.5"
        )],
        '╎' => ops![stroke(1, "M.5,.1 L.5,.4 M.5,.6 L.5,.9")],
        '╏' => ops![stroke(3, "M.5,.1 L.5,.4 M.5,.6 L.5,.9")],
        '┆' => ops![stroke(
            1,
            "M.5,.0667 L.5,.2667 M.5,.4 L.5,.6 M.5,.7333 L.5,.9333"
        )],
        '┇' => ops![stroke(
            3,
            "M.5,.0667 L.5,.2667 M.5,.4 L.5,.6 M.5,.7333 L.5,.9333"
        )],
        '┊' => ops![stroke(
            1,
            "M.5,.05 L.5,.2 M.5,.3 L.5,.45 L.5,.55 M.5,.7 L.5,.95"
        )],
        '┋' => ops![stroke(
            3,
            "M.5,.05 L.5,.2 M.5,.3 L.5,.45 L.5,.55 M.5,.7 L.5,.95"
        )],
        '╭' => ops![stroke(
            1,
            "M.5,1 L.5,{.5+t/.15*.5} C.5,{.5+t/.15*.5},.5,.5,1,.5"
        )],
        '╮' => ops![stroke(
            1,
            "M.5,1 L.5,{.5+t/.15*.5} C.5,{.5+t/.15*.5},.5,.5,0,.5"
        )],
        '╯' => ops![stroke(
            1,
            "M.5,0 L.5,{.5-t/.15*.5} C.5,{.5-t/.15*.5},.5,.5,0,.5"
        )],
        '╰' => ops![stroke(
            1,
            "M.5,0 L.5,{.5-t/.15*.5} C.5,{.5-t/.15*.5},.5,.5,1,.5"
        )],
        '▀' => ops![rect(0, 0, 8, 4)],
        '▁' => ops![rect(0, 7, 8, 1)],
        '▂' => ops![rect(0, 6, 8, 2)],
        '▃' => ops![rect(0, 5, 8, 3)],
        '▄' => ops![rect(0, 4, 8, 4)],
        '▅' => ops![rect(0, 3, 8, 5)],
        '▆' => ops![rect(0, 2, 8, 6)],
        '▇' => ops![rect(0, 1, 8, 7)],
        '█' => ops![rect(0, 0, 8, 8)],
        '▉' => ops![rect(0, 0, 7, 8)],
        '▊' => ops![rect(0, 0, 6, 8)],
        '▋' => ops![rect(0, 0, 5, 8)],
        '▌' => ops![rect(0, 0, 4, 8)],
        '▍' => ops![rect(0, 0, 3, 8)],
        '▎' => ops![rect(0, 0, 2, 8)],
        '▏' => ops![rect(0, 0, 1, 8)],
        '▐' => ops![rect(4, 0, 4, 8)],
        '▔' => ops![rect(0, 0, 8, 1)],
        '▕' => ops![rect(7, 0, 1, 8)],
        '▖' => ops![rect(0, 4, 4, 4)],
        '▗' => ops![rect(4, 4, 4, 4)],
        '▘' => ops![rect(0, 0, 4, 4)],
        '▙' => ops![rect(0, 0, 4, 8), rect(0, 4, 8, 4)],
        '▚' => ops![rect(0, 0, 4, 4), rect(4, 4, 4, 4)],
        '▛' => ops![rect(0, 0, 4, 8), rect(4, 0, 4, 4)],
        '▜' => ops![rect(0, 0, 8, 4), rect(4, 0, 4, 8)],
        '▝' => ops![rect(4, 0, 4, 4)],
        '▞' => ops![rect(4, 0, 4, 4), rect(0, 4, 4, 4)],
        '▟' => ops![rect(4, 0, 4, 8), rect(0, 4, 8, 4)],
        '\u{1FB70}' => ops![rect(1, 0, 1, 8)],
        '\u{1FB71}' => ops![rect(2, 0, 1, 8)],
        '\u{1FB72}' => ops![rect(3, 0, 1, 8)],
        '\u{1FB73}' => ops![rect(4, 0, 1, 8)],
        '\u{1FB74}' => ops![rect(5, 0, 1, 8)],
        '\u{1FB75}' => ops![rect(6, 0, 1, 8)],
        '\u{1FB76}' => ops![rect(0, 1, 8, 1)],
        '\u{1FB77}' => ops![rect(0, 2, 8, 1)],
        '\u{1FB78}' => ops![rect(0, 3, 8, 1)],
        '\u{1FB79}' => ops![rect(0, 4, 8, 1)],
        '\u{1FB7A}' => ops![rect(0, 5, 8, 1)],
        '\u{1FB7B}' => ops![rect(0, 6, 8, 1)],
        '\u{1FB7C}' => ops![rect(0, 0, 1, 8), rect(0, 7, 8, 1)],
        '\u{1FB7D}' => ops![rect(0, 0, 1, 8), rect(0, 0, 8, 1)],
        '\u{1FB7E}' => ops![rect(7, 0, 1, 8), rect(0, 0, 8, 1)],
        '\u{1FB7F}' => ops![rect(7, 0, 1, 8), rect(0, 7, 8, 1)],
        '\u{1FB80}' => ops![rect(0, 0, 8, 1), rect(0, 7, 8, 1)],
        '\u{1FB81}' => ops![
            rect(0, 0, 8, 1),
            rect(0, 2, 8, 1),
            rect(0, 4, 8, 1),
            rect(0, 7, 8, 1)
        ],
        '\u{1FB82}' => ops![rect(0, 0, 8, 2)],
        '\u{1FB83}' => ops![rect(0, 0, 8, 3)],
        '\u{1FB84}' => ops![rect(0, 0, 8, 5)],
        '\u{1FB85}' => ops![rect(0, 0, 8, 6)],
        '\u{1FB86}' => ops![rect(0, 0, 8, 7)],
        '\u{1FB87}' => ops![rect(6, 0, 2, 8)],
        '\u{1FB88}' => ops![rect(5, 0, 3, 8)],
        '\u{1FB89}' => ops![rect(3, 0, 5, 8)],
        '\u{1FB8A}' => ops![rect(2, 0, 6, 8)],
        '\u{1FB8B}' => ops![rect(1, 0, 7, 8)],
        '\u{1FB95}' => ops![
            rect(0, 0, 2, 2),
            rect(4, 0, 2, 2),
            rect(2, 2, 2, 2),
            rect(6, 2, 2, 2),
            rect(0, 4, 2, 2),
            rect(4, 4, 2, 2),
            rect(2, 6, 2, 2),
            rect(6, 6, 2, 2)
        ],
        '\u{1FB96}' => ops![
            rect(2, 0, 2, 2),
            rect(6, 0, 2, 2),
            rect(0, 2, 2, 2),
            rect(4, 2, 2, 2),
            rect(2, 4, 2, 2),
            rect(6, 4, 2, 2),
            rect(0, 6, 2, 2),
            rect(4, 6, 2, 2)
        ],
        '\u{1FB97}' => ops![rect(0, 2, 8, 2), rect(0, 6, 8, 2)],
        '\u{E0B0}' => ops![powerline("M0,0 L1,.5 L0,1", false, 0, 2)],
        '\u{E0B1}' => ops![powerline("M-1,-.5 L1,.5 L-1,1.5", true, 1, 1)],
        '\u{E0B2}' => ops![powerline("M1,0 L0,.5 L1,1", false, 2, 0)],
        '\u{E0B3}' => ops![powerline("M2,-.5 L0,.5 L2,1.5", true, 1, 1)],
        '\u{E0B4}' => ops![powerline(
            "M0,0 L0,1 C0.552,1,1,0.776,1,.5 C1,0.224,0.552,0,0,0",
            false,
            0,
            1
        )],
        '\u{E0B5}' => ops![powerline(
            "M.2,1 C.422,1,.8,.826,.78,.5 C.8,.174,0.422,0,.2,0",
            true,
            0,
            1
        )],
        '\u{E0B6}' => ops![powerline(
            "M1,0 L1,1 C0.448,1,0,0.776,0,.5 C0,0.224,0.448,0,1,0",
            false,
            1,
            0
        )],
        '\u{E0B7}' => ops![powerline(
            "M.8,1 C0.578,1,0.2,.826,.22,.5 C0.2,0.174,0.578,0,0.8,0",
            true,
            1,
            0
        )],
        '\u{E0B8}' => ops![powerline("M-.5,-.5 L1.5,1.5 L-.5,1.5", false, 0, 0)],
        '\u{E0B9}' => ops![powerline("M-.5,-.5 L1.5,1.5", true, 1, 1)],
        '\u{E0BA}' => ops![powerline("M1.5,-.5 L-.5,1.5 L1.5,1.5", false, 0, 0)],
        '\u{E0BC}' => ops![powerline("M1.5,-.5 L-.5,1.5 L-.5,-.5", false, 0, 0)],
        '\u{E0BD}' => ops![powerline("M1.5,-.5 L-.5,1.5", true, 1, 1)],
        '\u{E0BE}' => ops![powerline("M-.5,-.5 L1.5,1.5 L1.5,-.5", false, 0, 0)],
        _ => return None,
    };
    Some(ops)
}

/// Variables de las funciones de `boxDrawingDefinitions`: xterm.js las
/// llama con `e = .15` y `t = .15 / deviceCellHeight · deviceCellWidth`.
struct Vars {
    t: f64,
}

/// `e` de xterm.js.
const E: f64 = 0.15;

impl Vars {
    /// Valor de una variable `{…}` con el mismo orden de operaciones que el
    /// JavaScript original (los resultados en coma flotante coinciden).
    fn eval(&self, expr: &str) -> Option<f64> {
        let t = self.t;
        Some(match expr {
            ".5-e" => 0.5 - E,
            ".5+e" => 0.5 + E,
            ".5-t" => 0.5 - t,
            ".5+t" => 0.5 + t,
            ".5-t/.15*.5" => 0.5 - t / 0.15 * 0.5,
            ".5+t/.15*.5" => 0.5 + t / 0.15 * 0.5,
            _ => return None,
        })
    }
}

/// `Math.round` de JavaScript: al entero más cercano, empates hacia +∞.
fn js_round(v: f64) -> f64 {
    let floor = v.floor();
    if v - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// Transformación de coordenadas de `h()` en `CustomGlyphs.ts`.
#[derive(Clone, Copy)]
struct Place {
    cell_w: f64,
    cell_h: f64,
    /// Ajuste al centro del píxel (cajas sí, powerline no).
    snap: bool,
    /// Márgenes izquierdo y derecho ya en píxeles de dispositivo.
    left: f64,
    right: f64,
}

impl Place {
    fn x(&self, v: f64) -> f64 {
        let mut c = v * (self.cell_w - self.left - self.right);
        if self.snap && c != 0.0 {
            c = (js_round(c + 0.5) - 0.5).min(self.cell_w).max(0.0);
        }
        c + self.left
    }

    fn y(&self, v: f64) -> f64 {
        let mut c = v * self.cell_h;
        if self.snap && c != 0.0 {
            c = (js_round(c + 0.5) - 0.5).min(self.cell_h).max(0.0);
        }
        c
    }
}

/// Número o variable de una coordenada (`parseFloat` acepta `.5` y `-.5`).
fn coord(token: &str, vars: &Vars) -> Option<f64> {
    match token.strip_prefix('{').and_then(|t| t.strip_suffix('}')) {
        Some(expr) => vars.eval(expr),
        // Only the immutable definition literals reach this branch. Preserve
        // their exact f64 values without a runtime decimal parser per glyph.
        None => Some(match token {
            "-1" => -1.0,
            "-.5" => -0.5,
            "0" => 0.0,
            ".05" => 0.05,
            ".0667" => 0.0667,
            ".1" => 0.1,
            ".174" | "0.174" => 0.174,
            ".2" | "0.2" => 0.2,
            ".22" => 0.22,
            "0.224" => 0.224,
            ".2667" => 0.2667,
            ".3" => 0.3,
            ".4" => 0.4,
            ".422" | "0.422" => 0.422,
            "0.448" => 0.448,
            ".45" => 0.45,
            ".5" | "0.5" => 0.5,
            ".55" => 0.55,
            "0.552" => 0.552,
            "0.578" => 0.578,
            ".6" => 0.6,
            ".7" => 0.7,
            ".7333" => 0.7333,
            "0.776" => 0.776,
            ".78" => 0.78,
            ".8" | "0.8" => 0.8,
            ".826" => 0.826,
            ".9" => 0.9,
            ".9333" => 0.9333,
            ".95" => 0.95,
            "1" => 1.0,
            "1.5" => 1.5,
            "2" => 2.0,
            _ => return None,
        }),
    }
}

/// Recorre un trazo `M…L…C…` y añade sus órdenes. Devuelve las
/// instrucciones que no entendió (xterm.js las salta con un aviso).
fn push_path(out: &mut Vec<DrawOp>, d: &str, vars: &Vars, place: Place) -> usize {
    let mut skipped = 0;
    for instruction in d.split(' ') {
        let mut chars = instruction.chars();
        let Some(cmd) = chars.next() else {
            skipped += 1;
            continue;
        };
        let mut nums = [0.0_f64; 6];
        let mut count = 0;
        let mut ok = true;
        for (i, token) in chars.as_str().split(',').enumerate() {
            match (coord(token, vars), nums.get_mut(i)) {
                (Some(v), Some(slot)) => {
                    *slot = if i % 2 == 0 { place.x(v) } else { place.y(v) };
                    count += 1;
                }
                _ => ok = false,
            }
        }
        let [a, b, c, e, f, g] = nums;
        let op = match (cmd, count) {
            ('M', 2) if ok => DrawOp::MoveTo { x: a, y: b },
            ('L', 2) if ok => DrawOp::LineTo { x: a, y: b },
            ('C', 6) if ok => DrawOp::CurveTo {
                x1: a,
                y1: b,
                x2: c,
                y2: e,
                x: f,
                y: g,
            },
            _ => {
                skipped += 1;
                continue;
            }
        };
        out.push(op);
    }
    skipped
}

/// Órdenes de canvas para pintar `c` en una celda de `metrics`, añadidas a
/// `out` (no se vacía: quien pinta lo reutiliza). `false` si `c` lo pinta
/// la fuente.
pub fn draw_ops(c: char, metrics: &CellMetrics, out: &mut Vec<DrawOp>) -> bool {
    let Some(ops) = box_ops(c) else {
        return false;
    };
    push_ops(ops, metrics, out);
    true
}

fn push_ops(ops: &[BoxOp], m: &CellMetrics, out: &mut Vec<DrawOp>) -> usize {
    let vars = Vars {
        t: 0.15 / m.cell_h * m.cell_w,
    };
    let mut skipped = 0;
    for op in ops {
        match *op {
            BoxOp::Rect { x, y, w, h } => {
                // `fillRect(x + a.x · (ancho/8), …)` con los octavos exactos.
                let (eighth_w, eighth_h) = (m.cell_w / 8.0, m.cell_h / 8.0);
                let eighths = |v: f32| f64::from(v) * 8.0;
                out.push(DrawOp::FillRect {
                    x: eighths(x) * eighth_w,
                    y: eighths(y) * eighth_h,
                    w: eighths(w) * eighth_w,
                    h: eighths(h) * eighth_h,
                });
            }
            BoxOp::Pattern(mask) => out.push(DrawOp::FillPattern { mask }),
            BoxOp::Stroke { weight, d } => {
                out.push(DrawOp::BeginPath);
                let place = Place {
                    cell_w: m.cell_w,
                    cell_h: m.cell_h,
                    snap: true,
                    left: 0.0,
                    right: 0.0,
                };
                skipped += push_path(out, d, &vars, place);
                out.push(DrawOp::Stroke {
                    line_width: m.dpr * f64::from(weight),
                });
            }
            BoxOp::Powerline {
                d,
                stroke,
                left_pad,
                right_pad,
            } => {
                // `d = fontSize / 12`; márgenes `padding · (d / 2)` en px CSS.
                let unit = m.font_size / 12.0;
                let place = Place {
                    cell_w: m.cell_w,
                    cell_h: m.cell_h,
                    snap: false,
                    left: f64::from(left_pad) * (unit / 2.0) * m.dpr,
                    right: f64::from(right_pad) * (unit / 2.0) * m.dpr,
                };
                out.push(DrawOp::ClipCell);
                out.push(DrawOp::BeginPath);
                skipped += push_path(out, d, &vars, place);
                out.push(if stroke {
                    DrawOp::Stroke {
                        line_width: m.dpr * unit,
                    }
                } else {
                    DrawOp::Fill
                });
            }
        }
    }
    skipped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_chars() -> impl Iterator<Item = char> {
        ('\u{2500}'..='\u{259F}')
            .chain('\u{1FB70}'..='\u{1FB8B}')
            .chain('\u{1FB95}'..='\u{1FB97}')
            .chain('\u{E0B0}'..='\u{E0BF}')
    }

    #[test]
    fn every_definition_parses_completely() {
        let m = CellMetrics {
            cell_w: 9.0,
            cell_h: 20.0,
            dpr: 1.0,
            font_size: 14.0,
        };
        for c in all_chars() {
            let ops = box_ops(c).unwrap_or_else(|| panic!("falta {c:?}"));
            let mut out = Vec::new();
            assert_eq!(
                push_ops(ops, &m, &mut out),
                0,
                "{c:?} con instrucciones sin entender"
            );
            assert!(!out.is_empty(), "{c:?}");
        }
    }

    #[test]
    fn js_round_ties_go_up() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(2.4999999), 2.0);
        assert_eq!(".5".parse::<f64>().ok(), Some(0.5));
        assert_eq!("-.5".parse::<f64>().ok(), Some(-0.5));
    }
}
