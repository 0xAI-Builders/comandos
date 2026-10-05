//! Cota de las cadenas OSC antes de que lleguen a vte.
//!
//! Con la característica `std` (que `alacritty_terminal` exige), vte guarda el
//! cuerpo de un OSC en un `Vec` sin límite hasta ver el terminador; su buffer
//! no se puede acotar por configuración sin parchear vte. Este filtro retiene
//! cada OSC (desde el `]` que abre el OSC hasta BEL, CAN, SUB o ESC) y solo lo
//! entrega completo; si pasa de [`MAX_OSC_BYTES`] lo descarta entero, de modo
//! que vte nunca procesa un OSC parcial (un OSC 52 cortado copiaría basura) y
//! su buffer queda acotado por esta misma cifra.
//!
//! ## Modelo del estado `Escape` de vte 0.15.0
//!
//! Para saber qué `]` abre un OSC el filtro replica exactamente cuándo vte está
//! en `State::Escape` (`vte-0.15.0/src/lib.rs`):
//!
//! - **Se entra** con ESC desde cualquier estado: tierra (`advance_ground`,
//!   l. 603–606 y 615–619, también con UTF-8 cortado, l. 647–652), `anywhere`
//!   (l. 444–447: CSI, DCS ignorado, SOS/PM/APC y el resto que delega en él),
//!   DCS passthrough (l. 325–329), OSC (`advance_osc_string`, l. 419–423) y el
//!   propio `Escape` (l. 387, ESC lo deja en `Escape`). Un carácter UTF-8
//!   partido entre llamadas nunca se traga el ESC (`advance_partial_utf8`,
//!   l. 704–714, devuelve 0 bytes consumidos si el ESC lo invalida).
//! - **Se sigue** en `Escape` (`advance_esc`, l. 340–390) con los C0 que no son
//!   CAN, SUB ni ESC (l. 342: se ejecutan sin salir), con DEL y con cualquier
//!   byte 0x80–0xFF (l. 388, `_ => ()`), y con ESC (l. 387).
//! - **Se sale** con CAN o SUB (l. 383–386, a tierra), con 0x20–0x2F (l. 343–346,
//!   a `EscapeIntermediate`, donde `]` ya no abre OSC) y con 0x30–0x7E
//!   (l. 347–381). Solo `]` (0x5D, l. 372–376) lleva a `OscString`.
//!
//! En `OscString` (`advance_osc_string`, l. 407–434) terminan el OSC BEL,
//! CAN, SUB (que además se ejecutan: l. 414–418) y ESC; 0x9C no lo termina.

/// Tope de un OSC: cabe un OSC 52 con 1 MiB de texto en base64 (≈ 1,34 MiB)
/// más su cabecera. xterm.js admite 10 MB; aquí se prefiere menos memoria.
pub const MAX_OSC_BYTES: usize = 1_536 * 1024;

const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1A;
const ESC: u8 = 0x1B;
const OSC_START: u8 = b']';

#[derive(Debug, Default)]
enum State {
    /// Los bytes pasan tal cual.
    #[default]
    Pass,
    /// Dentro de un OSC: se retiene desde el `]`.
    Buffer(Vec<u8>),
    /// OSC demasiado largo: se tira hasta su terminador.
    Discard,
}

/// Filtro con estado entre llamadas (un OSC puede llegar en varios trozos).
#[derive(Debug)]
pub struct OscLimiter {
    /// vte está en `State::Escape` tras lo ya entregado.
    in_escape: bool,
    state: State,
    limit: usize,
}

impl Default for OscLimiter {
    fn default() -> OscLimiter {
        OscLimiter::with_limit(MAX_OSC_BYTES)
    }
}

/// ¿Sigue vte en `Escape` tras recibir `byte` estando en `Escape`? (`]` se
/// trata aparte: abre el OSC.)
fn stays_in_escape(byte: u8) -> bool {
    match byte {
        CAN | SUB | 0x20..=0x7E => false,
        // ESC, los demás C0, DEL y 0x80–0xFF: sigue (vte l. 342, 387, 388).
        _ => true,
    }
}

impl OscLimiter {
    /// Filtro con otro tope (las pruebas lo usan pequeño).
    #[doc(hidden)]
    pub fn with_limit(limit: usize) -> OscLimiter {
        OscLimiter {
            in_escape: false,
            state: State::Pass,
            limit,
        }
    }

    /// Pasa `input` por el filtro y entrega a `feed`, en orden, lo que vte
    /// debe procesar.
    pub fn filter(&mut self, input: &[u8], mut feed: impl FnMut(&[u8])) {
        let mut start = 0;
        let mut i = 0;
        while let Some(&byte) = input.get(i) {
            match &mut self.state {
                State::Pass => {
                    if !self.in_escape {
                        // Fuera de `Escape` solo un ESC cambia algo: salto
                        // directo al siguiente.
                        let rest = input.get(i..).unwrap_or_default();
                        match rest.iter().position(|&b| b == ESC) {
                            Some(offset) => {
                                i += offset + 1;
                                self.in_escape = true;
                                continue;
                            }
                            None => break,
                        }
                    }
                    if byte == OSC_START {
                        feed(input.get(start..i).unwrap_or_default());
                        self.state = State::Buffer(vec![byte]);
                        self.in_escape = false;
                    } else {
                        self.in_escape = stays_in_escape(byte);
                    }
                }
                State::Buffer(buffer) => {
                    buffer.push(byte);
                    if matches!(byte, BEL | CAN | SUB | ESC) {
                        feed(buffer);
                        self.state = State::Pass;
                        self.in_escape = byte == ESC;
                        start = i + 1;
                    } else if buffer.len() > self.limit {
                        // vte está en `Escape` (vio el ESC y quizá C0, DEL o
                        // bytes altos): CAN lo devuelve a tierra sin efecto
                        // (alacritty no hace nada con CAN).
                        feed(&[CAN]);
                        self.state = State::Discard;
                        self.in_escape = false;
                    }
                }
                State::Discard => match byte {
                    // BEL solo cerraba el OSC (no suena): se tira.
                    BEL => {
                        self.state = State::Pass;
                        start = i + 1;
                    }
                    // CAN y SUB se ejecutan además de cerrar (SUB llama a
                    // `substitute()`), y ESC deja a vte en `Escape`: pasan.
                    CAN | SUB | ESC => {
                        self.state = State::Pass;
                        self.in_escape = byte == ESC;
                        start = i;
                    }
                    _ => {}
                },
            }
            i += 1;
        }
        if matches!(self.state, State::Pass) {
            feed(input.get(start..).unwrap_or_default());
        }
    }
    /// Bytes retenidos ahora mismo (OSC sin terminar).
    pub fn buffered_len(&self) -> usize {
        match &self.state {
            State::Buffer(buffer) => buffer.len(),
            State::Pass | State::Discard => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(limiter: &mut OscLimiter, chunks: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in chunks {
            limiter.filter(chunk, |b| out.extend_from_slice(b));
        }
        out
    }

    #[test]
    fn plain_text_and_short_osc_pass_unchanged() {
        let mut l = OscLimiter::default();
        let input: &[u8] = b"a\x1b[1mb\x1b]0;hola\x07c\x1b]52;c;aG9s\x1b\\d";
        assert_eq!(run(&mut l, &[input]), input);
    }

    #[test]
    fn osc_split_across_chunks_is_delivered_whole_and_in_order() {
        let mut l = OscLimiter::default();
        let out = run(&mut l, &[b"x\x1b", b"]0;ti", b"tle\x07y"]);
        assert_eq!(out, b"x\x1b]0;title\x07y");
    }

    #[test]
    fn oversized_osc_is_dropped_and_buffer_stays_bounded() {
        let mut l = OscLimiter::default();
        let mut out = Vec::new();
        l.filter(b"\x1b]52;c;", |b| out.extend_from_slice(b));
        let chunk = vec![b'A'; 64 * 1024];
        for _ in 0..48 {
            l.filter(&chunk, |b| out.extend_from_slice(b));
            assert!(l.buffered_len() <= MAX_OSC_BYTES + 1);
        }
        l.filter(b"\x07ok", |b| out.extend_from_slice(b));
        assert_eq!(out, b"\x1b\x18ok");
        assert_eq!(l.buffered_len(), 0);
    }

    #[test]
    fn oversized_osc_ended_by_st_lets_the_escape_through() {
        let mut l = OscLimiter::default();
        let big = vec![b'A'; MAX_OSC_BYTES + 10];
        let out = run(&mut l, &[b"\x1b]0;", &big, b"\x1b\\z"]);
        assert_eq!(out, b"\x1b\x18\x1b\\z");
    }
}
