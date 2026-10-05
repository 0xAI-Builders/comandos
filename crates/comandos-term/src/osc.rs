//! Cota de las cadenas OSC antes de que lleguen a vte.
//!
//! Con la característica `std` (que `alacritty_terminal` exige), vte guarda el
//! cuerpo de un OSC en un `Vec` sin límite hasta ver el terminador; su buffer
//! no se puede acotar por configuración sin parchear vte. Este filtro retiene
//! cada OSC (desde el `]` que sigue a un ESC hasta BEL, CAN, SUB o ESC) y solo
//! lo entrega completo; si pasa de [`MAX_OSC_BYTES`] lo descarta entero, de
//! modo que vte nunca procesa un OSC parcial (un OSC 52 cortado copiaría
//! basura) y su buffer queda acotado por esta misma cifra.

/// Tope de un OSC: cabe un OSC 52 con 1 MiB de texto en base64 (≈ 1,34 MiB)
/// más su cabecera. xterm.js admite 10 MB; aquí se prefiere menos memoria.
pub const MAX_OSC_BYTES: usize = 1_536 * 1024;

const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1A;
const ESC: u8 = 0x1B;

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
#[derive(Debug, Default)]
pub struct OscLimiter {
    /// El último byte entregado a vte fue ESC.
    last_was_esc: bool,
    state: State,
}

impl OscLimiter {
    /// Pasa `input` por el filtro y entrega a `feed`, en orden, lo que vte
    /// debe procesar.
    pub fn filter(&mut self, input: &[u8], mut feed: impl FnMut(&[u8])) {
        let mut start = 0;
        for (i, &byte) in input.iter().enumerate() {
            match &mut self.state {
                State::Pass => {
                    if self.last_was_esc && byte == b']' {
                        feed(input.get(start..i).unwrap_or_default());
                        self.state = State::Buffer(vec![byte]);
                        self.last_was_esc = false;
                    } else {
                        self.last_was_esc = byte == ESC;
                    }
                }
                State::Buffer(buffer) => {
                    buffer.push(byte);
                    if matches!(byte, BEL | CAN | SUB | ESC) {
                        feed(buffer);
                        self.state = State::Pass;
                        self.last_was_esc = byte == ESC;
                        start = i + 1;
                    } else if buffer.len() > MAX_OSC_BYTES {
                        // vte ya vio el ESC: CAN lo devuelve a tierra sin
                        // efecto (alacritty no hace nada con CAN).
                        feed(&[CAN]);
                        self.state = State::Discard;
                        self.last_was_esc = false;
                    }
                }
                State::Discard => match byte {
                    BEL | CAN | SUB => {
                        self.state = State::Pass;
                        start = i + 1;
                    }
                    ESC => {
                        // El ESC (p. ej. de ST) pasa con lo que siga.
                        self.state = State::Pass;
                        self.last_was_esc = true;
                        start = i;
                    }
                    _ => {}
                },
            }
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
