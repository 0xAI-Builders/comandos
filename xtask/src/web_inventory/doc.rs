//! Documento del inventario (`docs/research/2026-10-04-fase-3-web-inventario.md`),
//! generado entero a partir de `inventory.json` e `interop.json`.
use super::{Kind, Unit, same_realm};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// Cifras de cabecera.
pub struct Summary {
    pub units: usize,
    pub scripts: usize,
    pub regions: usize,
    pub lines: usize,
    /// Nombres distintos definidos a nivel superior o en `window`.
    pub globals: usize,
    /// Globales con algún consumidor fuera de su unidad (claves de interop).
    pub interop_globals: usize,
    /// Pares (unidad, unidad de la que depende).
    pub edges: usize,
    pub host_calls: usize,
    pub dynamic_host_calls: usize,
    pub risky: usize,
}

impl Summary {
    pub fn of(units: &[Unit], ix: &Value) -> Summary {
        let globals: BTreeSet<&String> = units.iter().flat_map(|u| &u.defines).collect();
        let interop_globals = ix
            .as_object()
            .map(|m| m.keys().filter(|k| !k.starts_with('@')).count())
            .unwrap_or(0);
        let host_calls = ix["@hosts"]
            .as_object()
            .map(|m| m.values().filter_map(|h| h["calls"].as_u64()).sum::<u64>())
            .unwrap_or(0);
        let risky = ix
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(k, v)| {
                        !k.starts_with('@') && v["risks"].as_array().is_some_and(|a| !a.is_empty())
                    })
                    .count()
            })
            .unwrap_or(0);
        Summary {
            units: units.len(),
            scripts: units.iter().filter(|u| u.kind == Kind::Script).count(),
            regions: units.iter().filter(|u| u.kind == Kind::Region).count(),
            lines: units.iter().map(|u| u.lines).sum(),
            globals: globals.len(),
            interop_globals,
            edges: dependencies(units).values().map(BTreeMap::len).sum(),
            host_calls: usize::try_from(host_calls).unwrap_or(usize::MAX),
            dynamic_host_calls: ix["@host_dynamic"].as_array().map(Vec::len).unwrap_or(0),
            risky,
        }
    }
}

/// Por unidad: unidad de la que depende → globales que toma de ella.
pub fn dependencies(units: &[Unit]) -> BTreeMap<String, BTreeMap<String, Vec<String>>> {
    let mut out: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for u in units {
        for name in u.uses_globals.iter().chain(&u.mutates_globals) {
            for v in units
                .iter()
                .filter(|v| v.id != u.id && same_realm(u, v) && v.defines.contains(name))
            {
                let names = out
                    .entry(u.id.clone())
                    .or_default()
                    .entry(v.id.clone())
                    .or_default();
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
    }
    out
}

fn list(v: &[String], max: usize) -> String {
    if v.is_empty() {
        return "—".to_string();
    }
    let shown: Vec<String> = v
        .iter()
        .take(max)
        .map(|s| format!("`{}`", cell(s)))
        .collect();
    let rest = v.len().saturating_sub(max);
    if rest > 0 {
        format!("{} (+{rest})", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

/// Texto seguro dentro de una celda de tabla.
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('`', "'").replace('\n', " ")
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn render(units: &[Unit], inv: &Value, ix: &Value) -> String {
    let s = Summary::of(units, ix);
    let mut d = String::new();
    let head = inv["source"]["head"].as_str().unwrap_or("desconocido");
    let dirty = strs(&inv["source"]["dirty"]);
    let _ = writeln!(
        d,
        "# Fase 3b — inventario de la interfaz del tablero (B3)\n"
    );
    let _ = writeln!(
        d,
        "Generado por `cargo run -p xtask -- web-inventory --out xtask/web/inventory.json --doc docs/research/2026-10-04-fase-3-web-inventario.md` \
         sobre el checkout principal (`{head}`). No se edita a mano: se regenera. Los datos completos están en \
         `xtask/web/inventory.json` (unidades) y `xtask/web/interop.json` (globales, host, iframes y mensajes)."
    );
    if !dirty.is_empty() {
        let _ = writeln!(
            d,
            "\nEl origen tenía cambios sin comitear en: {}. El inventario refleja el disco, no el commit.",
            list(&dirty, 12)
        );
    }
    let _ = writeln!(d, "\n## Resumen\n");
    let _ = writeln!(d, "| Cifra | Valor |\n|---|---|");
    for (k, v) in [
        ("Unidades", s.units),
        ("Scripts `dash/*.js`", s.scripts),
        ("Regiones de scripts en línea", s.regions),
        ("Líneas de JS", s.lines),
        ("Globales definidos (nombres distintos)", s.globals),
        (
            "Globales con consumidores fuera de su unidad (`interop.json`)",
            s.interop_globals,
        ),
        ("Dependencias entre unidades (aristas)", s.edges),
        ("Llamadas del host a la página", s.host_calls),
        ("…de ellas sin JS literal (dinámicas)", s.dynamic_host_calls),
        ("Globales con riesgo anotado", s.risky),
    ] {
        let _ = writeln!(d, "| {k} | {v} |");
    }

    let _ = writeln!(d, "\n## Páginas y orden de carga\n");
    if let Some(pages) = inv["pages"].as_object() {
        for (page, entries) in pages {
            let e = strs(entries);
            let _ = writeln!(
                d,
                "- `{page}`: {}",
                e.iter()
                    .map(|x| format!("`{}`", cell(x)))
                    .collect::<Vec<_>>()
                    .join(" → ")
            );
        }
    }
    let loose: Vec<String> = units
        .iter()
        .filter(|u| u.pages.is_empty())
        .map(|u| u.id.clone())
        .collect();
    if !loose.is_empty() {
        let _ = writeln!(
            d,
            "- Sin página que los cargue (su propio ámbito): {}",
            list(&loose, 20)
        );
    }

    let _ = writeln!(d, "\n## Unidades\n");
    let _ = writeln!(
        d,
        "«Usa» y «Muta» solo cuentan globales de otras unidades de la misma página. «Host» son los globales de la unidad que \
         llaman `cc-app`, `cc-app-mac` o un iframe.\n"
    );
    let _ = writeln!(
        d,
        "| Unidad | Línea | Líneas | Define | Usa de otras | Muta | Rutas | `localStorage` | Intervalos (ms) | Host |\n|---|---:|---:|---|---|---|---|---|---|---|"
    );
    for u in units {
        let hosted: Vec<String> = u
            .defines
            .iter()
            .filter(|n| {
                ix[n.as_str()]["called_by"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty())
            })
            .cloned()
            .collect();
        let iv: Vec<String> = u.intervals_ms.iter().map(u64::to_string).collect();
        let _ = writeln!(
            d,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            u.id,
            u.line_start,
            u.lines,
            list(&u.defines, 6),
            list(&u.uses_globals, 6),
            list(&u.mutates_globals, 4),
            list(&u.routes, 4),
            list(&u.storage_keys, 4),
            if iv.is_empty() {
                "—".to_string()
            } else {
                iv.join(", ")
            },
            list(&hosted, 4),
        );
    }
    let warned: Vec<&Unit> = units.iter().filter(|u| !u.warnings.is_empty()).collect();
    if !warned.is_empty() {
        let _ = writeln!(d, "\nAvisos del corte por marcadores:\n");
        for u in warned {
            let _ = writeln!(d, "- `{}`: {}", u.id, u.warnings.join("; "));
        }
    }

    let _ = writeln!(d, "\n## Dependencias de globales entre unidades\n");
    let _ = writeln!(d, "| Unidad | Depende de | Globales |\n|---|---|---|");
    for (u, deps) in dependencies(units) {
        for (v, names) in deps {
            let _ = writeln!(d, "| `{u}` | `{v}` | {} |", list(&names, 10));
        }
    }

    let _ = writeln!(d, "\n## Puente con el host\n");
    let _ = writeln!(
        d,
        "`bin/cc-app` ejecuta JS en la página con `run_javascript` (literales, `_dash_js(code)`, `_dash_js_quiet(code)` y \
         `_dash_click(elem_id)`, que envuelven `f\"try{{{{{{code}}}}}}…\"`); `bin/cc-app-mac` con `evaluateJavaScript`. \
         La página habla con la app por `window.webkit.messageHandlers.<nombre>.postMessage`.\n"
    );
    if let Some(h) = ix["@hosts"].as_object() {
        for (label, v) in h {
            let found = v["found"].as_bool().unwrap_or(false);
            let _ = writeln!(
                d,
                "- `{label}`: {}",
                if found {
                    format!("{} llamadas", v["calls"].as_u64().unwrap_or(0))
                } else {
                    "no encontrado".to_string()
                }
            );
        }
    }
    let _ = writeln!(
        d,
        "\n| Global | Definido en | Llamado por | Llamadas (archivo:línea vía) |\n|---|---|---|---|"
    );
    if let Some(m) = ix.as_object() {
        for (name, v) in m.iter().filter(|(k, _)| !k.starts_with('@')) {
            let callers = strs(&v["called_by"]);
            if !callers.iter().any(|c| c.starts_with("bin/")) {
                continue;
            }
            let calls: Vec<String> = v["host_calls"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|c| {
                            format!(
                                "{}:{} {}",
                                c["from"].as_str().unwrap_or(""),
                                c["line"],
                                c["via"].as_str().unwrap_or("")
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let _ = writeln!(
                d,
                "| `{name}` | {} | {} | {} |",
                list(&strs(&v["defined_by"]), 3),
                list(&callers, 3),
                list(&calls, 4)
            );
        }
    }
    if let Some(dom) = ix["@dom_ids"].as_object().filter(|m| !m.is_empty()) {
        let _ = writeln!(
            d,
            "\nIds del DOM que toca el host:\n\n| Id | Llamado por | En el marcado |\n|---|---|---|"
        );
        for (id, v) in dom {
            let _ = writeln!(
                d,
                "| `{id}` | {} | {} |",
                list(&strs(&v["called_by"]), 3),
                if v["in_markup"].as_bool().unwrap_or(false) {
                    "sí"
                } else {
                    "no (lo crea el JS)"
                }
            );
        }
    }
    if let Some(dy) = ix["@host_dynamic"].as_array().filter(|a| !a.is_empty()) {
        let _ = writeln!(
            d,
            "\nLlamadas con JS no literal (archivo, red o variable sin cadena): no se pueden inventariar.\n"
        );
        for c in dy {
            let _ = writeln!(
                d,
                "- `{}:{}` vía `{}`",
                c["from"].as_str().unwrap_or(""),
                c["line"],
                c["via"].as_str().unwrap_or("")
            );
        }
    }
    if let Some(h) = ix["@host_handlers"].as_object() {
        let _ = writeln!(
            d,
            "\nManejadores `messageHandlers`:\n\n| Manejador | Registrado por | Lo usan |\n|---|---|---|"
        );
        for (name, v) in h {
            let _ = writeln!(
                d,
                "| `{name}` | {} | {} |",
                list(&strs(&v["registered_by"]), 3),
                list(&strs(&v["posted_by"]), 8)
            );
        }
    }

    let _ = writeln!(d, "\n## Iframes y mensajes\n");
    let parents: Vec<&Unit> = units.iter().filter(|u| !u.parent_refs.is_empty()).collect();
    if !parents.is_empty() {
        let _ = writeln!(
            d,
            "Llamadas directas `parent.X` desde un iframe (solo ven propiedades de `window`, no `let`/`const` de nivel superior):\n"
        );
        for u in parents {
            let _ = writeln!(d, "- `{}` → {}", u.id, list(&u.parent_refs, 10));
        }
    }
    if let Some(m) = ix["@messages"].as_object() {
        let _ = writeln!(
            d,
            "\n| Mensaje (`source/type`) | Lo envía | Lo atiende (`.type ===`) |\n|---|---|---|"
        );
        for (k, v) in m {
            let _ = writeln!(
                d,
                "| `{}` | {} | {} |",
                cell(k),
                list(&strs(&v["sent_by"]), 4),
                list(&strs(&v["handled_by"]), 4)
            );
        }
    }

    let _ = writeln!(d, "\n## Globales de riesgo\n");
    if let Some(m) = ix.as_object() {
        for (name, v) in m.iter().filter(|(k, _)| !k.starts_with('@')) {
            let risks = strs(&v["risks"]);
            if risks.is_empty() {
                continue;
            }
            let _ = writeln!(
                d,
                "- `{name}` ({}): {}",
                list(&strs(&v["defined_by"]), 3),
                risks.join("; ")
            );
        }
    }

    let _ = writeln!(d, "\n## Orden de port (hojas primero)\n");
    let _ = writeln!(
        d,
        "Nivel 0 = no toma globales de ninguna otra unidad. Un grupo con varias unidades es un ciclo: se portan juntas \
         o una exporta por `window` lo que la otra aún lee de JS.\n"
    );
    if let Some(groups) = inv["port_order"].as_array() {
        let mut by_level: BTreeMap<u64, Vec<String>> = BTreeMap::new();
        for g in groups {
            let ids = strs(&g["units"]);
            let txt = if ids.len() > 1 {
                format!(
                    "[ciclo: {}]",
                    ids.iter()
                        .map(|x| format!("`{x}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                ids.iter()
                    .map(|x| format!("`{x}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            by_level
                .entry(g["level"].as_u64().unwrap_or(0))
                .or_default()
                .push(txt);
        }
        for (lv, items) in by_level {
            let _ = writeln!(d, "{}. Nivel {lv}: {}", lv + 1, items.join(", "));
        }
    }

    let _ = writeln!(d, "\n## Límites del inventario léxico\n");
    for l in LIMITS {
        let _ = writeln!(d, "- {l}");
    }
    d
}

const LIMITS: &[&str] = &[
    "No ejecuta JS: un tokenizador separa cadenas, plantillas, comentarios y expresiones regulares del código; `/` es regex o división según el token anterior, y un `}` seguido de `/regex/` en otra línea se leería como división.",
    "Ámbitos aproximados: bloques `{}`; parámetros de funciones, flechas, métodos y `catch` se declaran en el cuerpo; `var` se trata como `let` (no se eleva a la función) y los parámetros de una flecha sin llaves valen para todo el bloque que la contiene. Un nombre declarado en un bloque tapa el global en ese bloque y sus hijos.",
    "Globales: `function`, `class`, `const`/`let`/`var` fuera de todo paréntesis, corchete o llave, y `window.X =`, `root.X =`, `globalThis.X =`, `self.X =` en cualquier sitio. No ve `Object.assign(window, …)`, `window[\"X\"] =` ni `defineProperty`.",
    "Usos: identificadores libres (no tras `.`, no claves de objeto, no nombres de método) y `window.X`/`root.X`; se cruzan solo con unidades de la misma página. Un nombre usado y además declarado localmente en otro bloque cuenta como uso si alguna aparición queda libre.",
    "Rutas: primer argumento literal (o `const` de la unidad) de `api(`, `fetch(`, `sendBeacon(` y `new EventSource(`, sin la consulta; los huecos de plantilla quedan como `${}`. Las rutas construidas en variables o pasadas por parámetro no se ven.",
    "`localStorage`: `getItem/setItem/removeItem` con literal o `const`, `localStorage.clave` y `localStorage[\"clave\"]`; las claves de `sessionStorage` llevan `session:`. Una clave guardada en una propiedad (`storage.getItem(this.key)`) no se resuelve.",
    "Intervalos: segundo argumento de `setInterval` si es número, `const` numérica o producto/suma de ellos.",
    "Mensajes: `postMessage` con objeto literal (`source/type`) o `JSON.stringify({…})`; los tipos atendidos salen de comparaciones `.type === \"…\"`, que también recogen tipos de eventos del DOM.",
    "Host: tokenizador mínimo de Python. Los envoltorios se descubren cuando un `def` pasa su parámetro a `run_javascript`/`evaluateJavaScript` u otro envoltorio; un argumento variable se resuelve por su última asignación en el mismo `def`; código leído de archivo o de red queda como dinámico. Los huecos `{expr}` de las f-strings se sustituyen por `__py__`.",
    "Regiones: marcadores `// ---------- … ----------` en columna 0 del primer script en línea; los marcadores sangrados son subsecciones y no cortan. El hash de una región es el del texto desde `marker_start` hasta `marker_end` (excluido), igual que el corte del compositor (B2). Un marcador repetido dentro del script se avisa en la tabla.",
    "El DOM que construye el JS (plantillas, `innerHTML`) no se inventaría: para eso está `shots dom-dump` (B4).",
];
