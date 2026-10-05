//! Enlaces bajo el ratón: los que encuentra
//! [`comandos_term::select::find_urls`] (OSC 8, `WebLinksAddon` y las URL
//! envueltas por tmux de `dash/term.html`), su subrayado y su apertura.
//!
//! Como `Linkifier` de xterm.js 5.5.0: un enlace se abre con un **clic
//! simple** (pulsar y soltar sobre el mismo enlace), y esa pulsación sigue
//! llegando a tmux o empezando una selección. El subrayado y el puntero
//! aparecen al pasar por encima; el subrayado usa el color de texto del tema
//! (`_fireUnderlineEvent` no pasa color y addon-canvas usa
//! `colors.foreground`). El enlace se olvida solo cuando se repintan sus
//! filas ([`repainted`]); entonces se vuelve a buscar bajo el ratón.
//!
//! ## Diferencias con `term.html`
//!
//! - Solo se abren `http://` y `https://` (lo mismo que ya filtra
//!   `find_urls`; aquí se vuelve a comprobar antes de abrir).
//! - Solo abre el botón izquierdo: xterm.js activa con cualquier botón (el
//!   central, que en Linux pega, también abriría la pestaña).
use comandos_term::select::{Point, UrlSpan};

/// Solo `http://` y `https://`, sin distinguir mayúsculas.
pub fn is_web_url(url: &str) -> bool {
    let lower = |prefix: &str| {
        url.get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    };
    (lower("http://") || lower("https://")) && !url.chars().any(char::is_control)
}

/// La celda `p` está dentro del enlace (orden de lectura, extremos incluidos).
pub fn span_contains(span: &UrlSpan, p: Point) -> bool {
    span.start <= p && p <= span.end
}

/// `onRenderedViewportChange` de `Linkifier`: el cuadro repintó las filas
/// `lo..=hi` de la vista y el enlace cae dentro (si el repintado empieza en
/// la primera fila, también vale un enlace que empieza más arriba), así que
/// ya no vale.
pub fn repainted(span: &UrlSpan, display_offset: usize, (lo, hi): (usize, usize)) -> bool {
    let view = |row: i32| i64::from(row) + i64::try_from(display_offset).unwrap_or(i64::MAX);
    let lo = i64::try_from(lo).unwrap_or(i64::MAX);
    let hi = i64::try_from(hi).unwrap_or(i64::MAX);
    (lo == 0 || view(span.start.0) >= lo) && view(span.end.0) <= hi
}

/// Enlaces de la última fila consultada; se rehacen cuando cambia la
/// rejilla (cada escritura sube la generación) o la fila.
#[derive(Debug, Clone, Default)]
pub struct LinkCache {
    key: Option<(u64, i32)>,
    spans: Vec<UrlSpan>,
}

impl LinkCache {
    /// Enlace bajo `p`, calculando los de su fila con `find` solo si hace
    /// falta.
    pub fn link_at(
        &mut self,
        generation: u64,
        p: Point,
        find: impl FnOnce(i32) -> Vec<UrlSpan>,
    ) -> Option<&UrlSpan> {
        if self.key != Some((generation, p.0)) {
            self.spans = find(p.0);
            self.key = Some((generation, p.0));
        }
        self.spans.iter().find(|span| span_contains(span, p))
    }
}

/// Tramo de subrayado en celdas de la vista: `(columna, fila, ancho)`.
pub type Underline = (u16, u16, u16);

/// `_handleShowLinkUnderline` de addon-canvas: una raya por fila del enlace
/// que se vea, de la primera celda al final (la última celda incluida).
pub fn underline_rects(
    span: &UrlSpan,
    display_offset: usize,
    rows: u16,
    cols: u16,
) -> Vec<Underline> {
    let view = |row: i32| i64::from(row) + display_offset as i64;
    let (y1, y2) = (view(span.start.0), view(span.end.0));
    let x1 = span.start.1.min(cols);
    let x2 = span.end.1.saturating_add(1).min(cols);
    let mut out = Vec::new();
    let mut push = |x: u16, y: i64, end: u16| {
        if let Ok(y) = u16::try_from(y)
            && y < rows
            && end > x
        {
            out.push((x, y, end - x));
        }
    };
    if y1 == y2 {
        push(x1, y1, x2);
        return out;
    }
    push(x1, y1, cols);
    // Solo las filas visibles: un enlace puede cruzar mucha historia.
    let first = (y1 + 1).max(0);
    let last = (y2 - 1).min(i64::from(rows) - 1);
    let mut y = first;
    while y <= last {
        push(0, y, cols);
        y += 1;
    }
    push(0, y2, x2);
    out
}

/// `openWebLink` de `term.html`: un `<a target="_blank">` sin `opener` ni
/// `referrer`, pulsado y quitado. Solo `http`/`https`.
pub fn open_web_link(document: &web_sys::Document, url: &str) {
    use wasm_bindgen::JsCast;
    if !is_web_url(url) {
        return;
    }
    let Some(body) = document.body() else {
        return;
    };
    let Ok(link) = document.create_element("a").and_then(|e| {
        e.dyn_into::<web_sys::HtmlAnchorElement>()
            .map_err(Into::into)
    }) else {
        return;
    };
    link.set_href(url);
    link.set_target("_blank");
    link.set_rel("noopener noreferrer");
    link.set_referrer_policy("no-referrer");
    if body.append_child(&link).is_ok() {
        link.click();
        link.remove();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: Point, end: Point) -> UrlSpan {
        UrlSpan {
            start,
            end,
            url: "https://a.mx".to_string(),
        }
    }

    #[test]
    fn only_web_urls_open() {
        assert!(is_web_url("https://ejemplo.mx/a"));
        assert!(is_web_url("HTTP://A.MX"));
        assert!(!is_web_url("javascript:alert(1)"));
        assert!(!is_web_url("file:///etc/passwd"));
        assert!(!is_web_url("http:/x"));
        assert!(!is_web_url("https://a.mx/\nb"));
        assert!(!is_web_url(""));
    }

    #[test]
    fn spans_contain_cells_in_reading_order() {
        let s = span((0, 18), (1, 4));
        assert!(span_contains(&s, (0, 18)));
        assert!(span_contains(&s, (0, 19)));
        assert!(span_contains(&s, (1, 0)));
        assert!(span_contains(&s, (1, 4)));
        assert!(!span_contains(&s, (0, 17)));
        assert!(!span_contains(&s, (1, 5)));
    }

    #[test]
    fn a_link_is_dropped_only_when_its_rows_are_repainted() {
        let s = span((2, 4), (3, 1));
        assert!(repainted(&s, 0, (2, 3)));
        assert!(repainted(&s, 0, (0, 23)), "todo");
        assert!(!repainted(&s, 0, (23, 23)), "barra de estado de tmux");
        assert!(!repainted(&s, 0, (3, 5)), "solo una de sus filas");
        assert!(!repainted(&s, 0, (1, 2)));
        // Con la vista desplazada, las filas de la vista son otras.
        assert!(repainted(&span((-5, 0), (-5, 9)), 5, (0, 0)));
        assert!(!repainted(&span((-5, 0), (-5, 9)), 4, (2, 23)));
        // Un enlace que empieza encima de la vista solo cae con la fila 0.
        assert!(repainted(&span((-1, 70), (0, 3)), 0, (0, 1)));
    }

    #[test]
    fn the_cache_asks_once_per_row_and_generation() {
        let mut c = LinkCache::default();
        let mut calls = 0;
        let mut find = |row: i32| {
            calls += 1;
            vec![span((row, 2), (row, 9))]
        };
        assert!(c.link_at(1, (0, 3), &mut find).is_some());
        assert!(c.link_at(1, (0, 12), &mut find).is_none());
        assert!(c.link_at(2, (0, 3), &mut find).is_some());
        assert!(c.link_at(2, (1, 3), &mut find).is_some());
        assert_eq!(calls, 3);
    }

    #[test]
    fn underlines_follow_the_visible_rows() {
        assert_eq!(
            underline_rects(&span((0, 4), (0, 9)), 0, 5, 20),
            vec![(4, 0, 6)]
        );
        assert_eq!(
            underline_rects(&span((0, 15), (2, 3)), 0, 5, 20),
            vec![(15, 0, 5), (0, 1, 20), (0, 2, 4)]
        );
        // En la historia, con la vista desplazada dos filas.
        assert_eq!(
            underline_rects(&span((-2, 0), (-2, 1)), 2, 5, 20),
            vec![(0, 0, 2)]
        );
        // Fuera de la vista.
        assert!(underline_rects(&span((-9, 0), (-8, 3)), 0, 5, 20).is_empty());
        // Un enlace enorme solo recorre las filas visibles.
        assert_eq!(
            underline_rects(&span((-1_000_000, 0), (1_000_000, 3)), 0, 2, 20).len(),
            2
        );
    }
}
