//! Resource snapshot markup. Missing measurements stay unknown, never zero.
use crate::escape::text as esc;
use serde_json::Value;

pub fn tabs(selected: &str) -> String {
    [
        ("cuentas", "Cuentas"),
        ("comparar", "Comparar"),
        ("pomodoro", "Pomodoro"),
        ("recursos", "Recursos"),
    ]
    .into_iter()
    .map(|(id, label)| {
        format!(
            "<button class=\"tab {}\" role=\"tab\" aria-selected=\"{}\" data-tab=\"{id}\">{label}</button>",
            if selected == id { "on" } else { "" },
            selected == id
        )
    })
    .collect()
}

fn bytes(value: &Value) -> String {
    let Some(value) = value.as_u64() else {
        return "—".into();
    };
    format_bytes(value)
}

fn format_bytes(value: u64) -> String {
    let value = value as f64;
    for (unit, scale) in [
        ("TiB", 1_099_511_627_776_f64),
        ("GiB", 1_073_741_824_f64),
        ("MiB", 1_048_576_f64),
        ("KiB", 1024_f64),
    ] {
        if value >= scale {
            return format!("{:.2} {unit}", value / scale);
        }
    }
    format!("{value:.0} B")
}

fn field<'a>(value: &'a Value, name: &str) -> &'a Value {
    value.get(name).unwrap_or(&Value::Null)
}

fn label(value: &Value, name: &str) -> String {
    esc(field(value, name).as_str().unwrap_or("—"))
}

fn card(title: &str, amount: &str, detail: &str) -> String {
    format!(
        "<div class=\"resource-card\"><span>{title}</span><strong>{amount}</strong><small>{detail}</small></div>"
    )
}

pub fn html(snapshot: &Value, error: &str, loading: bool) -> String {
    let mut out = format!(
        "<div class=\"mhead\"><h2>Analytics</h2><div class=\"tabs\" role=\"tablist\">{}</div></div>{STYLE}<section class=\"resources\" aria-label=\"Recursos de ComandOS\"><div class=\"resource-toolbar\"><div><h3>RAM y almacenamiento</h3><p>Medición del equipo donde corre ComandOS.</p></div><button type=\"button\" data-resource-refresh {}>{}</button></div>",
        tabs("recursos"),
        if loading { "disabled" } else { "" },
        if loading { "Midiendo…" } else { "Actualizar" },
    );
    if !error.is_empty() {
        out += &format!(
            "<p class=\"resource-warning\" role=\"alert\">No pude actualizar los recursos: {}{}</p>",
            esc(error),
            if snapshot.is_object() {
                " Se conserva la última medición."
            } else {
                ""
            }
        );
    }
    if !snapshot.is_object() {
        out += if loading {
            "<p class=\"resource-note\" role=\"status\">Leyendo la memoria y el disco…</p>"
        } else {
            "<p class=\"resource-note\">No hay una medición disponible. Pulsa Actualizar para volver a intentarlo.</p>"
        };
        out += "</section>";
        return out;
    }
    if let Some(sampled) = field(snapshot, "sampledAt").as_u64() {
        out += &format!(
            "<p class=\"resource-note\">Última medición: <time data-resource-sampled-at=\"{sampled}\">{sampled}</time>. Se actualiza al abrir esta pestaña o pulsar Actualizar; caché de hasta 60 s.</p>"
        );
    }
    let warnings = field(snapshot, "warnings").as_array();
    if let Some(warnings) = warnings.filter(|items| !items.is_empty()) {
        out += "<ul class=\"resource-warning\" aria-label=\"Avisos de recursos\">";
        for warning in warnings.iter().filter_map(Value::as_str) {
            out += &format!("<li>{}</li>", esc(warning));
        }
        out += "</ul>";
    }
    let memory = field(snapshot, "memory");
    let groups = field(memory, "groups").as_array();
    let total_pss = groups.and_then(|groups| {
        groups.iter().try_fold(0_u64, |sum, group| {
            sum.checked_add(field(group, "pssBytes").as_u64().unwrap_or(0))
        })
    });
    let unreadable = field(memory, "unreadableProcesses").as_u64().unwrap_or(0);
    let partial = unreadable > 0
        || field(memory, "partial").as_bool() == Some(true)
        || groups.is_some_and(|groups| {
            groups.iter().any(|group| {
                field(group, "partial").as_bool() == Some(true)
                    || field(group, "pssBytes").as_u64().is_none()
            })
        });
    let measured = field(memory, "measuredProcesses").as_u64();
    let total = total_pss
        .filter(|_| measured.unwrap_or(0) > 0)
        .map(format_bytes)
        .unwrap_or_else(|| "—".into());
    let total = if partial && total != "—" {
        format!("≥ {total}")
    } else {
        total
    };
    out += "<div class=\"resource-cards\">";
    out += &card(
        "ComandOS y sesiones · RAM",
        &total,
        "PSS: memoria compartida repartida entre procesos",
    );
    out += &card(
        "RAM disponible del equipo",
        &bytes(field(memory, "availableBytes")),
        &format!("de {} instalados", bytes(field(memory, "totalBytes"))),
    );
    out += &card(
        "Swap usada del equipo",
        &bytes(field(memory, "swapUsedBytes")),
        "Incluye todas las aplicaciones del equipo",
    );
    out += "</div><h4>RAM por componente</h4><p class=\"resource-note\">Incluye Codex y Claude de este usuario, también los abiertos fuera de ComandOS, y sus herramientas. Cada proceso aparece en un solo grupo. La suma de PSS evita contar varias veces la memoria compartida.</p><div class=\"resource-table-wrap\"><table class=\"resource-table\"><thead><tr><th>Componente</th><th>Procesos</th><th>RAM · PSS</th></tr></thead><tbody>";
    if let Some(groups) = groups.filter(|groups| !groups.is_empty()) {
        for group in groups {
            let amount = bytes(field(group, "pssBytes"));
            let amount = if field(group, "partial").as_bool() == Some(true) && amount != "—" {
                format!("≥ {amount}")
            } else {
                amount
            };
            out += &format!(
                "<tr><th scope=\"row\">{}</th><td>{}</td><td>{}</td></tr>",
                label(group, "label"),
                field(group, "processCount")
                    .as_u64()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "—".into()),
                amount
            );
        }
    } else {
        out += "<tr><td colspan=\"3\">No hay procesos medidos.</td></tr>";
    }
    out += "</tbody></table></div>";
    out += &format!(
        "<p class=\"resource-note\">{} procesos medidos; {unreadable} no se pudieron leer.</p>",
        measured
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".into())
    );
    if partial {
        out += "<p class=\"resource-warning\">Medición de RAM parcial: el consumo real puede ser mayor.</p>";
    }
    let disk = field(snapshot, "disk");
    out += "<h4>Disco</h4><div class=\"resource-cards\">";
    out += &card(
        "Espacio disponible",
        &bytes(field(disk, "availableBytes")),
        &format!(
            "de {} en el disco de ComandOS",
            bytes(field(disk, "totalBytes"))
        ),
    );
    let guards = field(snapshot, "safeguards");
    out += &card(
        "Presupuesto de compilación",
        &bytes(field(guards, "buildCacheLimitBytes")),
        "La caché se retira al terminar la entrega",
    );
    out += &card(
        "Reserva mínima de disco",
        &bytes(field(guards, "minFreeDiskBytes")),
        "Se rechazan nuevas compilaciones por debajo de esta reserva",
    );
    out += "</div><p class=\"resource-note\">Las rutas pueden incluir otras filas; sus tamaños no se suman entre sí. Las rutas pertenecen al equipo donde corre ComandOS.</p><div class=\"resource-paths\">";
    if let Some(paths) = field(disk, "paths")
        .as_array()
        .filter(|paths| !paths.is_empty())
    {
        for path in paths {
            let partial = field(path, "partial").as_bool() == Some(true);
            out += &format!(
                "<div class=\"resource-path\"><div><b>{}</b><strong>{}{}</strong></div><code>{}</code>{}</div>",
                label(path, "label"),
                if partial && field(path, "bytes").as_u64().is_some() {
                    "≥ "
                } else {
                    ""
                },
                bytes(field(path, "bytes")),
                label(path, "path"),
                if partial {
                    "<small class=\"resource-warning\">Medición parcial; hay archivos sin contar.</small>"
                } else {
                    ""
                }
            );
        }
    } else {
        out += "<p class=\"resource-note\">No hay tamaños de carpetas disponibles.</p>";
    }
    out += "</div><p class=\"resource-note\">Los controles de compilación evitan acumular caché entre entregas. Esta vista no cierra sesiones ni limita automáticamente la RAM de Codex, Claude o sus herramientas.</p></section>";
    out
}

const STYLE: &str = r#"<style>
.an .resources{min-width:0}.an .resource-toolbar{display:flex;align-items:center;justify-content:space-between;gap:12px}.an .resource-toolbar h3{font-size:17px;margin:0 0 5px}.an .resource-toolbar p,.an .resource-note{font-size:12px;color:var(--mut);line-height:1.6;margin:8px 0 16px}.an .resource-toolbar button{flex:none;border:1px solid var(--line2);border-radius:8px;background:var(--panel2);color:var(--fg);padding:8px 12px;font:inherit;font-size:12px;cursor:pointer}.an .resource-toolbar button:disabled{opacity:.6;cursor:wait}.an .resource-toolbar button:hover:not(:disabled){border-color:var(--brand)}.an .resource-cards{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:12px}.an .resource-card{border:1px solid var(--line);border-radius:12px;background:var(--well);padding:16px;display:flex;flex-direction:column;gap:8px;min-width:0}.an .resource-card span{font-size:12px;color:var(--mut)}.an .resource-card strong{font:600 23px var(--mono);overflow-wrap:anywhere}.an .resource-card small{font-size:11px;color:var(--mut);line-height:1.5}.an .resources h4{font-size:14px;margin:24px 0 10px}.an .resource-table-wrap{overflow:auto}.an .resource-table{width:100%;border-collapse:collapse;font-size:12px}.an .resource-table th,.an .resource-table td{padding:10px 8px;border-bottom:1px solid var(--line);text-align:left}.an .resource-table thead th{color:var(--mut);font-weight:500}.an .resource-table td{text-align:right;font-family:var(--mono);white-space:nowrap}.an .resource-table thead th:not(:first-child){text-align:right}.an .resource-table tbody th{font-weight:500}.an .resource-warning{font-size:12px;color:var(--warn);line-height:1.6}.an ul.resource-warning{background:var(--well);border:1px solid var(--line);border-radius:8px;padding:12px 12px 12px 28px}.an .resource-path{border-bottom:1px solid var(--line);padding:12px 0;min-width:0}.an .resource-path>div{display:flex;justify-content:space-between;gap:12px;font-size:12px}.an .resource-path strong{font-family:var(--mono);white-space:nowrap}.an .resource-path code{display:block;overflow-wrap:anywhere;white-space:normal;user-select:text;font:11px/1.6 var(--mono);color:var(--mut);margin-top:5px}.an .resource-path small{display:block;margin-top:5px}.an.phone .resource-cards{grid-template-columns:1fr}.an.phone .tab{padding:6px 7px;font-size:11.5px}.an.phone .resource-toolbar{align-items:flex-start}.an.phone .resource-table th,.an.phone .resource-table td{padding:9px 4px}
</style>"#;
