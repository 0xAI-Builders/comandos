# ComandOS en Rust: diseño de la migración completa

Fecha: 2026-10-04. Rama: `migration/rust-full` (worktree `.worktrees/rust-full`), que parte
del HEAD `b2d2bf6` de `migration/rust` (trabajo previo de Codex). Aprobado en conversación por
Jesús el 2026-10-04.

## 1. Objetivo

Reescribir **todo el código propio de ComandOS en Rust** (hoy 159 archivos, 62 k líneas de
Python, JavaScript, HTML y bash) y sustituir también los componentes de terceros que
entregamos como parte del producto, de modo que al terminar no quede código nuestro en otro
lenguaje. La reescritura aprovecha para corregir de raíz los fallos conocidos y para dejar el
sistema más ligero en RAM, con menos procesos y menos archivos de estado.

No hay plazo. Hay orden: cada componente se entrega, se verifica contra la versión Python
que sigue corriendo y se activa de forma reversible, antes de pasar al siguiente.

## 2. Reglas de oro (requisitos no negociables)

1. **Ninguna función se omite.** El inventario `docs/rust-component-inventory.json` (159
   entradas propias) es la lista de control; una entrada se da por migrada cuando sus
   funciones, no sus líneas, tienen equivalente verificado.
2. **Lo más ligero y eficiente posible**, medido (RSS por proceso, número de procesos, CPU
   en reposo), nunca afirmado.
3. **Ninguna sesión Claude Code, Codex, Gemini, agy u opencode se rompe ni se interrumpe.**
   Las sesiones viven en el servidor tmux del usuario; ningún paso escribe en él, lo reinicia
   ni lo sustituye.
4. **Nada se despliega sin haber corrido en sombra contra el estado real** y sin un camino de
   vuelta de un solo comando.
5. **Al abrir la versión Rust se ven exactamente las mismas pestañas, splits, cuentas y
   conversaciones** que mostraba la versión Python en ese momento.

## 3. Alcance

### 3.1 Entra (se reescribe en Rust)

| Hoy | Mañana |
|---|---|
| 24 ejecutables `bin/cc-*` (Python/bash), `hooks/*.sh`, `adapters/*` | Un binario `comandos` (sin GTK) con subcomandos; los nombres `cc-*` quedan como symlinks al binario (despacho por `argv[0]`) para que systemd, `~/.claude/settings.json`, `~/.codex/config.toml` y los hooks de gemini/agy no cambien |
| `bin/cc-app` (GTK3 + WebKit2GTK + VTE, 9 k líneas) | `comandos-app` en Rust: gtk3-rs + webkit2gtk-rs 4.1; **terminal propio** (ver §4.4), sin VTE |
| `bin/cc-app-mac` (PyObjC) y `services/browser/broker.py` (macmini) | Rust con `objc2`/AppKit/WKWebView; broker en Rust. Se compilan y prueban en macmini por SSH |
| `dash/index.html`, `term.html`, `extensions.html` y 20 archivos JS (17 k líneas) | Crate `comandos-web` compilado a `wasm32-unknown-unknown`: plantillas Rust (`maud`) que generan el mismo DOM y clases; toda la lógica de interfaz en Rust con `web-sys`. El CSS no cambia |
| ttyd (C) + `cc-webterm*` | `comandos dash` sirve la terminal web él mismo: PTY↔WebSocket en Rust |
| xterm.js + addons + opentype.js | `comandos-term`: motor de terminal en Rust (ver §4.4), renderizado en canvas (web) y cairo/GL (escritorio) |
| markdown-it + DOMPurify | `pulldown-cmark` + `ammonia`, renderizado en el servidor |
| uisfx | Sonidos UI por Web Audio desde WASM |
| `dash/sw.js` | Service worker en WASM con shim generado |
| `install.sh`, `lib/platform.sh`, `tools/*.py`, `tools/*.cjs`, `scripts/*.py` | `comandos install`, `comandos doctor`, `cargo xtask` |
| 71 archivos JSON en `~/.claude/hooks/` + `session-operations.sqlite3` | Un solo `comandos.sqlite3` (WAL) con migrador, respaldo y rollback (§4.6) |
| 173 tests Python + 25 JS | Oráculo durante la transición; al final se eliminan y quedan tests Rust más el arnés diferencial en Rust |
| `cc-model-proxy` (`vendor/claude-codex`, ya Rust, 310 MB RSS) | Se mantiene; se audita su RAM en la fase de optimización |

### 3.2 No entra, y por qué

- **tmux.** Contiene los procesos de las sesiones del usuario. Sustituirlo los mataría (regla 3).
  Se gobierna desde Rust por su CLI y control mode; no se reemplaza.
- **Loader de WASM (~30 líneas) y plugin de opencode (~3 líneas).** El navegador solo instancia
  WASM desde JavaScript y opencode solo carga plugins JavaScript. Ambos son generados o
  triviales y no contienen lógica.
- **CSS, fuentes, iconos, sonidos.** Son el diseño (Medidor LED pixel perfect), no código.
- **GTK3, WebKit2GTK 4.1, SQLite, glibc, PipeWire.** Bibliotecas del sistema enlazadas desde
  Rust. Ubuntu 22.04 no ofrece GTK4 utilizable (4.6, sin webkitgtk-6.0 ni vte-gtk4), así que
  el escritorio usa la misma generación que hoy.
- **Prototipos (`dash/prototypes/*`, `design/*`) y evidencias.** Se archivan en `docs/`; no
  son producto.

### 3.3 Se descarta del trabajo previo de Codex

Las 65 k líneas de motor de expresiones regulares CPython, tablas Unicode 14 y herramienta
de respaldos (`crates/comandos-extensions/src/output_schema/python_regex*`,
`crates/comandos-extensions/examples/checkpoint*`) salen del producto. La validación de
patrones de esquemas usa el crate `regex`; los respaldos de estado los hace el migrador de
§4.6. El resto de módulos de Codex (núcleo de eventos, estado, workspace, Pomodoro, cuentas,
cuotas, controles de sesión, transporte HTTP, cuatro rutas) se reutilizan.

## 4. Arquitectura

### 4.1 Binarios y crates

```
comandos            binario headless (~10 MB): dash, notifyd, ext, hook, usage, agents,
                    mobile, acp, snapshot, keys, doctor, install, model-proxy-ctl
comandos-app        escritorio Linux (GTK3 + WebKit2GTK + comandos-term)
comandos-app-mac    escritorio macOS (AppKit + WKWebView), compilado en macmini
comandos-broker-mac broker de navegador remoto en macmini

crates/
  comandos-core        reglas puras, sin E/S; compila a wasm (ya existe)
  comandos-store       SQLite: esquema único, migrador, transacciones (ya existe, se amplía)
  comandos-runtime     procesos, tmux, PTY, fs, systemd (ya existe, se amplía)
  comandos-term        modelo VT + renderizadores (nuevo)
  comandos-extensions  catálogo, metadatos, proxy y broker MCP (ya existe, se limpia)
  comandos-server      HTTP/SSE/WebSocket y las 124 rutas (ya existe, se completa)
  comandos-web         interfaz WASM (nuevo)
  comandos-app         GTK (nuevo)
  comandos-cli         binario `comandos` (nuevo)
  xtask                tareas de desarrollo: build wasm, arnés, empaquetado (nuevo)
```

Perfil release: `lto = "thin"`, `codegen-units = 1`, `strip`, `panic = "abort"` en los
binarios; `opt-level = "z"` y `wasm-opt -Oz` en WASM. `unsafe_code = "forbid"` salvo en
`comandos-term` (acceso a PTY) y bindings, con revisión explícita.

### 4.2 Servidor (`comandos dash`)

Reemplaza `cc-dash` en el puerto 4777 con las mismas 124 rutas, mismos JSON byte a byte y la
misma puerta de seguridad (Host allowlist, Origin allowlist, token para acceso remoto).
Cambios internos: estado en SQLite en lugar de JSON en memoria; SQLite fuera del hilo de red
con cola acotada (ya implementado por Codex); SSE (`/events/stream`) para que la interfaz deje
de hacer polling; cachés acotadas y medidas, para eliminar el crecimiento a 1.47 GB observado
en el servidor Python tras 29 h. Sirve además la terminal web (§4.4) y los assets embebidos
en el binario (`include_bytes!`), de modo que la instalación no necesita symlinks a `dash/`.

### 4.3 Notificaciones, hooks y adaptadores

`comandos notifyd` sustituye `cc-notifyd` (popups propios GTK, sonidos por `pw-play`, Telegram).
`comandos hook <agente> <evento>` sustituye `cc-notify.sh`, `cc-status.sh`, `cc-usage-tool.sh`,
`codex-notify.sh`, `gemini-hooks.sh`, `agy-hooks.sh`, `grok-hooks.py` y la lógica de
`opencode-comandos.js`. Un hook en Rust arranca en ~1 ms frente a bash+python, lo que se nota
en cada turno de cada agente. Los archivos de configuración de los agentes conservan las rutas
actuales porque los symlinks `cc-*` siguen existiendo.

### 4.4 Terminal (`comandos-term`)

Un solo motor para web, móvil, macOS y escritorio Linux:

- **Modelo VT** en Rust puro: `alacritty_terminal` 0.26 vendorizado con un parche de dos líneas
  que excluye sus módulos `tty`/`event_loop` para compilar a wasm, y un temporizador propio para
  las actualizaciones sincronizadas (`CSI ? 2026 h`, que Claude Code emite). Validado en el spike
  del 2026-10-04 (`docs/research/2026-10-04-spike-terminal-rust.md`): 191 KB de wasm, puente
  PTY↔WebSocket de 2.5 MB de RSS, 5 ms de ida y vuelta por tecla hasta el navegador de macmini,
  colores/atributos/UTF-8/ratón/resize correctos contra tmux.
- **Transporte**: en web, WebSocket servido por `comandos dash` (`/term/ws?session=…`), un PTY
  por conexión que ejecuta `tmux attach -t =<sesión>` con el tamaño que informa el cliente
  antes del attach, para no encoger las sesiones (comportamiento actual de `cc-app` y
  `cc-webterm-attach`). En escritorio, PTY directo.
- **Renderizadores**: canvas 2D/WebGL vía `web-sys` en web; cairo (y GL si rinde mejor) en GTK.
  Fuentes y ligaduras con `rustybuzz` + `swash`. Selección, copia, hipervínculos OSC 8 y
  detección de URL, scrollback de 10 000 líneas, ratón, IME.
- **Paridad**: la referencia es el comportamiento actual de xterm.js (web) y VTE (escritorio);
  se comparan capturas en macmini para la web y capturas de la ventana GTK para el escritorio.

### 4.5 Escritorio y móvil

- `comandos-app` (Linux): misma estructura que `cc-app` (tablero WebKit a la izquierda,
  pestañas de terminal a la derecha, Ctrl+T, splits, restauración de pestañas desde el estado,
  menús, atajos, foco pedido por `app-focus`). Al arrancar lee el mismo estado que la versión
  Python, por lo que muestra las mismas pestañas; cada pestaña se engancha a la misma sesión
  tmux. El servidor tmux sigue en `tmux.service`, fuera del scope de la app.
- **Aislamiento por panel**: cada agente lanzado desde la app corre en su propio scope systemd
  transitorio (`systemd-run --user --scope`), de modo que systemd-oomd puede matar un panel,
  nunca la aplicación entera ni tmux. Corrige de raíz el incidente del 2026-10-02.
- Móvil/tablet: la misma interfaz WASM como PWA por `tailscale serve` (`comandos mobile`), con
  la terminal web de §4.4. macOS: `comandos-app-mac` con la misma interfaz y terminal.
- **Versión remota = versión de escritorio** (Jesús, 4-oct-2026): el tablero remoto/móvil es
  exactamente el de escritorio — mismas pestañas, splits, sesiones, columna de uso, analítica,
  notificaciones, sonidos, tipografía y diseño LED — sin recortes de funciones. Solo difieren
  (1) los controles pensados para remoto: la barra de botones de la terminal (Esc, ⌫, flechas,
  Ctrl+C, pegar…) y el campo de entrada que hoy documentan `docs/remote-terminal-input.md` y
  `docs/remote-session-controls.md`, y (2) la disposición, que es *flex* y se adapta al ancho
  del teléfono/tableta (columnas que se apilan, pestañas desplazables, terminal a ancho
  completo con el teclado virtual abierto) sin cambiar los componentes ni su aspecto. Las
  capturas de verificación de la Fase 3 comparan escritorio y remoto componente a componente.

### 4.6 Estado único

Tabla por dominio en `~/.local/share/comandos/comandos.sqlite3` (WAL, `busy_timeout`,
transacciones): sesiones y pestañas, splits, cuentas, cuotas y uso, notificaciones, Pomodoro,
marcas, noticias, preferencias, perfiles, snippets, cadenas, extensiones, operador. El
migrador importa los 71 JSON y los dos SQLite actuales, deja respaldo con hash en
`~/.local/share/comandos/backups/<fecha>/` y ofrece `comandos state rollback <fecha>`. Durante
la transición (fases 1-5) el binario Rust lee y escribe los **formatos actuales** para poder
correr en sombra y revertir; la consolidación es la última fase, cuando ya no queda Python.

### 4.7 Extensiones y broker MCP

`comandos ext serve <nombre>` reemplaza al proxy Python (72 MB por proceso, 232 procesos hoy)
con el proxy Rust ya revisado (~5 MB). Encima, un **broker**: un único proceso por servidor MCP
compartido por todas las sesiones, con multiplexación JSON-RPC (espacio de identificadores por
cliente, suscripciones y notificaciones repartidas, ciclo de vida por conteo de clientes). Las
sesiones nuevas lo usan al arrancar; las sesiones vivas conservan sus procesos Python hasta que
terminan por sí solas. Ahorro estimado con la carga de hoy: ≈ 28 GB.

### 4.8 Identidad de conversaciones

ComandOS lanza los agentes, así que puede registrar la identidad desde el origen: para Codex,
el id del rollout que aparece en `~/.codex/sessions` (inotify en el momento del lanzamiento)
más el `thread-id` del hook `notify`; para Claude, el `session_id` de los hooks. El id queda en
el estado por panel y se usa en la restauración, en lugar de inferirlo por cwd o por archivos
abiertos. Si hay ambigüedad real, la restauración lo dice en pantalla y ofrece elegir, en vez
de abrir un shell vacío.

## 5. Transición sin romper nada

1. **Desarrollo aislado** en el worktree; compilaciones con `nice` y límite de trabajos; ninguna
   prueba contra el servidor tmux del usuario (socket `-L comandos-test` propio), ni contra
   `~/.claude/hooks`, ni contra puertos de producción.
2. **Sombra**: `comandos dash` en otro puerto leyendo el estado real en solo lectura; el arnés
   diferencial (`xtask parity`) replica cada ruta GET contra ambos servidores y compara bytes;
   las rutas POST se ejercitan sobre una copia del estado. Igual para `notifyd` (misma cola de
   avisos, salida comparada) y para los hooks (mismo evento a ambos, mismo efecto en estado).
3. **Cutover por componente**, cada uno con symlink y `systemctl --user restart` de su unidad
   (nunca de la app ni de tmux): `cc-extensions` → hooks y adaptadores → `cc-dash` →
   `cc-notifyd` → `cc-webterm` (retirado) → `cc-app` (**solo cuando Jesús reinicie la app**;
   tmux conserva todo) → web/WASM → móvil → macOS → consolidación de estado → retirada de
   Python, ttyd y vendor JS.
4. **Reversión**: `comandos install --rollback <componente>` restaura el symlink anterior y
   reinicia la unidad. Hasta la consolidación de estado, Python y Rust comparten formatos, por
   lo que la vuelta es inmediata.
5. **Criterio de aceptación de cada cutover**: arnés en verde, RSS y número de procesos
   medidos y menores o iguales, y, para la app, captura de la lista de pestañas/splits antes y
   después idéntica.

## 6. Verificación

- Tests Rust por crate (unitarios y de integración con SQLite en memoria y tmux `-L` propio).
- Oráculo Python: los 173 tests existentes siguen corriendo contra el Python hasta su retirada;
  el arnés `xtask parity` compara Rust contra Python con los mismos datos de prueba.
- Visual: capturas con `chrome-bg` en macmini para la interfaz web y `import -window` para la
  ventana GTK, comparadas con `xtask png-diff` (sustituye `tools/png_diff.py`).
- Rendimiento: `xtask bench` mide RSS por proceso, arranque de hooks y CPU en reposo; los
  resultados se guardan en `docs/verification/`.
- `cargo clippy -D warnings`, `cargo fmt --check`, `cargo deny` para licencias y avisos.

## 7. Fases

| Fase | Entrega | Aceptación |
|---|---|---|
| 0 | Spike del motor de terminal; limpieza del workspace (desvíos fuera); `comandos-cli` con despacho por `argv[0]`; `xtask` | Spike con recomendación y números; `cargo build --workspace` limpio |
| 1 | `comandos ext serve` + broker; hooks y adaptadores | Cutover de proxy y hooks; RSS medido; sesiones vivas intactas |
| 2 | `comandos dash` con las 124 rutas, SSE, terminal web; `notifyd` | Arnés en verde en sombra; cutover de dash, notifyd y webterm |
| 3 | `comandos-term` renderizador canvas; `comandos-web` (toda la UI) | Capturas idénticas en macmini; PWA móvil funcionando |
| 4 | `comandos-app` GTK con `comandos-term` nativo | Mismas pestañas/splits al reiniciar la app; RSS medido |
| 5 | `comandos-app-mac` y broker en macmini | Funciona en la Mac con la misma UI |
| 6 | Estado único + migrador; retirada de Python/ttyd/vendor JS; `comandos install` | Repo sin Python/bash/JS propio; rollback probado |

Cada fase tiene su propio plan en `docs/superpowers/plans/` y se replanifica al cerrar la
anterior.

## 8. Riesgos y cómo se tratan

- **Motor de terminal**: la pieza más grande. El spike decide el crate antes de invertir; la
  paridad con xterm.js se mide con casos reales (htop, vim, claude, codex) y no se corta el
  cutover de la web hasta tenerla.
- **Bindings GTK3 en Rust** (gtk3-rs en mantenimiento, webkit2gtk-rs 4.1): se valida en la
  fase 4 con una app mínima antes del port completo. Sin VTE, el único binding delicado es
  WebKit.
- **Rendimiento del arnés**: 124 rutas con estado real; se ejecuta por lotes y en sombra sin
  escribir.
- **macOS**: compilación remota; si la Mac no tiene toolchain, se instala con `rustup` en el
  usuario de Jesús, previa confirmación.
- **Tiempo**: estimación honesta de 10–14 semanas de trabajo continuo con hasta cuatro agentes
  en paralelo. Las fases 1 y 2 ya reducen RAM de forma medible en los primeros días.

## Enmiendas (4 de octubre de 2026, tras cerrar la Fase 0/1 y el inventario de la Fase 2)

Hechos que el inventario `docs/research/2026-10-04-fase-2-inventario.md` corrige, y decisiones que
cambian el orden de las fases sin quitar nada del alcance:

1. **Rutas**: `cc-dash` tiene 161 pares método+ruta (63 GET, 97 POST, 1 DELETE), no 124; el
   trabajo son ≈ 26 500 líneas de Python (`bin/cc-dash` + 46 módulos de `lib/`), no 10 000.
   39 rutas no tienen llamador vivo (eran del chat retirado): se portan como `410 Ruta retirada`,
   con lista, y se portan de verdad si aparece un llamador.
2. **Transición de `cc-dash` por proxy inverso**: `comandos dash` toma el 4777 y reenvía al Python
   (movido a 4781, `cc-dash-legacy.service`) todo lo que aún no es nativo; los dominios se cortan y
   revierten uno a uno. El cutover único de §5.3 era inviable con 161 rutas.
3. **Estáticos**: hasta la Fase 3 se sirven desde `H/dash` (disco), porque otras sesiones editan
   `dash/*.js` en vivo; `include_bytes!` llega con `comandos-web`.
4. **`cc-notifyd` solo hace popups GTK**: la voz y el chime ya los hace el hook Rust, los sonidos de
   Pomodoro los hace `cc-dash`, Telegram está retirado. §4.3 queda corregido así. `comandos notifyd`
   se porta en la **Fase 4** junto con el resto de GTK (el binario `comandos` sigue headless).
5. **No existe SSE hoy**; lo único en vivo es el long-poll `GET /notices/watch` (25 s). SSE llega
   con la interfaz de la Fase 3, no es una migración.
6. **Terminal web**: la barra lateral del tablero en `cc-app` usa la UI embebida de ttyd (4779), no
   `term.html`; ttyd se retira en la **Fase 3** con `comandos-term`, no en la 2. Los dos scripts
   bash (`cc-webterm`, `cc-webterm-attach`) sí se portan en la 2b.
7. **Memoria**: `cc-dash` está en 1.59 GB tras 40 h (pico 1.99 GB). El criterio de aceptación por
   dominio es el RSS medido con carga sintética real (`xtask poll`, calendario del inventario §1.11).
8. **Releases versionadas** del binario (`releases/<sha256:12>/comandos` + enlace `bin/comandos`,
   `install --rollback-release`) antes de cualquier cutover nuevo (hallazgo de la revisión final
   de la Fase 0/1).

La Fase 2 se ejecuta en sub-planes: **2a** (releases, frente, estáticos, reenvío, arnés, carga
sintética; `docs/superpowers/plans/2026-10-04-fase-2a-dash-cimientos.md`) y **2b** (dominios
nativos por orden de carga; se planifica al cerrar la 2a).
