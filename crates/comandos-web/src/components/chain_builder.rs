//! Chain editor. CommandSidebar remains an explicit JavaScript rendering boundary.
#[cfg(not(target_arch = "wasm32"))]
use wasm_bindgen::JsValue;

/// One request per instance; closing/reopening never transfers a pending save.
#[derive(Default)]
pub struct SaveGate {
    generation: u64,
    active: Option<u64>,
    pending: bool,
}
impl SaveGate {
    pub fn open(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.active = Some(self.generation);
    }
    pub fn close(&mut self) {
        self.active = None;
    }
    pub fn begin(&mut self) -> Option<u64> {
        if self.pending {
            return None;
        }
        let ticket = self.active?;
        self.pending = true;
        Some(ticket)
    }
    pub fn finish(&mut self, ticket: u64) -> bool {
        self.pending = false;
        self.active == Some(ticket)
    }
    pub fn pending(&self) -> bool {
        self.pending
    }
}
pub fn js_space(c: char) -> bool {
    matches!(c, '\u{9}'..='\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}
pub fn valid_command(text: &str) -> bool {
    !text.trim_matches(js_space).is_empty() && !text.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
}

#[cfg(target_arch = "wasm32")]
mod web {
    //! Browser interop only: DOM, callbacks, and Promise settlement; editor logic is Rust.
    use super::{SaveGate, js_space, valid_command};
    use crate::components::web_support::{all, query};
    use comandos_web_dom::{bridge::global_set, port::*};
    use js_sys::{Array, Set};
    use std::{cell::RefCell, rc::Rc};
    use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};

    type Builder = Rc<RefCell<Data>>;
    struct Data {
        opts: JsValue,
        host: JsValue,
        doc: JsValue,
        sidebar: JsValue,
        esc: JsValue,
        api: JsValue,
        catalog: JsValue,
        chains: JsValue,
        saved: JsValue,
        toast: JsValue,
        hydrate: JsValue,
        on_close: JsValue,
        state: JsValue,
        backdrop: JsValue,
        slug: JsValue,
        expanded: Set,
        error: JsValue,
        focus: JsValue,
        gate: SaveGate,
        listeners: Vec<(JsValue, &'static str, JsValue)>,
    }
    fn read<T>(b: &Builder, f: impl FnOnce(&Data) -> T) -> Result<T, JsValue> {
        b.try_borrow()
            .map(|v| f(&v))
            .map_err(|_| js_sys::Error::new("chain-builder reentrant state access").into())
    }
    fn write<T>(b: &Builder, f: impl FnOnce(&mut Data) -> T) -> Result<T, JsValue> {
        b.try_borrow_mut()
            .map(|mut v| f(&mut v))
            .map_err(|_| js_sys::Error::new("chain-builder reentrant state access").into())
    }
    fn noop() -> JsValue {
        function(|_| Ok(JsValue::UNDEFINED))
    }
    fn callback(opts: &JsValue, key: &str) -> JsValue {
        let v = get(opts, key);
        if v.is_undefined() { noop() } else { v }
    }
    fn nullish(v: JsValue) -> JsValue {
        if v.is_null() || v.is_undefined() {
            "".into()
        } else {
            v
        }
    }
    fn text(v: &JsValue) -> JsValue {
        utf16_value(&utf16_string(v))
    }
    fn array(v: JsValue) -> Array {
        if Array::is_array(&v) {
            v.unchecked_into()
        } else {
            Array::new()
        }
    }
    fn kind(v: &JsValue) -> &'static str {
        if *v == JsValue::from_str("shell") {
            "shell"
        } else {
            "pane"
        }
    }
    fn state(b: &Builder) -> Result<JsValue, JsValue> {
        read(b, |d| d.state.clone())
    }
    fn steps(b: &Builder) -> Result<Array, JsValue> {
        Ok(array(get(&state(b)?, "steps")))
    }
    fn q(b: &Builder, sel: &str) -> Result<JsValue, JsValue> {
        Ok(query(&read(b, |d| d.backdrop.clone())?, sel))
    }
    fn escape(b: &Builder, v: &JsValue) -> Result<String, JsValue> {
        Ok(utf16_string(&invoke(
            &read(b, |d| d.esc.clone())?,
            std::slice::from_ref(v),
        )?))
    }
    fn html(el: &JsValue, s: &str) -> Result<(), JsValue> {
        set(el, "innerHTML", &utf16_value(s))
    }
    fn focus(el: &JsValue) {
        if get(el, "focus").is_function() {
            let _ = call(el, "focus", &[]);
        }
    }
    fn closest(el: &JsValue, sel: &str) -> JsValue {
        call(el, "closest", &[sel.into()]).unwrap_or(JsValue::NULL)
    }
    fn dataset(el: &JsValue, key: &str) -> JsValue {
        get(&get(el, "dataset"), key)
    }
    fn index(el: &JsValue) -> f64 {
        let s = closest(el, ".slots .step");
        if truthy(&s) {
            number(&dataset(&s, "i"))
        } else {
            -1.0
        }
    }
    fn here(b: &Builder) -> Result<JsValue, JsValue> {
        let f = get(&read(b, |d| d.opts.clone())?, "here");
        let v = if f.is_function() {
            invoke(&f, &[]).unwrap_or(JsValue::NULL)
        } else {
            JsValue::NULL
        };
        Ok(if truthy(&v) { text(&v) } else { "".into() })
    }
    fn clis(b: &Builder) -> Result<Array, JsValue> {
        let v = invoke(&read(b, |d| d.catalog.clone())?, &[]).unwrap_or(JsValue::NULL);
        Ok(array(get(&v, "clis")))
    }
    fn notify(b: &Builder, v: JsValue) -> Result<(), JsValue> {
        invoke(&read(b, |d| d.toast.clone())?, &[v, JsValue::TRUE]).map(|_| ())
    }
    fn hydrate(b: &Builder, el: &JsValue) -> Result<(), JsValue> {
        let _ = invoke(&read(b, |d| d.hydrate.clone())?, std::slice::from_ref(el));
        Ok(())
    }
    fn render_body(b: &Builder) -> Result<(), JsValue> {
        let body = q(b, ".cb-body")?;
        if !truthy(&body) {
            return Ok(());
        }
        let list = clis(b)?;
        let here = here(b)?;
        let (sidebar, expanded) = read(b, |d| (d.sidebar.clone(), d.expanded.clone()))?;
        let mut out = String::new();
        for c in list.iter() {
            let opts = object();
            set(&opts, "mode", &"build".into())?;
            set(&opts, "open", &expanded)?;
            set(&opts, "here", &(get(&c, "id") == here).into())?;
            set(&opts, "q", &get(&state(b)?, "q"))?;
            out.push_str(&utf16_string(&call(&sidebar, "cliHTML", &[c, opts])?));
        }
        if list.length() == 0 {
            out.push_str("<div class=\"cb-empty\">El catálogo de comandos aún no cargó. Cierra y vuelve a abrir.</div>");
        }
        html(&body, &out)?;
        hydrate(b, &body)
    }
    fn render_slots(b: &Builder, restore: Option<(f64, f64)>) -> Result<(), JsValue> {
        let slots = q(b, ".slots")?;
        if !truthy(&slots) {
            return Ok(());
        }
        let steps = steps(b)?;
        let n = steps.length();
        let mut out = String::new();
        for (i, s) in steps.iter().enumerate() {
            let k = escape(b, &get(&s, "kind"))?;
            let t = escape(b, &get(&s, "text"))?;
            let num = i + 1;
            out.push_str(&format!("<div class=\"step\" draggable=\"true\" data-i=\"{i}\" data-kind=\"{k}\"><span class=\"k\"><b>{num}</b> {k}</span><span class=\"grab\" aria-hidden=\"true\">⠿</span><code title=\"{t}\">{t}</code><span class=\"ctl\"><button type=\"button\" data-flat data-move=\"-1\" aria-label=\"Mover el paso {num} antes\" title=\"Mover antes\"{}>←</button><button type=\"button\" data-flat data-move=\"1\" aria-label=\"Mover el paso {num} después\" title=\"Mover después\"{}>→</button><button type=\"button\" data-flat data-del aria-label=\"Quitar el paso {num}\" title=\"Quitar\">✕</button></span></div>", if i == 0 { " disabled" } else { "" }, if num == n as usize { " disabled" } else { "" }));
        }
        for _ in 0..6_u32.saturating_sub(n).max(1) {
            out.push_str("<div class=\"slot empty\">suelta aquí</div>");
        }
        html(&slots, &out)?;
        let count = q(b, ".cb-count")?;
        if truthy(&count) {
            set(
                &count,
                "textContent",
                &if n == 1 {
                    "1 paso".into()
                } else {
                    format!("{n} pasos").into()
                },
            )?;
        }
        if let Some((i, dir)) = restore {
            let btn = q(
                b,
                &format!(".slots .step[data-i=\"{i}\"] [data-move=\"{dir}\"]"),
            )?;
            if truthy(&btn) && !truthy(&call(&btn, "hasAttribute", &["disabled".into()])?) {
                focus(&btn);
            }
        }
        Ok(())
    }
    fn render_message(b: &Builder) -> Result<(), JsValue> {
        let el = q(b, ".cb-msg")?;
        if !truthy(&el) {
            return Ok(());
        }
        let err = read(b, |d| d.error.clone())?;
        html(
            &el,
            &if truthy(&err) {
                format!(
                    "<div class=\"m-error\" role=\"alert\">{}</div>",
                    escape(b, &err)?
                )
            } else {
                String::new()
            },
        )
    }
    fn show(b: &Builder, message: JsValue) -> Result<(), JsValue> {
        write(b, |d| d.error = message)?;
        render_message(b)
    }
    fn clear_error(b: &Builder) -> Result<(), JsValue> {
        if read(b, |d| truthy(&d.error))? {
            show(b, "".into())?;
        }
        Ok(())
    }
    fn busy(b: &Builder, on: bool) -> Result<(), JsValue> {
        for sel in ["[data-save]", "[data-run]"] {
            let el = q(b, sel)?;
            if !truthy(&el) {
                continue;
            }
            if on {
                call(&el, "setAttribute", &["disabled".into(), "".into()])?;
            } else {
                call(&el, "removeAttribute", &["disabled".into()])?;
            }
        }
        Ok(())
    }
    fn add(b: &Builder, k: JsValue, t: JsValue) -> Result<(), JsValue> {
        let t = text(&nullish(t));
        if !valid_command(&utf16_string(&t)) {
            return show(b, "Ese comando no se puede añadir".into());
        }
        let step = object();
        set(&step, "kind", &kind(&k).into())?;
        set(&step, "text", &t)?;
        steps(b)?.push(&step);
        clear_error(b)?;
        render_slots(b, None)?;
        let slots = q(b, ".slots")?;
        if truthy(&slots) {
            set(&slots, "scrollLeft", &get(&slots, "scrollWidth"))?;
        }
        Ok(())
    }
    fn move_step(b: &Builder, from: f64, to: f64, dir: Option<f64>) -> Result<(), JsValue> {
        let steps = steps(b)?;
        let n = f64::from(steps.length());
        if !(from >= 0.0 && from < n && to >= 0.0 && to < n) || from == to {
            return Ok(());
        }
        let removed = call(&steps, "splice", &[from.into(), 1.0.into()])?;
        call(
            &steps,
            "splice",
            &[to.into(), 0.0.into(), get(&removed, "0")],
        )?;
        render_slots(b, dir.map(|d| (to, d)))
    }
    fn remove_step(b: &Builder, i: f64) -> Result<(), JsValue> {
        let steps = steps(b)?;
        if !(i >= 0.0 && i < f64::from(steps.length())) {
            return Ok(());
        }
        call(&steps, "splice", &[i.into(), 1.0.into()])?;
        clear_error(b)?;
        render_slots(b, None)
    }
    fn toggle(b: &Builder, key: JsValue, restore: bool) -> Result<(), JsValue> {
        let open = read(b, |d| d.expanded.clone())?;
        if open.has(&key) {
            open.delete(&key);
        } else {
            open.add(&key);
        }
        render_body(b)?;
        if restore {
            let el = read(b, |d| d.backdrop.clone())?;
            if let Some(h) = all(&el, "[data-toggle]")
                .into_iter()
                .find(|h| dataset(h, "toggle") == key)
            {
                focus(&h);
            }
        }
        Ok(())
    }
    fn failure(err: &JsValue, fallback: &str) -> JsValue {
        let m = get(err, "message");
        if truthy(&m) { m } else { fallback.into() }
    }
    fn save(b: &Builder, run: bool) -> Result<(), JsValue> {
        if read(b, |d| d.gate.pending() || !truthy(&d.backdrop))? {
            return Ok(());
        }
        let name = utf16_string(&get(&state(b)?, "name"));
        let name = name.trim_matches(js_space);
        if name.is_empty() {
            show(b, "Ponle un nombre a la cadena".into())?;
            focus(&q(b, ".m-name")?);
            return Ok(());
        }
        let list = steps(b)?;
        if list.length() == 0 {
            return show(b, "Añade al menos un paso".into());
        }
        let payload = object();
        set(&payload, "name", &utf16_value(name))?;
        let mapped = Array::new();
        for s in list.iter() {
            let step = object();
            set(&step, "kind", &get(&s, "kind"))?;
            set(&step, "text", &get(&s, "text"))?;
            mapped.push(&step);
        }
        set(&payload, "steps", &mapped)?;
        let slug = read(b, |d| d.slug.clone())?;
        if truthy(&slug) {
            set(&payload, "slug", &slug)?;
        }
        clear_error(b)?;
        let Some(ticket) = write(b, |d| d.gate.begin())? else {
            return Ok(());
        };
        busy(b, true)?;
        // Call before spawning: async JavaScript invokes api synchronously before its first await.
        let response = invoke(&read(b, |d| d.api.clone())?, &["/chains".into(), payload]);
        let b = b.clone();
        let _ = promise(async move {
            let res = wait(response).await;
            let same = write(&b, |d| d.gate.finish(ticket))?;
            let chain = res
                .as_ref()
                .map(|r| get(r, "chain"))
                .unwrap_or(JsValue::NULL);
            let err = match res {
                Err(e) => failure(&e, "No se pudo guardar la cadena"),
                Ok(_) if !truthy(&get(&chain, "slug")) => "Respuesta inválida del servidor".into(),
                _ => "".into(),
            };
            if truthy(&err) {
                busy(&b, false)?;
                if same {
                    show(&b, err)?;
                } else {
                    notify(&b, err)?;
                }
                return Ok(JsValue::NULL);
            }
            if same {
                close(&b)?;
            }
            busy(&b, false)?;
            let opts = object();
            set(&opts, "run", &(same && run).into())?;
            if let Err(e) = wait(invoke(
                &read(&b, |d| d.saved.clone())?,
                &[chain.clone(), opts],
            ))
            .await
            {
                notify(&b, failure(&e, "No se pudo refrescar las cadenas"))?;
            }
            Ok(chain)
        });
        Ok(())
    }
    fn close(b: &Builder) -> Result<bool, JsValue> {
        let el = read(b, |d| d.backdrop.clone())?;
        if !truthy(&el) {
            return Ok(false);
        }
        let (listeners, back) = write(b, |d| {
            d.backdrop = JsValue::NULL;
            d.gate.close();
            let back = d.focus.clone();
            d.focus = JsValue::NULL;
            (std::mem::take(&mut d.listeners), back)
        })?;
        for (target, event, f) in listeners {
            call(&target, "removeEventListener", &[event.into(), f])?;
        }
        set(&state(b)?, "dragging", &JsValue::NULL)?;
        if get(&el, "remove").is_function() {
            call(&el, "remove", &[])?;
        } else {
            let p = get(&el, "parentNode");
            if truthy(&p) {
                call(&p, "removeChild", &[el])?;
            }
        }
        focus(&back);
        Ok(true)
    }
    fn user_close(b: &Builder) -> Result<(), JsValue> {
        if close(b)? {
            invoke(&read(b, |d| d.on_close.clone())?, &[])?;
        }
        Ok(())
    }
    fn modal(title: &str, target: &str, name: &str) -> String {
        let target = if target.is_empty() {
            String::new()
        } else {
            format!("<span>escribe en</span><span class=\"pill primary\">{target}</span>")
        };
        format!(
            "<div class=\"modal chain-only\" role=\"dialog\" aria-modal=\"true\" aria-labelledby=\"cb-title\"><div class=\"m-head\"><span class=\"hic\" data-icon=\"snippet\" data-size=\"18\"></span><h2 id=\"cb-title\">{title}</h2><div class=\"search\"><span class=\"hic\" data-icon=\"search\" data-size=\"14\"></span><input class=\"m-q\" type=\"search\" placeholder=\"Buscar…\" aria-label=\"Buscar comando\"></div><div class=\"target\">{target}<button type=\"button\" data-flat class=\"ghost x\" data-close aria-label=\"Cerrar\">✕</button></div></div><div class=\"cb-body cli-board\"></div><div class=\"cb-msg\"></div><div class=\"hotbar\"><span class=\"hb-t\"><span data-icon=\"layers\" data-size=\"16\"></span>Cadena</span><div class=\"slots\" aria-label=\"Pasos de la cadena\"></div><input class=\"m-name\" type=\"text\" maxlength=\"80\" autocomplete=\"off\" placeholder=\"Nombre\" aria-label=\"Nombre de la cadena\" value=\"{name}\"><button type=\"button\" data-flat class=\"primary\" data-save>Guardar</button><button type=\"button\" data-flat data-run title=\"Guarda la cadena y la corre en el pane de destino\"><span data-icon=\"play\" data-size=\"13\"></span>Correr</button></div><div class=\"m-foot\"><span>Aquí un clic <b>añade a la cadena</b>; los pasos se reordenan arrastrando; Correr los inserta uno a uno con Siguiente.</span><span class=\"cb-count\"></span><span class=\"right\"><button type=\"button\" data-flat class=\"ghost\" data-close>Cerrar</button></span></div></div>"
        )
    }
    fn open(b: &Builder, slug: JsValue) -> Result<JsValue, JsValue> {
        let mut chain = JsValue::NULL;
        if slug.is_string() && truthy(&slug) {
            let list = array(invoke(&read(b, |d| d.chains.clone())?, &[]).unwrap_or(JsValue::NULL));
            chain = list
                .iter()
                .find(|c| get(c, "slug") == slug)
                .unwrap_or(JsValue::NULL);
            if !truthy(&chain)
                || truthy(&get(&chain, "error"))
                || !Array::is_array(&get(&chain, "steps"))
            {
                notify(b, "Esa cadena no se puede editar".into())?;
                return Ok(JsValue::NULL);
            }
        }
        if read(b, |d| truthy(&d.backdrop))? {
            let keep = read(b, |d| d.focus.clone())?;
            close(b)?;
            write(b, |d| d.focus = keep)?;
        } else {
            let active = get(&read(b, |d| d.doc.clone())?, "activeElement");
            write(b, |d| {
                d.focus = if truthy(&active) {
                    active
                } else {
                    JsValue::NULL
                }
            })?;
        }
        let editing = truthy(&chain);
        let name = if editing {
            let n = get(&chain, "name");
            text(&if truthy(&n) { n } else { get(&chain, "slug") })
        } else {
            "".into()
        };
        let copied = Array::new();
        for s in array(get(&chain, "steps")).iter() {
            let step = object();
            set(&step, "kind", &kind(&get(&s, "kind")).into())?;
            set(&step, "text", &text(&get(&s, "text")))?;
            copied.push(&step);
        }
        let state = state(b)?;
        set(&state, "name", &name)?;
        set(&state, "steps", &copied)?;
        set(&state, "dragging", &JsValue::NULL)?;
        write(b, |d| {
            d.slug = if editing {
                get(&chain, "slug")
            } else {
                JsValue::NULL
            };
            d.expanded = Set::new(&JsValue::UNDEFINED);
            d.error = "".into();
        })?;
        let here = here(b)?;
        if truthy(&here) && clis(b)?.iter().any(|c| get(&c, "id") == here) {
            read(b, |d| d.expanded.clone())?.add(&here);
        }
        let el = call(
            &read(b, |d| d.doc.clone())?,
            "createElement",
            &["div".into()],
        )?;
        set(&el, "className", &"backdrop".into())?;
        call(&el, "setAttribute", &["data-mclose".into(), "".into()])?;
        write(b, |d| {
            d.backdrop = el.clone();
            d.gate.open();
        })?;
        let target = get(&read(b, |d| d.opts.clone())?, "target");
        let target = if target.is_function() {
            invoke(&target, &[])?
        } else {
            "".into()
        };
        let target = if truthy(&target) {
            escape(b, &target)?
        } else {
            String::new()
        };
        html(
            &el,
            &modal(
                if editing { "Editar cadena" } else { "Comandos" },
                &target,
                &escape(b, &name)?,
            ),
        )?;
        wire(b, &el)?;
        render_body(b)?;
        render_slots(b, None)?;
        render_message(b)?;
        hydrate(b, &el)?;
        if read(b, |d| d.gate.pending())? {
            busy(b, true)?;
        }
        call(&read(b, |d| d.host.clone())?, "appendChild", &[el])?;
        let doc = read(b, |d| d.doc.clone())?;
        listener(b, &doc, "keydown", |b, e| {
            if get(&e, "key") == JsValue::from_str("Escape") {
                user_close(b)?;
            }
            Ok(())
        })?;
        focus(&q(b, ".m-name")?);
        Ok(state)
    }
    fn listener(
        b: &Builder,
        el: &JsValue,
        event: &'static str,
        f: impl Fn(&Builder, JsValue) -> Result<(), JsValue> + 'static,
    ) -> Result<(), JsValue> {
        let owner = b.clone();
        let fun = function(move |args| {
            f(&owner, args.get(0))?;
            Ok(JsValue::UNDEFINED)
        });
        call(el, "addEventListener", &[event.into(), fun.clone()])?;
        write(b, |d| d.listeners.push((el.clone(), event, fun)))?;
        Ok(())
    }
    fn clear_marks(el: &JsValue, names: &[&str]) {
        for name in names {
            for s in all(el, &format!(".slots .step.{name}")) {
                let _ = call(&get(&s, "classList"), "remove", &[(*name).into()]);
            }
        }
    }
    fn wire(b: &Builder, el: &JsValue) -> Result<(), JsValue> {
        let root = el.clone();
        listener(b, el, "click", move |b, e| {
            let t = get(&e, "target");
            if t == root {
                return user_close(b);
            }
            if !get(&t, "closest").is_function() {
                return Ok(());
            }
            if truthy(&closest(&t, "[data-close]")) {
                return user_close(b);
            }
            if truthy(&closest(&t, "[data-save]")) {
                return save(b, false);
            }
            if truthy(&closest(&t, "[data-run]")) {
                return save(b, true);
            }
            let n = closest(&t, "[data-move]");
            if truthy(&n) {
                if truthy(&call(&n, "hasAttribute", &["disabled".into()])?) {
                    return Ok(());
                }
                let i = index(&n);
                let dir = if number(&dataset(&n, "move")) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                return move_step(b, i, i + dir, Some(dir));
            }
            if truthy(&closest(&t, "[data-del]")) {
                return remove_step(b, index(&t));
            }
            let n = closest(&t, "[data-add]");
            if truthy(&n) {
                return add(b, dataset(&n, "kind"), dataset(&n, "add"));
            }
            let n = closest(&t, "[data-toggle]");
            if truthy(&n) {
                return toggle(b, dataset(&n, "toggle"), false);
            }
            Ok(())
        })?;
        listener(b, el, "keydown", |b, e| {
            let t = get(&e, "target");
            let n = closest(&t, "[data-toggle]");
            if !truthy(&n)
                || n != t
                || !truthy(&call(
                    &read(b, |d| d.sidebar.clone())?,
                    "isToggleKey",
                    std::slice::from_ref(&e),
                )?)
            {
                return Ok(());
            }
            if get(&e, "preventDefault").is_function() {
                call(&e, "preventDefault", &[])?;
            }
            toggle(b, dataset(&n, "toggle"), true)
        })?;
        listener(b, el, "input", |b, e| {
            let t = get(&e, "target");
            let classes = get(&t, "classList");
            if !truthy(&classes) {
                return Ok(());
            }
            if truthy(&call(&classes, "contains", &["m-name".into()])?) {
                set(&state(b)?, "name", &text(&nullish(get(&t, "value"))))?;
                clear_error(b)?;
            }
            if truthy(&call(&classes, "contains", &["m-q".into()])?) {
                set(&state(b)?, "q", &text(&nullish(get(&t, "value"))))?;
                render_body(b)?;
            }
            Ok(())
        })?;
        listener(b, el, "dragstart", |b, e| {
            let s = closest(&get(&e, "target"), ".slots .step");
            if !truthy(&s) {
                return Ok(());
            }
            let i = index(&s);
            set(&state(b)?, "dragging", &i.into())?;
            let dt = get(&e, "dataTransfer");
            if truthy(&dt) {
                let _ = (|| -> Result<(), JsValue> {
                    set(&dt, "effectAllowed", &"move".into())?;
                    call(
                        &dt,
                        "setData",
                        &["application/x-comandos-step".into(), i.to_string().into()],
                    )?;
                    call(&dt, "setData", &["text/plain".into(), i.to_string().into()])?;
                    Ok(())
                })();
            }
            call(&get(&s, "classList"), "add", &["dragging".into()])?;
            Ok(())
        })?;
        let root = el.clone();
        listener(b, el, "dragover", move |b, e| {
            if get(&state(b)?, "dragging").is_null() {
                return Ok(());
            }
            let t = get(&e, "target");
            if !truthy(&closest(&t, ".slots")) {
                return Ok(());
            }
            call(&e, "preventDefault", &[])?;
            let dt = get(&e, "dataTransfer");
            if truthy(&dt) {
                let _ = set(&dt, "dropEffect", &"move".into());
            }
            clear_marks(&root, &["over"]);
            let s = closest(&t, ".slots .step");
            if truthy(&s) {
                call(&get(&s, "classList"), "add", &["over".into()])?;
            }
            Ok(())
        })?;
        listener(b, el, "drop", |b, e| {
            let t = get(&e, "target");
            let from = get(&state(b)?, "dragging");
            if !truthy(&closest(&t, ".slots")) || from.is_null() {
                return Ok(());
            }
            call(&e, "preventDefault", &[])?;
            set(&state(b)?, "dragging", &JsValue::NULL)?;
            let i = index(&t);
            move_step(
                b,
                number(&from),
                if i >= 0.0 {
                    i
                } else {
                    f64::from(steps(b)?.length()) - 1.0
                },
                None,
            )
        })?;
        let root = el.clone();
        listener(b, el, "dragend", move |b, _| {
            set(&state(b)?, "dragging", &JsValue::NULL)?;
            clear_marks(&root, &["over", "dragging"]);
            Ok(())
        })
    }
    /// CommonJS-equivalent callable export; mount publishes the browser global.
    #[wasm_bindgen(js_name = createChainBuilder)]
    pub fn create_chain_builder(opts: JsValue) -> Result<JsValue, JsValue> {
        let global = js_sys::global();
        let mut sidebar = get(&global, "ComandosCommandSidebar");
        if !truthy(&sidebar) {
            sidebar = get(&get(&global, "window"), "ComandosCommandSidebar");
        }
        if !truthy(&sidebar) {
            return Err(js_sys::Error::new(
                "chain-builder necesita command-sidebar.js cargado antes",
            )
            .into());
        }
        let host = get(&opts, "root");
        let doc = get(&opts, "doc");
        let doc = if truthy(&doc) {
            doc
        } else {
            let d = get(&host, "ownerDocument");
            if truthy(&d) {
                d
            } else {
                get(&global, "document")
            }
        };
        let state = object();
        set(&state, "name", &"".into())?;
        set(&state, "steps", &Array::new())?;
        set(&state, "dragging", &JsValue::NULL)?;
        set(&state, "q", &"".into())?;
        let on_close = get(&opts, "onClose");
        let b = Rc::new(RefCell::new(Data {
            host,
            doc,
            esc: get(&sidebar, "esc"),
            sidebar,
            api: get(&opts, "api"),
            catalog: callback(&opts, "catalog"),
            chains: callback(&opts, "chains"),
            saved: callback(&opts, "onSaved"),
            toast: callback(&opts, "toast"),
            hydrate: callback(&opts, "hydrate"),
            on_close: if on_close.is_function() {
                on_close
            } else {
                noop()
            },
            opts,
            state: state.clone(),
            backdrop: JsValue::NULL,
            slug: JsValue::NULL,
            expanded: Set::new(&JsValue::UNDEFINED),
            error: "".into(),
            focus: JsValue::NULL,
            gate: SaveGate::default(),
            listeners: Vec::new(),
        }));
        let out = object();
        let owner = b.clone();
        method(&out, "open", move |args| open(&owner, args.get(0)))?;
        method(&out, "close", move |_| close(&b).map(JsValue::from))?;
        let desc = object();
        set(&desc, "get", &function(move |_| Ok(state.clone())))?;
        set(&desc, "enumerable", &JsValue::TRUE)?;
        set(&desc, "configurable", &JsValue::TRUE)?;
        js_sys::Object::define_property(
            &js_sys::Object::from(out.clone()),
            &"state".into(),
            &js_sys::Object::from(desc),
        );
        Ok(out)
    }
    pub fn mount() -> Result<(), JsValue> {
        // Sidebar resolves at instance creation, after its retained script has loaded.
        let module = object();
        method(&module, "createChainBuilder", |args| {
            create_chain_builder(args.get(0))
        })?;
        global_set("ComandosChainBuilder", &module)
    }
}
#[cfg(target_arch = "wasm32")]
pub use web::{create_chain_builder, mount};
#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), JsValue> {
    Ok(())
}
