//! Lazy, bounded sidebar explorer and account credit view. No terminal ownership.
use crate::components::web_support::*;
use comandos_web_dom::port::*;
use comandos_web_view::escape::text as esc;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
use wasm_bindgen::JsValue;

pub struct Browser {
    opts: JsValue,
    el: JsValue,
    active: Cell<bool>,
    generation: Cell<u32>,
    target: RefCell<String>,
    root: RefCell<String>,
    folders: RefCell<BTreeMap<String, Value>>,
    expanded: RefCell<BTreeSet<String>>,
    pending: RefCell<BTreeSet<String>>,
    selected: RefCell<String>,
    credits_at: Cell<f64>,
    credits_busy: Cell<bool>,
}
fn text(v: &JsValue) -> String {
    v.as_string().unwrap_or_default()
}
fn html(el: &JsValue, content: &str) {
    let _ = set(el, "innerHTML", &content.into());
}
fn svg(path: &str) -> String {
    format!(
        r#"<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true">{path}</svg>"#
    )
}
fn file_icon(name: &str, dir: bool) -> String {
    if dir {
        return svg(r#"<path d="M3 6h6l2 2h10v12H3z"/>"#);
    }
    let (label, color) = match name.rsplit('.').next().unwrap_or("") {
        "rs" => ("rs", "#e7aa87"),
        "ts" | "tsx" => ("TS", "#6cb9f4"),
        "js" | "jsx" => ("JS", "#e8ce65"),
        "json" | "toml" | "yaml" | "yml" => ("{}", "#d2b76f"),
        "md" | "txt" => ("≡", "#92b7d4"),
        "py" => ("Py", "#80bbae"),
        "css" | "html" => ("#", "#b998ed"),
        "png" | "jpg" | "svg" | "webp" => ("▧", "#aa9cd9"),
        _ => ("·", "#8d9ab1"),
    };
    format!("<span class=\"sf-type\" style=\"color:{color}\" aria-hidden=\"true\">{label}</span>")
}
impl Browser {
    fn q(&self, selector: &str) -> JsValue {
        query(&self.el, selector)
    }
    fn target_value(&self) -> JsValue {
        invoke(&get(&self.opts, "getTarget"), &[]).unwrap_or(JsValue::NULL)
    }
    async fn api(&self, path: &str, body: Option<Value>) -> Result<Value, JsValue> {
        let mut args = vec![JsValue::from_str(path)];
        if let Some(body) = body {
            args.push(from_json(&body)?);
        }
        let r = wait(invoke(&get(&self.opts, "api"), &args)).await?;
        let v = to_json(&r);
        if let Some(error) = v.get("error").and_then(Value::as_str) {
            return Err(error.into());
        }
        Ok(v)
    }
    pub fn new(opts: JsValue, el: JsValue) -> Rc<Self> {
        let b = Rc::new(Self {
            opts,
            el,
            active: Cell::new(false),
            generation: Cell::new(0),
            target: RefCell::new(String::new()),
            root: RefCell::new(String::new()),
            folders: RefCell::new(BTreeMap::new()),
            expanded: RefCell::new(BTreeSet::new()),
            pending: RefCell::new(BTreeSet::new()),
            selected: RefCell::new(String::new()),
            credits_at: Cell::new(0.),
            credits_busy: Cell::new(false),
        });
        let owner = b.clone();
        listen(
            &b.el,
            "click",
            function(move |args| {
                owner.click(args.get(0));
                Ok(JsValue::UNDEFINED)
            }),
        );
        // One timer for the sidebar, not one per pane. Only the visible view fetches.
        let owner = b.clone();
        let _ = invoke(
            &global("setInterval"),
            &[
                function(move |_| {
                    if owner.active.get()
                        && text(&get(&doc(), "visibilityState")) != "hidden"
                        && number(&get(
                            &call(&owner.q(".cs-explorer"), "getClientRects", &[])
                                .unwrap_or(JsValue::NULL),
                            "length",
                        )) > 0.
                    {
                        owner.sync("files");
                        if !owner.folders.borrow().is_empty() {
                            owner.load_folder(String::new());
                        }
                    }
                    Ok(JsValue::UNDEFINED)
                }),
                5000.into(),
            ],
        );
        b
    }
    pub fn sync(self: &Rc<Self>, view: &str) {
        self.active.set(view == "files");
        if !truthy(&self.q(".cs-explorer")) {
            return;
        }
        if view == "usage" {
            self.load_credits(None);
            return;
        }
        if view != "files" {
            return;
        }
        let t = self.target_value();
        let key = format!("{}|{}", text(&get(&t, "session")), text(&get(&t, "pane")));
        if *self.target.borrow() != key {
            *self.target.borrow_mut() = key;
            self.reset();
        }
        if !truthy(&get(&t, "session")) {
            html(
                &self.q(".cs-explorer"),
                "<p class=\"sf-message\">Selecciona una sesión para explorar sus archivos.</p>",
            );
            return;
        }
        if self.folders.borrow().is_empty() {
            self.load_folder(String::new());
        }
    }
    fn reset(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.root.borrow_mut().clear();
        self.folders.borrow_mut().clear();
        self.expanded.borrow_mut().clear();
        self.pending.borrow_mut().clear();
        self.selected.borrow_mut().clear();
        html(
            &self.q(".cs-explorer"),
            "<p class=\"sf-message\">Cargando archivos…</p>",
        );
    }
    fn load_folder(self: &Rc<Self>, path: String) {
        if self.pending.borrow().contains(&path) {
            return;
        }
        if self.folders.borrow().len() + self.pending.borrow().len() >= 16
            && !self.folders.borrow().contains_key(&path)
        {
            self.notice("Cierra o actualiza el explorador para cargar más carpetas.");
            return;
        }
        self.pending.borrow_mut().insert(path.clone());
        let t = self.target_value();
        let request_target = target_key(&t);
        let request_generation = self.generation.get();
        let body = json!({"session":text(&get(&t,"session")),"pane":text(&get(&t,"pane")),"path":path,"root":if path.is_empty(){String::new()}else{self.root.borrow().clone()}});
        let b = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let r = b.api("/fs/explorer", Some(body)).await;
            if request_generation != b.generation.get() {
                return;
            }
            if request_target != target_key(&b.target_value()) {
                if b.active.get() {
                    b.sync("files");
                }
                return;
            }
            if !b.pending.borrow_mut().remove(&path) {
                return;
            }
            if !path.is_empty() && !b.expanded.borrow().contains(&path) {
                return;
            }
            match r {
                Ok(v) => {
                    let root = v["root"].as_str().unwrap_or("").to_owned();
                    let changed = b.folders.borrow().get(&path) != Some(&v);
                    if !b.root.borrow().is_empty() && *b.root.borrow() != root {
                        b.reset();
                    }
                    *b.root.borrow_mut() = root;
                    b.folders.borrow_mut().insert(path, v);
                    if changed {
                        b.paint();
                    }
                }
                Err(e) => {
                    b.notice(&format!("No se pudo leer la carpeta: {}", error_text(&e)));
                }
            }
        });
    }
    fn notice(&self, message: &str) {
        html(
            &self.q(".cs-explorer"),
            &format!(
                "<p class=\"sf-message\">{}</p><button data-flat data-sf-refresh>Reintentar</button>",
                esc(message)
            ),
        );
    }
    fn rows(&self, path: &str, depth: usize, out: &mut String, ancestors: &mut BTreeSet<String>) {
        let folders = self.folders.borrow();
        let Some(v) = folders.get(path) else {
            return;
        };
        let canonical = v["path"].as_str().unwrap_or(path).to_owned();
        if depth > 16 || !ancestors.insert(canonical.clone()) {
            out.push_str("<p class=\"sf-message\">Enlace a una carpeta ya visible.</p>");
            return;
        }
        if let Some(entries) = v["entries"].as_array() {
            for entry in entries {
                let name = entry["name"].as_str().unwrap_or("");
                let rel = entry["relative"].as_str().unwrap_or("");
                let absolute = entry["path"].as_str().unwrap_or("");
                let dir = entry["directory"].as_bool().unwrap_or(false);
                let open = self.expanded.borrow().contains(rel);
                let action = if dir { "dir" } else { "file" };
                out.push_str(&format!("<button type=\"button\" data-flat class=\"sf-row{}\" data-sf-{action}=\"{}\" data-path=\"{}\" title=\"{}\" style=\"padding-left:{}px\" {}><span class=\"sf-caret\">{}</span>{}<span class=\"sf-name\">{}</span>{}</button>",if *self.selected.borrow()==absolute{" selected"}else{""},esc(rel),esc(absolute),esc(absolute),8+depth*14,if dir{format!("aria-expanded=\"{open}\"")}else{String::new()},if dir{if open{"⌄"}else{"›"}}else{""},file_icon(name,dir),esc(name),if entry["symlink"]==true{"<span aria-label=\"Enlace simbólico\">↗</span>"}else{""}));
                if dir && open {
                    self.rows(rel, depth + 1, out, ancestors);
                }
            }
        }
        ancestors.remove(&canonical);
        if v["truncated"] == true {
            out.push_str(
                "<p class=\"sf-message\">Carpeta grande: se muestran hasta 300 entradas.</p>",
            );
        }
    }
    fn paint(&self) {
        let root = self.root.borrow();
        let title = root
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("/");
        let mut out = format!(
            "<div class=\"sf-heading\"><strong>{}</strong><button data-flat data-sf-refresh title=\"Actualizar archivos\" aria-label=\"Actualizar archivos\">{}</button></div><div class=\"sf-root\" title=\"{}\">{}</div><div class=\"sf-tree\" role=\"group\" aria-label=\"Archivos de la sesión\">",
            esc(title),
            svg(r#"<path d="M20 7v5h-5M4 17v-5h5M6 6a8 8 0 0 1 13 4M18 18a8 8 0 0 1-13-4"/>"#),
            esc(&root),
            esc(&root)
        );
        self.rows("", 0, &mut out, &mut BTreeSet::new());
        out.push_str("</div>");
        let selected = self.selected.borrow();
        let path = if selected.is_empty() {
            &*root
        } else {
            &*selected
        };
        out.push_str(&format!("<div class=\"sf-selection\"><code>{}</code><div><button data-flat data-sf-open=\"{}\">Abrir en equipo</button><button data-flat data-sf-copy=\"{}\">Copiar ruta</button></div><small>Abrir utiliza las aplicaciones del equipo donde corre ComandOS.</small></div>",esc(path),esc(path),esc(path)));
        html(&self.q(".cs-explorer"), &out);
    }
    fn click(self: &Rc<Self>, e: JsValue) {
        let t = get(&e, "target");
        for action in [
            "dir",
            "file",
            "refresh",
            "open",
            "copy",
            "credits-refresh",
            "credits-connect",
        ] {
            let n = call(&t, "closest", &[format!("[data-sf-{action}]").into()])
                .unwrap_or(JsValue::NULL);
            if !truthy(&n) {
                continue;
            }
            stop(&e);
            let value = text(
                &call(&n, "getAttribute", &[format!("data-sf-{action}").into()])
                    .unwrap_or_default(),
            );
            match action {
                "refresh" => {
                    self.reset();
                    self.sync("files");
                }
                "dir" => {
                    if self.expanded.borrow().contains(&value) {
                        self.expanded
                            .borrow_mut()
                            .retain(|p| p != &value && !p.starts_with(&format!("{value}/")));
                        self.folders
                            .borrow_mut()
                            .retain(|p, _| p != &value && !p.starts_with(&format!("{value}/")));
                    } else {
                        self.expanded.borrow_mut().insert(value.clone());
                        if !self.folders.borrow().contains_key(&value) {
                            self.load_folder(value);
                        }
                    }
                    self.paint();
                }
                "file" => {
                    *self.selected.borrow_mut() =
                        text(&call(&n, "getAttribute", &["data-path".into()]).unwrap_or_default());
                    self.paint();
                }
                "copy" => {
                    let b = self.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let result = wait(call(
                            &get(&global("navigator"), "clipboard"),
                            "writeText",
                            &[value.into()],
                        ))
                        .await;
                        let _ = invoke(
                            &get(&b.opts, "toast"),
                            &[if result.is_ok() {
                                "Ruta copiada"
                            } else {
                                "No se pudo copiar; selecciona la ruta"
                            }
                            .into()],
                        );
                    });
                }
                "open" => {
                    let b = self.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        if let Err(e) = b.api("/open-path", Some(json!({"path":value}))).await {
                            let _ = invoke(
                                &get(&b.opts, "toast"),
                                &[error_text(&e).into(), true.into()],
                            );
                        }
                    });
                }
                "credits-refresh" => {
                    self.credits_at.set(0.);
                    self.load_credits(None);
                }
                "credits-connect" => {
                    let input = self.q(".sf-credit-key");
                    let key = text(&get(&input, "value"));
                    let _ = set(&input, "value", &"".into());
                    if !key.trim().is_empty() {
                        self.load_credits(Some(key));
                    }
                }
                _ => {}
            }
            return;
        }
    }
    fn load_credits(self: &Rc<Self>, key: Option<String>) {
        let at = js_sys::Date::now();
        if self.credits_busy.get() || (key.is_none() && at - self.credits_at.get() < 60000.) {
            return;
        }
        self.credits_busy.set(true);
        self.credits_at.set(at);
        let b = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let r = b
                .api("/credits/openrouter", key.map(|key| json!({"key":key})))
                .await;
            b.credits_busy.set(false);
            let content = match r {
                Ok(v) if v["connected"] == true => format!(
                    "<div class=\"sf-credit-numbers\"><div><small>Disponible · USD</small><strong>${:.2}</strong></div><div><small>Comprado</small><b>${:.2}</b></div><div><small>Consumido</small><b>${:.2}</b></div></div>",
                    v["remaining"].as_f64().unwrap_or(0.),
                    v["total_credits"].as_f64().unwrap_or(0.),
                    v["total_usage"].as_f64().unwrap_or(0.)
                ),
                result => {
                    let message = match result {
                        Err(e) => error_text(&e),
                        Ok(_) => "Conecta tu clave de administración para consultar el saldo real."
                            .into(),
                    };
                    format!(
                        "<p>{}</p><details><summary>Conectar OpenRouter</summary><label>Clave de administración<input class=\"sf-credit-key\" type=\"password\" autocomplete=\"off\" placeholder=\"sk-or-…\"></label><button data-flat data-sf-credits-connect>Guardar y conectar</button><small>Se guarda solo en el servidor. No uses aquí una clave de modelos.</small></details>",
                        esc(&message)
                    )
                }
            };
            html(
                &b.q(".cs-credits"),
                &format!(
                    "<div class=\"sf-credit-head\"><strong>OpenRouter</strong><button data-flat data-sf-credits-refresh aria-label=\"Actualizar créditos\">↻</button></div>{content}"
                ),
            );
        });
    }
}
fn target_key(t: &JsValue) -> String {
    format!("{}|{}", text(&get(t, "session")), text(&get(t, "pane")))
}
fn error_text(e: &JsValue) -> String {
    let m = get(e, "message");
    if let Some(m) = m.as_string() {
        m
    } else {
        e.as_string()
            .unwrap_or_else(|| "Vuelve a intentarlo".into())
    }
}
