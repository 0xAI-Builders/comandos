//! One shelf and one explicit pane. All editing uses the last observed scope.
use comandos_web_view::escape::text as esc;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
pub const HARNESSES: &[(&str, &str)] = &[
    ("claude", "Claude"),
    ("codex", "Codex"),
    ("grok", "Grok"),
    ("opencode", "OpenCode"),
    ("agy", "Antigravity"),
];
pub fn field<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or(&Value::Null)
}
pub fn text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        _ => v.to_string(),
    }
}
pub fn yes(v: &Value) -> bool {
    v.as_bool().unwrap_or(false)
}
pub fn rows(v: &Value) -> Vec<&Value> {
    v.as_array().map(|a| a.iter().collect()).unwrap_or_default()
}
pub fn active(s: &Value) -> bool {
    let op = field(s, "operation");
    !op.is_null()
        && !matches!(
            field(op, "state").as_str(),
            Some("confirmed" | "failed" | "rolled_back")
        )
}
pub fn valid_target(session: &str, pane: &str) -> bool {
    !session.is_empty()
        && pane
            .strip_prefix('%')
            .is_some_and(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
}
pub fn shelf_height(px: f64, heights: &[f64]) -> f64 {
    let min = heights
        .iter()
        .copied()
        .filter(|h| *h > 0.0)
        .reduce(f64::min)
        .unwrap_or(300.0);
    px.clamp(180.0, (min - 120.0).max(180.0)).round()
}
type Group<'a> = (String, Vec<(&'a str, &'a Value, bool)>);
#[cfg(not(target_arch = "wasm32"))]
fn locale_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.cmp(b)
}
#[cfg(target_arch = "wasm32")]
fn locale_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    js_sys::JsString::from(a)
        .locale_compare(b, &js_sys::Array::new(), &js_sys::Object::new())
        .cmp(&0)
}
pub struct Shelf {
    pub target: Value,
    pub state: Option<Value>,
    pub filter: String,
    pub query: String,
    pub sending: bool,
    pub stale: bool,
    pub error: String,
    pub message: String,
    pub generation: u64,
    pub template_name: String,
    pub closed_groups: BTreeSet<String>,
    pub enter: bool,
    signature: String,
}
impl Shelf {
    pub fn new(target: Value) -> Self {
        Self {
            target,
            state: None,
            filter: "all".into(),
            query: String::new(),
            sending: false,
            stale: false,
            error: String::new(),
            message: String::new(),
            generation: 0,
            template_name: String::new(),
            closed_groups: BTreeSet::new(),
            enter: true,
            signature: String::new(),
        }
    }
    pub fn locked(&self) -> bool {
        self.sending || self.stale || self.state.as_ref().is_some_and(active)
    }
    pub fn guards(&self) -> Value {
        let mut body = self.target.clone();
        if let Some(s) = &self.state
            && let Some(m) = body.as_object_mut()
        {
            m.insert("expectedIdentity".into(), field(s, "identity").clone());
            m.insert(
                "expectedConversationId".into(),
                json!(text(field(s, "conversationId"))),
            );
            m.insert("revision".into(), field(s, "revision").clone());
        }
        body
    }
    pub fn receive(&mut self, generation: u64, next: Value) -> bool {
        if generation != self.generation {
            return false;
        }
        if let Some(s) = &self.state
            && (field(s, "identity") != field(&next, "identity")
                || field(s, "conversationId") != field(&next, "conversationId"))
            && !active(s)
        {
            self.stale = true;
            self.error = "El panel cambió de conversación. Actualiza antes de editar.".into();
            return true;
        }
        let signature = next.to_string();
        let same = self.state.is_some()
            && !self.stale
            && self.error.is_empty()
            && self.message.is_empty()
            && signature == self.signature;
        self.state = Some(next);
        self.signature = signature;
        self.stale = false;
        self.error.clear();
        !same
    }
    pub fn items(&self) -> Vec<(&str, &Value, bool)> {
        let Some(s) = &self.state else { return vec![] };
        ["mcps", "skills"]
            .into_iter()
            .flat_map(|kind| {
                rows(field(field(s, "inventory"), kind))
                    .into_iter()
                    .map(move |row| {
                        (
                            kind,
                            row,
                            yes(field(
                                field(field(s, "desired"), kind),
                                &text(field(row, "id")),
                            )),
                        )
                    })
            })
            .collect()
    }
    pub fn candidates(&self, group: Option<&str>, visible: bool) -> Vec<(&str, &Value, bool)> {
        let Some(s) = &self.state else { return vec![] };
        self.items()
            .into_iter()
            .filter(|(k, r, _)| {
                yes(field(r, "toggleable"))
                    && field(field(field(s, "desired"), k), &text(field(r, "id"))).is_boolean()
                    && (!visible
                        || ((self.filter == "all" || self.filter == *k)
                            && text(field(r, "name"))
                                .to_lowercase()
                                .contains(&self.query.to_lowercase())))
                    && group.is_none_or(|g| {
                        text(field(field(r, "origin"), "id")) == g
                            || (g == "unknown" && field(r, "origin").is_null())
                    })
            })
            .collect()
    }
    pub fn batch(&self, on: bool, group: Option<&str>, visible: bool) -> Option<Value> {
        if self.locked() {
            return None;
        }
        let s = self.state.as_ref()?;
        let mut desired = field(s, "desired").clone();
        let mut changed = false;
        for (k, r, current) in self.candidates(group, visible) {
            if current != on
                && let Some(m) = desired.get_mut(k).and_then(Value::as_object_mut)
            {
                m.insert(text(field(r, "id")), json!(on));
                changed = true;
            }
        }
        changed.then_some(desired)
    }
    pub fn toggle(&self, kind: &str, id: &str, on: bool) -> Option<Value> {
        if self.locked() {
            return None;
        }
        let s = self.state.as_ref()?;
        if !self
            .candidates(None, false)
            .iter()
            .any(|(k, r, _)| *k == kind && text(field(r, "id")) == id)
        {
            return None;
        }
        let mut d = field(s, "desired").clone();
        d.get_mut(kind)?
            .as_object_mut()?
            .insert(id.into(), json!(on));
        Some(d)
    }
    pub fn diff(&self) -> Option<(usize, usize)> {
        let s = self.state.as_ref()?;
        if field(s, "loaded").is_null() {
            return None;
        }
        let (mut add, mut remove) = (0, 0);
        for kind in ["mcps", "skills"] {
            for (id, on) in field(field(s, "desired"), kind)
                .as_object()
                .into_iter()
                .flatten()
            {
                if on != field(field(field(s, "loaded"), kind), id) {
                    if yes(on) { add += 1 } else { remove += 1 }
                }
            }
        }
        Some((add, remove))
    }
    pub fn accept_save(&mut self, suffix: &str, extra: &Value, data: Value) -> bool {
        let Some(s) = &self.state else { return false };
        let mut missing = vec![];
        for k in ["missing", "unavailable"] {
            let v = field(&data, k);
            if v.is_array() {
                missing.extend(rows(v))
            } else {
                for a in v.as_object().into_iter().flatten().map(|(_, v)| v) {
                    missing.extend(rows(a));
                }
            }
        }
        if !missing.is_empty() {
            self.message = format!(
                "Plantilla cargada. No disponibles en este CLI: {}",
                missing
                    .into_iter()
                    .map(|v| if v.is_string() {
                        text(v)
                    } else {
                        let n = field(v, "name");
                        text(if n.is_null() { field(v, "id") } else { n })
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        } else if suffix == "/template" && !field(extra, "name").is_null() {
            self.message = "Plantilla guardada.".into();
        }
        if suffix == "/apply" {
            let mut op = data;
            if let Some(m) = op.as_object_mut() {
                m.entry("state").or_insert(json!("validating"));
            }
            if let Some(m) = self.state.as_mut().and_then(Value::as_object_mut) {
                m.insert("operation".into(), op);
            }
            return false;
        }
        if !field(&data, "inventory").is_null()
            && !field(&data, "desired").is_null()
            && field(&data, "revision").is_number()
            && field(&data, "identity") == field(s, "identity")
            && field(&data, "conversationId") == field(s, "conversationId")
        {
            let mut next = data;
            if let Some(m) = next.as_object_mut() {
                for k in ["missing", "unavailable", "template"] {
                    m.remove(k);
                }
            }
            self.signature = next.to_string();
            self.state = Some(next);
            return true;
        }
        false
    }
    fn picker(&self) -> String {
        format!(
            "<div class=\"toolbar\"><label for=\"ext-harness\">CLI para iniciar</label><select id=\"ext-harness\" {}><option value=\"\">Elige un CLI…</option>{}</select></div>",
            disabled(self.sending),
            HARNESSES
                .iter()
                .map(|(h, n)| format!(
                    "<option value=\"{h}\" {}>{n}</option>",
                    if field(&self.target, "harness").as_str() == Some(h) {
                        "selected"
                    } else {
                        ""
                    }
                ))
                .collect::<String>()
        )
    }
    fn bubble(&self, r: &Value, k: &str, on: bool) -> String {
        let s = self.state.as_ref().unwrap_or(&Value::Null);
        let id = text(field(r, "id"));
        let n = field(field(field(field(s, "usage"), "counts"), k), &id);
        let usage = if n.is_null() {
            "sin dato".into()
        } else if n.as_i64() == Some(0) {
            "sin uso".into()
        } else {
            format!("{} usos", text(n))
        };
        let unknown = !field(field(field(s, "desired"), k), &id).is_boolean();
        let changed = !field(s, "loaded").is_null()
            && !unknown
            && json!(on) != *field(field(field(s, "loaded"), k), &id);
        let tokens = field(field(r, "size"), "tokens")
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0);
        let size = tokens
            .map(|n| format!("{n} tokens"))
            .unwrap_or("Sin medir".into());
        let diameter = tokens
            .map(|n| (88.0 * (n / 1000.0).sqrt()).clamp(76.0, 152.0).round() as u32)
            .unwrap_or(88);
        let label = format!(
            "{size} · {} · cl100k_base · {} · {} · {usage}{}",
            if k == "skills" {
                "archivo de instrucciones"
            } else {
                "definiciones de herramientas"
            },
            text(field(r, "name")),
            if k == "mcps" { "MCP" } else { "Skill" },
            if field(r, "reason").is_null() {
                String::new()
            } else {
                format!(" · {}", text(field(r, "reason")))
            }
        );
        format!(
            "<button style=\"--bubble-size:{diameter}px\" class=\"bubble {k} {}\" data-kind=\"{k}\" data-id=\"{}\" data-on=\"{on}\" {} title=\"{}\" aria-label=\"{}{}\" aria-pressed=\"{on}\"><span class=\"name\">{}</span><small>{size}</small>{}</button>",
            if on { "" } else { "off" },
            esc(&id),
            disabled(self.locked() || !yes(field(r, "toggleable")) || unknown),
            esc(&label),
            if on { "Quitar " } else { "Añadir " },
            esc(&label),
            esc(&text(field(r, "name"))),
            if changed {
                format!(
                    "<span class=\"change\">{}</span>",
                    if on { "+" } else { "−" }
                )
            } else {
                String::new()
            }
        )
    }
    fn groups(&self, items: Vec<(&str, &Value, bool)>, on: bool) -> String {
        if items.is_empty() {
            return "<div class=\"empty\">Ninguna con este filtro.</div>".into();
        }
        let mut groups: BTreeMap<String, Group<'_>> = BTreeMap::new();
        for item in items {
            let o = field(item.1, "origin");
            let (id, label) = if o.is_null() {
                ("unknown".into(), "Origen no registrado".into())
            } else {
                (text(field(o, "id")), text(field(o, "label")))
            };
            groups.entry(id).or_insert((label, vec![])).1.push(item);
        }
        let mut groups: Vec<_> = groups.into_iter().collect();
        groups.sort_by(|a, b| locale_cmp(&a.1.0, &b.1.0));
        groups.into_iter().map(|(id,(label,mut items))|{let key=format!("{}:{id}",if on{"on"}else{"off"});items.sort_by(|a,b|locale_cmp(&text(field(a.1,"name")),&text(field(b.1,"name"))));format!("<details class=\"origin-group\" data-group=\"{}\" {}><summary>{} <span>{}</span> <button type=\"button\" data-batch=\"{}\" data-batch-group=\"{}\" {}>{}</button></summary><div class=\"origin-items\">{}</div></details>",esc(&key),if self.closed_groups.contains(&key){""}else{"open"},esc(&label),items.len(),if on{"off"}else{"on"},esc(&id),disabled(self.locked()||!self.candidates(Some(&id),false).iter().any(|x|x.2==on)),if on{"Quitar categoría"}else{"Añadir categoría"},items.into_iter().map(|(k,r,_)|self.bubble(r,k,on)).collect::<String>())}).collect()
    }
    pub fn html(&self) -> String {
        let title = format!(
            "{} · {}",
            esc(&text(field(&self.target, "session"))),
            esc(&text(field(&self.target, "pane")))
        );
        let Some(s) = &self.state else {
            return format!(
                "<section class=\"shelf\"><header><strong>Extensiones · {title}</strong><span class=\"spacer\"></span><button data-action=\"close\" aria-label=\"Cerrar estante\">×</button></header><div class=\"loading\" role=\"status\">{}<span>{}</span>{}</div>{}</section>",
                if self.error.is_empty() {
                    "<span class=\"loading-spinner\" aria-hidden=\"true\"></span>"
                } else {
                    ""
                },
                esc(if self.error.is_empty() {
                    "Cargando MCPs y skills…"
                } else {
                    &self.error
                }),
                if self.error.is_empty() {
                    ""
                } else {
                    "<button data-action=\"refresh\">Reintentar</button>"
                },
                if text(field(&self.target, "harness")).is_empty() {
                    self.picker()
                } else {
                    String::new()
                }
            );
        };
        let locked = self.locked();
        let op = field(s, "operation");
        let op_state = text(field(op, "state"));
        let d = self.diff();
        let pending = d.is_some_and(|(a, r)| a + r > 0);
        let items = self.items();
        let visible: Vec<_> = items
            .iter()
            .copied()
            .filter(|(k, r, on)| {
                (*on || (yes(field(r, "toggleable"))
                    && field(field(field(s, "desired"), k), &text(field(r, "id"))).as_bool()
                        == Some(false)))
                    && (self.filter == "all" || self.filter == *k)
                    && text(field(r, "name"))
                        .to_lowercase()
                        .contains(&self.query.to_lowercase())
            })
            .collect();
        let candidates = self.candidates(None, false);
        let selected = candidates.iter().filter(|x| x.2).count();
        let excluded: Vec<_> = items
            .iter()
            .filter(|(k, r, _)| {
                !yes(field(r, "toggleable"))
                    || !field(field(field(s, "desired"), k), &text(field(r, "id"))).is_boolean()
            })
            .collect();
        let unused = items
            .iter()
            .filter(|(k, r, on)| {
                *on && yes(field(r, "toggleable"))
                    && field(
                        field(field(field(s, "usage"), "counts"), k),
                        &text(field(r, "id")),
                    )
                    .as_i64()
                        == Some(0)
            })
            .count();
        let configuration =
            field(s, "configurationStatus")
                .as_str()
                .unwrap_or(if field(s, "loaded").is_null() {
                    "unknown"
                } else {
                    "verified"
                });
        let process = match configuration {
            "external" => {
                "Este proceso se inició fuera del selector de extensiones. La selección guardada se aplica desde los controles de este panel."
            }
            "not_started" => "La selección se usará si eliges Iniciar con este set.",
            "verified" => "Configuración comprobada en este proceso.",
            _ => {
                "La selección está guardada; no hay una comprobación válida de la configuración de este proceso."
            }
        };
        let stage = stage(&op_state);
        let operation = format!(
            "{}{}",
            stage,
            if text(field(op, "error")).is_empty() {
                String::new()
            } else {
                format!(" {}", text(field(op, "error")))
            }
        );
        let notice = [
            self.error.clone(),
            if self.sending && op_state.is_empty() {
                "Guardando selección…".into()
            } else {
                String::new()
            },
            self.message.clone(),
            operation,
            text(field(s, "reason")),
            if field(field(s, "inventory"), "status").as_str() == Some("incomplete") {
                "Inventario incompleto: las extensiones no editables se conservan sin cambios."
                    .into()
            } else {
                String::new()
            },
            if configuration == "unverified" {
                "No se pudo verificar la configuración aplicada a este proceso.".into()
            } else {
                String::new()
            },
        ]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_default();
        let mut out = format!(
            "<section class=\"shelf{}\"><header><span class=\"label\">Extensiones</span><strong>{title} · {}</strong><span class=\"muted\">Solo este panel</span><span class=\"spacer\"></span><span class=\"muted\">{}</span>",
            if self.enter { " shelf-enter" } else { "" },
            esc(&HARNESSES
                .iter()
                .find(|(h, _)| Some(*h) == field(s, "harness").as_str())
                .map(|(_, n)| n.to_string())
                .unwrap_or_else(|| text(field(s, "harness")))),
            match d {
                Some((a, r)) if pending => format!("+{a} / −{r} pendientes"),
                Some(_) => "Sin cambios".into(),
                None => "Selección guardada".into(),
            }
        );
        if pending {
            out += &format!(
                "<button data-action=\"discard\" {}>Deshacer</button>",
                disabled(locked)
            )
        }
        out += &format!(
            "<button class=\"go\" data-action=\"apply\" {}>{}</button><button class=\"close\" data-action=\"close\" aria-label=\"Cerrar estante\">×</button></header><div class=\"notice {}\" role=\"status\">{}",
            disabled(
                locked
                    || field(s, "applySupported").as_bool() == Some(false)
                    || (!pending && !field(s, "loaded").is_null())
            ),
            if active(s) {
                "Aplicando…"
            } else if text(field(s, "conversationId")).is_empty() {
                "Iniciar con este set"
            } else if yes(field(s, "busy")) {
                "Aplicar al terminar"
            } else {
                "Aplicar y reanudar"
            },
            if !self.error.is_empty() || matches!(op_state.as_str(), "failed" | "recovery_required")
            {
                "error"
            } else {
                ""
            },
            esc(&notice)
        );
        for (show, action, label) in [
            (self.stale, "refresh", "Actualizar panel"),
            (
                matches!(op_state.as_str(), "validating" | "waiting" | "snapshot"),
                "cancel",
                "Cancelar espera",
            ),
            (
                matches!(
                    op_state.as_str(),
                    "recovery_required" | "awaiting_confirmation"
                ),
                "recover",
                "Recuperar sesión anterior",
            ),
            (
                yes(field(s, "busy"))
                    && !locked
                    && field(s, "applySupported").as_bool() != Some(false),
                "interrupt",
                "Interrumpir turno y aplicar",
            ),
        ] {
            if show {
                out += &format!("<button data-action=\"{action}\">{label}</button>")
            }
        }
        out += "</div>";
        if text(field(s, "conversationId")).is_empty() {
            out += &self.picker()
        }
        out += &format!(
            "<div class=\"toolbar\"><input id=\"ext-search\" type=\"search\" placeholder=\"Buscar extensión…\" aria-label=\"Buscar extensión\" value=\"{}\">",
            esc(&self.query)
        );
        for (k, l) in [("all", "Todo"), ("mcps", "MCPs"), ("skills", "Skills")] {
            out += &format!(
                "<button data-filter=\"{k}\" class=\"{}\" aria-pressed=\"{}\">{l}</button>",
                if self.filter == k { "active" } else { "" },
                self.filter == k
            )
        }
        if self.filter != "all" || !self.query.is_empty() {
            let shown = self.candidates(None, true);
            for on in [true, false] {
                out += &format!(
                    "<button data-batch=\"{}\" data-batch-scope=\"visible\" {}>{} resultados{}</button>",
                    if on { "on" } else { "off" },
                    disabled(locked || !shown.iter().any(|x| x.2 != on)),
                    if on { "Añadir" } else { "Quitar" },
                    if on {
                        format!(" ({})", shown.len())
                    } else {
                        String::new()
                    }
                )
            }
        }
        out += "<span class=\"spacer\"></span><span class=\"muted gesture\">Toca o arrastra · azul MCP · ámbar skill</span></div>";
        out += &format!(
            "<div class=\"selection-toolbar\" aria-label=\"Selección global\"><strong role=\"status\">{} · {selected} de {}</strong>{}<button data-batch=\"on\" {}>Seleccionar todos</button><button data-batch=\"off\" {}>Quitar todos</button><span class=\"muted\">Todos los MCPs y skills editables, incluidos los ocultos por filtros. El uso registrado no cambia la selección.</span></div><div class=\"body\"><aside class=\"templates\"><div class=\"label\">Plantillas</div>",
            if selected == 0 {
                "Ninguna seleccionada"
            } else if selected == candidates.len() {
                "Todas seleccionadas"
            } else {
                "Selección parcial"
            },
            candidates.len(),
            if excluded.is_empty() {
                String::new()
            } else {
                format!(
                    "<span class=\"muted\">{} fuera del lote · detalle al pie</span>",
                    excluded.len()
                )
            },
            disabled(locked || selected == candidates.len()),
            disabled(locked || selected == 0)
        );
        for t in rows(field(s, "templates")) {
            out += &format!(
                "<button data-template=\"{}\" {}>{}</button>",
                esc(&text(field(t, "id"))),
                disabled(locked),
                esc(&text(field(t, "name")))
            )
        }
        out += &format!(
            "<form><input id=\"template-name\" value=\"{}\" placeholder=\"Nombre del set\" aria-label=\"Nombre de la plantilla\" maxlength=\"80\" required {}><button type=\"submit\" data-action=\"save-template\" {}>+ Guardar set</button></form><p class=\"muted\">Disponibles entre agentes. Se cargan solo cuando las eliges.</p></aside>",
            esc(&self.template_name),
            disabled(locked),
            disabled(locked)
        );
        for on in [true, false] {
            let zone: Vec<_> = visible.iter().copied().filter(|x| x.2 == on).collect();
            out += &format!(
                "<div class=\"zone {}\" data-zone=\"{}\"><div class=\"label\">{} · {}</div><span class=\"muted\">{}</span><div class=\"field\">{}</div></div>",
                if on { "on" } else { "off" },
                if on { "on" } else { "off" },
                if on {
                    "Seleccionadas"
                } else {
                    "Sin seleccionar"
                },
                zone.len(),
                if on {
                    "Selección guardada para este panel"
                } else {
                    "Puedes añadirlas a este panel"
                },
                self.groups(zone, on)
            )
        }
        out += "</div><footer>";
        if unused > 0 {
            out += &format!(
                "<details><summary>Uso registrado</summary><p>Quita únicamente las extensiones con cero usos registrados. No incluye las que no tienen datos.</p><button data-action=\"unused\" {}>Quitar {unused} con cero usos</button></details>",
                disabled(locked)
            )
        }
        out += &format!(
            "<details class=\"process-state\"><summary>Estado del proceso</summary><p>{process}</p></details><span>{} MCPs + {} skills</span><span title=\"Tamaño del archivo principal de la skill o de las definiciones MCP medidas. No representa consumo por turno ni contexto cargado.\">Tamaño: tokens · cl100k_base · Sin medir: tamaño neutro</span><span>{}</span>",
            rows(field(field(s, "inventory"), "mcps")).len(),
            rows(field(field(s, "inventory"), "skills")).len(),
            if yes(field(field(s, "usage"), "complete")) {
                "Uso registrado en esta conversación"
            } else {
                "Uso parcial: ausencia de datos ≠ sin uso"
            }
        );
        if !excluded.is_empty() {
            out += &format!(
                "<details><summary>{} excluidas o gestionadas aparte</summary>",
                excluded.len()
            );
            for (_, r, _) in excluded {
                let reason = if !yes(field(r, "toggleable")) {
                    let reason = text(field(r, "reason"));
                    if reason.is_empty() {
                        "Gestionada fuera de este panel".into()
                    } else {
                        reason
                    }
                } else {
                    "Estado desconocido en este panel; se conserva sin cambios.".into()
                };
                out += &format!(
                    "<p><b>{}</b> · {}</p>",
                    esc(&text(field(r, "name"))),
                    esc(&reason)
                )
            }
            out += "</details>"
        }
        out += "</footer></section>";
        out
    }
}
fn disabled(on: bool) -> &'static str {
    if on { "disabled" } else { "" }
}
fn stage(s: &str) -> &str {
    match s {
        "validating" => "Comprobando selección…",
        "waiting" => "En espera: se aplicará al terminar el turno.",
        "snapshot" => "Guardando punto de recuperación…",
        "applying" => "Reanudando con la selección…",
        "verifying" => "Verificando el proceso…",
        "recovering" => "Recuperando la sesión anterior…",
        "recovery_required" => "Hace falta recuperar la sesión anterior.",
        "awaiting_confirmation" => "Revisa la terminal: hay una confirmación pendiente.",
        "confirmed" => "Selección verificada en el proceso.",
        "rolled_back" => "Se recuperó la sesión anterior.",
        "failed" => "No se aplicó el cambio.",
        _ => s,
    }
}
#[cfg(target_arch = "wasm32")]
#[path = "extensions_web.rs"]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::{attach, mount};
#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn attach() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
