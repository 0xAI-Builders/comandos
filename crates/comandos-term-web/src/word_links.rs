//! On-demand recovery of labels whose OSC 8 destination was stripped by tmux.
use crate::Inner;
use comandos_term::select::{SelectMode, Selection, selected_text};
use serde_json::json;
use std::{cell::RefCell, rc::Weak};
use wasm_bindgen_futures::spawn_local;

#[derive(Debug)]
pub(crate) struct Request {
    before: String,
    after: String,
    col: u16,
    row: u16,
    x: f64,
    y: f64,
    generation: u64,
}
impl Inner {
    pub(crate) fn word_at(&self, x: f64, y: f64) -> Option<Request> {
        let rect = self.dom.screen.get_bounding_client_rect();
        let cell = self.metrics.css_cell(self.size.cols, self.size.rows)?;
        let (col, row) = crate::mouse::coords_cell(
            x - rect.left(),
            y - rect.top(),
            cell,
            self.size.cols,
            self.size.rows,
        )?;
        let line = i32::from(row) - i32::try_from(self.engine.display_offset()).ok()?;
        let text = |a, b| {
            selected_text(
                &self.engine,
                &Selection {
                    anchor: (line, a),
                    head: (line, b),
                    mode: SelectMode::Simple,
                },
            )
        };
        let cell_text = text(col, col);
        let prefix = text(0, col);
        let before = prefix.strip_suffix(&cell_text)?.to_owned();
        let after = text(col, self.size.cols - 1);
        if !after.chars().next().is_some_and(|c| c.is_alphanumeric())
            || before.len() + after.len() > 8192
        {
            return None;
        }
        Some(Request {
            before,
            after,
            col,
            row,
            x,
            y,
            generation: self.word_generation,
        })
    }
    pub(crate) fn release_word(&mut self, x: f64, y: f64) -> bool {
        let Some(request) = self.word_press.take() else {
            return false;
        };
        if (x - request.x).abs() > 8. || (y - request.y).abs() > 8. {
            return false;
        }
        let same = self
            .word_at(x, y)
            .is_some_and(|now| now.before == request.before && now.after == request.after);
        if !same {
            return false;
        }
        self.outbox.word = Some((self.weak.clone(), request));
        true
    }
}
pub(crate) fn resolve(weak: Weak<RefCell<Inner>>, request: Request) {
    let Some(owner) = weak.upgrade() else { return };
    let window = owner.borrow().window.clone();
    drop(owner);
    let Ok(params) =
        web_sys::UrlSearchParams::new_with_str(&window.location().search().unwrap_or_default())
    else {
        return;
    };
    let session = params.get("arg").unwrap_or_default();
    let token = params.get("auth").unwrap_or_default();
    if session.is_empty() || token.is_empty() {
        return;
    }
    spawn_local(async move {
        let Ok(abort) = web_sys::AbortController::new() else {
            return;
        };
        let signal = abort.signal();
        // Capture above the mobile composer, which consumes keyboard events.
        // These temporary listeners disappear as soon as this lookup settles.
        let listeners: Vec<_> = ["keydown", "beforeinput", "paste", "touchstart", "wheel"]
            .into_iter()
            .map(|kind| {
                let abort = abort.clone();
                comandos_web_dom::events::on_with(
                    window.as_ref(),
                    kind,
                    comandos_web_dom::events::Options {
                        capture: true,
                        passive: true,
                        once: false,
                    },
                    move |_| abort.abort(),
                )
            })
            .collect();
        let timeout = comandos_web_dom::timers::timeout(4000, move || abort.abort());
        let result=crate::pane_chrome::fetch_api(token,"/terminal-link",Some(json!({"session":session,"col":request.col,"row":request.row,"before":request.before,"after":request.after})),Some("No se pudo recuperar el enlace"),Some(signal.clone())).await;
        drop(timeout);
        drop(listeners);
        if signal.aborted() {
            return;
        }
        let Ok(data) = result else { return };
        let Some(url) = data["url"].as_str().filter(|u| crate::links::is_web_url(u)) else {
            return;
        };
        let Some(owner) = weak.upgrade() else { return };
        let Ok(inner) = owner.try_borrow() else {
            return;
        };
        if inner.word_generation != request.generation
            || !inner
                .word_at(request.x, request.y)
                .is_some_and(|now| now.before == request.before && now.after == request.after)
        {
            return;
        }
        let (w, d, t) = (
            inner.window.clone(),
            inner.document.clone(),
            inner.dom.textarea.clone(),
        );
        drop(inner);
        crate::link_menu::show(&w, &d, &t, url, request.x, request.y);
    });
}
