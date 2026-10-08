# Fase 3 — `comandos-term` y `comandos-web`: plan de implementación (índice y piezas comunes)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** retirar ttyd, xterm.js (con sus addons y opentype.js), markdown-it, DOMPurify y los 20 archivos JS propios de lo que sirve el tablero, sustituyéndolos por una terminal en Rust (`comandos-term`, canvas) y una interfaz en Rust (`comandos-web`, WASM) que se ven **píxel a píxel igual** que hoy en escritorio y en remoto, más la PWA móvil; cada pieza entra en sombra y se activa y revierte por componente sin reiniciar nada.

**Architecture:** la interfaz es WASM escrito con `web-sys` y plantillas `maud`, sin framework; el servidor (`comandos dash`) compone la página: lee `dash/index.html` del disco, quita los `<script>` o regiones de los componentes ya portados (solo si el archivo en disco sigue siendo el que se portó) e inyecta un cargador generado; los componentes que siguen en JS conviven con los de WASM a través de un puente de globales. La terminal se migra en dos pasos: primero el puente PTY↔WebSocket en Rust hablando el protocolo de ttyd (xterm.js sigue), después el motor y el renderizador `comandos-term` con su propio protocolo. La verificación visual corre en el Chrome del Mac (`chrome-bg`, por `cc-browser-remote`) y compara recortes por componente con tolerancia.

**Tech Stack:** Rust 1.96 (edition 2024), `wasm32-unknown-unknown`, `wasm-bindgen = "=0.2.129"` (CLI 0.2.129 ya instalado en `~/.cargo/bin`), `web-sys = "=0.3.106"`, `js-sys = "=0.3.106"`, `wasm-bindgen-futures = "=0.4.79"`, `maud = "=0.27.0"`, `alacritty_terminal = "=0.26.0"` vendorizado con parche wasm, `tokio-tungstenite = "=0.30.0"`, `pty-process = "=0.5.3"`, hyper 1.11 (upgrade), `pulldown-cmark = "=0.13.4"`, `ammonia = "=4.2.1"`, `png = "=0.18.1"`, `scraper = "=0.27.0"`, `base64 = "=0.22.1"`; `wasm-opt` 0.116.1 (`cargo install wasm-opt --version 0.116.1 --locked`).

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§3.1 filas `dash/*`, ttyd, xterm.js, markdown-it, uisfx, `sw.js`; §4.4; §4.5 «Versión remota = versión de escritorio»; §5; §6; §7 fila 3; §8; Enmiendas 3, 5 y 6). Spike del motor: `docs/research/2026-10-04-spike-terminal-rust.md`. Inventario de la Fase 2: `docs/research/2026-10-04-fase-2-inventario.md` §5 (terminal web) y §6 (unidades).

**Planes hijos (este archivo es el índice y contiene las tareas comunes T1–T4):**
- `docs/superpowers/plans/2026-10-04-fase-3a-comandos-term.md` — tareas A1–A12 (A12, banco y paridad, se ejecuta antes que A11, el paso 2).
- `docs/superpowers/plans/2026-10-04-fase-3b-comandos-web.md` — tareas B1–B15.

**Precondición:** las Fases 2a–2e están fusionadas en `main` y el frente Rust sirve el 4777 con el Python heredado en 4781. Las sub-fases de la 2 que queden (2f/2g) pueden correr en paralelo con la 3: la interfaz llama a las mismas rutas sean nativas o reenviadas. Si al empezar un nombre de los bloques **Interfaces** de la 2 no coincide (`DashConfig`, `Config`, `Limits`, `RouteClass`, `native::route`, `statics::serve`, `install --stage`), se usa el real y se anota en el commit.

---

## Rulings del controlador que fijan este plan

1. **Nunca se rompe una sesión viva.** Ninguna prueba, sombra ni arnés se engancha al servidor tmux del usuario: las pruebas usan servidores privados con `-S`/`-L` propio (Global Constraints, regla tmux); la sombra sirve la terminal desde grabaciones (`replay`). Solo producción hace `tmux attach` a sesiones reales, como hoy ttyd, y con la lista cerrada de comandos de A3.
2. **Todo cutover entra primero en sombra y es reversible** con un solo comando; por componente, sin reiniciar la app ni tmux, y la interfaz sin reiniciar siquiera el frente (Decisión D2).
3. **Automatización de navegador solo en el Mac** (`chrome-bg` por `~/.local/bin/cc-browser-remote`, puertos expuestos con `cc-browser-expose`). Nunca un Chrome, Playwright o Xvfb local como sustituto. La única excepción posible, el sondeo del motor WebKitGTK de `cc-app` en T1, requiere el visto bueno explícito de Jesús y no es automatización (la página se autoevalúa).
4. Comentarios en español, identificadores en inglés; sin `unsafe`, sin `unwrap`/`expect`/indexado en código que no sea de prueba (también en WASM: los `Result<_, JsValue>` se propagan); `cargo clippy -D warnings` en host **y** en `wasm32-unknown-unknown`; `rustfmt`; versiones exactas (`=x.y.z`); trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
5. **Cero Python o bash nuevos. Cero JS escrito a mano nuevo** fuera de: (a) la cola generada por `wasm-bindgen`; (b) el cargador `boot.js` y el shim `sw.js`, que `xtask web-build` genera desde constantes Rust; (c) `gate.js`, que el frente genera por petición; (d) expresiones de una línea que el arnés pasa a `evaluate_script` (cadenas en código Rust de `xtask`, nunca archivos `.js`).
6. Los tests nunca tocan el HOME real, `~/.claude`, `~/.local/state`, systemd, el tmux del usuario ni los puertos 4777–4782; los puertos de prueba salen de la banda de devhost 7300–7399 (`devhost add` o enlace efímero dentro de esa banda).
7. Los cutovers los ejecuta el controlador; el Python heredado no se modifica.

## Global Constraints

- Compilar y probar con `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target nice -n 10 cargo <cmd> -j 6` (abreviado `$C <cmd>`). Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings`, `$C clippy -p comandos-web -p comandos-term-web -p comandos-web-sw --target wasm32-unknown-unknown -j 6 -- -D warnings` (desde que existan) y las pruebas de los paquetes tocados.
- Perfil WASM: `[profile.release-wasm]` hereda `release` con `opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = "symbols"`; después `wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int`.
- Commits con `git add <rutas>` explícitas; prefijos `feat(term): …`, `feat(web): …`, `feat(dash): …`, `feat(xtask): …`, `docs(verification): …`, en español. Los planes: `git add -f docs/superpowers/plans/2026-10-04-fase-3*.md`.
- `unsafe_code = "forbid"` en todo el workspace, también en `comandos-term`: `pty-process` encapsula el PTY y la spec (§4.1) solo *permitía* `unsafe` allí; no hace falta.
- Toda pantalla nueva conserva literalmente los textos, clases, ids, atributos `aria-*`, orden de nodos y CSS de la que sustituye. **El CSS no cambia** (spec §3.1).
- Sonidos suaves y un control por concepto (memoria `feedback-notificaciones-y-ui`): el port no añade ni quita controles; texto completo siempre accesible.
- **Remoto = escritorio** (spec §4.5, memoria `feedback-remoto-igual-escritorio`): los mismos componentes; solo difieren la barra de botones y el campo de entrada de la terminal remota y la disposición *flex*. T2 lo convierte en prueba.
- **tmux (regla de `CLAUDE.md` del repo, vinculante; el 2026-10-04 a las 21:59 un subagente mató las ~20 sesiones vivas de Jesús):**
  - todo tmux de prueba va con socket explícito: `Tmux::private(dir)` (`crates/comandos-server/src/dash/native/tmux.rs`, pasa `-S <dir>/tmux-1000/default`), `private_tmux(dir)` en `xtask`, o `-L comandos-test-<pid>`; **`TMUX_TMPDIR` solo no vale** (tmux 3.2a lo ignora si el directorio no existe y cae en `/tmp/tmux-1000/default`);
  - al terminar una prueba: primero `kill-server` **con ese mismo `-S`/`-L`**, después borrar el directorio (guardia `PrivateTmux` con `Drop`, A3);
  - prohibido en código, pruebas, arnés y pasos manuales: `kill-server`, `kill-session`, `pkill tmux`, `kill -9 -1` sin `-S`/`-L` propio; prohibido teclear tmux a mano durante la implementación (todo pasa por el código y sus pruebas);
  - el attach de producción de `comandos-term` solo ejecuta `tmux attach [-f active-pane] -t =<sesión>` (y `has-session -t =<sesión>` para validar) en el socket del usuario: **ningún comando de alcance de servidor**, nunca `kill-*`, `set -g`, `source-file` ni `new-session`. A3 lo convierte en prueba (`AttachCommand` es un enum cerrado; la prueba `live_commands_are_attach_or_has_session_only` recorre todas sus variantes).
- `macmini` es la máquina remota del Chrome de pruebas (Ubuntu 26.04 x86_64, Chrome for Testing 153 detrás del broker `comandos-browser`; «el Mac» en este plan = esa máquina). Sus capturas sirven igual para la paridad: se comparan siempre dos variantes tomadas en el mismo navegador, nunca contra una captura de otro equipo. Sus fuentes del sistema son casi nulas: las fuentes de ComandOS se cargan por `@font-face` en ambas variantes (spike, «Caveat about the remote browser»).
- Presupuestos medidos (Decisión D7; los fija `xtask term-bench`/`web-bench`, A12 y B15, y un cutover no se ejecuta si alguno falla):
  - Artefactos: `comandos_web_bg.wasm` ≤ 600 KiB gzip; `comandos_term_web_bg.wasm` ≤ 250 KiB gzip; `comandos_web_sw_bg.wasm` ≤ 64 KiB gzip.
  - Terminal, servidor: puente sin conexiones ≤ +0 (vive dentro del frente); **+≤ 256 KiB de Pss del frente por terminal abierta** en régimen y ≤ 2 MiB por terminal durante una ráfaga (cola acotada a 1 MiB, A3); ningún hilo extra por terminal (`pty-process` con su `AsyncRead` de tokio sobre el descriptor del PTY, en el mismo runtime del frente). El cliente `tmux attach` (~4,5 MB, spike) es el mismo que con ttyd: no cuenta como ahorro ni como coste.
  - Terminal, navegador: memoria por iframe (`performance.measureUserAgentSpecificMemory()` si existe; si no, `usedJSHeapSize` + `WebAssembly.Memory.buffer.byteLength`) ≤ 0,6 × la de xterm.js con 10 000 líneas de historia llenas; CPU en reposo: 0 cuadros pintados por segundo sin salida (el parpadeo del cursor repinta solo la celda del cursor cada 600 ms, como xterm.js).
  - Latencia: eco de tecla mediana ≤ 8 ms y ≤ referencia xterm.js + 1 ms (mismo túnel, mismo navegador); cuadro peor ≤ 60 ms con `seq 1 500000`.
  - Interfaz: memoria lineal del WASM del tablero ≤ 16 MiB tras 10 min de fixtures en bucle y sin crecimiento entre el minuto 5 y el 10; primer pintado ≤ referencia JS + 50 ms; tiempo de CPU del hilo principal por minuto en reposo ≤ referencia JS.

## Decisión de arquitectura: qué significa «la UI en Rust»

La spec (§3.1) pide «plantillas Rust (`maud`) que generan el mismo DOM y clases; toda la lógica de interfaz en Rust con `web-sys`», y su estado final (Fase 6) es un repositorio sin JS propio. Antes de fijarlo se compararon cuatro caminos con criterios que T1 mide y deja registrados en `docs/research/2026-10-04-fase-3-ui-rust.md`.

| Criterio (cómo se mide en T1) | `web-sys` + `maud`, sin framework | Leptos 0.8.21 (CSR) | Dioxus 0.7.10 (web) | Yew 0.23.0 (CSR) | HTML en servidor + JS de terceros (htmx) |
|---|---|---|---|---|---|
| Tamaño: gzip tras `wasm-opt -Oz` de la porción portada de `work-marks.js` y de una app vacía | el menor esperado: sin runtime reactivo | runtime de señales | runtime + VDOM | VDOM | 0 WASM, pero htmx ≈ 14 KiB y el resto sigue en JS |
| Paridad de DOM (`xtask dom-diff` contra el legado, normalizado) | exacta: el texto HTML de `maud` reproduce la plantilla JS | inserta nodos marcadores y reordena atributos al hidratar | inserta marcadores de plantilla | VDOM propio, atributos por propiedad | exacta en lo que renderiza el servidor |
| Convivencia con JS heredado que muta los mismos nodos durante meses | sin dueño del DOM: no hay conflicto | el runtime asume la propiedad de sus nodos | idem | idem | — |
| Esfuerzo (líneas Rust / líneas JS de la porción) | ~1:1, port casi mecánico del código imperativo actual | reescritura a componentes reactivos | idem | idem | todo lo interactivo (terminal, arrastre, audio, PWA sin red) se queda en JS |
| Mantenimiento (estable a 2026-10-04, rupturas en 12 meses) | `web-sys` estable; `maud` sin runtime | 0.9.0-beta2 en curso | 0.8.0-alpha.1 en curso | 0.23 | — |
| Funciona en WebKitGTK 2.50 (`cc-app`, sondeo opcional de T1) | WASM MVP + bulk-memory | idem | idem | idem | sí |

**Decisión:** `web-sys` + `maud` sin framework. Es lo que dice la spec y lo que exige el estrangulamiento por componentes: durante meses la página mezcla componentes WASM y JS heredado que tocan los mismos nodos (la fila de sesión la decoran `work-marks.js`, `workspace-dock.js` y el script en línea), y un framework reactivo es dueño de sus nodos; además sus marcadores rompen la comparación de DOM que usamos como oráculo. El HTML en servidor con htmx no cumple el estado final: la terminal, el arrastre de pestañas, Web Audio, el Pomodoro sin red y el service worker necesitan lógica en el cliente, y cada interacción pasaría a ser un viaje por el tailnet. **Regla de reversión de la decisión:** solo se cambia si en T1 un framework pasa la paridad de DOM y la convivencia **y** queda ≥ 30 % por debajo en gzip **y** ≤ 0,8× en líneas; si no, queda registrada con sus números.

Concretamente, «UI en Rust» queda así:

1. **Plantillas puras** (`comandos-web-view`): funciones `maud` que devuelven `Markup`/`String` con el mismo DOM que la plantilla JS que sustituyen; compilan en host y en WASM y se prueban en host. El servidor las usa para la carcasa de la página (B14) y el cliente para los fragmentos dinámicos.
2. **Lógica de interfaz** (`comandos-web`, `comandos-term-web`): `web-sys` para DOM, eventos, `fetch`, temporizadores, `localStorage`, Web Audio, canvas y WebSocket; delgada sobre las plantillas y sobre `comandos-web-dom` (utilidades compartidas).
3. **JS que queda** al cerrar la Fase 3: solo generado (cola de `wasm-bindgen`, `boot.js`, `sw.js`, `gate.js`). Los archivos `dash/*.js` y el script en línea siguen **en disco** hasta la Fase 6 (otras sesiones los editan y son el oráculo), pero dejan de servirse cuando su componente está activo. `assets/xterm/` sigue en disco porque la pestaña experimental de `cc-app` (`file://…/dash/term.html`) y `cc-app-mac` lo cargan hasta las Fases 4 y 5.
4. **Markdown** en el servidor (`pulldown-cmark` + `ammonia`, spec §3.1) por una ruta nativa nueva; la forma del texto la decide el navegador (HarfBuzz de Chrome/WebKit), igual que hoy con xterm.js. `rustybuzz` + `swash` (spec §4.4) se quedan para el renderizador cairo de la Fase 4: en web añadirían ≈ 400 KiB y otra forma de las ligaduras distinta de la del navegador.

## Decisiones del plan

- **D1 — Tres artefactos WASM.** `comandos_web` (tablero), `comandos_term_web` (página de terminal: cada terminal es un iframe y no debe cargar el tablero entero) y `comandos_web_sw` (service worker). Las utilidades comunes viven en `comandos-web-dom` y las plantillas en `comandos-web-view`; el navegador cachea el módulo compilado entre iframes.
- **D2 — Estrangulamiento por composición, recargable sin reinicio.** El frente compone `/` y `/term/` en cada petición (B2): registro de componentes en el repo (`crates/comandos-web/components.json`: id, tipo, origen, `sha256` del origen portado, función de montaje, exportaciones, dependencias) y selección en `H/comandos-web.json` (`{"on": [...], "shadow": [...]}`, `H = ~/.claude/hooks`), releída por `mtime`. Un componente se activa solo si (a) está en `on` (o en `shadow` y la petición trae la cookie `cc_web=shadow`), (b) el `sha256` del origen en disco coincide con el registrado y (c) sus dependencias están activas. Si otra sesión edita el JS después del port, el frente vuelve a servir el JS heredado de ese componente y lo publica en `GET /web/status`: **un objetivo en movimiento nunca rompe la página**, solo detiene el cutover hasta re-sincronizar (`xtask web-port check`). `?web=off` sirve la página heredada pura; `?web=shadow` pone la cookie de sesión `cc_web=shadow` (la heredan los iframes de terminal).
- **D3 — Compuerta de arranque (`gate.js`).** El JS heredado llama globales de otros archivos al cargar y espera `DOMContentLoaded`; el WASM se instancia de forma asíncrona (Chrome prohíbe compilar en síncrono > 4 KiB en el hilo principal). La página compuesta lleva al principio de `<head>` `<script type="module" async src="/web/<hash>/boot.js" data-k="<nonce>">` y justo después `<script src="/web/gate.js?k=<nonce>"></script>` (clásico, bloquea el análisis). `boot.js` instancia el WASM, monta los componentes, instala sus exportaciones en `window` y hace `POST /web/ready {"k":…}`; el frente suelta la respuesta de `gate.js` (cuerpo vacío) y el análisis sigue con el WASM ya listo, de modo que el orden, los globales y `DOMContentLoaded` del JS heredado no cambian. Si a los 8 s no llegó `ready`, `gate.js` responde `if(!sessionStorage.cc_web_fallback){sessionStorage.cc_web_fallback=1;location.replace(<misma URL con web=off>)}`: **reversión automática por cliente** (p. ej. un WebKitGTK sin WASM) y una línea en `/ui-log`. Nonces acotados: 256 en vuelo, caducan a los 10 s.
- **D4 — Terminal en dos pasos.** Paso 1 (A4): el puente Rust habla el protocolo de ttyd en `/term/ws` del 4777 y en un oyente de compatibilidad en 4780; `dash/term.html` + xterm.js no cambian; se retira el ttyd de 4780. `tailscale serve` no cambia en ningún paso: el oyente de compatibilidad ocupa los mismos puertos que ttyd. Paso 2 (A11): `/term/` sirve la página `comandos-term`, el oyente de compatibilidad toma también 4779 (perfil `plain` que reproduce la UI embebida de ttyd para la barra lateral de `cc-app`) y se retira el último ttyd. El códec ttyd se conserva para la pestaña experimental de `cc-app` y `cc-app-mac` (`ws://127.0.0.1:4779/ws`) hasta las Fases 4 y 5.
- **D5 — Protocolo de terminal.** Un solo códec con dos dialectos (A2): `tty` (ttyd 1.6.3, byte a byte: primer mensaje JSON `{"AuthToken","columns","rows"}`, prefijos `'0'`–`'3'`, sesión y token como `arg` repetidos en la URL) y `comandos.term.v1` (mismo enmarcado; primer mensaje `{"v":1,"cols":C,"rows":R,"session":"…"}` y la autenticación es la puerta del tablero —Host, Origin, loopback o token por cookie `cc_token`—). Ambos mandan al abrir el título (`'1'`) y las preferencias (`'2'`) que hoy manda ttyd por sus `-t` (`fontSize: 11`, la familia de `bin/cc-webterm`): xterm.js las aplica y la página nueva también, así la letra efectiva no cambia.
- **D6 — Sombra segura.** (a) **Componentes con datos fijos**: el frente real con `--no-native`, `COMANDOS_DASH_TEST_HOOKS=1` y el heredado sustituido por el servidor de fixtures de `xtask` (rutas → JSON fijos, registra los POST) en un puerto de la banda 7300–7399. (b) **Sombra contra la UI viva**: frente en 4782 con `--shadow-readonly` (todo lo que no es GET responde `{"ok":true,"shadow":true}` sin reenviar, `--no-usage-effects`, `/term/ws` reproduce grabaciones) leyendo el estado real y reenviando GET al heredado vivo. Nunca se apunta el navegador del Mac al 4777: su `POST /presence` registraría un dispositivo sin audio y silenciaría los avisos reales (precedente: `cutover-dash.md` §4).
- **D7 — Medir, no afirmar.** Referencias de xterm.js y del JS heredado tomadas en las mismas condiciones (mismo navegador del Mac, mismas fixtures) y guardadas en `docs/verification/fase3/`; cada cutover compara contra ellas.
- **D8 — SSE como invalidación** (Enmienda 5). `GET /events/stream` (B14) emite `event: <dominio>\ndata: {"rev":N}`; los componentes WASM vuelven a pedir su ruta JSON de siempre (la paridad de las respuestas no cambia) y conservan el sondeo como respaldo a ritmo lento mientras el flujo está abierto.
- **D9 — Estáticos desde disco hasta el final** (Enmienda 3). Los artefactos WASM se sirven desde `COMANDOS_WEB_DIR` (por omisión `<release>/web/`) en rutas con hash y `Cache-Control: public, max-age=31536000, immutable`; B15 los embebe con `include_bytes!` junto con CSS, fuentes e iconos.
- **D10 — Diferencias aceptadas**, listadas en `docs/verification/cutover-web.md` y `cutover-term.md` con su razón; ninguna se acepta en silencio. Ya previstas: el HTML de markdown de `pulldown-cmark` frente al de markdown-it en entradas exóticas (B8); la identidad DA2 (A1) se mantiene igual que xterm.js precisamente para no tener que aceptar una.

## Review Focus

1. **Otra sesión edita `dash/pomodoro.js` (o una región del script en línea) después de portarlo y con el componente activo.** El frente vuelve a servir el JS heredado de ese componente en la siguiente carga, `/web/status` lo marca `drift` y la página funciona igual. Prueba `compose_drift_keeps_legacy` (B2).
2. **El WASM no arranca en un cliente** (WebKitGTK de `cc-app`, iOS viejo, red cortada a mitad): a los 8 s la compuerta recarga con `web=off` una sola vez y queda una línea en `/ui-log`; nunca un tablero en blanco ni un bucle de recargas. Prueba `gate_timeout_falls_back_once` (B2) y e2e `boot_failure_falls_back` (B4).
3. **`cc-app` llama globales por `run_javascript`** (`nsOpen()`, `pomoRender()`, `notifRender()`, `document.getElementById(id).click()`, el código de `bin/cc-app:4980` y `:7521`) y el tablero manda mensajes a los iframes de terminal (`postMessage` con `theme`, `session`). Cada componente que posea uno de esos nombres los reexporta en `window` con la misma firma. Inventario B3 y prueba `host_calls_still_resolve` (B4, e2e).
4. **Teclado del teléfono** (GBoard sin `keydown`, composición IME, dictado): Backspace llega como `0x7f`, Enter como `\r`, el texto compuesto entra una sola vez y el borrador se conserva al reconectar. Pruebas `gboard_beforeinput_bytes` e `ime_commit_once` (A7) y e2e de A10.
5. **Respuesta DA2 de la terminal nueva.** `config/terminal-replies.conf` traga las de VTE 0.68 y xterm.js (`ESC [ > 0;276;0 c`); si `comandos-term` respondiera la de alacritty, tmux 3.2a dejaría pasar `…;1c` a la aplicación al iniciar. El motor responde exactamente lo de xterm.js. Prueba `da1_and_da2_replies_match_xterm_js` (A1).

## Estructura de archivos

```
Cargo.toml                                   (miembros nuevos, [patch.crates-io], profile release-wasm)   T4, A1, B1
vendor/alacritty_terminal/                   (0.26.0 + parche wasm de dos líneas)                         A1
crates/comandos-server/src/lib.rs            (Config.websocket, Limits.websockets, upgrade)               T3
crates/comandos-server/src/ws.rs             (handshake, WsStream, rechazo)                               T3
crates/comandos-server/src/dash/term/        (mod, bridge, routes, compat, replay)                        A3, A4, A11
crates/comandos-server/src/dash/web/         (registry, compose, gate, assets, status, markdown, sse)     B2, B8, B14, B15
crates/comandos-term/                        (engine, proto, input, render, select, links, glyphs)        A1, A2, A5, A7
crates/comandos-term-web/                    (cdylib: canvas, teclado, página de terminal)                A6, A7, A9, A10
crates/comandos-web-dom/                     (dom, events, api, storage, log, i18n, bridge)               B1
crates/comandos-web-view/                    (plantillas maud puras, modelos de vista)                    B1, B4–B14
crates/comandos-web/                         (cdylib: tablero; components.json, interop.json)             B1–B15
crates/comandos-web-sw/                      (cdylib: service worker)                                     B9
crates/comandos-cli/src/{dispatch,web,webterm}.rs  (`comandos web`, `comandos webterm`)                    A4, B2
crates/comandos-cli/src/install/             (copia web/ a la release)                                    T4
xtask/src/{mcp,shots,png_diff,dom_diff,fixtures,web_build,web_inventory,web_port,term_bench}.rs     T2, T4, B3, B4, A12
xtask/web/{shots.json,fixtures/,replays/}                                                                 T2, A12
docs/research/2026-10-04-fase-3-ui-rust.md, 2026-10-04-fase-3-web-inventario.md                           T1, B3
docs/verification/{cutover-term.md,cutover-web.md,fase3/}                                                 A4, A11, B4, B15
```

## Grupos de tareas y paralelismo

Cada fila puede ir en su propio worktree (`superpowers:using-git-worktrees`) en cuanto sus dependencias estén en `main`.

| Grupo | Tareas | Depende de | Independientes entre sí |
|---|---|---|---|
| 0 — cimientos | T1 medición/decisión, T2 arnés visual, T3 WebSocket en el transporte, T4 compilación WASM | — | **sí, las cuatro** |
| 1 — motor | A1 motor, A2 protocolo | — | **sí** (y con todo el grupo 0) |
| 2 — puente | A3 puente PTY | A2 | — |
| 3 — paso 1 terminal | A4 rutas `/term` con ttyd, compat 4780, `comandos webterm` | T3, A3 | — |
| 4 — render | A5 render puro | A1 | con A3/A4 |
| 5 — canvas y entrada | A6 canvas, A7 entrada | A5, T4 | A6 ∥ A7 |
| 6 — página terminal | A8 selección/enlaces, A9 carcasa, A10 controles remotos | A6, A7, B1, B3 (A10 además B5) | A8 ∥ A9; A10 tras A9 |
| 7 — medición y paso 2 | A12 banco y paridad, luego A11 paso 2 | A8–A10, T2 (A11 además B2) | — |
| 8 — base web | B1 crates, B2 compositor, B3 inventario | T1, T4 (B3: nada) | B3 ∥ B1; B2 tras B1 |
| 9 — primer port | B4 procedimiento + `ui-sounds` | B1–B3, T2 | — |
| 10 — ports | B5 hojas, B6 marcas+Pomodoro, B7 analítica, B8 markdown+noticias+avisos, B9 PWA, B10 extensiones+cadenas+barra, B11 dock+workspace | B4 | **sí, las siete** (y con el grupo 6) |
| 11 — script en línea | B12 núcleo, B13 terminales y modales | B5–B11 | B13 tras B12 |
| 12 — cierre | B14 carcasa + SSE, B15 embebido y retirada | B12, B13, A11 | — |

---

## Tareas comunes

### Task T1: Medición de frameworks y registro de la decisión

**Files:**
- Create (no se versiona; `.spike/` está ignorado por `.spike/.gitignore` = `*`): `.spike/ui/{websys,leptos,dioxus,yew}/` (un crate `cdylib` cada uno), `.spike/ui/fixture/work-marks.json`, `.spike/ui/index-<variante>.html`
- Create: `docs/research/2026-10-04-fase-3-ui-rust.md`

**Interfaces:**
- Consumes: `dash/work-marks.js` (`menuItems`, `openMenu`, `paint`, `indicator`) del commit de `main` al empezar; T2 si ya existe (`xtask dom-diff`); si no, la comparación de DOM se hace con `evaluate_script` + `outerHTML` y se normaliza a mano en el informe.
- Produces: el informe con la tabla de criterios medidos y la decisión; B1 lo cita.

- [ ] **Step 1: Herramientas**

```bash
cargo install wasm-opt --version 0.116.1 --locked
wasm-bindgen --version    # wasm-bindgen 0.2.129
```

- [ ] **Step 2: Porción común.** Las cuatro variantes implementan lo mismo: leen `fixture/work-marks.json` (lista de filas con `scope`, `key`, `mark`, `favorite`), pintan el botón de cada fila con el HTML de `paint` (`aiIconSvg(c.ai) + (fav ? iconSvg('favorite', 12) : '')`) y abren el menú de `openMenu` (el `el.innerHTML = items.map(...)` de `work-marks.js`) al hacer clic. La variante `websys` usa `maud` + `web-sys` con los rasgos `Document`, `Element`, `HtmlElement`, `Event`, `MouseEvent`; las otras, la API idiomática de su framework. Mismo perfil de compilación para las cuatro:

```toml
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = "symbols"
```

Versiones: `leptos = { version = "=0.8.21", features = ["csr"] }`, `dioxus = { version = "=0.7.10", features = ["web"] }`, `yew = { version = "=0.23.0", features = ["csr"] }`, `maud = "=0.27.0"`, `web-sys = "=0.3.106"`, `wasm-bindgen = "=0.2.129"`.

- [ ] **Step 3: Medir tamaño.** Para cada variante y para una variante «vacía» (solo monta un `<div>`):

```bash
cd .spike/ui/<v> && CARGO_TARGET_DIR=../target nice -n 10 cargo build --release --target wasm32-unknown-unknown -j 6
wasm-bindgen --target web --out-dir pkg ../target/wasm32-unknown-unknown/release/<crate>.wasm
wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int pkg/<crate>_bg.wasm -o pkg/opt.wasm
gzip -9 -c pkg/opt.wasm | wc -c
```

Anotar raw y gzip; «coste fijo» = vacía; «coste de la porción» = porción − vacía.

- [ ] **Step 4: Paridad de DOM y convivencia en el Mac.** Servir `.spike/ui` en un puerto de la banda 7300–7399 con el servidor de fixtures de T2 (`cargo run -p xtask -- fixtures --root .spike/ui --port <p>`, que también sirve archivos estáticos) y `cc-browser-expose start <p>`; si T2 aún no está, T1 espera a su Step 9. Con `chrome-bg`: abrir `index-legacy.html` (carga el `work-marks.js` real con la fixture) y cada `index-<v>.html`; tras abrir el menú, `evaluate_script` de `() => document.querySelector('#wm-host').outerHTML` y comparar normalizado (orden de atributos indiferente, espacios colapsados). Convivencia: en cada variante, `evaluate_script` que añade la clase `wm-legacy-touch` y un atributo `data-x` al botón (como hace el JS heredado al decorar) y luego dispara una actualización de la fixture; se anota si la variante conserva, pisa o falla.
- [ ] **Step 5 (opcional, con visto bueno de Jesús): WebKitGTK 2.50.** Con permiso explícito para usar Xvfb local solo para el motor de `cc-app`: `timeout 20 xvfb-run -a /usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/MiniBrowser --enable-write-console-messages-to-stdout=true http://127.0.0.1:<p>/index-websys.html` y buscar en la salida la línea `WASM_OK <ms>` que imprime la variante al montar. Sin permiso se omite y se anota «no medido»: la compuerta D3 cubre el fallo en producción.
- [ ] **Step 6: Informe.** `docs/research/2026-10-04-fase-3-ui-rust.md` con: la tabla de §«Decisión de arquitectura» rellenada con los números, líneas Rust/JS por variante (`wc -l`), versiones exactas y fecha de su última ruptura (changelog), resultado de convivencia, WebKitGTK, y la aplicación literal de la regla de reversión. Si la regla cambiara la decisión, el plan 3b se replanifica antes de B1 (el controlador decide); si no, B1 cita el informe.
- [ ] **Step 7: Commit**

```bash
git add docs/research/2026-10-04-fase-3-ui-rust.md
git commit -m "docs(research): medición de frameworks web en Rust para la Fase 3 y decisión

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task T2: Arnés de paridad visual y de DOM en el Mac (`xtask shots`, `png-diff`, `dom-diff`, `fixtures`)

Sustituye `tools/png_diff.py` (spec §6) y da a toda la fase su oráculo visual: capturas en el Chrome del Mac por `cc-browser-remote` (MCP de Chrome DevTools 1.9.0 detrás del broker), recortes por componente y comparación con tolerancia; más una comparación de DOM normalizado, que localiza la diferencia cuando un recorte falla.

**Files:**
- Create: `xtask/src/mcp.rs`, `xtask/src/png_diff.rs`, `xtask/src/dom_diff.rs`, `xtask/src/shots.rs`, `xtask/src/fixtures.rs`
- Create: `xtask/web/shots.json`, `xtask/web/fixtures/base/*.json`
- Modify: `xtask/src/main.rs` (subcomandos), `xtask/Cargo.toml`
- Test: `xtask/tests/png_diff.rs`, `xtask/tests/dom_diff.rs`, `xtask/tests/mcp_frames.rs`

**Interfaces:**
- Produces:
  - `mcp::Client::spawn(command: &[&str]) -> Result<Client, String>`; `Client::call(&mut self, tool: &str, args: Value) -> Result<Value, String>`; `Client::tools(&self) -> &[ToolInfo]`; `Client::screenshot_png(&mut self) -> Result<Vec<u8>, String>`; `Client::eval(&mut self, function: &str) -> Result<Value, String>`.
  - `png_diff::Rgba { width: u32, height: u32, pixels: Vec<u8> }`, `png_diff::decode(&[u8]) -> Result<Rgba, String>`, `png_diff::crop(&Rgba, Rect) -> Result<Rgba, String>`, `png_diff::mask(&mut Rgba, Rect)`, `png_diff::diff(&Rgba, &Rgba, channel: u8) -> Result<DiffStats, String>`, `png_diff::write_diff_png(&Rgba, &Rgba, channel: u8, out: &Path) -> Result<(), String>`; `DiffStats { differing: u64, total: u64 }` con `ratio() -> f64`.
  - `dom_diff::normalize(html: &str) -> String`, `dom_diff::first_difference(a: &str, b: &str) -> Option<String>`.
  - `fixtures::serve(root: &Path, port: u16, posts: Arc<Mutex<Vec<Value>>>) -> io::Result<()>` (servidor de fixtures: si `root/<ruta>` es un archivo existente lo sirve tal cual con el tipo de `statics::mime_for`; si no, `GET /<ruta>` → `root/<ruta con / → _>.json`; POST registrado y respondido `{"ok":true}`).
  - `shots.json`: `[{"id","page","widths":[1400,844,390,320],"dpr":[1,2],"touch":bool,"selectors":[...],"mask":[...],"channel":24,"ratio":0.001}]`.
  - CLI: `cargo run -p xtask -- shots pair --base URL --suite NAME --out DIR` (carga `?web=off` y `?web=shadow` en el mismo navegador y compara), `shots remote-vs-desktop --base URL --out DIR`, `png-diff A B OUT`, `dom-diff A B`, `fixtures --root DIR --port N`.

- [ ] **Step 1: Prueba que falla — `png_diff`**

```rust
// xtask/tests/png_diff.rs
use xtask::png_diff::{Rect, Rgba, crop, diff, mask};

fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Rgba {
    Rgba { width: w, height: h, pixels: rgba.repeat((w * h) as usize) }
}

#[test]
fn identical_images_have_zero_ratio() {
    let a = solid(4, 4, [10, 20, 30, 255]);
    let stats = diff(&a, &a.clone(), 24).unwrap();
    assert_eq!((stats.differing, stats.total), (0, 16));
}

#[test]
fn channel_threshold_matches_png_diff_py() {
    // tools/png_diff.py: diferencia por canal convertida a gris, > 24 cuenta.
    let a = solid(2, 1, [100, 100, 100, 255]);
    let mut b = a.clone();
    b.pixels[0] = 124; // Δ=24 en R → gris ≈ 7: no cuenta
    b.pixels[4] = 220; // Δ=120 en R del segundo píxel → gris ≈ 36: cuenta
    let stats = diff(&a, &b, 24).unwrap();
    assert_eq!(stats.differing, 1);
}

#[test]
fn crop_and_mask_bound_the_comparison() {
    let mut a = solid(10, 10, [0, 0, 0, 255]);
    let mut b = a.clone();
    b.pixels[0] = 255; // (0,0) distinto, fuera del recorte
    let r = Rect { x: 2, y: 2, w: 4, h: 4 };
    assert_eq!(diff(&crop(&a, r).unwrap(), &crop(&b, r).unwrap(), 24).unwrap().differing, 0);
    let full = Rect { x: 0, y: 0, w: 10, h: 10 };
    mask(&mut a, Rect { x: 0, y: 0, w: 1, h: 1 });
    mask(&mut b, Rect { x: 0, y: 0, w: 1, h: 1 });
    assert_eq!(diff(&crop(&a, full).unwrap(), &crop(&b, full).unwrap(), 24).unwrap().differing, 0);
    assert!(crop(&a, Rect { x: 8, y: 8, w: 4, h: 4 }).is_err());
}

#[test]
fn size_mismatch_is_an_error() {
    assert!(diff(&solid(2, 2, [0; 4]), &solid(3, 2, [0; 4]), 24).is_err());
}
```

- [ ] **Step 2:** `$C test -p xtask --test png_diff` → FAIL (`png_diff` no existe).
- [ ] **Step 3: Implementación**

```rust
// xtask/src/png_diff.rs
//! Comparación de capturas: sustituye tools/png_diff.py con la misma regla
//! (gris de la diferencia por canal > umbral) y recortes por componente.
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect { pub x: u32, pub y: u32, pub w: u32, pub h: u32 }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba { pub width: u32, pub height: u32, pub pixels: Vec<u8> }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffStats { pub differing: u64, pub total: u64 }

impl DiffStats {
    pub fn ratio(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.differing as f64 / self.total as f64 }
    }
}

pub fn decode(bytes: &[u8]) -> Result<Rgba, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().map_err(|e| format!("png: {e}"))?;
    let size = reader.output_buffer_size().ok_or("png: tamaño desconocido")?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("png: {e}"))?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(format!("png: formato {:?}/{:?} no soportado", info.color_type, info.bit_depth));
    }
    buf.truncate(info.buffer_size());
    Ok(Rgba { width: info.width, height: info.height, pixels: buf })
}

fn offset(img: &Rgba, x: u32, y: u32) -> usize {
    ((y as usize) * (img.width as usize) + x as usize) * 4
}

pub fn crop(img: &Rgba, r: Rect) -> Result<Rgba, String> {
    if r.x.saturating_add(r.w) > img.width || r.y.saturating_add(r.h) > img.height {
        return Err(format!("recorte {r:?} fuera de {}x{}", img.width, img.height));
    }
    let mut pixels = Vec::with_capacity((r.w * r.h * 4) as usize);
    for y in r.y..r.y + r.h {
        let start = offset(img, r.x, y);
        let row = img.pixels.get(start..start + (r.w as usize) * 4).ok_or("recorte: fila corta")?;
        pixels.extend_from_slice(row);
    }
    Ok(Rgba { width: r.w, height: r.h, pixels })
}

/// Pinta el rectángulo de magenta opaco: regiones volátiles (relojes, contadores).
pub fn mask(img: &mut Rgba, r: Rect) {
    let x_end = r.x.saturating_add(r.w).min(img.width);
    let y_end = r.y.saturating_add(r.h).min(img.height);
    for y in r.y.min(y_end)..y_end {
        for x in r.x.min(x_end)..x_end {
            let at = offset(img, x, y);
            if let Some(px) = img.pixels.get_mut(at..at + 4) {
                px.copy_from_slice(&[255, 0, 255, 255]);
            }
        }
    }
}

fn gray_delta(a: &[u8], b: &[u8]) -> u32 {
    // ImageChops.difference(...).convert("L"): ITU-R 601-2 sobre |Δ| por canal.
    let d = |i: usize| u32::from(a.get(i).copied().unwrap_or(0).abs_diff(b.get(i).copied().unwrap_or(0)));
    (d(0) * 299 + d(1) * 587 + d(2) * 114) / 1000
}

pub fn diff(a: &Rgba, b: &Rgba, channel: u8) -> Result<DiffStats, String> {
    if (a.width, a.height) != (b.width, b.height) {
        return Err(format!("tamaños distintos: {}x{} vs {}x{}", a.width, a.height, b.width, b.height));
    }
    let differing = a.pixels.chunks_exact(4).zip(b.pixels.chunks_exact(4))
        .filter(|(pa, pb)| gray_delta(pa, pb) > u32::from(channel)).count() as u64;
    Ok(DiffStats { differing, total: u64::from(a.width) * u64::from(a.height) })
}

pub fn write_diff_png(a: &Rgba, b: &Rgba, channel: u8, out: &Path) -> Result<(), String> {
    diff(a, b, channel)?;
    let mut pixels = a.pixels.clone();
    for (i, (pa, pb)) in a.pixels.chunks_exact(4).zip(b.pixels.chunks_exact(4)).enumerate() {
        if gray_delta(pa, pb) > u32::from(channel) {
            if let Some(px) = pixels.get_mut(i * 4..i * 4 + 4) { px.copy_from_slice(&[255, 0, 0, 255]); }
        }
    }
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), a.width, a.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| format!("png: {e}"))?;
    w.write_image_data(&pixels).map_err(|e| format!("png: {e}"))
}
```

`xtask/Cargo.toml`: `png = "=0.18.1"`, `base64 = "=0.22.1"`, `scraper = "=0.27.0"`, `hyper`/`hyper-util`/`tokio`/`http-body-util` con las mismas versiones que `comandos-server` (para `fixtures`); `[lib] path = "src/lib.rs"` con `pub mod png_diff; pub mod dom_diff; pub mod mcp; pub mod shots; pub mod fixtures;` (si `xtask` aún no tiene `lib.rs`, se crea y `main.rs` lo usa).

- [ ] **Step 4:** `$C test -p xtask --test png_diff` → PASS.
- [ ] **Step 5: Prueba que falla — `dom_diff` y tramas MCP**

```rust
// xtask/tests/dom_diff.rs
use xtask::dom_diff::{first_difference, normalize};

#[test]
fn attribute_order_and_whitespace_do_not_matter() {
    let a = r#"<div class="row" id="r1">  <b>Hola</b>
      </div>"#;
    let b = r#"<div id="r1" class="row"><b>Hola</b></div>"#;
    assert_eq!(normalize(a), normalize(b));
}

#[test]
fn class_order_matters_and_is_reported() {
    let a = r#"<span class="a b">x</span>"#;
    let b = r#"<span class="b a">x</span>"#;
    let d = first_difference(a, b).unwrap();
    assert!(d.contains("class"), "{d}");
}

#[test]
fn text_difference_is_located() {
    let d = first_difference("<p>Pomodoro 25</p>", "<p>Pomodoro 15</p>").unwrap();
    assert!(d.contains("25") && d.contains("15"), "{d}");
}
```

```rust
// xtask/tests/mcp_frames.rs
use xtask::mcp::{image_from_result, request_line};

#[test]
fn request_is_one_json_line() {
    let line = request_line(7, "tools/call", serde_json::json!({"name": "list_pages", "arguments": {}}));
    assert!(line.ends_with('\n') && !line[..line.len() - 1].contains('\n'));
    let v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], 7);
}

#[test]
fn screenshot_image_is_extracted() {
    let result = serde_json::json!({"content": [
        {"type": "text", "text": "Took a screenshot"},
        {"type": "image", "mimeType": "image/png", "data": "iVBORw0KGgo="}]});
    assert_eq!(image_from_result(&result).unwrap(), vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
}
```

- [ ] **Step 6:** `$C test -p xtask --test dom_diff --test mcp_frames` → FAIL.
- [ ] **Step 7: Implementación de `dom_diff` y `mcp`**

```rust
// xtask/src/dom_diff.rs
//! DOM normalizado: etiquetas, atributos ordenados por nombre (el valor de
//! `class` conserva su orden, que sí cambia el CSS), texto con espacios
//! colapsados; los nodos de solo espacios desaparecen. Una línea por nodo.
use scraper::{Html, Node};

pub fn normalize(html: &str) -> String {
    let doc = Html::parse_fragment(html);
    let mut out = String::new();
    walk(doc.tree.root(), 0, &mut out);
    out
}

fn walk(node: ego_tree::NodeRef<'_, Node>, depth: usize, out: &mut String) {
    for child in node.children() {
        match child.value() {
            Node::Element(el) => {
                let mut attrs: Vec<(String, String)> =
                    el.attrs().map(|(k, v)| (k.to_string(), v.split_whitespace().collect::<Vec<_>>().join(" "))).collect();
                attrs.sort();
                out.push_str(&"  ".repeat(depth));
                out.push('<');
                out.push_str(el.name());
                for (k, v) in attrs { out.push_str(&format!(" {k}={v:?}")); }
                out.push_str(">\n");
                walk(child, depth + 1, out);
            }
            Node::Text(t) => {
                let text = t.split_whitespace().collect::<Vec<_>>().join(" ");
                if !text.is_empty() {
                    out.push_str(&"  ".repeat(depth));
                    out.push_str(&format!("#text {text:?}\n"));
                }
            }
            _ => walk(child, depth, out),
        }
    }
}

pub fn first_difference(a: &str, b: &str) -> Option<String> {
    let (na, nb) = (normalize(a), normalize(b));
    let mut la = na.lines();
    let mut lb = nb.lines();
    for n in 1.. {
        match (la.next(), lb.next()) {
            (None, None) => return None,
            (x, y) if x == y => continue,
            (x, y) => return Some(format!("nodo {n}: {:?} ≠ {:?}", x.unwrap_or("<fin>"), y.unwrap_or("<fin>"))),
        }
    }
    None
}
```

(`ego_tree` llega con `scraper`; se declara con la misma versión que resuelva `Cargo.lock` como `ego-tree = "=<versión del lock>"`.)

```rust
// xtask/src/mcp.rs
//! Cliente MCP mínimo (JSON-RPC 2.0, una línea por mensaje) sobre
//! `cc-browser-remote`, que reenvía por SSH al broker de Chrome del Mac.
//! Nunca lanza un navegador local.
use base64::Engine as _;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct ToolInfo { pub name: String, pub schema: Value }

pub struct Client { child: Child, stdin: ChildStdin, stdout: BufReader<ChildStdout>, next: u64, tools: Vec<ToolInfo> }

pub fn request_line(id: u64, method: &str, params: Value) -> String {
    let mut s = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
    s.push('\n');
    s
}

pub fn image_from_result(result: &Value) -> Result<Vec<u8>, String> {
    let items = result.get("content").and_then(Value::as_array).ok_or("respuesta sin content")?;
    let data = items.iter()
        .find(|c| c.get("type").and_then(Value::as_str) == Some("image"))
        .and_then(|c| c.get("data")).and_then(Value::as_str).ok_or("respuesta sin imagen")?;
    base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| format!("base64: {e}"))
}

impl Client {
    /// `command` por omisión: `[~/.local/bin/cc-browser-remote]`.
    pub fn spawn(command: &[&str]) -> Result<Client, String> {
        let (program, args) = command.split_first().ok_or("comando vacío")?;
        let mut child = Command::new(program).args(args)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit())
            .spawn().map_err(|e| format!("{program}: {e}"))?;
        let stdin = child.stdin.take().ok_or("sin stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("sin stdout")?);
        let mut c = Client { child, stdin, stdout, next: 1, tools: Vec::new() };
        c.rpc("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "comandos-xtask-shots", "version": "1"}}))?;
        c.notify("notifications/initialized")?;
        let listed = c.rpc("tools/list", json!({}))?;
        c.tools = listed.get("tools").and_then(Value::as_array).into_iter().flatten()
            .filter_map(|t| Some(ToolInfo { name: t.get("name")?.as_str()?.to_string(),
                schema: t.get("inputSchema").cloned().unwrap_or(Value::Null) }))
            .collect();
        Ok(c)
    }

    pub fn tools(&self) -> &[ToolInfo] { &self.tools }

    fn notify(&mut self, method: &str) -> Result<(), String> {
        let line = json!({"jsonrpc": "2.0", "method": method}).to_string() + "\n";
        self.stdin.write_all(line.as_bytes()).map_err(|e| format!("mcp: {e}"))
    }

    fn rpc(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        self.stdin.write_all(request_line(id, method, params).as_bytes()).map_err(|e| format!("mcp: {e}"))?;
        self.stdin.flush().map_err(|e| format!("mcp: {e}"))?;
        let mut line = String::new();
        loop {
            line.clear();
            if self.stdout.read_line(&mut line).map_err(|e| format!("mcp: {e}"))? == 0 {
                return Err("mcp: el broker cerró la conexión".into());
            }
            let msg: Value = serde_json::from_str(line.trim_end()).map_err(|e| format!("mcp: {e}"))?;
            if msg.get("id").and_then(Value::as_u64) != Some(id) { continue; } // notificaciones
            if let Some(err) = msg.get("error") { return Err(format!("mcp {method}: {err}")); }
            return msg.get("result").cloned().ok_or_else(|| "mcp: respuesta sin result".into());
        }
    }

    pub fn call(&mut self, tool: &str, args: Value) -> Result<Value, String> {
        if !self.tools.iter().any(|t| t.name == tool) {
            return Err(format!("el broker no ofrece la herramienta {tool}"));
        }
        let result = self.rpc("tools/call", json!({"name": tool, "arguments": args}))?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(format!("{tool}: {result}"));
        }
        Ok(result)
    }

    pub fn screenshot_png(&mut self) -> Result<Vec<u8>, String> {
        let r = self.call("take_screenshot", json!({"format": "png", "fullPage": true}))?;
        image_from_result(&r)
    }

    /// `function` es una función JS de una línea, p. ej. `() => document.title`.
    pub fn eval(&mut self, function: &str) -> Result<Value, String> {
        let r = self.call("evaluate_script", json!({"function": function}))?;
        let text = r.get("content").and_then(Value::as_array).into_iter().flatten()
            .filter_map(|c| c.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n");
        // La herramienta devuelve el valor como bloque ```json … ```.
        let body = text.split("```json").nth(1).and_then(|s| s.split("```").next()).unwrap_or(&text);
        serde_json::from_str(body.trim()).map_err(|e| format!("eval: {e}: {text}"))
    }
}

impl Drop for Client {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}
```

- [ ] **Step 8:** `$C test -p xtask --test dom_diff --test mcp_frames` → PASS.
- [ ] **Step 9: `fixtures` y `shots`.** `fixtures::serve` es un servidor hyper de un hilo (`tokio` `current_thread`) que mapea la ruta sin consulta a un archivo (`/usage/state` → `usage_state.json`; si existe `<archivo>.<método>.json` lo prefiere), responde `404 {"error":"fixture ausente"}` si falta (el arnés lo cuenta como fallo) y registra cada POST en `posts`. `shots` implementa:
  - `pair`: por cada entrada de la suite y cada `width × dpr × touch`: `resize_page`/`emulate` (si `tools()` incluye `emulate` con `viewport`, úsalo con `deviceScaleFactor`, `hasTouch`, `isMobile`; si no, `resize_page` y anota `"touch": "OMITIDO"` en el informe — nunca en silencio), `navigate_page` a `base + page + "?web=off&__clock=<ms>"`, espera la marca de listo (`eval("() => !!window.__comandosReady || document.readyState === 'complete'")`, reintentos 100 ms, 10 s), captura; repite con `?web=shadow`; para cada selector: `eval("() => [...document.querySelectorAll(SEL)].map(e => {const r=e.getBoundingClientRect(); return [r.x,r.y,r.width,r.height]})")` (escalado por `dpr`), recorta, enmascara `mask`, compara con `channel`/`ratio`, guarda `legacy.png`, `web.png`, `diff.png` y, si falla, el `first_difference` de `outerHTML` de ese selector. Salida: `DIR/report.json` + resumen en stdout; código 1 si algún recorte supera `ratio`.
  - `remote-vs-desktop`: carga la misma variante como escritorio (origen loopback) y como remoto (fixture con `COMANDOS_DASH_TEST_HOOKS=1` y `?__devwebterm=1`, que el compositor convierte en `window.__COMANDOS_DEV_WEBTERM = true`, el gancho de prueba que `dash/index.html` ya contempla) a 1400, 844, 390 y 320 px; para cada selector de la lista de componentes comprueba **presencia** en ambos y **mismos estilos calculados** (`color`, `background-color`, `font-family`, `font-size`, `font-weight`, `border-*-color`) salvo los selectores de `"remote_only"` (`#term-toolbar`, campo de entrada remoto) y las propiedades de disposición (`display`, `flex-*`, `grid-*`, `width`, `height`, `order`). Es la regla de Jesús convertida en prueba.
- [ ] **Step 10: Comprobación real (sin tocar producción).** `devhost add comandos-shots` (puerto de la banda 7300–7399; si devhost asigna 7200–7299, se pide uno de 73xx con `devhost add comandos-shots 73NN`), `cargo run -p xtask -- fixtures --root xtask/web/fixtures/base --port <p> &`, `cc-browser-expose start <p>`, `cargo run -p xtask -- shots pair --base http://127.0.0.1:<p> --suite smoke --out /tmp/claude-1000/shots-smoke` con una suite `smoke` que captura una página estática de fixtures dos veces: ratio 0. `cc-browser-expose stop <p>`.
- [ ] **Step 11: Commit**

```bash
git add xtask/Cargo.toml xtask/src/lib.rs xtask/src/main.rs xtask/src/mcp.rs xtask/src/png_diff.rs xtask/src/dom_diff.rs xtask/src/shots.rs xtask/src/fixtures.rs xtask/web/shots.json xtask/web/fixtures/base xtask/tests/png_diff.rs xtask/tests/dom_diff.rs xtask/tests/mcp_frames.rs Cargo.lock
git commit -m "feat(xtask): arnés de paridad visual y de DOM en el Chrome del Mac (shots, png-diff, dom-diff, fixtures)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task T3: WebSocket en el transporte de `comandos-server`

**Files:**
- Create: `crates/comandos-server/src/ws.rs`
- Modify: `crates/comandos-server/src/lib.rs` (`Config.websocket`, `Limits.websockets`, `dispatch`, `serve`), `crates/comandos-server/src/dash/mod.rs` (`limits()` con `websockets: 64`; `build` pasa `websocket: None` hasta A4), `crates/comandos-server/Cargo.toml`
- Test: `crates/comandos-server/tests/ws_upgrade.rs`

**Interfaces:**
- Consumes: `dispatch(request, peer, state)` y la puerta (`access::request_admission`) existentes en `lib.rs`.
- Produces:
  - `pub type WsStream = tokio_tungstenite::WebSocketStream<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>>;`
  - `pub struct WsRequest { pub target: String, pub peer: SocketAddr, pub headers: Vec<(String, String)>, pub protocol: Option<&'static str>, pub internal_producer: bool, pub shutdown: watch::Receiver<bool> }`
  - `pub type WsHandler = Arc<dyn Fn(WsRequest, WsStream) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;`
  - `pub struct WsRoute { pub accepts: Arc<dyn Fn(&str) -> Option<&'static [&'static str]> + Send + Sync>, pub handler: WsHandler }` — `accepts(path)` devuelve los subprotocolos admitidos para esa ruta (`Some(&[])` = sin subprotocolo) o `None` si la ruta no es WebSocket.
  - `Config.websocket: Option<WsRoute>`, `Limits.websockets: usize`.
  - `ws::is_upgrade(headers: &http::HeaderMap) -> bool`, `ws::accept_key(key: &str) -> String`, `ws::pick_protocol(offered: &http::HeaderMap, allowed: &'static [&'static str]) -> Result<Option<&'static str>, WsReject>`, `ws::switching_response(key: &str, protocol: Option<&str>) -> http::Response<…>`, `ws::into_stream(on: hyper::upgrade::OnUpgrade) -> io::Result<WsStream>` (configuración: `max_message_size = 1 MiB`, `max_frame_size = 1 MiB`, `max_write_buffer_size = 4 MiB`). Los usa también el oyente de compatibilidad de A4.

- [ ] **Step 1: Prueba que falla**

```rust
// crates/comandos-server/tests/ws_upgrade.rs
//! Upgrade a WebSocket: puerta de seguridad, subprotocolo, cupo y eco.
use comandos_server::{Config, WsRoute, WsHandler};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

mod support; // levanta `serve` en 127.0.0.1:0 con token "t0k" y devuelve el puerto

fn echo_route() -> WsRoute {
    let handler: WsHandler = Arc::new(|_req, mut ws| Box::pin(async move {
        while let Some(Ok(msg)) = ws.next().await {
            if msg.is_binary() || msg.is_text() { let _ = ws.send(msg).await; }
        }
    }));
    WsRoute { accepts: Arc::new(|p| (p == "/echo").then_some(&["tty", "comandos.term.v1"][..])), handler }
}

#[tokio::test(flavor = "current_thread")]
async fn loopback_upgrade_echoes_and_picks_first_offered_protocol() {
    let port = support::serve_with_ws(echo_route(), 4).await;
    let mut req = format!("ws://127.0.0.1:{port}/echo").into_client_request().unwrap();
    req.headers_mut().insert("Sec-WebSocket-Protocol", "tty, comandos.term.v1".parse().unwrap());
    req.headers_mut().insert("Origin", format!("http://127.0.0.1:{port}").parse().unwrap());
    let (mut ws, resp) = tokio_tungstenite::connect_async(req).await.unwrap();
    assert_eq!(resp.headers()["sec-websocket-protocol"], "tty");
    ws.send(Message::binary(vec![b'0', b'x'])).await.unwrap();
    assert_eq!(ws.next().await.unwrap().unwrap(), Message::binary(vec![b'0', b'x']));
}

#[tokio::test(flavor = "current_thread")]
async fn foreign_origin_is_rejected_before_upgrade() {
    let port = support::serve_with_ws(echo_route(), 4).await;
    let mut req = format!("ws://127.0.0.1:{port}/echo").into_client_request().unwrap();
    req.headers_mut().insert("Origin", "https://evil.example".parse().unwrap());
    let err = tokio_tungstenite::connect_async(req).await.unwrap_err();
    assert!(err.to_string().contains("403"), "{err}");
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_protocol_and_unknown_path_are_refused() {
    let port = support::serve_with_ws(echo_route(), 4).await;
    let mut req = format!("ws://127.0.0.1:{port}/echo").into_client_request().unwrap();
    req.headers_mut().insert("Sec-WebSocket-Protocol", "chat".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(req).await.is_err());
    let req = format!("ws://127.0.0.1:{port}/nada").into_client_request().unwrap();
    assert!(tokio_tungstenite::connect_async(req).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_cap_answers_503_and_http_keeps_working() {
    let port = support::serve_with_ws(echo_route(), 1).await;
    let first = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/echo")).await.unwrap();
    let err = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/echo")).await.unwrap_err();
    assert!(err.to_string().contains("503"), "{err}");
    assert_eq!(support::get_status(port, "/").await, 404); // el HTTP normal sigue atendiendo
    drop(first);
}
```

`support::serve_with_ws(route, cap)` construye `Config { websocket: Some(route), limits: Limits { websockets: cap, ..support::limits() }, .. }` con el manejador HTTP de prueba que ya usan los tests del transporte (404 JSON para todo).

- [ ] **Step 2:** `$C test -p comandos-server --test ws_upgrade` → FAIL (no compila: `WsRoute` no existe).
- [ ] **Step 3: Implementación de `ws.rs`**

```rust
// crates/comandos-server/src/ws.rs
//! Apretón de manos WebSocket (RFC 6455) sobre el upgrade de hyper. La puerta
//! de seguridad ya corrió en `dispatch`; aquí solo se valida el protocolo.
use http::{HeaderMap, HeaderValue, Response, StatusCode, header};
use http_body_util::Full;
use bytes::Bytes;
use hyper_util::rt::TokioIo;
use std::io;
use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::{Role, WebSocketConfig}};

pub type WsStream = WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsReject { BadHandshake, Protocol }

fn has_token(headers: &HeaderMap, name: header::HeaderName, token: &str) -> bool {
    headers.get_all(name).iter().filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|t| t.trim().eq_ignore_ascii_case(token))
}

pub fn is_upgrade(headers: &HeaderMap) -> bool {
    has_token(headers, header::CONNECTION, "upgrade") && has_token(headers, header::UPGRADE, "websocket")
}

pub fn accept_key(key: &str) -> String {
    tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes())
}

/// Primer subprotocolo ofrecido por el cliente que la ruta admite. Sin
/// cabecera y con lista admitida no vacía: rechazo (ttyd exige `tty`).
pub fn pick_protocol(headers: &HeaderMap, allowed: &'static [&'static str]) -> Result<Option<&'static str>, WsReject> {
    let offered: Vec<String> = headers.get_all(header::SEC_WEBSOCKET_PROTOCOL).iter()
        .filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(','))
        .map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
    if allowed.is_empty() {
        return if offered.is_empty() { Ok(None) } else { Err(WsReject::Protocol) };
    }
    offered.iter().find_map(|o| allowed.iter().find(|a| **a == o.as_str()).copied())
        .map(Some).ok_or(WsReject::Protocol)
}

pub fn handshake_key(headers: &HeaderMap) -> Result<String, WsReject> {
    if headers.get(header::SEC_WEBSOCKET_VERSION).and_then(|v| v.to_str().ok()) != Some("13") {
        return Err(WsReject::BadHandshake);
    }
    headers.get(header::SEC_WEBSOCKET_KEY).and_then(|v| v.to_str().ok())
        .filter(|k| k.len() == 24).map(str::to_string).ok_or(WsReject::BadHandshake)
}

pub fn switching_response(key: &str, protocol: Option<&str>) -> Response<Full<Bytes>> {
    let mut resp = Response::new(Full::new(Bytes::new()));
    *resp.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    let h = resp.headers_mut();
    h.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
    h.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
    if let Ok(v) = HeaderValue::from_str(&accept_key(key)) { h.insert(header::SEC_WEBSOCKET_ACCEPT, v); }
    if let Some(p) = protocol.and_then(|p| HeaderValue::from_str(p).ok()) { h.insert(header::SEC_WEBSOCKET_PROTOCOL, p); }
    resp
}

pub fn config() -> WebSocketConfig {
    let mut c = WebSocketConfig::default();
    c.max_message_size = Some(1 << 20);
    c.max_frame_size = Some(1 << 20);
    c.max_write_buffer_size = 4 << 20;
    c
}

pub async fn into_stream(on: hyper::upgrade::OnUpgrade) -> io::Result<WsStream> {
    let upgraded = on.await.map_err(io::Error::other)?;
    Ok(WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Server, Some(config())).await)
}
```

(Si la versión fijada de `tungstenite` expone `WebSocketConfig` con *builder* en lugar de campos públicos, se usa el *builder* con los mismos valores; la prueba de A3 que manda 2 MiB lo verifica.)

- [ ] **Step 4: Integración en `lib.rs`.** En `dispatch`, justo después de calcular `admission` e `internal_producer` (la puerta ya pasó) y antes de leer el cuerpo:

```rust
    if let Some(route) = state.config.websocket.as_ref()
        && method == access::Method::Get
        && crate::ws::is_upgrade(&parts.headers)
        && let Some(allowed) = (route.accepts)(parts.uri.path())
    {
        return websocket(state.clone(), parts, peer, headers, internal_producer, allowed, route.handler.clone());
    }
```

y la función, que responde 101 y lanza el manejador con su propio permiso de cupo (el de la conexión HTTP se suelta cuando hyper cede el socket):

```rust
fn websocket(
    state: Arc<State>,
    mut parts: http::request::Parts,
    peer: SocketAddr,
    headers: Vec<(String, String)>,
    internal_producer: bool,
    allowed: &'static [&'static str],
    handler: WsHandler,
) -> Response<OutputBody> {
    let key = match crate::ws::handshake_key(&parts.headers) {
        Ok(k) => k,
        Err(_) => return reject(400, "WebSocket invalido", true),
    };
    let protocol = match crate::ws::pick_protocol(&parts.headers, allowed) {
        Ok(p) => p,
        Err(_) => return reject(400, "Subprotocolo no soportado", true),
    };
    let Ok(permit) = state.websockets.clone().try_acquire_owned() else {
        return reject(503, "Tablero ocupado", true);
    };
    let Some(on) = parts.extensions.remove::<hyper::upgrade::OnUpgrade>() else {
        return reject(400, "WebSocket invalido", true);
    };
    let request = WsRequest {
        target: parts.uri.path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default(),
        peer, headers, protocol, internal_producer, shutdown: state.shutdown.clone(),
    };
    tokio::spawn(async move {
        let _permit = permit;
        if let Ok(stream) = crate::ws::into_stream(on).await { handler(request, stream).await; }
    });
    // `OutputBody` se construye como en `reject`, con la respuesta 101 vacía.
    switching_reply(crate::ws::switching_response(&key, protocol))
}
```

`State` gana `websockets: Arc<Semaphore>` (`Semaphore::new(limits.websockets)`) y `shutdown: watch::Receiver<bool>` (el `connection_shutdown` que ya existe en `serve`); `serve_connection(...)` pasa a `.serve_connection(...).with_upgrades()`. `switching_reply` adapta la `Response<Full<Bytes>>` al tipo de respuesta que devuelve `dispatch` (mismo patrón que `reject`). `Cargo.toml`: `tokio-tungstenite = { version = "=0.30.0", default-features = false, features = ["handshake"] }`; en `[dev-dependencies]` lo mismo con `features = ["connect"]` para el cliente de prueba.

- [ ] **Step 5:** `$C test -p comandos-server --test ws_upgrade` → PASS; `$C test -p comandos-server` → todo PASS (el transporte HTTP no cambia).
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/ws.rs crates/comandos-server/src/lib.rs crates/comandos-server/src/dash/mod.rs crates/comandos-server/Cargo.toml crates/comandos-server/tests/ws_upgrade.rs crates/comandos-server/tests/support Cargo.lock
git commit -m "feat(server): upgrade a WebSocket tras la puerta de seguridad, con subprotocolo y cupo propio

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task T4: Compilación WASM y su lugar en la release (`xtask web-build`)

**Files:**
- Create: `xtask/src/web_build.rs`
- Modify: `xtask/src/main.rs`, `Cargo.toml` (perfil `release-wasm`), `crates/comandos-cli/src/install/release.rs` (o el módulo real de `stage_release`), `crates/comandos-server/src/dash/mod.rs` (`DashConfig.web_dir`, `WEB_DIR_ENV = "COMANDOS_WEB_DIR"`)
- Test: `xtask/tests/web_build.rs`, `crates/comandos-cli/tests/install_web.rs`

**Interfaces:**
- Produces:
  - `web_build::BOOT_TEMPLATE: &str` y `web_build::render_boot(module_js: &str, wasm: &str) -> String` (cargador generado, ≤ 30 líneas, sin lógica: importa la cola de `wasm-bindgen`, instancia, llama `boot(data-k)` exportada por el WASM).
  - `web_build::Manifest { files: BTreeMap<String, String> }` (nombre lógico → ruta con hash, p. ej. `"comandos_web_bg.wasm" → "a1b2c3d4e5f6/comandos_web_bg.wasm"`), escrito en `target/web/manifest.json`. *(Implementado en T4: el tipo vive en `comandos_core::web_assets::Manifest` y `xtask` lo reexporta. Las claves por crate son `<lib>.js`, `<lib>_bg.wasm` y, en módulos ES, `<lib>_boot.js` → `<hash>/boot.js`; el cargador del tablero es `"comandos_web_boot.js"`, no `"boot.js"`. `target/web` es `<CARGO_TARGET_DIR o <workspace>/target>/web`, `xtask::web_build::out_dir()`.)*
  - CLI `cargo run -p xtask -- web-build [--crate comandos-web|comandos-term-web|comandos-web-sw|all] [--check-budget]`: compila con `--profile release-wasm --target wasm32-unknown-unknown`, `wasm-bindgen --target web` (sw: `--target no-modules`), `wasm-opt -Oz`, hash `sha256:12` del contenido, copia a `target/web/<hash>/`, escribe `manifest.json`; con `--check-budget` falla si un `.wasm` supera su presupuesto gzip (600/250/64 KiB).
  - `DashConfig.web_dir: Option<PathBuf>`: `COMANDOS_WEB_DIR` o `<directorio del ejecutable real>/web` si existe.
  - `comandos install --stage --web <dir>` (o `COMANDOS_WEB_SOURCE=<dir>`; vacío = sin web) copia ese `web/` a `releases/<id>/web/`. **Origen siempre explícito** (fallo del controlador a I2 de la revisión T4): nunca se deduce `../web` ni otro vecino del binario. Única excepción: re-instalar desde `releases/<id>/comandos` usa el `web/` de esa release y exige el mismo id. `web/` necesita un `manifest.json` válido (rutas llanas que existen). El id es `sha12(binario ‖ "web\0" ‖ árbol de web/)`; sin web, `sha12(binario)` como antes. `--stage` imprime `release <id> (web: <ruta canónica>, N archivos)` o `release <id> (sin web)`.

- [ ] **Step 1: Prueba que falla**

```rust
// xtask/tests/web_build.rs
use xtask::web_build::{Manifest, content_hash, render_boot};

#[test]
fn boot_is_generated_short_and_logic_free() {
    let js = render_boot("./comandos_web.js", "./comandos_web_bg.wasm");
    assert!(js.lines().count() <= 30, "{js}");
    assert!(js.contains("import init, { boot } from \"./comandos_web.js\""));
    assert!(js.contains("await init({ module_or_path: new URL(\"./comandos_web_bg.wasm\", import.meta.url) })"));
    assert!(!js.contains("fetch(\"/") , "el cargador no habla con el servidor: lo hace el WASM");
}

#[test]
fn hash_is_stable_and_12_hex() {
    let h = content_hash(&[b"a".as_slice(), b"b".as_slice()]);
    assert_eq!(h.len(), 12);
    assert_eq!(h, content_hash(&[b"a".as_slice(), b"b".as_slice()]));
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn manifest_round_trips() {
    let mut m = Manifest::default();
    m.files.insert("boot.js".into(), "0123456789ab/boot.js".into());
    let back: Manifest = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(back.files["boot.js"], "0123456789ab/boot.js");
}
```

- [ ] **Step 2:** `$C test -p xtask --test web_build` → FAIL.
- [ ] **Step 3: Implementación**

```rust
// xtask/src/web_build.rs (extracto: plantilla y hash; el resto orquesta procesos)
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Cargador generado. Única lógica: instanciar y ceder el control al WASM,
/// que monta, publica sus globales y avisa a la compuerta (`/web/ready`).
pub const BOOT_TEMPLATE: &str = r#"// Generado por `cargo xtask web-build`. No editar.
import init, { boot } from "{{MODULE}}";
const me = document.currentScript || document.querySelector("script[data-k]");
await init({ module_or_path: new URL("{{WASM}}", import.meta.url) });
boot(me ? me.dataset.k || "" : "");
"#;

pub fn render_boot(module_js: &str, wasm: &str) -> String {
    BOOT_TEMPLATE.replace("{{MODULE}}", module_js).replace("{{WASM}}", wasm)
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Manifest { pub files: BTreeMap<String, String> }

pub fn content_hash(parts: &[&[u8]]) -> String {
    let mut h = Sha256::new();
    for p in parts { h.update(p); }
    h.finalize().iter().take(6).map(|b| format!("{b:02x}")).collect()
}
```

(`document.currentScript` es `null` en módulos; por eso el selector `script[data-k]`. El resto de `web_build.rs` llama a `cargo`, `wasm-bindgen` y `wasm-opt` con `std::process::Command`, comprueba que `wasm-bindgen --version` sea `0.2.129` y que `wasm-opt --version` contenga `116`, y falla con el comando de instalación si no.) `xtask/Cargo.toml` gana `sha2.workspace = true` y `serde = { version = "=1.0.229", features = ["derive"] }`.

Perfil en `Cargo.toml` raíz:

```toml
[profile.release-wasm]
inherits = "release"
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = "symbols"
```

- [ ] **Step 4:** `$C test -p xtask --test web_build` → PASS.
- [ ] **Step 5: Prueba que falla — release con `web/`.** `crates/comandos-cli/tests/install_web.rs`: con un HOME temporal y un directorio de origen con `comandos` y `web/{manifest.json,abc/boot.js}`, `install --stage --web <origen>/web` deja `releases/<id>/web/abc/boot.js`; sin `--web` (aunque exista `web/` junto al binario o en `../web`) la release se instala sin `web/` y con id `sha12(binario)`.
- [ ] **Step 6:** Implementar la copia en `stage_release` (copia recursiva sin seguir symlinks, permisos 0644/0755) → PASS. `DashConfig.web_dir` se resuelve en `from_env`.
- [ ] **Step 7: Commit**

```bash
git add Cargo.toml xtask/Cargo.toml xtask/src/web_build.rs xtask/src/main.rs xtask/tests/web_build.rs crates/comandos-cli/src/install crates/comandos-cli/tests/install_web.rs crates/comandos-server/src/dash/mod.rs Cargo.lock
git commit -m "feat(xtask): web-build compila los WASM con hash, presupuesto y lugar en la release

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Cutover y reversión (resumen; el detalle vive en los planes hijos)

| Pieza | Sombra | Activar | Revertir |
|---|---|---|---|
| Terminal paso 1 (A4) | frente 4782 `--shadow-readonly` + `/term/ws` con grabaciones; `xtask shots pair --suite term-xterm` | detener `cc-webterm-path`, reiniciar `cc-dash.service` con `COMANDOS_DASH_TERM=ttyd` y `COMANDOS_DASH_WEBTERM_COMPAT=4780` (drop-in) | quitar el drop-in + reinicio; `install --rollback cc-webterm` y `cc-webterm` vuelve a levantar ttyd |
| Terminal paso 2 (A11) | `comandos web set term shadow` y una pestaña «+» con `?web=shadow` | `comandos web set term on`; drop-in `COMANDOS_DASH_TERM=native`, `COMANDOS_DASH_WEBTERM_COMPAT=4780,4779`; detener `cc-webterm` | `comandos web set term off` (sin reinicio); oyente 4779 fuera + `cc-webterm` |
| Cada componente web (B4–B13) | `comandos web set <id> shadow` + `xtask shots pair` contra 4782 | `comandos web set <id> on` | `comandos web set <id> off` (sin reinicio) |
| Embebido y retirada (B15) | release nueva en sombra 4782 | `install --stage` + reinicio | `install --rollback-release` + reinicio |

Procedimientos completos: `docs/verification/cutover-term.md` (A4, A11) y `docs/verification/cutover-web.md` (B4, B15).
