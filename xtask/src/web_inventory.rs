//! Inventario léxico de la interfaz del tablero (Fase 3b, B3).
//!
//! Recorre `dash/index.html` (regiones del primer script en línea por los
//! marcadores `// ---------- … ----------` y el segundo script en línea como
//! `tail`), `dash/term.html` (mismo criterio) y `dash/*.js`, y anota por
//! unidad los globales que define, usa y muta, rutas, ids del DOM, claves de
//! `localStorage`, intervalos y mensajes. `interop` cruza esas unidades con
//! las llamadas del host (`bin/cc-app`, `bin/cc-app-mac`) y de los iframes.
//!
//! Es un análisis estático con heurísticas (sin motor de JS); los límites
//! están en `docs/research/2026-10-04-fase-3-web-inventario.md`.
pub mod analyze;
pub mod doc;
pub mod host;
pub mod js_lex;

use analyze::Facts;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Script,
    Region,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Script => "script",
            Kind::Region => "region",
        }
    }
}

/// Una pieza portable de la interfaz: un archivo `dash/*.js` o una región
/// de un script en línea.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// `script:<archivo>`, `region:<nombre>` (index.html) o `term:<nombre>`.
    pub id: String,
    pub kind: Kind,
    /// Id sugerido para `components.json` (`quick-terminal`, `red`, `term-main`).
    pub component: String,
    /// Ruta relativa al repositorio.
    pub source: String,
    /// Páginas que la cargan (`index.html`, `term.html`): comparten globales.
    pub pages: Vec<String>,
    /// Para regiones: qué script en línea de la página (1 = el primero).
    pub script_index: Option<usize>,
    pub marker_start: Option<String>,
    pub marker_end: Option<String>,
    /// SHA-256 del texto de la unidad (archivo entero, o `marker_start..marker_end`).
    pub sha256: String,
    pub lines: usize,
    /// Línea donde empieza en `source`.
    pub line_start: usize,
    pub defines: Vec<String>,
    pub define_kinds: BTreeMap<String, String>,
    /// Globales de otras unidades de la misma página que lee o llama.
    pub uses_globals: Vec<String>,
    /// Globales de otras unidades que reasigna (`X = …`, `X++`).
    pub mutates_globals: Vec<String>,
    /// `parent.X` desde un iframe (preflight B3: llamadas directas al tablero).
    pub parent_refs: Vec<String>,
    /// Globales de otra unidad que reasigna con `window.X =` (parche, no definición).
    pub patches: Vec<String>,
    /// Lo que lee del iframe: `contentWindow.X`, `win.X` con `win = ….contentWindow`,
    /// `contentDocument` (como `document`), `frames[…]`.
    pub child_refs: Vec<String>,
    /// Propiedades que escribe en el `window` de un iframe.
    pub child_writes: Vec<String>,
    /// `window.X =` con guarda (`if(!window.X)`, `??=`, `||=`) sobre un
    /// global que otra unidad ya crea: respaldo, no parche.
    pub fallbacks: Vec<String>,
    /// Toca el `document` de un iframe (`contentDocument`, `win.document`).
    pub child_dom_access: bool,
    pub dom_ids: Vec<String>,
    pub routes: Vec<String>,
    pub storage_keys: Vec<String>,
    pub post_messages: Vec<String>,
    /// Valores comparados con `.type ===` (mensajes que atiende).
    pub message_types: Vec<String>,
    pub intervals_ms: Vec<u64>,
    /// Literales de cadena y plantilla.
    pub strings: usize,
    pub warnings: Vec<String>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Dónde están los archivos: el repositorio real (`dash/`, `bin/`) o un
/// directorio de fixtures plano (`index.html`, `a.js`, `cc-app.py.txt`).
#[derive(Debug, Clone)]
pub struct Layout {
    pub root: PathBuf,
    pub dash: PathBuf,
    /// Prefijo de `source` (`dash/` o vacío).
    pub prefix: String,
    /// (etiqueta lógica, archivo).
    pub hosts: Vec<(String, PathBuf)>,
}

impl Layout {
    pub fn detect(repo: &Path) -> Layout {
        if repo.join("dash/index.html").is_file() {
            Layout {
                root: repo.to_path_buf(),
                dash: repo.join("dash"),
                prefix: "dash/".to_string(),
                hosts: vec![
                    ("bin/cc-app".to_string(), repo.join("bin/cc-app")),
                    ("bin/cc-app-mac".to_string(), repo.join("bin/cc-app-mac")),
                ],
            }
        } else {
            Layout {
                root: repo.to_path_buf(),
                dash: repo.to_path_buf(),
                prefix: String::new(),
                hosts: vec![
                    ("bin/cc-app".to_string(), repo.join("cc-app.py.txt")),
                    ("bin/cc-app-mac".to_string(), repo.join("cc-app-mac.py.txt")),
                ],
            }
        }
    }
}

/// Páginas con scripts en línea: (archivo, prefijo de id, prefijo de componente).
const PAGES: &[(&str, &str, &str)] = &[
    ("index.html", "region:", ""),
    ("term.html", "term:", "term-"),
];

/// Etiqueta `<script>` de una página.
#[derive(Debug, Clone)]
pub struct ScriptTag {
    pub src: Option<String>,
    /// Cuerpo de un script en línea (offsets de bytes en el HTML).
    pub body: Option<(usize, usize)>,
}

/// Etiquetas `<script>` en orden, saltando comentarios HTML.
pub fn script_tags(html: &str) -> Vec<ScriptTag> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let rest = html.get(pos..).unwrap_or("");
        let next_script = rest.find("<script");
        let next_comment = rest.find("<!--");
        match (next_script, next_comment) {
            (Some(s), Some(c)) if c < s => {
                let after = pos + c + 4;
                match html.get(after..).and_then(|r| r.find("-->")) {
                    Some(e) => pos = after + e + 3,
                    None => break,
                }
            }
            (Some(s), _) => {
                let tag_start = pos + s;
                let Some(tag_len) = html.get(tag_start..).and_then(|r| r.find('>')) else {
                    break;
                };
                let tag = html.get(tag_start..tag_start + tag_len).unwrap_or("");
                let body_start = tag_start + tag_len + 1;
                let Some(close) = html.get(body_start..).and_then(|r| r.find("</script>")) else {
                    break;
                };
                let body_end = body_start + close;
                let src = attr(tag, "src");
                out.push(ScriptTag {
                    body: if src.is_none() {
                        Some((body_start, body_end))
                    } else {
                        None
                    },
                    src,
                });
                pos = body_end + "</script>".len();
            }
            (None, _) => break,
        }
    }
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    for q in ['"', '\''] {
        let pat = format!(" {name}={q}");
        if let Some(i) = tag.find(&pat) {
            let rest = tag.get(i + pat.len()..)?;
            let end = rest.find(q)?;
            return rest.get(..end).map(str::to_string);
        }
    }
    None
}

/// Nombre de archivo de un `src` (`/x.js?v=2`, `../x.js` → `x.js`).
fn src_basename(src: &str) -> &str {
    let path = src.split(['?', '#']).next().unwrap_or("");
    path.rsplit('/').next().unwrap_or(path)
}

/// Texto `marker_start..marker_end` dentro de un script en línea (como lo
/// corta el compositor de B2). `marker_end == "</script>"` llega al final
/// del script. `script` (1-based) limita la búsqueda a ese script en línea.
pub fn region_text<'a>(
    html: &'a str,
    marker_start: &str,
    marker_end: &str,
    script: Option<usize>,
) -> Option<&'a str> {
    let bodies = script_tags(html).into_iter().filter_map(|t| t.body);
    for (n, (s, e)) in bodies.enumerate() {
        if script.is_some_and(|k| k != n + 1) {
            continue;
        }
        let body = html.get(s..e)?;
        let Some(p) = body.find(marker_start) else {
            continue;
        };
        let end = if marker_end == "</script>" {
            body.len()
        } else {
            let from = p + marker_start.len();
            from + body.get(from..)?.find(marker_end)?
        };
        return body.get(p..end);
    }
    None
}

/// Identificador corto de un marcador: hasta `:` o `(`, sin acentos.
fn slug(title: &str) -> String {
    let cut = title.split([':', '(']).next().unwrap_or(title);
    let mut s = String::new();
    for ch in cut.trim().chars().flat_map(char::to_lowercase) {
        let c = match ch {
            'á' | 'à' | 'ä' => 'a',
            'é' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'ï' => 'i',
            'ó' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            'ñ' => 'n',
            c if c.is_ascii_alphanumeric() => c,
            _ => '-',
        };
        if c == '-' && (s.is_empty() || s.ends_with('-')) {
            continue;
        }
        s.push(c);
    }
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() {
        "region".to_string()
    } else {
        s
    }
}

fn marker_title(line: &str) -> &str {
    line.trim_start_matches('/')
        .trim()
        .trim_start_matches('-')
        .trim_end_matches('-')
        .trim()
}

fn is_marker(line: &str) -> bool {
    line.starts_with("// ----------")
}

/// Región antes de analizar: offsets absolutos en el HTML.
struct RawRegion {
    name: String,
    script_index: usize,
    start: usize,
    end: usize,
    marker_start: String,
    marker_end: String,
}

/// Primera línea no vacía desde `from` (sin el blanco inicial ni el salto).
fn first_line(body: &str) -> Option<(usize, String)> {
    let off = body.find(|c: char| !c.is_whitespace())?;
    let line = body.get(off..)?.lines().next()?.trim_end().to_string();
    Some((off, line))
}

fn regions_of(html: &str) -> Vec<RawRegion> {
    let mut out = Vec::new();
    let bodies: Vec<(usize, usize)> = script_tags(html)
        .into_iter()
        .filter_map(|t| t.body)
        .collect();
    for (n, &(s, e)) in bodies.iter().enumerate() {
        let Some(body) = html.get(s..e) else { continue };
        let Some((first_off, first)) = first_line(body) else {
            continue;
        };
        if n > 0 {
            out.push(RawRegion {
                name: if n == 1 {
                    "tail".to_string()
                } else {
                    format!("tail-{}", n + 1)
                },
                script_index: n + 1,
                start: s + first_off,
                end: e,
                marker_start: first,
                marker_end: "</script>".to_string(),
            });
            continue;
        }
        // Marcadores en columna 0 del primer script.
        let mut markers: Vec<(usize, String)> = Vec::new();
        let mut off = 0;
        for line in body.split_inclusive('\n') {
            if is_marker(line) {
                markers.push((off, line.trim_end().to_string()));
            }
            off += line.len();
        }
        let Some((m0, m0_line)) = markers.first().cloned() else {
            out.push(RawRegion {
                name: "main".to_string(),
                script_index: 1,
                start: s + first_off,
                end: e,
                marker_start: first,
                marker_end: "</script>".to_string(),
            });
            continue;
        };
        if first_off < m0 {
            out.push(RawRegion {
                name: "prelude".to_string(),
                script_index: 1,
                start: s + first_off,
                end: s + m0,
                marker_start: first,
                marker_end: m0_line,
            });
        }
        for (k, (m_off, m_line)) in markers.iter().enumerate() {
            let next = markers.get(k + 1);
            out.push(RawRegion {
                name: slug(marker_title(m_line)),
                script_index: 1,
                start: s + m_off,
                end: next.map(|(o, _)| s + o).unwrap_or(e),
                marker_start: m_line.clone(),
                marker_end: next
                    .map(|(_, l)| l.clone())
                    .unwrap_or_else(|| "</script>".to_string()),
            });
        }
    }
    out
}

/// Unidad sin cruzar con las demás (antes de resolver usos entre unidades).
struct Draft {
    unit: Unit,
    facts: Facts,
}

fn line_of(text: &str, offset: usize) -> usize {
    text.get(..offset)
        .map(|p| p.matches('\n').count() + 1)
        .unwrap_or(1)
}

fn draft(
    id: String,
    kind: Kind,
    component: String,
    source: String,
    text: &str,
    line_start: usize,
    markers: Option<(usize, String, String)>,
) -> Draft {
    let facts = analyze::analyze(text);
    let (script_index, marker_start, marker_end) = match markers {
        Some((i, a, b)) => (Some(i), Some(a), Some(b)),
        None => (None, None, None),
    };
    let unit = Unit {
        id,
        kind,
        component,
        source,
        pages: Vec::new(),
        script_index,
        marker_start,
        marker_end,
        sha256: sha256_hex(text.as_bytes()),
        lines: text.lines().count(),
        line_start,
        defines: facts.defines.iter().map(|(n, _)| n.clone()).collect(),
        define_kinds: facts
            .defines
            .iter()
            .map(|(n, b)| (n.clone(), b.as_str().to_string()))
            .collect(),
        uses_globals: Vec::new(),
        mutates_globals: Vec::new(),
        parent_refs: sorted(facts.parent_refs.clone()),
        patches: Vec::new(),
        child_refs: sorted(facts.child_refs.clone()),
        child_writes: sorted(facts.child_writes.clone()),
        fallbacks: Vec::new(),
        child_dom_access: facts.child_dom_access,
        dom_ids: facts.dom_ids.clone(),
        routes: facts.routes.clone(),
        storage_keys: facts.storage_keys.clone(),
        post_messages: facts.post_messages.clone(),
        message_types: facts.message_types.clone(),
        intervals_ms: facts.intervals_ms.clone(),
        strings: facts.strings,
        warnings: Vec::new(),
    };
    Draft { unit, facts }
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

/// Lo que se lee de disco para una página: sus entradas en orden.
#[derive(Debug, Clone)]
pub enum PageEntry {
    Unit(String),
    External(String),
}

struct Scan {
    drafts: Vec<Draft>,
    pages: Vec<(String, Vec<PageEntry>)>,
}

fn dash_scripts(layout: &Layout) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(&layout.dash)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.ends_with(".js"))
                .collect()
        })
        .unwrap_or_default();
    for vendor in [
        "vendor/markdown-it-15.0.2.umd.min.js",
        "vendor/purify-3.4.16.min.js",
    ] {
        if layout.dash.join(vendor).is_file() {
            v.push(vendor.into());
        }
    }
    v.sort();
    v
}

fn script_draft(layout: &Layout, name: &str) -> Option<Draft> {
    let bytes = std::fs::read(layout.dash.join(name)).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut d = draft(
        format!("script:{name}"),
        Kind::Script,
        match name {
            "vendor/markdown-it-15.0.2.umd.min.js" => "vendor-markdown-it",
            "vendor/purify-3.4.16.min.js" => "vendor-purify",
            _ => name.trim_end_matches(".js"),
        }
        .to_string(),
        format!("{}{name}", layout.prefix),
        &text,
        1,
        None,
    );
    // El hash es el de los bytes del archivo (lo que lee el servidor).
    d.unit.sha256 = sha256_hex(&bytes);
    Some(d)
}

fn scan_all(repo: &Path) -> Scan {
    let layout = Layout::detect(repo);
    let scripts = dash_scripts(&layout);
    let mut drafts: Vec<Draft> = Vec::new();
    let mut index_of: BTreeMap<String, usize> = BTreeMap::new();
    let mut pages = Vec::new();
    for (page, id_prefix, comp_prefix) in PAGES {
        let Ok(html) = std::fs::read_to_string(layout.dash.join(page)) else {
            continue;
        };
        let mut entries = Vec::new();
        let regions = regions_of(&html);
        let mut used_names: BTreeMap<String, usize> = BTreeMap::new();
        let mut inline_n = 0;
        for tag in script_tags(&html) {
            if let Some(src) = &tag.src {
                let path = src
                    .split(['?', '#'])
                    .next()
                    .unwrap_or(src)
                    .trim_start_matches('/');
                let base = if scripts.iter().any(|s| s == path) {
                    path.to_owned()
                } else {
                    src_basename(src).to_string()
                };
                if !scripts.contains(&base) {
                    entries.push(PageEntry::External(src.clone()));
                    continue;
                }
                let idx = match index_of.get(&base) {
                    Some(&i) => i,
                    None => {
                        let Some(d) = script_draft(&layout, &base) else {
                            continue;
                        };
                        drafts.push(d);
                        index_of.insert(base.clone(), drafts.len() - 1);
                        drafts.len() - 1
                    }
                };
                if let Some(d) = drafts.get_mut(idx) {
                    if !d.unit.pages.iter().any(|p| p == page) {
                        d.unit.pages.push(page.to_string());
                    }
                    entries.push(PageEntry::Unit(d.unit.id.clone()));
                }
                continue;
            }
            inline_n += 1;
            for r in regions.iter().filter(|r| r.script_index == inline_n) {
                let count = used_names.entry(r.name.clone()).or_insert(0);
                *count += 1;
                let name = if *count > 1 {
                    format!("{}-{}", r.name, count)
                } else {
                    r.name.clone()
                };
                let positional = html.get(r.start..r.end).unwrap_or("");
                let cut = region_text(&html, &r.marker_start, &r.marker_end, Some(r.script_index));
                let text = cut.unwrap_or(positional);
                let mut d = draft(
                    format!("{id_prefix}{name}"),
                    Kind::Region,
                    format!("{comp_prefix}{name}"),
                    format!("{}{page}", layout.prefix),
                    text,
                    line_of(&html, r.start),
                    Some((r.script_index, r.marker_start.clone(), r.marker_end.clone())),
                );
                if cut != Some(positional) {
                    d.unit.warnings.push(
                        "marcador ambiguo: el corte por marcadores no coincide con la región"
                            .to_string(),
                    );
                }
                d.unit.pages.push(page.to_string());
                entries.push(PageEntry::Unit(d.unit.id.clone()));
                drafts.push(d);
            }
        }
        pages.push((page.to_string(), entries));
    }
    for name in &scripts {
        if index_of.contains_key(name) {
            continue;
        }
        if let Some(d) = script_draft(&layout, name) {
            drafts.push(d);
        }
    }
    unique_components(&mut drafts);
    resolve(&mut drafts);
    Scan { drafts, pages }
}

/// Un id de componente por unidad (un archivo `components/<id>.json` cada
/// uno). Si una región choca con otro componente (p. ej. la región
/// `analytics` y `dash/analytics.js`), la región pasa a `<nombre>-inline`; si
/// aún chocara, se numera.
fn unique_components(drafts: &mut [Draft]) {
    let mut taken: BTreeSet<String> = drafts
        .iter()
        .filter(|d| d.unit.kind != Kind::Region)
        .map(|d| d.unit.component.clone())
        .collect();
    for d in drafts.iter_mut().filter(|d| d.unit.kind == Kind::Region) {
        let base = d.unit.component.clone();
        let mut name = base.clone();
        if taken.contains(&name) {
            name = format!("{base}-inline");
            let mut n = 2;
            while taken.contains(&name) {
                name = format!("{base}-inline-{n}");
                n += 1;
            }
        }
        taken.insert(name.clone());
        d.unit.component = name;
    }
}

/// Dos unidades comparten globales si alguna página carga las dos.
pub fn same_realm(u: &Unit, v: &Unit) -> bool {
    u.pages.iter().any(|p| v.pages.contains(p))
}

/// Usos y mutaciones entre unidades de la misma página.
fn resolve(drafts: &mut [Draft]) {
    // Parches: `window.X =` sobre un global que ya define otra unidad de la
    // misma página (con declaración propia, o antes en el orden de carga).
    let before: Vec<Unit> = drafts.iter().map(|d| d.unit.clone()).collect();
    for (i, d) in drafts.iter_mut().enumerate() {
        let Some(me) = before.get(i) else { continue };
        let patched: Vec<String> = me
            .defines
            .iter()
            .filter(|n| me.define_kinds.get(*n).is_some_and(|k| k == "window"))
            .filter(|n| {
                before.iter().enumerate().any(|(j, v)| {
                    j != i
                        && same_realm(me, v)
                        && v.define_kinds
                            .get(*n)
                            .is_some_and(|k| k != "window" || j < i)
                })
            })
            .cloned()
            .collect();
        d.unit.defines.retain(|n| !patched.contains(n));
        for n in &patched {
            d.unit.define_kinds.remove(n);
        }
        // Con guarda solo actúa si falta: respaldo, no parche.
        let (fallbacks, patches): (Vec<String>, Vec<String>) = patched
            .into_iter()
            .partition(|n| d.facts.guarded.contains(n));
        d.unit.fallbacks = fallbacks;
        d.unit.patches = patches;
    }
    let units: Vec<Unit> = drafts.iter().map(|d| d.unit.clone()).collect();
    for i in 0..drafts.len() {
        let Some(me) = units.get(i) else { continue };
        let foreign: BTreeSet<&String> = units
            .iter()
            .enumerate()
            .filter(|(j, v)| *j != i && same_realm(me, v))
            .flat_map(|(_, v)| &v.defines)
            .collect();
        let own: BTreeSet<String> = me.defines.iter().cloned().collect();
        let Some(d) = drafts.get_mut(i) else { continue };
        let uses: BTreeSet<String> = d
            .facts
            .uses
            .iter()
            .map(|u| &u.name)
            .filter(|n| foreign.contains(n) && !own.contains(*n))
            .cloned()
            .collect();
        let muts: BTreeSet<String> = d
            .facts
            .assigns
            .iter()
            .chain(&me.patches)
            .filter(|n| foreign.contains(n) && !own.contains(*n))
            .cloned()
            .collect();
        d.unit.uses_globals = uses.into_iter().collect();
        d.unit.mutates_globals = muts.into_iter().collect();
    }
}

/// Inventario de `repo`: unidades en el orden en que las cargan las páginas
/// (index.html, luego term.html) y después los `dash/*.js` sueltos.
pub fn scan(repo: &Path) -> Vec<Unit> {
    scan_all(repo).drafts.into_iter().map(|d| d.unit).collect()
}

/// Grupos de unidades en orden de port: primero las que no dependen de
/// ninguna otra (hojas); un ciclo forma un solo grupo.
pub fn port_order(units: &[Unit]) -> Vec<Vec<String>> {
    let n = units.len();
    // Arista i → j si i usa o muta un global que define j (misma página).
    let mut edges: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    for (i, u) in units.iter().enumerate() {
        let needs: BTreeSet<&String> = u.uses_globals.iter().chain(&u.mutates_globals).collect();
        for (j, v) in units.iter().enumerate() {
            if i != j
                && same_realm(u, v)
                && v.defines.iter().any(|d| needs.contains(d))
                && let Some(e) = edges.get_mut(i)
            {
                e.insert(j);
            }
        }
    }
    let sccs = tarjan(&edges);
    // Nivel = 1 + el mayor nivel de sus dependencias.
    let comp_of: BTreeMap<usize, usize> = sccs
        .iter()
        .enumerate()
        .flat_map(|(c, g)| g.iter().map(move |&i| (i, c)))
        .collect();
    let mut level = vec![0usize; sccs.len()];
    for (c, g) in sccs.iter().enumerate() {
        let lv = g
            .iter()
            .flat_map(|&i| edges.get(i).into_iter().flatten())
            .filter_map(|j| comp_of.get(j))
            .filter(|&&d| d != c)
            .filter_map(|&d| level.get(d).map(|l| l + 1))
            .max()
            .unwrap_or(0);
        if let Some(l) = level.get_mut(c) {
            *l = lv;
        }
    }
    let mut order: Vec<usize> = (0..sccs.len()).collect();
    order.sort_by_key(|&c| {
        (
            level.get(c).copied().unwrap_or(0),
            sccs.get(c)
                .and_then(|g| g.iter().min().copied())
                .unwrap_or(0),
        )
    });
    order
        .into_iter()
        .filter_map(|c| sccs.get(c))
        .map(|g| {
            let mut g = g.clone();
            g.sort();
            g.iter()
                .filter_map(|&i| units.get(i))
                .map(|u| u.id.clone())
                .collect()
        })
        .collect()
}

/// Nivel de cada grupo de `port_order` (0 = hoja).
pub fn port_levels(units: &[Unit], order: &[Vec<String>]) -> Vec<usize> {
    let mut level: BTreeMap<&str, usize> = BTreeMap::new();
    let by_id: BTreeMap<&str, &Unit> = units.iter().map(|u| (u.id.as_str(), u)).collect();
    let mut out = Vec::new();
    for g in order {
        let mut lv = 0;
        for id in g {
            let Some(u) = by_id.get(id.as_str()) else {
                continue;
            };
            let needs: BTreeSet<&String> =
                u.uses_globals.iter().chain(&u.mutates_globals).collect();
            for v in units {
                if g.contains(&v.id)
                    || !same_realm(u, v)
                    || !v.defines.iter().any(|d| needs.contains(d))
                {
                    continue;
                }
                if let Some(l) = level.get(v.id.as_str()) {
                    lv = lv.max(l + 1);
                }
            }
        }
        for id in g {
            level.insert(id.as_str(), lv);
        }
        out.push(lv);
    }
    out
}

fn tarjan(edges: &[BTreeSet<usize>]) -> Vec<Vec<usize>> {
    struct St<'a> {
        edges: &'a [BTreeSet<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        out: Vec<Vec<usize>>,
    }
    fn visit(s: &mut St, v: usize) {
        if let Some(x) = s.index.get_mut(v) {
            *x = Some(s.next);
        }
        if let Some(x) = s.low.get_mut(v) {
            *x = s.next;
        }
        s.next += 1;
        s.stack.push(v);
        if let Some(x) = s.on.get_mut(v) {
            *x = true;
        }
        let succ: Vec<usize> = s
            .edges
            .get(v)
            .map(|e| e.iter().copied().collect())
            .unwrap_or_default();
        for w in succ {
            match s.index.get(w).copied().flatten() {
                None => {
                    visit(s, w);
                    let lw = s.low.get(w).copied().unwrap_or(usize::MAX);
                    if let Some(x) = s.low.get_mut(v) {
                        *x = (*x).min(lw);
                    }
                }
                Some(iw) if s.on.get(w).copied().unwrap_or(false) => {
                    if let Some(x) = s.low.get_mut(v) {
                        *x = (*x).min(iw);
                    }
                }
                Some(_) => {}
            }
        }
        if s.low.get(v) == s.index.get(v).copied().flatten().as_ref() {
            let mut comp = Vec::new();
            while let Some(w) = s.stack.pop() {
                if let Some(x) = s.on.get_mut(w) {
                    *x = false;
                }
                comp.push(w);
                if w == v {
                    break;
                }
            }
            s.out.push(comp);
        }
    }
    let n = edges.len();
    let mut s = St {
        edges,
        index: vec![None; n],
        low: vec![0; n],
        on: vec![false; n],
        stack: Vec::new(),
        next: 0,
        out: Vec::new(),
    };
    for v in 0..n {
        if s.index.get(v).copied().flatten().is_none() {
            visit(&mut s, v);
        }
    }
    s.out
}

/// Nombres del navegador que el host o los iframes pueden usar sin que una
/// unidad los defina.
const BUILTINS: &[&str] = &[
    "document",
    "window",
    "console",
    "setTimeout",
    "setInterval",
    "clearTimeout",
    "clearInterval",
    "getComputedStyle",
    "JSON",
    "Object",
    "Array",
    "Math",
    "Date",
    "Promise",
    "location",
    "navigator",
    "localStorage",
    "sessionStorage",
    "fetch",
    "String",
    "Number",
    "Boolean",
    "parseInt",
    "parseFloat",
    "encodeURIComponent",
    "decodeURIComponent",
    "requestAnimationFrame",
    "alert",
    "history",
    "performance",
    "URL",
    "URLSearchParams",
    "Error",
    "Map",
    "Set",
    "CustomEvent",
    "Event",
    "postMessage",
    "webkit",
    "dispatchEvent",
    "addEventListener",
    "focus",
    "scrollTo",
    host::PY_HOLE,
];

fn push_str(map: &mut Map<String, Value>, key: &str, v: &str) {
    let arr = map.entry(key.to_string()).or_insert_with(|| json!([]));
    if let Some(a) = arr.as_array_mut()
        && !a.iter().any(|x| x == v)
    {
        a.push(json!(v));
    }
}

/// Interoperabilidad por nombre global: quién lo define, quién lo usa o
/// muta entre unidades, y quién lo llama desde fuera (host o iframe). Claves
/// con `@` (no son identificadores JS): `@dom_ids`, `@messages`,
/// `@host_handlers`, `@host_dynamic`, `@hosts`.
pub fn interop(units: &[Unit], repo: &Path) -> Value {
    let layout = Layout::detect(repo);
    let mut g: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let mut special = Map::new();
    let defined: BTreeMap<&str, Vec<&Unit>> = units.iter().fold(BTreeMap::new(), |mut m, u| {
        for d in &u.defines {
            m.entry(d.as_str()).or_insert_with(Vec::new).push(u);
        }
        m
    });
    let entry = |g: &mut BTreeMap<String, Map<String, Value>>, name: &str| {
        g.entry(name.to_string()).or_insert_with(|| {
            let mut m = Map::new();
            let by: Vec<&str> = defined
                .get(name)
                .map(|v| v.iter().map(|u| u.id.as_str()).collect())
                .unwrap_or_default();
            let mut kinds: Vec<&str> = defined
                .get(name)
                .map(|v| {
                    v.iter()
                        .filter_map(|u| u.define_kinds.get(name).map(String::as_str))
                        .collect()
                })
                .unwrap_or_default();
            kinds.sort();
            kinds.dedup();
            m.insert("defined_by".into(), json!(by));
            m.insert("binding".into(), json!(kinds));
            for k in [
                "patched_by",
                "fallback_by",
                "used_by",
                "mutated_by",
                "called_by",
                "host_calls",
                "risks",
            ] {
                m.insert(k.into(), json!([]));
            }
            m
        });
    };
    // Parches y colisiones dentro de una página: siempre en el inventario.
    for u in units {
        for (names, key) in [(&u.patches, "patched_by"), (&u.fallbacks, "fallback_by")] {
            for n in names {
                entry(&mut g, n);
                if let Some(m) = g.get_mut(n) {
                    push_str(m, key, &u.id);
                }
            }
        }
    }
    for (name, by) in &defined {
        let collide = by
            .iter()
            .enumerate()
            .any(|(a, u)| by.iter().skip(a + 1).any(|v| same_realm(u, v)));
        if collide {
            entry(&mut g, name);
            if let Some(m) = g.get_mut(*name) {
                m.insert("collides".into(), json!(true));
            }
        }
    }
    // Contrato padre → iframe: lo que el tablero lee o escribe en el
    // `window` de un iframe.
    let mut frame: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    for u in units {
        for (props, key) in [(&u.child_refs, "read_by"), (&u.child_writes, "written_by")] {
            for p in props {
                push_str(frame.entry(p.clone()).or_default(), key, &u.id);
            }
        }
    }
    for (p, m) in frame.iter_mut() {
        // Definido en otra página (el iframe), no en la del lector.
        let readers: Vec<&Unit> = units
            .iter()
            .filter(|u| u.child_refs.contains(p) || u.child_writes.contains(p))
            .collect();
        let defs: Vec<&Unit> = defined
            .get(p.as_str())
            .map(|v| {
                v.iter()
                    .filter(|d| readers.iter().all(|r| !same_realm(r, d)))
                    .copied()
                    .collect()
            })
            .unwrap_or_default();
        m.insert(
            "defined_by".into(),
            json!(defs.iter().map(|d| d.id.as_str()).collect::<Vec<_>>()),
        );
        for k in ["read_by", "written_by"] {
            m.entry(k).or_insert_with(|| json!([]));
        }
        if defs.is_empty() {
            continue;
        }
        let pages: BTreeSet<String> = units
            .iter()
            .filter(|u| u.child_refs.contains(p))
            .flat_map(|u| u.pages.iter().map(|pg| format!("parent:{pg}")))
            .collect();
        entry(&mut g, p);
        if let Some(gm) = g.get_mut(p) {
            for pg in &pages {
                push_str(gm, "called_by", pg);
                push_str(gm, "member_access", &format!("{pg} (contentWindow.{p})"));
            }
        }
    }
    // Entre unidades.
    for u in units {
        for n in &u.uses_globals {
            entry(&mut g, n);
            if let Some(m) = g.get_mut(n) {
                push_str(m, "used_by", &u.id);
            }
        }
        for n in &u.mutates_globals {
            entry(&mut g, n);
            if let Some(m) = g.get_mut(n) {
                push_str(m, "mutated_by", &u.id);
            }
        }
        // Un iframe solo ve como `parent.X` lo que es propiedad de window.
        if u.parent_refs.is_empty() {
            continue;
        }
        let page = u.source.rsplit('/').next().unwrap_or(&u.source).to_string();
        for n in &u.parent_refs {
            entry(&mut g, n);
            if let Some(m) = g.get_mut(n) {
                push_str(m, "called_by", &format!("iframe:{page}"));
                push_str(m, "member_access", &format!("{} (parent.{n})", u.id));
            }
        }
    }
    // Host.
    let mut hosts = Map::new();
    let mut dynamic = Vec::new();
    let mut handlers: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let mut messages: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let mut dom: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let markup = markup_ids(&layout);
    for (label, path) in &layout.hosts {
        let Ok(src) = std::fs::read_to_string(path) else {
            hosts.insert(label.clone(), json!({"found": false}));
            continue;
        };
        let scan = host::scan_host(label, &src);
        hosts.insert(
            label.clone(),
            json!({"found": true, "calls": scan.calls.len()}),
        );
        for h in &scan.handlers {
            push_str(
                handlers.entry(h.clone()).or_default(),
                "registered_by",
                label,
            );
        }
        for c in &scan.calls {
            let Some(js) = &c.js else {
                dynamic.push(
                    json!({"from": c.from, "line": c.line, "via": c.via, "origin": c.origin}),
                );
                continue;
            };
            let f = analyze::analyze(js);
            let call = json!({"from": c.from, "line": c.line, "via": c.via, "origin": c.origin});
            let mut names: Vec<(String, bool)> =
                f.uses.iter().map(|u| (u.name.clone(), u.member)).collect();
            names.dedup();
            for (n, member) in names {
                if BUILTINS.contains(&n.as_str()) {
                    continue;
                }
                entry(&mut g, &n);
                if let Some(m) = g.get_mut(&n) {
                    push_str(m, "called_by", label);
                    if let Some(a) = m.get_mut("host_calls").and_then(Value::as_array_mut)
                        && !a.contains(&call)
                    {
                        a.push(call.clone());
                    }
                    if member {
                        push_str(
                            m,
                            "member_access",
                            &format!("{label}:{} (window.{n})", c.line),
                        );
                    }
                }
            }
            for id in &f.dom_ids {
                let m = dom.entry(id.clone()).or_default();
                push_str(m, "called_by", label);
                m.insert("in_markup".into(), json!(markup.contains(id)));
            }
            for msg in &f.post_messages {
                push_str(messages.entry(msg.clone()).or_default(), "sent_by", label);
            }
        }
    }
    // Mensajes de las unidades.
    for u in units {
        for msg in &u.post_messages {
            if let Some(rest) = msg.strip_prefix("webkit.") {
                let h = rest.split(':').next().unwrap_or(rest);
                push_str(
                    handlers.entry(h.to_string()).or_default(),
                    "posted_by",
                    &u.id,
                );
            } else {
                push_str(messages.entry(msg.clone()).or_default(), "sent_by", &u.id);
            }
        }
    }
    for (k, m) in messages.iter_mut() {
        let ty = k.rsplit('/').next().unwrap_or(k);
        for u in units
            .iter()
            .filter(|u| u.message_types.iter().any(|t| t == ty))
        {
            push_str(m, "handled_by", &u.id);
        }
        m.entry("sent_by").or_insert_with(|| json!([]));
        m.entry("handled_by").or_insert_with(|| json!([]));
    }
    for m in handlers.values_mut() {
        m.entry("registered_by").or_insert_with(|| json!([]));
        m.entry("posted_by").or_insert_with(|| json!([]));
    }
    let section = |b: BTreeMap<String, Map<String, Value>>| {
        Value::Object(b.into_iter().map(|(k, v)| (k, Value::Object(v))).collect())
    };
    // Riesgos.
    for (name, m) in g.iter_mut() {
        let by = m
            .get("defined_by")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        let kinds: Vec<String> = m
            .get("binding")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let lexical = kinds
            .iter()
            .any(|k| matches!(k.as_str(), "let" | "const" | "class"));
        let mutated = m
            .get("mutated_by")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty());
        let member = m.contains_key("member_access");
        let mut risks: Vec<String> = Vec::new();
        if by == 0 {
            risks.push(
                "nadie lo define: la llamada falla o depende de algo fuera del inventario"
                    .to_string(),
            );
        }
        if m.get("collides").is_some_and(|c| c == true) {
            risks.push(format!(
                "definido en {by} unidades de la misma página: gana la última que carga"
            ));
        }
        let patchers: Vec<String> = m
            .get("patched_by")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if !patchers.is_empty() {
            risks.push(format!(
                "parcheado con window.{name} = desde {}: portar el definidor sin seguir exportando {name} por window rompe el parche, y portar el parche exige que el definidor ya lo haya creado",
                patchers.join(", ")
            ));
        }
        let fallbacks: Vec<String> = m
            .get("fallback_by")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if !fallbacks.is_empty() {
            risks.push(format!(
                "respaldo con guarda en {}: solo crea window.{name} si falta; el port del definidor debe seguir publicándolo en window antes de que corra el respaldo, o habrá dos instancias",
                fallbacks.join(", ")
            ));
        }
        if mutated {
            risks.push(if lexical {
                "mutado desde otra unidad y declarado con let/const: no es propiedad de window, el puente global_set no lo alcanza".to_string()
            } else {
                "mutado desde otra unidad: debe seguir siendo propiedad de window mientras ambos lados vivan".to_string()
            });
        }
        if member && lexical {
            risks.push(format!(
                "leído como propiedad (window.{name}/parent.{name}) pero declarado con let/const/class: por esa vía vale undefined, sin excepción; el lector sigue con su valor por defecto"
            ));
        }
        m.insert("risks".into(), json!(risks));
    }
    let mut top = Map::new();
    for (name, m) in g {
        let external = [
            "patched_by",
            "fallback_by",
            "used_by",
            "mutated_by",
            "called_by",
        ]
        .iter()
        .any(|k| {
            m.get(*k)
                .and_then(Value::as_array)
                .is_some_and(|a| !a.is_empty())
        });
        if external {
            top.insert(name, Value::Object(m));
        }
    }
    special.insert("@frame_contract".into(), section(frame));
    let dom_access: Vec<&str> = units
        .iter()
        .filter(|u| u.child_dom_access)
        .map(|u| u.id.as_str())
        .collect();
    special.insert("@frame_dom_access".into(), json!(dom_access));
    special.insert("@dom_ids".into(), section(dom));
    special.insert("@messages".into(), section(messages));
    special.insert("@host_handlers".into(), section(handlers));
    special.insert("@host_dynamic".into(), json!(dynamic));
    special.insert("@hosts".into(), Value::Object(hosts));
    for (k, v) in special {
        top.insert(k, v);
    }
    Value::Object(top)
}

/// Ids presentes como `id="…"` en el HTML de las páginas (fuera de scripts).
fn markup_ids(layout: &Layout) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (page, _, _) in PAGES {
        let Ok(html) = std::fs::read_to_string(layout.dash.join(page)) else {
            continue;
        };
        let mut markup = String::new();
        let mut pos = 0;
        for (s, e) in script_tags(&html).into_iter().filter_map(|t| t.body) {
            markup.push_str(html.get(pos..s).unwrap_or(""));
            pos = e;
        }
        markup.push_str(html.get(pos..).unwrap_or(""));
        for q in ['"', '\''] {
            let pat = format!(" id={q}");
            let mut rest = markup.as_str();
            while let Some(i) = rest.find(&pat) {
                let after = rest.get(i + pat.len()..).unwrap_or("");
                if let Some(end) = after.find(q)
                    && let Some(id) = after.get(..end)
                {
                    out.insert(id.to_string());
                }
                rest = after;
            }
        }
    }
    out
}

pub fn unit_json(u: &Unit) -> Value {
    json!({
        "id": u.id,
        "kind": u.kind.as_str(),
        "component": u.component,
        "source": u.source,
        "pages": u.pages,
        "script_index": u.script_index,
        "marker_start": u.marker_start,
        "marker_end": u.marker_end,
        "sha256": u.sha256,
        "lines": u.lines,
        "line_start": u.line_start,
        "defines": u.defines,
        "define_kinds": u.define_kinds,
        "uses_globals": u.uses_globals,
        "mutates_globals": u.mutates_globals,
        "parent_refs": u.parent_refs,
        "patches": u.patches,
        "child_refs": u.child_refs,
        "child_writes": u.child_writes,
        "child_dom_access": u.child_dom_access,
        "fallbacks": u.fallbacks,
        "dom_ids": u.dom_ids,
        "routes": u.routes,
        "storage_keys": u.storage_keys,
        "post_messages": u.post_messages,
        "message_types": u.message_types,
        "intervals_ms": u.intervals_ms,
        "strings": u.strings,
        "warnings": u.warnings,
    })
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Commit y archivos sin comitear del origen (trazabilidad del inventario).
pub fn source_state(repo: &Path) -> Value {
    let head = git(repo, &["rev-parse", "HEAD"]);
    let dirty: Vec<String> = git(
        repo,
        &[
            "status",
            "--porcelain",
            "--",
            "dash",
            "bin/cc-app",
            "bin/cc-app-mac",
        ],
    )
    .map(|s| {
        s.lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    })
    .unwrap_or_default();
    json!({"head": head, "dirty": dirty})
}

/// `inventory.json`: origen, páginas en orden de carga, unidades y orden de port.
pub fn inventory_json(units: &[Unit], repo: &Path) -> Value {
    let s = scan_all(repo);
    let mut pages = Map::new();
    for (page, entries) in &s.pages {
        let v: Vec<Value> = entries
            .iter()
            .map(|e| match e {
                PageEntry::Unit(id) => json!(id),
                PageEntry::External(src) => json!(format!("external:{src}")),
            })
            .collect();
        pages.insert(page.clone(), json!(v));
    }
    let order = port_order(units);
    let levels = port_levels(units, &order);
    let groups: Vec<Value> = order
        .iter()
        .zip(&levels)
        .map(|(g, l)| json!({"level": l, "units": g}))
        .collect();
    json!({
        "generator": "cargo run -p xtask -- web-inventory",
        "source": source_state(repo),
        "pages": pages,
        "units": units.iter().map(unit_json).collect::<Vec<_>>(),
        "port_order": groups,
    })
}

/// Entrada de la línea de órdenes: `web-inventory [--repo DIR] [--out F]
/// [--interop F] [--doc F]`. Rutas relativas a la raíz del workspace.
pub fn main(args: &[String]) -> i32 {
    let mut repo: Option<PathBuf> = None;
    let mut out = PathBuf::from("xtask/web/inventory.json");
    let mut interop_out: Option<PathBuf> = None;
    let mut doc_out: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let val = |it: &mut std::slice::Iter<String>| it.next().map(PathBuf::from);
        let slot = match a.as_str() {
            "--repo" => &mut repo,
            "--out" => {
                match val(&mut it) {
                    Some(p) => out = p,
                    None => return usage_inventory(),
                }
                continue;
            }
            "--interop" => &mut interop_out,
            "--doc" => &mut doc_out,
            _ => return usage_inventory(),
        };
        match val(&mut it) {
            Some(p) => *slot = Some(p),
            None => return usage_inventory(),
        }
    }
    let ws = crate::web_port::workspace_root();
    let repo = repo.unwrap_or_else(crate::web_port::default_repo);
    if !repo.join("dash/index.html").is_file() {
        eprintln!("error: {} no tiene dash/index.html", repo.display());
        return 1;
    }
    let abs = |p: PathBuf| if p.is_absolute() { p } else { ws.join(p) };
    let out = abs(out);
    let interop_out = abs(interop_out.unwrap_or_else(|| out.with_file_name("interop.json")));
    let units = scan(&repo);
    let inv = inventory_json(&units, &repo);
    let ix = interop(&units, &repo);
    let write = |p: &Path, text: String| -> Result<(), String> {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::write(p, text).map_err(|e| format!("{}: {e}", p.display()))
    };
    let pretty = |v: &Value| {
        serde_json::to_string_pretty(v)
            .map(|s| s + "\n")
            .unwrap_or_default()
    };
    let mut res = write(&out, pretty(&inv)).and_then(|_| write(&interop_out, pretty(&ix)));
    if let Some(d) = doc_out {
        res = res.and_then(|_| write(&abs(d), doc::render(&units, &inv, &ix)));
    }
    if let Err(e) = res {
        eprintln!("error: {e}");
        return 1;
    }
    let s = doc::Summary::of(&units, &ix);
    println!(
        "{} unidades ({} scripts, {} regiones), {} globales definidos, {} con consumidores fuera de su unidad, {} aristas entre unidades, {} llamadas del host",
        s.units, s.scripts, s.regions, s.globals, s.interop_globals, s.edges, s.host_calls
    );
    println!(
        "inventario: {}\ninterop: {}",
        out.display(),
        interop_out.display()
    );
    0
}

fn usage_inventory() -> i32 {
    eprintln!(
        "uso: cargo run -p xtask -- web-inventory [--repo DIR] [--out F] [--interop F] [--doc F]"
    );
    2
}
