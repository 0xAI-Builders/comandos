//! Chat view over the terminal area: the session's agent transcript as a conversation,
//! a composer that types into its pane, and ‹ › / swipe to move between sessions.
//! One 1 s timer; it fetches only while the view is visible and the page is shown, and
//! the server answers `unchanged` when the transcript did not move.
use comandos_web_view::escape::text as esc;
use serde_json::Value;
#[cfg(target_arch = "wasm32")]
pub use web::install;

#[cfg(target_arch = "wasm32")]
mod web {
use super::{render_messages, render_text};
use crate::components::web_support::*;
use comandos_web_dom::port::*;
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;

const STORE: &str = "comandos.viewMode";
const BUBBLE: &str = r#"<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12Z"/></svg>"#;
const TERM: &str = r#"<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 8 4 4-4 4M12 17h7"/></svg>"#;

#[derive(Default)]
struct Chat {
    on: Cell<bool>,
    session: RefCell<String>,
    token: RefCell<String>,
    busy: Cell<bool>,
    ticks: Cell<u32>,
    swipe: Cell<Option<(f64, f64)>>,
}

fn text(v: &JsValue) -> String {
    v.as_string().unwrap_or_default()
}
fn el() -> JsValue {
    id("chat-view")
}
fn tabs() -> Vec<String> {
    all(&id("tabbar"), ".apptab")
        .iter()
        .map(|t| text(&get(t, "_target")))
        .filter(|t| t.starts_with("term:"))
        .collect()
}
fn step(delta: i32) {
    let list = tabs();
    if list.is_empty() {
        return;
    }
    let current = format!("term:{}", text(&global("activeTerm")));
    let at = list.iter().position(|t| *t == current).unwrap_or(0) as i32;
    let next = (at + delta).rem_euclid(list.len() as i32) as usize;
    let _ = invoke(&global("showView"), &[list[next].as_str().into(), true.into()]);
}

impl Chat {
    fn head(&self) {
        let session = self.session.borrow().clone();
        let row = all(&id("tabbar"), ".apptab")
            .into_iter()
            .find(|t| text(&get(t, "_target")) == format!("term:{session}"));
        let field = |sel: &str| {
            row.as_ref()
                .map(|r| text(&get(&query(r, sel), "textContent")).trim().to_owned())
                .unwrap_or_default()
        };
        let model = field(".mdl");
        let name = Some(field(".lbl")).filter(|n| !n.is_empty()).unwrap_or(session);
        let _ = set(&query(&el(), ".cv-title b"), "textContent", &name.into());
        let _ = set(&query(&el(), ".cv-title span"), "textContent", &model.into());
    }
    fn show(self: &Rc<Self>, on: bool) {
        self.on.set(on);
        let _ = call(&global("localStorage"), "setItem", &[STORE.into(), (if on { "chat" } else { "term" }).into()]);
        let _ = set(&el(), "hidden", &(!on).into());
        classes(&get(&doc(), "body"), "chat-on", on);
        let button = id("tab-chat");
        attr(&button, "aria-pressed", if on { "true" } else { "false" });
        attr(&button, "title", if on { "Ver terminal" } else { "Ver como chat" });
        attr(&button, "aria-label", if on { "Ver terminal" } else { "Ver como chat" });
        let _ = set(&button, "innerHTML", &(if on { TERM } else { BUBBLE }).into());
        if on {
            let active = text(&global("activeTerm"));
            if !active.is_empty() && text(&global("activeView")) != format!("term:{active}") {
                let _ = invoke(&global("showView"), &[format!("term:{active}").into(), true.into()]);
            }
            self.session.replace(String::new());
            self.tick(true);
        }
    }
    fn tick(self: &Rc<Self>, force: bool) {
        if !self.on.get() || text(&get(&doc(), "visibilityState")) == "hidden" {
            return;
        }
        let session = text(&global("activeTerm"));
        if session.is_empty() {
            return;
        }
        let moved = *self.session.borrow() != session;
        if moved {
            self.session.replace(session.clone());
            self.token.replace(String::new());
            let _ = set(&query(&el(), ".cv-feed"), "innerHTML", &"<p class=\"cv-empty\">Cargando conversación…</p>".into());
            self.head();
        }
        self.ticks.set(self.ticks.get().wrapping_add(1));
        if self.busy.get() || !(force || moved || self.ticks.get() % 2 == 0) {
            return;
        }
        self.busy.set(true);
        let me = self.clone();
        spawn_local(async move {
            let since = me.token.borrow().clone();
            let body = from_json(&json!({"session":session,"since":since})).unwrap_or(JsValue::NULL);
            let reply = request("POST", "/chat/transcript", body).await;
            me.busy.set(false);
            if *me.session.borrow() != session {
                return;
            }
            let Ok(reply) = reply else { return };
            let v = to_json(&get(&reply, "body"));
            if v["unchanged"] == true {
                return;
            }
            let feed = query(&el(), ".cv-feed");
            if v["agent"].is_null() {
                let _ = set(&feed, "innerHTML", &"<p class=\"cv-empty\">Esta sesión no tiene un agente con historial (Claude o Codex). Usa la terminal.</p>".into());
                return;
            }
            me.token.replace(v["token"].as_str().unwrap_or("").into());
            let pinned = number(&get(&feed, "scrollHeight"))
                - number(&get(&feed, "scrollTop"))
                - number(&get(&feed, "clientHeight"))
                < 120.;
            let first = since.is_empty();
            let empty = Vec::new();
            let html = render_messages(v["messages"].as_array().unwrap_or(&empty));
            let _ = set(&feed, "innerHTML", &html.into());
            if pinned || first {
                let _ = set(&feed, "scrollTop", &get(&feed, "scrollHeight"));
            }
        });
    }
    fn send(self: &Rc<Self>) {
        let area = query(&el(), ".cv-compose textarea");
        let value = text(&get(&area, "value"));
        let session = text(&global("activeTerm"));
        if value.trim().is_empty() || session.is_empty() {
            return;
        }
        let _ = set(&area, "value", &"".into());
        let feed = query(&el(), ".cv-feed");
        let _ = call(&feed, "insertAdjacentHTML", &["beforeend".into(), format!("<div class=\"cv-me pending\">{}</div>", render_text(&value)).into()]);
        let _ = set(&feed, "scrollTop", &get(&feed, "scrollHeight"));
        let me = self.clone();
        spawn_local(async move {
            let body = from_json(&json!({"session":session,"text":value})).unwrap_or(JsValue::NULL);
            match request("POST", "/send", body).await {
                Ok(r) if number(&get(&r, "status")) < 300. => {}
                _ => toast("No se pudo enviar el mensaje a la sesión"),
            }
            me.tick(true);
        });
    }
    fn key(&self, key: &str) {
        let session = text(&global("activeTerm"));
        if session.is_empty() {
            return;
        }
        let key = key.to_owned();
        spawn_local(async move {
            let body = from_json(&json!({"session":session,"key":key})).unwrap_or(JsValue::NULL);
            if !matches!(request("POST", "/key", body).await, Ok(r) if number(&get(&r, "status")) < 300.) {
                toast("No se pudo enviar la tecla");
            }
        });
    }
}

/// Mounts the view and its toggle once; remote dashboards only.
pub fn install() -> Result<(), JsValue> {
    let area = id("term-area");
    if !truthy(&area) || truthy(&el()) {
        return Ok(());
    }
    call(&area, "insertAdjacentHTML", &["beforeend".into(), r#"<section id="chat-view" hidden aria-label="Sesión como chat"><header class="cv-head"><button type="button" data-cv="prev" aria-label="Sesión anterior">‹</button><div class="cv-title"><b></b><span></span></div><button type="button" data-cv="next" aria-label="Sesión siguiente">›</button></header><div class="cv-feed" aria-live="polite"></div><div class="cv-keys" role="toolbar" aria-label="Teclas rápidas"><button type="button" class="y" data-cv-key="Enter">Sí</button><button type="button" class="n" data-cv-key="Escape">No</button><button type="button" data-cv-key="Escape">Esc</button><button type="button" data-cv-key="Up">↑</button><button type="button" data-cv-key="Down">↓</button><button type="button" data-cv-key="1">1</button><button type="button" data-cv-key="2">2</button><button type="button" data-cv-key="Tab">Tab</button></div><form class="cv-compose"><textarea rows="1" aria-label="Mensaje para la sesión" placeholder="Escribe a la sesión…"></textarea><button type="submit" aria-label="Enviar"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12h14M13 6l6 6-6 6"/></svg></button></form></section>"#.into()])?;
    let open = id("tab-open");
    if truthy(&open) {
        call(&open, "insertAdjacentHTML", &["beforebegin".into(), format!(r#"<button type="button" id="tab-chat" class="tab-nav-btn" aria-pressed="false" aria-label="Ver como chat" title="Ver como chat">{BUBBLE}</button>"#).into()])?;
    }
    let chat = Rc::new(Chat::default());
    let me = chat.clone();
    listen(&id("tab-chat"), "click", function(move |_| {
        if !me.on.get() && !has_term() {
            toast("Abre una sesión primero");
            return Ok(JsValue::UNDEFINED);
        }
        me.show(!me.on.get());
        Ok(JsValue::UNDEFINED)
    }));
    let me = chat.clone();
    listen(&el(), "click", function(move |args| {
        let target = get(&args.get(0), "target");
        let button = call(&target, "closest", &["button".into()]).unwrap_or(JsValue::NULL);
        if !truthy(&button) {
            return Ok(JsValue::UNDEFINED);
        }
        let data = get(&button, "dataset");
        match text(&get(&data, "cv")).as_str() {
            "prev" => step(-1),
            "next" => step(1),
            _ => {}
        }
        let key = text(&get(&data, "cvKey"));
        if !key.is_empty() {
            me.key(&key);
        }
        Ok(JsValue::UNDEFINED)
    }));
    let me = chat.clone();
    listen(&query(&el(), ".cv-compose"), "submit", function(move |args| {
        stop(&args.get(0));
        me.send();
        Ok(JsValue::UNDEFINED)
    }));
    let me = chat.clone();
    listen(&query(&el(), ".cv-compose textarea"), "keydown", function(move |args| {
        let e = args.get(0);
        let coarse = truthy(&call(&js_sys::global(), "matchMedia", &["(pointer:coarse)".into()]).map(|m| get(&m, "matches")).unwrap_or(JsValue::FALSE));
        if text(&get(&e, "key")) == "Enter" && !truthy(&get(&e, "shiftKey")) && !coarse {
            stop(&e);
            me.send();
        }
        Ok(JsValue::UNDEFINED)
    }));
    let feed = query(&el(), ".cv-feed");
    let me = chat.clone();
    listen(&feed, "touchstart", function(move |args| {
        let t = get(&get(&args.get(0), "touches"), "0");
        me.swipe.set(Some((number(&get(&t, "clientX")), number(&get(&t, "clientY")))));
        Ok(JsValue::UNDEFINED)
    }));
    let me = chat.clone();
    listen(&feed, "touchend", function(move |args| {
        let Some((x, y)) = me.swipe.take() else { return Ok(JsValue::UNDEFINED) };
        let t = get(&get(&args.get(0), "changedTouches"), "0");
        let (dx, dy) = (number(&get(&t, "clientX")) - x, number(&get(&t, "clientY")) - y);
        if dx.abs() > 70. && dx.abs() > dy.abs() * 2. {
            step(if dx < 0. { 1 } else { -1 });
        }
        Ok(JsValue::UNDEFINED)
    }));
    let me = chat.clone();
    let _ = invoke(&global("setInterval"), &[function(move |_| {
        me.tick(false);
        Ok(JsValue::UNDEFINED)
    }), 1000.into()]);
    let me = chat.clone();
    listen(&doc(), "visibilitychange", function(move |_| {
        me.tick(true);
        Ok(JsValue::UNDEFINED)
    }));
    let saved = call(&global("localStorage"), "getItem", &[STORE.into()]).ok().map(|v| text(&v));
    // Phones open in chat unless the person chose the terminal before.
    let phone = global("innerWidth").as_f64().is_some_and(|w| w <= 747.);
    if saved.as_deref() == Some("chat") || phone && saved.as_deref().unwrap_or("").is_empty() {
        chat.show(true);
    }
    Ok(())
}
fn has_term() -> bool {
    !text(&global("activeTerm")).is_empty()
}
}

/// Inline markdown of one escaped line: `code`, **bold**.
fn inline(line: &str) -> String {
    let mut out = String::new();
    for (j, seg) in line.split('`').enumerate() {
        if j % 2 == 1 {
            out.push_str(&format!("<code>{seg}</code>"));
            continue;
        }
        for (k, b) in seg.split("**").enumerate() {
            if k % 2 == 1 {
                out.push_str(&format!("<b>{b}</b>"));
            } else {
                out.push_str(b);
            }
        }
    }
    out
}

/// Escaped text with fenced code, headings, bullets, `code`, **bold** and line breaks.
pub fn render_text(raw: &str) -> String {
    let mut out = String::new();
    for (i, part) in raw.split("```").enumerate() {
        if i % 2 == 1 {
            let body = part.split_once('\n').map_or(part, |(_, b)| b);
            out.push_str(&format!("<pre>{}</pre>", esc(body.trim_end())));
            continue;
        }
        let escaped = esc(part);
        for (n, line) in escaped.split('\n').enumerate() {
            let t = line.trim_start();
            if let Some(h) = t.strip_prefix("### ").or(t.strip_prefix("## ")).or(t.strip_prefix("# ")) {
                out.push_str(&format!("<b class=\"cv-h\">{}</b>", inline(h)));
                continue;
            }
            if n > 0 {
                out.push_str("<br>");
            }
            match t.strip_prefix("- ").or(t.strip_prefix("* ")) {
                Some(item) => out.push_str(&format!("<span class=\"cv-li\">{}</span>", inline(item))),
                None => out.push_str(&inline(line)),
            }
        }
    }
    out
}
pub fn render_messages(messages: &[Value]) -> String {
    if messages.is_empty() {
        return "<p class=\"cv-empty\">Sin conversación todavía. Escribe abajo o vuelve a la terminal.</p>".into();
    }
    let mut out = String::new();
    for m in messages {
        let t = m["t"].as_str().unwrap_or("");
        match m["r"].as_str() {
            Some("u") => out.push_str(&format!("<div class=\"cv-me\">{}</div>", render_text(t))),
            Some("a") => out.push_str(&format!("<div class=\"cv-ai\">{}</div>", render_text(t))),
            _ => {
                out.push_str("<div class=\"cv-tool\">");
                for line in t.lines() {
                    out.push_str(&format!("<span>{}</span>", esc(line)));
                }
                out.push_str("</div>");
            }
        }
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn text_escapes_and_formats_light_markdown() {
        assert_eq!(
            render_text("<b>x</b> **ok** `a<b`\nfin"),
            "&lt;b&gt;x&lt;/b&gt; <b>ok</b> <code>a&lt;b</code><br>fin"
        );
        assert_eq!(render_text("ver:\n```rust\nlet a = 1;\n```"), "ver:<br><pre>let a = 1;</pre>");
        assert_eq!(
            render_text("## Plan\n- uno\n- **dos**"),
            "<b class=\"cv-h\">Plan</b><br><span class=\"cv-li\">uno</span><br><span class=\"cv-li\"><b>dos</b></span>"
        );
    }
    #[test]
    fn roles_render_as_bubbles_and_tool_lines() {
        let html = render_messages(&[
            json!({"r":"u","t":"hola"}),
            json!({"r":"t","t":"Bash · ls\nRead · a.rs"}),
            json!({"r":"a","t":"listo"}),
        ]);
        assert_eq!(html, "<div class=\"cv-me\">hola</div><div class=\"cv-tool\"><span>Bash · ls</span><span>Read · a.rs</span></div><div class=\"cv-ai\">listo</div>");
    }
}
