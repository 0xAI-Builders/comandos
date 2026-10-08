//! Capturas de paridad en el Chrome del Mac (`chrome-bg`).
//!
//! - `pair`: misma página con `?web=off` (legado) y `?web=shadow` (vista web)
//!   en la misma sesión; recorta cada selector, enmascara regiones volátiles y
//!   compara con `png_diff`; si un recorte falla, adjunta la primera diferencia
//!   del `outerHTML` normalizado (`comandos-domdiff`).
//! - `remote-vs-desktop`: presencia y estilos calculados de cada componente
//!   entre la carga de escritorio y la remota (regla de Jesús: el remoto es el
//!   escritorio, salvo disposición y los componentes solo remotos).
//!
//! Suites: un archivo por suite en `xtask/web/shots/<suite>.json` (preflight R11).
use crate::dom_diff::first_difference;
use crate::mcp::{Client, EmulationMode, Page};
use crate::png_diff::{self, Rect, Rgba};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Puertos del tablero vivo: el arnés nunca los toca.
pub const LIVE_PORTS: std::ops::RangeInclusive<u16> = 4777..=4782;

/// Entrada de una suite `pair`.
#[derive(Debug, Clone, PartialEq)]
pub struct PairEntry {
    pub id: String,
    pub page: String,
    pub widths: Vec<u32>,
    pub height: u32,
    pub dpr: Vec<u32>,
    pub touch: Vec<bool>,
    pub selectors: Vec<String>,
    pub mask: Vec<String>,
    pub channel: u8,
    pub ratio: f64,
    pub full_page: bool,
    pub prepare: Option<String>,
    pub require_wasm: bool,
}

/// Suite `remote-vs-desktop`.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteSuite {
    pub page: String,
    pub widths: Vec<u32>,
    pub height: u32,
    pub components: Vec<String>,
    pub remote_only: Vec<String>,
    pub remote_query: String,
    pub desktop_query: String,
    pub prepare: Option<String>,
    pub require_wasm: bool,
}

/// Propiedades que deben coincidir entre escritorio y remoto; las de
/// disposición (`display`, `flex-*`, `grid-*`, `width`, `height`, `order`)
/// quedan fuera a propósito.
pub const STYLE_PROPS: &[&str] = &[
    "color",
    "background-color",
    "font-family",
    "font-size",
    "font-weight",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
];

fn strings(v: Option<&Value>, field: &str) -> Result<Vec<String>, String> {
    match v {
        None => Ok(Vec::new()),
        Some(Value::Array(a)) => a
            .iter()
            .map(|s| {
                s.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("«{field}» debe ser una lista de textos"))
            })
            .collect(),
        Some(_) => Err(format!("«{field}» debe ser una lista")),
    }
}

fn numbers(v: Option<&Value>, field: &str, default: &[u32]) -> Result<Vec<u32>, String> {
    match v {
        None => Ok(default.to_vec()),
        Some(Value::Array(a)) if !a.is_empty() => a
            .iter()
            .map(|n| {
                n.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("«{field}» debe tener enteros positivos"))
            })
            .collect(),
        Some(_) => Err(format!("«{field}» debe ser una lista no vacía")),
    }
}

fn u32_field(o: &Map<String, Value>, field: &str, default: u32) -> Result<u32, String> {
    match o.get(field) {
        None => Ok(default),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| format!("«{field}» debe ser un entero positivo")),
    }
}

/// Lee una suite `pair`: lista de entradas con los valores por omisión del
/// plan (`widths` 1400/844/390/320, `dpr` 1 y 2, `channel` 24, `ratio` 0.001).
/// `touch` admite `true`/`false` o una lista de ambos (dimensión del barrido).
pub fn parse_pair_suite(text: &str) -> Result<Vec<PairEntry>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("suite: {e}"))?;
    let items = v.as_array().ok_or("suite pair: se esperaba una lista")?;
    if items.is_empty() {
        return Err("suite pair vacía".into());
    }
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let o = item
                .as_object()
                .ok_or_else(|| format!("entrada {i}: se esperaba un objeto"))?;
            let text = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
            let id = text("id").ok_or_else(|| format!("entrada {i}: falta «id»"))?;
            let ctx = |e: String| format!("{id}: {e}");
            let page = text("page").ok_or_else(|| ctx("falta «page»".into()))?;
            let touch = match o.get("touch") {
                None => vec![false],
                Some(Value::Bool(b)) => vec![*b],
                Some(Value::Array(a)) if !a.is_empty() => a
                    .iter()
                    .map(|b| {
                        b.as_bool()
                            .ok_or_else(|| ctx("«touch» con valores no booleanos".into()))
                    })
                    .collect::<Result<_, _>>()?,
                Some(_) => return Err(ctx("«touch» debe ser booleano o lista".into())),
            };
            let selectors = strings(o.get("selectors"), "selectors").map_err(ctx)?;
            if selectors.is_empty() {
                return Err(ctx("sin «selectors»".into()));
            }
            let channel = match o.get("channel") {
                None => 24,
                Some(c) => c
                    .as_u64()
                    .and_then(|c| u8::try_from(c).ok())
                    .ok_or_else(|| ctx("«channel» fuera de 0..=255".into()))?,
            };
            let ratio = match o.get("ratio") {
                None => 0.001,
                Some(r) => r
                    .as_f64()
                    .filter(|r| (0.0..=1.0).contains(r))
                    .ok_or_else(|| ctx("«ratio» fuera de 0..=1".into()))?,
            };
            Ok(PairEntry {
                page,
                widths: numbers(o.get("widths"), "widths", &[1400, 844, 390, 320]).map_err(ctx)?,
                height: u32_field(o, "height", 900).map_err(ctx)?,
                dpr: numbers(o.get("dpr"), "dpr", &[1, 2]).map_err(ctx)?,
                touch,
                selectors,
                mask: strings(o.get("mask"), "mask").map_err(ctx)?,
                channel,
                ratio,
                full_page: o.get("full_page").and_then(Value::as_bool).unwrap_or(true),
                prepare: text("prepare"),
                require_wasm: o
                    .get("require_wasm")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                id,
            })
        })
        .collect()
}

/// Lee la suite `remote-vs-desktop`.
pub fn parse_remote_suite(text: &str) -> Result<RemoteSuite, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("suite: {e}"))?;
    let o = v
        .as_object()
        .ok_or("suite remote-vs-desktop: se esperaba un objeto")?;
    let components = strings(o.get("components"), "components")?;
    if components.is_empty() {
        return Err("suite remote-vs-desktop sin «components»".into());
    }
    Ok(RemoteSuite {
        page: o
            .get("page")
            .and_then(Value::as_str)
            .unwrap_or("/")
            .to_string(),
        widths: numbers(o.get("widths"), "widths", &[1400, 844, 390, 320])?,
        height: u32_field(o, "height", 900)?,
        components,
        remote_only: strings(o.get("remote_only"), "remote_only")?,
        remote_query: o
            .get("remote_query")
            .and_then(Value::as_str)
            .unwrap_or("__devwebterm=1")
            .to_string(),
        desktop_query: o
            .get("desktop_query")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        prepare: o.get("prepare").and_then(Value::as_str).map(str::to_string),
        require_wasm: o
            .get("require_wasm")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// Rechaza bases que no sean http y las que apunten al tablero vivo.
pub fn check_base(base: &str) -> Result<(), String> {
    let rest = base
        .strip_prefix("http://")
        .or_else(|| base.strip_prefix("https://"))
        .ok_or_else(|| format!("--base debe ser http(s)://…: {base}"))?;
    let authority = rest.split('/').next().unwrap_or("");
    if let Some((_, port)) = authority.rsplit_once(':')
        && let Ok(port) = port.parse::<u16>()
        && LIVE_PORTS.contains(&port)
    {
        return Err(format!(
            "--base apunta al tablero vivo (puerto {port}); usa un servidor de fixtures en 73xx"
        ));
    }
    Ok(())
}

/// `base + page` con los parámetros añadidos tras `?` o `&`.
pub fn page_url(base: &str, page: &str, params: &str) -> String {
    let mut url = format!(
        "{}/{}",
        base.trim_end_matches('/'),
        page.trim_start_matches('/')
    );
    if !params.is_empty() {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str(params);
    }
    url
}

/// Rectángulo CSS (coordenadas de documento) a píxeles de la captura,
/// recortado a la imagen; `None` si no queda área.
pub fn device_rect(css: [f64; 4], scale: f64, width: u32, height: u32) -> Option<Rect> {
    let [x, y, w, h] = css;
    if !(x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let clamp = |v: f64, max: u32| v.max(0.0).min(f64::from(max)) as u32;
    let x0 = clamp((x * scale).floor(), width);
    let y0 = clamp((y * scale).floor(), height);
    let x1 = clamp(((x + w) * scale).ceil(), width);
    let y1 = clamp(((y + h) * scale).ceil(), height);
    (x1 > x0 && y1 > y0).then(|| Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

/// Diferencias de presencia y estilo de un componente; `remote_only` solo
/// exige presencia en el remoto.
pub fn compare_styles(
    selector: &str,
    desktop: Option<&Map<String, Value>>,
    remote: Option<&Map<String, Value>>,
    remote_only: bool,
) -> Vec<String> {
    if remote_only {
        return match remote {
            Some(_) => Vec::new(),
            None => vec![format!(
                "{selector}: falta en el remoto (componente solo remoto)"
            )],
        };
    }
    match (desktop, remote) {
        (None, None) => vec![format!("{selector}: ausente en escritorio y en remoto")],
        (None, Some(_)) => vec![format!("{selector}: falta en escritorio")],
        (Some(_), None) => vec![format!("{selector}: falta en el remoto")],
        (Some(d), Some(r)) => STYLE_PROPS
            .iter()
            .filter_map(|p| {
                let (a, b) = (d.get(*p), r.get(*p));
                (a != b).then(|| {
                    format!(
                        "{selector}: {p} escritorio={} remoto={}",
                        a.map_or("<nada>".into(), Value::to_string),
                        b.map_or("<nada>".into(), Value::to_string)
                    )
                })
            })
            .collect(),
    }
}

fn js_list(items: &[String]) -> String {
    Value::Array(items.iter().cloned().map(Value::String).collect()).to_string()
}

const READY_JS: &str = "async () => { if (window.fixtureReady) { await window.fixtureReady; const mode=new URLSearchParams(location.search).get('web'); if ((mode==='on'||mode==='shadow') && (!window.__comandosReady || !window.fixtureWasmUrl || !window.fixtureVisualReady)) throw new Error('fixture candidate did not load/actions actual WASM'); return true; } return !!window.__comandosReady || document.readyState === 'complete'; }";

/// Espera la marca de listo: reintentos cada 100 ms hasta 10 s.
fn wait_ready(page: &mut Page<'_>) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match page.eval(READY_JS) {
            Ok(Value::Bool(true)) => return Ok(()),
            Ok(_) => {}
            Err(e) if e.contains("Execution context was destroyed") => {}
            Err(e) => return Err(e),
        }
        if Instant::now() >= deadline {
            return Err("la página no marcó listo en 10 s".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Elemento capturado: rectángulo CSS en coordenadas de documento y `outerHTML`.
#[derive(Debug, Clone)]
struct Element {
    rect: [f64; 4],
    html: String,
}

struct Capture {
    image: Rgba,
    scale: f64,
    proof: Value,
    elements: Vec<Vec<Element>>,
}

fn rect_of(v: &Value) -> Option<[f64; 4]> {
    let a = v.as_array()?;
    let n = |i: usize| a.get(i).and_then(Value::as_f64);
    Some([n(0)?, n(1)?, n(2)?, n(3)?])
}

fn capture(page: &mut Page<'_>, url: &str, entry: &PairEntry) -> Result<Capture, String> {
    page.navigate(url)?;
    wait_ready(page)?;
    if let Some(prepare) = &entry.prepare {
        page.eval(prepare)?;
    }
    if entry.require_wasm && url.contains("web=shadow") {
        let proof = page.eval("() => ({ready:window.__comandosReady,mode:new URLSearchParams(location.search).get('web'),wasm:performance.getEntriesByType('resource').some(x=>x.name.endsWith('comandos_web_bg.wasm'))})")?;
        if proof["ready"] != true || proof["mode"] != "shadow" || proof["wasm"] != true {
            return Err(format!(
                "production candidate did not boot actual WASM: {proof}"
            ));
        }
    }
    // Se mide antes de capturar: así máscara y recorte describen el mismo
    // estado que la captura que sigue, sin una ventana entre ambas.
    let js = format!(
        "() => {{ const box = e => {{ const r = e.getBoundingClientRect(); \
         return [r.x + scrollX, r.y + scrollY, r.width, r.height]; }}; \
         const all = s => [...document.querySelectorAll(s)]; \
         return {{ scale: devicePixelRatio, proof: {{wasm_url:window.fixtureWasmUrl || performance.getEntriesByType('resource').find(x=>x.name.endsWith('comandos_web_bg.wasm'))?.name || null, actions:window.productApiCalls || window.fixtureApiCalls || null, product:window.productProof || null}}, \
         sel: {sel}.map(s => all(s).map(e => [box(e), e.outerHTML])), \
         mask: {mask}.map(s => all(s).map(box)) }}; }}",
        sel = js_list(&entry.selectors),
        mask = js_list(&entry.mask),
    );
    let data = page.eval(&js)?;
    let png = page.screenshot_png(entry.full_page)?;
    let mut image = png_diff::decode(&png)?;
    let scale = data.get("scale").and_then(Value::as_f64).unwrap_or(1.0);
    let list = |k: &str| {
        data.get(k)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    for group in list("mask") {
        for r in group.as_array().into_iter().flatten().filter_map(rect_of) {
            if let Some(r) = device_rect(r, scale, image.width, image.height) {
                png_diff::mask(&mut image, r);
            }
        }
    }
    let elements = list("sel")
        .iter()
        .map(|group| {
            group
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|pair| {
                    let p = pair.as_array()?;
                    Some(Element {
                        rect: rect_of(p.first()?)?,
                        html: p.get(1)?.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .collect();
    Ok(Capture {
        image,
        scale,
        proof: data.get("proof").cloned().unwrap_or(Value::Null),
        elements,
    })
}

fn slug(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    s.trim_matches('_').to_string()
}

fn mkdir(p: &Path) -> Result<(), String> {
    std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn clock_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Compara un selector entre las dos capturas; devuelve las filas del informe.
fn compare_selector(
    entry: &PairEntry,
    index: usize,
    legacy: &Capture,
    web: &Capture,
    dir: &Path,
    base: &Value,
) -> Result<Vec<Value>, String> {
    let selector = entry.selectors.get(index).cloned().unwrap_or_default();
    let empty = Vec::new();
    let le = legacy.elements.get(index).unwrap_or(&empty);
    let we = web.elements.get(index).unwrap_or(&empty);
    let row = |extra: Value| {
        let mut r = base.clone();
        if let (Some(r), Some(extra)) = (r.as_object_mut(), extra.as_object()) {
            r.insert("selector".into(), Value::String(selector.clone()));
            r.extend(extra.clone());
        }
        r
    };
    if le.len() != we.len() || le.is_empty() {
        let legacy_html: String = le.iter().map(|e| e.html.as_str()).collect();
        let web_html: String = we.iter().map(|e| e.html.as_str()).collect();
        return Ok(vec![row(json!({
            "pass": false,
            "error": format!("coincidencias: legado {} ≠ web {}", le.len(), we.len()),
            "dom_difference": first_difference(&legacy_html, &web_html),
        }))]);
    }
    let mut rows = Vec::new();
    for (n, (l, w)) in le.iter().zip(we).enumerate() {
        let sub = dir.join(format!("{index}-{}-{n}", slug(&selector)));
        mkdir(&sub)?;
        let lr = device_rect(
            l.rect,
            legacy.scale,
            legacy.image.width,
            legacy.image.height,
        );
        let wr = device_rect(w.rect, web.scale, web.image.width, web.image.height);
        let (Some(lr), Some(wr)) = (lr, wr) else {
            rows.push(row(json!({"element": n, "pass": false,
                "error": "elemento sin área visible en la captura",
                "dom_difference": first_difference(&l.html, &w.html)})));
            continue;
        };
        let a = png_diff::crop(&legacy.image, lr)?;
        let b = png_diff::crop(&web.image, wr)?;
        png_diff::write_png(&a, &sub.join("legacy.png"))?;
        png_diff::write_png(&b, &sub.join("web.png"))?;
        let stats = match png_diff::diff(&a, &b, entry.channel) {
            Ok(s) => s,
            Err(e) => {
                rows.push(row(json!({"element": n, "pass": false, "error": e,
                    "dom_difference": first_difference(&l.html, &w.html)})));
                continue;
            }
        };
        png_diff::write_diff_png(&a, &b, entry.channel, &sub.join("diff.png"))?;
        let pass = stats.ratio() <= entry.ratio;
        let mut r = row(
            json!({"element": n, "differing": stats.differing, "total": stats.total,
            "ratio": stats.ratio(), "limit": entry.ratio, "pass": pass,
            "dir": sub.display().to_string()}),
        );
        if !pass && let Some(o) = r.as_object_mut() {
            o.insert(
                "dom_difference".into(),
                json!(first_difference(&l.html, &w.html)),
            );
        }
        rows.push(r);
    }
    Ok(rows)
}

/// Suites en `xtask/web/shots/`: `name` o una ruta a un `.json`.
pub fn suite_path(name: &str) -> PathBuf {
    if name.ends_with(".json") {
        PathBuf::from(name)
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("web/shots/{name}.json"))
    }
}

/// Lee el texto de una suite.
pub fn read_suite(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn write_report(out: &Path, name: &str, report: &Value) -> Result<(), String> {
    let path = out.join(name);
    let text = serde_json::to_string_pretty(report).map_err(|e| format!("informe: {e}"))?;
    std::fs::write(&path, text + "\n").map_err(|e| format!("{}: {e}", path.display()))
}

/// Barrido de `pair`: emula, captura las dos variantes y compara; añade las
/// filas a `rows` según avanza, para que un error deje el informe parcial.
fn pair_combos(
    page: &mut Page<'_>,
    base: &str,
    clock: u128,
    entries: &[PairEntry],
    out: &Path,
    rows: &mut Vec<Value>,
    omitted: &mut bool,
) -> Result<(), String> {
    for entry in entries {
        for &width in &entry.widths {
            for &dpr in &entry.dpr {
                for &touch in &entry.touch {
                    let mode = page.emulate(width, entry.height, dpr, touch)?;
                    *omitted |= mode == EmulationMode::ResizeOnly;
                    let combo = format!("{width}w-{dpr}x{}", if touch { "-touch" } else { "" });
                    let dir = out.join(slug(&entry.id)).join(&combo);
                    mkdir(&dir)?;
                    let legacy_url =
                        page_url(base, &entry.page, &format!("web=off&__clock={clock}"));
                    let web_url =
                        page_url(base, &entry.page, &format!("web=shadow&__clock={clock}"));
                    let legacy = capture(page, &legacy_url, entry)?;
                    let web = capture(page, &web_url, entry)?;
                    png_diff::write_png(&legacy.image, &dir.join("legacy-full.png"))?;
                    png_diff::write_png(&web.image, &dir.join("web-full.png"))?;
                    let base_row = json!({
                        "id": entry.id, "width": width, "dpr": dpr, "touch": touch,
                        "emulation": match mode {
                            EmulationMode::Viewport => "viewport",
                            EmulationMode::ResizeOnly => "resize_page (dpr y touch OMITIDOS)",
                        },
                        "touch_mode": if mode == EmulationMode::ResizeOnly { "OMITIDO" } else { "emulado" },
                        "legacy_url": legacy_url, "web_url": web_url,
                        "legacy_proof":legacy.proof, "web_proof":web.proof,
                    });
                    for index in 0..entry.selectors.len() {
                        rows.extend(compare_selector(
                            entry, index, &legacy, &web, &dir, &base_row,
                        )?);
                    }
                }
            }
        }
    }
    Ok(())
}

/// `shots pair`: devuelve el número de recortes que fallan.
pub fn run_pair(
    client: &mut Client,
    base: &str,
    suite_name: &str,
    entries: &[PairEntry],
    out: &Path,
) -> Result<usize, String> {
    check_base(base)?;
    mkdir(out)?;
    let clock = clock_ms();
    let mut rows = Vec::new();
    let mut omitted = false;
    let mut page = client.open_page("about:blank")?;
    let outcome = pair_combos(
        &mut page,
        base,
        clock,
        entries,
        out,
        &mut rows,
        &mut omitted,
    )
    .and_then(|()| page.close());
    let failures = rows
        .iter()
        .filter(|r| r.get("pass").and_then(Value::as_bool) != Some(true))
        .count();
    for r in &rows {
        let ok = r.get("pass").and_then(Value::as_bool) == Some(true);
        println!(
            "{} {} {}w {}x touch={} {} ratio={}{}",
            if ok { "OK  " } else { "FALLA" },
            r.get("id").and_then(Value::as_str).unwrap_or(""),
            r.get("width").unwrap_or(&Value::Null),
            r.get("dpr").unwrap_or(&Value::Null),
            r.get("touch_mode").and_then(Value::as_str).unwrap_or(""),
            r.get("selector").and_then(Value::as_str).unwrap_or(""),
            r.get("ratio").unwrap_or(&Value::Null),
            r.get("error")
                .and_then(Value::as_str)
                .map(|e| format!(" ({e})"))
                .unwrap_or_default(),
        );
    }
    if omitted {
        println!("AVISO: el broker no emula viewport: DPR y táctil OMITIDOS (ver report.json)");
    }
    println!("{} recortes, {failures} fallan", rows.len());
    let error = outcome.as_ref().err().cloned();
    if let Some(e) = &error {
        println!("ERROR (informe parcial): {e}");
    }
    write_report(
        out,
        "report.json",
        &json!({"suite": suite_name, "base": base, "clock": clock.to_string(),
            "touch_omitted": omitted, "failures": failures, "error": error, "results": rows}),
    )?;
    outcome.map(|()| failures)
}

fn styles_js(selectors: &[String]) -> String {
    let props: Vec<String> = STYLE_PROPS.iter().map(|p| (*p).to_string()).collect();
    format!(
        "() => {{ const props = {props}; return {sels}.map(s => {{ \
         const e = document.querySelector(s); if (!e) return null; \
         const cs = getComputedStyle(e); const o = {{__proof:{{ready:window.__comandosReady||false,wasm_url:performance.getEntriesByType('resource').find(x=>x.name.endsWith('comandos_web_bg.wasm'))?.name||null,actions:window.productApiCalls||null}}}}; \
         for (const p of props) o[p] = cs.getPropertyValue(p); return o; }}); }}",
        props = js_list(&props),
        sels = js_list(selectors),
    )
}

fn load_styles(
    page: &mut Page<'_>,
    url: &str,
    selectors: &[String],
    prepare: Option<&str>,
    require_wasm: bool,
) -> Result<Vec<Option<Map<String, Value>>>, String> {
    page.navigate(url)?;
    wait_ready(page)?;
    if let Some(prepare) = prepare {
        page.eval(prepare)?;
    }
    let v = page.eval(&styles_js(selectors))?;
    let list = v.as_array().ok_or("estilos: se esperaba una lista")?;
    if require_wasm
        && url.contains("web=shadow")
        && list
            .iter()
            .any(|row| row["__proof"]["ready"] != true || row["__proof"]["wasm_url"].is_null())
    {
        return Err("remote style candidate did not boot actual WASM".into());
    }
    Ok(list.iter().map(|s| s.as_object().cloned()).collect())
}

/// `shots remote-vs-desktop`: devuelve el número de diferencias.
pub fn run_remote_vs_desktop(
    client: &mut Client,
    base: &str,
    remote_base: &str,
    suite: &RemoteSuite,
    out: &Path,
) -> Result<usize, String> {
    check_base(base)?;
    check_base(remote_base)?;
    mkdir(out)?;
    let selectors: Vec<String> = suite
        .components
        .iter()
        .chain(&suite.remote_only)
        .cloned()
        .collect();
    let desktop_url = page_url(base, &suite.page, &suite.desktop_query);
    let remote_url = page_url(remote_base, &suite.page, &suite.remote_query);
    let mut rows = Vec::new();
    let mut page = client.open_page("about:blank")?;
    for &width in &suite.widths {
        page.emulate(width, suite.height, 1, false)?;
        let desktop = load_styles(
            &mut page,
            &desktop_url,
            &selectors,
            suite.prepare.as_deref(),
            suite.require_wasm,
        )?;
        let remote = load_styles(
            &mut page,
            &remote_url,
            &selectors,
            suite.prepare.as_deref(),
            suite.require_wasm,
        )?;
        for (i, sel) in selectors.iter().enumerate() {
            let remote_only = i >= suite.components.len();
            let d = desktop.get(i).and_then(Option::as_ref);
            let r = remote.get(i).and_then(Option::as_ref);
            let problems = compare_styles(sel, d, r, remote_only);
            rows.push(
                json!({"width": width, "selector": sel, "remote_only": remote_only,
                "pass": problems.is_empty(), "problems": problems,
                "desktop_proof": d.and_then(|row| row.get("__proof")),
                "remote_proof": r.and_then(|row| row.get("__proof"))}),
            );
        }
    }
    page.close()?;
    let failures = rows
        .iter()
        .filter(|r| r.get("pass").and_then(Value::as_bool) != Some(true))
        .count();
    for r in &rows {
        for p in r
            .get("problems")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            println!(
                "FALLA {}w {}",
                r.get("width").unwrap_or(&Value::Null),
                p.as_str().unwrap_or("")
            );
        }
    }
    println!("{} comprobaciones, {failures} fallan", rows.len());
    write_report(
        out,
        "remote-vs-desktop.json",
        &json!({"desktop_url": desktop_url, "remote_url": remote_url,
            "failures": failures, "results": rows}),
    )?;
    Ok(failures)
}
