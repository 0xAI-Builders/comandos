//! Notice templates and grouping. Text remains available in every full-text disclosure.
use serde_json::{Value, json};
use std::sync::OnceLock;
pub const CATEGORIES: [&str; 7] = [
    "attention",
    "error",
    "focus",
    "usage",
    "done",
    "news",
    "info",
];
pub const FILTERS: [(&str, &str); 7] = [
    ("all", "Todos"),
    ("unread", "Nuevos"),
    ("pending", "Pendientes"),
    ("error", "Errores"),
    ("done", "Terminados"),
    ("usage", "Uso"),
    ("news", "Noticias"),
];
pub const TYPES: [(&str, &str, &str); 7] = [
    ("attention", "Permisos y preguntas", "permission"),
    ("error", "Errores", "error"),
    ("focus", "Pomodoro", "success"),
    ("done", "Turnos terminados", "complete"),
    ("usage", "Uso y límites", "warning"),
    ("news", "Noticias y anuncios", "attention"),
    ("info", "Otros", "attention"),
];
pub fn s(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().unwrap_or(0.).to_string(),
        _ => v.to_string(),
    }
}
pub fn field(v: &Value, k: &str) -> String {
    s(at(v, k))
}
fn esc(v: &Value) -> String {
    crate::escape::text(&s(v))
}
fn e(s: &str) -> String {
    crate::escape::text(s)
}
pub fn n(v: &Value, k: &str) -> f64 {
    at(v, k).as_f64().unwrap_or(0.)
}
pub fn truth(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|n| n != 0.).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
pub fn array(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}
fn pending(v: &Value, id: &Value) -> bool {
    array(at(v, "pending")).contains(id)
}
fn tpl(id: usize, vals: &[String]) -> String {
    static PARTS: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    let p = PARTS.get_or_init(|| {
        serde_json::from_str(include_str!("notifications_templates.json")).unwrap_or_default()
    });
    let mut out = String::new();
    if let Some(parts) = p.get(id) {
        for (i, s) in parts.iter().enumerate() {
            out.push_str(s);
            if let Some(v) = vals.get(i) {
                out.push_str(v)
            }
        }
    }
    out
}
macro_rules! t{($id:expr $(,$v:expr)* $(,)?)=>{tpl($id,&[$(($v).to_string()),*])};}
pub fn icon(name: &str) -> String {
    static SHAPES: OnceLock<Value> = OnceLock::new();
    let shapes = SHAPES.get_or_init(|| {
        serde_json::from_str(include_str!("notifications_shapes.json")).unwrap_or(Value::Null)
    });
    t!(
        0,
        if at(shapes, name).is_null() {
            field(shapes, "bell")
        } else {
            field(shapes, name)
        }
    )
}
pub fn is_news(v: &Value) -> bool {
    at(v, "category") == "news"
        || at(v, "kind") == "news_edition"
        || at(v, "kind") == "announcement"
}
pub fn category(v: &Value) -> String {
    let cat = field(v, "category");
    if CATEGORIES.contains(&cat.as_str()) {
        cat
    } else {
        "info".into()
    }
}
pub fn group_key(v: &Value) -> String {
    if is_news(v) {
        "__news__".into()
    } else if truth(at(v, "projectKey")) {
        field(v, "projectKey")
    } else if truth(at(v, "project")) {
        field(v, "project")
    } else {
        "__general__".into()
    }
}
pub fn matches(v: &Value, filter: &str, ids: &[Value]) -> bool {
    match filter {
        "unread" => !truth(at(v, "read")),
        "pending" => ids.contains(at(v, "eventId")),
        "error" => at(v, "category") == "error",
        "done" => at(v, "category") == "done" || at(v, "category") == "focus",
        "usage" => at(v, "category") == "usage",
        "news" => is_news(v),
        _ => true,
    }
}
pub fn groups(list: Vec<Value>, filter: &str, ids: &[Value]) -> Vec<Value> {
    let mut list = list;
    list.sort_by(|a, b| n(b, "sequence").total_cmp(&n(a, "sequence")));
    let mut groups: Vec<Value> = Vec::new();
    for v in list.into_iter().filter(|v| matches(v, filter, ids)) {
        let key = group_key(&v);
        let idx = groups
            .iter()
            .position(|g| *at(g, "key") == key)
            .unwrap_or(groups.len());
        if idx == groups.len() {
            let project = if truth(at(&v, "project")) {
                at(&v, "project").clone()
            } else {
                at(&v, "projectKey").clone()
            };
            let label = match key.as_str() {
                "__news__" => "Noticias".into(),
                "__general__" => "General".into(),
                _ => {
                    if truth(&project) {
                        s(&project)
                    } else {
                        key.clone()
                    }
                }
            };
            groups.push(json!({"key":key,"label":label,"project":if key=="__news__"||key=="__general__"{Value::Null}else{project},"notices":[],"unread":0,"pendingCount":0,"latest":0}));
        }
        if let Some(g) = groups.get_mut(idx) {
            if !truth(at(&v, "read")) {
                let count = n(g, "unread") as usize + 1;
                *slot(g, &"unread") = json!(count);
            }
            if ids.contains(at(&v, "eventId")) {
                let count = n(g, "pendingCount") as usize + 1;
                *slot(g, &"pendingCount") = json!(count);
            }
            let latest = n(g, "latest").max(n(&v, "sequence"));
            *slot(g, &"latest") = json!(latest);
            if let Some(rows) = slot(g, &"notices").as_array_mut() {
                rows.push(v);
            }
        }
    }
    let rank = |g: &Value| match field(g, "key").as_str() {
        "__news__" => 2,
        "__general__" => 1,
        _ => 0,
    };
    groups.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| n(b, "latest").total_cmp(&n(a, "latest")))
    });
    groups
}
pub fn defaults() -> Value {
    json!({"modes":{"attention":"sound","error":"sound","focus":"sound","done":"visual","news":"visual","usage":"visual","info":"visual"},"volume":0.6,"muted":false,"floatMs":6000,"burstMs":10000})
}
fn js_number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.,
        Some(Value::Bool(b)) => {
            if *b {
                1.
            } else {
                0.
            }
        }
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => {
            if s.trim().is_empty() {
                0.
            } else {
                s.trim().parse().unwrap_or(f64::NAN)
            }
        }
        _ => f64::NAN,
    }
}
pub fn normalize(p: &Value) -> Value {
    let mut out = defaults();
    let mut modes = at(&out, "modes").clone();
    if let Some(m) = at(p, "modes").as_object() {
        for (k, v) in m {
            if v == "sound" || v == "visual" {
                *slot(&mut modes, k) = v.clone();
            }
        }
    }
    if let Some(src) = p.as_object() {
        for (k, v) in src {
            *slot(&mut out, k) = v.clone();
        }
    }
    *slot(&mut out, &"modes") = modes;
    let volume = js_number(p.get("volume"));
    *slot(&mut out, &"volume") = json!(if volume.is_finite() {
        volume.clamp(0., 1.)
    } else {
        0.6
    });
    *slot(&mut out, &"muted") = json!(truth(at(p, "muted")));
    out
}
pub fn clamp_height(px: f64, viewport: f64) -> f64 {
    let viewport = if viewport == 0. || viewport.is_nan() {
        800.
    } else {
        viewport
    };
    let px = if px == 0. || px.is_nan() { 260. } else { px };
    (px.min((viewport - 140.).max(96.)).max(96.) + 0.5).floor()
}
pub fn rel_time(ms: f64, now: f64) -> String {
    if ms == 0. || now == 0. {
        return String::new();
    }
    let seconds = ((now - ms) / 1000. + 0.5).floor().max(0.);
    if seconds < 45. {
        "ahora".into()
    } else if seconds < 3600. {
        format!("hace {} min", (seconds / 60. + 0.5).floor().max(1.))
    } else if seconds < 86400. {
        format!("hace {} h", (seconds / 3600. + 0.5).floor())
    } else {
        String::new()
    }
}
pub fn describe(v: &Value) -> String {
    if let Some(s) = v.get("_origin") {
        return self::s(s);
    }
    if is_news(v) {
        return "Noticias".into();
    }
    let mut parts = vec![if truth(at(v, "project")) {
        field(v, "project")
    } else if truth(at(v, "projectKey")) {
        field(v, "projectKey")
    } else {
        "General".into()
    }];
    if truth(at(v, "sessionKey")) && at(v, "sessionKey") != at(v, "project") {
        parts.push(field(v, "sessionKey"));
    }
    if truth(at(v, "paneId")) {
        parts.push(format!("pane {}", field(v, "paneId")));
    }
    parts.join(" · ")
}
fn row(v: &Value, view: &Value) -> String {
    let pend = pending(view, at(v, "eventId"));
    let read = truth(at(v, "read"));
    let origin = e(&describe(v));
    let id = esc(at(v, "eventId"));
    let title = esc(at(v, "title"));
    let cat = category(v);
    let time = if v.get("_time").is_some() {
        field(v, "_time")
    } else {
        rel_time(n(v, "occurredAtMs"), n(view, "now"))
    };
    t!(
        1,
        cat,
        if read { " read" } else { "" },
        if pend { " pending" } else { "" },
        id
    ) + &t!(2, id, id, origin, title)
        + &t!(3, icon(&cat), title)
        + &t!(
            4,
            origin,
            if pend {
                " · <em>Pendiente · se responde en su terminal</em>"
            } else {
                ""
            }
        )
        + &t!(
            5,
            if truth(at(v, "excerpt")) {
                t!(6, esc(at(v, "excerpt")))
            } else {
                String::new()
            }
        )
        + &t!(7, e(&time))
        + &t!(
            8,
            id,
            id,
            if read {
                "disabled aria-label=\"Leído\"".into()
            } else {
                t!(9, title)
            },
            if read {
                icon("read")
            } else {
                "<i class=\"nt-dot\"></i>".into()
            }
        )
        + "</article>"
        + &t!(
            10,
            id,
            id,
            if truth(at(v, "excerpt")) {
                esc(at(v, "excerpt"))
            } else {
                title
            }
        )
}
fn gone(v: &Value) -> String {
    let Some(notice) = array(at(v, "notices"))
        .into_iter()
        .find(|n| at(n, "eventId") == at(v, "unavailable"))
    else {
        return String::new();
    };
    t!(
        11,
        "El panel ya no existe",
        e(&describe(&notice)),
        esc(at(&notice, "title"))
    ) + &t!(
        12,
        if truth(at(&notice, "excerpt")) {
            t!(13, esc(at(&notice, "excerpt")))
        } else {
            String::new()
        }
    ) + "<button type=\"button\" data-nt-act=\"gone-close\" data-nt-focus=\"gone-close\">Volver a los avisos</button></div>"
}
fn empty(filter: &str) -> &str {
    match filter {
        "unread" => "Ya viste todos los avisos.",
        "pending" => "Ninguna solicitud pendiente.",
        "error" => "Sin errores.",
        "done" => "Sin turnos terminados.",
        "usage" => "Sin alertas de uso.",
        "news" => "Sin noticias.",
        _ => "Aún no hay avisos. Cuando llegue uno, podrás volver a su terminal desde aquí.",
    }
}
pub fn render_strip(v: &Value) -> String {
    let notices = array(at(v, "notices"));
    let ids = array(at(v, "pending"));
    let unread = notices.iter().filter(|n| !truth(at(n, "read"))).count();
    let pending_n = notices
        .iter()
        .filter(|n| ids.contains(at(n, "eventId")))
        .count();
    let local = notices
        .iter()
        .filter(|n| !truth(at(n, "read")) || ids.contains(at(n, "eventId")))
        .count();
    let attend = at(v, "badge")
        .as_f64()
        .filter(|n| n.is_finite())
        .unwrap_or(local as f64);
    let collapsed = truth(at(v, "collapsed"));
    let loaded = v.get("loaded").map(truth).unwrap_or(true);
    let filter = if at(v, "filter").is_null() {
        "all".into()
    } else {
        field(v, "filter")
    };
    let mut body = String::new();
    if !collapsed {
        let groups = groups(notices, &filter, &ids);
        let status = if truth(at(v, "error")) {
            "<div class=\"nt-status\" role=\"status\">No pudimos actualizar los avisos. Conservamos los anteriores.</div>"
        } else if !loaded {
            "<div class=\"nt-status\" role=\"status\">Cargando avisos…</div>"
        } else {
            ""
        };
        let list = if groups.is_empty() {
            if loaded {
                t!(22, icon("bell"), empty(&filter))
            } else {
                String::new()
            }
        } else {
            let mut html = String::new();
            for g in groups {
                let id = esc(at(&g, "key"));
                let label = esc(at(&g, "label"));
                let count = array(at(&g, "notices")).len();
                let pn = n(&g, "pendingCount") as usize;
                html += &t!(16, id, label);
                html += &t!(
                    17,
                    label,
                    count,
                    if count == 1 { "aviso" } else { "avisos" },
                    if pn > 0 {
                        t!(18, pn, if pn == 1 { "" } else { "s" })
                    } else {
                        String::new()
                    }
                );
                html += &t!(
                    19,
                    if n(&g, "unread") > 0. {
                        t!(20, id, id, label, icon("read"))
                    } else {
                        String::new()
                    }
                );
                html += &t!(
                    21,
                    array(at(&g, "notices"))
                        .iter()
                        .map(|r| row(r, v))
                        .collect::<String>()
                );
            }
            t!(15, html)
        };
        body = t!(
            23,
            status,
            if truth(at(v, "unavailable")) {
                gone(v)
            } else {
                String::new()
            },
            list
        );
        if pending_n > 0 {
            body += "<footer class=\"nt-foot\"><span>Leer no resuelve permisos: se responden en su terminal.</span></footer>";
        }
        body += "</div>";
    }
    let filters = FILTERS
        .iter()
        .map(|(id, label)| {
            t!(
                14,
                id,
                id,
                *id == filter,
                if *id == filter { "on" } else { "" },
                label
            )
        })
        .collect::<String>();
    let mut out = t!(24, if collapsed { " collapsed" } else { "" });
    if !collapsed {
        out += "<div class=\"nt-grip\" role=\"separator\" aria-orientation=\"horizontal\" tabindex=\"0\" data-nt-focus=\"grip\" aria-label=\"Altura de los avisos: arrastra, usa ↑/↓ o doble clic para maximizar\" title=\"Arrastra para cambiar la altura · doble clic: maximizar\"></div>";
    }
    out += "<header class=\"nt-head\">";
    out += &t!(
        25,
        !collapsed,
        icon("bell"),
        if attend != 0. {
            t!(26, attend)
        } else {
            String::new()
        },
        icon("chevron")
    );
    out += &t!(
        27,
        unread,
        if unread == 1 { "" } else { "s" },
        pending_n,
        if pending_n == 1 { "" } else { "s" }
    );
    if !collapsed {
        out += &t!(28, filters)
    }
    out += &t!(
        29,
        if unread > 0 || attend > pending_n as f64 {
            ""
        } else {
            "disabled"
        }
    );
    if !collapsed {
        let max = truth(at(v, "maximized"));
        let label = if max { "Restaurar altura" } else { "Maximizar" };
        out += &t!(30, label, label, icon(if max { "shrink" } else { "grow" }));
        out += &t!(31, icon("close"));
    }
    out + "</header>" + &body + "</section>"
}
pub fn float_members(v: &Value) -> Vec<Value> {
    let notices = array(at(v, "notices"));
    array(at(at(v, "float"), "eventIds"))
        .into_iter()
        .filter_map(|id| notices.iter().find(|n| *at(n, "eventId") == id).cloned())
        .collect()
}
pub fn float_summary(members: &[Value]) -> String {
    let Some(last) = members.last() else {
        return String::new();
    };
    if members.len() == 1 {
        return field(last, "title");
    }
    let plural = match field(last, "kind").as_str() {
        "turn_completed" => "turnos terminados",
        "turn_failed" => "turnos con error",
        "turn_cancelled" => "turnos cancelados",
        "permission_requested" => "permisos pedidos",
        "input_requested" => "preguntas",
        "focus_completed" => "bloques de foco terminados",
        "news_edition" => "ediciones",
        "announcement" => "anuncios",
        "usage_alert" => "alertas de uso",
        _ => "avisos",
    };
    let where_ = if is_news(last) {
        "Noticias".into()
    } else if truth(at(last, "project")) {
        field(last, "project")
    } else if truth(at(last, "projectKey")) {
        field(last, "projectKey")
    } else {
        "General".into()
    };
    t!(32, members.len(), plural, where_)
}
pub fn render_float(v: &Value) -> String {
    let members = float_members(v);
    let Some(last) = members.last() else {
        return String::new();
    };
    let cat = members
        .iter()
        .min_by_key(|n| {
            CATEGORIES
                .iter()
                .position(|c| *c == category(n))
                .unwrap_or(6)
        })
        .map(category)
        .unwrap_or("info".into());
    let origin = e(&describe(last));
    let title = e(&float_summary(&members));
    let gone = members
        .iter()
        .any(|n| at(n, "eventId") == at(v, "unavailable"));
    t!(
        33,
        cat,
        if truth(at(at(v, "float"), "persistent")) {
            " persistent"
        } else {
            ""
        }
    ) + &t!(34, icon(&cat))
        + &t!(35, origin, title)
        + &t!(36, origin, title)
        + &if gone {
            t!(37, "El panel ya no existe")
        } else {
            t!(38, icon("arrow"))
        }
        + "</button>"
        + &t!(39, icon("close"))
        + "</aside>"
}
pub fn render_settings(v: &Value) -> String {
    let p = normalize(at(v, "prefs"));
    let rows = TYPES
        .iter()
        .map(|(cat, label, cue)| {
            t!(40, icon(cat), e(label))
                + &t!(
                    41,
                    cat,
                    e(label),
                    if at(at(&p, "modes"), *cat) == "visual" {
                        " selected"
                    } else {
                        ""
                    }
                )
                + &t!(
                    42,
                    if at(at(&p, "modes"), *cat) == "sound" {
                        " selected"
                    } else {
                        ""
                    }
                )
                + &t!(43, cue, e(label))
        })
        .collect::<String>();
    let sound = truth(at(v, "localSound"));
    let available = v.get("localAvailable").map(truth).unwrap_or(true);
    "<div class=\"nt-settings\"><label>Avisos de proyectos: sonido por tipo</label>".to_string()
        + &t!(
            44,
            sound,
            if available { "" } else { " disabled" },
            if sound {
                "Sonido activo en este navegador"
            } else {
                "Activar sonido en este navegador"
            }
        )
        + &t!(
            45,
            if truth(at(&p, "muted")) {
                " checked"
            } else {
                ""
            }
        )
        + &t!(46, (n(&p, "volume") * 100. + 0.5).floor())
        + &t!(47, rows)
        + "<div class=\"desc\">Suena una sola vez, en el último equipo que usaste y que tenga el sonido activo. Cargar el historial nunca suena.</div></div>"
}

pub fn at<K: serde_json::value::Index>(v: &Value, k: K) -> &Value {
    v.get(k).unwrap_or(&Value::Null)
}
fn slot<'a, K: AsRef<str>>(v: &'a mut Value, k: &K) -> &'a mut Value {
    if !v.is_object() {
        *v = json!({});
    }
    match v {
        Value::Object(m) => m.entry(k.as_ref()).or_insert(Value::Null),
        other => other,
    }
}

pub fn put(v: &mut Value, key: &str, value: Value) {
    *slot(v, &key) = value;
}
