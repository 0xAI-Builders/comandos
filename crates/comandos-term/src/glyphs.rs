//! Glifos de dibujo que xterm.js 5.5.0 pinta a mano con `customGlyphs: true`
//! (por omisión) en vez de pedírselos a la fuente: líneas de caja
//! U+2500–U+257F, bloques y sombreados U+2580–U+259F y las flechas de
//! powerline U+E0B0–U+E0B3. Así las cajas se tocan entre celdas aunque la
//! fuente no llene la celda (el interlineado es 1.2).
//!
//! Las definiciones copian las de `CustomGlyphs.ts` de xterm.js 5.5.0
//! (`boxDrawingDefinitions`, `blockElementDefinitions`,
//! `powerlineDefinitions`; leídas del bundle `assets/xterm/addon-canvas.js`
//! 0.7.0): cada trazo de SVG `M…L…` es aquí una polilínea con los mismos
//! puntos.
//!
//! ## Contrato con quien pinta (A6)
//!
//! - Coordenadas en unidades de celda: `x` en fracción del **ancho**, `y`
//!   en fracción del **alto**, `0..1` dentro de la celda.
//! - El grosor `w` de `HLine`/`VLine` es siempre fracción del **alto** de la
//!   celda (para las dos orientaciones), así una línea vertical y una
//!   horizontal miden los mismos píxeles: fina 1/12 (≈1 px a 11–14 px de
//!   letra con interlineado 1.2) y gruesa 1/6.
//! - `Rect` usa el color del texto con opacidad `alpha`.
//! - Todos los `Tri` de un glifo tienen la misma orientación (área con
//!   signo positiva en coordenadas de celda) y se rellenan en **un solo
//!   trazado** con la regla `nonzero`: así no quedan costuras entre
//!   triángulos vecinos. Pueden salirse un poco de la celda (extremos de un
//!   trazo diagonal, flechas de powerline); quien pinta recorta a la celda,
//!   como hace xterm.js con powerline.
//!
//! ## Diferencias con xterm.js
//!
//! - xterm.js traza con `lineWidth` en píxeles de dispositivo (1 fina,
//!   3 gruesa); aquí el grosor es relativo a la celda (1/12 y 1/6) porque las
//!   operaciones no conocen el tamaño en píxeles. Quien pinta redondea a
//!   píxeles enteros.
//! - Los sombreados ░▒▓ son en xterm.js tramas de píxeles (cobertura media
//!   12,5 %, 25 % y 75 %); aquí son un rectángulo de opacidad 0,25, 0,5 y
//!   0,75 (los valores nominales de Unicode que fija el plan).
//! - Las dobles líneas y las esquinas redondeadas de xterm.js dependen de la
//!   proporción real de la celda; aquí se fija [`CELL_ASPECT`].
use std::sync::OnceLock;

/// Una operación de dibujo de un glifo, en unidades de celda.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoxOp {
    /// Línea horizontal centrada en `y`, de `x0` a `x1`, de grosor `w`.
    HLine { y: f32, x0: f32, x1: f32, w: f32 },
    /// Línea vertical centrada en `x`, de `y0` a `y1`, de grosor `w`.
    VLine { x: f32, y0: f32, y1: f32, w: f32 },
    /// Rectángulo relleno con el color del texto a opacidad `alpha`.
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        alpha: f32,
    },
    /// Triángulo relleno (ver el contrato de orientación del módulo).
    Tri { pts: [(f32, f32); 3] },
}

/// Grosor de una línea fina, en fracción del alto de la celda.
pub const THIN: f32 = 1.0 / 12.0;
/// Grosor de una línea gruesa, en fracción del alto de la celda.
pub const THICK: f32 = 1.0 / 6.0;

/// Proporción ancho/alto de la celda que se supone para las medidas que en
/// xterm.js dependen de ella. Fuente monoespaciada de avance ≈ 0,6 em con
/// el interlineado 1.2 de `dash/term.html`: 0,6 / 1,2 = 0,5.
pub const CELL_ASPECT: f32 = 0.5;

/// Interlineado de `dash/term.html` (`lineHeight: 1.2`).
const LINE_HEIGHT: f32 = 1.2;

/// Media separación de una doble línea: xterm.js usa 0,15 del ancho en
/// horizontal y `0,15 · ancho / alto` en vertical (mismos píxeles).
const DOUBLE_X: f32 = 0.15;
const DOUBLE_Y: f32 = DOUBLE_X * CELL_ASPECT;
/// Columnas y filas de las dos líneas de una doble línea.
const XL: f32 = 0.5 - DOUBLE_X;
const XR: f32 = 0.5 + DOUBLE_X;
const YT: f32 = 0.5 - DOUBLE_Y;
const YB: f32 = 0.5 + DOUBLE_Y;
/// Centro.
const C: f32 = 0.5;

/// Tramo recto de una esquina redondeada: xterm.js va recto hasta
/// `0,5 ± 0,5 · ancho / alto` y desde ahí traza la curva.
const ROUND_B: f32 = 0.5 + 0.5 * CELL_ASPECT;
const ROUND_T: f32 = 0.5 - 0.5 * CELL_ASPECT;

/// Grosor del contorno de powerline: xterm.js usa `fontSize / 12` px; la
/// letra mide `alto / 1,2`.
const POWERLINE_STROKE: f32 = 1.0 / 12.0 / LINE_HEIGHT;
/// Margen de powerline (`rightPadding: 2` = `fontSize / 12` px), en
/// fracción del ancho.
const POWERLINE_PAD: f32 = POWERLINE_STROKE / CELL_ASPECT;
/// Punta del triángulo relleno U+E0B0 (y la base de U+E0B2).
const PL_TIP: f32 = 1.0 - POWERLINE_PAD;
/// Contorno U+E0B1: `M-1,-.5 L1,.5 L-1,1.5` con margen de medio paso a cada
/// lado (`x · (1 − p) + p / 2`), recortado a `x = 0`.
const PL_OUT: f32 = -1.0 + 1.5 * POWERLINE_PAD;
const PL_IN: f32 = 1.0 - 0.5 * POWERLINE_PAD;
const PL_CUT: f32 = -0.5 + (0.0 - PL_OUT) / (PL_IN - PL_OUT);
const PL_CUT_LOW: f32 = 1.0 - PL_CUT;

/// Tramos con que se aproxima la Bézier de una esquina redondeada.
const CURVE_STEPS: u8 = 8;

/// Cómo se describe un glifo antes de convertirlo en [`BoxOp`].
enum Shape {
    /// Polilíneas (cada una un subtrazo `M…L…L…`) de grosor dado.
    Lines(f32, &'static [&'static [(f32, f32)]]),
    /// Esquina redondeada: `M start L p0 C p1, p2, p3` de xterm.js.
    Round([(f32, f32); 5]),
    /// Rectángulos en octavos de celda: `(x, y, ancho, alto)`.
    Blocks(&'static [(u8, u8, u8, u8)]),
    /// Sombreado de celda completa con esta opacidad.
    Shade(f32),
    /// Triángulo relleno.
    Fill([(f32, f32); 3]),
}

macro_rules! thin {
    ($([$($x:expr, $y:expr);+])+) => {
        Shape::Lines(THIN, &[$(&[$(($x, $y)),+]),+])
    };
}

macro_rules! thick {
    ($([$($x:expr, $y:expr);+])+) => {
        Shape::Lines(THICK, &[$(&[$(($x, $y)),+]),+])
    };
}

/// Definición de cada glifo, en el orden de `CustomGlyphs.ts`.
fn spec(c: char) -> Option<&'static [Shape]> {
    let shapes: &'static [Shape] = match c {
        // Líneas simples y gruesas.
        '─' => &[thin!([0.0, C; 1.0, C])],
        '━' => &[thick!([0.0, C; 1.0, C])],
        '│' => &[thin!([C, 0.0; C, 1.0])],
        '┃' => &[thick!([C, 0.0; C, 1.0])],
        '┌' => &[thin!([C, 1.0; C, C; 1.0, C])],
        '┏' => &[thick!([C, 1.0; C, C; 1.0, C])],
        '┐' => &[thin!([0.0, C; C, C; C, 1.0])],
        '┓' => &[thick!([0.0, C; C, C; C, 1.0])],
        '└' => &[thin!([C, 0.0; C, C; 1.0, C])],
        '┗' => &[thick!([C, 0.0; C, C; 1.0, C])],
        '┘' => &[thin!([C, 0.0; C, C; 0.0, C])],
        '┛' => &[thick!([C, 0.0; C, C; 0.0, C])],
        '├' => &[thin!([C, 0.0; C, 1.0] [C, C; 1.0, C])],
        '┣' => &[thick!([C, 0.0; C, 1.0] [C, C; 1.0, C])],
        '┤' => &[thin!([C, 0.0; C, 1.0] [C, C; 0.0, C])],
        '┫' => &[thick!([C, 0.0; C, 1.0] [C, C; 0.0, C])],
        '┬' => &[thin!([0.0, C; 1.0, C] [C, C; C, 1.0])],
        '┳' => &[thick!([0.0, C; 1.0, C] [C, C; C, 1.0])],
        '┴' => &[thin!([0.0, C; 1.0, C] [C, C; C, 0.0])],
        '┻' => &[thick!([0.0, C; 1.0, C] [C, C; C, 0.0])],
        '┼' => &[thin!([0.0, C; 1.0, C] [C, 0.0; C, 1.0])],
        '╋' => &[thick!([0.0, C; 1.0, C] [C, 0.0; C, 1.0])],
        '╴' => &[thin!([C, C; 0.0, C])],
        '╸' => &[thick!([C, C; 0.0, C])],
        '╵' => &[thin!([C, C; C, 0.0])],
        '╹' => &[thick!([C, C; C, 0.0])],
        '╶' => &[thin!([C, C; 1.0, C])],
        '╺' => &[thick!([C, C; 1.0, C])],
        '╷' => &[thin!([C, C; C, 1.0])],
        '╻' => &[thick!([C, C; C, 1.0])],
        // Dobles líneas.
        '═' => &[thin!([0.0, YT; 1.0, YT] [0.0, YB; 1.0, YB])],
        '║' => &[thin!([XL, 0.0; XL, 1.0] [XR, 0.0; XR, 1.0])],
        '╒' => &[thin!([C, 1.0; C, YT; 1.0, YT] [C, YB; 1.0, YB])],
        '╓' => &[thin!([XL, 1.0; XL, C; 1.0, C] [XR, C; XR, 1.0])],
        '╔' => &[thin!([1.0, YT; XL, YT; XL, 1.0] [1.0, YB; XR, YB; XR, 1.0])],
        '╕' => &[thin!([0.0, YT; C, YT; C, 1.0] [0.0, YB; C, YB])],
        '╖' => &[thin!([XR, 1.0; XR, C; 0.0, C] [XL, C; XL, 1.0])],
        '╗' => &[thin!([0.0, YB; XL, YB; XL, 1.0] [0.0, YT; XR, YT; XR, 1.0])],
        '╘' => &[thin!([C, 0.0; C, YB; 1.0, YB] [C, YT; 1.0, YT])],
        '╙' => &[thin!([1.0, C; XL, C; XL, 0.0] [XR, C; XR, 0.0])],
        '╚' => &[thin!([1.0, YT; XR, YT; XR, 0.0] [1.0, YB; XL, YB; XL, 0.0])],
        '╛' => &[thin!([0.0, YB; C, YB; C, 0.0] [0.0, YT; C, YT])],
        '╜' => &[thin!([0.0, C; XR, C; XR, 0.0] [XL, C; XL, 0.0])],
        '╝' => &[thin!([0.0, YT; XL, YT; XL, 0.0] [0.0, YB; XR, YB; XR, 0.0])],
        '╞' => &[thin!([C, 0.0; C, 1.0] [C, YT; 1.0, YT] [C, YB; 1.0, YB])],
        '╟' => &[thin!([XL, 0.0; XL, 1.0] [XR, 0.0; XR, 1.0] [XR, C; 1.0, C])],
        '╠' => &[thin!(
            [XL, 0.0; XL, 1.0]
            [1.0, YB; XR, YB; XR, 1.0]
            [1.0, YT; XR, YT; XR, 0.0]
        )],
        '╡' => &[thin!([C, 0.0; C, 1.0] [0.0, YT; C, YT] [0.0, YB; C, YB])],
        '╢' => &[thin!([0.0, C; XL, C] [XL, 0.0; XL, 1.0] [XR, 0.0; XR, 1.0])],
        '╣' => &[thin!(
            [XR, 0.0; XR, 1.0]
            [0.0, YB; XL, YB; XL, 1.0]
            [0.0, YT; XL, YT; XL, 0.0]
        )],
        '╤' => &[thin!([0.0, YT; 1.0, YT] [0.0, YB; 1.0, YB] [C, YB; C, 1.0])],
        '╥' => &[thin!([0.0, C; 1.0, C] [XL, C; XL, 1.0] [XR, C; XR, 1.0])],
        '╦' => &[thin!(
            [0.0, YT; 1.0, YT]
            [0.0, YB; XL, YB; XL, 1.0]
            [1.0, YB; XR, YB; XR, 1.0]
        )],
        '╧' => &[thin!([C, 0.0; C, YT] [0.0, YT; 1.0, YT] [0.0, YB; 1.0, YB])],
        '╨' => &[thin!([0.0, C; 1.0, C] [XL, C; XL, 0.0] [XR, C; XR, 0.0])],
        '╩' => &[thin!(
            [0.0, YB; 1.0, YB]
            [0.0, YT; XL, YT; XL, 0.0]
            [1.0, YT; XR, YT; XR, 0.0]
        )],
        '╪' => &[thin!([C, 0.0; C, 1.0] [0.0, YT; 1.0, YT] [0.0, YB; 1.0, YB])],
        '╫' => &[thin!([0.0, C; 1.0, C] [XL, 0.0; XL, 1.0] [XR, 0.0; XR, 1.0])],
        '╬' => &[thin!(
            [0.0, YB; XL, YB; XL, 1.0]
            [1.0, YB; XR, YB; XR, 1.0]
            [0.0, YT; XL, YT; XL, 0.0]
            [1.0, YT; XR, YT; XR, 0.0]
        )],
        // Diagonales.
        '╱' => &[thin!([1.0, 0.0; 0.0, 1.0])],
        '╲' => &[thin!([0.0, 0.0; 1.0, 1.0])],
        '╳' => &[thin!([1.0, 0.0; 0.0, 1.0] [0.0, 0.0; 1.0, 1.0])],
        // Mitad fina, mitad gruesa.
        '╼' => &[thin!([C, C; 0.0, C]), thick!([C, C; 1.0, C])],
        '╽' => &[thin!([C, C; C, 0.0]), thick!([C, C; C, 1.0])],
        '╾' => &[thin!([C, C; 1.0, C]), thick!([C, C; 0.0, C])],
        '╿' => &[thin!([C, C; C, 1.0]), thick!([C, C; C, 0.0])],
        '┍' => &[thin!([C, C; C, 1.0]), thick!([C, C; 1.0, C])],
        '┎' => &[thin!([C, C; 1.0, C]), thick!([C, C; C, 1.0])],
        '┑' => &[thin!([C, C; C, 1.0]), thick!([C, C; 0.0, C])],
        '┒' => &[thin!([C, C; 0.0, C]), thick!([C, C; C, 1.0])],
        '┕' => &[thin!([C, C; C, 0.0]), thick!([C, C; 1.0, C])],
        '┖' => &[thin!([C, C; 1.0, C]), thick!([C, C; C, 0.0])],
        '┙' => &[thin!([C, C; C, 0.0]), thick!([C, C; 0.0, C])],
        '┚' => &[thin!([C, C; 0.0, C]), thick!([C, C; C, 0.0])],
        '┝' => &[thin!([C, 0.0; C, 1.0]), thick!([C, C; 1.0, C])],
        '┞' => &[thin!([C, 1.0; C, C; 1.0, C]), thick!([C, C; C, 0.0])],
        '┟' => &[thin!([C, 0.0; C, C; 1.0, C]), thick!([C, C; C, 1.0])],
        '┠' => &[thin!([C, C; 1.0, C]), thick!([C, 0.0; C, 1.0])],
        '┡' => &[thin!([C, C; C, 1.0]), thick!([C, 0.0; C, C; 1.0, C])],
        '┢' => &[thin!([C, C; C, 0.0]), thick!([C, 1.0; C, C; 1.0, C])],
        '┥' => &[thin!([C, 0.0; C, 1.0]), thick!([C, C; 0.0, C])],
        '┦' => &[thin!([0.0, C; C, C; C, 1.0]), thick!([C, C; C, 0.0])],
        '┧' => &[thin!([C, 0.0; C, C; 0.0, C]), thick!([C, C; C, 1.0])],
        '┨' => &[thin!([C, C; 0.0, C]), thick!([C, 0.0; C, 1.0])],
        '┩' => &[thin!([C, C; C, 1.0]), thick!([C, 0.0; C, C; 0.0, C])],
        '┪' => &[thin!([C, C; C, 0.0]), thick!([0.0, C; C, C; C, 1.0])],
        '┭' => &[thin!([C, 1.0; C, C; 1.0, C]), thick!([C, C; 0.0, C])],
        '┮' => &[thin!([0.0, C; C, C; C, 1.0]), thick!([C, C; 1.0, C])],
        '┯' => &[thin!([C, C; C, 1.0]), thick!([0.0, C; 1.0, C])],
        '┰' => &[thin!([0.0, C; 1.0, C]), thick!([C, C; C, 1.0])],
        '┱' => &[thin!([C, C; 1.0, C]), thick!([0.0, C; C, C; C, 1.0])],
        '┲' => &[thin!([C, C; 0.0, C]), thick!([C, 1.0; C, C; 1.0, C])],
        '┵' => &[thin!([C, 0.0; C, C; 1.0, C]), thick!([C, C; 0.0, C])],
        '┶' => &[thin!([C, 0.0; C, C; 0.0, C]), thick!([C, C; 1.0, C])],
        '┷' => &[thin!([C, C; C, 0.0]), thick!([0.0, C; 1.0, C])],
        '┸' => &[thin!([0.0, C; 1.0, C]), thick!([C, C; C, 0.0])],
        '┹' => &[thin!([C, C; 1.0, C]), thick!([C, 0.0; C, C; 0.0, C])],
        '┺' => &[thin!([C, C; 0.0, C]), thick!([C, 0.0; C, C; 1.0, C])],
        '┽' => &[
            thin!([C, 0.0; C, 1.0] [C, C; 1.0, C]),
            thick!([C, C; 0.0, C]),
        ],
        '┾' => &[
            thin!([C, 0.0; C, 1.0] [C, C; 0.0, C]),
            thick!([C, C; 1.0, C]),
        ],
        '┿' => &[thin!([C, 0.0; C, 1.0]), thick!([0.0, C; 1.0, C])],
        '╀' => &[
            thin!([0.0, C; 1.0, C] [C, C; C, 1.0]),
            thick!([C, C; C, 0.0]),
        ],
        '╁' => &[
            thin!([C, C; C, 0.0] [0.0, C; 1.0, C]),
            thick!([C, C; C, 1.0]),
        ],
        '╂' => &[thin!([0.0, C; 1.0, C]), thick!([C, 0.0; C, 1.0])],
        '╃' => &[
            thin!([C, 1.0; C, C; 1.0, C]),
            thick!([C, 0.0; C, C; 0.0, C]),
        ],
        '╄' => &[
            thin!([0.0, C; C, C; C, 1.0]),
            thick!([C, 0.0; C, C; 1.0, C]),
        ],
        '╅' => &[
            thin!([C, 0.0; C, C; 1.0, C]),
            thick!([0.0, C; C, C; C, 1.0]),
        ],
        '╆' => &[
            thin!([C, 0.0; C, C; 0.0, C]),
            thick!([C, 1.0; C, C; 1.0, C]),
        ],
        '╇' => &[
            thin!([C, C; C, 1.0]),
            thick!([C, C; C, 0.0] [0.0, C; 1.0, C]),
        ],
        '╈' => &[
            thin!([C, C; C, 0.0]),
            thick!([0.0, C; 1.0, C] [C, C; C, 1.0]),
        ],
        '╉' => &[
            thin!([C, C; 1.0, C]),
            thick!([C, 0.0; C, 1.0] [C, C; 0.0, C]),
        ],
        '╊' => &[
            thin!([C, C; 0.0, C]),
            thick!([C, 0.0; C, 1.0] [C, C; 1.0, C]),
        ],
        // Discontinuas.
        '╌' => &[thin!([0.1, C; 0.4, C] [0.6, C; 0.9, C])],
        '╍' => &[thick!([0.1, C; 0.4, C] [0.6, C; 0.9, C])],
        '┄' => &[thin!(
            [0.0667, C; 0.2667, C]
            [0.4, C; 0.6, C]
            [0.7333, C; 0.9333, C]
        )],
        '┅' => &[thick!(
            [0.0667, C; 0.2667, C]
            [0.4, C; 0.6, C]
            [0.7333, C; 0.9333, C]
        )],
        '┈' => &[thin!(
            [0.05, C; 0.2, C]
            [0.3, C; 0.45, C]
            [0.55, C; 0.7, C]
            [0.8, C; 0.95, C]
        )],
        '┉' => &[thick!(
            [0.05, C; 0.2, C]
            [0.3, C; 0.45, C]
            [0.55, C; 0.7, C]
            [0.8, C; 0.95, C]
        )],
        '╎' => &[thin!([C, 0.1; C, 0.4] [C, 0.6; C, 0.9])],
        '╏' => &[thick!([C, 0.1; C, 0.4] [C, 0.6; C, 0.9])],
        '┆' => &[thin!(
            [C, 0.0667; C, 0.2667]
            [C, 0.4; C, 0.6]
            [C, 0.7333; C, 0.9333]
        )],
        '┇' => &[thick!(
            [C, 0.0667; C, 0.2667]
            [C, 0.4; C, 0.6]
            [C, 0.7333; C, 0.9333]
        )],
        // xterm.js define ┊/┋ con un tramo de 0,3 a 0,55 (no simétrico).
        '┊' => &[thin!(
            [C, 0.05; C, 0.2]
            [C, 0.3; C, 0.45; C, 0.55]
            [C, 0.7; C, 0.95]
        )],
        '┋' => &[thick!(
            [C, 0.05; C, 0.2]
            [C, 0.3; C, 0.45; C, 0.55]
            [C, 0.7; C, 0.95]
        )],
        // Esquinas redondeadas.
        '╭' => &[Shape::Round([
            (C, 1.0),
            (C, ROUND_B),
            (C, ROUND_B),
            (C, C),
            (1.0, C),
        ])],
        '╮' => &[Shape::Round([
            (C, 1.0),
            (C, ROUND_B),
            (C, ROUND_B),
            (C, C),
            (0.0, C),
        ])],
        '╯' => &[Shape::Round([
            (C, 0.0),
            (C, ROUND_T),
            (C, ROUND_T),
            (C, C),
            (0.0, C),
        ])],
        '╰' => &[Shape::Round([
            (C, 0.0),
            (C, ROUND_T),
            (C, ROUND_T),
            (C, C),
            (1.0, C),
        ])],
        // Bloques (en octavos).
        '▀' => &[Shape::Blocks(&[(0, 0, 8, 4)])],
        '▁' => &[Shape::Blocks(&[(0, 7, 8, 1)])],
        '▂' => &[Shape::Blocks(&[(0, 6, 8, 2)])],
        '▃' => &[Shape::Blocks(&[(0, 5, 8, 3)])],
        '▄' => &[Shape::Blocks(&[(0, 4, 8, 4)])],
        '▅' => &[Shape::Blocks(&[(0, 3, 8, 5)])],
        '▆' => &[Shape::Blocks(&[(0, 2, 8, 6)])],
        '▇' => &[Shape::Blocks(&[(0, 1, 8, 7)])],
        '█' => &[Shape::Blocks(&[(0, 0, 8, 8)])],
        '▉' => &[Shape::Blocks(&[(0, 0, 7, 8)])],
        '▊' => &[Shape::Blocks(&[(0, 0, 6, 8)])],
        '▋' => &[Shape::Blocks(&[(0, 0, 5, 8)])],
        '▌' => &[Shape::Blocks(&[(0, 0, 4, 8)])],
        '▍' => &[Shape::Blocks(&[(0, 0, 3, 8)])],
        '▎' => &[Shape::Blocks(&[(0, 0, 2, 8)])],
        '▏' => &[Shape::Blocks(&[(0, 0, 1, 8)])],
        '▐' => &[Shape::Blocks(&[(4, 0, 4, 8)])],
        '░' => &[Shape::Shade(0.25)],
        '▒' => &[Shape::Shade(0.5)],
        '▓' => &[Shape::Shade(0.75)],
        '▔' => &[Shape::Blocks(&[(0, 0, 8, 1)])],
        '▕' => &[Shape::Blocks(&[(7, 0, 1, 8)])],
        '▖' => &[Shape::Blocks(&[(0, 4, 4, 4)])],
        '▗' => &[Shape::Blocks(&[(4, 4, 4, 4)])],
        '▘' => &[Shape::Blocks(&[(0, 0, 4, 4)])],
        '▙' => &[Shape::Blocks(&[(0, 0, 4, 8), (0, 4, 8, 4)])],
        '▚' => &[Shape::Blocks(&[(0, 0, 4, 4), (4, 4, 4, 4)])],
        '▛' => &[Shape::Blocks(&[(0, 0, 4, 8), (4, 0, 4, 4)])],
        '▜' => &[Shape::Blocks(&[(0, 0, 8, 4), (4, 0, 4, 8)])],
        '▝' => &[Shape::Blocks(&[(4, 0, 4, 4)])],
        '▞' => &[Shape::Blocks(&[(4, 0, 4, 4), (0, 4, 4, 4)])],
        '▟' => &[Shape::Blocks(&[(4, 0, 4, 8), (0, 4, 8, 4)])],
        // Powerline: flechas rellenas (tipo 0) y contornos (tipo 1).
        '\u{E0B0}' => &[Shape::Fill([(0.0, 0.0), (PL_TIP, C), (0.0, 1.0)])],
        '\u{E0B1}' => &[Shape::Lines(
            POWERLINE_STROKE,
            &[&[(0.0, PL_CUT), (PL_IN, C), (0.0, PL_CUT_LOW)]],
        )],
        '\u{E0B2}' => &[Shape::Fill([(1.0, 0.0), (POWERLINE_PAD, C), (1.0, 1.0)])],
        '\u{E0B3}' => &[Shape::Lines(
            POWERLINE_STROKE,
            &[&[(1.0, PL_CUT), (1.0 - PL_IN, C), (1.0, PL_CUT_LOW)]],
        )],
        _ => return None,
    };
    Some(shapes)
}

/// Primer código de cada bloque de la tabla y cuántos glifos tiene.
const BOX_FIRST: u32 = 0x2500;
const BOX_COUNT: u32 = 0xA0;
const POWERLINE_FIRST: u32 = 0xE0B0;
const POWERLINE_COUNT: u32 = 4;

/// Posición de `c` en la tabla, si es un glifo dibujado.
fn slot(c: char) -> Option<usize> {
    let code = u32::from(c);
    let index = if (BOX_FIRST..BOX_FIRST + BOX_COUNT).contains(&code) {
        code - BOX_FIRST
    } else if (POWERLINE_FIRST..POWERLINE_FIRST + POWERLINE_COUNT).contains(&code) {
        BOX_COUNT + code - POWERLINE_FIRST
    } else {
        return None;
    };
    usize::try_from(index).ok()
}

/// Operaciones de cada glifo, construidas una sola vez.
static TABLE: OnceLock<Vec<Box<[BoxOp]>>> = OnceLock::new();

fn build_table() -> Vec<Box<[BoxOp]>> {
    (BOX_FIRST..BOX_FIRST + BOX_COUNT)
        .chain(POWERLINE_FIRST..POWERLINE_FIRST + POWERLINE_COUNT)
        .map(|code| {
            let mut ops = Vec::new();
            if let Some(shapes) = char::from_u32(code).and_then(spec) {
                for shape in shapes {
                    push_shape(&mut ops, shape);
                }
            }
            ops.into_boxed_slice()
        })
        .collect()
}

/// Cómo dibuja xterm.js 5.5.0 el carácter `c` con `customGlyphs`, o `None`
/// si lo pinta la fuente.
pub fn box_ops(c: char) -> Option<&'static [BoxOp]> {
    let index = slot(c)?;
    let ops = TABLE.get_or_init(build_table).get(index)?;
    (!ops.is_empty()).then_some(&**ops)
}

fn push_shape(out: &mut Vec<BoxOp>, shape: &Shape) {
    match shape {
        Shape::Lines(w, polylines) => {
            for points in polylines.iter() {
                if is_axis_aligned(points) {
                    push_axis_lines(out, *w, points);
                } else {
                    push_stroke(out, *w, points, None);
                }
            }
        }
        Shape::Round([start, p0, p1, p2, p3]) => {
            let mut points = Vec::with_capacity(usize::from(CURVE_STEPS) + 2);
            points.push(*start);
            points.push(*p0);
            for step in 1..=CURVE_STEPS {
                let t = f32::from(step) / f32::from(CURVE_STEPS);
                points.push(cubic(*p0, *p1, *p2, *p3, t));
            }
            // El final usa la tangente exacta de la curva (`p3 − p2`): el
            // extremo queda recto contra el borde de la celda.
            push_stroke(out, THIN, &points, Some((p3.0 - p2.0, p3.1 - p2.1)));
        }
        Shape::Blocks(rects) => {
            for &(x, y, w, h) in rects.iter() {
                out.push(BoxOp::Rect {
                    x: f32::from(x) / 8.0,
                    y: f32::from(y) / 8.0,
                    w: f32::from(w) / 8.0,
                    h: f32::from(h) / 8.0,
                    alpha: 1.0,
                });
            }
        }
        Shape::Shade(alpha) => out.push(BoxOp::Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
            alpha: *alpha,
        }),
        Shape::Fill([a, b, c]) => push_tri(out, *a, *b, *c),
    }
}

fn cubic(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), t: f32) -> (f32, f32) {
    let mt = 1.0 - t;
    let (a, b, c, d) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
    (
        a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
        a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
    )
}

fn is_axis_aligned(points: &[(f32, f32)]) -> bool {
    points.windows(2).all(|pair| match pair {
        [a, b] => a.0 == b.0 || a.1 == b.1,
        _ => true,
    })
}

/// Polilínea de tramos horizontales y verticales. En un vértice interior
/// (una unión dentro del mismo subtrazo) los dos tramos se prolongan medio
/// grosor, como la unión «miter» de xterm.js: sin eso la esquina queda con
/// una muesca. Los extremos libres no se prolongan (`lineCap: butt`).
fn push_axis_lines(out: &mut Vec<BoxOp>, w: f32, points: &[(f32, f32)]) {
    let last = points.len().saturating_sub(2);
    for (i, pair) in points.windows(2).enumerate() {
        let [a, b] = pair else { continue };
        let (joined_start, joined_end) = (i > 0, i < last);
        if a.1 == b.1 {
            // Medio grosor (fracción del alto) en fracción del ancho.
            let ext = w / 2.0 / CELL_ASPECT;
            let (x0, x1, ext0, ext1) = if a.0 <= b.0 {
                (a.0, b.0, joined_start, joined_end)
            } else {
                (b.0, a.0, joined_end, joined_start)
            };
            out.push(BoxOp::HLine {
                y: a.1,
                x0: clamp01(if ext0 { x0 - ext } else { x0 }),
                x1: clamp01(if ext1 { x1 + ext } else { x1 }),
                w,
            });
        } else {
            let ext = w / 2.0;
            let (y0, y1, ext0, ext1) = if a.1 <= b.1 {
                (a.1, b.1, joined_start, joined_end)
            } else {
                (b.1, a.1, joined_end, joined_start)
            };
            out.push(BoxOp::VLine {
                x: a.0,
                y0: clamp01(if ext0 { y0 - ext } else { y0 }),
                y1: clamp01(if ext1 { y1 + ext } else { y1 }),
                w,
            });
        }
    }
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Trazo de grosor `w` (fracción del alto) a lo largo de una polilínea con
/// tramos oblicuos, como triángulos. Se calcula en un espacio de píxeles
/// cuadrados (`x · CELL_ASPECT`) para que el grosor no dependa de la
/// dirección; uniones «miter» (acotadas) y extremos rectos. `end_dir` fija
/// la dirección del último extremo (tangente de una curva).
fn push_stroke(out: &mut Vec<BoxOp>, w: f32, points: &[(f32, f32)], end_dir: Option<(f32, f32)>) {
    let mut square: Vec<(f32, f32)> = Vec::with_capacity(points.len());
    for &(x, y) in points {
        let p = (x * CELL_ASPECT, y);
        if square.last() != Some(&p) {
            square.push(p);
        }
    }
    let half = w / 2.0;
    let count = square.len();
    let mut sides: Vec<((f32, f32), (f32, f32))> = Vec::with_capacity(count);
    for (i, &p) in square.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|j| square.get(j));
        let next = square.get(i + 1);
        let offset = match (prev, next) {
            (None, Some(n)) => scale(normal(sub(*n, p)), half),
            (Some(q), None) => {
                let dir = end_dir.map_or_else(|| sub(p, *q), |(dx, dy)| (dx * CELL_ASPECT, dy));
                scale(normal(dir), half)
            }
            (Some(q), Some(n)) => {
                let (n1, n2) = (normal(sub(p, *q)), normal(sub(*n, p)));
                let miter = unit(add(n1, n2)).unwrap_or(n1);
                // Unión «miter» con el límite de 4 medios grosores.
                let cos = dot(miter, n1).max(0.25);
                scale(miter, half / cos)
            }
            (None, None) => continue,
        };
        sides.push((add(p, offset), sub(p, offset)));
    }
    let to_cell = |(x, y): (f32, f32)| (x / CELL_ASPECT, y);
    for pair in sides.windows(2) {
        let [(l0, r0), (l1, r1)] = pair else { continue };
        let (l0, r0, l1, r1) = (to_cell(*l0), to_cell(*r0), to_cell(*l1), to_cell(*r1));
        push_tri(out, l0, r0, r1);
        push_tri(out, l0, r1, l1);
    }
}

/// Añade un triángulo con la orientación común (área con signo positiva);
/// descarta los degenerados.
fn push_tri(out: &mut Vec<BoxOp>, a: (f32, f32), b: (f32, f32), c: (f32, f32)) {
    let area = (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1);
    if area.abs() < 1e-7 {
        return;
    }
    let pts = if area > 0.0 { [a, b, c] } else { [a, c, b] };
    out.push(BoxOp::Tri { pts });
}

fn add(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0 + b.0, a.1 + b.1)
}

fn sub(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0 - b.0, a.1 - b.1)
}

fn scale(a: (f32, f32), k: f32) -> (f32, f32) {
    (a.0 * k, a.1 * k)
}

fn dot(a: (f32, f32), b: (f32, f32)) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

fn unit(a: (f32, f32)) -> Option<(f32, f32)> {
    let len = dot(a, a).sqrt();
    (len > 1e-6).then(|| scale(a, 1.0 / len))
}

/// Normal unitaria (girada 90°) de una dirección; cero si es nula.
fn normal(dir: (f32, f32)) -> (f32, f32) {
    unit((-dir.1, dir.0)).unwrap_or((0.0, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_covers_exactly_the_drawn_ranges() {
        let table = TABLE.get_or_init(build_table);
        assert_eq!(table.len(), 164);
        assert!(table.iter().all(|ops| !ops.is_empty()));
        assert_eq!(slot('\u{24FF}'), None);
        assert_eq!(slot('\u{2500}'), Some(0));
        assert_eq!(slot('\u{E0B3}'), Some(163));
    }

    #[test]
    fn powerline_outline_is_clipped_at_the_cell_edge() {
        // El contorno entra en la celda por x = 0 a la altura de la línea
        // original de xterm.js (cerca de las esquinas).
        const { assert!(PL_CUT > -0.1 && PL_CUT < 0.1 && PL_CUT_LOW > 0.9) };
        const { assert!(PL_TIP > 0.8 && PL_TIP < 1.0) };
    }
}
