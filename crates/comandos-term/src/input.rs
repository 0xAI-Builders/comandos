//! Entrada de la terminal: teclado, pegado, ratón, rueda, foco, IME y
//! teclados en pantalla. Puro (sin DOM): la capa web traduce los eventos del
//! navegador a estos tipos y escribe los bytes que salen.
//!
//! La referencia es `Keyboard.evaluateKeyboardEvent` de xterm.js 5.5.0 en
//! Linux con `macOptionIsMeta = false`, más `CoreMouseService`, `paste` y el
//! manejador de rueda de `Terminal`. xterm.js decide por `keyCode`, que la
//! capa web pasa tal cual en [`KeyInput::key_code`] (paridad exacta en
//! cualquier distribución de teclado, teclado numérico incluido).
//!
//! ## Contrato con quien llama
//!
//! * `encode_key` devuelve `Send`/`ScrollPage` para teclas que xterm.js
//!   consume: en ese caso hay que hacer `preventDefault()` del `keydown` (así
//!   no se duplica por `beforeinput`). Con `None` el evento no se toca.
//!   Los caracteres imprimibles (incluidos espacio y mayúsculas, que xterm.js
//!   manda por `keypress`) salen aquí con los mismos bytes.
//! * Alt+tecla muerta no envía nada, como xterm.js; este además marca la tecla
//!   muerta como pendiente y se traga la siguiente que produzca bytes: esa
//!   parte (`_unprocessedDeadKey`) es de la capa web.
//! * El ratón en modo X10 clásico (sin `sgr_mouse`) emite bytes crudos, que
//!   pueden pasar de 0x7f: no son UTF-8 y hay que transportarlos como binario.
//! * xterm.js descarta un `Move` idéntico al anterior (misma celda, botón y
//!   modificadores). Aquí no hay estado: esa deduplicación es de quien llama.
//! * [`wheel`] debe llamarse una vez por evento `wheel` del DOM, no por línea.
//!
//! ## Diferencia deliberada con xterm.js
//!
//! El pegado entre corchetes quita todo `ESC[201~` del texto. xterm.js 5.5.0
//! no lo hace, y sin ello un portapapeles hostil cierra el pegado y ejecuta el
//! resto como si lo hubiera escrito el usuario (inyección de bracketed
//! paste). Excepción de seguridad aprobada: es la única diferencia de bytes.

use crate::engine::{Modes, MouseMode};

const ESC: u8 = 0x1b;

/// Tecla pulsada, tal como la da `KeyboardEvent`.
#[derive(Debug, Clone, Copy)]
pub struct KeyInput<'a> {
    pub key: &'a str,
    pub code: &'a str,
    /// `KeyboardEvent.keyCode`: xterm.js decide por él (Backspace 8, flechas
    /// 37–40, F1–F12 112–123, letras 65–90, teclado numérico 96–111…). 0 si
    /// el navegador no lo da; solo se reconocen entonces las teclas
    /// `UIKeyInput*` de iOS y el texto.
    pub key_code: u32,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

/// Qué hacer con una tecla.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyAction {
    /// Bytes para el PTY.
    Send(Vec<u8>),
    /// Desplazar la vista local una página (−1 arriba, +1 abajo).
    ScrollPage(i32),
    /// Nada que enviar.
    None,
}

/// Agrega un número de hasta tres cifras en decimal.
fn push_decimal(out: &mut Vec<u8>, n: u8) {
    if n >= 100 {
        out.push(b'0' + n / 100);
    }
    if n >= 10 {
        out.push(b'0' + (n / 10) % 10);
    }
    out.push(b'0' + n % 10);
}

/// `ESC [ 1 ; <p> <final>` (cursor, Home/End, F1–F4 con modificadores).
fn csi_mod_final(param: u8, fin: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(8);
    v.extend_from_slice(&[ESC, b'[', b'1', b';']);
    push_decimal(&mut v, param);
    v.push(fin);
    v
}

/// `ESC [ <n> ; <p> ~` (Delete, PageUp/Down, F5–F12 con modificadores).
fn csi_tilde_mod(n: u8, param: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(9);
    v.extend_from_slice(&[ESC, b'[']);
    push_decimal(&mut v, n);
    v.push(b';');
    push_decimal(&mut v, param);
    v.push(b'~');
    v
}

/// `ESC [ <n> ~` sin modificadores.
fn csi_tilde(n: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(6);
    v.extend_from_slice(&[ESC, b'[']);
    push_decimal(&mut v, n);
    v.push(b'~');
    v
}

/// `ESC <intermedio> <final>` (`ESC [ A` o `ESC O A`).
fn esc2(mid: u8, fin: u8) -> Vec<u8> {
    vec![ESC, mid, fin]
}

/// Tabla `KEYCODE_KEY_MAPPINGS` de xterm.js: (keyCode, sin shift, con shift).
const ALT_KEYS: [(u32, u8, u8); 21] = [
    (48, b'0', b')'),
    (49, b'1', b'!'),
    (50, b'2', b'@'),
    (51, b'3', b'#'),
    (52, b'4', b'$'),
    (53, b'5', b'%'),
    (54, b'6', b'^'),
    (55, b'7', b'&'),
    (56, b'8', b'*'),
    (57, b'9', b'('),
    (186, b';', b':'),
    (187, b'=', b'+'),
    (188, b',', b'<'),
    (189, b'-', b'_'),
    (190, b'.', b'>'),
    (191, b'/', b'?'),
    (192, b'`', b'~'),
    (219, b'[', b'{'),
    (220, b'\\', b'|'),
    (221, b']', b'}'),
    (222, b'\'', b'"'),
];

fn alt_char(kc: u32, shift: bool) -> Option<u8> {
    ALT_KEYS
        .iter()
        .find(|(code, _, _)| *code == kc)
        .map(|&(_, plain, shifted)| if shift { shifted } else { plain })
}

/// El único carácter de `s`, si tiene exactamente uno.
fn single_char(s: &str) -> Option<char> {
    let mut it = s.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// `ESC [ A` o `ESC O A` según el modo de cursor de aplicación.
fn cursor_seq(m: &Modes, fin: u8) -> Vec<u8> {
    esc2(if m.app_cursor { b'O' } else { b'[' }, fin)
}

/// Codifica una pulsación como lo haría `Keyboard.evaluateKeyboardEvent`
/// (que decide por `keyCode`; ver [`KeyInput::key_code`]).
pub fn encode_key(k: &KeyInput, m: &Modes) -> KeyAction {
    // Parámetro de modificadores de xterm: shift 1, alt 2, ctrl 4, meta 8.
    let a: u8 = u8::from(k.shift)
        | (u8::from(k.alt) << 1)
        | (u8::from(k.ctrl) << 2)
        | (u8::from(k.meta) << 3);
    let p = a + 1;
    let send = KeyAction::Send;

    match k.key_code {
        // Teclado de iOS: xterm.js lo reconoce con keyCode 0 y por `key`.
        0 => match k.key {
            "UIKeyInputUpArrow" => send(cursor_seq(m, b'A')),
            "UIKeyInputDownArrow" => send(cursor_seq(m, b'B')),
            "UIKeyInputRightArrow" => send(cursor_seq(m, b'C')),
            "UIKeyInputLeftArrow" => send(cursor_seq(m, b'D')),
            _ => encode_other(k),
        },
        8 => {
            let b = if k.ctrl { 0x08 } else { 0x7f };
            send(if k.alt { vec![ESC, b] } else { vec![b] })
        }
        9 => send(if k.shift {
            vec![ESC, b'[', b'Z']
        } else {
            vec![b'\t']
        }),
        13 => send(if k.alt { vec![ESC, b'\r'] } else { vec![b'\r'] }),
        27 => send(if k.alt { vec![ESC, ESC] } else { vec![ESC] }),
        // Flechas: 37 ←, 38 ↑, 39 →, 40 ↓. Con Meta xterm.js no manda nada
        // (Cmd+flecha es del navegador).
        37..=40 => {
            if k.meta {
                return KeyAction::None;
            }
            let fin = match k.key_code {
                37 => b'D',
                38 => b'A',
                39 => b'C',
                _ => b'B',
            };
            if a == 0 {
                return send(cursor_seq(m, fin));
            }
            // En Linux xterm.js convierte Alt+flecha en Ctrl+flecha
            // (ESC[1;5X): el 1;3 solo se conserva en macOS (ESC b / ESC f).
            send(csi_mod_final(if p == 3 { 5 } else { p }, fin))
        }
        // Shift+Insert y Ctrl+Insert son pegar y copiar del navegador.
        45 => {
            if k.shift || k.ctrl {
                KeyAction::None
            } else {
                send(csi_tilde(2))
            }
        }
        46 => send(if a == 0 {
            csi_tilde(3)
        } else {
            csi_tilde_mod(3, p)
        }),
        36 | 35 => {
            let fin = if k.key_code == 36 { b'H' } else { b'F' };
            send(if a != 0 {
                csi_mod_final(p, fin)
            } else {
                cursor_seq(m, fin)
            })
        }
        33 | 34 => {
            let up = k.key_code == 33;
            if k.shift {
                KeyAction::ScrollPage(if up { -1 } else { 1 })
            } else if k.ctrl {
                send(csi_tilde_mod(if up { 5 } else { 6 }, p))
            } else {
                send(csi_tilde(if up { 5 } else { 6 }))
            }
        }
        // F1–F4: 112–115, SS3 P..S.
        112..=115 => {
            let fin = b'P' + (k.key_code - 112) as u8;
            send(if a == 0 {
                esc2(b'O', fin)
            } else {
                csi_mod_final(p, fin)
            })
        }
        // F5–F12: 116–123, números de CSI `~` con el hueco de xterm.
        116..=123 => {
            const F_TILDE: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
            let n = F_TILDE
                .get((k.key_code - 116) as usize)
                .copied()
                .unwrap_or(15);
            send(if a == 0 {
                csi_tilde(n)
            } else {
                csi_tilde_mod(n, p)
            })
        }
        _ => encode_other(k),
    }
}

/// Rama `default` de xterm.js: caracteres, Ctrl+letra y Alt+carácter.
fn encode_other(k: &KeyInput) -> KeyAction {
    let kc = k.key_code;
    let ctrl_only = k.ctrl && !k.shift && !k.alt && !k.meta;

    if ctrl_only {
        let byte = match kc {
            65..=90 => Some((kc - 64) as u8),
            32 => Some(0x00),
            // Ctrl+3..7 → ESC FS GS RS US; Ctrl+8 → DEL.
            51..=55 => Some((kc - 51 + 27) as u8),
            56 => Some(0x7f),
            219 => Some(0x1b),
            220 => Some(0x1c),
            221 => Some(0x1d),
            // Ctrl+/, Ctrl+- y Ctrl+teclado numérico (96–111): nada en xterm.js.
            _ => None,
        };
        return byte.map_or(KeyAction::None, |b| KeyAction::Send(vec![b]));
    }

    if !k.alt || k.meta {
        // Un solo carácter UTF-16 sin Ctrl/Alt/Meta: es texto (xterm.js lo
        // manda por keydown o por keypress; los bytes son los mismos).
        if let Some(c) = single_char(k.key)
            && !k.ctrl
            && !k.alt
            && !k.meta
            && c.len_utf16() == 1
        {
            return KeyAction::Send(k.key.as_bytes().to_vec());
        }
        if k.ctrl {
            match k.key {
                "_" => return KeyAction::Send(vec![0x1f]),
                "@" => return KeyAction::Send(vec![0x00]),
                _ => {}
            }
        }
        return KeyAction::None;
    }

    // Alt sin Meta (en Linux Alt nunca se trata como Meta de Mac). Las teclas
    // del teclado numérico (96–111) no están en la tabla: no mandan nada.
    if let Some(ch) = alt_char(kc, k.shift) {
        return KeyAction::Send(vec![ESC, ch]);
    }
    if (65..=90).contains(&kc) {
        let b = if k.ctrl {
            (kc - 64) as u8
        } else if k.shift {
            kc as u8
        } else {
            (kc + 32) as u8
        };
        return KeyAction::Send(vec![ESC, b]);
    }
    if kc == 32 {
        return KeyAction::Send(vec![ESC, if k.ctrl { 0x00 } else { b' ' }]);
    }
    // Alt+tecla muerta: xterm.js no manda nada (`_unprocessedDeadKey`).
    KeyAction::None
}

/// Texto pegado → bytes: `\r?\n` pasa a `\r`; con `bracketed_paste` va entre
/// `ESC[200~` y `ESC[201~` y se le quita cualquier `ESC[201~` interno.
///
/// **Se aparta de xterm.js 5.5 a propósito**: xterm.js no filtra el cierre y
/// un texto con `ESC[201~` saldría del pegado e inyectaría órdenes. Es una
/// excepción de seguridad; el resto de los bytes coincide con xterm.js.
pub fn encode_paste(text: &str, m: &Modes) -> Vec<u8> {
    const OPEN: &[u8] = b"\x1b[200~";
    const CLOSE: &[u8] = b"\x1b[201~";
    let src = text.as_bytes();
    let mut out = Vec::with_capacity(src.len() + OPEN.len() + CLOSE.len());
    if m.bracketed_paste {
        out.extend_from_slice(OPEN);
    }
    let base = out.len();
    let mut i = 0;
    while i < src.len() {
        match (src.get(i), src.get(i + 1)) {
            (Some(b'\r'), Some(b'\n')) => {
                out.push(b'\r');
                i += 2;
            }
            (Some(b'\n'), _) => {
                out.push(b'\r');
                i += 1;
            }
            (Some(&b), _) => {
                out.push(b);
                i += 1;
            }
            (None, _) => break,
        }
    }
    if m.bracketed_paste {
        // Quitar un cierre puede juntar los bytes de otro (`ESC[20` + `ESC[201~`
        // + `1~`): se repite hasta que no quede ninguno.
        let mut from = base;
        while let Some(p) = out
            .get(from..)
            .and_then(|t| t.windows(CLOSE.len()).position(|w| w == CLOSE))
        {
            let at = from + p;
            out.drain(at..at + CLOSE.len());
            from = at.saturating_sub(CLOSE.len() - 1).max(base);
        }
        out.extend_from_slice(CLOSE);
    }
    out
}

/// Botón del ratón.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    /// Sin botón (movimiento a secas).
    None,
}

/// Qué pasó con el botón.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    Press,
    Release,
    Move,
}

/// Informe de ratón para la aplicación, o `None` si el modo no lo pide.
///
/// `col` y `row` son celdas a partir de 0; `mods` es `(ctrl, alt, shift)`.
/// Con `sgr_mouse` sale `ESC[<Cb;Cx;CyM|m`; si no, el formato clásico
/// `ESC[M` con tres bytes crudos, que no admite celdas por encima de 222.
pub fn encode_mouse(
    b: Button,
    kind: MouseKind,
    col: u16,
    row: u16,
    mods: (bool, bool, bool),
    m: &Modes,
) -> Option<Vec<u8>> {
    // Valores internos de xterm.js: botón 0..2, 3 = ninguno, 4 = rueda.
    let (button, wheel_down) = match b {
        Button::Left => (0u8, false),
        Button::Middle => (1, false),
        Button::Right => (2, false),
        Button::None => (3, false),
        Button::WheelUp => (4, false),
        Button::WheelDown => (4, true),
    };
    let wheel = button == 4;
    // `action` de xterm.js: 0 suelta/rueda arriba, 1 pulsa/rueda abajo, 32 mueve.
    let action: u8 = match kind {
        MouseKind::Press if wheel => u8::from(wheel_down),
        MouseKind::Press => 1,
        MouseKind::Release => 0,
        MouseKind::Move => 32,
    };
    if wheel && kind != MouseKind::Press {
        return None;
    }
    if button == 3 && kind != MouseKind::Move {
        return None;
    }
    // Qué eventos admite cada nivel de seguimiento.
    let allowed = match m.mouse {
        MouseMode::Off => false,
        MouseMode::Click => kind != MouseKind::Move,
        MouseMode::Drag => kind != MouseKind::Move || button != 3,
        MouseMode::Motion => true,
    };
    if !allowed {
        return None;
    }

    let (ctrl, alt, shift) = mods;
    let mut cb: u32 = (u32::from(ctrl) << 4) | (u32::from(shift) << 2) | (u32::from(alt) << 3);
    if wheel {
        cb |= 64 | u32::from(action);
    } else {
        cb |= u32::from(button & 3);
        if kind == MouseKind::Move {
            cb |= 32;
        } else if kind == MouseKind::Release && !m.sgr_mouse {
            cb |= 3;
        }
    }
    let x = u32::from(col) + 1;
    let y = u32::from(row) + 1;

    if m.sgr_mouse {
        let fin = if kind == MouseKind::Release && !wheel {
            b'm'
        } else {
            b'M'
        };
        let mut v = Vec::with_capacity(16);
        v.extend_from_slice(&[ESC, b'[', b'<']);
        push_u32(&mut v, cb);
        v.push(b';');
        push_u32(&mut v, x);
        v.push(b';');
        push_u32(&mut v, y);
        v.push(fin);
        Some(v)
    } else {
        let (cb, x, y) = (cb + 32, x + 32, y + 32);
        if cb > 255 || x > 255 || y > 255 {
            return None;
        }
        Some(vec![ESC, b'[', b'M', cb as u8, x as u8, y as u8])
    }
}

/// Decimal sin asignar.
fn push_u32(out: &mut Vec<u8>, mut n: u32) {
    let mut buf = [0u8; 10];
    let mut len = 0;
    loop {
        if let Some(slot) = buf.get_mut(len) {
            *slot = b'0' + (n % 10) as u8;
        }
        len += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for d in buf.iter().take(len).rev() {
        out.push(*d);
    }
}

/// Qué hacer con un paso de rueda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WheelAction {
    /// Informe de ratón para tmux u otra aplicación.
    Mouse(Vec<u8>),
    /// Flechas repetidas (pantalla alternativa sin ratón).
    Arrows(Vec<u8>),
    /// Desplazar la vista local esas líneas (negativo = hacia arriba).
    Scroll(i32),
}

/// Tope de flechas por paso de rueda en pantalla alternativa. xterm.js no lo
/// tiene; acota la reserva ante un `lines` desmesurado (delta de página o
/// evento sintético). 200 supera con holgura cualquier pantalla real.
pub const MAX_WHEEL_ARROWS: usize = 200;

/// Rueda: `lines < 0` es hacia arriba; `mods` es `(ctrl, alt, shift)`.
///
/// Con ratón activo sale **un** informe por llamada con los modificadores
/// (como xterm.js: el sentido manda, no la cantidad; tmux ya desplaza varias
/// líneas por cada rueda). Sin ratón y en pantalla alternativa (sin historia,
/// la regla `!hasScrollback` de xterm.js) salen `|lines|` flechas, hasta
/// [`MAX_WHEEL_ARROWS`]; si no, desplazamiento local.
pub fn wheel(lines: i32, col: u16, row: u16, mods: (bool, bool, bool), m: &Modes) -> WheelAction {
    if lines == 0 {
        return WheelAction::Scroll(0);
    }
    let up = lines < 0;
    if m.mouse != MouseMode::Off {
        let b = if up {
            Button::WheelUp
        } else {
            Button::WheelDown
        };
        // Fuera de rango en X10 no hay informe posible: no se hace nada.
        return encode_mouse(b, MouseKind::Press, col, row, mods, m)
            .map_or(WheelAction::Scroll(0), WheelAction::Mouse);
    }
    if m.alt_screen {
        let fin = if up { b'A' } else { b'B' };
        let n = (lines.unsigned_abs() as usize).min(MAX_WHEEL_ARROWS);
        let one = [ESC, if m.app_cursor { b'O' } else { b'[' }, fin];
        let mut v = Vec::with_capacity(n * one.len());
        for _ in 0..n {
            v.extend_from_slice(&one);
        }
        return WheelAction::Arrows(v);
    }
    WheelAction::Scroll(lines)
}

/// Informe de foco (`CSI I` / `CSI O`) si la aplicación lo pidió.
pub fn focus(on: bool, m: &Modes) -> Option<&'static [u8]> {
    if !m.focus_events {
        return None;
    }
    Some(if on { b"\x1b[I" } else { b"\x1b[O" })
}

/// Eventos `beforeinput` de teclados en pantalla (GBoard no manda `keydown`).
pub fn beforeinput(input_type: &str, data: Option<&str>) -> Option<Vec<u8>> {
    match input_type {
        "deleteContentBackward" => Some(vec![0x7f]),
        "insertLineBreak" | "insertParagraph" => Some(vec![b'\r']),
        "insertText" => data
            .filter(|d| !d.is_empty())
            .map(|d| d.as_bytes().to_vec()),
        _ => None,
    }
}

/// Estado puro del IME: entrega el texto compuesto una sola vez.
#[derive(Debug, Clone, Default)]
pub struct Ime {
    composing: bool,
    last_commit: Option<String>,
}

impl Ime {
    /// `compositionstart`.
    pub fn start(&mut self) {
        self.composing = true;
        self.last_commit = None;
    }

    /// `compositionend`: entrega el texto compuesto y recuerda cuál fue, porque
    /// Chrome manda justo después un `input` con el mismo texto.
    pub fn end(&mut self, data: &str) -> Option<Vec<u8>> {
        self.composing = false;
        if data.is_empty() {
            self.last_commit = None;
            return None;
        }
        self.last_commit = Some(data.to_owned());
        Some(data.as_bytes().to_vec())
    }

    /// `input`/`beforeinput`: ignora lo que llega componiendo y el eco del
    /// último texto comprometido; el resto lo resuelve [`beforeinput`].
    pub fn input(
        &mut self,
        input_type: &str,
        data: Option<&str>,
        is_composing: bool,
    ) -> Option<Vec<u8>> {
        if is_composing || self.composing {
            return None;
        }
        let echo = input_type == "insertText"
            && matches!((&self.last_commit, data), (Some(c), Some(d)) if c == d);
        // El eco solo vale una vez y solo inmediatamente después del commit.
        self.last_commit = None;
        if echo {
            return None;
        }
        beforeinput(input_type, data)
    }

    /// Anula el eco pendiente; la capa web lo llama al enviar una tecla por
    /// `keydown` para no tragarse un texto idéntico escrito después.
    pub fn forget_commit(&mut self) {
        self.last_commit = None;
    }

    /// `keydown` que no debe codificarse: el IME lo tiene (keyCode 229) o hay
    /// una composición en curso.
    pub fn keydown_ignored(&self, key_code: u32) -> bool {
        key_code == 229 || self.composing
    }
}
