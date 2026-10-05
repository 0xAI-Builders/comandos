//! La pila de popups: tope, a quién cerrar cuando se llena y la clave de un
//! popup por agente (`STACK_MAX`, `evict_candidate`, `by_session` del Python).

/// Ancho de cada popup y márgenes de la pila (`WIDTH`, `MARGIN`, `TOP`).
pub const WIDTH: i32 = 320;
pub const MARGIN: i64 = 14;
pub const TOP: i64 = 48;
/// Tope anti spam de pantalla: entra el nuevo y sale el más viejo.
pub const STACK_MAX: usize = 8;
/// «Cerrar todas» aparece con 2 avisos o más.
pub const CLEAR_ALL_MIN: usize = 2;
/// Segundos hasta el cierre automático de un «listo» (`timeout_add_seconds(10, auto)`).
pub const AUTO_CLOSE_SECS: u32 = 10;

/// Lo que la pila sabe de cada popup (`_kind`, `_session`, `_pane`, `_born`).
#[derive(Clone, Debug, PartialEq)]
pub struct PopupMeta {
    pub kind: String,
    pub session: String,
    pub pane: String,
    /// Segundos desde la época (`time.time()`).
    pub born: f64,
}

/// `evict_candidate(wins)`: con la pila llena, el más viejo de los que NO
/// esperan respuesta; si todos esperan, el más viejo. Índice en `wins`.
pub fn evict_candidate(wins: &[PopupMeta]) -> Option<usize> {
    let calm: Vec<usize> = (0..wins.len())
        .filter(|&i| wins.get(i).is_some_and(|w| w.kind != "waiting"))
        .collect();
    let pool: Vec<usize> = if calm.is_empty() {
        (0..wins.len()).collect()
    } else {
        calm
    };
    // `min` de Python: el primero de los mínimos.
    let mut best: Option<(usize, f64)> = None;
    for i in pool {
        let Some(born) = wins.get(i).map(|w| w.born) else {
            continue;
        };
        if best.is_none_or(|(_, b)| born < b) {
            best = Some((i, born));
        }
    }
    best.map(|(i, _)| i)
}

/// `PANE_RE = ^%\d{1,7}$` con `fullmatch`. Solo dígitos ASCII: `\d` de
/// Python acepta también otros dígitos decimales de Unicode, que tmux nunca
/// usa en un id de pane (diferencia documentada).
pub fn valid_pane(pane: &str) -> bool {
    pane.strip_prefix('%').is_some_and(|digits| {
        (1..=7).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
    })
}

/// Clave de `by_session`: `sesión|pane` si el pane es válido; si no, la sesión.
pub fn popup_key(session: &str, pane: &str) -> String {
    if valid_pane(pane) {
        format!("{session}|{pane}")
    } else {
        session.to_string()
    }
}
