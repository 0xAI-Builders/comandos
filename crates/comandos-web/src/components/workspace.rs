//! Session workspace controls; lexical dashboard owners remain shared through thin bindings.
use comandos_web_view::escape::{attr_esc, md_esc};
use serde_json::{Value, json};

fn at<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or(&Value::Null)
}
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        _ => v.to_string(),
    }
}
fn truth(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        _ => true,
    }
}
fn fallback(v: &Value, keys: &[&str], default: &str) -> String {
    keys.iter()
        .map(|k| at(v, k))
        .find(|v| truth(v))
        .map(text)
        .unwrap_or_else(|| default.into())
}
fn rows(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}
fn put(v: &mut Value, k: &str, x: Value) {
    if let Some(o) = v.as_object_mut() {
        o.insert(k.into(), x);
    }
}
fn remove(v: &mut Value, k: &str) {
    if let Some(o) = v.as_object_mut() {
        o.remove(k);
    }
}

pub fn option(value: &str, label: &str, selected: &str, disabled: bool) -> String {
    format!(
        "<option value=\"{}\" {} {}>{}</option>",
        attr_esc(value),
        if value == selected { "selected" } else { "" },
        if disabled { "disabled" } else { "" },
        md_esc(label)
    )
}
pub fn select(name: &str, label: &str, options: &str) -> String {
    format!("<label class=\"sc-field\">{label}<select name=\"{name}\">{options}</select></label>")
}
pub fn initial_draft(current: &Value) -> Value {
    let agent = fallback(current, &["agent"], "codex");
    json!({"name":"Mi perfil","harness":if agent=="shell"{"codex"}else{&agent},"motor":fallback(current,&["motor","agent"],"codex"),"model":fallback(current,&["model"],""),"effort":fallback(current,&["effort"],""),"harnessAccount":fallback(current,&["harnessAccount","account"],"main"),"motorAccount":fallback(current,&["motorAccount","account"],"main"),"skills":{},"mcps":{}})
}
pub fn change_harness(draft: &mut Value, harness: &str) {
    put(draft, "harness", json!(harness));
    put(draft, "motor", json!(harness));
    remove(draft, "id");
    for k in ["model", "effort"] {
        put(draft, k, json!(""));
    }
    put(draft, "routeId", json!(format!("{harness}:{harness}")));
    for k in ["skills", "mcps"] {
        put(draft, k, json!({}));
    }
}
pub fn change_field(providers: &Value, draft: &Value, name: &str, value: &str) -> Value {
    let mut selection = draft.clone();
    put(&mut selection, "toHarness", at(draft, "harness").clone());
    let mut next = super::session_config::update(providers, &selection, name, value);
    remove(&mut next, "toHarness");
    let route_id = format!(
        "{}:{}",
        text(at(&next, "harness")),
        text(at(&next, "motor"))
    );
    put(&mut next, "routeId", json!(route_id));
    next
}
pub fn saved_draft(draft: &Value) -> Value {
    let mut next = draft.clone();
    remove(&mut next, "templateReport");
    next
}
fn deep_research(s: &str) -> bool {
    s.char_indices().any(|(i, _)| {
        s.get(i..).is_some_and(|t| {
            t.strip_prefix("deep")
                .and_then(|t| t.chars().next().and_then(|c| t.get(c.len_utf8()..)))
                .is_some_and(|t| t.starts_with("research"))
        })
    })
}
pub fn product_template(draft: &mut Value, inventory: &Value) {
    put(draft, "name", json!("Producto completo"));
    let skills = rows(at(inventory, "skills"));
    let mut found = [false; 5];
    for x in skills {
        if at(&x, "toggleable") == &Value::Bool(false) {
            continue;
        }
        let s = format!("{} {}", text(at(&x, "name")), text(at(&x, "path"))).to_lowercase();
        let families = [
            s.contains("superpowers") || s.contains("brainstorm") || s.contains("systematic-debug"),
            s.contains("digitalocean"),
            s.contains("design-research"),
            deep_research(&s),
            s.contains("x402"),
        ];
        for (have, next) in found.iter_mut().zip(families) {
            *have |= next;
        }
        let selected = families.iter().any(|v| *v)
            || s.contains("executing-plans")
            || s.contains("verification-before");
        let key = fallback(&x, &["id", "name"], "");
        let mut chosen = at(draft, "skills").clone();
        if !chosen.is_object() {
            chosen = json!({});
        }
        put(&mut chosen, &key, json!(selected));
        put(draft, "skills", chosen);
    }
    let labels = [
        "Superpowers",
        "DigitalOcean",
        "Diseño",
        "Investigación",
        "x402",
    ];
    let missing = labels
        .iter()
        .zip(found)
        .filter_map(|(label, found)| (!found).then_some(*label))
        .collect::<Vec<_>>();
    put(
        draft,
        "templateReport",
        json!(if missing.is_empty() {
            "Las cinco familias tienen skills seleccionadas. Revisa la lista antes de guardar."
                .into()
        } else {
            format!(
                "Sin selección verificable para: {}. Revisa las skills disponibles.",
                missing.join(", ")
            )
        }),
    );
}
fn number(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::Bool(b) => u8::from(*b) as f64,
        Value::Null => 0.0,
        Value::String(s) => {
            if s.trim().is_empty() {
                0.0
            } else {
                s.trim().parse().unwrap_or(f64::NAN)
            }
        }
        _ => f64::NAN,
    }
}
fn fixed(n: f64, places: usize) -> String {
    // Number.toFixed rounds the exact binary value, with ties away from zero.
    // Multiplying in f64 first loses that distinction (1.15 becomes 1.2).
    // The original renderer uses only zero and one fractional digit.
    if !n.is_finite() || n.abs() >= 1e21 {
        return if n.is_nan() {
            "NaN".into()
        } else if n == f64::INFINITY {
            "Infinity".into()
        } else if n == f64::NEG_INFINITY {
            "-Infinity".into()
        } else {
            n.to_string()
        };
    }
    let bits = n.abs().to_bits();
    let raw_exponent = ((bits >> 52) & 0x7ff) as i32;
    let mantissa =
        u128::from(bits & ((1_u64 << 52) - 1)) | if raw_exponent == 0 { 0 } else { 1_u128 << 52 };
    let exponent = if raw_exponent == 0 {
        -1074
    } else {
        raw_exponent - 1075
    };
    let scale = 10_u128.pow(places as u32);
    let numerator = mantissa * scale;
    let rounded = if exponent >= 0 {
        numerator.checked_shl(exponent as u32).unwrap_or(0)
    } else {
        let shift = (-exponent) as u32;
        if shift >= 128 {
            0
        } else {
            let denominator = 1_u128 << shift;
            (numerator >> shift) + u128::from(numerator & (denominator - 1) >= denominator / 2)
        }
    };
    let sign = if n < 0. { "-" } else { "" };
    if places == 0 {
        format!("{sign}{rounded}")
    } else {
        let whole = rounded / scale;
        let fraction = rounded % scale;
        format!("{sign}{whole}.{fraction:0places$}")
    }
}
fn numeric(v: &Value) -> String {
    let n = number(v);
    if n.is_nan() {
        "NaN".into()
    } else if n == 0.0 {
        "0".into()
    } else {
        n.to_string()
    }
}
pub fn usage_html(result: &Value, item: &Value) -> String {
    let scope = if item.is_null() {
        "Todas las sesiones".into()
    } else {
        format!(
            "{} · {}",
            fallback(item, &["project", "session"], ""),
            fallback(item, &["pane"], "")
        )
    };
    let items = ["items", "extensions", "usage"]
        .iter()
        .map(|k| at(result, k))
        .find(|v| truth(v))
        .map(rows)
        .unwrap_or_default();
    let table = if items.is_empty() {
        "<p>Todavía no hay llamadas observadas para este filtro.</p>".into()
    } else {
        let mut body = String::new();
        for r in items {
            let calls = r
                .get("calls")
                .filter(|v| !v.is_null())
                .or_else(|| r.get("count").filter(|v| !v.is_null()))
                .unwrap_or(&Value::Null);
            let duration = r
                .get("durationMs")
                .filter(|v| !v.is_null())
                .map(|v| format!("{} s", fixed(number(v) / 1000.0, 1)))
                .unwrap_or_else(|| "Sin atribución".into());
            body.push_str(&format!(
                "<tr><td>{}<br><small>{}</small></td><td>{}</td><td>{}</td></tr>",
                md_esc(&fallback(&r, &["name", "tool", "id"], "")),
                md_esc(&fallback(&r, &["kind", "type"], "")),
                numeric(calls),
                duration
            ));
        }
        format!(
            "<table class=\"sc-table\"><thead><tr><th>Skill / herramienta</th><th>Llamadas</th><th>Tiempo medido</th></tr></thead><tbody>{body}</tbody></table>"
        )
    };
    let provenance: String = if at(result, "provenance") == "observed" {
        "Medido en eventos locales".into()
    } else {
        result
            .get("provenance")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into()
    };
    let note = fallback(result, &["note"], &provenance);
    let note = if note.is_empty() {
        "Los tokens de una conversación no se atribuyen automáticamente a cada skill."
    } else {
        &note
    };
    format!(
        "<p class=\"sc-note\">{} · últimos 7 días. Uso observado; habilitar una herramienta no cuenta como usarla.</p>{table}<p class=\"sc-note\">{}<br>{} llamadas a skills sin nombre registrado.<br>No se calculan ahorros de tokens sin una medición comparable.</p>",
        md_esc(&scope),
        md_esc(note),
        numeric(at(result, "unattributedSkillCalls"))
    )
}
fn account_options(items: &Value, value: &str, motor: bool) -> String {
    let mut html = option("", "Selecciona cuenta", value, false);
    for a in rows(items) {
        let alias = text(at(&a, "alias"));
        let identity = fallback(&a, &["identity"], "");
        let label = if identity.is_empty() {
            alias.clone()
        } else {
            format!("{alias} · {identity}")
        };
        html.push_str(&option(
            &alias,
            &label,
            value,
            !truth(at(
                &a,
                if motor {
                    "motorSelectable"
                } else {
                    "selectable"
                },
            )),
        ));
    }
    html
}
fn inventory_group(kind: &str, label: &str, draft: &Value, data: &Value) -> String {
    let reason = fallback(
        at(at(data, "capabilities"), kind),
        &["reason"],
        "Se aplican al iniciar una sesión nueva.",
    );
    let mut items = String::new();
    for x in rows(at(at(data, "inventory"), kind)) {
        let key = fallback(&x, &["id", "name"], "");
        let enabled = at(draft, kind)
            .get(&key)
            .map(truth)
            .unwrap_or_else(|| at(&x, "enabled") != &Value::Bool(false));
        let description = if kind == "mcps" {
            format!(
                "<small class=\"mcp-description\">{}</small>",
                md_esc(&fallback(
                    &x,
                    &["description"],
                    "Este servidor no tiene una descripción disponible."
                ))
            )
        } else {
            String::new()
        };
        let scope = match text(at(&x, "scope")).as_str() {
            "user" => "Cuenta",
            "project" => "Proyecto",
            "mixed" => "Varias configuraciones",
            "compatible" => "Configuración compartida",
            _ => "",
        };
        let fixed = at(&x, "toggleable") == &Value::Bool(false);
        items.push_str(&format!("<label><input type=\"checkbox\" data-kind=\"{kind}\" data-id=\"{}\" {} {}><span>{}{description}<small>{}{}{}</small></span></label>",attr_esc(&key),if enabled{"checked"}else{""},if fixed{"disabled"}else{""},md_esc(&fallback(&x,&["name"],&key)),md_esc(scope),if scope.is_empty(){""}else{" · "},if fixed{"No configurable por perfil"}else{"Al iniciar"}));
    }
    if items.is_empty() {
        items = "<p class=\"sc-note\">No hay elementos disponibles.</p>".into();
    }
    format!(
        "<fieldset><legend>{label}</legend><p class=\"sc-note\">{}</p>{items}</fieldset>",
        md_esc(&reason)
    )
}
pub fn profiles_html(draft: &Value, data: &Value, providers: &Value) -> String {
    let mut selection = draft.clone();
    put(&mut selection, "toHarness", at(draft, "harness").clone());
    let choices = super::session_config::choices(providers, &selection);
    let value = |k| text(at(draft, k));
    let mut profiles = option("", "Nuevo perfil", &value("id"), false);
    for p in rows(at(data, "profiles")) {
        profiles.push_str(&option(
            &text(at(&p, "id")),
            &text(at(&p, "name")),
            &value("id"),
            false,
        ));
    }
    let mut harnesses = String::new();
    for (h, s) in at(providers, "harnesses").as_object().into_iter().flatten() {
        if h != "shell" {
            harnesses.push_str(&option(
                h,
                &fallback(s, &["label"], h),
                &value("harness"),
                false,
            ));
        }
    }
    let mut motors = String::new();
    for r in rows(at(providers, "matrix")) {
        if at(&r, "harness") == at(draft, "harness") {
            let m = text(at(&r, "motor"));
            motors.push_str(&option(
                &m,
                &fallback(at(at(providers, "motors"), &m), &["label"], &m),
                &value("motor"),
                !truth(at(&r, "selectable")),
            ));
        }
    }
    let mut models = option("", "Predeterminado", &value("model"), false);
    for m in rows(at(&choices, "models")) {
        models.push_str(&option(
            &text(at(&m, "id")),
            &fallback(&m, &["name", "id"], ""),
            &value("model"),
            false,
        ));
    }
    let mut efforts = option("", "Predeterminado", &value("effort"), false);
    for e in rows(at(&choices, "efforts")) {
        let e = text(&e);
        efforts.push_str(&option(&e, &e, &value("effort"), false));
    }
    let acp = value("harness") == "acp";
    let account = if acp {
        "motorAccount"
    } else {
        "harnessAccount"
    };
    let extra = if value("motor") != value("harness") && !acp {
        select(
            "motorAccount",
            "Cuenta del motor",
            &account_options(at(&choices, "motorAccounts"), &value("motorAccount"), true),
        )
    } else {
        String::new()
    };
    let report = if truth(at(draft, "templateReport")) {
        format!(
            "<p class=\"sc-note\">{}</p>",
            md_esc(&value("templateReport"))
        )
    } else {
        String::new()
    };
    format!(
        "<p class=\"sc-note\">Guarda CLI, modelo, cuentas, skills y MCPs para iniciar con el mismo equipo. Editar un perfil no modifica las sesiones abiertas.</p><div class=\"sc-grid\">{}<label class=\"sc-field\">Nombre<input name=\"name\" maxlength=\"80\" value=\"{}\"></label>{}{}{}{}{}{extra}</div><div class=\"sc-actions\" style=\"margin:12px 0\"><button class=\"sc-btn\" data-template>Producto completo</button><span class=\"sc-note\">Superpowers · infraestructura · diseño · investigación · x402</span></div>{report}<div class=\"sc-extensions\">{}{}</div><p class=\"sc-error\" data-error role=\"status\"></p><div class=\"sc-actions\"><button class=\"sc-apply\" data-save>Guardar perfil</button><button class=\"sc-btn\" data-launch {}>Nueva sesión con este perfil</button></div>",
        select("profile", "Perfil guardado", &profiles),
        attr_esc(&value("name")),
        select("harness", "CLI", &harnesses),
        select("motor", "Motor", &motors),
        select("model", "Modelo", &models),
        select("effort", "Esfuerzo", &efforts),
        select(
            account,
            if acp {
                "Cuenta del motor"
            } else {
                "Cuenta del CLI"
            },
            &account_options(at(&choices, "accounts"), &value(account), false)
        ),
        inventory_group("skills", "Skills", draft, data),
        inventory_group("mcps", "MCPs", draft, data),
        if truth(at(draft, "id")) {
            ""
        } else {
            "disabled"
        }
    )
}

pub fn overview_card(row: &Value, key: &str, status: &str, token_text: &str) -> String {
    let effort = fallback(row, &["effort"], "");
    format!(
        "<article class=\"overview-card\" data-key=\"{}\"><h3>{} · {}</h3><p>{}</p><p>{} → {}{}</p><p>Cuenta {}{token_text}</p><div class=\"sc-actions\"><button class=\"sc-btn\" data-open>Terminal</button><button class=\"sc-btn\" data-tools>Herramientas</button></div></article>",
        attr_esc(key),
        md_esc(&fallback(row, &["project", "session"], "")),
        md_esc(&fallback(row, &["pane"], "")),
        md_esc(status),
        md_esc(&fallback(row, &["agent"], "shell")),
        md_esc(&fallback(row, &["model"], "modelo sin confirmar")),
        if effort.is_empty() {
            String::new()
        } else {
            format!(" · {}", md_esc(&effort))
        },
        md_esc(&fallback(
            row,
            &["harnessAccount", "account"],
            "sin confirmar"
        ))
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn attach() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(target_arch = "wasm32")]
pub use web::{attach, mount};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    use crate::components::web_support::*;
    use comandos_web_dom::port::*;
    use std::{cell::RefCell, rc::Rc};
    use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
    // Only lexical access is JS. The objects stay owned by the original dashboard.
    #[wasm_bindgen(
        inline_js = "export function workspaceProviders(){return PROVIDERS;} export function workspaceSetProviders(value){PROVIDERS=value;} export function workspaceNS(){return NS;} export function workspaceLabels(){return LABEL;}"
    )]
    extern "C" {
        #[wasm_bindgen(catch,js_name=workspaceProviders)]
        fn providers() -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch,js_name=workspaceSetProviders)]
        fn set_providers(v: &JsValue) -> Result<(), JsValue>;
        #[wasm_bindgen(catch,js_name=workspaceNS)]
        fn ns() -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch,js_name=workspaceLabels)]
        fn labels() -> Result<JsValue, JsValue>;
    }
    thread_local! {static RETURN_FOCUS:RefCell<JsValue>=const{RefCell::new(JsValue::NULL)};static PROFILES:RefCell<Value>=const{RefCell::new(Value::Null)};}
    fn err(message: &str) -> JsValue {
        js_sys::Error::new(message).into()
    }
    fn set_html(body: &JsValue, html: String) -> Result<(), JsValue> {
        set(body, "innerHTML", &utf16_value(&html))
    }
    fn message(body: &JsValue, error: JsValue) {
        let value = get(&error, "message");
        let _ = set(body, "textContent", &value);
    }
    fn g(name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
        invoke(&global(name), args)
    }
    async fn api(path: &str, body: Option<Value>) -> Result<JsValue, JsValue> {
        let mut args = vec![utf16_value(path)];
        if let Some(body) = body {
            args.push(from_utf16_json(&body)?);
        }
        wait(g("api", &args)).await
    }
    fn close() -> Result<(), JsValue> {
        set(&id("session-workspace-modal"), "hidden", &true.into())?;
        RETURN_FOCUS.with(|p| {
            if let Ok(p) = p.try_borrow() {
                let options = object();
                let _ = set(&options, "preventScroll", &true.into());
                let _ = call(&p, "focus", &[options]);
            }
        });
        Ok(())
    }
    fn bind(
        body: &JsValue,
        selector: &str,
        event: &str,
        f: impl Fn(js_sys::Array) -> Result<JsValue, JsValue> + 'static,
    ) -> Result<(), JsValue> {
        let el = query(body, selector);
        if el.is_null() || el.is_undefined() {
            return Ok(());
        }
        set(&el, event, &function(f))
    }
    fn dialog(title: &str) -> Result<JsValue, JsValue> {
        let mut modal = id("session-workspace-modal");
        if modal.is_null() {
            modal = call(&doc(), "createElement", &["div".into()])?;
            set(&modal, "id", &"session-workspace-modal".into())?;
            set(&modal, "className", &"sc-modal".into())?;
            call(&get(&doc(), "body"), "appendChild", &[modal.clone()])?;
        }
        RETURN_FOCUS.with(|p| {
            if let Ok(mut p) = p.try_borrow_mut() {
                *p = get(&doc(), "activeElement");
            }
        });
        set(&modal, "hidden", &false.into())?;
        set_html(
            &modal,
            format!(
                "<section class=\"sc-panel\" role=\"dialog\" aria-modal=\"true\" aria-label=\"{}\"><header><h2>{}</h2><button class=\"sc-btn\" data-close-sc aria-label=\"Cerrar\">×</button></header><div class=\"sc-body\" aria-live=\"polite\">Cargando…</div></section>",
                attr_esc(title),
                md_esc(title)
            ),
        )?;
        bind(&modal, "[data-close-sc]", "onclick", |_| {
            close()?;
            Ok(JsValue::UNDEFINED)
        })?;
        let m = modal.clone();
        set(
            &modal,
            "onclick",
            &function(move |a| {
                if get(&a.get(0), "target") == m {
                    call(&query(&m, "[data-close-sc]"), "click", &[])?;
                }
                Ok(JsValue::UNDEFINED)
            }),
        )?;
        let m = modal.clone();
        set(
            &modal,
            "onkeydown",
            &function(move |a| {
                let e = a.get(0);
                if get(&e, "key") == "Escape" {
                    call(&e, "stopPropagation", &[])?;
                    call(&query(&m, "[data-close-sc]"), "click", &[])?;
                }
                Ok(JsValue::UNDEFINED)
            }),
        )?;
        let options = object();
        set(&options, "preventScroll", &true.into())?;
        call(&query(&modal, "[data-close-sc]"), "focus", &[options])?;
        Ok(query(&modal, ".sc-body"))
    }
    fn params(pairs: &[(&str, String)]) -> Result<String, JsValue> {
        let p = js_sys::Reflect::construct(
            &get(&js_sys::global(), "URLSearchParams").unchecked_into::<js_sys::Function>(),
            &js_sys::Array::new(),
        )?;
        for (k, v) in pairs {
            call(&p, "set", &[(*k).into(), utf16_value(v)])?;
        }
        Ok(string(&call(&p, "toString", &[])?))
    }
    async fn usage(item: JsValue) -> Result<JsValue, JsValue> {
        let body = dialog("Uso de skills y MCPs")?;
        let item = if truthy(&item) {
            to_utf16_json(&item)
        } else {
            Value::Null
        };
        let mut pairs = vec![("days", "7".into())];
        for key in ["session", "pane"] {
            if truth(at(&item, key)) {
                pairs.push((key, text(at(&item, key))));
            }
        }
        let result =
            async { api(&format!("/extension-usage?{}", params(&pairs)?), None).await }.await;
        match result {
            Ok(result) => {
                if truthy(&get(&body, "isConnected")) {
                    set_html(&body, usage_html(&to_utf16_json(&result), &item))?;
                }
            }
            Err(e) => message(&body, e),
        }
        Ok(JsValue::UNDEFINED)
    }
    #[derive(Clone)]
    struct Editor {
        draft: Value,
        data: Value,
        current: Value,
        body: JsValue,
    }
    type Shared = Rc<RefCell<Editor>>;
    fn snapshot(e: &Shared) -> Result<Editor, JsValue> {
        e.try_borrow()
            .map(|x| x.clone())
            .map_err(|_| err("Profile editor is busy"))
    }
    fn mutate(e: &Shared, f: impl FnOnce(&mut Editor)) -> Result<(), JsValue> {
        let mut e = e
            .try_borrow_mut()
            .map_err(|_| err("Profile editor is busy"))?;
        f(&mut e);
        Ok(())
    }
    fn disabled(e: &Shared) -> Result<(), JsValue> {
        let s = snapshot(e)?;
        set(&query(&s.body, "[data-launch]"), "disabled", &true.into())
    }
    fn editor_error(e: &Shared, error: JsValue) {
        if let Ok(s) = snapshot(e) {
            message(&query(&s.body, "[data-error]"), error);
        }
    }
    async fn load(e: &Shared) -> Result<(), JsValue> {
        let s = snapshot(e)?;
        let mut pairs = vec![
            ("harness", text(at(&s.draft, "harness"))),
            ("account", text(at(&s.draft, "harnessAccount"))),
        ];
        let cwd = fallback(
            &s.current,
            &["cwd"],
            &utf16_string(&get(&id("ns-cwd"), "value")),
        );
        if !cwd.is_empty() {
            pairs.push(("cwd", cwd));
        }
        let result =
            to_utf16_json(&api(&format!("/session-profiles?{}", params(&pairs)?), None).await?);
        PROFILES.with(|p| {
            if let Ok(mut p) = p.try_borrow_mut() {
                *p = at(&result, "profiles").clone();
            }
        });
        mutate(e, |e| e.data = result)
    }
    fn async_handler(
        e: Shared,
        task: impl std::future::Future<Output = Result<(), JsValue>> + 'static,
    ) -> JsValue {
        promise(async move {
            if let Err(error) = task.await {
                editor_error(&e, error);
            }
            Ok(JsValue::UNDEFINED)
        })
    }
    fn render(e: &Shared) -> Result<(), JsValue> {
        let s = snapshot(e)?;
        let registry = to_utf16_json(&providers()?);
        set_html(&s.body, profiles_html(&s.draft, &s.data, &registry))?;
        let ee = e.clone();
        bind(&s.body, "[name=name]", "oninput", move |a| {
            let name = to_utf16_json(&get(&get(&a.get(0), "target"), "value"));
            mutate(&ee, |e| put(&mut e.draft, "name", name))?;
            disabled(&ee)?;
            Ok(JsValue::UNDEFINED)
        })?;
        let ee = e.clone();
        bind(&s.body, "[name=profile]", "onchange", move |a| {
            let id = to_utf16_json(&get(&get(&a.get(0), "target"), "value"));
            let p = PROFILES.with(|p| {
                p.try_borrow()
                    .ok()
                    .and_then(|p| rows(&p).into_iter().find(|p| at(p, "id") == &id))
            });
            mutate(&ee, |e| {
                if let Some(p) = p {
                    e.draft = p;
                } else {
                    remove(&mut e.draft, "id");
                    put(&mut e.draft, "name", json!("Mi perfil"));
                }
            })?;
            let editor = ee.clone();
            Ok(async_handler(ee.clone(), async move {
                load(&editor).await?;
                render(&editor)
            }))
        })?;
        let ee = e.clone();
        bind(&s.body, "[name=harness]", "onchange", move |a| {
            let harness = utf16_string(&get(&get(&a.get(0), "target"), "value"));
            mutate(&ee, |e| change_harness(&mut e.draft, &harness))?;
            let editor = ee.clone();
            Ok(async_handler(ee.clone(), async move {
                load(&editor).await?;
                render(&editor)
            }))
        })?;
        for name in ["motor", "model", "effort", "harnessAccount", "motorAccount"] {
            let ee = e.clone();
            let field = query(&s.body, &format!("[name={name}]"));
            let input = field.clone();
            if field.is_null() {
                continue;
            }
            set(
                &field,
                "onchange",
                &function(move |_| {
                    let value = utf16_string(&get(&input, "value"));
                    let providers = to_utf16_json(&providers()?);
                    mutate(&ee, |e| {
                        e.draft = change_field(&providers, &e.draft, name, &value)
                    })?;
                    let editor = ee.clone();
                    Ok(async_handler(ee.clone(), async move {
                        if name == "harnessAccount" {
                            load(&editor).await?;
                        }
                        render(&editor)?;
                        disabled(&editor)
                    }))
                }),
            )?;
        }
        for el in all(&s.body, "[data-kind]") {
            let ee = e.clone();
            let input = el.clone();
            set(
                &el,
                "onchange",
                &function(move |_| {
                    let dataset = get(&input, "dataset");
                    let kind = utf16_string(&get(&dataset, "kind"));
                    let key = utf16_string(&get(&dataset, "id"));
                    let checked = truthy(&get(&input, "checked"));
                    mutate(&ee, |e| {
                        let mut selected = at(&e.draft, &kind).clone();
                        if !truth(&selected) {
                            selected = json!({});
                        }
                        put(&mut selected, &key, json!(checked));
                        put(&mut e.draft, &kind, selected);
                    })?;
                    disabled(&ee)?;
                    Ok(JsValue::UNDEFINED)
                }),
            )?;
        }
        let ee = e.clone();
        bind(&s.body, "[data-template]", "onclick", move |_| {
            mutate(&ee, |e| {
                product_template(&mut e.draft, at(&e.data, "inventory"))
            })?;
            render(&ee)?;
            disabled(&ee)?;
            Ok(JsValue::UNDEFINED)
        })?;
        let ee = e.clone();
        bind(&s.body, "[data-save]", "onclick", move |a| {
            let button = get(&a.get(0), "target");
            set(&button, "disabled", &true.into())?;
            let editor = ee.clone();
            Ok(promise(async move {
                let result = async {
                    let s = snapshot(&editor)?;
                    let r = to_utf16_json(
                        &api("/session-profiles", Some(saved_draft(&s.draft))).await?,
                    );
                    let draft = if truth(at(&r, "profile")) {
                        at(&r, "profile").clone()
                    } else {
                        r
                    };
                    mutate(&editor, |e| e.draft = draft)?;
                    if !truth(at(&snapshot(&editor)?.draft, "id")) {
                        return Err(err("El servidor no devolvió el perfil guardado"));
                    }
                    load(&editor).await?;
                    render(&editor)?;
                    g(
                        "toast",
                        &["Perfil guardado. Se aplica a nuevas sesiones.".into()],
                    )?;
                    Ok(())
                }
                .await;
                if let Err(error) = result {
                    editor_error(&editor, error);
                    set(&button, "disabled", &false.into())?;
                }
                Ok(JsValue::UNDEFINED)
            }))
        })?;
        let ee = e.clone();
        bind(&s.body, "[data-launch]", "onclick", move |_| {
            let editor = ee.clone();
            Ok(async_handler(ee.clone(), async move {
                let s = snapshot(&editor)?;
                use_profile(
                    from_utf16_json(at(&s.draft, "id"))?,
                    from_utf16_json(at(&s.current, "cwd"))?,
                )
                .await?;
                set(&id("session-workspace-modal"), "hidden", &true.into())
            }))
        })?;
        Ok(())
    }
    async fn profiles(item: JsValue) -> Result<JsValue, JsValue> {
        let body = dialog("Perfiles de sesión")?;
        let current = if truthy(&item) {
            item
        } else {
            g("pickSel", &[get(&state(), "list")]).unwrap_or_else(|_| object())
        };
        let current = to_utf16_json(&current);
        let editor = Rc::new(RefCell::new(Editor {
            draft: initial_draft(&current),
            current,
            data: Value::Null,
            body: body.clone(),
        }));
        let result = async {
            if !truthy(&providers()?) {
                set_providers(&api("/providers", None).await?)?;
            }
            load(&editor).await?;
            render(&editor)
        }
        .await;
        if let Err(error) = result {
            message(&body, error);
        }
        Ok(JsValue::UNDEFINED)
    }
    async fn use_profile(id: JsValue, cwd: JsValue) -> Result<JsValue, JsValue> {
        let path_cwd = if truthy(&cwd) {
            cwd.clone()
        } else {
            let value = get(&id_element("ns-cwd"), "value");
            if truthy(&value) { value } else { "".into() }
        };
        let r = api(
            "/session-profile-apply",
            Some(json!({"profileId":to_utf16_json(&id),"cwd":to_utf16_json(&path_cwd)})),
        )
        .await?;
        let draft = get(&r, "launchDraft");
        let draft = if truthy(&draft) { draft } else { object() };
        let ns = ns()?;
        set(&ns, "intoPane", &JsValue::NULL)?;
        wait(g("nsOpen", &[])).await?;
        call(
            &global("Object"),
            "assign",
            &[ns.clone(), draft.clone(), {
                let p = object();
                set(&p, "profileId", &id)?;
                p
            }],
        )?;
        for key in ["agent", "toHarness"] {
            let v = get(&draft, key);
            if truthy(&v) {
                set(&ns, "harness", &v)?;
            }
        }
        if truthy(&cwd) {
            set(&id_element("ns-cwd"), "value", &cwd)?;
        }
        g("nsSelectDefaults", &[])?;
        g("nsRender", &[])?;
        let note = id_element("ns-profile-note");
        if !note.is_null() {
            set(
                &note,
                "textContent",
                &"Perfil seleccionado. Skills y MCPs se aplicarán al iniciar.".into(),
            )?;
        }
        Ok(JsValue::UNDEFINED)
    }
    fn id_element(name: &str) -> JsValue {
        id(name)
    }
    fn overview(list: JsValue) -> Result<JsValue, JsValue> {
        let box_ = id("session-overview");
        if box_.is_null() || truthy(&get(&box_, "hidden")) {
            return Ok(JsValue::UNDEFINED);
        }
        let list = if truthy(&list) {
            list
        } else {
            js_sys::Array::new().into()
        };
        let items = js_sys::Array::from(&list)
            .iter()
            .filter(|it| truthy(&get(it, "alive")))
            .collect::<Vec<_>>();
        let signature = js_sys::Array::new();
        for it in &items {
            let r = js_sys::Array::new();
            r.push(&g("rowKey", std::slice::from_ref(it))?);
            for key in [
                "project", "agent", "motor", "model", "effort", "account", "status",
            ] {
                r.push(&get(it, key));
            }
            let usage = g("usageForItem", std::slice::from_ref(it))?;
            r.push(&get(&usage, "total_tokens"));
            signature.push(&r);
        }
        let signature = js_sys::JSON::stringify(&signature)?;
        let dataset = get(&box_, "dataset");
        if get(&dataset, "signature") == JsValue::from(signature.clone()) {
            return Ok(JsValue::UNDEFINED);
        }
        set(&dataset, "signature", &signature.into())?;
        let labels = labels()?;
        let mut html = String::new();
        for it in &items {
            let row = to_utf16_json(it);
            let key = utf16_string(&g("rowKey", std::slice::from_ref(it))?);
            let usage = g("usageForItem", std::slice::from_ref(it))?;
            let tokens = ["tokens", "totalTokens", "total_tokens"]
                .into_iter()
                .map(|k| get(&usage, k))
                .find(|v| !v.is_null() && !v.is_undefined());
            let status = get(&labels, &text(at(&row, "status")));
            let status = if truthy(&status) {
                utf16_string(&status)
            } else {
                text(at(&row, "status"))
            };
            let token_text = if let Some(tokens) = tokens {
                format!(
                    " · {} tokens",
                    md_esc(&utf16_string(&g("fmtTokens", &[tokens])?))
                )
            } else {
                String::new()
            };
            html.push_str(&overview_card(&row, &key, &status, &token_text));
        }
        if html.is_empty() {
            html = "<p class=\"sc-note\">No hay sesiones activas.</p>".into();
        }
        set_html(&box_, html)?;
        for card in all(&box_, "[data-key]") {
            let key = get(&get(&card, "dataset"), "key");
            let it = items
                .iter()
                .find(|it| g("rowKey", &[(*it).clone()]).is_ok_and(|v| v == key))
                .cloned()
                .unwrap_or(JsValue::UNDEFINED);
            let current = it.clone();
            bind(&card, "[data-open]", "onclick", move |_| {
                g("openSession", std::slice::from_ref(&current))
            })?;
            bind(&card, "[data-tools]", "onclick", move |_| {
                Ok(promise(profiles(it.clone())))
            })?;
        }
        Ok(JsValue::UNDEFINED)
    }
    pub fn attach() -> Result<(), JsValue> {
        for name in ["open-session-profiles", "ns-profile-open"] {
            set(
                &id(name),
                "onclick",
                &function(|_| Ok(promise(profiles(JsValue::UNDEFINED)))),
            )?;
        }
        set(
            &id("open-extension-usage"),
            "onclick",
            &function(|_| Ok(promise(usage(JsValue::UNDEFINED)))),
        )?;
        set(
            &id("toggle-overview"),
            "onclick",
            &function(|a| {
                let b = id("session-overview");
                let hidden = !truthy(&get(&b, "hidden"));
                set(&b, "hidden", &hidden.into())?;
                attr(
                    &get(&a.get(0), "currentTarget"),
                    "aria-pressed",
                    if hidden { "false" } else { "true" },
                );
                overview(get(&state(), "list"))
            }),
        )?;
        set(
            &id("ns-profile-clear"),
            "onclick",
            &function(|_| {
                js_sys::Reflect::delete_property(
                    &js_sys::Object::from(ns()?),
                    &"profileId".into(),
                )?;
                set(&id("ns-profile-note"), "textContent", &"Sin perfil".into())?;
                Ok(JsValue::UNDEFINED)
            }),
        )?;
        Ok(())
    }
    pub fn mount() -> Result<(), JsValue> {
        let root = js_sys::global();
        for (name, callback) in [
            (
                "openExtensionUsage",
                function(|a| Ok(promise(usage(a.get(0))))),
            ),
            (
                "openSessionProfiles",
                function(|a| Ok(promise(profiles(a.get(0))))),
            ),
            (
                "useSessionProfile",
                function(|a| Ok(promise(use_profile(a.get(0), a.get(1))))),
            ),
            ("renderSessionOverview", function(|a| overview(a.get(0)))),
            ("scDialog", function(|a| dialog(&utf16_string(&a.get(0))))),
            (
                "scOption",
                function(|a| {
                    Ok(utf16_value(&option(
                        &utf16_string(&a.get(0)),
                        &utf16_string(&a.get(1)),
                        &utf16_string(&a.get(2)),
                        truthy(&a.get(3)),
                    )))
                }),
            ),
            (
                "scSelect",
                function(|a| {
                    Ok(utf16_value(&select(
                        &utf16_string(&a.get(0)),
                        &utf16_string(&a.get(1)),
                        &utf16_string(&a.get(2)),
                    )))
                }),
            ),
            (
                "initSessionWorkspace",
                function(|_| {
                    attach()?;
                    Ok(JsValue::UNDEFINED)
                }),
            ),
        ] {
            set(&root, name, &callback)?;
        }
        Ok(())
    }
    use wasm_bindgen::JsCast;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_preserve_shell_motor_and_independent_accounts() {
        let d = initial_draft(&json!({"agent":"shell","account":"work","motorAccount":"other"}));
        assert_eq!(at(&d, "harness"), "codex");
        assert_eq!(at(&d, "motor"), "shell");
        assert_eq!(at(&d, "harnessAccount"), "work");
        assert_eq!(at(&d, "motorAccount"), "other");
        assert_eq!(at(&initial_draft(&json!({})), "motor"), "codex");
    }
    #[test]
    fn harness_change_discards_id_and_extension_overrides_but_keeps_accounts() {
        let mut d = json!({"id":"saved","name":"n","model":"m","effort":"high","skills":{"s":true},"mcps":{"m":false},"harnessAccount":"work","motorAccount":"other"});
        change_harness(&mut d, "acp");
        assert!(d.get("id").is_none());
        assert_eq!(at(&d, "routeId"), "acp:acp");
        assert_eq!(at(&d, "skills"), &json!({}));
        assert_eq!(at(&d, "mcps"), &json!({}));
        assert_eq!(at(&d, "harnessAccount"), "work");
        assert_eq!(at(&d, "motorAccount"), "other");
        assert_eq!(at(&d, "model"), "");
    }
    #[test]
    fn template_respects_fixed_skills_and_reports_verifiable_families() {
        let mut d = json!({"skills":{"fixed":false},"mcps":{"keep":false}});
        let inventory = json!({"skills":[{"id":"fixed","name":"x402","toggleable":false},{"id":"sp","name":"Brainstorm"},{"id":"do","path":"DigitalOcean"},{"id":"ui","name":"design-research"},{"id":"research","name":"deep research"},{"id":"other","name":"unknown"},{"id":"plans","name":"executing-plans"}]});
        product_template(&mut d, &inventory);
        assert_eq!(d.pointer("/skills/fixed"), Some(&json!(false)));
        assert_eq!(d.pointer("/skills/other"), Some(&json!(false)));
        assert_eq!(d.pointer("/skills/plans"), Some(&json!(true)));
        assert_eq!(d.pointer("/mcps/keep"), Some(&json!(false)));
        assert!(text(at(&d, "templateReport")).contains("x402"));
        let complete = json!({"skills":[{"name":"superpowers digitalocean design-research deep-research x402"}]});
        product_template(&mut d, &complete);
        assert!(text(at(&d, "templateReport")).starts_with("Las cinco familias"));
        assert!(saved_draft(&d).get("templateReport").is_none());
    }
    #[test]
    fn profile_ui_keeps_scope_and_capabilities_readonly_and_disables_ineligible_accounts() {
        let d = json!({"id":"p","name":"<saved>\"","harness":"acp","motor":"codex","motorAccount":"locked","skills":{"s":false},"mcps":{}});
        let data = json!({"profiles":[{"id":"p","name":"<saved>"}],"inventory":{"skills":[{"id":"s","name":"skill","enabled":true,"scope":"mixed","toggleable":false}],"mcps":[{"id":"m","name":"<mcp>","description":"<desc>"}]},"capabilities":{"skills":{"reason":"<why>"}}});
        let providers = json!({"harnesses":{"shell":{"label":"Shell"},"acp":{},"codex":{"accounts":[{"alias":"locked","identity":"a<b","selectable":false}]}},"matrix":[{"harness":"acp","motor":"codex","selectable":false}],"motors":{"codex":{"models":[]}}});
        let html = profiles_html(&d, &data, &providers);
        assert!(html.contains("name=\"motorAccount\""));
        assert!(!html.contains("name=\"harnessAccount\""));
        assert!(html.contains("value=\"locked\" selected disabled"));
        assert!(html.contains("&lt;desc&gt;"));
        assert!(html.contains("Varias configuraciones · No configurable por perfil"));
        assert!(html.contains("&lt;why&gt;"));
        assert!(!html.contains("value=\"shell\""));
        assert!(html.contains("data-id=\"s\"  disabled"));
        assert!(html.contains("data-launch >"));
        assert!(html.contains("value=\"&lt;saved&gt;&quot;\""));
    }
    #[test]
    fn usage_fallbacks_count_zero_and_keep_observed_provenance() {
        let html = usage_html(
            &json!({"extensions":[{"tool":"<tool>","calls":0,"count":8,"durationMs":1250,"kind":"mcp"}],"provenance":"observed","unattributedSkillCalls":2}),
            &json!({"session":"<session>","pane":"%0"}),
        );
        assert!(html.contains("&lt;tool&gt;"));
        assert!(html.contains("<td>0</td><td>1.3 s</td>"));
        assert!(html.contains("Medido en eventos locales"));
        assert!(html.contains("2 llamadas a skills"));
        assert!(html.contains("&lt;session&gt; · %0"));
        let empty = usage_html(
            &json!({"items":[],"extensions":[{"name":"ignored"}]}),
            &Value::Null,
        );
        assert!(empty.contains("Todavía no hay llamadas"));
        assert!(!empty.contains("ignored"));
        assert!(empty.contains("Todas las sesiones"));
    }
    #[test]
    fn select_markup_escapes_attributes_without_changing_labels() {
        let html = option("x\"<&", "<label>", "x\"<&", true);
        assert_eq!(
            html,
            "<option value=\"x&quot;&lt;&amp;\" selected disabled>&lt;label&gt;</option>"
        );
    }
    #[test]
    fn field_change_uses_shared_model_account_rules_and_keeps_saved_id() {
        let registry = json!({"motors":{"codex":{"models":[{"id":"m","efforts":["low","high"],"defaultEffort":"high"}]}},"harnesses":{"acp":{},"codex":{}},"matrix":[{"harness":"acp","motor":"codex","selectable":true}]});
        let d = json!({"id":"p","harness":"acp","motor":"codex","model":"m","effort":"invalid","harnessAccount":"work","motorAccount":"motor"});
        let next = change_field(&registry, &d, "model", "m");
        assert!(next.get("toHarness").is_none());
        assert_eq!(at(&next, "harnessAccount"), "main");
        assert_eq!(at(&next, "motorAccount"), "motor");
        assert_eq!(at(&next, "effort"), "high");
        assert_eq!(at(&next, "routeId"), "acp:codex");
        assert_eq!(at(&next, "id"), "p");
    }
    #[test]
    fn overview_keeps_account_priority_and_tokens_without_inventing_model() {
        let html = overview_card(
            &json!({"project":"<p>","session":"fallback","pane":"%3","agent":"codex","harnessAccount":"cli","account":"generic","effort":"high"}),
            "x\"",
            "waiting",
            " · 0 tokens",
        );
        assert!(html.contains("data-key=\"x&quot;\""));
        assert!(html.contains("&lt;p&gt; · %3"));
        assert!(html.contains("codex → modelo sin confirmar · high"));
        assert!(html.contains("Cuenta cli · 0 tokens"));
        assert!(html.contains("data-open") && html.contains("data-tools"));
    }
    #[test]
    fn duration_matches_exact_binary_javascript_rounding() {
        assert_eq!(fixed(1.25, 1), "1.3");
        assert_eq!(fixed(1.15, 1), "1.1");
        assert_eq!(fixed(-0.001, 1), "-0.0");
        assert_eq!(fixed(-0.0, 1), "0.0");
        assert_eq!(fixed(f64::INFINITY, 1), "Infinity");
    }
}
