use super::super::{web_support::*, workspace_layout};
use super::*;
use comandos_web_dom::{bridge::global_set, port::*};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
// Only exposes existing lexical bindings; all workspace behavior lives in Rust.
#[wasm_bindgen(
    inline_js = "export function dockGlobals(){return {openTerms:typeof openTerms==='undefined'?undefined:openTerms,activeTerm:typeof activeTerm==='undefined'?undefined:activeTerm,activeView:typeof activeView==='undefined'?undefined:activeView};}"
)]
extern "C" {
    fn dockGlobals() -> JsValue;
}
struct Gesture {
    pointer: JsValue,
    el: JsValue,
    source: String,
    path: Option<String>,
    x: f64,
    y: f64,
    kind: String,
    active: bool,
    target: Option<Target>,
    ghost: JsValue,
    preview: JsValue,
    trays: JsValue,
    timer: JsValue,
    raf: JsValue,
    lx: f64,
    ly: f64,
    changed: bool,
    original: Option<Value>,
}
#[derive(Clone)]
enum Target {
    Bar {
        index: i64,
        rect: Rect,
        label: String,
    },
    Dock {
        target: String,
        edge: String,
        rect: Rect,
        label: String,
    },
    Mark(String),
}
struct Ui {
    dock: Dock,
    gesture: Option<Gesture>,
    suppress: f64,
    key_timer: JsValue,
}
type Shared = Rc<RefCell<Ui>>;
thread_local! {static UI:Shared=Rc::new(RefCell::new(Ui{dock:Dock::default(),gesture:None,suppress:0.0,key_timer:JsValue::NULL}));}
fn shared() -> Result<Shared, JsValue> {
    UI.try_with(Clone::clone)
        .map_err(|_| JsValue::from_str("dock unavailable"))
}
fn event(f: impl Fn(JsValue) -> Result<(), JsValue> + 'static) -> JsValue {
    function(move |a| {
        f(a.get(0))?;
        Ok(JsValue::UNDEFINED)
    })
}
fn closest(el: &JsValue, selector: &str) -> JsValue {
    call(el, "closest", &[selector.into()]).unwrap_or(JsValue::NULL)
}
fn data(el: &JsValue, key: &str) -> String {
    get(&get(el, "dataset"), key)
        .as_string()
        .unwrap_or_default()
}
fn rect(el: &JsValue) -> Rect {
    let r = call(el, "getBoundingClientRect", &[]).unwrap_or(JsValue::NULL);
    Rect {
        left: number(&get(&r, "left")),
        top: number(&get(&r, "top")),
        width: number(&get(&r, "width")),
        height: number(&get(&r, "height")),
    }
}
fn from_rect(v: &Value) -> Rect {
    Rect {
        left: field(v, "left").as_f64().unwrap_or(0.0),
        top: field(v, "top").as_f64().unwrap_or(0.0),
        width: field(v, "width").as_f64().unwrap_or(0.0),
        height: field(v, "height").as_f64().unwrap_or(0.0),
    }
}
fn rect_json(r: Rect) -> Value {
    json!({"left":r.left,"top":r.top,"width":r.width,"height":r.height})
}
fn active_term() -> String {
    get(&dockGlobals(), "activeTerm")
        .as_string()
        .unwrap_or_default()
}
fn active_view() -> JsValue {
    get(&dockGlobals(), "activeView")
}
fn terms() -> JsValue {
    get(&dockGlobals(), "openTerms")
}
fn live(d: &Dock, tab: &str) -> bool {
    truthy(&call(&terms(), "has", &[d.session(tab).into()]).unwrap_or(JsValue::FALSE))
}
fn label(d: &Dock, tab: &str) -> String {
    let open = call(&terms(), "get", &[d.session(tab).into()]).unwrap_or(JsValue::NULL);
    let l = get(&open, "label").as_string().unwrap_or_default();
    if !l.is_empty() {
        return l;
    }
    d.doc
        .as_ref()
        .map(|doc| text(field(field(field(doc, "tabs"), tab), "label")))
        .filter(|s| !s.is_empty())
        .unwrap_or(tab.into())
}
fn translate(es: &str, en: &str) -> String {
    invoke(&global("tf"), &[es.into(), en.into()])
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or(es.into())
}
fn focus(d: &Dock, g: &Value) -> String {
    let ids = tab_ids(field(g, "tree"));
    let active = active_term();
    if ids.contains(&active) {
        return active;
    }
    let remembered = d
        .focus
        .get(&text(field(g, "id")))
        .cloned()
        .unwrap_or_default();
    if ids.contains(&remembered) {
        return remembered;
    }
    ids.iter()
        .find(|t| live(d, t))
        .cloned()
        .or_else(|| ids.first().cloned())
        .unwrap_or_default()
}
fn entries(d: &Dock) -> Value {
    if d.doc.is_none() {
        return Value::Null;
    }
    json!(d.groups().iter().map(|g|{let ids=tab_ids(field(g,"tree"));let first=ids.first().cloned().unwrap_or_default();let session=d.session(&first);let focus=focus(d,g);let group_id=text(field(g,"id"));json!({"key":format!("group:{group_id}"),"target":format!("term:{}",d.session(&focus)),"group":g,"tabs":ids,"label":label(d,&first),"count":ids.len(),"session":session,"closable":ids.len()>1||session!="local","groupId":if ids.len()>1{json!(group_id)}else{Value::Null},"source":if ids.len()==1{first.clone()}else{format!("group:{group_id}")},"live":ids.iter().any(|t|live(d,t))})}).collect::<Vec<_>>())
}
fn node_html(d: &Dock, n: &Value, path: &[String], single: bool) -> String {
    use comandos_web_view::escape::text as esc;
    if field(n, "type").as_str() == Some("tab") {
        let t = text(field(n, "tabId"));
        let ok = live(d, &t);
        return format!(
            "<section class=\"ws-leaf{}{}\" data-tab=\"{}\"><header class=\"ws-leaf-head\" data-ws-source=\"{}\" title=\"{}\"><span class=\"ws-grip\" aria-hidden=\"true\">⠿</span><b>{}</b></header><div class=\"ws-body\" data-body=\"{}\">{}</div></section>",
            if single { " single" } else { "" },
            if ok { "" } else { " unavailable" },
            esc(&t),
            esc(&t),
            esc(&translate(
                "Arrastra para mover esta tab",
                "Drag to move this tab"
            )),
            esc(&label(d, &t)),
            esc(&t),
            if ok {
                String::new()
            } else {
                format!(
                    "<p class=\"ws-missing\">{}</p>",
                    esc(&translate(
                        "Sesión no disponible. Su lugar se conserva.",
                        "Session unavailable. Its place is kept."
                    ))
                )
            }
        );
    }
    let p = path.join(".");
    let mut first = path.to_vec();
    first.push("first".into());
    let mut second = path.to_vec();
    second.push("second".into());
    format!(
        "<div class=\"ws-split\" data-path=\"{p}\" data-axis=\"{}\"><div class=\"ws-slot\">{}</div><div class=\"ws-divider\" data-ws-divider=\"{p}\" role=\"separator\" tabindex=\"0\" aria-label=\"{}\"></div><div class=\"ws-slot\">{}</div></div>",
        esc(&text(field(n, "axis"))),
        node_html(d, field(n, "first"), &first, false),
        esc(&translate("Ajustar división", "Resize split")),
        node_html(d, field(n, "second"), &second, false)
    )
}
fn tree_el() -> Result<JsValue, JsValue> {
    let area = id("term-area");
    let el = query(&area, ":scope > .ws-tree");
    if !el.is_null() {
        return Ok(el);
    }
    let el = call(&doc(), "createElement", &["div".into()])?;
    set(&el, "className", &"ws-tree".into())?;
    call(&area, "prepend", std::slice::from_ref(&el))?;
    Ok(el)
}
fn child(el: &JsValue, n: u32) -> JsValue {
    get(&get(el, "children"), &n.to_string())
}
fn grid(n: &Value, el: &JsValue, m: &Measurement) {
    if field(n, "type").as_str() == Some("tab") {
        return;
    }
    let ratio = field(n, "ratio").as_f64().unwrap_or(0.5);
    let _ = set(&get(el, "dataset"), "axis", &m.axis.into());
    style(
        el,
        "grid-template-columns",
        &if m.axis == "x" {
            format!("minmax(0,{ratio}fr) 6px minmax(0,{}fr)", 1.0 - ratio)
        } else {
            "minmax(0,1fr)".into()
        },
    );
    style(
        el,
        "grid-template-rows",
        &if m.axis == "y" {
            if field(n, "axis").as_str() == Some("y") {
                format!("minmax(0,{ratio}fr) 6px minmax(0,{}fr)", 1.0 - ratio)
            } else {
                format!(
                    "minmax(0,{}fr) 6px minmax(0,{}fr)",
                    m.a.as_ref().map(|a| a.height).unwrap_or(0.0),
                    m.b.as_ref().map(|b| b.height).unwrap_or(0.0)
                )
            }
        } else {
            "minmax(0,1fr)".into()
        },
    );
    let divider = child(el, 1);
    attr(
        &divider,
        "aria-orientation",
        if m.axis == "x" {
            "vertical"
        } else {
            "horizontal"
        },
    );
    let _ = call(
        &divider,
        "toggleAttribute",
        &[
            "data-stacked".into(),
            (Some(m.axis) != field(n, "axis").as_str()).into(),
        ],
    );
    if let Some(a) = &m.a {
        grid(
            field(n, "first"),
            &get(&child(el, 0), "firstElementChild"),
            a,
        )
    }
    if let Some(b) = &m.b {
        grid(
            field(n, "second"),
            &get(&child(el, 2), "firstElementChild"),
            b,
        )
    }
}
fn place_frames(d: &Dock, tabs: &[String]) {
    let area = id("term-area");
    let base = rect(&area);
    let entries = js_sys::Array::from(&terms());
    for entry in entries.iter() {
        let entry = js_sys::Array::from(&entry);
        let sess = string(&entry.get(0));
        let frame = get(&entry.get(1), "frame");
        if !truthy(&frame) {
            continue;
        }
        let Some(tab) = tabs.iter().find(|t| d.session(t) == sess) else {
            let _ = set(&get(&frame, "style"), "cssText", &"".into());
            continue;
        };
        let escaped = call(&global("CSS"), "escape", &[tab.clone().into()])
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or(tab.clone());
        let body = query(&doc(), &format!(".ws-tree [data-body=\"{escaped}\"]"));
        if body.is_null() {
            continue;
        }
        let r = rect(&body);
        for (k, v) in [
            ("inset", "auto".into()),
            (
                "left",
                format!(
                    "{}px",
                    r.left - base.left + number(&get(&area, "scrollLeft"))
                ),
            ),
            (
                "top",
                format!("{}px", r.top - base.top + number(&get(&area, "scrollTop"))),
            ),
            ("width", format!("{}px", r.width)),
            ("height", format!("{}px", r.height)),
        ] {
            style(&frame, k, &v)
        }
    }
}
fn layout(u: &Ui) -> Result<(), JsValue> {
    let Some(g) = u.dock.group(&active_term()) else {
        return Ok(());
    };
    layout_group(&u.dock, g)
}
fn layout_group(d: &Dock, g: &Value) -> Result<(), JsValue> {
    layout_at(d, g, id("term-area"), tree_el()?)
}
fn layout_at(d: &Dock, g: &Value, area: JsValue, tree: JsValue) -> Result<(), JsValue> {
    if truthy(&get(&tree, "hidden")) {
        return Ok(());
    }
    let m = measure(field(g, "tree"), number(&get(&area, "clientWidth")), None);
    style(
        &tree,
        "height",
        &format!("{}px", number(&get(&area, "clientHeight")).max(m.height)),
    );
    grid(field(g, "tree"), &get(&tree, "firstElementChild"), &m);
    place_frames(d, &tab_ids(field(g, "tree")));
    Ok(())
}
fn render(u: &mut Ui, shown: &str) -> Result<(), JsValue> {
    let tree = tree_el()?;
    let g = u.dock.group(shown).cloned();
    let Some(g) = g else {
        set(&tree, "hidden", &true.into())?;
        set(&tree, "innerHTML", &"".into())?;
        u.dock.signature.clear();
        place_frames(&u.dock, &[]);
        return Ok(());
    };
    u.dock.focus.insert(text(field(&g, "id")), shown.into());
    let n = field(&g, "tree");
    let single = field(n, "type").as_str() == Some("tab");
    let signature = json!([
        field(&g, "id"),
        if single { n.clone() } else { strip_ratios(n) },
        tab_ids(n)
            .iter()
            .map(|t| json!([label(&u.dock, t), live(&u.dock, t)]))
            .collect::<Vec<_>>()
    ])
    .to_string();
    if signature != u.dock.signature {
        set(
            &tree,
            "innerHTML",
            &node_html(&u.dock, n, &[], single).into(),
        )?;
        u.dock.signature = signature;
    }
    set(&tree, "hidden", &false.into())?;
    for el in all(&tree, ".ws-leaf") {
        classes(&el, "focused", !single && data(&el, "tab") == shown)
    }
    layout_group(&u.dock, &g)
}
fn show(key: JsValue, force: bool) {
    let args = if force {
        vec![key, true.into()]
    } else {
        vec![key]
    };
    let _ = invoke(&global("showView"), &args);
}
fn commit(ui: Shared, next: Value, focus_tab: Option<String>) {
    let (before, revision, key) = {
        let Ok(mut u) = ui.try_borrow_mut() else {
            return;
        };
        let before = u.dock.doc.clone();
        let revision = u.dock.revision;
        u.dock.doc = Some(next.clone());
        u.dock.pending = true;
        let key = focus_tab
            .as_ref()
            .map(|t| format!("term:{}", u.dock.session(t)).into())
            .unwrap_or_else(active_view);
        (before, revision, key)
    };
    show(key, focus_tab.is_some());
    wasm_bindgen_futures::spawn_local(async move {
        let headers = object();
        let _ = set(&headers, "Content-Type", &"application/json".into());
        let token = invoke(&global("authToken"), &[]).unwrap_or(JsValue::NULL);
        if truthy(&token) {
            let _ = set(&headers, "X-Comandos-Token", &token);
        }
        let id = call(&global("crypto"), "randomUUID", &[]).unwrap_or_else(|_| {
            format!("{}{}", js_sys::Date::now(), js_sys::Math::random()).into()
        });
        let options = object();
        let _ = set(&options, "method", &"POST".into());
        let _ = set(&options, "headers", &headers);
        let _ = set(
            &options,
            "body",
            &json!({"requestId":string(&id),"expectedRevision":revision,"document":next})
                .to_string()
                .into(),
        );
        let result = wait(call(
            &js_sys::global(),
            "fetch",
            &["/workspace".into(), options],
        ))
        .await;
        let result = match result {
            Ok(r) => {
                let body = wait(call(&r, "json", &[]))
                    .await
                    .unwrap_or_else(|_| object());
                Ok((number(&get(&r, "status")), to_json(&body)))
            }
            Err(e) => Err(e),
        };
        let mut message = None;
        {
            let Ok(mut u) = ui.try_borrow_mut() else {
                return;
            };
            u.dock.pending = false;
            match result {
                Ok((200.0, body)) => {
                    u.dock.adopt(body);
                }
                Ok((409.0, body)) if !field(&body, "current").is_null() => {
                    u.dock.adopt(field(&body, "current").clone());
                    message = Some(translate(
                        "Otro dispositivo cambió la distribución; se muestra la versión actual.",
                        "Another device changed the layout; showing the current one.",
                    ));
                }
                other => {
                    u.dock.doc = before;
                    u.dock.revision = revision;
                    message = Some(match other {
                        Ok((_, body)) => {
                            let error = text(field(&body, "error"));
                            if error.is_empty() {
                                translate(
                                    "No se pudo guardar la distribución",
                                    "Could not save the layout",
                                )
                            } else {
                                error
                            }
                        }
                        Err(_) => translate(
                            "No se pudo guardar la distribución",
                            "Could not save the layout",
                        ),
                    });
                }
            }
        }
        if let Some(m) = message {
            let _ = invoke(&global("toast"), &[m.into(), true.into()]);
        }
        show(active_view(), false);
    });
}
fn resize(u: &Ui, group: &str, path: &str, ratio: f64) -> Option<Value> {
    workspace_layout::resize_split(
        u.dock.doc.as_ref()?,
        group,
        &path
            .split('.')
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>(),
        ratio,
    )
    .ok()
}
fn target(u: &Ui, x: f64, y: f64, source: &str) -> Option<Target> {
    let moved = if let Some(gid) = source.strip_prefix("group:") {
        u.dock
            .groups()
            .iter()
            .find(|g| field(g, "id").as_str() == Some(gid))
            .map(|g| tab_ids(field(g, "tree")))
            .unwrap_or_else(|| vec![String::new()])
    } else {
        vec![source.into()]
    };
    let strip = id("tabbar");
    let sr = rect(&strip);
    if x >= sr.left && x <= sr.right() && y >= sr.top && y <= sr.bottom() {
        let entries = all(&strip, "[data-tab-key^=\"group:\"]");
        let index = entries
            .iter()
            .position(|e| {
                let r = rect(e);
                x < r.left + r.width / 2.0
            })
            .unwrap_or(entries.len());
        let r = entries.get(index).map(rect);
        return Some(Target::Bar {
            index: index as i64,
            label: translate("Separar en la barra", "Separate into the strip"),
            rect: Rect {
                left: r.map(|r| r.left - 3.0).unwrap_or(sr.right() - 60.0),
                top: sr.top,
                width: if r.is_some() { 6.0 } else { 60.0 },
                height: sr.height,
            },
        });
    }
    let ar = rect(&id("term-area"));
    if x < ar.left || x > ar.right() || y < ar.top || y > ar.bottom() {
        return None;
    }
    if let Some(g) = u.dock.group(&active_term())
        && let Some(edge) = outer_edge(ar, x, y, 16.0)
        && !tab_ids(field(g, "tree")).iter().all(|t| moved.contains(t))
    {
        return Some(Target::Dock {
            target: format!("group:{}", text(field(g, "id"))),
            edge: edge.into(),
            rect: preview_rect(ar, edge),
            label: translate("Dividir todo el espacio", "Split the whole space"),
        });
    }
    let leaf = closest(
        &call(&doc(), "elementFromPoint", &[x.into(), y.into()]).unwrap_or(JsValue::NULL),
        ".ws-leaf",
    );
    let tab = data(&leaf, "tab");
    if leaf.is_null() || moved.contains(&tab) {
        return None;
    }
    let r = rect(&leaf);
    let edge = edge_for(r, x, y, 0.31)?;
    Some(Target::Dock {
        label: format!(
            "{}{}",
            translate("Colocar junto a ", "Place next to "),
            label(&u.dock, &tab)
        ),
        target: tab,
        edge: edge.into(),
        rect: preview_rect(r, edge),
    })
}
fn tray_at(g: &Gesture, x: f64, y: f64) -> Option<String> {
    if g.trays.is_null() || truthy(&get(&g.trays, "hidden")) {
        return None;
    }
    let children = get(&g.trays, "children");
    (0..number(&get(&children, "length")) as u32)
        .map(|i| get(&children, &i.to_string()))
        .find(|el| {
            let r = rect(el);
            x >= r.left - 8.0 && x <= r.right() + 8.0 && y >= r.top - 8.0 && y <= r.bottom() + 8.0
        })
        .map(|el| data(&el, "mark"))
}
fn paint(g: &Gesture) {
    if !g.trays.is_null() {
        let children = get(&g.trays, "children");
        for i in 0..number(&get(&children, "length")) as u32 {
            let el = get(&children, &i.to_string());
            classes(
                &el,
                "hl",
                matches!(&g.target,Some(Target::Mark(mark))if *mark==data(&el,"mark")),
            )
        }
    }
    let show = g.target.as_ref().filter(|t| !matches!(t, Target::Mark(_)));
    let _ = set(&g.preview, "hidden", &show.is_none().into());
    if let Some(t) = show {
        let (r, label, bar) = match t {
            Target::Bar { rect, label, .. } => (rect, label, true),
            Target::Dock { rect, label, .. } => (rect, label, false),
            Target::Mark(_) => return,
        };
        for (k, v) in [
            ("left", r.left),
            ("top", r.top),
            ("width", r.width),
            ("height", r.height),
        ] {
            style(&g.preview, k, &format!("{v}px"))
        }
        let _ = set(&g.preview, "textContent", &label.clone().into());
        classes(&g.preview, "bar", bar)
    }
}
fn move_ghost(g: &Gesture, x: f64, y: f64) {
    if !g.ghost.is_null() {
        style(&g.ghost, "left", &format!("{x}px"));
        style(&g.ghost, "top", &format!("{y}px"));
    }
}
fn raf(ui: Shared) {
    let next = ui.clone();
    let cb = event(move |_| {
        let Ok(mut u) = next.try_borrow_mut() else {
            return Ok(());
        };
        let Some(g) = u.gesture.as_ref() else {
            return Ok(());
        };
        if !g.active {
            return Ok(());
        }
        let (x, y, source) = (g.lx, g.ly, g.source.clone());
        let strip = id("tabbar");
        let sr = rect(&strip);
        if !strip.is_null() && y >= sr.top - 20.0 && y <= sr.bottom() + 20.0 {
            let dx = edge_scroll(x, sr.left, sr.right(), 56.0, 18.0);
            if dx != 0.0 {
                set(
                    &strip,
                    "scrollLeft",
                    &(number(&get(&strip, "scrollLeft")) + dx).into(),
                )?;
                let target = target(&u, x, y, &source);
                if let Some(g) = u.gesture.as_mut() {
                    g.target = target;
                    paint(g);
                }
            }
        }
        drop(u);
        raf(next.clone());
        Ok(())
    });
    let request = call(&js_sys::global(), "requestAnimationFrame", &[cb]).unwrap_or(JsValue::NULL);
    if let Ok(mut u) = ui.try_borrow_mut()
        && let Some(g) = u.gesture.as_mut()
    {
        g.raf = request;
    }
}
fn activate(ui: Shared) -> Result<(), JsValue> {
    let mut u = ui
        .try_borrow_mut()
        .map_err(|_| JsValue::from_str("dock busy"))?;
    let Some(g) = u.gesture.as_ref() else {
        return Ok(());
    };
    if g.active {
        return Ok(());
    }
    let source = g.source.clone();
    let names = if let Some(gid) = source.strip_prefix("group:") {
        u.dock
            .groups()
            .iter()
            .find(|g| field(g, "id").as_str() == Some(gid))
            .map(|g| {
                tab_ids(field(g, "tree"))
                    .iter()
                    .map(|t| label(&u.dock, t))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        vec![label(&u.dock, &source)]
    };
    let ghost = call(&doc(), "createElement", &["div".into()])?;
    set(&ghost, "className", &"ws-ghost".into())?;
    set(&ghost, "textContent", &names.join(" + ").into())?;
    let preview = call(&doc(), "createElement", &["div".into()])?;
    set(&preview, "className", &"ws-preview".into())?;
    set(&preview, "hidden", &true.into())?;
    call(
        &get(&doc(), "body"),
        "append",
        &[ghost.clone(), preview.clone()],
    )?;
    let trays = if !source.starts_with("group:")
        && source != "local"
        && truthy(&global("WorkMarks"))
    {
        let trays = call(&doc(), "createElement", &["div".into()])?;
        set(&trays, "className", &"ws-trays".into())?;
        set(&trays, "hidden", &true.into())?;
        let html=TRAYS.iter().map(|(mark,label,glyph)|format!("<div class=\"ws-tray ws-tray-{mark}\" data-mark=\"{mark}\"><b>{glyph}</b>{}</div>",translate(label,label))).collect::<String>();
        set(&trays, "innerHTML", &html.into())?;
        call(&get(&doc(), "body"), "append", std::slice::from_ref(&trays))?;
        trays
    } else {
        JsValue::NULL
    };
    if let Some(g) = u.gesture.as_mut() {
        g.active = true;
        g.ghost = ghost;
        g.preview = preview;
        g.trays = trays;
        g.lx = g.x;
        g.ly = g.y;
        move_ghost(g, g.x, g.y);
    }
    classes(&get(&doc(), "body"), "ws-dragging", true);
    drop(u);
    raf(ui);
    Ok(())
}
fn start(ui: Shared, e: &JsValue) -> Result<(), JsValue> {
    let mut u = ui
        .try_borrow_mut()
        .map_err(|_| JsValue::from_str("dock busy"))?;
    if number(&get(e, "button")) != 0.0 || u.dock.doc.is_none() {
        return Ok(());
    }
    let target = get(e, "target");
    let divider = closest(&target, "[data-ws-divider]");
    let handle = if divider.is_null() {
        closest(&target, "[data-ws-source]")
    } else {
        JsValue::NULL
    };
    if divider.is_null() && handle.is_null() {
        return Ok(());
    }
    if !divider.is_null()
        && truthy(
            &call(&divider, "hasAttribute", &["data-stacked".into()]).unwrap_or(JsValue::FALSE),
        )
    {
        return Ok(());
    }
    if !handle.is_null() && !closest(&target, ".tab-fav, .x").is_null() {
        return Ok(());
    }
    let path = (!divider.is_null()).then(|| data(&divider, "wsDivider"));
    let el = if divider.is_null() { handle } else { divider };
    let kind = get(e, "pointerType").as_string().unwrap_or_default();
    u.dock.gesturing = true;
    u.gesture = Some(Gesture {
        pointer: get(e, "pointerId"),
        el: el.clone(),
        source: data(&el, "wsSource"),
        path: path.clone(),
        x: number(&get(e, "clientX")),
        y: number(&get(e, "clientY")),
        kind: kind.clone(),
        active: path.is_some(),
        target: None,
        ghost: JsValue::NULL,
        preview: JsValue::NULL,
        trays: JsValue::NULL,
        timer: JsValue::NULL,
        raf: JsValue::NULL,
        lx: 0.0,
        ly: 0.0,
        changed: false,
        original: if path.is_some() {
            u.dock.doc.clone()
        } else {
            None
        },
    });
    if path.is_some() {
        call(e, "preventDefault", &[])?;
        let _ = call(&el, "setPointerCapture", &[get(e, "pointerId")]);
        classes(&get(&doc(), "body"), "ws-dragging", true);
        return Ok(());
    }
    if matches!(kind.as_str(), "touch" | "pen") {
        let next = ui.clone();
        let pointer = get(e, "pointerId");
        let cb = event(move |_| {
            let current = next
                .try_borrow()
                .ok()
                .and_then(|u| u.gesture.as_ref().map(|g| g.pointer.clone()));
            if current.as_ref() != Some(&pointer) {
                return Ok(());
            }
            let _ = call(&el, "setPointerCapture", std::slice::from_ref(&pointer));
            activate(next.clone())
        });
        let timer = later(cb, 180.0);
        if let Some(g) = u.gesture.as_mut() {
            g.timer = timer;
        }
    }
    Ok(())
}
fn motion(ui: Shared, e: &JsValue) -> Result<(), JsValue> {
    let mut u = ui
        .try_borrow_mut()
        .map_err(|_| JsValue::from_str("dock busy"))?;
    let Some(g) = u.gesture.as_ref() else {
        return Ok(());
    };
    if g.pointer != get(e, "pointerId") {
        return Ok(());
    }
    let (x, y) = (number(&get(e, "clientX")), number(&get(e, "clientY")));
    if let Some(path) = g.path.clone() {
        call(e, "preventDefault", &[])?;
        let el = query(&doc(), &format!(".ws-split[data-path=\"{path}\"]"));
        let Some(group) = u.dock.group(&active_term()) else {
            return Ok(());
        };
        if el.is_null() {
            return Ok(());
        }
        let r = rect(&el);
        let ratio = if data(&el, "axis") == "x" {
            (x - r.left) / r.width
        } else {
            (y - r.top) / r.height
        };
        if let Some(next) = resize(&u, &text(field(group, "id")), &path, ratio) {
            u.dock.doc = Some(next);
            if let Some(g) = u.gesture.as_mut() {
                g.changed = true;
            }
            layout(&u)?;
        }
        return Ok(());
    }
    if !g.active {
        if (x - g.x).hypot(y - g.y) <= 7.0 {
            return Ok(());
        }
        if matches!(g.kind.as_str(), "touch" | "pen") {
            cancel(g.timer.clone());
            u.gesture = None;
            u.dock.gesturing = false;
            return Ok(());
        }
        let _ = call(&g.el, "setPointerCapture", &[get(e, "pointerId")]);
        drop(u);
        activate(ui.clone())?;
        u = ui
            .try_borrow_mut()
            .map_err(|_| JsValue::from_str("dock busy"))?;
    }
    call(e, "preventDefault", &[])?;
    let target = {
        let Some(g) = u.gesture.as_ref() else {
            return Ok(());
        };
        if !g.trays.is_null() {
            let sr = rect(&id("tabbar"));
            set(&g.trays, "hidden", &(y < sr.bottom() + 30.0).into())?;
        }
        tray_at(g, x, y)
            .map(Target::Mark)
            .or_else(|| target(&u, x, y, &g.source))
    };
    if let Some(g) = u.gesture.as_mut() {
        move_ghost(g, x, y);
        g.lx = x;
        g.ly = y;
        g.target = target;
        paint(g);
    }
    Ok(())
}
fn finish(ui: Shared, pointer: JsValue, cancelled: bool) -> Result<(), JsValue> {
    let (g, document) = {
        let mut u = ui
            .try_borrow_mut()
            .map_err(|_| JsValue::from_str("dock busy"))?;
        if u.gesture.as_ref().is_none_or(|g| g.pointer != pointer) {
            return Ok(());
        }
        let Some(g) = u.gesture.take() else {
            return Ok(());
        };
        cancel(g.timer.clone());
        let _ = call(
            &g.el,
            "releasePointerCapture",
            std::slice::from_ref(&pointer),
        );
        for el in [&g.ghost, &g.preview, &g.trays] {
            if !el.is_null() {
                let _ = call(el, "remove", &[]);
            }
        }
        if truthy(&g.raf) {
            let _ = call(
                &js_sys::global(),
                "cancelAnimationFrame",
                std::slice::from_ref(&g.raf),
            );
        }
        classes(&get(&doc(), "body"), "ws-dragging", false);
        u.dock.gesturing = false;
        if !g.active {
            return Ok(());
        }
        u.suppress = js_sys::Date::now() + 350.0;
        if g.path.is_some() && cancelled {
            u.dock.doc = g.original.clone().or(u.dock.doc.clone());
            layout(&u)?;
            return Ok(());
        }
        (g, u.dock.doc.clone())
    };
    if g.path.is_some() {
        if g.changed
            && let Some(document) = document
        {
            commit(ui, document, None);
        }
        return Ok(());
    }
    if cancelled {
        return Ok(());
    }
    let Some(t) = g.target else { return Ok(()) };
    if let Target::Mark(mark) = t {
        let result = call(
            &global("WorkMarks"),
            "setMark",
            &["session".into(), g.source.into(), mark.into()],
        );
        wasm_bindgen_futures::spawn_local(async move {
            if wait(result).await.is_ok() {
                let _ = call(&global("WorkMarks"), "decorate", &[]);
            }
        });
        return Ok(());
    }
    let Some(document) = document else {
        return Ok(());
    };
    let next = match t {
        Target::Bar { index, .. } => workspace_layout::detach_tab(&document, &g.source, index),
        Target::Dock { target, edge, .. } => {
            workspace_layout::move_tab(&document, &g.source, &target, &edge)
        }
        Target::Mark(_) => return Ok(()),
    };
    if let Ok(next) = next {
        let focus = (!g.source.starts_with("group:")).then_some(g.source);
        commit(ui, next, focus)
    }
    Ok(())
}
fn key_resize(ui: Shared, e: &JsValue) -> Result<(), JsValue> {
    let divider = closest(&get(e, "target"), "[data-ws-divider]");
    let key = get(e, "key").as_string().unwrap_or_default();
    if divider.is_null()
        || truthy(
            &call(&divider, "hasAttribute", &["data-stacked".into()]).unwrap_or(JsValue::FALSE),
        )
        || !matches!(
            key.as_str(),
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown"
        )
    {
        return Ok(());
    }
    let mut u = ui
        .try_borrow_mut()
        .map_err(|_| JsValue::from_str("dock busy"))?;
    let Some(group) = u.dock.group(&active_term()) else {
        return Ok(());
    };
    call(e, "preventDefault", &[])?;
    call(e, "stopPropagation", &[])?;
    let path = data(&divider, "wsDivider");
    let mut node = field(group, "tree");
    for step in path.split('.').filter(|p| !p.is_empty()) {
        node = field(node, step);
    }
    let ratio = field(node, "ratio").as_f64().unwrap_or(0.5)
        + if matches!(key.as_str(), "ArrowLeft" | "ArrowUp") {
            -0.05
        } else {
            0.05
        };
    let Some(next) = resize(&u, &text(field(group, "id")), &path, ratio) else {
        return Ok(());
    };
    u.dock.doc = Some(next);
    layout(&u)?;
    cancel(u.key_timer.clone());
    let next = ui.clone();
    u.key_timer = later(
        event(move |_| {
            let document = next.try_borrow().ok().and_then(|u| u.dock.doc.clone());
            if let Some(document) = document {
                commit(next.clone(), document, None)
            }
            Ok(())
        }),
        400.0,
    );
    Ok(())
}
pub fn mount() -> Result<(), JsValue> {
    let api = object();
    method(&api, "edgeScroll", |a| {
        let r = to_json(&a.get(1));
        let left = field(&r, "left").as_f64().unwrap_or(0.0);
        let right = field(&r, "right").as_f64().unwrap_or(0.0);
        Ok(edge_scroll(
            number(&a.get(0)),
            left,
            right,
            a.get(2).as_f64().unwrap_or(56.0),
            a.get(3).as_f64().unwrap_or(18.0),
        )
        .into())
    })?;
    for (name, outer) in [("edgeFor", false), ("outerEdge", true)] {
        method(&api, name, move |a| {
            let r = from_rect(&to_json(&a.get(0)));
            let result = if outer {
                outer_edge(
                    r,
                    number(&a.get(1)),
                    number(&a.get(2)),
                    a.get(3).as_f64().unwrap_or(16.0),
                )
            } else {
                edge_for(
                    r,
                    number(&a.get(1)),
                    number(&a.get(2)),
                    a.get(3).as_f64().unwrap_or(0.31),
                )
            };
            Ok(result.map(JsValue::from_str).unwrap_or(JsValue::NULL))
        })?;
    }
    set(
        &api,
        "TRAYS",
        &from_json(&json!(
            TRAYS
                .iter()
                .map(|(mark, label, glyph)| json!({"mark":mark,"label":label,"glyph":glyph}))
                .collect::<Vec<_>>()
        ))?,
    )?;
    method(&api, "measure", |a| {
        from_json(&measure(&to_json(&a.get(0)), number(&a.get(1)), a.get(2).as_f64()).json())
    })?;
    method(&api, "previewRect", |a| {
        from_json(&rect_json(preview_rect(
            from_rect(&to_json(&a.get(0))),
            &string(&a.get(1)),
        )))
    })?;
    let ui = shared()?;
    let u = ui.clone();
    method(&api, "adopt", move |a| {
        let mut u = u
            .try_borrow_mut()
            .map_err(|_| JsValue::from_str("dock busy"))?;
        Ok(u.dock.adopt(to_json(&a.get(0))).into())
    })?;
    let u = ui.clone();
    method(&api, "stripEntries", move |_| {
        let u = u.try_borrow().map_err(|_| JsValue::from_str("dock busy"))?;
        from_json(&entries(&u.dock))
    })?;
    method(&api, "isSelected", |a| {
        let entry = to_json(&a.get(0));
        let active = active_term();
        Ok(rows(field(&entry, "tabs"))
            .iter()
            .any(|t| t.as_str() == Some(&active))
            .into())
    })?;
    let u = ui.clone();
    method(&api, "visibleTabs", move |a| {
        let u = u.try_borrow().map_err(|_| JsValue::from_str("dock busy"))?;
        from_json(&json!(
            u.dock
                .visible_tabs(&a.get(0).as_string().unwrap_or_default())
        ))
    })?;
    let u = ui.clone();
    method(&api, "render", move |a| {
        let mut u = u
            .try_borrow_mut()
            .map_err(|_| JsValue::from_str("dock busy"))?;
        render(&mut u, &a.get(0).as_string().unwrap_or_default())?;
        Ok(JsValue::UNDEFINED)
    })?;
    let u = ui.clone();
    method(&api, "layout", move |a| {
        let u = u.try_borrow().map_err(|_| JsValue::from_str("dock busy"))?;
        let group = if truthy(&a.get(2)) {
            to_json(&a.get(2))
        } else {
            u.dock.group(&active_term()).cloned().unwrap_or(Value::Null)
        };
        if !group.is_null() {
            let area = if truthy(&a.get(0)) {
                a.get(0)
            } else {
                id("term-area")
            };
            let tree = if truthy(&a.get(1)) {
                a.get(1)
            } else {
                tree_el()?
            };
            layout_at(&u.dock, &group, area, tree)?;
        }
        Ok(JsValue::UNDEFINED)
    })?;
    let u = ui.clone();
    getter(&api, "doc", move || {
        u.try_borrow()
            .ok()
            .and_then(|u| u.dock.doc.as_ref().and_then(|d| from_json(d).ok()))
            .unwrap_or(JsValue::NULL)
    })?;
    getter(&api, "revision", move || {
        ui.try_borrow()
            .map(|u| (u.dock.revision as f64).into())
            .unwrap_or(JsValue::NULL)
    })?;
    global_set("WorkspaceDock", &api)
}
fn schedule_layout(ui: Shared) {
    let cb = event(move |_| {
        let u = ui
            .try_borrow()
            .map_err(|_| JsValue::from_str("dock busy"))?;
        layout(&u)
    });
    let _ = call(&js_sys::global(), "requestAnimationFrame", &[cb]);
}
pub fn attach() -> Result<(), JsValue> {
    let ui = shared()?;
    let u = ui.clone();
    listen(
        &doc(),
        "pointerdown",
        event(move |e| {
            let target = get(&e, "target");
            if (!closest(&target, "#tabbar").is_null() || !closest(&target, "#term-area").is_null())
                && !u.try_borrow().map_or(true, |u| {
                    u.gesture.as_ref().is_some_and(|g| g.path.is_some())
                })
            {
                start(u.clone(), &e)?;
            }
            Ok(())
        }),
    );
    let u = ui.clone();
    let options = object();
    set(&options, "passive", &false.into())?;
    call(
        &doc(),
        "addEventListener",
        &[
            "pointermove".into(),
            event(move |e| motion(u.clone(), &e)),
            options,
        ],
    )?;
    for (t, cancelled) in [("pointerup", false), ("pointercancel", true)] {
        let u = ui.clone();
        listen(
            &doc(),
            t,
            event(move |e| finish(u.clone(), get(&e, "pointerId"), cancelled)),
        )
    }
    let u = ui.clone();
    call(
        &doc(),
        "addEventListener",
        &[
            "click".into(),
            event(move |e| {
                if u.try_borrow()
                    .is_ok_and(|u| js_sys::Date::now() < u.suppress)
                {
                    call(&e, "preventDefault", &[])?;
                    call(&e, "stopImmediatePropagation", &[])?;
                }
                Ok(())
            }),
            true.into(),
        ],
    )?;
    let u = ui.clone();
    call(
        &doc(),
        "addEventListener",
        &[
            "keydown".into(),
            event(move |e| {
                if get(&e, "key").as_string().as_deref() == Some("Escape") {
                    let pointer = u
                        .try_borrow()
                        .ok()
                        .and_then(|u| u.gesture.as_ref().map(|g| g.pointer.clone()));
                    if let Some(pointer) = pointer {
                        finish(u.clone(), pointer, true)?;
                        call(&e, "stopPropagation", &[])?;
                        return Ok(());
                    }
                }
                key_resize(u.clone(), &e)
            }),
            true.into(),
        ],
    )?;
    let area = id("term-area");
    if !area.is_null() && truthy(&global("ResizeObserver")) {
        let u = ui.clone();
        let observer = js_sys::Reflect::construct(
            &global("ResizeObserver").into(),
            &[event(move |_| {
                schedule_layout(u.clone());
                Ok(())
            })]
            .into_iter()
            .collect::<js_sys::Array>(),
        )?;
        call(&observer, "observe", &[area])?;
    }
    listen(
        &js_sys::global(),
        "resize",
        event(move |_| {
            schedule_layout(ui.clone());
            Ok(())
        }),
    );
    Ok(())
}
