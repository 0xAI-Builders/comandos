# Fase 3b — `comandos-web`: el tablero entero en Rust, componente a componente, y la PWA — plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** que el tablero (`dash/index.html` con su script en línea y los 20 archivos `dash/*.js`) lo sirvan y ejecuten Rust (plantillas `maud` y WASM con `web-sys`), píxel a píxel igual que hoy en escritorio, en `cc-app` y en remoto/móvil, con la PWA (manifest, service worker y Web Push) incluida; un componente cada vez, en sombra primero y reversible sin reiniciar nada.

**Architecture:** el frente compone la página por petición (D2 del índice): toma `dash/index.html` del disco, quita los `<script>` y regiones del script en línea de los componentes activos cuyo origen no ha cambiado desde el port, e inyecta `<meta name="comandos-web">`, el cargador generado y la compuerta (D3). El WASM monta esos componentes, publica en `window` los mismos globales que exportaba su JS y avisa `POST /web/ready`; el JS que queda sigue funcionando sin saber nada. Al final la carcasa se genera en el servidor con `maud` (B14), los recursos se embeben (B15) y ya no se sirve JS escrito a mano.

**Tech Stack:** índice. Específico: `maud = "=0.27.0"`, `wasm-bindgen = "=0.2.129"`, `web-sys = "=0.3.106"` (rasgos por crate en B1), `js-sys = "=0.3.106"`, `wasm-bindgen-futures = "=0.4.79"`, `pulldown-cmark = { version = "=0.13.4", default-features = false, features = ["html"] }`, `ammonia = "=4.2.1"`, `scraper = "=0.27.0"` (inventario y pruebas), `serde = "=1.0.229"`.

**Spec e índice:** `docs/superpowers/plans/2026-10-04-fase-3-term-y-web.md` (decisión de arquitectura, D1–D10, Rulings, Global Constraints —regla tmux incluida: ningún paso de 3b lanza tmux; las pruebas que necesiten paneles usan fixtures JSON—, Review Focus 1–4, T1–T4). Memorias que son requisito: remoto = escritorio (solo difieren la barra/entrada remotas y la disposición *flex*), texto completo siempre en avisos, sonidos suaves, un control por concepto, todo JS del tablero se valida también en el motor de `cc-app`.

## Hechos del sistema actual que fija este plan (leídos el 2026-10-04)

- `dash/index.html` (5 956 líneas): empieza por `<meta charset="utf-8">` (sin `<head>`), `<style>` en línea 12–1365, tres hojas (`workspace.css`, `buttons.css`, `analytics.css`), seis scripts antes del cuerpo (`session-config.js`, `workspace-layout.js`, `workspace-dock.js`, `quick-terminal.js`, `command-sidebar.js`, `chain-builder.js`), el cuerpo HTML (≈ 1376–1781), el script en línea grande 1782–5862 con regiones marcadas `// ---------- <nombre> ----------` (i18n, app nativa, red, helpers, App combinada, identidad de fila, barra de comandos, render, toasts, registro local, avisos, loop, Ctrl+K, iconos, tema, favoritos, prefs de terminal, tabs de modales, rango, tipografía, remoto, servidores, notificaciones del sistema, modales, UI general, snippets, funciones globales, Analytics), y después `analytics-render.js`, `analytics.js`, `extensions.js`, `workspace.js`, `work-marks.js`, `ui-sounds.js`, `pomodoro.js`, `vendor/markdown-it-15.0.2`, `vendor/purify-3.4.16`, `news-reader.js`, `push-settings.js`, `notifications.js` y un segundo script en línea 5876–5956. Los `?v=` cambian con cada edición de otras sesiones.
- `api(path, body)` (región «red»): `X-Comandos-Token` desde `localStorage.cc_token` (que llega por `?token=` y se borra de la URL); POST con JSON; error con `j.error || j.message || statusText`.
- `cc-app` llama a la página por `run_javascript`: `nsOpen()`, `pomoRender()`, `notifRender()`, `document.getElementById(<id>).click()`, y código arbitrario (`bin/cc-app:4980`, `:7521`); `cc-app-mac` por `evaluateJavaScript` (`bin/cc-app-mac:1303`). La página habla con la app por `window.webkit.messageHandlers.centro.postMessage(...)` (cabecera, tema, estilo de botones, paneles, cadenas).
- `dash/sw.js`: caché `comandos-shell-v13`, lista viva de prefijos, red primero para el shell, Web Push con `eventId` acotado y `notificationclick` que enfoca la ventana existente.
- Pruebas de JS existentes que son oráculo de comportamiento: `tests/*_checks.cjs` (con `tests/dom_stub.cjs`) y `tests/e2e_*.js`. Otras sesiones las siguen editando.

## Estructura de archivos

```
crates/comandos-web-dom/        (lib host+wasm: dom.rs, events.rs, api.rs, storage.rs, i18n.rs, log.rs,
                                  bridge.rs, timers.rs, drafts.rs)                                        B1, B5
crates/comandos-web-view/       (lib host+wasm: plantillas maud por componente, modelos de vista)         B1, B4–B14
crates/comandos-web/            (cdylib: lib.rs con boot(k); components/*.rs; components.json;
                                  interop.json)                                                            B1–B14
crates/comandos-web-sw/         (cdylib no-modules: service worker)                                       B9
crates/comandos-server/src/dash/web/ (registry.rs, compose.rs, gate.rs, assets.rs, status.rs,
                                  markdown.rs, sse.rs, embed.rs)                                          B2, B8, B14, B15
crates/comandos-cli/src/web.rs  (`comandos web set|status`)                                               B2
xtask/src/{web_inventory.rs,web_port.rs,web_bench.rs}                                                     B3, B4, B15
xtask/web/{inventory.json,interop.json,fixtures/<suite>/,shots.json}                                      B3–B15
docs/research/2026-10-04-fase-3-web-inventario.md                                                         B3
docs/verification/cutover-web.md, docs/verification/fase3/web-*.json                                     B4, B15
```

---

### Task B1: Crates base (`comandos-web-dom`, `comandos-web-view`, `comandos-web`) y el puente de globales

**Depende de:** T1 (decisión confirmada), T4.

**Files:**
- Create: `crates/comandos-web-dom/{Cargo.toml,src/lib.rs,src/dom.rs,src/events.rs,src/api.rs,src/storage.rs,src/i18n.rs,src/log.rs,src/bridge.rs,src/timers.rs}`, `crates/comandos-web-view/{Cargo.toml,src/lib.rs,src/escape.rs}`, `crates/comandos-web/{Cargo.toml,src/lib.rs,src/registry.rs,components.json,interop.json}`
- Modify: `Cargo.toml` (miembros)
- Test: `crates/comandos-web-view/tests/escape.rs`, `crates/comandos-web-dom/tests/api_host.rs`, `crates/comandos-web/tests/registry.rs`

**Interfaces:**
- Produces:
  - `comandos_web_view::escape::text(s: &str) -> String` (`& < > " '` como el `esc()` del tablero: `&amp; &lt; &gt; &quot; &#39;`), `md_esc(s) -> String` (el `mdEsc` de `index.html`: solo `& < >`), y la regla: todo texto de usuario entra por `maud` (escapa) o por estas funciones; `PreEscaped` solo para HTML que el propio crate genera (iconos).
  - `comandos_web_dom::api`: `pub async fn get(path: &str) -> Result<serde_json::Value, ApiError>`, `pub async fn post(path: &str, body: &Value) -> Result<Value, ApiError>`; misma semántica que `api()` (cabecera `X-Comandos-Token` desde `localStorage.cc_token`; error = `j.error || j.message || statusText || "La acción no se completó"`; POST con `ok === false` es error). `pub fn auth_token() -> String`. La lógica pura (`error_message(status_text: &str, body: &Value, is_post: bool, ok: bool) -> Option<String>`) se prueba en host.
  - `comandos_web_dom::bridge`: `pub fn export_fn0(name: &str, f: impl Fn() + 'static)`, `export_fn1(name, f: impl Fn(JsValue) -> JsValue + 'static)`, `export_fn2(..)`, `export_obj(name: &str, obj: &js_sys::Object)`, `pub fn global_get(name: &str) -> JsValue`, `global_set(name: &str, v: &JsValue)`, `pub fn call_global(name: &str, args: &[JsValue]) -> Result<JsValue, JsValue>` (llama a una función que aún vive en JS; `Err` si no existe). Las clausuras se guardan en un `thread_local` (`Vec<Closure<..>>`) para que vivan lo que la página.
  - `comandos_web_dom::dom`: `by_id(id) -> Option<Element>`, `query(sel)`, `query_all(sel) -> Vec<Element>`, `set_html(el, html: &str)`, `on(el, ev, f) -> Listener` (al soltarse quita el oyente), `create(tag)`.
  - `comandos_web_dom::{storage::{get, set, remove}, timers::{interval, timeout, Handle}, log::ui_log(event: &str, data: &Value)}` (el registro local de uso de la región «registro LOCAL de uso»: mismo endpoint y forma).
  - `comandos_web::registry::{Component, mount_all}`: `pub struct Component { pub id: &'static str, pub mount: fn() -> Result<(), JsValue> }`, `pub const COMPONENTS: &[Component]` (crece con cada port); `#[wasm_bindgen] pub fn boot(k: &str)`: lee `<meta name="comandos-web" content="id1 id2 …">`, monta en ese orden, marca `window.__comandosReady = true` cuando todo montó y hace `POST /web/ready {"k": k, "mounted": [...], "failed": [{"id","error"}]}`. Un componente que falla se registra y **no** impide montar los demás; el servidor decide (B2).
  - `crates/comandos-web/components.json` (lo lee el servidor, `include_str!`): `[{"id","kind":"script"|"region"|"page","source","marker_start","marker_end","sha256","exports":[...],"deps":[...]}]`; empieza vacío `[]`.
  - `crates/comandos-web/interop.json`: nombres globales que `cc-app`, `cc-app-mac`, los iframes y los demás scripts llaman de cada componente (lo rellena B3); la prueba `interop_exports_are_declared` exige que cada componente activo declare en `exports` todos los suyos.

- [ ] **Step 1: Prueba que falla**

```rust
// crates/comandos-web-view/tests/escape.rs
use comandos_web_view::escape::{md_esc, text};
#[test]
fn escapes_like_the_dashboard() {
    assert_eq!(text(r#"<a href="x">'&'</a>"#), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
    assert_eq!(md_esc("<b>&\"'</b>"), "&lt;b&gt;&amp;\"'&lt;/b&gt;");
}
#[test]
fn maud_escapes_user_text() {
    let name = "<img src=x onerror=alert(1)>";
    let html = maud::html! { span.row-name { (name) } }.into_string();
    assert!(!html.contains("<img"), "{html}");
}
```

```rust
// crates/comandos-web-dom/tests/api_host.rs
use comandos_web_dom::api::error_message;
use serde_json::json;
#[test]
fn api_error_rules_match_the_inline_api_function() {
    assert_eq!(error_message("Bad", &json!({"error": "x"}), false, false).as_deref(), Some("x"));
    assert_eq!(error_message("Bad", &json!({"message": "m"}), false, false).as_deref(), Some("m"));
    assert_eq!(error_message("", &json!({}), true, true), None);
    assert_eq!(error_message("", &json!({"ok": false}), true, true).as_deref(), Some("La acción no se completó"));
    assert_eq!(error_message("", &json!({"ok": false}), false, true), None, "GET no mira ok:false");
}
```

```rust
// crates/comandos-web/tests/registry.rs
#[test]
fn components_json_parses_and_ids_are_unique() {
    let raw = include_str!("../components.json");
    let list: Vec<serde_json::Value> = serde_json::from_str(raw).unwrap();
    let mut ids: Vec<&str> = list.iter().filter_map(|c| c["id"].as_str()).collect();
    let n = ids.len(); ids.sort(); ids.dedup();
    assert_eq!(n, ids.len());
}
```

- [ ] **Step 2:** `$C test -p comandos-web-view -p comandos-web-dom -p comandos-web` → FAIL.
- [ ] **Step 3: Implementación.** Cargo: `comandos-web-dom` y `comandos-web-view` son `lib` con dependencias web detrás de `#[cfg(target_arch = "wasm32")]` (la lógica pura compila y se prueba en host); `comandos-web` es `crate-type = ["cdylib", "rlib"]`. Rasgos de `web-sys` de `comandos-web-dom`: `Window`, `Document`, `Element`, `HtmlElement`, `HtmlInputElement`, `HtmlTemplateElement`, `Node`, `NodeList`, `Event`, `EventTarget`, `KeyboardEvent`, `MouseEvent`, `PointerEvent`, `CustomEvent`, `Storage`, `Headers`, `Request`, `RequestInit`, `Response`, `Location`, `History`, `UrlSearchParams`, `Url`, `HtmlMetaElement`, `Performance`, `MutationObserver`, `ResizeObserver`, `IntersectionObserver`, `AudioContext`, `OfflineAudioContext`, `AudioBuffer`, `OscillatorNode`, `GainNode`, `BiquadFilterNode`, `AudioParam`, `AudioDestinationNode`, `BroadcastChannel`, `EventSource`, `MessageEvent`. El `fetch` de `api.rs` usa `RequestInit` sin `unwrap` (`Result<_, JsValue>` → `ApiError`). Cuando `window.fetch` está sustituido por un doble de pruebas (los e2e actuales lo hacen), el WASM usa `window.fetch` dinámicamente (`js_sys::Reflect::get(&window, "fetch")`), no el enlace estático, para que los dobles sigan funcionando.
- [ ] **Step 4:** PASS; `cargo run -p xtask -- web-build --crate comandos-web --check-budget` (WASM casi vacío).
- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/comandos-web-dom crates/comandos-web-view crates/comandos-web
git commit -m "feat(web): crates base del tablero en Rust (DOM, API, puente de globales, plantillas maud)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B2: Compositor de la página, compuerta de arranque, selección por componente y `comandos web`

**Depende de:** B1.

**Files:**
- Create: `crates/comandos-server/src/dash/web/{mod.rs,registry.rs,compose.rs,gate.rs,assets.rs,status.rs}`, `crates/comandos-cli/src/web.rs`
- Modify: `crates/comandos-server/src/dash/{mod.rs,router.rs}` (`RouteClass::Web(WebRoute)` antes de `Native`), `crates/comandos-cli/src/dispatch.rs` (`Command::Web`)
- Test: `crates/comandos-server/tests/web_compose.rs`, `crates/comandos-cli/tests/web_cli.rs`

**Interfaces:**
- Consumes: `components.json` (B1, `include_str!` desde el crate `comandos-web` como dependencia de build de datos), `DashConfig.web_dir` (T4), `statics::serve`.
- Produces:
  - `pub struct Selection { pub on: BTreeSet<String>, pub shadow: BTreeSet<String> }`; `Selection::load(path: &Path) -> Selection` (archivo ausente o inválido → vacía; releída si cambia el `mtime`, como mucho una lectura por segundo).
  - `pub enum ComponentState { Off, On, Shadow, Drift { expected: String, found: String }, MissingSource, MissingDep(String) }`.
  - `pub struct Composed { pub html: Vec<u8>, pub active: Vec<String>, pub states: BTreeMap<String, ComponentState> }`; `pub fn compose(page: &[u8], reg: &Resolved, sel: &Selection, shadow: bool, assets: &Manifest, nonce: &str) -> Composed`; `pub struct Resolved` (entradas + lector de orígenes): `Resolved::from_repo(entries: Vec<Entry>, repo: &Path) -> Resolved` en producción y `Resolved::in_memory(entries: Vec<Entry>, sources: &[(&str, &str)]) -> Resolved` en pruebas, con `entries()` y `source_hash(&Entry) -> Option<String>`; `Entry::script(id, source, sha256, exports, deps)` y `Entry::region(id, source, marker_start, marker_end, sha256, exports, deps)`; `pub struct Manifest` con `path(logical: &str) -> String` y `Manifest::test()` (todo bajo `0123456789ab/`).
  - Rutas (`WebRoute`): `GET /` y `/index.html` (con al menos un componente aplicable) → `compose`; `?web=off` → bytes del disco intactos; `?web=shadow` → igual que `/` con la sombra aplicada y `Set-Cookie: cc_web=shadow; Path=/; SameSite=Strict; HttpOnly`; la cookie `cc_web=shadow` activa la sombra en peticiones siguientes (los iframes la heredan); `GET /web/gate.js?k=<nonce>` (espera hasta 8 s al `ready`); `POST /web/ready` (`{"k", "mounted", "failed"}` → `{"ok":true}`; nonce desconocido → 400 `{"error":"nonce desconocido"}`); `GET /web/status` (estados por componente, artefactos, nonces en vuelo); `GET /web/<hash>/<archivo>` → `web_dir/<hash>/<archivo>` con `Cache-Control: public, max-age=31536000, immutable` y tipo MIME de `statics::mime_for`.
  - `comandos web set <id> on|off|shadow` y `comandos web status` (escritura atómica de `H/comandos-web.json` con `files::write_text_atomic`; `id` desconocido → error y código 2).
  - Límites: 256 nonces en vuelo (el 257.º sirve la página heredada sin componentes y anota `gate_full` en `/web/status`); caducidad 10 s.

- [ ] **Step 1: Prueba que falla**

```rust
// crates/comandos-server/tests/web_compose.rs
use comandos_server::dash::web::{compose::compose, registry::{Entry, Resolved}, Selection, ComponentState, Manifest};
use std::collections::BTreeSet;

const PAGE: &str = "<meta charset=\"utf-8\">\n<title>ComandOS</title>\n\
<script src=\"/quick-terminal.js?v=2\"></script>\n<script>\n// ---------- red ----------\nfunction api(){}\n// ---------- helpers ----------\nfunction esc(){}\n</script>\n";

fn sha(s: &str) -> String { comandos_server::dash::web::registry::sha256_hex(s.as_bytes()) }

fn entries(qt_sha: &str) -> Vec<Entry> {
    vec![
        Entry::script("quick-terminal", "dash/quick-terminal.js", qt_sha, &["ComandosQuickTerminal"], &[]),
        Entry::region("red", "dash/index.html", "// ---------- red ----------", "// ---------- helpers ----------",
            &sha("// ---------- red ----------\nfunction api(){}\n"), &["api", "authToken"], &[]),
    ]
}

fn sel(on: &[&str]) -> Selection { Selection { on: on.iter().map(|s| s.to_string()).collect(), shadow: BTreeSet::new() } }

#[test]
fn active_script_component_is_removed_and_boot_injected_after_charset() {
    let src = "(function(){})()";
    let c = compose(PAGE.as_bytes(), &Resolved::in_memory(entries(&sha(src)), &[("dash/quick-terminal.js", src)]),
        &sel(&["quick-terminal"]), false, &Manifest::test(), "n1");
    let html = String::from_utf8(c.html).unwrap();
    assert!(!html.contains("quick-terminal.js"));
    let after_charset = html.split_once("<meta charset=\"utf-8\">\n").unwrap().1;
    assert!(after_charset.starts_with("<meta name=\"comandos-web\" content=\"quick-terminal\">"));
    assert!(html.contains("<script type=\"module\" async src=\"/web/0123456789ab/boot.js\" data-k=\"n1\"></script>"));
    assert!(html.contains("<script src=\"/web/gate.js?k=n1\"></script>"));
}

#[test]
fn compose_drift_keeps_legacy() {
    // Review Focus 1: otra sesión editó el JS después del port.
    let c = compose(PAGE.as_bytes(), &Resolved::in_memory(entries(&sha("versión portada")), &[("dash/quick-terminal.js", "versión nueva")]),
        &sel(&["quick-terminal"]), false, &Manifest::test(), "n1");
    let html = String::from_utf8(c.html).unwrap();
    assert!(html.contains("quick-terminal.js"), "el JS heredado se sigue sirviendo");
    assert!(matches!(c.states["quick-terminal"], ComponentState::Drift { .. }));
    assert!(!html.contains("comandos-web"), "sin componentes activos no se inyecta nada");
}

#[test]
fn region_is_cut_only_with_matching_hash() {
    let c = compose(PAGE.as_bytes(), &Resolved::in_memory(entries("x"), &[]), &sel(&["red"]), false, &Manifest::test(), "n1");
    let html = String::from_utf8(c.html).unwrap();
    assert!(!html.contains("function api(){}") && html.contains("function esc(){}"));
}

#[test]
fn shadow_only_applies_to_shadow_requests() {
    let mut s = sel(&[]);
    s.shadow.insert("red".into());
    let off = compose(PAGE.as_bytes(), &Resolved::in_memory(entries("x"), &[]), &s, false, &Manifest::test(), "n1");
    assert_eq!(off.html, PAGE.as_bytes(), "sin sombra la página es la del disco, byte a byte");
    let on = compose(PAGE.as_bytes(), &Resolved::in_memory(entries("x"), &[]), &s, true, &Manifest::test(), "n1");
    assert!(!String::from_utf8(on.html).unwrap().contains("function api(){}"));
}

#[test]
fn missing_dependency_keeps_component_off() {
    let mut es = entries("x");
    es.push(Entry::script("dep-user", "dash/quick-terminal.js", &sha("q"), &[], &["no-existe"]));
    let c = compose(PAGE.as_bytes(), &Resolved::in_memory(es, &[("dash/quick-terminal.js", "q")]), &sel(&["dep-user"]), false, &Manifest::test(), "n1");
    assert!(matches!(c.states["dep-user"], ComponentState::MissingDep(_)));
}
```

Pruebas de la compuerta (frente de prueba completo, `support::front` con `web_dir` temporal):

```rust
#[tokio::test(flavor = "current_thread")]
async fn gate_releases_on_ready_and_web_off_is_byte_exact() { /* GET / → extraer nonce; GET /web/gate.js?k=… en paralelo con
   POST /web/ready {"k":…} → gate 200 cuerpo vacío en < 100 ms; GET /?web=off → bytes del index.html del disco */ }

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn gate_timeout_falls_back_once() {
    // Review Focus 2: sin ready en 8 s la compuerta pide recargar con web=off, una sola vez por pestaña.
    /* GET /web/gate.js?k=n sin ready; avanzar el reloj 8 s → cuerpo ==
       "if(!sessionStorage.cc_web_fallback){sessionStorage.cc_web_fallback=1;location.replace(<URL con web=off>)}"
       y /ui-log recibió {"event":"web-gate-timeout"}. */
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_nonce_and_cap() { /* ready con nonce inventado → 400; 257 páginas sin ready → la 257 sin inyección y /web/status gate_full */ }
```

(Los cuerpos de estas tres pruebas se escriben completos con el cliente hyper de `support`; las aserciones son las de los comentarios.)

- [ ] **Step 2:** `$C test -p comandos-server --test web_compose` → FAIL.
- [ ] **Step 3: Implementación.**

```rust
// crates/comandos-server/src/dash/web/compose.rs (núcleo)
//! Página compuesta: la del disco menos los componentes activos cuyo origen
//! no cambió, más el cargador y la compuerta. Sin componentes aplicables, los
//! bytes del disco tal cual (paridad exacta con la 2a).
pub fn compose(page: &[u8], reg: &Resolved, sel: &Selection, shadow: bool, assets: &Manifest, nonce: &str) -> Composed {
    let mut states = BTreeMap::new();
    let wanted = |id: &str| sel.on.contains(id) || (shadow && sel.shadow.contains(id));
    let mut active: Vec<&Entry> = Vec::new();
    for e in reg.entries() {
        let state = if !wanted(&e.id) { ComponentState::Off }
            else { match reg.source_hash(e) {
                None => ComponentState::MissingSource,
                Some(found) if found != e.sha256 => ComponentState::Drift { expected: e.sha256.clone(), found },
                Some(_) => if sel.on.contains(e.id.as_str()) { ComponentState::On } else { ComponentState::Shadow },
            } };
        states.insert(e.id.clone(), state);
    }
    // Dependencias: un componente sin sus dependencias activas se apaga (punto fijo).
    loop {
        let mut changed = false;
        for e in reg.entries() {
            if !matches!(states.get(&e.id), Some(ComponentState::On | ComponentState::Shadow)) { continue; }
            if let Some(dep) = e.deps.iter().find(|d| !matches!(states.get(*d), Some(ComponentState::On | ComponentState::Shadow))) {
                states.insert(e.id.clone(), ComponentState::MissingDep(dep.clone()));
                changed = true;
            }
        }
        if !changed { break; }
    }
    for e in reg.entries() {
        if matches!(states.get(&e.id), Some(ComponentState::On | ComponentState::Shadow)) { active.push(e); }
    }
    if active.is_empty() { return Composed { html: page.to_vec(), active: Vec::new(), states }; }
    let mut text = String::from_utf8_lossy(page).into_owned();
    for e in &active { text = e.cut(&text); } // script: quita la etiqueta; region: quita marker_start..marker_end
    let ids: Vec<&str> = active.iter().map(|e| e.id.as_str()).collect();
    let head = format!(
        "<meta name=\"comandos-web\" content=\"{}\">\n<script type=\"module\" async src=\"/web/{}\" data-k=\"{nonce}\"></script>\n<script src=\"/web/gate.js?k={nonce}\"></script>\n",
        ids.join(" "), assets.path("comandos_web_boot.js"));
    let html = match text.split_once("<meta charset=\"utf-8\">\n") {
        Some((before, after)) => format!("{before}<meta charset=\"utf-8\">\n{head}{after}"),
        None => format!("{head}{text}"),
    };
    Composed { html: html.into_bytes(), active: ids.iter().map(|s| s.to_string()).collect(), states }
}
```

`Entry::cut` para `script`: quita la primera etiqueta `<script src="/?<archivo>(?…)?"…></script>` y su salto de línea (búsqueda literal del nombre de archivo dentro de una etiqueta `<script`, sin regex sobre todo el documento); para `region`: localiza `marker_start` y el siguiente `marker_end` dentro del script en línea, calcula el hash de `marker_start..marker_end` (lo que valida `Drift`) y quita ese tramo. `Resolved` lee los orígenes de `repo_root` (`COMANDOS_DASH_REPO`) con caché por `mtime`. `gate.rs`: `HashMap<String, (Instant, watch::Sender<bool>)>` bajo `Mutex` con cota 256 y barrido de caducados en cada inserción; `GET /web/gate.js` espera `ready` o 8 s con `tokio::time::timeout`. La página compuesta y `gate.js` llevan `Cache-Control: no-store` (el transporte ya lo pone en todo). `/`, `/index.html`, `/web/gate.js` y `/web/ready` pasan la puerta como hoy `/` (estáticos públicos según `public_asset`); `/web/status` exige el acceso del tablero.

- [ ] **Step 4:** pruebas → PASS; `$C test -p comandos-server` → PASS (con selección vacía nada cambia: la prueba de la 2a de estáticos sigue en verde).
- [ ] **Step 5: CLI.** `crates/comandos-cli/tests/web_cli.rs`: `web set quick-terminal shadow` con HOME temporal escribe `{"on":[],"shadow":["quick-terminal"]}`; `web set nada on` → código 2; `web status` imprime la tabla de `/web/status` (pide al 4777 si responde; si no, calcula desde los archivos). Implementar → PASS.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/web crates/comandos-server/src/dash/mod.rs crates/comandos-server/src/dash/router.rs crates/comandos-server/tests/web_compose.rs crates/comandos-cli/src/web.rs crates/comandos-cli/src/dispatch.rs crates/comandos-cli/tests/web_cli.rs
git commit -m "feat(dash): página compuesta por componentes con compuerta de arranque, deriva y selección sin reinicio

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B3: Inventario de la interfaz y vigilancia del objetivo en movimiento (`xtask web-inventory`, `web-port check`)

**Independiente:** sí (no necesita B1/B2).

**Files:**
- Create: `xtask/src/web_inventory.rs`, `xtask/src/web_port.rs`, `xtask/web/inventory.json`, `xtask/web/interop.json`, `docs/research/2026-10-04-fase-3-web-inventario.md`
- Test: `xtask/tests/web_inventory.rs` (con `xtask/tests/fixtures/web/{index.html,a.js,b.js,cc-app.py.txt}`)

**Interfaces:**
- Produces:
  - `web_inventory::Unit { id, kind: Script|Region, source, marker_start, marker_end, sha256, lines, defines: Vec<String>, uses_globals: Vec<String>, mutates_globals: Vec<String>, dom_ids: Vec<String>, routes: Vec<String>, storage_keys: Vec<String>, post_messages: Vec<String>, intervals_ms: Vec<u64>, strings: usize }`.
  - `web_inventory::scan(repo: &Path) -> Vec<Unit>` sobre `dash/index.html` (regiones por los marcadores `// ---------- … ----------` del primer script en línea; el segundo script en línea es la región `tail`), `dash/term.html` (regiones del mismo modo; consumido por A9/A10), `dash/*.js`.
  - `interop.json`: por nombre global, quién lo define y quién lo usa: otros `Unit`, `bin/cc-app` (`run_javascript(...)` literales y `f"try{{{code}}}"` con el origen de `code`), `bin/cc-app-mac` (`evaluateJavaScript`), iframes (`postMessage` `type`).
  - `cargo run -p xtask -- web-inventory --out xtask/web/inventory.json --doc docs/research/2026-10-04-fase-3-web-inventario.md`.
  - `cargo run -p xtask -- web-port check`: compara `sha256` de cada entrada de `crates/comandos-web/components.json` con el origen actual del checkout principal (o `--repo DIR`) y lista `ok`/`drift` con el diff de líneas (`git diff --no-index` del texto portado guardado en `xtask/web/ported/<id>.txt` contra el actual). Código 1 si hay deriva.
  - `cargo run -p xtask -- web-port pin <id>`: guarda el texto y el hash actuales del origen de `<id>` (`xtask/web/ported/<id>.txt`) y actualiza `components.json`.

- [ ] **Step 1: Prueba que falla**

```rust
// xtask/tests/web_inventory.rs
use xtask::web_inventory::scan;
#[test]
fn finds_regions_globals_routes_and_host_calls() {
    let units = scan(std::path::Path::new("tests/fixtures/web"));
    let red = units.iter().find(|u| u.id == "region:red").unwrap();
    assert!(red.defines.contains(&"api".to_string()));
    assert!(red.routes.contains(&"/webterm-token".to_string()));
    let a = units.iter().find(|u| u.id == "script:a.js").unwrap();
    assert!(a.uses_globals.contains(&"api".to_string()), "a.js llama a api() definido en la región red");
    assert!(a.storage_keys.contains(&"cc-axo".to_string()));
    assert_eq!(a.intervals_ms, vec![2000]);
    let interop: serde_json::Value = xtask::web_inventory::interop(&units, std::path::Path::new("tests/fixtures/web"));
    assert_eq!(interop["pomoRender"]["called_by"][0], "bin/cc-app");
}
```

Fixtures: `index.html` con un script en línea que contiene `// ---------- red ----------\nasync function api(path){ return fetch("/webterm-token") }\n// ---------- helpers ----------\nfunction esc(s){return s}`; `a.js` con `api("/state"); localStorage.getItem("cc-axo"); setInterval(tick, 2000); function pomoRender(){}`; `cc-app.py.txt` con `wv.run_javascript("try{typeof pomoRender==='function'&&pomoRender()}catch(e){}", None, None, None)`.

- [ ] **Step 2:** `$C test -p xtask --test web_inventory` → FAIL.
- [ ] **Step 3: Implementación** léxica (sin ejecutar JS): un tokenizador mínimo que salta cadenas, plantillas y comentarios para no confundir texto con código; `defines` = `function X`, `async function X`, `const|let|var X` a nivel superior, `window.X =`, `root.X =`; `uses_globals` = identificadores libres llamados (`X(`) o leídos que coinciden con `defines` de otra unidad; `mutates_globals` = `X =` sobre un global de otra unidad (deben quedar como propiedades de `window` mientras ambos lados vivan, B4 Step 3); `routes` = primer argumento literal de `api(`, `fetch(`, `new EventSource(`; `dom_ids` = `getElementById("…")` y `#id` en `querySelector`; `storage_keys` = `localStorage.getItem|setItem|removeItem("…")`; `intervals_ms` = segundo argumento literal de `setInterval`.
- [ ] **Step 4:** PASS. Ejecutar sobre el checkout principal y comitear `inventory.json`, `interop.json` y el documento (tabla por unidad: líneas, globales definidos/usados, rutas, `localStorage`, intervalos, llamadas desde `cc-app`; y el **orden de port** que sigue de las dependencias: hojas primero).
- [ ] **Step 5: Commit**

```bash
git add xtask/src/web_inventory.rs xtask/src/web_port.rs xtask/src/main.rs xtask/tests/web_inventory.rs xtask/tests/fixtures/web xtask/web/inventory.json xtask/web/interop.json docs/research/2026-10-04-fase-3-web-inventario.md
git commit -m "feat(xtask): inventario léxico de la interfaz, interoperabilidad con cc-app y vigilancia de deriva

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B4: Procedimiento de port, cutover de componente y primer port: `ui-sounds` (UISFX en Web Audio)

**Depende de:** B1, B2, B3, T2.

**Files:**
- Create: `crates/comandos-web/src/components/ui_sounds.rs`, `crates/comandos-web-dom/src/audio.rs` (síntesis de las señales que usa el catálogo), `docs/verification/cutover-web.md`, `xtask/web/fixtures/sounds/`
- Modify: `crates/comandos-web/{src/registry.rs,components.json}`, `xtask/web/shots.json` (`boot`), `xtask/src/web_bench.rs` (subcomando `audio-diff`)
- Test: `crates/comandos-web-dom/tests/audio_host.rs`, e2e `boot_failure_falls_back`, `host_calls_still_resolve`, `audio-diff`

**Interfaces:**
- Consumes: puente (B1), compositor (B2), `interop.json` (B3), `xtask shots` (T2).
- Produces:
  - Componente `ui-sounds` (`kind: script`, `source: dash/ui-sounds.js`, `exports: ["ComandosUISounds"]`): la misma API (`play(cue, {eventId, volume})`, `preview`, `stop`, `setEnabled`, `isEnabled`, `setVolume`, `getVolume`, `register`, `cues`, `unlock`) con las mismas reglas (opt-in, sin bucles, una reproducción por `eventId` compartida entre pestañas por `BroadcastChannel`/`localStorage` como hoy, silencio con la página oculta, audio solo tras gesto confiable, nada remoto) y el mismo `DEFAULT_CATALOG`.
  - `audio::render_cue(name: &str, sample_rate: f32) -> Vec<f32>`: síntesis en Rust de las señales de UISFX 0.4.0 usadas por el catálogo (`open`, `close`, `complete`, `success`, `level-up`, `notification`, `error`, `warning`, más las que el inventario encuentre), probada en host; en web se reproduce como `AudioBuffer` (sin osciladores en vivo: un búfer precalculado y cacheado por señal, más barato y determinista).
  - **Procedimiento de port** (texto en `cutover-web.md`, lo siguen B5–B13): (1) `web-port pin <id>` sobre el origen actual; (2) leer su entrada de `inventory.json`/`interop.json`; (3) plantillas en `comandos-web-view` con prueba de DOM normalizado contra el HTML que genera el JS (fixtures capturadas con `shots dom-dump`); (4) lógica en `comandos-web/src/components/<id>.rs`, globales del `interop.json` exportados con `bridge`, globales mutables compartidos como propiedades de `window` (`global_get/global_set`) mientras su otro lado siga en JS; (5) portar los casos de `tests/<id>_checks.cjs` a pruebas Rust (host o `wasm-bindgen-test`) con los mismos nombres; (6) entrada en `components.json`; (7) `shots pair --suite <id>` en el Mac contra fixtures y `shots remote-vs-desktop --suite <id>`; (8) sombra 24 h en el teléfono y en un navegador de escritorio contra 4782 (`--shadow-readonly`); (9) `comandos web set <id> on`; (10) revertir: `comandos web set <id> off`.

- [ ] **Step 1: Prueba que falla (host)**

```rust
// crates/comandos-web-dom/tests/audio_host.rs
use comandos_web_dom::audio::{render_cue, CATALOG};
#[test]
fn every_catalog_cue_renders_short_and_bounded() {
    for (cue, _) in CATALOG {
        let s = render_cue(cue, 48_000.0);
        assert!(!s.is_empty() && s.len() < 48_000 * 2, "{cue}: sin bucles ni colas largas");
        assert!(s.iter().all(|v| v.abs() <= 1.0), "{cue}: sin saturar");
    }
}
#[test]
fn loop_cues_are_refused() {
    for cue in ["loading", "processing", "recording", "connecting", "scanning", "streaming"] {
        assert!(render_cue(cue, 48_000.0).is_empty());
    }
}
```

- [ ] **Step 2:** `$C test -p comandos-web-dom --test audio_host` → FAIL.
- [ ] **Step 3: Implementación** de `audio.rs` siguiendo las recetas de `assets/uisfx/uisfx-0.4.0.js` para cada señal del catálogo (tipo de onda, frecuencias, envolvente ADSR, filtros, duración), y del componente siguiendo `dash/ui-sounds.js` línea a línea.
- [ ] **Step 4: Paridad de audio en el Mac.** `cargo run -p xtask -- web-bench audio-diff --base http://127.0.0.1:<p>`: en la misma página de fixtures, `evaluate_script` renderiza cada señal con `OfflineAudioContext(1, …, 48000)` dos veces —con UISFX JS y con el WASM (función de prueba exportada solo en `COMANDOS_DASH_TEST_HOOKS=1`)— y devuelve la diferencia absoluta máxima. Criterio: ≤ 1e-3 por muestra y misma duración. Resultado en `docs/verification/fase3/web-ui-sounds.json`.
- [ ] **Step 5: E2e de arranque** (en el Mac, frente de prueba con fixtures):
  - `host_calls_still_resolve` (Review Focus 3): con `ui-sounds` activo, `evaluate_script` de cada llamada de `interop.json` que toca a este componente (`ComandosUISounds.play('complete')`, etc.) no lanza.
  - `boot_failure_falls_back` (Review Focus 2): el arnés sirve un `boot.js` que no carga (variable `COMANDOS_DASH_TEST_BOOT_FAIL=1` del frente de prueba): a los 8 s la pestaña está en `?web=off`, `sessionStorage.cc_web_fallback === "1"` y una segunda carga no vuelve a redirigir.
  - `shots pair --suite boot` y `--suite settings-sounds` (Ajustes › Sonido): 0 recortes fuera de tolerancia.
- [ ] **Step 6: Cutover** según el procedimiento (Steps 8–10 de arriba) — lo ejecuta el controlador. Documentar en `cutover-web.md` la tabla de estado por componente (fecha de sombra, de activación, release, notas de deriva).
- [ ] **Step 7: Commit**

```bash
git add crates/comandos-web crates/comandos-web-dom/src/audio.rs crates/comandos-web-dom/tests/audio_host.rs xtask/src/web_bench.rs xtask/web/shots.json xtask/web/fixtures/sounds docs/verification/cutover-web.md docs/verification/fase3/web-ui-sounds.json
git commit -m "feat(web): procedimiento de port por componente y primer port, ui-sounds con UISFX sintetizado en Rust

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B5: Hojas — `quick-terminal`, `device-drafts`, `session-config`, `workspace-layout`, `push-settings`

**Depende de:** B4. **Paralela a** B6–B11.

**Files:**
- Create: `crates/comandos-web/src/components/{quick_terminal,session_config,workspace_layout,push_settings}.rs`, `crates/comandos-web-dom/src/drafts.rs`, plantillas en `crates/comandos-web-view/src/{session_config,push_settings}.rs`
- Modify: `crates/comandos-web/{src/registry.rs,components.json}`, `xtask/web/shots.json` (suites `push-settings`, `session-config`)
- Test: pruebas Rust que portan `tests/device_drafts_checks.cjs`, `tests/push_settings_checks.cjs`, `tests/workspace_layout_checks.cjs` (mismos nombres de caso), y las de `quick-terminal` de abajo

**Interfaces:**
- Produces (exportaciones exactas de `interop.json`):
  - `ComandosQuickTerminal.createQuickTerminal({api, openTerm, toast, onOpened, storage, makeId, place, storageKey})` → `{open(), pendingRequestId, busy}` con la idempotencia de `quick-terminal.js` (un `requestId` por clic reutilizado hasta el éxito, clave `comandos.quickTerminal.pending`).
  - `comandos_web_dom::drafts` (Rust, lo usa A10) y su exportación JS con el nombre global de `device-drafts.js`.
  - `session-config`, `workspace-layout`, `push-settings` con sus globales del inventario.

- [ ] **Step 1: Pruebas que fallan.** Para `quick-terminal`, en host con un `Api` falso:

```rust
// crates/comandos-web/src/components/quick_terminal.rs (pruebas)
#[cfg(test)]
mod tests {
    use super::Core;
    #[test]
    fn same_request_id_until_success_then_new() {
        let mut c = Core::new(None, || "id-1".to_string());
        assert_eq!(c.begin().as_deref(), Some("id-1"));
        assert_eq!(c.begin(), None, "doble clic mientras vuela: no hay segunda petición");
        c.fail();
        assert_eq!(c.begin().as_deref(), Some("id-1"), "tras un error se reutiliza el mismo id");
        c.succeed("id-1");
        let mut c2 = Core::new(None, || "id-2".to_string());
        assert_eq!(c2.begin().as_deref(), Some("id-2"));
    }
    #[test]
    fn stored_pending_id_survives_reload() {
        let mut c = Core::new(Some("guardado".into()), || "nuevo".into());
        assert_eq!(c.begin().as_deref(), Some("guardado"));
    }
}
```

y los casos de los tres `*_checks.cjs` traducidos uno a uno (cada `assert` del archivo JS es un `assert!` en Rust con los mismos datos de entrada).

- [ ] **Step 2:** `$C test -p comandos-web -p comandos-web-dom` → FAIL.
- [ ] **Step 3: Implementación** por el procedimiento de B4 (pin, plantillas, lógica, exportaciones, `components.json` con `kind: script` y `deps: []`).
- [ ] **Step 4:** PASS; en el Mac `shots pair --suite push-settings` y `--suite session-config` y `shots remote-vs-desktop` de ambas: 0 fallos. Resultados en `docs/verification/fase3/web-b5.json`.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-web crates/comandos-web-dom/src/drafts.rs crates/comandos-web-view xtask/web/shots.json docs/verification/fase3/web-b5.json
git commit -m "feat(web): hojas en Rust — terminal rápida, borradores, configuración de sesión, disposición y avisos push

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B6: `work-marks` y `pomodoro`

**Depende de:** B4 (`ui-sounds` activo: el Pomodoro suena por él). **Paralela a** B5, B7–B11.

**Files:**
- Create: `crates/comandos-web/src/components/{work_marks,pomodoro}.rs`, `crates/comandos-web-view/src/{work_marks,pomodoro}.rs`
- Modify: `components.json` (`work-marks`, `pomodoro` con `deps: ["ui-sounds"]`), `xtask/web/shots.json` (`marks`, `pomodoro`)
- Test: port de `tests/work_marks_checks.cjs`, `tests/pomodoro_ui_checks.cjs`, `tests/pomodoro_sync_checks.cjs`

**Interfaces:**
- Produces: globales `pomoRender` (lo llama `cc-app` por `run_javascript`) y los demás de `interop.json`; el mismo DOM del panel (anillo SVG con `var(--brand)`, 15/25/50, descansos ☕ 5/10 en `--done`, tiles hoy/min/racha, barras de 7 días, enlace a Dedicación) y del menú de marcas (`menuItems`, `paint`, `indicator`, iconos `aiIconSvg`/`iconSvg`).

- [ ] **Step 1: Pruebas que fallan**: cada caso de los tres `*.cjs` como prueba Rust con su nombre; además `pomodoro_is_manual_never_autostarts` (memoria del usuario: él lo inicia) y `marks_menu_dom_matches_legacy` (`dom_diff` contra `xtask/web/fixtures/marks/menu.html`).
- [ ] **Step 2:** FAIL.
- [ ] **Step 3: Implementación** por el procedimiento de B4. Los temporizadores del Pomodoro siguen el reloj del servidor (`GET /pomodoro`) exactamente como `pomodoro.js` (misma tolerancia de resincronización); el sondeo se sustituye por SSE en B14, no aquí.
- [ ] **Step 4:** PASS; `shots pair --suite marks,pomodoro` con `__clock` fijo y máscara sobre los segundos; `remote-vs-desktop`: 0 fallos; `host_calls_still_resolve` con `pomoRender()`.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-web crates/comandos-web-view xtask/web/shots.json xtask/web/fixtures/marks
git commit -m "feat(web): marcas de trabajo y Pomodoro en Rust

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B7: Analítica (`analytics-render`, `analytics`) y avisos (`notifications`)

**Depende de:** B4. **Paralela a** B5, B6, B8–B11.

**Files:**
- Create: `crates/comandos-web/src/components/{analytics,notifications}.rs`, `crates/comandos-web-view/src/{analytics,notifications}.rs`
- Modify: `components.json` (`analytics` cubre los dos archivos: dos entradas `script` con `deps` mutuas, se activan juntas; `notifications` con `deps: ["ui-sounds"]`), `xtask/web/shots.json`
- Test: port de `tests/analytics_ui_checks.cjs`, `analytics_parity_checks.cjs`, `analytics_agy_checks.cjs`, `notification_ui_checks.cjs`

**Interfaces:**
- Produces: globales `notifRender` (llamado por `cc-app`) y los de `interop.json`; render de Cuentas · Comparar · Pomodoro idéntico al módulo generado del mockup; avisos con la regla de texto completo (memoria: «Ver TODO» siempre presente, nunca truncar sin acceso al texto entero) y el diseño «lectura primero».

- [ ] **Step 1: Pruebas que fallan**: casos de los cuatro `*.cjs`; `notice_full_text_is_always_reachable` (un aviso de 5 000 caracteres: el DOM contiene el texto completo, accesible por «Ver TODO»).
- [ ] **Step 2:** FAIL.
- [ ] **Step 3: Implementación** por el procedimiento de B4. El long-poll `GET /notices/watch` (25 s) se conserva tal cual (una petición en vuelo, mismo `AbortController` lógico).
- [ ] **Step 4:** PASS; `shots pair --suite analytics,notices` (fixtures `analytics_week`, `usage_state`, `notices`), `remote-vs-desktop`; `host_calls_still_resolve` con `notifRender()`.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-web crates/comandos-web-view xtask/web/shots.json xtask/web/fixtures
git commit -m "feat(web): analítica y avisos en Rust con texto completo siempre accesible

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B8: Markdown en el servidor (`pulldown-cmark` + `ammonia`) y lector de noticias

**Depende de:** B4. **Paralela a** B5–B7, B9–B11.

**Files:**
- Create: `crates/comandos-server/src/dash/web/markdown.rs`, `crates/comandos-web/src/components/news_reader.rs`, `crates/comandos-web-view/src/news.rs`
- Modify: `crates/comandos-server/src/dash/native/mod.rs` (ruta `POST /web/markdown`), `components.json` (`news-reader`; además retira del documento las etiquetas de `vendor/markdown-it-15.0.2.umd.min.js` y `vendor/purify-3.4.16.min.js` como entradas `script` con `deps: ["news-reader"]`)
- Test: `crates/comandos-server/tests/web_markdown.rs`, port de `tests/news_reader_checks.cjs`

**Interfaces:**
- Produces:
  - `POST /web/markdown {"text": "...", "profile": "news"}` → `{"html": "..."}`; límite 256 KiB de entrada (413 `{"error":"texto demasiado largo"}`); perfil `news` = opciones de `createRenderer` (`html: false`, `linkify: true`, `typographer: false`, `breaks: false`) + saneado con la lista de etiquetas y atributos que hoy permite DOMPurify en `news-reader.js` (enlaces solo `http/https`, sin imágenes remotas, `rel="noopener noreferrer"`, `target="_blank"` si el JS lo pone).
  - `pub fn render(text: &str, profile: Profile) -> String`.
  - Componente `news-reader` que pide el HTML a esa ruta (caché por hash del texto, máx. 64 entradas) en vez de usar markdown-it en el cliente.

- [ ] **Step 1: Prueba que falla**

```rust
// crates/comandos-server/tests/web_markdown.rs
use comandos_server::dash::web::markdown::{render, Profile};
#[test]
fn raw_html_is_text_and_links_are_safe() {
    let h = render("hola <script>alert(1)</script> [x](javascript:alert(1)) https://ej.mx", Profile::News);
    assert!(!h.contains("<script") && !h.contains("javascript:"), "{h}");
    assert!(h.contains(r#"href="https://ej.mx""#), "linkify: {h}");
}
#[test]
fn remote_images_are_dropped() {
    assert!(!render("![a](https://ej.mx/a.png)", Profile::News).contains("<img"));
}
#[test]
fn tables_never_break() {
    let h = render("| a | b |\n|---|---|\n| 1 | 2 |", Profile::News);
    assert!(h.contains("<table>"), "{h}");
}
```

Más una prueba diferencial en el Mac (`xtask web-bench md-diff`): 200 textos de `xtask/web/fixtures/markdown/corpus/` (titulares y cuerpos de noticias reales anonimizados, listas, tablas, código, enlaces, HTML crudo) renderizados por markdown-it+DOMPurify en la página y por la ruta; se comparan con `dom_diff`. Las diferencias que queden se listan en `cutover-web.md` (D10) con su ejemplo; criterio: ninguna diferencia visible en el corpus de noticias reales (las de entradas sintéticas exóticas se aceptan documentadas).

- [ ] **Step 2:** FAIL.
- [ ] **Step 3: Implementación** (`pulldown_cmark::Options::ENABLE_TABLES | ENABLE_STRIKETHROUGH`, eventos `Html`/`InlineHtml` convertidos a `Text`, linkify con el autolink de GFM; `ammonia::Builder` con `url_schemes(["http","https"])`, `link_rel(Some("noopener noreferrer"))`, sin `img`). La ruta se registra como nativa de clase `POST` sin efectos.
- [ ] **Step 4:** PASS; `shots pair --suite news` y `remote-vs-desktop`.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-server/src/dash/web/markdown.rs crates/comandos-server/src/dash/native/mod.rs crates/comandos-server/tests/web_markdown.rs crates/comandos-web crates/comandos-web-view xtask/web
git commit -m "feat(web): markdown saneado en el servidor y lector de noticias en Rust; markdown-it y DOMPurify dejan de servirse

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B9: PWA — service worker en WASM, manifest y Web Push

**Depende de:** B4, T4. **Paralela a** B5–B8, B10, B11.

**Files:**
- Create: `crates/comandos-web-sw/{Cargo.toml,src/lib.rs,src/policy.rs}`, `xtask/src/web_build.rs` (generación de `sw.js` y `sw_wasm.js`)
- Modify: `crates/comandos-server/src/dash/web/assets.rs` (`GET /sw.js` generado cuando el componente `sw` está activo), `components.json` (`sw`, `kind: page`, `source: dash/sw.js`)
- Test: `crates/comandos-web-sw/tests/policy.rs`, port de `tests/sw_push_checks.cjs`

**Interfaces:**
- Produces:
  - `policy::Decision { Bypass, ShellNetworkFirst, TokenShell, CacheNetworkFirst }`; `policy::decide(method: &str, same_origin: bool, path: &str, has_token_query: bool) -> Decision` con la lista viva de `dash/sw.js` **más** `/web/gate.js`, `/web/ready`, `/web/status`, `/web/markdown` y `/events/stream` (nunca en caché); `/web/<hash>/…` sí se cachea (inmutable).
  - `pub const SHELL_CACHE: &str = "comandos-shell-v14"` (sube de v13 porque la lista viva cambia; el `activate` borra las demás, como hoy).
  - Push: `push_payload(data: &Value) -> Notification { title (≤ 60), body (≤ 120), tag, event_id }` y `click_target(event_id: &str) -> String` con las reglas de `eventIdFrom` (≤ 128, sin controles) y sin URL del payload.
  - `sw.js` generado (≤ 10 líneas): `importScripts("/web/<hash>/sw_wasm.js", "/web/<hash>/comandos_web_sw.js"); wasm_bindgen.initSync({ module: self.__COMANDOS_SW_WASM }); wasm_bindgen.install();` — `sw_wasm.js` también generado: `self.__COMANDOS_SW_WASM = Uint8Array.from(atob("<base64>"), c => c.charCodeAt(0));`. La instanciación es **síncrona** y `install()` registra `install`, `activate`, `fetch`, `push` y `notificationclick` durante la evaluación inicial, como exige el navegador (un registro asíncrono perdería eventos).

- [ ] **Step 1: Prueba que falla**

```rust
// crates/comandos-web-sw/tests/policy.rs
use comandos_web_sw::policy::{Decision, click_target, decide, push_payload};
#[test]
fn live_routes_bypass_and_shell_is_network_first() {
    for p in ["/state", "/usage/state", "/term/ws", "/web/gate.js", "/events/stream", "/operator/chat/stream", "/session-x"] {
        assert_eq!(decide("GET", true, p, false), Decision::Bypass, "{p}");
    }
    assert_eq!(decide("GET", true, "/", true), Decision::TokenShell);
    assert_eq!(decide("GET", true, "/", false), Decision::ShellNetworkFirst);
    assert_eq!(decide("GET", true, "/web/0123456789ab/comandos_web_bg.wasm", false), Decision::CacheNetworkFirst);
    assert_eq!(decide("GET", true, "/datos.bin", false), Decision::Bypass, "nunca se cachea una API desconocida");
    assert_eq!(decide("POST", true, "/", false), Decision::Bypass);
    assert_eq!(decide("GET", false, "/x.js", false), Decision::Bypass);
}
#[test]
fn push_is_bounded_and_click_never_uses_payload_urls() {
    let n = push_payload(&serde_json::json!({"title": "t".repeat(90), "body": "b".repeat(200), "eventId": "e1", "url": "https://malo"}));
    assert_eq!((n.title.chars().count(), n.body.chars().count(), n.tag.as_str()), (60, 120, "comandos-event-e1"));
    assert_eq!(click_target("e1"), "/?event=e1");
    assert_eq!(click_target("a\u{0001}b"), "/");
}
```

- [ ] **Step 2:** `$C test -p comandos-web-sw --test policy` → FAIL.
- [ ] **Step 3: Implementación** (`crate-type = ["cdylib", "rlib"]`, `wasm-bindgen --target no-modules`, rasgos `ServiceWorkerGlobalScope`, `FetchEvent`, `ExtendableEvent`, `PushEvent`, `NotificationEvent`, `Clients`, `Client`, `WindowClient`, `Cache`, `CacheStorage`, `Notification`, `NotificationOptions`) y la generación en `web_build`.
- [ ] **Step 4:** PASS; casos de `tests/sw_push_checks.cjs` portados → PASS; en el Mac (frente de prueba con HTTPS no hace falta: `localhost` es contexto seguro, se expone con `cc-browser-expose` y se abre `http://127.0.0.1:<p>`): registrar, comprobar que `/state` no pasa por la caché (`caches.match` vacío), que el shell funciona sin red tras una visita (`emulate` con `networkConditions: "Offline"`) y que `chrome-bg` muestra el SW activo; instalar la PWA en el teléfono (lo hace Jesús) y comprobar un aviso de prueba (`POST /push/test` existente).
- [ ] **Step 5: Cutover** con el procedimiento de B4 (la sombra de un SW afecta a todo el origen, así que la sombra se hace **solo** contra 4782, nunca activando `sw` en `shadow` en producción; en producción se pasa directo de `off` a `on` tras la sombra de 4782; reversión: `comandos web set sw off` sirve otra vez `dash/sw.js`, que al activarse borra `comandos-shell-v14`).
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-web-sw crates/comandos-server/src/dash/web/assets.rs crates/comandos-web/components.json xtask/src/web_build.rs Cargo.toml Cargo.lock
git commit -m "feat(web): service worker y Web Push en WASM con instanciación síncrona y shim generado

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B10: Extensiones, constructor de cadenas y barra de comandos

**Depende de:** B4. **Paralela a** B5–B9, B11.

**Files:**
- Create: `crates/comandos-web/src/components/{extensions,chain_builder,command_sidebar}.rs`, plantillas en `crates/comandos-web-view/src/{extensions,chains,sidebar}.rs`
- Modify: `components.json`, `xtask/web/shots.json` (`extensions`, `chains`, `sidebar`)
- Test: port de `tests/chain_builder_checks.cjs`, `tests/command_sidebar_checks.cjs` (con `tests/fixtures/command-catalog.json`), `tests/e2e_pane_extensions.cjs` (e2e en el Mac)

**Interfaces:**
- Produces: los globales de `extensions.js`, `chain-builder.js` y `command-sidebar.js` según `interop.json`; mismos atributos `aria-*` y `role=switch`; las descripciones de MCP siguen escapadas (`tests/e2e_session_workspace.js` comprueba que `<img onerror>` en una descripción no se ejecuta: el caso se porta como prueba de vista Rust).

- [ ] **Step 1:** Pruebas Rust de los `*.cjs` y `mcp_description_is_text_not_html` (la plantilla de la fila con la descripción hostil del e2e actual no produce `<img`).
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implementación por el procedimiento de B4; `command-sidebar` declara `deps: ["quick-terminal"]` (usa `ComandosQuickTerminal`).
- [ ] **Step 4:** PASS; `shots pair` y `remote-vs-desktop` de las tres suites; e2e de extensiones por panel con fixtures.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-web crates/comandos-web-view xtask/web/shots.json
git commit -m "feat(web): extensiones, constructor de cadenas y barra de comandos en Rust

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B11: Dock de espacios y `workspace.js`

**Depende de:** B4, B5 (`workspace-layout`). **Paralela a** B6–B10.

**Files:**
- Create: `crates/comandos-web/src/components/{workspace_dock,workspace}.rs`, `crates/comandos-web-view/src/workspace.rs`
- Modify: `components.json` (`deps: ["workspace-layout"]`), `xtask/web/shots.json` (`dock`, `workspace`)
- Test: port de `tests/workspace_dock_checks.cjs`, e2e `tests/e2e_session_workspace.js` y `tests/e2e_remote_workspace.cjs` como recorridos `xtask` en el Mac con fixtures

**Interfaces:**
- Produces: globales de `workspace-dock.js` y `workspace.js` del inventario; selector de CLI/motor/modelo/esfuerzo/cuentas con borrador y «Aplicar cambios», tarjetas de «Todas las sesiones» con el observador de 2 s sin acumular consultas (`docs/remote-session-controls.md`).

- [ ] **Step 1:** Pruebas de `workspace_dock_checks.cjs` en Rust; `card_observer_never_overlaps_requests` (con un `Api` falso que tarda 5 s, en 10 s hay como máximo 2 peticiones, no 5).
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implementación por el procedimiento de B4.
- [ ] **Step 4:** PASS; recorridos e2e y `shots pair`/`remote-vs-desktop` de `dock` y `workspace` a 320/390/844/1400 px.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-web crates/comandos-web-view xtask/web/shots.json
git commit -m "feat(web): dock de espacios y controles de sesión en Rust

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B12: Script en línea I — núcleo (i18n, app nativa, red, helpers, render, toasts, registro local, loop, Ctrl+K, iconos, tema, favoritos, UI general, funciones globales)

**Depende de:** B5–B11 activos (las regiones del núcleo las usan casi todos y conviene portarlas cuando sus llamadores ya no son JS, para reducir globales mutables compartidos).

**Files:**
- Create: `crates/comandos-web/src/components/core/{i18n,app_bridge,net,helpers,render,toasts,ui_log,poll_loop,switcher,icons,theme,favorites,ui_general,globals}.rs`, `crates/comandos-web-view/src/{rows,icons,switcher}.rs`
- Modify: `components.json` (una entrada `region` por marcador de las regiones nombradas, con sus `deps`), `xtask/web/shots.json` (`main` con todas las vistas del tablero)
- Test: pruebas de vista por región; e2e `tests/e2e_mobile_remote.js` y `tests/e2e_sidebar_parity.js` como recorridos `xtask`

**Interfaces:**
- Produces: todos los globales que estas regiones definen según `interop.json` (entre ellos `api`, `authToken`, `webtermAccessToken`, `esc`, `mdEsc`, `copyText`, `fmtMoney`, `toast`, `tf`, `render`, `inApp`, `hydrateIcons`); los globales mutables compartidos con código aún JS (lista `mutates_globals` del inventario) como propiedades de `window`.
  - Puente con la app (`app_bridge`): `window.webkit.messageHandlers.centro.postMessage(JSON.stringify(msg))` con los mismos mensajes (`session/win/label`, `headerAction`, `theme`, `buttonStyle`, `leftPanel`, `chainModal`).
  - Deduplicación de filas de `/state` (la misma sesión puede llegar hasta 7 veces: memoria de arquitectura) igual que `render()`.

- [ ] **Step 1:** Por cada región, prueba de vista con `dom_diff` contra la captura del JS (`shots dom-dump --selector <raíz de la región>` sobre fixtures de `/state` con 1, 7 duplicadas y 40 filas), más `rows_are_deduplicated` y `token_from_query_moves_to_storage` (`?token=` se guarda en `localStorage.cc_token` y desaparece de la URL con `history.replaceState`).
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implementación por el procedimiento de B4, región a región en el orden del inventario (red → helpers → i18n → iconos → tema → toasts → registro local → render → loop → Ctrl+K → favoritos → UI general → funciones globales → app nativa); cada región se activa y verifica por separado (se pueden activar de una en una con `comandos web set`).
- [ ] **Step 4:** PASS; `shots pair --suite main` en las cuatro anchuras y dos dpr; `remote-vs-desktop --suite main` (la regla de Jesús completa sobre el tablero); e2e móvil y paridad de barra lateral; validación en el motor de `cc-app` (WebKitGTK 2.50): Jesús abre `cc-app` con `?web=shadow` en el webview (menú «Recargar con sombra», o F5 tras `comandos web set <id> shadow` y la cookie) y confirma; si no se puede, se registra como pendiente en `cutover-web.md` y la región no pasa a `on` hasta esa confirmación.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-web crates/comandos-web-view xtask/web
git commit -m "feat(web): núcleo del tablero en Rust (red, render, temas, iconos, conmutador, puente con la app)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B13: Script en línea II — terminales en el tablero, prefs de terminal, remoto, servidores, notificaciones del sistema, modales, snippets, Analytics y el script final

**Depende de:** B12. A11 si se quiere comparar con la página de terminal nueva (no es requisito: el tablero solo maneja iframes).

**Files:**
- Create: `crates/comandos-web/src/components/inline/{terms,sidebar_glue,term_prefs,typography,remote,servers,sys_notifs,modals,snippets,analytics_glue,tail}.rs`, plantillas correspondientes
- Modify: `components.json` (regiones restantes y `tail` = segundo script en línea), `xtask/web/shots.json` (`terms`, `remote`, `servers`, `snippets`, `modals`)
- Test: pruebas de vista; e2e `tests/e2e_terminal_native.js`, `tests/e2e_remote_pane_controls.cjs`, `tests/e2e_session_audit.cjs` como recorridos `xtask`

**Interfaces:**
- Produces:
  - Terminales: la misma lógica de `TERM_BASE` (remoto `https` no loopback → `${origin}/term`, sondeo `/term/token` 3 intentos × 400 ms con plazo 800 ms, respaldo `https://<host>:8443` si `/remote-state.fallbackTerminalOn`, aviso «Terminal en modo degradado» una vez; loopback dentro de `cc-app` → `http://127.0.0.1:4779/?arg=<token>&arg=<sesión>`), los mismos `postMessage` con los iframes (`theme`, `button-style`), `openTerms`/`activeTerm` como propiedades de `window` mientras lo pida el inventario.
  - Remoto: `/remote-state`, `/remote-*`, QR, `?token=`; la barra y entrada remotas son las únicas piezas que existen solo en remoto (regla de Jesús).
  - Snippets con `<dialog>` nativo (`margin:auto`, nunca `class="modal"`: memoria de arquitectura).

- [ ] **Step 1:** Pruebas de vista por región; `term_base_rules` (tabla de orígenes → base esperada, incluyendo `__COMANDOS_DEV_WEBTERM` solo en loopback); `snippets_dialog_is_not_modal_class`.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implementación por el procedimiento de B4, región a región.
- [ ] **Step 4:** PASS; `shots pair` y `remote-vs-desktop` de las cinco suites; e2e remotos; validación en `cc-app` como en B12.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-web crates/comandos-web-view xtask/web
git commit -m "feat(web): terminales del tablero, remoto, servidores, modales, snippets y script final en Rust

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B14: Carcasa generada en el servidor y `GET /events/stream` (SSE)

**Depende de:** B12, B13 (todas las regiones y scripts activos).

**Files:**
- Create: `crates/comandos-web-view/src/shell.rs`, `crates/comandos-server/src/dash/web/sse.rs`, `crates/comandos-web/assets/index-inline.css` (extraído por `xtask web-port extract-css`, idéntico al `<style>` de `index.html`)
- Modify: `crates/comandos-server/src/dash/web/compose.rs` (componente `page:index` que sustituye la página entera), los componentes con sondeo (`pomodoro`, `work-marks`, `notifications`, `core/poll_loop`, `analytics`) para suscribirse a SSE, `components.json`
- Test: `crates/comandos-web-view/tests/shell.rs`, `crates/comandos-server/tests/web_sse.rs`

**Interfaces:**
- Produces:
  - `shell::index(ctx: &ShellCtx) -> Markup`: el documento completo con el mismo `<meta>`, `<title>`, `<link>`, el `<style>` (de `index-inline.css`, intacto), el cuerpo HTML y, en lugar de los `<script>`, solo `boot.js` y la compuerta. `ShellCtx { lang, theme, nonce, assets }`.
  - `GET /events/stream` (`text/event-stream`, `ReplyBody::Stream` con canal acotado de 64 mensajes; latido `: ping` cada 25 s; máximo 32 flujos, el 33.º → 503): `event: state|work-marks|notices|pomodoro|usage\ndata: {"rev":N}\n\n`. Las revisiones salen de los productores que ya existen en el frente (generación de `/state` de la 2d, de uso de la 2e, `notices`, marcas, Pomodoro); si un dominio sigue en el Python, su revisión es un sondeo interno del frente a 2 s de la ruta reenviada con hash del cuerpo (una sola consulta para todos los clientes, en lugar de una por cliente).
  - Clientes: al abrir el flujo, cada componente baja su sondeo a 30 s (respaldo) y vuelve a pedir su ruta al recibir su evento (D8); si el flujo se cae, recupera su intervalo de siempre.

- [ ] **Step 1: Pruebas que fallan.** `shell.rs`: `dom_diff` de `shell::index` frente a `index.html` del disco sin `<script>` → igual; el `<style>` generado es byte a byte `index-inline.css`, y `xtask web-port check` vigila que siga igual al `<style>` del disco. `web_sse.rs`: un cliente recibe `event: notices` tras un `POST /notices` de prueba; el 33.º flujo → 503; un cliente que no lee no hace crecer la memoria más de 64 mensajes.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implementación.
- [ ] **Step 4:** PASS; `shots pair --suite main` completo (ahora `web=off` frente a la carcasa generada); `web-bench`: peticiones por minuto del tablero en reposo antes/después (objetivo: ≥ 80 % menos), CPU del frente en reposo con 3 clientes.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-web-view/src/shell.rs crates/comandos-web-view/tests/shell.rs crates/comandos-server/src/dash/web crates/comandos-server/tests/web_sse.rs crates/comandos-web
git commit -m "feat(web): carcasa del tablero generada en el servidor y SSE como invalidación de los sondeos

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task B15: Recursos embebidos, retirada de lo servido en JS y verificación final de la Fase 3

**Depende de:** B14, A11.

**Files:**
- Create: `crates/comandos-server/src/dash/web/embed.rs`, `crates/comandos-server/build.rs` (incluye `<target>/web/manifest.json` y los artefactos si existen; si no, compila sin embebidos y sigue sirviendo del disco. `<target>/web` se resuelve con `comandos_core::web_assets::out_dir_from` (`CARGO_TARGET_DIR` o `<workspace>/target`, como `xtask::web_build::out_dir()`), con `rerun-if-changed`/`rerun-if-env-changed=CARGO_TARGET_DIR`, y el manifiesto se valida con `Manifest::check_paths`)
- Modify: `crates/comandos-server/src/dash/statics.rs` (orden: embebido → disco), `docs/verification/cutover-web.md` (sección final), `xtask/src/web_bench.rs`
- Test: `crates/comandos-server/tests/web_embed.rs`

**Interfaces:**
- Produces:
  - `embed::get(path: &str) -> Option<(&'static [u8], &'static str)>` para: artefactos WASM y cargadores, `workspace.css`, `buttons.css`, `analytics.css`, `extensions.css`, fuentes (`assets/fonts/*` en `woff2`; las TTF de JetBrains Mono se subconjuntan a `woff2` en `xtask web-build` solo para la cara de ligaduras), iconos, `manifest.webmanifest`, sonidos usados.
  - `COMANDOS_WEB_EMBED=0` vuelve a servir del disco (reversión sin release).
  - Lo que deja de servirse con todos los componentes activos: `dash/*.js`, el script en línea, `vendor/markdown-it`, `vendor/purify`, `assets/xterm/*` y `assets/opentype/*` **en `/` y `/term/`**; los archivos siguen en disco y `?web=off` los sigue sirviendo (reversión y oráculo hasta la Fase 6). `cc-app` (pestaña experimental) y `cc-app-mac` siguen cargando `assets/xterm` por `file://` o su propia ruta hasta las Fases 4 y 5.

- [ ] **Step 1: Prueba que falla**: con un `manifest.json` de prueba, `GET /web/<hash>/boot.js` y `/workspace.css` responden desde `embed` aunque se borre el archivo del `dash_dir` de prueba; con `COMANDOS_WEB_EMBED=0`, desde disco.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implementación (`include_bytes!` generado por `build.rs` en `OUT_DIR/embedded.rs`; nada de `unsafe`).
- [ ] **Step 4: Verificación final** (controlador, en sombra 4782 y después en producción):
  - `shots pair --suite all` y `remote-vs-desktop --suite all` en 320/390/844/1400 px y dpr 1/2: 0 recortes fuera de tolerancia, salvo las diferencias aceptadas listadas (D10).
  - `web-bench`: tamaños de artefactos, primer pintado, memoria del WASM tras 10 min, peticiones por minuto en reposo, CPU del hilo principal; frente: Pss con 3 clientes y 8 terminales abiertas. Todo contra la referencia JS de `docs/verification/fase3/` y los presupuestos del índice.
  - `term-bench` con la release final.
  - PWA instalada en el teléfono: abrir, terminal, aviso push, sin red.
  - `cc-app` con la release final: Jesús abre el tablero (F5) y una terminal de la barra lateral; nada se reinicia por esto.
- [ ] **Step 5: Cutover final** en `cutover-web.md`:

````markdown
## Release final de la Fase 3
```sh
"$NEW" install --stage --web "<CARGO_TARGET_DIR del build>/web"   # origen explícito (T4, I2); imprime «release <id> (web: …)»
systemctl --user restart cc-dash.service
~/.local/share/comandos/bin/comandos web status          # todos "on", ninguno "drift"
```
Revertir: `~/.local/share/comandos/bin/comandos install --rollback-release && systemctl --user restart cc-dash.service`
(la selección `H/comandos-web.json` se conserva; `comandos web set <id> off` revierte componentes sueltos sin reinicio).
````

- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/build.rs crates/comandos-server/src/dash/web/embed.rs crates/comandos-server/src/dash/statics.rs crates/comandos-server/tests/web_embed.rs xtask/src/web_bench.rs docs/verification/cutover-web.md docs/verification/fase3
git commit -m "feat(web): recursos embebidos en el binario y cierre de la Fase 3 con paridad y presupuestos medidos

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

## Riesgos propios de 3b

- **Objetivo en movimiento**: otras sesiones editan `dash/` a diario. El hash por componente (D2) impide servir un port obsoleto; el coste es re-sincronizar (`web-port check` en CI de la rama y antes de cada `set on`). Si un componente deriva más de dos veces en una semana, se coordina con Jesús una ventana sin ediciones de ese archivo.
- **Globales compartidos** entre WASM y JS durante la transición (`mutates_globals`): se mantienen como propiedades de `window`, nunca como estado Rust privado, hasta que su último usuario JS se porta; el inventario los lista.
- **WebKitGTK de `cc-app`** (JSC más viejo, ya rompió el rig de PerezOS): cada región del núcleo exige confirmación en `cc-app` antes de `on`; la compuerta da reversión automática por cliente.
- **Markdown**: `pulldown-cmark` no es markdown-it; el corpus real decide y las diferencias se documentan.
- **Tamaño del WASM del tablero**: presupuesto 600 KiB gzip vigilado por `web-build --check-budget` en cada tarea; si se rebasa, se separan componentes pesados (noticias, analítica) en un segundo módulo cargado bajo demanda.
