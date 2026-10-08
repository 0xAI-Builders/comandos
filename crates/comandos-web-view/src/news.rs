//! News reader presentation. Strings use the shared lossless UTF-16 encoding.
use crate::escape::text as esc;
use serde_json::{Value, json};
pub fn at<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or(&Value::Null)
}
pub fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
pub fn s(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        _ => v.to_string(),
    }
}
pub fn n(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| s(v).parse().unwrap_or(0.))
}
pub fn yes(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(x) => *x,
        Value::Number(_) => n(v) != 0.,
        Value::String(x) => !x.is_empty(),
        _ => true,
    }
}
pub fn field(v: &Value, key: &str) -> String {
    s(at(v, key))
}
pub fn e(v: &Value, key: &str) -> String {
    esc(&field(v, key))
}
pub fn status_label(status: &str) -> String {
    match status {
        "published" => "Listo",
        "partial" => "Parcial",
        "empty" => "Sin novedades",
        "failed" => "Falló",
        "not_published" => "No se generó",
        "running" => "Generando…",
        "scheduled" => "Programado",
        _ => status,
    }
    .into()
}
pub fn terminal_status(status: &str) -> bool {
    matches!(
        status,
        "published" | "partial" | "empty" | "failed" | "not_published"
    )
}
pub fn model_name(label: &str) -> String {
    let after = label.split_once(':').map(|(_, s)| s).unwrap_or("");
    let tail = after.rsplit('/').next().unwrap_or("");
    if tail.is_empty() {
        label.into()
    } else {
        tail.into()
    }
}
pub fn story_kicker(story: &Value) -> (String, bool) {
    let cat = field(story, "category");
    let lab = field(at(story, "meta"), "lab");
    match cat.as_str() {
        "oficial" => (
            format!(
                "Oficial · {}",
                if lab.is_empty() { "anuncio" } else { &lab }
            ),
            false,
        ),
        "hot" => (
            format!(
                "Hot · {}",
                if lab.is_empty() || lab == "Comunidad" {
                    "comunidad"
                } else {
                    &lab
                }
            ),
            true,
        ),
        _ => (
            (match cat.as_str() {
                "modelo" => "Modelos",
                "mcp" => "MCP",
                "skill" => "Skill",
                "bounty" => "Bounty",
                "hackathon" => "Hackathon",
                "ia" => "IA",
                "" => "IA",
                _ => &cat,
            })
            .into(),
            matches!(cat.as_str(), "bounty" | "hackathon"),
        ),
    }
}
pub fn inline_text(text: &str) -> String {
    let escaped = esc(text);
    let mut out = String::new();
    let mut rest = escaped.as_str();
    while let Some((before, after)) = rest.split_once('`') {
        out.push_str(before);
        if let Some((body, tail)) = after.split_once('`')
            && !body.is_empty()
            && !body.contains('\n')
            && crate::utf16::decode(body).len() <= 300
        {
            out.push_str("<code>");
            out.push_str(body);
            out.push_str("</code>");
            rest = tail;
        } else {
            out.push('`');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}
pub fn media_name(name: &str) -> bool {
    name.split_once('.').is_some_and(|(hash, ext)| {
        hash.len() == 32
            && hash
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            && matches!(ext, "png" | "jpg" | "webp" | "gif" | "avif")
    })
}
pub fn blocks_html(blocks: &Value) -> String {
    let mut out = String::new();
    let mut list = false;
    for b in arr(blocks) {
        if !b.is_object() {
            continue;
        }
        let kind = field(b, "type");
        if kind == "li" {
            if !list {
                out.push_str("<ul>");
                list = true
            }
            out.push_str(&format!("<li>{}</li>", inline_text(&field(b, "text"))));
            continue;
        }
        if list {
            out.push_str("</ul>");
            list = false
        }
        let t = inline_text(&field(b, "text"));
        out.push_str(&match kind.as_str(){"img"=>{let name=field(b,"media");if !media_name(&name){continue}format!("<figure class=\"nr-fig\"><img data-media=\"{}\" alt=\"{}\" loading=\"lazy\">{}</figure>",esc(&name),e(b,"alt"),if yes(at(b,"alt")){format!("<figcaption>{}</figcaption>",e(b,"alt"))}else{String::new()})},"h"=>format!("<h3>{t}</h3>"),"quote"=>format!("<blockquote>{t}</blockquote>"),"code"=>format!("<pre><code>{}</code></pre>",e(b,"text")),"caption"=>format!("<p class=\"nr-caption\">{t}</p>"),_=>format!("<p>{t}</p>")});
    }
    if list {
        out.push_str("</ul>")
    }
    out
}
pub fn note_command(text: &str) -> Option<String> {
    let t = text.trim_start();
    let p = t.get(..5)?;
    if !p.eq_ignore_ascii_case("/nota") {
        return None;
    }
    let tail = t.get(5..)?;
    if !tail.chars().next().is_some_and(char::is_whitespace) || tail.trim_start().is_empty() {
        return None;
    }
    Some(tail.trim().into())
}
pub fn clamp_share(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(40., 70.)
    } else {
        58.
    }
}
fn fixed_cost(n: f64) -> String {
    if !n.is_finite() || n.abs() >= 1e21 {
        return n.to_string();
    }
    let bits = n.abs().to_bits();
    let raw = ((bits >> 52) & 0x7ff) as i32;
    let mantissa = u128::from(bits & ((1u64 << 52) - 1)) | if raw == 0 { 0 } else { 1u128 << 52 };
    let exp = if raw == 0 { -1074 } else { raw - 1075 };
    let numerator = mantissa * 100;
    let rounded = if exp >= 0 {
        numerator.checked_shl(exp as u32).unwrap_or(0)
    } else {
        let shift = (-exp) as u32;
        if shift >= 128 {
            0
        } else {
            let d = 1u128 << shift;
            (numerator >> shift) + u128::from(numerator & (d - 1) >= d / 2)
        }
    };
    format!(
        "{}{}.{:02}",
        if n < 0. { "-" } else { "" },
        rounded / 100,
        rounded % 100
    )
}
pub fn provenance(edition: &Value) -> Value {
    let models = at(edition, "models");
    let mut keys = models
        .as_object()
        .map(|o| o.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort_by(|a, b| {
        fn idx(s: &str) -> Option<u32> {
            let n = s.parse::<u32>().ok()?;
            if n == u32::MAX || n.to_string() != s {
                None
            } else {
                Some(n)
            }
        }
        match (idx(a), idx(b)) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        }
    });
    let mut label = keys
        .iter()
        .map(|m| format!("{} ({})", model_name(m), s(at(models, m))))
        .collect::<Vec<_>>()
        .join(" · ");
    if label.is_empty() {
        label = field(edition, "model")
            .rsplit('/')
            .next()
            .unwrap_or("")
            .into()
    }
    let job = at(edition, "job");
    let duration = if yes(at(job, "startedAt")) && yes(at(job, "finishedAt")) {
        format!(
            "{} min",
            ((n(at(job, "finishedAt")) - n(at(job, "startedAt"))) / 60000. + 0.5)
                .floor()
                .max(1.)
        )
    } else {
        String::new()
    };
    let total = n(at(edition, "sourceCount"));
    let failed = n(at(edition, "failedSourceCount"));
    json!({"models":label,"cost":format!("${}",fixed_cost(n(at(edition,"costUsd")))),"duration":duration,"sources":if total!=0.{format!("{} de {} fuentes",total-failed,total)}else{String::new()},"problems":arr(at(edition,"notes")).len()})
}
pub fn day_line(list: &Value, today: &str) -> Value {
    let mut cards = arr(at(list, "editions"))
        .iter()
        .filter(|e| field(e, "localDate") == today)
        .cloned()
        .collect::<Vec<_>>();
    cards.sort_by_key(|e| field(e, "slot"));
    for card in &mut cards {
        if let Some(o) = card.as_object_mut() {
            o.insert("day".into(), "Hoy".into());
        }
    }
    let next = at(list, "next");
    if yes(next) && !cards.iter().any(|c| at(c, "id") == at(next, "id")) {
        let mut next = next.clone();
        let label = if field(&next, "localDate") == today {
            "Hoy"
        } else {
            "Mañana"
        };
        if let Some(o) = next.as_object_mut() {
            o.insert("day".into(), label.into());
        }
        cards.push(next)
    }
    cards.into()
}
pub fn plain(text: &str) -> String {
    format!(
        "<p class=\"nr-plain\">{}</p>",
        esc(text).replace('\n', "<br>")
    )
}
pub const TEMPLATE: &str = include_str!("news_template.html");
pub const COPY_ICON: &str = r#"<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>"#;
/// Server-rendered Markdown is the only pre-escaped dynamic fragment here.
pub struct Render<'a> {
    pub state: &'a Value,
    pub markdown: &'a dyn Fn(&str) -> String,
    pub date: &'a dyn Fn(f64, &str) -> String,
    pub href: &'a dyn Fn(&str) -> Option<String>,
    pub host: &'a dyn Fn(&str) -> String,
    pub groups: &'a dyn Fn(&Value) -> Value,
}
impl Render<'_> {
    fn md(&self, v: &Value) -> String {
        (self.markdown)(&s(v))
    }
    fn date(&self, v: &Value, tz: &str) -> String {
        (self.date)(n(v), tz)
    }
    fn selected(&self, key: &str, v: &Value) -> bool {
        at(self.state, key) == v
    }
    pub fn picker(&self) -> String {
        let data = at(self.state, "current");
        let list = at(self.state, "list");
        let today = if !data.is_null() {
            field(at(data, "edition"), "localDate")
        } else {
            arr(at(list, "editions"))
                .first()
                .map(|e| field(e, "localDate"))
                .unwrap_or_default()
        };
        let cards = day_line(list, &today);
        let cur = at(at(data, "edition"), "id");
        arr(&cards).iter().map(|c|{let status=field(c,"status");let readable=matches!(status.as_str(),"published"|"partial");let first=at(c,"models").as_object().and_then(|o|o.keys().next()).map(|s|model_name(s));let sub=if readable{format!("{} noticias{}",e(c,"storyCount"),first.map(|s|format!(" · {}",esc(&s))).unwrap_or_default())}else if status=="scheduled"{"Se generará a su hora".into()}else{esc(&crate::utf16::encode(crate::utf16::decode(&s(arr(at(c,"notes")).first().unwrap_or(&Value::Null))).into_iter().take(70)))};format!("<button type=\"button\" class=\"nr-slot{}\" data-edition=\"{}\" {} aria-pressed=\"{}\"><small>{}</small><b>{}</b><span class=\"nr-st st-{}\">{}</span><em>{sub}</em></button>",if at(c,"id")==cur&&field(self.state,"view")=="edition"{" on"}else{""},e(c,"id"),if status=="scheduled"{"disabled"}else{""},at(c,"id")==cur,e(c,"day"),e(c,"slot"),esc(&status),esc(&status_label(&status)))}).collect()
    }
    pub fn story(&self, story: &Value, rank: usize) -> String {
        let (k, hot) = story_kicker(story);
        let counts = at(story, "counts");
        let opened = self.selected("open", at(story, "id"));
        let sources = arr(at(story, "sources")).len();
        let notes = n(at(counts, "notes"));
        format!(
            "<section class=\"nr-edition-story{}\" id=\"nr-story-{}\" data-story=\"{}\">\n        <small class=\"nr-kicker\"><span class=\"{}\">{}</span><span class=\"nr-n\">#{rank} del día</span><span class=\"nr-n\">{sources} fuente{}</span>{}</small>\n        <h2><button type=\"button\" data-flat class=\"nr-open-story\" data-open=\"{}\">{}</button></h2>\n        <div class=\"nr-markdown nr-summary\">{}</div>\n        <div class=\"nr-acts\"><button type=\"button\" class=\"nr-open-btn{}\" data-open=\"{}\">{}</button><button type=\"button\" class=\"nr-save\" data-save=\"{}\" aria-pressed=\"{}\">{}</button></div>\n      </section>",
            if opened { " on" } else { "" },
            e(story, "id"),
            e(story, "id"),
            if hot { "nr-hot" } else { "" },
            esc(&k),
            if sources == 1 { "" } else { "s" },
            if notes != 0. {
                format!(
                    "<span class=\"nr-nn\">{notes} nota{}</span>",
                    if notes == 1. { "" } else { "s" }
                )
            } else {
                String::new()
            },
            e(story, "id"),
            e(story, "title"),
            self.md(at(story, "summary")),
            if opened { " on" } else { "" },
            e(story, "id"),
            if opened { "Abierta →" } else { "Abrir" },
            e(story, "id"),
            yes(at(counts, "saved")),
            if yes(at(counts, "saved")) {
                "Guardada"
            } else {
                "Guardar"
            }
        )
    }
    fn details(&self, edition: &Value) -> String {
        let p = provenance(edition);
        let tz = field(edition, "timezone");
        let job = at(edition, "job");
        let chain = arr(at(at(self.state, "list"), "chain"))
            .iter()
            .map(|s| model_name(&crate::news::s(s)))
            .collect::<Vec<_>>()
            .join(" → ");
        let fallback = |s: String| if s.is_empty() { "—".into() } else { s };
        let time = if yes(at(job, "startedAt")) {
            format!(
                "{} · {}",
                self.date(at(job, "startedAt"), &tz),
                if yes(at(job, "finishedAt")) {
                    self.date(at(job, "finishedAt"), &tz)
                } else {
                    "en curso".into()
                }
            )
        } else {
            "—".into()
        };
        let rows = [
            ("Modelos que escribieron", fallback(field(&p, "models"))),
            ("Cadena de respaldo", fallback(chain)),
            ("Costo", field(&p, "cost")),
            (
                "Programado",
                format!("{} {}", field(edition, "localDate"), field(edition, "slot")),
            ),
            ("Inicio · fin", time),
            ("Duración", fallback(field(&p, "duration"))),
            (
                "Intentos",
                if yes(at(job, "attempts")) {
                    field(job, "attempts")
                } else {
                    "—".into()
                },
            ),
            ("Fuentes", fallback(field(&p, "sources"))),
        ];
        let notes = arr(at(edition, "notes"))
            .iter()
            .map(|n| format!("<li>{}</li>", esc(&s(n))))
            .collect::<String>();
        format!(
            "<div class=\"nr-details\"><dl>{}</dl>{}</div>",
            rows.iter()
                .map(|(k, v)| format!("<div><dt>{}</dt><dd>{}</dd></div>", esc(k), esc(v)))
                .collect::<String>(),
            if notes.is_empty() {
                String::new()
            } else {
                format!("<h3>Qué falló o quedó incompleto</h3><ul>{notes}</ul>")
            }
        )
    }
    pub fn edition(&self) -> String {
        if field(self.state, "view") == "saved" {
            return self.saved();
        }
        let data = at(self.state, "current");
        if data.is_null() {
            return String::new();
        }
        let edition = at(data, "edition");
        let stories = arr(at(data, "stories"));
        let status = field(edition, "status");
        let readable = matches!(status.as_str(), "published" | "partial");
        let p = provenance(edition);
        let details = yes(at(self.state, "details"));
        let line = if readable {
            let models = field(&p, "models");
            let sources = field(&p, "sources");
            let duration = field(&p, "duration");
            let problems = n(at(&p, "problems"));
            format!(
                "<p class=\"nr-prov\"><span>Escrito por <b>{}</b></span><span>{} noticias</span>{}<span>{}{}</span><button type=\"button\" class=\"nr-details-toggle\" aria-expanded=\"{details}\">{}{}</button></p>",
                esc(if models.is_empty() { "IA" } else { &models }),
                e(edition, "storyCount"),
                if sources.is_empty() {
                    String::new()
                } else {
                    format!("<span>{}</span>", esc(&sources))
                },
                e(&p, "cost"),
                if duration.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", esc(&duration))
                },
                if problems != 0. {
                    format!(
                        "{problems} aviso{} · ",
                        if problems == 1. { "" } else { "s" }
                    )
                } else {
                    String::new()
                },
                if details {
                    "Ocultar detalles"
                } else {
                    "Detalles"
                }
            )
        } else {
            format!(
                "<p class=\"nr-prov nr-bad\">{}: {}</p>",
                esc(&status_label(&status)),
                esc(&arr(at(edition, "notes"))
                    .first()
                    .map(s)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "sin noticias publicadas".into()))
            )
        };
        let head = format!(
            "<header class=\"nr-edition-head\">\n        <small>{} · {}</small>\n        <h1>Resumen de las {}</h1>\n        {}\n        {line}{}\n      </header>",
            e(edition, "localDate"),
            esc(&status_label(&status)),
            e(edition, "slot"),
            if yes(at(edition, "lead")) {
                format!("<p class=\"nr-lead\">{}</p>", e(edition, "lead"))
            } else {
                String::new()
            },
            if details || !readable {
                self.details(edition)
            } else {
                String::new()
            }
        );
        let list = if stories.is_empty() {
            format!(
                "<p class=\"nr-empty\">{}</p>",
                if readable {
                    "No hay noticias en este resumen."
                } else {
                    "Este resumen no tiene noticias."
                }
            )
        } else {
            stories
                .iter()
                .enumerate()
                .map(|(i, s)| self.story(s, i + 1))
                .collect()
        };
        let next = at(at(self.state, "list"), "next");
        let foot = if stories.is_empty() {
            String::new()
        } else {
            format!(
                "<p class=\"nr-foot\">Fin del resumen{}.</p>",
                if next.is_null() {
                    String::new()
                } else {
                    format!(" · el siguiente sale a las {}", e(next, "slot"))
                }
            )
        };
        head + &list + &foot
    }
    fn saved(&self) -> String {
        let list = arr(at(self.state, "saved"));
        let mut out = format!(
            "<header class=\"nr-edition-head\"><small>Guardadas</small><h1>Noticias guardadas</h1>\n        <p class=\"nr-prov\"><span>{} noticia{}</span><button type=\"button\" class=\"nr-back-edition\">Volver al resumen</button></p></header>",
            list.len(),
            if list.len() == 1 { "" } else { "s" }
        );
        if list.is_empty() {
            out.push_str("<p class=\"nr-empty\">Aún no guardas noticias. Toca «Guardar» en una para tenerla aquí.</p>")
        }
        for story in list {
            let (k, _) = story_kicker(story);
            let lab = field(at(story, "meta"), "lab");
            out.push_str(&format!("<section class=\"nr-edition-story\"><small class=\"nr-kicker\"><span>{}</span><span class=\"nr-n\">{}</span></small>\n            <h2><button type=\"button\" data-flat class=\"nr-open-story\" data-goto=\"{}\" data-goto-story=\"{}\">{}</button></h2>\n            <div class=\"nr-markdown nr-summary\">{}</div>\n            <div class=\"nr-acts\"><button type=\"button\" data-goto=\"{}\" data-goto-story=\"{}\">Abrir</button><button type=\"button\" class=\"nr-save\" data-save=\"{}\" aria-pressed=\"true\">Guardada</button></div></section>",esc(if lab.is_empty(){&k}else{&lab}),esc(&field(story,"editionId").replacen('@'," · ",1)),e(story,"editionId"),e(story,"id"),e(story,"title"),self.md(at(story,"summary")),e(story,"editionId"),e(story,"id"),e(story,"id")))
        }
        out
    }
    pub fn panel(&self) -> String {
        let open = at(self.state, "open");
        if s(open) == "notes" {
            return format!(
                "<div class=\"nr-ph\"><button type=\"button\" class=\"nr-panel-back\" aria-label=\"Volver a la lista\">←</button><strong class=\"nr-ph-title\">Mis notas</strong><button type=\"button\" class=\"nr-panel-close\" aria-label=\"Cerrar panel\">✕</button></div>\n          <div class=\"nr-pb\">{}</div>",
                self.notes(&Value::Null)
            );
        }
        let Some(story) = arr(at(at(self.state, "current"), "stories"))
            .iter()
            .find(|s| at(s, "id") == open)
        else {
            return String::new();
        };
        let (k, hot) = story_kicker(story);
        let tab = field(self.state, "tab");
        let mut dock = String::new();
        let body = match tab.as_str() {
            "resumen" => {
                let opp = at(story, "opportunity");
                let mut facts = String::new();
                if yes(opp) {
                    facts.push_str("<dl class=\"nr-bounty-facts\">");
                    for (k, key) in [
                        ("Recompensa", "reward"),
                        ("Fecha límite", "deadline"),
                        ("Zona horaria", "timezone"),
                        ("Elegibilidad", "eligibility"),
                        ("Entrega", "submission"),
                    ] {
                        let v = field(opp, key);
                        facts.push_str(&format!(
                            "<div><dt>{k}</dt><dd>{}</dd></div>",
                            esc(if v.is_empty() { "desconocido" } else { &v })
                        ))
                    }
                    facts.push_str("</dl>")
                }
                format!(
                    "<div class=\"nr-markdown nr-summary\">{}</div>{facts}<div class=\"nr-markdown\">{}</div><p class=\"nr-foot\">Resumen IA{}. Las fuentes completas están en «Fuentes».</p>",
                    self.md(at(story, "summary")),
                    self.md(at(story, "body")),
                    if yes(at(story, "model")) {
                        format!(" · {}", esc(&model_name(&field(story, "model"))))
                    } else {
                        String::new()
                    }
                )
            }
            "fuentes" => self.sources(story),
            "chat" => {
                dock = format!(
                    "<form class=\"nr-dock\"><input class=\"nr-chat-input\" autocomplete=\"off\" placeholder=\"Pregunta a esta noticia… (/nota guarda lo que escribas)\" value=\"{}\" aria-label=\"Mensaje\"><button type=\"submit\" class=\"nr-pri\">Enviar</button></form>",
                    e(self.state, "chatDraft")
                );
                self.chat(story)
            }
            _ => self.notes(story),
        };
        let tabs=[("resumen","Resumen IA",0.),("fuentes","Fuentes",arr(at(story,"sources")).len()as f64),("chat","Chat",n(at(at(story,"counts"),"chat"))),("notas","Notas",n(at(at(story,"counts"),"notes")))].iter().map(|(key,label,count)|format!("<button type=\"button\" data-flat role=\"tab\" data-tab=\"{key}\" aria-selected=\"{}\" class=\"{}\">{label}{}</button>",tab==*key,if tab==*key{"on"}else{""},if *count!=0.{format!("<i>{count}</i>")}else{String::new()})).collect::<String>();
        format!(
            "<div class=\"nr-ph\"><button type=\"button\" class=\"nr-panel-back\" aria-label=\"Volver a la lista\">←</button><div class=\"nr-tabs\" role=\"tablist\">{tabs}</div><button type=\"button\" class=\"nr-panel-close\" aria-label=\"Cerrar panel\">✕</button></div>\n        <div class=\"nr-pt\"><span class=\"{}\">{}</span><b>{}</b></div>\n        <div class=\"nr-pb\">{body}</div>{dock}",
            if hot { "nr-hot" } else { "" },
            esc(&k),
            e(story, "title")
        )
    }
    fn sources(&self, story: &Value) -> String {
        let sources = arr(at(story, "sources"));
        let Some(current) = sources
            .iter()
            .find(|s| at(s, "id") == at(self.state, "sourceId"))
            .or_else(|| sources.iter().find(|s| yes(at(s, "captured"))))
            .or_else(|| sources.first())
        else {
            return "<p class=\"nr-empty\">Esta noticia no tiene fuentes guardadas.</p>".into();
        };
        let tabs=sources.iter().map(|s|format!("<button type=\"button\" class=\"nr-src-pick{}\" data-source=\"{}\"><b>{}{}</b><small>{}</small></button>",if at(s,"id")==at(current,"id"){" on"}else{""},e(s,"id"),if yes(at(s,"official")){"Oficial · "}else{""},e(s,"origin"),if yes(at(s,"heat"))&&field(s,"heat")!="oficial"{e(s,"heat")}else{esc(&(self.host)(&field(s,"url")))})).collect::<String>();
        let loaded = at(at(self.state, "sources"), &field(current, "id"));
        let article = if loaded.is_null() {
            "<p class=\"nr-empty\">Cargando la fuente…</p>".into()
        } else if yes(at(loaded, "error")) {
            format!(
                "<p class=\"nr-empty\">No pude abrir la fuente: {}</p>",
                e(loaded, "error")
            )
        } else {
            self.source_article(loaded)
        };
        format!("<div class=\"nr-src-list\">{tabs}</div>{article}")
    }
    fn source_article(&self, src: &Value) -> String {
        let cap = at(src, "capture");
        let tr = at(src, "translation");
        let original =
            arr(at(self.state, "original")).contains(at(src, "id")) || field(tr, "state") != "done";
        let button = if !yes(cap) {
            String::new()
        } else if !yes(tr) || field(tr, "state") == "failed" {
            format!(
                "<button type=\"button\" data-flat class=\"nr-xs\" data-translate=\"{}\">{}</button>",
                e(src, "id"),
                if field(tr, "state") == "failed" {
                    "Reintentar traducción"
                } else {
                    "Traducir con IA"
                }
            )
        } else if field(tr, "state") == "running" {
            "<span class=\"nr-xs nr-busy\">Traduciendo…</span>".into()
        } else {
            format!(
                "<button type=\"button\" data-flat class=\"nr-xs{}\" data-lang=\"es\" data-source-lang=\"{}\">★ Traducción guardada</button><button type=\"button\" data-flat class=\"nr-xs{}\" data-lang=\"orig\" data-source-lang=\"{}\">Original</button>",
                if original { "" } else { " nr-saved" },
                e(src, "id"),
                if original { " nr-saved" } else { "" },
                e(src, "id")
            )
        };
        let href = (self.href)(&field(src, "url"));
        let head=format!("<div class=\"nr-src-head\">{}<span class=\"nr-src-host\">{}{}</span><span class=\"nr-sp\"></span>{button}{}</div>",if yes(at(src,"official")){"<span class=\"nr-of\">Fuente oficial</span>"}else{""},esc(&(self.host)(&field(src,"url"))),if yes(at(src,"heat")){format!(" · {}",e(src,"heat"))}else{String::new()},href.map(|h|format!("<a class=\"nr-xs\" href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer nofollow\">Abrir ↗</a>",esc(&h))).unwrap_or_default());
        if !yes(cap) {
            return format!(
                "<article class=\"nr-article\">{head}<h1>{}</h1><p class=\"nr-empty\">Esta fuente no se pudo capturar completa{}. Ábrela en su sitio para leerla.</p></article>",
                e(src, "title"),
                if field(src, "role") == "discussion" {
                    " (el hilo no respondió)"
                } else {
                    ""
                }
            );
        }
        let title = if original {
            field(cap, "title")
        } else {
            field(tr, "title")
        };
        let title = if title.is_empty() {
            let captitle = field(cap, "title");
            if captitle.is_empty() {
                field(src, "title")
            } else {
                captitle
            }
        } else {
            title
        };
        let mut by = vec![field(cap, "byline")];
        if yes(at(src, "publishedAt")) {
            by.push(self.date(at(src, "publishedAt"), ""))
        }
        if !original && yes(at(tr, "model")) {
            by.push(format!("traducido con {}", model_name(&field(tr, "model"))))
        }
        let by = by
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        format!(
            "<article class=\"nr-article\">{head}{}<h1>{}</h1>{}<div class=\"nr-markdown nr-captured\">{}</div><p class=\"nr-foot\">Capturada {}{}; se lee aunque el sitio cambie.</p></article>",
            if field(tr, "state") == "failed" {
                format!(
                    "<p class=\"nr-note-bad\">La traducción falló: {}.</p>",
                    esc(&{
                        let error = field(tr, "error");
                        if error.is_empty() {
                            "sin detalle".into()
                        } else {
                            error
                        }
                    })
                )
            } else {
                String::new()
            },
            esc(&title),
            if by.is_empty() {
                String::new()
            } else {
                format!("<div class=\"nr-by\">{}</div>", esc(&by))
            },
            blocks_html(if original {
                at(cap, "blocks")
            } else {
                at(tr, "blocks")
            }),
            esc(&self.date(at(cap, "capturedAt"), "")),
            if yes(at(cap, "partial")) {
                " · solo el extracto que publicó el feed"
            } else {
                ""
            }
        )
    }
    fn chat(&self, story: &Value) -> String {
        let messages = at(at(self.state, "chat"), &field(story, "id"));
        if messages.is_null() {
            return "<p class=\"nr-empty\">Cargando la conversación…</p>".into();
        }
        let count = arr(at(story, "sources")).len();
        let mut out = format!(
            "<div class=\"nr-ctx\">Chat con esta noticia · {count} fuente{} capturada{} · se guarda con la noticia</div>",
            if count == 1 { "" } else { "s" },
            if count == 1 { "" } else { "s" }
        );
        let messages = arr(messages);
        if messages.is_empty() {
            out.push_str("<p class=\"nr-empty\">Pregunta lo que quieras de esta noticia. La IA responde con las fuentes capturadas y cita el párrafo.</p>\n          <div class=\"nr-chips\">");
            for q in [
                "¿Qué cambia para mí?",
                "¿Qué dice la comunidad?",
                "¿Qué no dice la fuente?",
            ] {
                out.push_str(&format!(
                    "<button type=\"button\" data-ask=\"{}\">{}</button>",
                    esc(q),
                    esc(q)
                ))
            }
            out.push_str("</div>");
            return out;
        }
        for m in messages {
            let mine = field(m, "role") == "user";
            let noted = yes(at(m, "noted"));
            let state = field(m, "state");
            let text = if state == "pending" {
                "<span class=\"nr-thinking\">Pensando<i>.</i><i>.</i><i>.</i></span>".into()
            } else if mine {
                format!("<p>{}</p>", e(m, "text").replace('\n', "<br>"))
            } else {
                format!(
                    "<div class=\"nr-markdown\">{}</div>",
                    self.md(at(m, "text"))
                )
            };
            let tools = if state == "done" {
                format!(
                    "<div class=\"nr-tip\"><button type=\"button\" data-flat class=\"nr-xs{}\" data-note-chat=\"{}\" title=\"{}\" aria-pressed=\"{noted}\">{}</button><button type=\"button\" data-flat class=\"nr-xs\" data-copy-chat=\"{}\" title=\"Copiar\" aria-label=\"Copiar\">{COPY_ICON}</button></div>",
                    if noted { " nr-saved" } else { "" },
                    e(m, "id"),
                    if noted {
                        "Quitar de mis notas"
                    } else {
                        "Guardar como nota"
                    },
                    if noted { "★" } else { "☆" },
                    e(m, "id")
                )
            } else {
                String::new()
            };
            out.push_str(&format!("<div class=\"nr-msg{}{}{}\">{tools}<small class=\"nr-who\">{}</small>{text}{}</div>",if mine{" me"}else{""},if noted{" noted"}else{""},if state=="failed"{" failed"}else{""},if mine{"Tú".into()}else{format!("{} · con las fuentes",if yes(at(m,"model")){esc(&model_name(&field(m,"model")))}else{"IA".into()})},if yes(at(m,"cite")){format!("<cite>{}</cite>",e(m,"cite"))}else{String::new()}))
        }
        out.push_str("<p class=\"nr-foot\">☆ en una burbuja la guarda como nota con su cita.</p>");
        out
    }
    fn note_item(&self, note: &Value, with_story: bool) -> String {
        let editing = self.selected("editing", at(note, "id"));
        let confirm = self.selected("confirmDelete", at(note, "id"));
        let id = e(note, "id");
        let meta = format!(
            "{}{} · {}",
            esc(&self.date(at(note, "createdAt"), "")),
            if with_story {
                format!(" · <b>{}</b>", e(note, "storyTitle"))
            } else {
                String::new()
            },
            if field(note, "kind") == "chat" {
                "del chat"
            } else {
                "escrita"
            }
        );
        let quote = if yes(at(note, "quote")) {
            format!(
                "<div class=\"nr-quote\">{}</div>",
                self.md(at(note, "quote"))
            )
        } else {
            String::new()
        };
        let text = if editing {
            format!(
                "<textarea class=\"nr-note-edit\" aria-label=\"Editar nota\">{}</textarea>",
                e(note, "text")
            )
        } else if yes(at(note, "text")) {
            format!("<p>{}</p>", e(note, "text").replace('\n', "<br>"))
        } else {
            String::new()
        };
        let cite = if yes(at(note, "cite")) {
            format!("<cite>{}</cite>", e(note, "cite"))
        } else {
            String::new()
        };
        let actions = if editing {
            format!(
                "<button type=\"button\" data-flat class=\"nr-xs nr-saved\" data-note-save=\"{id}\">Guardar</button><button type=\"button\" data-flat class=\"nr-xs\" data-note-cancel=\"1\">Cancelar</button>"
            )
        } else {
            format!(
                "<button type=\"button\" data-flat class=\"nr-xs\" data-note-edit=\"{id}\">Editar</button>{}<button type=\"button\" data-flat class=\"nr-xs\" data-note-copy=\"{id}\">Copiar</button>{}",
                if with_story {
                    format!(
                        "<button type=\"button\" data-flat class=\"nr-xs\" data-goto=\"{}\" data-goto-story=\"{}\" data-goto-tab=\"notas\">Ir a la noticia</button>",
                        e(note, "editionId"),
                        e(note, "storyId")
                    )
                } else {
                    String::new()
                },
                if confirm {
                    format!(
                        "<button type=\"button\" data-flat class=\"nr-xs nr-danger\" data-note-delete=\"{id}\">¿Borrar? Sí</button><button type=\"button\" data-flat class=\"nr-xs\" data-note-cancel=\"1\">No</button>"
                    )
                } else {
                    format!(
                        "<button type=\"button\" data-flat class=\"nr-xs\" data-note-ask-delete=\"{id}\">Borrar</button>"
                    )
                }
            )
        };
        format!(
            "<div class=\"nr-note\" data-note=\"{id}\"><small>{meta}</small>{quote}{text}{cite}<div class=\"nr-note-acts\">{actions}</div></div>"
        )
    }
    fn notes(&self, story: &Value) -> String {
        let all = story.is_null() || field(self.state, "scope") == "todas";
        let own = at(self.state, "notes");
        let all_notes = at(self.state, "allNotes");
        if (!story.is_null() && (own.is_null() || at(own, "storyId") != at(story, "id")))
            || (all && at(all_notes, "notes").is_null())
        {
            return "<p class=\"nr-empty\">Cargando notas…</p>".into();
        }
        let mine = arr(at(own, "notes"));
        let total = n(at(all_notes, "total"));
        let scope = if story.is_null() {
            String::new()
        } else {
            format!(
                "<div class=\"nr-chips\"><button type=\"button\" data-scope=\"esta\" class=\"{}\">Esta noticia <i>{}</i></button><button type=\"button\" data-scope=\"todas\" class=\"{}\">Todas <i>{total}</i></button></div>",
                if all { "" } else { "on" },
                mine.len(),
                if all { "on" } else { "" }
            )
        };
        if !all {
            return scope
                + &if mine.is_empty() {
                    "<p class=\"nr-empty\">Sin notas en esta noticia. Escribe una abajo o guarda una burbuja del chat con ☆.</p>".into()
                } else {
                    mine.iter()
                        .map(|n| self.note_item(n, false))
                        .collect::<String>()
                }
                + &format!(
                    "<div class=\"nr-newnote\"><textarea class=\"nr-note-new\" placeholder=\"Nueva nota sobre esta noticia… (Ctrl+Enter guarda)\" aria-label=\"Nueva nota\">{}</textarea><div class=\"nr-chips\"><button type=\"button\" class=\"nr-pri\" data-note-add=\"1\">Guardar nota</button></div></div>",
                    e(self.state, "noteDraft")
                );
        }
        let groups = (self.groups)(at(all_notes, "notes"));
        let list = if arr(&groups).is_empty() {
            format!(
                "<p class=\"nr-empty\">{}</p>",
                if yes(at(self.state, "query")) {
                    "Ninguna nota coincide."
                } else {
                    "Aún no tienes notas."
                }
            )
        } else {
            arr(&groups)
                .iter()
                .map(|g| {
                    format!(
                        "<div class=\"nr-day-label\">{}</div>{}",
                        e(g, "label"),
                        arr(at(g, "notes"))
                            .iter()
                            .map(|n| self.note_item(n, true))
                            .collect::<String>()
                    )
                })
                .collect()
        };
        format!(
            "{scope}<div class=\"nr-search\"><input class=\"nr-notes-q\" type=\"search\" placeholder=\"Buscar en todas mis notas…\" value=\"{}\" aria-label=\"Buscar en mis notas\"><button type=\"button\" data-flat class=\"nr-xs\" data-notes-export=\"1\">Copiar como .md</button></div>{list}",
            e(self.state, "query")
        )
    }
}
