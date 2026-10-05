# Fase 4 — `comandos-app`: escritorio GTK en Rust con `comandos-term` nativo — plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** sustituir `bin/cc-app` (Python, GTK3 + WebKit2GTK + VTE, 9 019 líneas) y la parte GTK de `bin/cc-notifyd` (1 139 líneas) por el binario Rust `comandos-app`, con terminal propio sobre `comandos-term`, de forma que al reiniciar la app se vean exactamente las mismas pestañas, splits, cuentas y conversaciones, con RSS medido menor o igual, y con vuelta atrás de un solo comando.

**Architecture:** un crate nuevo `crates/comandos-app` (binario `comandos-app`, despacho por `argv[0]`: `cc-app`/`comandos-app` → app, `cc-notifyd`/`comandos-notifyd` → popups). Tres modos de ejecución que el tipo hace cumplir: **sandbox** (servidor tmux propio `-L`, hooks en un directorio temporal: desarrollo y pruebas), **shadow** (servidor tmux del usuario con clientes `attach -f read-only,ignore-size`, estado real en solo lectura, cero escrituras en tmux, en `~/.claude/hooks`, en el tablero o en la presencia) y **live** (el sustituto real, mismos archivos, mismo candado de instancia única que el Python, así que nunca corren los dos). La lógica que no necesita GTK (tmux, archivos de estado, reanudación, plan de restauración, colores, codificación de teclado y ratón, mensajes del puente, comandos IPC) vive en módulos puros con pruebas sin pantalla y oráculo Python; la capa GTK es fina. El terminal es un `gtk::DrawingArea` pintado con cairo + pango sobre el modelo de `comandos-term`, con PTY propio (`pty-process`) vigilado por el bucle de GLib: un solo hilo de interfaz, sin VTE.

**Tech Stack:** Rust 1.96 (edition 2024), gtk-rs GTK3 `gtk = "=0.18.2"`, `gdk = "=0.18.2"`, `glib = "=0.18.5"`, `gio = "=0.18.4"`, `cairo-rs = "=0.18.5"`, `pango = "=0.18.3"`, `pangocairo = "=0.18.0"`, `gdk-pixbuf = "=0.18.5"`, `webkit2gtk = "=2.0.2"` (WebKit2GTK 4.1, `features = ["v2_40"]`), `javascriptcore-rs = "=1.1.2"`, `soup3 = "=0.5.0"` (arrastrado por webkit2gtk), `pty-process = "=0.5.3"` (API `blocking`), `async-channel = "=2.5.0"`, `ureq = "=3.4.2"` (`default-features = false`, solo HTTP a 127.0.0.1), `regex = "=1.13.1"`, `nix = "=0.31.3"`, `serde_json`, `sha2` (workspace), `comandos-term` (Fase 3), `comandos-core`, `comandos-runtime` (`pane_snapshot::PaneInspector`).

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§2 reglas de oro, §3.2, §4.1, §4.4, §4.5, §5, §6, §7 fila «4», §8 riesgo «Bindings GTK3», Enmienda 4: la parte GTK de `cc-notifyd` se porta aquí y el binario `comandos` sigue headless). Spike del terminal: `docs/research/2026-10-04-spike-terminal-rust.md`. Plan de la Fase 3 (`docs/superpowers/plans/2026-10-04-fase-3-term-y-web.md`): **no existía al escribir este plan**; la sección «Interfaz que se asume de `comandos-term`» fija lo que esta fase consume y qué hacer si la Fase 3 entrega otra cosa. Oráculo: `bin/cc-app`, `bin/cc-notifyd`, `lib/tmux_snapshot.py`, `lib/agent_stop.py`, `lib/tmux_clipboard.py`, `lib/gtk_workspace.py`, `lib/gtk_tabstrip.py`, `lib/workspace_layout.py`, `lib/work_marks.py`; las líneas citadas son las de `bin/cc-app` en `HEAD` de `main` del 2026-10-04 (9 019 líneas) más el diff sin commitear del checkout principal (ver «Blanco móvil»).

**Precondición:** las Fases 2 (todas sus sub-fases) y 3 están fusionadas en `main` y `comandos dash` sirve el 4777. Si al empezar falta la Fase 3, la Tarea 4 cubre el motor con `alacritty_terminal` directo (ver «Interfaz que se asume»). El paquete de desarrollo `libwebkit2gtk-4.1-dev` debe estar instalado: hoy solo están las bibliotecas de ejecución (`libwebkit2gtk-4.1-0 2.50.4`) y `libgtk-3-dev 3.24.33`. **Lo instala Jesús** (`sudo apt install libwebkit2gtk-4.1-dev`); ningún agente usa `sudo`.

---

## Rulings del controlador que fijan este plan

1. **Ninguna sesión se rompe ni se interrumpe.** Ningún paso reinicia el servidor tmux, ni `tmux.service`, ni la app Python, ni escribe en una sesión del usuario fuera del modo **live** ya activado por Jesús. Las pruebas usan `tmux -L comandos-app-test-<pid>` con `TMUX_TMPDIR` temporal y `-f /dev/null`, y matan solo ese servidor.
2. **Cutover reversible y en manos de Jesús**: el cambio de `~/.local/bin/cc-app` (y de `cc-notifyd`) lo ejecuta el controlador solo con el OK explícito de Jesús en el chat; la vuelta es `comandos install --rollback cc-app`.
3. **Comentarios en español, identificadores en inglés**; `#![forbid(unsafe_code)]` en `comandos-app` (la hereda de `[workspace.lints]`); sin `unwrap`/`expect`/indexado con `[]` en código que no sea de prueba (usar `get`, `?`, `let … else`); `cargo clippy -D warnings`; `cargo fmt`; versiones exactas `=x.y.z`; cero archivos Python o bash nuevos (los oráculos Python se pasan como texto a `python3 -c`, como en `crates/comandos-runtime/tests/hook_claude_parity.rs`).
4. **Bindings GTK sin `unsafe`**: todo lo que este plan necesita existe en la API segura de gtk-rs 0.18 / webkit2gtk-rs 2.0 (`DrawingArea::connect_draw`, `glib::source::unix_fd_add_local`, `UserContentManager::connect_script_message_received`, `WebsiteDataManager::builder`). Si una tarea encuentra un hueco que solo se cubre con `unsafe`, **se detiene** y lo informa al controlador con el símbolo exacto; no se añade `unsafe` ni `#[allow(unsafe_code)]` sin su decisión.
5. **Independencia para worktrees paralelos**: cada tarea declara de qué tareas depende; las de la 4b añaden su módulo y **una sola línea** de registro en `ui/app.rs` (sección «Registro de rebanadas») para que los merges no choquen.
6. **Nada de Xvfb, navegador local ni Playwright en esta máquina** (preferencia global de Jesús). Las pruebas GTK solo corren si `COMANDOS_GTK_TEST_DISPLAY` está definido (el controlador decide la pantalla); sin esa variable se saltan con aviso. Toda la lógica decisiva tiene prueba pura sin pantalla.

## Global Constraints

- Todo se compila y prueba con
  `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target nice -n 10 cargo <cmd> -j 6` (abreviado `$C <cmd>`). Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings` y las pruebas de los paquetes tocados.
- Edition 2024, `rust-version = "1.96"`, `unsafe_code = "forbid"` (workspace).
- Commits con `git add <rutas>` explícitas; mensajes `feat(app): …`, `feat(xtask): …`, `feat(install): …`, `docs(verification): …` en español, línea en blanco y `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. El plan: `git add -f docs/superpowers/plans/2026-10-04-fase-4-app-gtk.md`.
- Puertos: nunca 4777–4782 en pruebas. Los servidores de prueba (popups en sombra) usan la banda de `devhost` 7200–7399 (por omisión 7311). Nunca 3000, 5173, 8000, 8080 ni el rango efímero 32768–60999 para puertos fijos.
- Rutas reales que el modo **live** usa, idénticas a las del Python (`bin/cc-app` línea entre paréntesis): `~/.claude/hooks/app-tabs.json` (1900, con candado `app-tabs.json.lock` compartido con `cc-dash`), `app-tabs-history.json` (1901), `app-tabs-snapshot.json` (2469), `app-sessions-v2.json` (2634), `app-tab-active.json` (4942), `app-tab-models.json` (4943), `app-extension-shelf.json` (5448), `app-layout.json` (7492), `app-pane-position.json` (8013), `app-focus.json` (8745), `app-tab-close.json` (8780), `app-tab-open.json` (8805), `app-command.json` (8982), `snippets.json` (443), `acp-panes.json` (2764). WebKit: datos en `~/.local/share/comandos/` y caché en `~/.cache/comandos/` (el Python fija `GLib.set_prgname("comandos")`, línea 192).
- Ventana live: `WM_CLASS` `("comandos","comandos")`, título `ComandOS`, `prgname` `comandos`, `GDK_BACKEND=x11` (el `.desktop` ya lo exporta; la app lo fija con `gdk::set_allowed_backends("x11")` antes de `gtk::init`).
- Candado de instancia única live: `$XDG_RUNTIME_DIR/cc-app-<DISPLAY sin ':' ni '/'>.lock` (fallback `/tmp/comandos-<uid>/` y `/tmp`), el **mismo archivo** que el Python (líneas 43–73): si uno corre, el otro activa la ventana con `wmctrl -a ComandOS` y sale.
- Sombra: título `Sombra de la app (Rust)` y `WM_CLASS` `("sombra-app-rs","sombra-app-rs")`; **ninguno contiene `comandos`** sin distinguir mayúsculas, porque `cc-dash` y el Python enfocan con `wmctrl -x -a comandos` y `wmctrl -a ComandOS`, que comparan por subcadena.
- Idioma de la interfaz: `CC_LANG` de `~/.claude/hooks/cc-notify.conf`, si no `es` cuando `$LANG` empieza por `es` (`_ui_lang`, 177).

## Review Focus

1. **La sombra no escribe nunca en el tmux del usuario ni lo redimensiona.** Un cliente que se engancha a una sesión real con otro tamaño de ventana encoge la sesión para todos sus clientes (incidente «escribe sola», 1-oct). Esperado: los clientes de la sombra llevan `read-only,ignore-size`, el tamaño de la sesión no cambia, y cualquier verbo de tmux fuera de la lista de lectura devuelve `TmuxError::ShadowRefused`. Pruebas `shadow_attach_keeps_session_size` y `shadow_refuses_every_mutating_verb` (Tarea 3).
2. **Arranque con la pantalla bloqueada o antes de que GTK dé tamaño.** El terminal mide 1 px y un `attach` a 80×24 encoge la sesión. Esperado: el primer attach usa el tamaño que la sesión ya tiene en tmux y solo cambia al tamaño real cuando la asignación se estabiliza (250 ms sin cambios, tope 1 500 ms). Pruebas `settle_waits_for_quiet_allocation` y `attach_before_allocation_uses_tmux_size` (Tarea 5).
3. **Dos escritores sobre los archivos de estado** (Python y sombra a la vez; live y `cc-dash`). Esperado: en sombra `WriteGuard` rechaza toda escritura en `~/.claude/hooks`; en live `app-tabs.json` se escribe bajo `flock` de `app-tabs.json.lock` (el candado de `cc-dash`) con renombrado atómico; un arranque parcial nunca sobrescribe un snapshot completo. Pruebas `shadow_guard_refuses_hooks_writes`, `tabs_write_holds_shared_lock` (Tarea 9) y `no_snapshot_before_restore_finishes` (Tarea 11).
4. **Reanudación de conversaciones**: una pestaña muerta con Codex o Claude nunca debe reanudar otra conversación (`--last`, `--continue`) desde el snapshot por pane, y los flags `-c` de Codex son de configuración, no de continuar (cambio sin commitear de `_sane_flags`). Esperado: mismos comandos que el Python, byte a byte. Prueba diferencial `resume_matches_python_oracle` con casos Claude main/relotto, Codex con `-c`, Grok, ACP y snapshots rotos (Tarea 11).
5. **Texto no ASCII y método de entrada**: `ñandú 漢字 🚀` tecleado (incluido IME) y pegado llega intacto a tmux; los caracteres anchos ocupan dos celdas. Pruebas `ime_commit_reaches_pty` (GTK, Tarea 6) y `wide_cells_paint_once` (Tarea 4).

Además (cubiertos en sus tareas): `localStorage` del tablero conservado entre Python y Rust (Tarea 7, paso de sombra), presión de memoria (oomd) antes del cutover (Tarea 20), mensajes del puente malformados ignorados sin pánico (Tarea 13).

---

## Decisión de framework

**Decisión: gtk-rs GTK3 0.18 sin framework de arquitectura (sin relm), con WebKit2GTK 4.1 vía `webkit2gtk = "=2.0.2"`.** El estado de la app es un `Rc<App>` con `RefCell` por dominio y módulos puros probados aparte.

| Criterio | gtk4-rs 0.11 + relm4 0.11 | gtk4-rs 0.11 sin framework | **gtk-rs GTK3 0.18 sin framework** |
|---|---|---|---|
| WebKit embebible en Ubuntu 22.04 | **No**: el tablero necesita `webkitgtk-6.0`, que jammy no ofrece (solo hay `webkit2gtk-4.0/4.1`, ligados a GTK3) | **No**, mismo motivo | **Sí**: `libwebkit2gtk-4.1-0 2.50.4` instalada; `webkit2gtk 2.0.2` la envuelve |
| GTK disponible | 4.6.9 solo en ejecución, sin `-dev` | igual | 3.24.33 con `-dev` |
| RSS de una ventana vacía (orden de magnitud, a medir en la Tarea 7) | mayor: el renderizador GL de GSK reserva contexto y texturas | igual que la columna izquierda | menor: cairo por software, sin contexto GL |
| Arranque | inicialización GL añadida | igual | sin GL |
| Riesgo de bindings | relm4 añade macros y su propio ciclo de vida encima de gtk4-rs | mantenido activamente | gtk-rs GTK3 está marcado sin mantenimiento (avisos RustSec `RUSTSEC-2024-0411` a `-0420` sobre `gtk`, `gdk`, `atk`, `gtk-sys`… y `RUSTSEC-2024-0429` sobre `glib::VariantStrIter` en glib 0.18, que este código no usa); la API es estable y el único binding delicado es WebKit (spec §8) |
| relm4 para GTK3 | no existe (relm4 solo soporta GTK4; el `relm` para GTK3 está abandonado) | — | — |
| Coste de migrar después | — | — | acotado: la capa GTK es fina y la lógica pura no depende del toolkit |

Notas:
- Existe `gtk 0.19.0` (GTK3 sobre glib 0.22), pero `webkit2gtk 2.0.2` exige `gtk ^0.18` y `javascriptcore-rs =1.1`; mezclar glib 0.18 y 0.22 en el mismo proceso no es posible con tipos compartidos. Se queda en 0.18 hasta que haya un `webkit2gtk` sobre glib 0.22; cambiar de versión es una tarea aparte.
- Con Ubuntu 24.04 (GTK 4.14 + `webkitgtk-6.0`) se podría reescribir la capa GTK sobre gtk4-rs; los módulos puros no cambian. No es parte de esta fase.
- La Tarea 7 es la **puerta**: si la app mínima no alcanza el presupuesto de RSS o el puente de WebKit falla, se para y se informa antes del port completo (spec §8).

## Blanco móvil: `bin/cc-app` cambia mientras se porta

Otras sesiones editan `bin/cc-app` en el checkout principal: al escribir este plan, `git diff --stat bin/cc-app` daba 50 inserciones y 17 borrados sin commitear (`_sane_flags`/`_proc_flags` con parámetro `agent` para Codex, `_pane_card_keys` sin el botón «⚙ IA», `_account_popover` con «+ Añadir cuenta» por `POST /account/add`, identidad por cuenta y filas que llevan a iniciar sesión). Reglas:

1. La Tarea 2 crea `cargo xtask app-drift`, que guarda un hash por función (`def`/`class`, anidadas incluidas) de `bin/cc-app` y `bin/cc-notifyd` en `docs/verification/app-drift-baseline.json`.
2. **Cada tarea de port empieza** con `cargo xtask app-drift /home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` (lectura del checkout principal, sin tocarlo). Las funciones de su alcance que salgan como `changed`/`added` se portan en su versión nueva; la tarea lo anota en su commit y actualiza la línea base solo para esas funciones (`--accept <nombre>…`).
3. Los oráculos Python leen `COMANDOS_CC_APP_ORACLE` si está definido (ruta al `bin/cc-app` del checkout principal) y si no el `bin/cc-app` del worktree.
4. La Tarea 20 (cutover) exige `app-drift` vacío contra el checkout principal el mismo día del cambio.

## Interfaz que se asume de `comandos-term` (Fase 3)

La Fase 3 entrega `crates/comandos-term` (modelo VT sobre `alacritty_terminal` 0.26 + renderizador canvas para la web). Esta fase consume **solo** el modelo y la codificación de entrada, a través del adaptador `crates/comandos-app/src/term/engine.rs` (Tarea 4). Lo que se asume:

```rust
// comandos_term (Fase 3): superficie que usa el escritorio.
pub struct GridSize { pub cols: u16, pub rows: u16 }
pub struct TermConfig { pub scrollback: usize }
pub struct Terminal { /* alacritty_terminal::Term + vte::Processor<SyncTimer> */ }
impl Terminal {
    pub fn new(size: GridSize, config: TermConfig) -> Self;
    pub fn advance(&mut self, bytes: &[u8]);
    pub fn resize(&mut self, size: GridSize);
    pub fn take_events(&mut self) -> Vec<TermEvent>;   // respuestas al PTY, portapapeles, título, timbre
    pub fn damaged_rows(&mut self) -> Damage;          // Full | Rows(Vec<u16>)
    pub fn line(&self, row: u16) -> Vec<CellView>;     // celdas visibles de la fila
    pub fn cursor(&self) -> CursorView;
    pub fn modes(&self) -> Modes;
    pub fn scroll_display(&mut self, lines: i32);
    pub fn sync_deadline_ms(&self) -> Option<u64>;     // fin de CSI ? 2026 h
    pub fn sync_tick(&mut self, now_ms: u64);
    pub fn selection_begin(&mut self, row: u16, col: u16);
    pub fn selection_extend(&mut self, row: u16, col: u16);
    pub fn selection_text(&self) -> Option<String>;
    pub fn selection_clear(&mut self);
}
pub enum TermEvent { PtyWrite(Vec<u8>), ClipboardStore(String), Title(String), Bell }
pub struct CellView { pub ch: char, pub zerowidth: Vec<char>, pub fg: TermColor, pub bg: TermColor,
                      pub flags: CellFlags, pub wide: bool, pub spacer: bool, pub hyperlink: Option<String> }
pub enum TermColor { Foreground, Background, Named(u8), Indexed(u8), Rgb(u8, u8, u8) }
pub struct CellFlags { pub bold: bool, pub dim: bool, pub italic: bool, pub underline: bool,
                       pub double_underline: bool, pub strike: bool, pub inverse: bool, pub hidden: bool }
pub struct CursorView { pub row: u16, pub col: u16, pub visible: bool, pub shape: CursorShape }
pub enum CursorShape { Block, Beam, Underline }
pub struct Modes { pub mouse_report: bool, pub mouse_motion: bool, pub mouse_drag: bool, pub sgr_mouse: bool,
                   pub alt_screen: bool, pub alternate_scroll: bool, pub bracketed_paste: bool,
                   pub app_cursor: bool, pub focus_report: bool }
```

Reglas si la Fase 3 entrega otra cosa:
- **Nombres o firmas distintos**: el adaptador `term/engine.rs` traduce; el resto de `comandos-app` solo ve `TermEngine` (Tarea 4). No se modifica `comandos-term` desde esta fase.
- **`comandos-term` no existe todavía**: `TermEngine` se implementa directamente sobre `alacritty_terminal = "=0.26.0"` (compila sin parche en Linux nativo; el parche del spike era solo para wasm) con el temporizador de sincronización propio del spike (`vte::ansi::Timeout` sobre `std::time::Instant`, que en nativo no entra en pánico). Cuando la Fase 3 aterrice, una tarea aparte cambia el adaptador.
- **La codificación de teclado/ratón**: si la Fase 3 no exporta `input::{encode_key, encode_mouse, encode_paste}`, la Tarea 6 usa `term/keys.rs` y `term/mouse.rs` de este plan (código completo abajo).
- El renderizador cairo, el widget GTK y el PTY de escritorio son de esta fase (spec §4.4: «cairo … en GTK»), no de `comandos-term`.

## Presupuesto y línea base de RSS

Medido el 2026-10-04 con `ps -o rss` (solo lectura) sobre la app viva, 22,5 h de actividad:

| Proceso | RSS hoy | Hilos | Objetivo Rust | Aceptación |
|---|---|---|---|---|
| `cc-app` (python3) → `comandos-app` | 130 MB | 84 | ≤ 60 MB con 20 pestañas adjuntas tras 1 h | ≤ 130 MB |
| `WebKitWebProcess` (tablero) | 146 MB | 63 | ≤ 146 MB (mismo motor 2.50) | ≤ 146 MB |
| `WebKitNetworkProcess` | 885 MB | 12 | ≤ 250 MB tras el experimento de la Tarea 20 | ≤ 885 MB |
| `cc-notifyd` (python3) → `comandos-app` en modo popups | 46 MB | 6 | ≤ 15 MB en reposo | ≤ 46 MB |
| Arranque a ventana visible | sin medir | — | ≤ 600 ms | medido y anotado |

Las mediciones de Rust se guardan con `cargo xtask rss` en `docs/verification/rss.jsonl` (formato existente) y se resumen en `docs/verification/cutover-app.md`.

## Estructura de archivos

```
Cargo.toml                                    (miembro crates/comandos-app)                        T1
crates/comandos-app/Cargo.toml                                                                      T1
crates/comandos-app/src/main.rs               (despacho argv[0])                                    T1
crates/comandos-app/src/lib.rs                                                                      T1
crates/comandos-app/src/config.rs             (RunMode, AppConfig, parse_args, ui_lang)            T1
crates/comandos-app/src/guard.rs              (WriteGuard)                                          T3
crates/comandos-app/src/tmux.rs               (TmuxCtl, TmuxError, READ_VERBS)                      T3
crates/comandos-app/src/jobs.rs               (hilo de trabajo → bucle de GLib)                     T3
crates/comandos-app/src/term/engine.rs        (TermEngine sobre comandos-term)                      T4
crates/comandos-app/src/term/paint.rs         (Palette, resolve, runs)                              T4
crates/comandos-app/src/term/boxdraw.rs       (U+2500–U+259F dibujados en código)                   T4
crates/comandos-app/src/term/settle.rs        (Settle)                                              T5
crates/comandos-app/src/term/pty.rs           (PtySession)                                          T5
crates/comandos-app/src/term/keys.rs          (gdk → bytes)                                         T6
crates/comandos-app/src/term/mouse.rs         (SGR / X10)                                           T6
crates/comandos-app/src/term/links.rs         (url_from_wrapped_text, OSC 8)                        T6
crates/comandos-app/src/term/view.rs          (TermView: DrawingArea + PTY + entrada)               T6
crates/comandos-app/src/ui/app.rs             (App, run, «Registro de rebanadas»)                   T7
crates/comandos-app/src/ui/window.rs          (ventana, cabecera, Paned)                            T7
crates/comandos-app/src/ui/webview.rs         (tablero, datos por modo, puente «centro»)            T7
crates/comandos-app/src/layout_dump.rs        (LayoutDump)                                          T7
crates/comandos-app/src/dash_client.rs        (DashClient)                                          T8
crates/comandos-app/src/poll.rs               (Poller, PollUpdate)                                  T8
crates/comandos-app/src/theme.rs              (THEMES, theme_css, header_css, terminal prefs)       T8
crates/comandos-app/src/state_files.rs        (lectura/escritura con flock)                         T9
crates/comandos-app/src/ui/tabs.rs            (TabRegistry puro + TabStrip GTK)                     T9
crates/comandos-app/src/ui/tab_label.rs                                                             T9
crates/comandos-app/src/ui/confirm.rs         (diálogo que solo cierra con Sí/No)                   T9
crates/comandos-app/src/ipc.rs                (IpcRequest, monitores de archivos)                   T9
crates/comandos-app/src/ui/workspace.rs       (grupos del /workspace, arrastre y acople)            T10
crates/comandos-app/src/resume.rs             (sane_flags, resume_command, exact_resume_command)    T11
crates/comandos-app/src/snapshot.rs           (snapshot de pestañas y de layouts)                   T11
crates/comandos-app/src/restore.rs            (RestorePlan y ejecutor)                              T11
crates/comandos-app/src/ui/{bridge,header,keys,switcher,menu,clipboard,overlays,accounts,
  extensions,side,mosaic,reader,modals,snippets,help,hourglass,presence,app_commands}.rs     T13–T18
crates/comandos-app/src/notifyd/{mod,server,popup,markdown,stack}.rs                                T19
crates/comandos-app/tests/support/{mod,oracle,tmux}.rs                                              T3, T11
crates/comandos-app/tests/*.rs, crates/comandos-app/tests/gtk_smoke.rs (harness = false)            T3–T19
xtask/src/app_drift.rs, xtask/src/app_layout.rs, xtask/src/main.rs                                  T2, T12
crates/comandos-cli/src/install.rs, crates/comandos-cli/src/install/release.rs (binario app)        T20
docs/verification/app-drift-baseline.json, docs/verification/cutover-app.md                         T2, T12, T20
```

Orden y paralelismo (dependencias entre paréntesis):

- **4a — cimientos y app mínima**: T1 → {T2, T3, T4, T8} en paralelo → T5 (T3, T4) → T6 (T4, T5) → T7 (T3, T6, T8) → T9 (T7) → {T10, T11} en paralelo (T9) → T12 (T9, T10, T11).
- **4b — paridad por rebanadas**, todas después de T12 y entre sí en paralelo salvo lo indicado: T13, T14 (T13), T15, T16, T17, T18 (T13); T19 solo depende de T1 y del módulo `theme` de T8; T20 al final.

---

## 4a — Cimientos y app mínima

### Task 1: crate `comandos-app`, versiones fijadas, despacho y modos

**Depende de:** nada. **Paralelizable:** no (las demás lo necesitan).

**Files:**
- Modify: `Cargo.toml` (miembro nuevo)
- Create: `crates/comandos-app/Cargo.toml`, `crates/comandos-app/src/{main,lib,config}.rs`
- Test: `crates/comandos-app/tests/config.rs`

**Interfaces:**
- Produces: `comandos_app::config::{RunMode, AppConfig, parse_args, ui_lang, resolve_entry, Entry}`; `AppConfig::{lock_file_name, wm_class, title, layout_dump_path, writes_allowed}`.

- [ ] **Step 1: comprobar el entorno y las versiones**

Run:
```bash
pkg-config --modversion gtk+-3.0 webkit2gtk-4.1 javascriptcoregtk-4.1 libsoup-3.0
```
Expected: `3.24.33` y tres versiones. Si `webkit2gtk-4.1` falta, parar y pedir a Jesús `sudo apt install libwebkit2gtk-4.1-dev` (no seguir sin ese paquete).

Run (lectura del índice; confirma que las versiones del Tech Stack siguen siendo las últimas de su serie):
```bash
for c in gtk gdk glib gio cairo-rs pango pangocairo gdk-pixbuf webkit2gtk javascriptcore-rs pty-process async-channel ureq; do cargo info "$c" 2>/dev/null | sed -n 's/^version: //p' | head -1 | sed "s/^/$c /"; done
```
Si una serie 0.18 / 2.0 / 1.1 tiene un parche más nuevo que el de la tabla, se fija ese parche y se anota en el commit. No se sube de serie.

- [ ] **Step 2: escribir la prueba que falla**

`crates/comandos-app/tests/config.rs`:
```rust
use comandos_app::config::{parse_args, resolve_entry, AppConfig, Entry, RunMode};
use std::path::PathBuf;

fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |k| pairs.iter().find(|(a, _)| *a == k).map(|(_, v)| v.to_string())
}

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn entry_follows_argv0() {
    assert_eq!(resolve_entry("/home/u/.local/bin/cc-app"), Entry::App { default_live: true });
    assert_eq!(resolve_entry("comandos-app"), Entry::App { default_live: false });
    assert_eq!(resolve_entry("/x/cc-notifyd"), Entry::Notifyd);
    assert_eq!(resolve_entry("comandos-notifyd"), Entry::Notifyd);
}

#[test]
fn bare_comandos_app_is_sandbox() {
    let env = env_of(&[("HOME", "/h"), ("XDG_RUNTIME_DIR", "/run/user/1000"), ("DISPLAY", ":1")]);
    let cfg = parse_args(&args(&[]), false, &env).expect("config");
    assert_eq!(cfg.mode, RunMode::Sandbox);
    assert_eq!(cfg.tmux_socket.as_deref(), Some("comandos-app-sbx"));
    assert_eq!(cfg.hooks_dir, PathBuf::from("/run/user/1000/comandos-app-sbx/hooks"));
    assert_eq!(cfg.dash_url, None);
    assert!(cfg.writes_allowed());
}

#[test]
fn cc_app_defaults_to_live_with_real_paths() {
    let env = env_of(&[("HOME", "/h"), ("XDG_RUNTIME_DIR", "/run/user/1000"), ("DISPLAY", ":1")]);
    let cfg = parse_args(&args(&[]), true, &env).expect("config");
    assert_eq!(cfg.mode, RunMode::Live);
    assert_eq!(cfg.tmux_socket, None);
    assert_eq!(cfg.hooks_dir, PathBuf::from("/h/.claude/hooks"));
    assert_eq!(cfg.dash_url.as_deref(), Some("http://127.0.0.1:4777"));
    assert_eq!(cfg.web_data_dir, PathBuf::from("/h/.local/share/comandos"));
    assert_eq!(cfg.web_cache_dir, PathBuf::from("/h/.cache/comandos"));
    assert_eq!(cfg.lock_file_name(":1"), "cc-app-1.lock");
    assert_eq!(cfg.wm_class(), "comandos");
    assert_eq!(cfg.title(), "ComandOS");
}

#[test]
fn shadow_never_looks_like_the_real_app() {
    let env = env_of(&[("HOME", "/h"), ("XDG_RUNTIME_DIR", "/run/user/1000")]);
    let cfg: AppConfig = parse_args(&args(&["--mode", "shadow"]), false, &env).expect("config");
    assert_eq!(cfg.mode, RunMode::Shadow);
    assert!(!cfg.writes_allowed());
    for name in [cfg.title(), cfg.wm_class()] {
        assert!(!name.to_lowercase().contains("comandos"), "{name}");
    }
    assert_eq!(cfg.lock_file_name(":1"), "sombra-app-rs-1.lock");
    assert_eq!(cfg.web_data_dir, PathBuf::from("/run/user/1000/comandos-app-shadow/data"));
    assert_eq!(cfg.layout_dump_path(), PathBuf::from("/run/user/1000/comandos-app-shadow-layout.json"));
}

#[test]
fn dash_url_must_be_loopback() {
    let env = env_of(&[("HOME", "/h")]);
    let err = parse_args(&args(&["--dash-url", "http://example.com:4777"]), false, &env).unwrap_err();
    assert!(err.contains("127.0.0.1"), "{err}");
}

#[test]
fn unknown_flag_is_usage_error() {
    let env = env_of(&[("HOME", "/h")]);
    assert!(parse_args(&args(&["--modo", "live"]), false, &env).is_err());
}
```

- [ ] **Step 3: correrla y ver que falla**

Run: `$C test -p comandos-app --test config`
Expected: FAIL (`comandos-app` no existe).

- [ ] **Step 4: crear el crate**

`Cargo.toml` (raíz): añadir `"crates/comandos-app"` a `members` (antes de `"xtask"`).

`crates/comandos-app/Cargo.toml`:
```toml
[package]
name = "comandos-app"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[lints]
workspace = true

[[bin]]
name = "comandos-app"
path = "src/main.rs"

[[test]]
name = "gtk_smoke"
harness = false

[dependencies]
comandos-core = { path = "../comandos-core" }
comandos-runtime = { path = "../comandos-runtime" }
serde_json.workspace = true
sha2.workspace = true
gtk = { version = "=0.18.2", features = ["v3_24"] }
gdk = "=0.18.2"
glib = "=0.18.5"
gio = "=0.18.4"
cairo-rs = "=0.18.5"
pango = "=0.18.3"
pangocairo = "=0.18.0"
gdk-pixbuf = "=0.18.5"
webkit2gtk = { version = "=2.0.2", features = ["v2_40"] }
javascriptcore-rs = "=1.1.2"
pty-process = "=0.5.3"
async-channel = "=2.5.0"
ureq = { version = "=3.4.2", default-features = false }
regex = "=1.13.1"
nix = { version = "=0.31.3", default-features = false, features = ["fs", "signal", "process"] }
```
(`comandos-term = { path = "../comandos-term" }` se añade en la Tarea 4; `alacritty_terminal = "=0.26.0"` solo si la Fase 3 no existe.)

`crates/comandos-app/src/lib.rs`:
```rust
//! Escritorio GTK de ComandOS: tablero WebKit, pestañas de terminal sobre tmux y popups.
pub mod config;
```

`crates/comandos-app/src/config.rs`:
```rust
//! Modo de ejecución y rutas. El modo decide qué puede escribir la app: solo
//! `Live` toca el estado real; `Shadow` mira el estado real sin escribir nada.
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// tmux propio (`-L`), hooks temporales: desarrollo y pruebas.
    Sandbox,
    /// tmux del usuario con clientes de solo lectura; ninguna escritura.
    Shadow,
    /// Sustituto real de `cc-app`.
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    App { default_live: bool },
    Notifyd,
}

/// `cc-app` (el symlink del cutover) arranca en live; `comandos-app` a secas, en sandbox.
pub fn resolve_entry(argv0: &str) -> Entry {
    let name = argv0.rsplit('/').next().unwrap_or(argv0);
    match name {
        "cc-notifyd" | "comandos-notifyd" => Entry::Notifyd,
        "cc-app" => Entry::App { default_live: true },
        _ => Entry::App { default_live: false },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub mode: RunMode,
    pub home: PathBuf,
    pub hooks_dir: PathBuf,
    pub tmux_socket: Option<String>,
    pub dash_url: Option<String>,
    pub web_data_dir: PathBuf,
    pub web_cache_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub repo_root: Option<PathBuf>,
}

const DEFAULT_DASH: &str = "http://127.0.0.1:4777";
const USAGE: &str = "uso: comandos-app [--mode sandbox|shadow|live] [--tmux-socket NOMBRE] \
                     [--hooks-dir DIR] [--dash-url http://127.0.0.1:PUERTO] [--repo DIR]";

impl AppConfig {
    pub fn writes_allowed(&self) -> bool {
        self.mode != RunMode::Shadow
    }

    /// Mismo nombre que el Python en live (línea 43): los dos se excluyen.
    pub fn lock_file_name(&self, display: &str) -> String {
        let d: String = display.chars().filter(|c| *c != ':' && *c != '/').collect();
        let d = if d.is_empty() { "x".to_string() } else { d };
        match self.mode {
            RunMode::Live => format!("cc-app-{d}.lock"),
            RunMode::Shadow => format!("sombra-app-rs-{d}.lock"),
            RunMode::Sandbox => format!("comandos-app-sbx-{d}.lock"),
        }
    }

    pub fn wm_class(&self) -> &'static str {
        match self.mode {
            RunMode::Live => "comandos",
            RunMode::Shadow => "sombra-app-rs",
            RunMode::Sandbox => "sandbox-app-rs",
        }
    }

    pub fn title(&self) -> &'static str {
        match self.mode {
            RunMode::Live => "ComandOS",
            RunMode::Shadow => "Sombra de la app (Rust)",
            RunMode::Sandbox => "Sandbox de la app (Rust)",
        }
    }

    pub fn layout_dump_path(&self) -> PathBuf {
        let name = match self.mode {
            RunMode::Live => "comandos-app-layout.json",
            RunMode::Shadow => "comandos-app-shadow-layout.json",
            RunMode::Sandbox => "comandos-app-sbx-layout.json",
        };
        self.runtime_dir.join(name)
    }
}

fn loopback_only(url: &str) -> Result<String, String> {
    let rest = url
        .strip_prefix("http://127.0.0.1:")
        .ok_or_else(|| format!("--dash-url solo acepta http://127.0.0.1:PUERTO, no {url}"))?;
    let port = rest.trim_end_matches('/');
    if port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("--dash-url con puerto inválido: {url}"));
    }
    Ok(format!("http://127.0.0.1:{port}"))
}

/// `COMANDOS_APP_REPO`, o el destino de `<hooks>/dash/index.html` dos niveles arriba
/// (mismo criterio que `comandos dash`).
fn repo_root(hooks: &Path, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(raw) = env("COMANDOS_APP_REPO").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(raw));
    }
    let index = std::fs::canonicalize(hooks.join("dash/index.html")).ok()?;
    index.parent()?.parent().map(Path::to_path_buf)
}

pub fn parse_args(
    args: &[String],
    default_live: bool,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<AppConfig, String> {
    let home = env("HOME").filter(|h| !h.is_empty()).map(PathBuf::from).ok_or("HOME no está definido")?;
    let uid = nix::unistd::getuid();
    let runtime_dir = env("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/comandos-{uid}")));
    let mut mode = if default_live { RunMode::Live } else { RunMode::Sandbox };
    let (mut socket, mut hooks, mut dash, mut repo) = (None, None, None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} necesita valor\n{USAGE}"));
        match arg.as_str() {
            "--mode" => {
                mode = match value()?.as_str() {
                    "sandbox" => RunMode::Sandbox,
                    "shadow" => RunMode::Shadow,
                    "live" => RunMode::Live,
                    other => return Err(format!("modo desconocido: {other}\n{USAGE}")),
                }
            }
            "--tmux-socket" => socket = Some(value()?),
            "--hooks-dir" => hooks = Some(PathBuf::from(value()?)),
            "--dash-url" => dash = Some(loopback_only(&value()?)?),
            "--repo" => repo = Some(PathBuf::from(value()?)),
            other => return Err(format!("opción desconocida: {other}\n{USAGE}")),
        }
    }
    let real_hooks = home.join(".claude/hooks");
    let (hooks_dir, tmux_socket, dash_url, web_data_dir, web_cache_dir) = match mode {
        RunMode::Sandbox => {
            let base = runtime_dir.join("comandos-app-sbx");
            (
                hooks.unwrap_or_else(|| base.join("hooks")),
                Some(socket.unwrap_or_else(|| "comandos-app-sbx".into())),
                dash,
                base.join("data"),
                base.join("cache"),
            )
        }
        RunMode::Shadow => {
            let base = runtime_dir.join("comandos-app-shadow");
            let default = env("COMANDOS_DASH_URL").map(|u| loopback_only(&u)).transpose()?;
            (
                hooks.unwrap_or(real_hooks),
                socket,
                Some(dash.or(default).unwrap_or_else(|| DEFAULT_DASH.into())),
                base.join("data"),
                base.join("cache"),
            )
        }
        RunMode::Live => {
            let default = env("COMANDOS_DASH_URL").map(|u| loopback_only(&u)).transpose()?;
            (
                hooks.unwrap_or(real_hooks),
                socket,
                Some(dash.or(default).unwrap_or_else(|| DEFAULT_DASH.into())),
                home.join(".local/share/comandos"),
                home.join(".cache/comandos"),
            )
        }
    };
    let repo_root = repo.or_else(|| repo_root(&hooks_dir, env));
    Ok(AppConfig { mode, home, hooks_dir, tmux_socket, dash_url, web_data_dir, web_cache_dir, runtime_dir, repo_root })
}

/// `_ui_lang` (línea 177): `CC_LANG` de cc-notify.conf; si no, `es` cuando `$LANG` empieza por `es`.
pub fn ui_lang(hooks_dir: &Path, lang_env: Option<&str>) -> &'static str {
    let conf = std::fs::read_to_string(hooks_dir.join("cc-notify.conf")).unwrap_or_default();
    for line in conf.lines() {
        if let Some(v) = line.trim().strip_prefix("CC_LANG=") {
            match v.trim().trim_matches(|c| c == '"' || c == '\'') {
                "es" => return "es",
                "en" => return "en",
                _ => {}
            }
        }
    }
    if lang_env.is_some_and(|l| l.to_lowercase().starts_with("es")) { "es" } else { "en" }
}
```

`crates/comandos-app/src/main.rs`:
```rust
use comandos_app::config::{resolve_entry, Entry};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut argv = std::env::args();
    let argv0 = argv.next().unwrap_or_default();
    let rest: Vec<String> = argv.collect();
    if rest.first().is_some_and(|a| a == "--version") {
        println!("comandos-app {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    match resolve_entry(&argv0) {
        // T7 sustituye este brazo por `comandos_app::ui::app::run(&rest, default_live)`.
        Entry::App { .. } => {
            eprintln!("comandos-app: interfaz pendiente (Tarea 7)");
            ExitCode::from(2)
        }
        // T19 sustituye este brazo por `comandos_app::notifyd::run(&rest)`.
        Entry::Notifyd => {
            eprintln!("comandos-app: popups pendientes (Tarea 19)");
            ExitCode::from(2)
        }
    }
}
```

- [ ] **Step 5: correr pruebas, clippy y comprobar el binario**

Run: `$C test -p comandos-app --test config && $C clippy -p comandos-app --all-targets -j 6 -- -D warnings && $C run -q -p comandos-app -- --version`
Expected: 6 pruebas PASS, clippy limpio, `comandos-app 0.1.0`. La primera compilación de GTK/WebKit tarda; si `webkit2gtk-sys` no encuentra `webkit2gtk-4.1`, volver al Step 1.

Anotar en el mensaje del commit los avisos que dé `cargo audit` o `cargo deny check advisories` si alguno está instalado (`command -v cargo-audit cargo-deny`); si ninguno está, decirlo.

- [ ] **Step 6: commit**

```bash
git add Cargo.toml Cargo.lock crates/comandos-app/Cargo.toml crates/comandos-app/src/main.rs crates/comandos-app/src/lib.rs crates/comandos-app/src/config.rs crates/comandos-app/tests/config.rs
git commit -m "feat(app): crate comandos-app con modos sandbox/sombra/live y despacho por argv[0]

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 2: `cargo xtask app-drift` — hash por función de `bin/cc-app`

**Depende de:** nada (solo `xtask`). **Paralelizable:** sí, con T3, T4, T8.

**Files:**
- Create: `xtask/src/app_drift.rs`
- Modify: `xtask/src/main.rs` (subcomando `app-drift`)
- Create: `docs/verification/app-drift-baseline.json`
- Test: pruebas unitarias en `xtask/src/app_drift.rs`

**Interfaces:**
- Produces: `cargo xtask app-drift [--write-baseline] [--accept NOMBRE…] [--baseline RUTA] RUTA_CC_APP [RUTA_CC_NOTIFYD]`; `app_drift::{defs(src: &str) -> Vec<Def>, Def { qualname: String, hash: String }, diff(base, now) -> Drift }`.

- [ ] **Step 1: prueba que falla**

Al final de `xtask/src/app_drift.rs` (archivo nuevo con solo el módulo de pruebas para empezar):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "import os\n\ndef a(x):\n    return x\n\n\nclass K:\n    def m(self):\n        # comentario\n        return 1\n\n    def n(self):\n        pass\n\ndef b():\n    def inner():\n        pass\n    return inner\n";

    #[test]
    fn qualified_names_include_nested() {
        let names: Vec<String> = defs(SRC).into_iter().map(|d| d.qualname).collect();
        assert_eq!(names, ["a", "K", "K.m", "K.n", "b", "b.inner"]);
    }

    #[test]
    fn body_change_changes_only_that_def() {
        let base = defs(SRC);
        let now = defs(&SRC.replace("return 1", "return 2"));
        let d = diff(&base, &now);
        assert_eq!(d.changed, ["K", "K.m"]);
        assert!(d.added.is_empty() && d.removed.is_empty());
    }

    #[test]
    fn comment_only_change_is_ignored() {
        let d = diff(&defs(SRC), &defs(&SRC.replace("# comentario", "# otro")));
        assert!(d.changed.is_empty());
    }
}
```

- [ ] **Step 2: correrla**

Run: `$C test -p xtask app_drift`
Expected: FAIL (`defs` no existe).

- [ ] **Step 3: implementar**

`xtask/src/app_drift.rs` (encima del módulo de pruebas):
```rust
//! Deriva de `bin/cc-app`: hash por función (incluidas las anidadas) para saber qué
//! cambió en el Python desde que se portó cada rebanada.
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Def {
    pub qualname: String,
    pub hash: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Drift {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Nombre tras `def `/`class ` si la línea abre una definición.
fn opener(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let rest = t.strip_prefix("def ").or_else(|| t.strip_prefix("async def ")).or_else(|| t.strip_prefix("class "))?;
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    rest.get(..end)
}

/// Cuerpo normalizado: sin líneas vacías ni comentarios de línea completa.
fn normalized(lines: &[&str]) -> String {
    lines
        .iter()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn defs(src: &str) -> Vec<Def> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(name) = opener(line) else { continue };
        let ind = indent_of(line);
        while stack.last().is_some_and(|(d, _)| *d >= ind) {
            stack.pop();
        }
        let qual = stack.iter().map(|(_, n)| n.as_str()).chain([name]).collect::<Vec<_>>().join(".");
        // El cuerpo acaba en la primera línea con código a sangría <= la de la definición.
        let end = lines
            .iter()
            .enumerate()
            .skip(i + 1)
            .find(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#') && indent_of(l) <= ind)
            .map_or(lines.len(), |(j, _)| j);
        let body = lines.get(i..end).unwrap_or_default();
        let hash = Sha256::digest(normalized(body).as_bytes());
        out.push(Def { qualname: qual.clone(), hash: format!("{hash:x}").chars().take(12).collect() });
        stack.push((ind, name.to_string()));
    }
    out
}

pub fn diff(base: &[Def], now: &[Def]) -> Drift {
    let find = |set: &[Def], q: &str| set.iter().find(|d| d.qualname == q).map(|d| d.hash.clone());
    let mut d = Drift::default();
    for n in now {
        match find(base, &n.qualname) {
            None => d.added.push(n.qualname.clone()),
            Some(h) if h != n.hash => d.changed.push(n.qualname.clone()),
            Some(_) => {}
        }
    }
    for b in base {
        if find(now, &b.qualname).is_none() {
            d.removed.push(b.qualname.clone());
        }
    }
    d
}

fn to_json(file: &str, defs: &[Def]) -> Value {
    let mut m = Map::new();
    for d in defs {
        m.insert(d.qualname.clone(), Value::String(d.hash.clone()));
    }
    json!({ "file": file, "defs": m })
}

fn from_json(v: &Value) -> Vec<Def> {
    v.get("defs")
        .and_then(Value::as_object)
        .map(|m| m.iter().filter_map(|(k, h)| Some(Def { qualname: k.clone(), hash: h.as_str()?.to_string() })).collect())
        .unwrap_or_default()
}

/// `paths`: `[cc-app, cc-notifyd?]`. Devuelve el código de salida (1 si hay deriva).
pub fn run(baseline: &Path, paths: &[String], write: bool, accept: &[String]) -> Result<i32, String> {
    let mut files = Map::new();
    let previous: Value = fs::read_to_string(baseline).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let mut drifted = false;
    for path in paths {
        let src = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let key = Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or("archivo").to_string();
        let now = defs(&src);
        let base = previous.get("files").and_then(|f| f.get(&key)).map(from_json).unwrap_or_default();
        let d = diff(&base, &now);
        for (label, list) in [("added", &d.added), ("removed", &d.removed), ("changed", &d.changed)] {
            for q in list {
                println!("{key} {label} {q}");
            }
        }
        drifted |= !(d.added.is_empty() && d.removed.is_empty() && d.changed.is_empty());
        // Solo se acepta lo pedido: el resto conserva el hash anterior.
        let merged: Vec<Def> = if write {
            now
        } else {
            now.into_iter()
                .map(|n| {
                    if accept.contains(&n.qualname) {
                        n
                    } else {
                        base.iter().find(|b| b.qualname == n.qualname).cloned().unwrap_or(n)
                    }
                })
                .collect()
        };
        files.insert(key, to_json(path, &merged));
    }
    if write || !accept.is_empty() {
        let text = serde_json::to_string_pretty(&json!({ "files": files })).map_err(|e| e.to_string())?;
        fs::write(baseline, text + "\n").map_err(|e| format!("{}: {e}", baseline.display()))?;
    }
    Ok(if drifted && !write { 1 } else { 0 })
}
```

En `xtask/src/main.rs`, declarar `mod app_drift;` y añadir el brazo (seguir el estilo de `parity`/`poll`):
```rust
Some("app-drift") => {
    let rest: Vec<String> = args.collect();
    let mut baseline = std::path::PathBuf::from("docs/verification/app-drift-baseline.json");
    let (mut write, mut accept, mut paths) = (false, Vec::new(), Vec::new());
    let mut it = rest.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--write-baseline" => write = true,
            "--baseline" => baseline = it.next().map(Into::into).unwrap_or(baseline),
            "--accept" => accept.extend(it.next()),
            _ => paths.push(a),
        }
    }
    if paths.is_empty() {
        fail("uso: cargo xtask app-drift [--write-baseline] [--accept NOMBRE] [--baseline RUTA] CC_APP [CC_NOTIFYD]".into());
    }
    match app_drift::run(&baseline, &paths, write, &accept) {
        Ok(code) => exit(code),
        Err(e) => fail(e),
    }
}
```
(El nombre exacto del iterador `args` y de `fail` son los de `xtask/src/main.rs`; si difieren, usar los reales.)

- [ ] **Step 4: pruebas y línea base**

Run: `$C test -p xtask app_drift`
Expected: 3 PASS.

Run (línea base desde lo commiteado del worktree; el checkout principal se compara después):
```bash
cargo xtask app-drift --write-baseline bin/cc-app bin/cc-notifyd
cargo xtask app-drift /home/someguy/codebase/0xJesus/ComandOS/bin/cc-app /home/someguy/codebase/0xJesus/ComandOS/bin/cc-notifyd
```
Expected: el segundo comando lista como `changed` al menos `_sane_flags`, `_proc_flags`, `_codex_info_for_session`, `resume_command`, `_pane_card_keys`, `_account_popover` (el diff sin commitear) y sale con 1. Copiar esa salida al mensaje del commit.

- [ ] **Step 5: commit**

```bash
git add xtask/src/app_drift.rs xtask/src/main.rs
git add -f docs/verification/app-drift-baseline.json
git commit -m "feat(xtask): app-drift, hash por función de cc-app y cc-notifyd para portar un blanco móvil

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 3: tmux con guardia de modo, escrituras vigiladas y trabajos fuera del hilo de interfaz

**Depende de:** T1. **Paralelizable:** sí, con T2, T4, T8.

**Files:**
- Create: `crates/comandos-app/src/{tmux,guard,jobs}.rs`
- Modify: `crates/comandos-app/src/lib.rs`
- Create: `crates/comandos-app/tests/support/{mod,tmux}.rs`, `crates/comandos-app/tests/tmux_guard.rs`

**Interfaces:**
- Consumes: `config::{RunMode, AppConfig}`.
- Produces:
  - `tmux::TmuxCtl::{new(mode: RunMode, socket: Option<String>) -> Self, query(&self, args: &[&str]) -> Result<String, TmuxError>, mutate(&self, args: &[&str]) -> Result<String, TmuxError>, has_session(&self, s: &str) -> bool, window_size(&self, s: &str) -> Option<(u16, u16)>, attach_argv(&self, s: &str) -> Vec<String>, hub_argv(&self) -> Option<Vec<String>>, mode(&self) -> RunMode}`; `tmux::{TmuxError, READ_VERBS, valid_session}`.
  - `guard::WriteGuard::{new(cfg: &AppConfig) -> Self, write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), GuardError>, allowed(&self, path: &Path) -> bool}`; `guard::GuardError`.
  - `jobs::Jobs::{start() -> Self, run<T: Send + 'static>(&self, work: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) + 'static)}`.
  - Pruebas: `support::tmux::TestTmux::{start() -> Option<Self>, socket(&self) -> &str, new_session(&self, name, cols, rows), session_size(&self, name) -> (u16, u16), client_flags(&self) -> Vec<String>}` (mata solo su servidor en `Drop`).

- [ ] **Step 1: pruebas que fallan**

`crates/comandos-app/tests/support/mod.rs`:
```rust
pub mod tmux;
```

`crates/comandos-app/tests/support/tmux.rs`:
```rust
//! Servidor tmux aislado: socket propio, TMUX_TMPDIR temporal y sin configuración del usuario.
use std::{path::PathBuf, process::Command};

pub struct TestTmux {
    socket: String,
    tmpdir: PathBuf,
}

impl TestTmux {
    /// `None` (y aviso) si no hay tmux en el PATH.
    pub fn start() -> Option<Self> {
        if Command::new("tmux").arg("-V").output().is_err() {
            eprintln!("aviso: sin tmux, prueba saltada");
            return None;
        }
        let socket = format!("comandos-app-test-{}-{}", std::process::id(), rand_suffix());
        let tmpdir = std::env::temp_dir().join(&socket);
        std::fs::create_dir_all(&tmpdir).ok()?;
        Some(Self { socket, tmpdir })
    }
    pub fn socket(&self) -> &str {
        &self.socket
    }
    pub fn cmd(&self, args: &[&str]) -> String {
        let out = Command::new("tmux")
            .env("TMUX_TMPDIR", &self.tmpdir)
            .env_remove("TMUX")
            .args(["-L", &self.socket, "-f", "/dev/null"])
            .args(args)
            .output()
            .expect("tmux");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
    pub fn new_session(&self, name: &str, cols: u16, rows: u16) {
        self.cmd(&["new-session", "-d", "-s", name, "-x", &cols.to_string(), "-y", &rows.to_string()]);
        self.cmd(&["set-option", "-g", "window-size", "latest"]);
    }
    pub fn session_size(&self, name: &str) -> (u16, u16) {
        let t = self.cmd(&["display-message", "-p", "-t", &format!("={name}:"), "#{window_width} #{window_height}"]);
        let mut it = t.split_whitespace().filter_map(|x| x.parse().ok());
        (it.next().unwrap_or(0), it.next().unwrap_or(0))
    }
    pub fn client_flags(&self) -> Vec<String> {
        self.cmd(&["list-clients", "-F", "#{client_flags}"]).lines().map(str::to_string).collect()
    }
    pub fn tmpdir(&self) -> &std::path::Path {
        &self.tmpdir
    }
}

fn rand_suffix() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0)
}

impl Drop for TestTmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux").env("TMUX_TMPDIR", &self.tmpdir).args(["-L", &self.socket, "kill-server"]).output();
        let _ = std::fs::remove_dir_all(&self.tmpdir);
    }
}
```

`crates/comandos-app/tests/tmux_guard.rs`:
```rust
mod support;
use comandos_app::config::RunMode;
use comandos_app::tmux::{TmuxCtl, TmuxError, READ_VERBS};
use std::process::{Command, Stdio};
use support::tmux::TestTmux;

const MUTATING: &[&str] = &[
    "new-session", "kill-session", "kill-server", "send-keys", "select-window", "select-pane",
    "split-window", "resize-pane", "set-option", "set-buffer", "load-buffer", "delete-buffer",
    "switch-client", "rename-session", "kill-pane", "respawn-pane", "attach-session", "attach",
];

#[test]
fn shadow_refuses_every_mutating_verb() {
    let ctl = TmuxCtl::new(RunMode::Shadow, None);
    for verb in MUTATING {
        assert!(matches!(ctl.mutate(&[verb, "-t", "=x"]), Err(TmuxError::ShadowRefused(_))), "{verb}");
        assert!(matches!(ctl.query(&[verb, "-t", "=x"]), Err(TmuxError::NotARead(_))), "{verb}");
    }
}

#[test]
fn read_verbs_never_mutate() {
    for verb in READ_VERBS {
        assert!(!MUTATING.contains(verb), "{verb}");
    }
}

#[test]
fn shadow_attach_is_read_only_and_ignores_size() {
    let ctl = TmuxCtl::new(RunMode::Shadow, None);
    assert_eq!(ctl.attach_argv("term-1-2"), ["tmux", "attach-session", "-f", "read-only,ignore-size", "-t", "=term-1-2"]);
    assert_eq!(ctl.hub_argv(), None, "la sombra no engancha el hub «local»");
    let live = TmuxCtl::new(RunMode::Live, None);
    assert_eq!(live.attach_argv("a"), ["tmux", "attach-session", "-t", "=a"]);
    assert_eq!(live.hub_argv().unwrap_or_default(), ["tmux", "new-session", "-A", "-s", "local", "-c", "~"]);
    let sbx = TmuxCtl::new(RunMode::Sandbox, Some("sbx".into()));
    assert_eq!(sbx.attach_argv("a"), ["tmux", "-L", "sbx", "attach-session", "-t", "=a"]);
}

#[test]
fn invalid_session_names_are_rejected() {
    let ctl = TmuxCtl::new(RunMode::Live, None);
    assert!(matches!(ctl.query(&["has-session", "-t", "=a;b"]), Err(TmuxError::BadTarget(_))));
    assert!(!ctl.has_session("../x"));
}

#[test]
fn shadow_attach_keeps_session_size() {
    let Some(t) = TestTmux::start() else { return };
    t.new_session("s1", 160, 44);
    assert_eq!(t.session_size("s1"), (160, 44));
    let ctl = TmuxCtl::new(RunMode::Shadow, Some(t.socket().to_string()));
    let argv = ctl.attach_argv("s1");
    // Cliente de 80x24 en un PTY propio: con ignore-size la sesión no cambia.
    let mut child = Command::new("script")
        .args(["-qfec", &format!("stty cols 80 rows 24; {}", shell_join(&argv)), "/dev/null"])
        .env("TMUX_TMPDIR", t.tmpdir())
        .env_remove("TMUX")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("script");
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert_eq!(t.session_size("s1"), (160, 44));
    let flags = t.client_flags().join(",");
    assert!(flags.contains("read-only") && flags.contains("ignore-size"), "{flags}");
    let _ = child.kill();
    let _ = child.wait();
}

fn shell_join(argv: &[String]) -> String {
    argv.iter().map(|a| format!("'{}'", a.replace('\'', r"'\''"))).collect::<Vec<_>>().join(" ")
}
```
(`script` de util-linux da el PTY de 80×24 sin escribir ningún archivo de shell; el comando es texto en la prueba.)

Añadir a `crates/comandos-app/tests/tmux_guard.rs` las pruebas de `WriteGuard`:
```rust
use comandos_app::config::{parse_args, AppConfig};
use comandos_app::guard::{GuardError, WriteGuard};

fn cfg(mode: &str, home: &std::path::Path) -> AppConfig {
    let h = home.to_string_lossy().into_owned();
    let env = move |k: &str| match k { "HOME" => Some(h.clone()), "XDG_RUNTIME_DIR" => Some(h.clone()), _ => None };
    parse_args(&["--mode".into(), mode.into()], false, &env).expect("cfg")
}

#[test]
fn shadow_guard_refuses_hooks_writes() {
    let dir = std::env::temp_dir().join(format!("guard-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".claude/hooks")).expect("dir");
    let g = WriteGuard::new(&cfg("shadow", &dir));
    let target = dir.join(".claude/hooks/app-tabs.json");
    assert!(matches!(g.write_atomic(&target, b"{}"), Err(GuardError::Shadow(_))));
    assert!(!target.exists());
    let live = WriteGuard::new(&cfg("live", &dir));
    live.write_atomic(&target, b"{\"a\":1}").expect("live escribe");
    assert_eq!(std::fs::read_to_string(&target).ok().as_deref(), Some("{\"a\":1}"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sandbox_guard_only_writes_inside_its_hooks() {
    let dir = std::env::temp_dir().join(format!("guard-sbx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let c = cfg("sandbox", &dir);
    let g = WriteGuard::new(&c);
    assert!(g.allowed(&c.hooks_dir.join("app-tabs.json")));
    assert!(!g.allowed(&dir.join(".claude/hooks/app-tabs.json")));
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: correrlas**

Run: `$C test -p comandos-app --test tmux_guard`
Expected: FAIL (módulos inexistentes).

- [ ] **Step 3: implementar**

`crates/comandos-app/src/tmux.rs`:
```rust
//! Acceso a tmux con el modo como guardia: en sombra solo se ejecutan verbos de
//! lectura y los attach llevan `read-only,ignore-size`. Nunca se llama desde el
//! hilo de interfaz: quien lo use pasa por `jobs::Jobs`.
use crate::config::RunMode;
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Verbos que no cambian nada en el servidor.
pub const READ_VERBS: &[&str] = &[
    "list-sessions", "list-windows", "list-panes", "list-clients", "list-buffers",
    "show-buffer", "display-message", "has-session", "show-options", "capture-pane",
];

const TIMEOUT: Duration = Duration::from_secs(5); // tmuxc del Python (línea 414)

#[derive(Debug, PartialEq, Eq)]
pub enum TmuxError {
    ShadowRefused(String),
    NotARead(String),
    BadTarget(String),
    Spawn(String),
    Timeout,
    Failed { code: i32, stderr: String },
}

/// Mismo patrón que `SESSION_RE` (línea 3572) y que cc-dash.
pub fn valid_session(s: &str) -> bool {
    (1..=80).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

/// `-t =sesión`, `=sesión:` o `=sesión:ventana` / ids `%12`, `@3`.
fn valid_target(t: &str) -> bool {
    let body = t.strip_prefix('=').unwrap_or(t);
    let session = body.split(':').next().unwrap_or("");
    if body.starts_with('%') || body.starts_with('@') || body.starts_with('$') {
        return body.len() > 1 && body.bytes().skip(1).all(|b| b.is_ascii_digit());
    }
    valid_session(session)
}

#[derive(Debug, Clone)]
pub struct TmuxCtl {
    mode: RunMode,
    socket: Option<String>,
}

impl TmuxCtl {
    pub fn new(mode: RunMode, socket: Option<String>) -> Self {
        Self { mode, socket }
    }

    pub fn mode(&self) -> RunMode {
        self.mode
    }

    fn base(&self) -> Vec<String> {
        let mut v = vec!["tmux".to_string()];
        if let Some(s) = &self.socket {
            v.extend(["-L".to_string(), s.clone()]);
        }
        v
    }

    fn check_targets(args: &[&str]) -> Result<(), TmuxError> {
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if *a == "-t" || *a == "-s" && args.first() != Some(&"new-session") {
                let Some(t) = it.next() else { return Err(TmuxError::BadTarget(String::new())) };
                if !valid_target(t) {
                    return Err(TmuxError::BadTarget((*t).to_string()));
                }
            }
        }
        Ok(())
    }

    pub fn query(&self, args: &[&str]) -> Result<String, TmuxError> {
        let verb = args.first().copied().unwrap_or_default();
        if !READ_VERBS.contains(&verb) {
            return Err(TmuxError::NotARead(verb.to_string()));
        }
        Self::check_targets(args)?;
        self.run(args)
    }

    pub fn mutate(&self, args: &[&str]) -> Result<String, TmuxError> {
        let verb = args.first().copied().unwrap_or_default();
        if self.mode == RunMode::Shadow {
            return Err(TmuxError::ShadowRefused(verb.to_string()));
        }
        Self::check_targets(args)?;
        self.run(args)
    }

    pub fn has_session(&self, s: &str) -> bool {
        valid_session(s) && self.query(&["has-session", "-t", &format!("={s}")]).is_ok()
    }

    /// `_tmux_window_size` (línea 749): `(cols, rows)` si es usable (≥ 20×5).
    pub fn window_size(&self, s: &str) -> Option<(u16, u16)> {
        let out = self.query(&["display-message", "-p", "-t", &format!("={s}:"), "#{window_width} #{window_height}"]).ok()?;
        let mut it = out.split_whitespace().filter_map(|x| x.parse::<u16>().ok());
        let (c, r) = (it.next()?, it.next()?);
        (c >= 20 && r >= 5).then_some((c, r))
    }

    pub fn attach_argv(&self, s: &str) -> Vec<String> {
        let mut v = self.base();
        v.push("attach-session".into());
        if self.mode == RunMode::Shadow {
            v.extend(["-f".into(), "read-only,ignore-size".into()]);
        }
        v.extend(["-t".into(), format!("={s}")]);
        v
    }

    /// Pestaña ⌂: en live/sandbox crea o engancha `local` (línea 4085); la sombra no la engancha.
    pub fn hub_argv(&self) -> Option<Vec<String>> {
        if self.mode == RunMode::Shadow {
            return None;
        }
        let mut v = self.base();
        v.extend(["new-session", "-A", "-s", "local", "-c", "~"].map(String::from));
        Some(v)
    }

    fn run(&self, args: &[&str]) -> Result<String, TmuxError> {
        let base = self.base();
        let mut cmd = Command::new(base.first().map(String::as_str).unwrap_or("tmux"));
        cmd.args(base.iter().skip(1)).args(args).env_remove("TMUX").env_remove("TMUX_PANE");
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TmuxError::Spawn(e.to_string()))?;
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if start.elapsed() > TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(TmuxError::Timeout);
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => return Err(TmuxError::Spawn(e.to_string())),
            }
        };
        let mut out = String::new();
        let mut err = String::new();
        if let Some(mut o) = child.stdout.take() {
            let _ = o.read_to_string(&mut out);
        }
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        if status.success() {
            Ok(out)
        } else {
            Err(TmuxError::Failed { code: status.code().unwrap_or(-1), stderr: err })
        }
    }
}
```
Nota para la implementación: la salida se lee al terminar porque las respuestas de los verbos de lectura caben de sobra en el búfer del pipe (64 KiB); `show-buffer` puede pasar de eso, así que para `show-buffer` el hilo lee stdout en paralelo (la Tarea 15 lo usa; ahí se cambia `run` para leer stdout mientras espera, con su prueba de 1 MiB).

`crates/comandos-app/src/guard.rs`:
```rust
//! Toda escritura en disco pasa por aquí: en sombra no se escribe nada y en
//! sandbox solo dentro de su directorio de hooks y de su directorio de ejecución.
use crate::config::{AppConfig, RunMode};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub enum GuardError {
    Shadow(PathBuf),
    Outside(PathBuf),
    Io(String),
}

#[derive(Debug, Clone)]
pub struct WriteGuard {
    mode: RunMode,
    roots: Vec<PathBuf>,
}

impl WriteGuard {
    pub fn new(cfg: &AppConfig) -> Self {
        let roots = match cfg.mode {
            RunMode::Shadow => Vec::new(),
            RunMode::Sandbox => vec![cfg.hooks_dir.clone(), cfg.runtime_dir.join("comandos-app-sbx")],
            RunMode::Live => vec![cfg.hooks_dir.clone(), cfg.runtime_dir.clone()],
        };
        Self { mode: cfg.mode, roots }
    }

    pub fn allowed(&self, path: &Path) -> bool {
        self.mode != RunMode::Shadow && self.roots.iter().any(|r| path.starts_with(r))
    }

    /// Escritura atómica (temporal + rename en el mismo directorio), como el Python.
    pub fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), GuardError> {
        if self.mode == RunMode::Shadow {
            return Err(GuardError::Shadow(path.to_path_buf()));
        }
        if !self.allowed(path) {
            return Err(GuardError::Outside(path.to_path_buf()));
        }
        let dir = path.parent().ok_or_else(|| GuardError::Io("ruta sin directorio".into()))?;
        fs::create_dir_all(dir).map_err(|e| GuardError::Io(e.to_string()))?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("estado");
        let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
        let res = (|| {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
            fs::rename(&tmp, path)
        })();
        if res.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        res.map_err(|e| GuardError::Io(e.to_string()))
    }
}
```

`crates/comandos-app/src/jobs.rs`:
```rust
//! Un hilo de trabajo para lo bloqueante (tmux, /proc, HTTP corto); el resultado
//! vuelve al bucle de GLib por un canal, así GTK nunca espera a tmux (el Python
//! llamaba a tmux desde el hilo de GTK en muchas rutas).
use std::sync::mpsc;

type Work = Box<dyn FnOnce() -> Box<dyn FnOnce() + Send> + Send>;

#[derive(Clone)]
pub struct Jobs {
    tx: mpsc::Sender<Work>,
    back: async_channel::Sender<Box<dyn FnOnce() + Send>>,
}

impl Jobs {
    /// Debe llamarse desde el hilo de GTK (con el contexto principal de GLib).
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel::<Work>();
        let (back, back_rx) = async_channel::unbounded::<Box<dyn FnOnce() + Send>>();
        let back_in_thread = back.clone();
        std::thread::Builder::new()
            .name("comandos-jobs".into())
            .spawn(move || {
                for work in rx {
                    let done = work();
                    if back_in_thread.send_blocking(done).is_err() {
                        break;
                    }
                }
            })
            .ok();
        glib::MainContext::default().spawn_local(async move {
            while let Ok(done) = back_rx.recv().await {
                done();
            }
        });
        Self { tx, back }
    }

    /// `work` corre en el hilo de trabajo; `done` en el de GTK con su resultado.
    pub fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(T) + Send + 'static,
    ) {
        let job: Work = Box::new(move || {
            let value = work();
            Box::new(move || done(value))
        });
        if self.tx.send(job).is_err() {
            eprintln!("comandos-app: hilo de trabajo caído");
        }
        let _ = &self.back;
    }
}
```
Nota: `done` es `Send` porque cruza el canal; los cierres que necesitan widgets (no `Send`) capturan un `glib::SendWeakRef`/identificador y buscan el widget en el registro del hilo de GTK. Las tareas siguientes usan ese patrón: el resultado lleva claves (`String`), no widgets.

`lib.rs`: añadir `pub mod guard; pub mod jobs; pub mod tmux;`.

- [ ] **Step 4: pruebas**

Run: `$C test -p comandos-app --test tmux_guard`
Expected: PASS (las dos pruebas con tmux real se saltan con aviso si no hay tmux).

- [ ] **Step 5: commit**

```bash
git add crates/comandos-app/src/lib.rs crates/comandos-app/src/tmux.rs crates/comandos-app/src/guard.rs crates/comandos-app/src/jobs.rs crates/comandos-app/tests/support/mod.rs crates/comandos-app/tests/support/tmux.rs crates/comandos-app/tests/tmux_guard.rs
git commit -m "feat(app): tmux con guardia de modo (sombra solo lectura e ignore-size), escrituras vigiladas e hilo de trabajos

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 4: motor del terminal (adaptador), colores, corridas de pintado y dibujo de cajas

**Depende de:** T1 (y de la Fase 3 si existe). **Paralelizable:** sí, con T2, T3, T8.

**Files:**
- Modify: `crates/comandos-app/Cargo.toml` (`comandos-term`, o `alacritty_terminal = "=0.26.0"` si la Fase 3 falta)
- Create: `crates/comandos-app/src/term/{mod,engine,paint,boxdraw}.rs`
- Test: `crates/comandos-app/tests/term_paint.rs`

**Interfaces:**
- Consumes: `comandos_term::{Terminal, GridSize, TermConfig, TermEvent, CellView, TermColor, CellFlags, CursorView, CursorShape, Modes, Damage}` (ver «Interfaz que se asume»).
- Produces:
  - `term::engine::TermEngine::{new(cols: u16, rows: u16, scrollback: usize) -> Self, feed(&mut self, bytes: &[u8]), resize(&mut self, cols: u16, rows: u16), size(&self) -> (u16, u16), events(&mut self) -> Vec<TermEvent>, damage(&mut self) -> Damage, row(&self, r: u16) -> Vec<CellView>, cursor(&self) -> CursorView, modes(&self) -> Modes, scroll(&mut self, lines: i32), sync_deadline_ms(&self) -> Option<u64>, sync_tick(&mut self, now_ms: u64), select_begin(&mut self, r: u16, c: u16), select_extend(&mut self, r: u16, c: u16), selection_text(&self) -> Option<String>, select_clear(&mut self), plain_text(&self, top: u16, rows: u16) -> String}`.
  - `term::paint::{Rgb, Palette { fg, bg, cursor, ansi: [Rgb; 16] }, Palette::from_hex(fg, bg, cursor, ansi) -> Option<Palette>, resolve(c: &TermColor, p: &Palette, is_fg: bool) -> Rgb, CellStyle { fg, bg, bold, italic, underline: u8, strike, dim, draw: bool }, style_of(cell: &CellView, p: &Palette) -> CellStyle, Run { col: u16, width: u16, text: String, style: CellStyle, kind: RunKind }, RunKind { Text, Glyph, BoxDraw }, runs(cells: &[CellView], p: &Palette) -> Vec<Run>}`.
  - `term::boxdraw::{is_box(ch: char) -> bool, segments(ch: char) -> Option<BoxSpec>, BoxSpec { up, down, left, right: Weight, block: Option<Block> }, Weight { None, Light, Heavy, Double }}`.

- [ ] **Step 1: pruebas que fallan**

`crates/comandos-app/tests/term_paint.rs`:
```rust
use comandos_app::term::boxdraw::{is_box, segments, Weight};
use comandos_app::term::engine::TermEngine;
use comandos_app::term::paint::{resolve, runs, Palette, Rgb, RunKind};
use comandos_term::TermColor;

fn noche() -> Palette {
    // PALETTE de cc-app (línea 109) con fg/bg/cursor del tema «noche».
    Palette::from_hex(
        "#EAF0FB", "#0A0D13", "#FFAE1A",
        &["#232B3E", "#FF6B6B", "#2EE59D", "#FFAE1A", "#7AA5FF", "#B49CFF", "#4DD0E1", "#EAF0FB",
          "#5E6980", "#FF8F8F", "#6FF0BC", "#FFC55C", "#A3C0FF", "#CDBBFF", "#86E3F0", "#FFFFFF"],
    )
    .expect("paleta")
}

#[test]
fn xterm_256_cube_and_grays() {
    let p = noche();
    assert_eq!(resolve(&TermColor::Indexed(16), &p, true), Rgb(0, 0, 0));
    assert_eq!(resolve(&TermColor::Indexed(196), &p, true), Rgb(255, 0, 0));
    assert_eq!(resolve(&TermColor::Indexed(232), &p, true), Rgb(8, 8, 8));
    assert_eq!(resolve(&TermColor::Indexed(255), &p, true), Rgb(238, 238, 238));
    assert_eq!(resolve(&TermColor::Named(1), &p, true), Rgb(0xFF, 0x6B, 0x6B));
    assert_eq!(resolve(&TermColor::Background, &p, false), Rgb(0x0A, 0x0D, 0x13));
}

#[test]
fn ascii_runs_merge_and_wide_cells_paint_once() {
    let mut t = TermEngine::new(20, 2, 100);
    t.feed("ab\x1b[31mcd\x1b[0m漢e".as_bytes());
    let row = t.row(0);
    let rs = runs(&row, &noche());
    let texts: Vec<(&str, u16, u16)> = rs.iter().filter(|r| !r.text.trim().is_empty()).map(|r| (r.text.as_str(), r.col, r.width)).collect();
    assert_eq!(texts.first(), Some(&("ab", 0, 2)));
    assert_eq!(texts.get(1), Some(&("cd", 2, 2)));
    assert_eq!(texts.get(2), Some(&("漢", 4, 2)), "ancho: 2 celdas, una sola vez");
    assert_eq!(texts.get(3), Some(&("e", 6, 1)));
    assert!(rs.iter().all(|r| r.kind != RunKind::BoxDraw));
}

#[test]
fn inverse_swaps_and_hidden_does_not_draw() {
    let mut t = TermEngine::new(10, 1, 0);
    t.feed(b"\x1b[7mX\x1b[0m\x1b[8mY");
    let rs = runs(&t.row(0), &noche());
    let x = rs.iter().find(|r| r.text == "X").expect("X");
    assert_eq!(x.style.fg, Rgb(0x0A, 0x0D, 0x13));
    assert_eq!(x.style.bg, Rgb(0xEA, 0xF0, 0xFB));
    let y = rs.iter().find(|r| r.text == "Y").expect("Y");
    assert!(!y.style.draw);
}

#[test]
fn box_drawing_is_drawn_in_code() {
    assert!(is_box('─') && is_box('│') && is_box('╭') && is_box('█'));
    assert!(!is_box('a'));
    let s = segments('┼').expect("cruz");
    assert_eq!((s.up, s.down, s.left, s.right), (Weight::Light, Weight::Light, Weight::Light, Weight::Light));
    let h = segments('━').expect("gruesa");
    assert_eq!((h.left, h.right, h.up), (Weight::Heavy, Weight::Heavy, Weight::None));
    let mut t = TermEngine::new(5, 1, 0);
    t.feed("─│".as_bytes());
    assert!(runs(&t.row(0), &noche()).iter().any(|r| r.kind == RunKind::BoxDraw));
}

#[test]
fn synchronized_update_holds_damage_until_end() {
    let mut t = TermEngine::new(10, 2, 0);
    let _ = t.damage();
    t.feed(b"\x1b[?2026hhola");
    assert!(t.sync_deadline_ms().is_some(), "CSI ? 2026 h abre un plazo");
    t.feed(b"\x1b[?2026l");
    assert!(t.sync_deadline_ms().is_none());
    assert!(t.plain_text(0, 1).starts_with("hola"));
}
```

- [ ] **Step 2: correrlas**

Run: `$C test -p comandos-app --test term_paint`
Expected: FAIL.

- [ ] **Step 3: implementar**

`crates/comandos-app/Cargo.toml`: `comandos-term = { path = "../comandos-term" }`. (Sin Fase 3: `alacritty_terminal = { version = "=0.26.0", default-features = false }` y `vte = { version = "=0.15.0", features = ["std", "ansi"] }`, y `term/engine.rs` define localmente los tipos de la sección «Interfaz que se asume» con los mismos nombres, de modo que `paint.rs` y las pruebas no cambian; las pruebas importan entonces `comandos_app::term::engine::TermColor` — ajustar el `use` en ese caso y anotarlo.)

`crates/comandos-app/src/term/mod.rs`:
```rust
//! Terminal de escritorio: modelo (comandos-term), pintado cairo, PTY y widget GTK.
pub mod boxdraw;
pub mod engine;
pub mod paint;
```

`crates/comandos-app/src/term/engine.rs` (adaptador fino; si la Fase 3 trae otros nombres, solo cambia este archivo):
```rust
//! Única puerta al modelo VT. El resto de la app no conoce comandos-term.
pub use comandos_term::{CellView, CursorView, Damage, Modes, TermEvent};
use comandos_term::{GridSize, TermConfig, Terminal};

pub struct TermEngine {
    term: Terminal,
    cols: u16,
    rows: u16,
}

impl TermEngine {
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let (cols, rows) = (cols.max(2), rows.max(1));
        Self { term: Terminal::new(GridSize { cols, rows }, TermConfig { scrollback }), cols, rows }
    }
    pub fn feed(&mut self, bytes: &[u8]) {
        self.term.advance(bytes);
    }
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let (cols, rows) = (cols.max(2), rows.max(1));
        if (cols, rows) != (self.cols, self.rows) {
            self.term.resize(GridSize { cols, rows });
            self.cols = cols;
            self.rows = rows;
        }
    }
    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }
    pub fn events(&mut self) -> Vec<TermEvent> {
        self.term.take_events()
    }
    pub fn damage(&mut self) -> Damage {
        self.term.damaged_rows()
    }
    pub fn row(&self, r: u16) -> Vec<CellView> {
        self.term.line(r)
    }
    pub fn cursor(&self) -> CursorView {
        self.term.cursor()
    }
    pub fn modes(&self) -> Modes {
        self.term.modes()
    }
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(lines);
    }
    pub fn sync_deadline_ms(&self) -> Option<u64> {
        self.term.sync_deadline_ms()
    }
    pub fn sync_tick(&mut self, now_ms: u64) {
        self.term.sync_tick(now_ms);
    }
    pub fn select_begin(&mut self, r: u16, c: u16) {
        self.term.selection_begin(r, c);
    }
    pub fn select_extend(&mut self, r: u16, c: u16) {
        self.term.selection_extend(r, c);
    }
    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_text()
    }
    pub fn select_clear(&mut self) {
        self.term.selection_clear();
    }
    /// Texto plano de `rows` filas desde `top` (sin espaciadores de ancho), con `\n`.
    pub fn plain_text(&self, top: u16, rows: u16) -> String {
        (top..top.saturating_add(rows).min(self.rows))
            .map(|r| {
                let line: String = self.row(r).iter().filter(|c| !c.spacer).map(|c| c.ch).collect();
                line.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
```

`crates/comandos-app/src/term/paint.rs`:
```rust
//! Del modelo a corridas de pintado. Puro: no conoce cairo.
use crate::term::boxdraw;
use comandos_term::{CellView, TermColor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn parse(hex: &str) -> Option<Rgb> {
        let h = hex.strip_prefix('#')?;
        let v = u32::from_str_radix(h, 16).ok()?;
        (h.len() == 6).then_some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }
    /// Mezcla hacia `other` (0.0 = self, 1.0 = other). Se usa para «dim».
    pub fn mix(self, other: Rgb, t: f64) -> Rgb {
        let m = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8;
        Rgb(m(self.0, other.0), m(self.1, other.1), m(self.2, other.2))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub fg: Rgb,
    pub bg: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
}

impl Palette {
    pub fn from_hex(fg: &str, bg: &str, cursor: &str, ansi: &[&str]) -> Option<Palette> {
        let mut out = [Rgb(0, 0, 0); 16];
        for (slot, hex) in out.iter_mut().zip(ansi.iter()) {
            *slot = Rgb::parse(hex)?;
        }
        (ansi.len() == 16).then_some(())?;
        Some(Palette { fg: Rgb::parse(fg)?, bg: Rgb::parse(bg)?, cursor: Rgb::parse(cursor)?, ansi: out })
    }
}

const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

pub fn resolve(c: &TermColor, p: &Palette, is_fg: bool) -> Rgb {
    match c {
        TermColor::Foreground => p.fg,
        TermColor::Background => p.bg,
        TermColor::Rgb(r, g, b) => Rgb(*r, *g, *b),
        TermColor::Named(i) | TermColor::Indexed(i) if *i < 16 => p.ansi.get(usize::from(*i)).copied().unwrap_or(if is_fg { p.fg } else { p.bg }),
        TermColor::Indexed(i) if *i < 232 => {
            let n = usize::from(*i - 16);
            let at = |k: usize| CUBE.get(k).copied().unwrap_or(0);
            Rgb(at(n / 36), at((n / 6) % 6), at(n % 6))
        }
        TermColor::Indexed(i) => {
            let v = 8 + 10 * (*i - 232);
            Rgb(v, v, v)
        }
        TermColor::Named(_) => if is_fg { p.fg } else { p.bg },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellStyle {
    pub fg: Rgb,
    pub bg: Rgb,
    /// El fondo es el del tema (se pinta con la opacidad del tema, no opaco).
    pub default_bg: bool,
    pub bold: bool,
    pub italic: bool,
    /// 0 = nada, 1 = simple, 2 = doble.
    pub underline: u8,
    pub strike: bool,
    pub draw: bool,
}

/// VTE por omisión: negrita no aclara (`bold-is-bright` falso desde 0.52); «dim» mezcla
/// un tercio hacia el fondo; «inverse» intercambia; «hidden» no pinta el glifo.
pub fn style_of(cell: &CellView, p: &Palette) -> CellStyle {
    let mut fg = resolve(&cell.fg, p, true);
    let mut bg = resolve(&cell.bg, p, false);
    let mut default_bg = matches!(cell.bg, TermColor::Background);
    if cell.flags.inverse {
        std::mem::swap(&mut fg, &mut bg);
        default_bg = false;
    }
    if cell.flags.dim {
        fg = fg.mix(bg, 1.0 / 3.0);
    }
    CellStyle {
        fg,
        bg,
        default_bg,
        bold: cell.flags.bold,
        italic: cell.flags.italic,
        underline: if cell.flags.double_underline { 2 } else { u8::from(cell.flags.underline) },
        strike: cell.flags.strike,
        draw: !cell.flags.hidden,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    /// ASCII imprimible de un mismo estilo: un solo `show_layout`.
    Text,
    /// No ASCII o ancho: se dibuja suelto en el origen de su celda.
    Glyph,
    /// U+2500–U+259F: dibujado en código (sin huecos entre filas).
    BoxDraw,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub col: u16,
    pub width: u16,
    pub text: String,
    pub style: CellStyle,
    pub kind: RunKind,
}

pub fn runs(cells: &[CellView], p: &Palette) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for (i, cell) in cells.iter().enumerate() {
        if cell.spacer {
            continue;
        }
        let col = u16::try_from(i).unwrap_or(u16::MAX);
        let style = style_of(cell, p);
        let width = if cell.wide { 2 } else { 1 };
        let kind = if boxdraw::is_box(cell.ch) {
            RunKind::BoxDraw
        } else if cell.ch.is_ascii_graphic() || cell.ch == ' ' {
            if cell.zerowidth.is_empty() { RunKind::Text } else { RunKind::Glyph }
        } else {
            RunKind::Glyph
        };
        let mut text = String::new();
        text.push(cell.ch);
        text.extend(cell.zerowidth.iter());
        if kind == RunKind::Text {
            if let Some(last) = out.last_mut() {
                if last.kind == RunKind::Text && last.style == style && last.col + last.width == col {
                    last.text.push_str(&text);
                    last.width += 1;
                    continue;
                }
            }
        }
        out.push(Run { col, width, text, style, kind });
    }
    out
}
```

`crates/comandos-app/src/term/boxdraw.rs` (líneas rectas y esquinas por tabla; bloques completos y medios; lo demás de U+2500–U+259F se marca como caja pero `segments` devuelve `None` y el pintor cae a la fuente):
```rust
//! Dibujo en código de las líneas de caja, como VTE: sin huecos entre filas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    None,
    Light,
    Heavy,
    Double,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    Full,
    UpperHalf,
    LowerHalf,
    LeftHalf,
    RightHalf,
    Shade(u8), // 1, 2, 3 = ░ ▒ ▓
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxSpec {
    pub up: Weight,
    pub down: Weight,
    pub left: Weight,
    pub right: Weight,
    /// Esquinas redondeadas (╭╮╯╰).
    pub rounded: bool,
    pub block: Option<Block>,
}

pub fn is_box(ch: char) -> bool {
    ('\u{2500}'..='\u{259F}').contains(&ch)
}

const fn s(up: Weight, down: Weight, left: Weight, right: Weight) -> BoxSpec {
    BoxSpec { up, down, left, right, rounded: false, block: None }
}

pub fn segments(ch: char) -> Option<BoxSpec> {
    use Weight::{Double as D, Heavy as H, Light as L, None as N};
    let spec = match ch {
        '─' => s(N, N, L, L), '━' => s(N, N, H, H), '│' => s(L, L, N, N), '┃' => s(H, H, N, N),
        '┌' => s(N, L, N, L), '┐' => s(N, L, L, N), '└' => s(L, N, N, L), '┘' => s(L, N, L, N),
        '┏' => s(N, H, N, H), '┓' => s(N, H, H, N), '┗' => s(H, N, N, H), '┛' => s(H, N, H, N),
        '├' => s(L, L, N, L), '┤' => s(L, L, L, N), '┬' => s(N, L, L, L), '┴' => s(L, N, L, L),
        '┼' => s(L, L, L, L), '┣' => s(H, H, N, H), '┫' => s(H, H, H, N), '┳' => s(N, H, H, H),
        '┻' => s(H, N, H, H), '╋' => s(H, H, H, H),
        '═' => s(N, N, D, D), '║' => s(D, D, N, N), '╔' => s(N, D, N, D), '╗' => s(N, D, D, N),
        '╚' => s(D, N, N, D), '╝' => s(D, N, D, N), '╠' => s(D, D, N, D), '╣' => s(D, D, D, N),
        '╦' => s(N, D, D, D), '╩' => s(D, N, D, D), '╬' => s(D, D, D, D),
        '╭' => BoxSpec { rounded: true, ..s(N, L, N, L) }, '╮' => BoxSpec { rounded: true, ..s(N, L, L, N) },
        '╯' => BoxSpec { rounded: true, ..s(L, N, L, N) }, '╰' => BoxSpec { rounded: true, ..s(L, N, N, L) },
        '╴' => s(N, N, L, N), '╵' => s(L, N, N, N), '╶' => s(N, N, N, L), '╷' => s(N, L, N, N),
        '█' => BoxSpec { block: Some(Block::Full), ..s(N, N, N, N) },
        '▀' => BoxSpec { block: Some(Block::UpperHalf), ..s(N, N, N, N) },
        '▄' => BoxSpec { block: Some(Block::LowerHalf), ..s(N, N, N, N) },
        '▌' => BoxSpec { block: Some(Block::LeftHalf), ..s(N, N, N, N) },
        '▐' => BoxSpec { block: Some(Block::RightHalf), ..s(N, N, N, N) },
        '░' => BoxSpec { block: Some(Block::Shade(1)), ..s(N, N, N, N) },
        '▒' => BoxSpec { block: Some(Block::Shade(2)), ..s(N, N, N, N) },
        '▓' => BoxSpec { block: Some(Block::Shade(3)), ..s(N, N, N, N) },
        _ => return None,
    };
    Some(spec)
}
```
Ajuste de `runs`: si `is_box(ch)` pero `segments(ch)` es `None`, la corrida es `Glyph` (la fuente lo dibuja). Cambiar la condición a `boxdraw::segments(cell.ch).is_some()` y mantener la prueba.

`lib.rs`: `pub mod term;`.

- [ ] **Step 4: pruebas**

Run: `$C test -p comandos-app --test term_paint`
Expected: 5 PASS.

- [ ] **Step 5: commit**

```bash
git add crates/comandos-app/Cargo.toml Cargo.lock crates/comandos-app/src/lib.rs crates/comandos-app/src/term/mod.rs crates/comandos-app/src/term/engine.rs crates/comandos-app/src/term/paint.rs crates/comandos-app/src/term/boxdraw.rs crates/comandos-app/tests/term_paint.rs
git commit -m "feat(app): adaptador de comandos-term, colores xterm-256, corridas de pintado y cajas dibujadas en código

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 5: PTY de escritorio y enganche cuando el tamaño se estabiliza

**Depende de:** T3, T4. **Paralelizable:** no.

**Files:**
- Create: `crates/comandos-app/src/term/{settle,pty}.rs`
- Modify: `crates/comandos-app/src/term/mod.rs`
- Test: `crates/comandos-app/tests/term_pty.rs`

**Interfaces:**
- Consumes: `tmux::TmuxCtl::{attach_argv, window_size}`.
- Produces:
  - `term::settle::Settle::{new(quiet_ms: u64, cap_ms: u64, start_ms: u64) -> Self, on_alloc(&mut self, now_ms: u64), due(&self, now_ms: u64) -> bool, fired(&mut self) -> bool, next_check_ms(&self, now_ms: u64) -> Option<u64>}` (`fired` marca y devuelve `true` solo la primera vez).
  - `term::settle::initial_size(allocated_px: i32, cell_w: f64, cell_h: f64, padding: (f64, f64), tmux_size: Option<(u16, u16)>) -> (u16, u16)`.
  - `term::pty::PtySession::{spawn(argv: &[String], cols: u16, rows: u16, cwd: &Path) -> Result<Self, PtyError>, read_chunk(&mut self, buf: &mut [u8]) -> ReadOutcome, write(&mut self, bytes: &[u8]) -> Result<(), PtyError>, flush_pending(&mut self) -> Result<bool, PtyError>, resize(&self, cols: u16, rows: u16) -> Result<(), PtyError>, raw_fd(&self) -> i32, child_tty(&self) -> Option<String>, try_reap(&mut self) -> Option<i32>, kill(&mut self)}`; `ReadOutcome { Data(usize), WouldBlock, Closed }`; `PtyError`.
  - Constantes: `SETTLE_QUIET_MS = 250`, `SETTLE_CAP_MS = 1500`, `RESPAWN_MS = 800` (líneas 760 y 896).

- [ ] **Step 1: pruebas que fallan**

`crates/comandos-app/tests/term_pty.rs`:
```rust
mod support;
use comandos_app::config::RunMode;
use comandos_app::term::pty::{PtySession, ReadOutcome};
use comandos_app::term::settle::{initial_size, Settle};
use comandos_app::tmux::TmuxCtl;
use support::tmux::TestTmux;

#[test]
fn settle_waits_for_quiet_allocation() {
    let mut s = Settle::new(250, 1500, 0);
    s.on_alloc(10);
    s.on_alloc(100); // 174 → 168 → 163 columnas al arrancar
    s.on_alloc(200);
    assert!(!s.due(400));
    assert!(s.due(451));
    assert!(s.fired());
    assert!(!s.fired(), "solo una vez");
}

#[test]
fn settle_cap_fires_even_while_resizing() {
    let mut s = Settle::new(250, 1500, 0);
    for t in (0..1600).step_by(100) {
        s.on_alloc(t);
    }
    assert!(s.due(1500));
}

#[test]
fn attach_before_allocation_uses_tmux_size() {
    // Asignación de 1 px (pantalla bloqueada): manda el tamaño de tmux.
    assert_eq!(initial_size((1, 1), 8.0, 18.0, (20.0, 30.0), Some((163, 44))), (163, 44));
    // Sin dato de tmux: 80x24, como VTE.
    assert_eq!(initial_size((1, 1), 8.0, 18.0, (20.0, 30.0), None), (80, 24));
    // Con tamaño real: (1300-20)/8 = 160 columnas, (822-30)/18 = 44 filas.
    assert_eq!(initial_size((1300, 822), 8.0, 18.0, (20.0, 30.0), Some((10, 10))), (160, 44));
}

#[test]
fn pty_roundtrip_and_resize() {
    let dir = std::env::temp_dir();
    let mut p = PtySession::spawn(&["/bin/cat".to_string()], 100, 30, &dir).expect("pty");
    p.write("ñandú 漢字\n".as_bytes()).expect("write");
    let mut buf = [0u8; 4096];
    let mut got = Vec::new();
    for _ in 0..200 {
        match p.read_chunk(&mut buf) {
            ReadOutcome::Data(n) => got.extend_from_slice(buf.get(..n).unwrap_or_default()),
            ReadOutcome::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(5)),
            ReadOutcome::Closed => break,
        }
        if String::from_utf8_lossy(&got).matches("ñandú 漢字").count() >= 2 {
            break;
        }
    }
    assert!(String::from_utf8_lossy(&got).contains("ñandú 漢字"));
    p.resize(120, 40).expect("resize");
    p.kill();
}

#[test]
fn tmux_attach_in_pty_gets_session_and_tty() {
    let Some(t) = TestTmux::start() else { return };
    t.new_session("s1", 120, 40);
    let ctl = TmuxCtl::new(RunMode::Sandbox, Some(t.socket().to_string()));
    let size = ctl.window_size("s1");
    assert_eq!(size, Some((120, 40)));
    let (c, r) = size.unwrap_or((80, 24));
    // El hijo necesita el TMUX_TMPDIR del servidor de prueba (sin `set_var`, que es unsafe en 2024).
    let mut p = PtySession::spawn_with_env(&ctl.attach_argv("s1"), c, r, &std::env::temp_dir(), &[("TMUX_TMPDIR", t.tmpdir())])
        .expect("pty");
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(p.child_tty().is_some_and(|tty| tty.starts_with("/dev/pts/")));
    assert_eq!(t.session_size("s1"), (120, 40), "el attach no encoge la sesión");
    p.kill();
}
```

- [ ] **Step 2: correrlas**

Run: `$C test -p comandos-app --test term_pty`
Expected: FAIL.

- [ ] **Step 3: implementar `settle.rs`**

```rust
//! `_spawn_when_settled` (línea 760): engancha cuando la asignación deja de cambiar
//! (`quiet_ms`) o, como tope, a los `cap_ms`. Así tmux redimensiona una sola vez.
pub const SETTLE_QUIET_MS: u64 = 250;
pub const SETTLE_CAP_MS: u64 = 1500;
pub const RESPAWN_MS: u64 = 800;

#[derive(Debug, Clone)]
pub struct Settle {
    quiet: u64,
    cap_at: u64,
    last_alloc: Option<u64>,
    done: bool,
}

impl Settle {
    pub fn new(quiet_ms: u64, cap_ms: u64, start_ms: u64) -> Self {
        Self { quiet: quiet_ms, cap_at: start_ms + cap_ms, last_alloc: None, done: false }
    }
    pub fn on_alloc(&mut self, now_ms: u64) {
        self.last_alloc = Some(now_ms);
    }
    pub fn due(&self, now_ms: u64) -> bool {
        !self.done && (now_ms >= self.cap_at || self.last_alloc.is_some_and(|t| now_ms >= t + self.quiet))
    }
    pub fn fired(&mut self) -> bool {
        let first = !self.done;
        self.done = true;
        first
    }
    /// Cuándo volver a mirar (ms absolutos), o `None` si ya disparó.
    pub fn next_check_ms(&self, now_ms: u64) -> Option<u64> {
        if self.done {
            return None;
        }
        let quiet = self.last_alloc.map(|t| t + self.quiet).unwrap_or(self.cap_at);
        Some(quiet.min(self.cap_at).max(now_ms + 1))
    }
}

/// Tamaño del primer attach: con la asignación real si existe; si GTK aún no dio
/// tamaño (1 px), el que la sesión ya tiene en tmux; si no, 80×24.
pub fn initial_size(allocated_px: i32, cell_w: f64, cell_h: f64, padding: (f64, f64), tmux_size: Option<(u16, u16)>) -> (u16, u16) {
    if allocated_px <= 1 {
        return tmux_size.unwrap_or((80, 24));
    }
    let cols = ((f64::from(allocated_px) - padding.0) / cell_w.max(1.0)).floor().max(2.0);
    let rows = (padding.1 / cell_h.max(1.0)).floor().max(1.0);
    (cols.min(f64::from(u16::MAX)) as u16, rows.min(f64::from(u16::MAX)) as u16)
}
```
(La firma de `initial_size` recibe ancho y alto en px; corregir: `allocated: (i32, i32)` y `padding: (f64, f64)` = relleno horizontal y vertical total. La prueba usa la forma `(ancho, cell_w, cell_h, padding, tmux)`; al implementar, cambiar firma y prueba juntas a `initial_size((w, h), cell_w, cell_h, (pad_x, pad_y), tmux_size)` con `w <= 1` como «sin tamaño», y anotar la firma final en el bloque Interfaces.)

- [ ] **Step 4: implementar `pty.rs`**

```rust
//! PTY por pestaña con `pty-process` (API bloqueante, sin `unsafe` en este crate).
//! El descriptor se pone no bloqueante y lo vigila el bucle de GLib (`term::view`).
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use std::{
    collections::VecDeque,
    io::{ErrorKind, Read, Write},
    os::fd::{AsFd, AsRawFd},
    path::Path,
    process::Child,
};

#[derive(Debug)]
pub enum PtyError {
    Open(String),
    Spawn(String),
    Io(String),
}

pub enum ReadOutcome {
    Data(usize),
    WouldBlock,
    Closed,
}

pub struct PtySession {
    pty: pty_process::blocking::Pty,
    child: Child,
    pending: VecDeque<u8>,
}

impl PtySession {
    pub fn spawn(argv: &[String], cols: u16, rows: u16, cwd: &Path) -> Result<Self, PtyError> {
        Self::spawn_with_env(argv, cols, rows, cwd, &[])
    }

    pub fn spawn_with_env(argv: &[String], cols: u16, rows: u16, cwd: &Path, env: &[(&str, &Path)]) -> Result<Self, PtyError> {
        let (program, args) = argv.split_first().ok_or_else(|| PtyError::Spawn("argv vacío".into()))?;
        let (pty, pts) = pty_process::blocking::open().map_err(|e| PtyError::Open(e.to_string()))?;
        // El tamaño va ANTES del attach: tmux nunca ve 80×24 (comportamiento de cc-app).
        pty.resize(pty_process::Size::new(rows, cols)).map_err(|e| PtyError::Open(e.to_string()))?;
        let mut cmd = pty_process::blocking::Command::new(program);
        cmd = cmd
            .args(args)
            .current_dir(cwd)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("NO_COLOR")
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor");
        for (k, v) in env {
            cmd = cmd.env(k, v);
        }
        let child = cmd.spawn(pts).map_err(|e| PtyError::Spawn(e.to_string()))?;
        let flags = fcntl(pty.as_fd(), FcntlArg::F_GETFL).map_err(|e| PtyError::Io(e.to_string()))?;
        fcntl(pty.as_fd(), FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK))
            .map_err(|e| PtyError::Io(e.to_string()))?;
        Ok(Self { pty, child, pending: VecDeque::new() })
    }

    pub fn raw_fd(&self) -> i32 {
        self.pty.as_raw_fd()
    }

    pub fn read_chunk(&mut self, buf: &mut [u8]) -> ReadOutcome {
        match self.pty.read(buf) {
            Ok(0) => ReadOutcome::Closed,
            Ok(n) => ReadOutcome::Data(n),
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::Interrupted => ReadOutcome::WouldBlock,
            // EIO = el hijo cerró el esclavo.
            Err(_) => ReadOutcome::Closed,
        }
    }

    /// Escribe lo que quepa; el resto queda pendiente para `flush_pending`.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), PtyError> {
        self.pending.extend(bytes);
        self.flush_pending().map(|_| ())
    }

    /// `true` si quedó todo escrito.
    pub fn flush_pending(&mut self) -> Result<bool, PtyError> {
        while !self.pending.is_empty() {
            let (a, _) = self.pending.as_slices();
            match self.pty.write(a) {
                Ok(0) => return Ok(false),
                Ok(n) => {
                    self.pending.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(false),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(PtyError::Io(e.to_string())),
            }
        }
        Ok(true)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyError> {
        self.pty.resize(pty_process::Size::new(rows, cols)).map_err(|e| PtyError::Io(e.to_string()))
    }

    /// TTY del hijo (`/proc/<pid>/fd/0`): identifica el cliente tmux de la pestaña (línea 875).
    pub fn child_tty(&self) -> Option<String> {
        std::fs::read_link(format!("/proc/{}/fd/0", self.child.id())).ok().map(|p| p.to_string_lossy().into_owned())
    }

    pub fn try_reap(&mut self) -> Option<i32> {
        self.child.try_wait().ok().flatten().map(|s| s.code().unwrap_or(-1))
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
```
Si la API de `pty-process 0.5.3` difiere en nombres (`open`, `Command::spawn(pts)`, `Size::new(rows, cols)`), ver `cargo doc -p pty-process --open` y ajustar solo este archivo; anotarlo en el commit.

- [ ] **Step 5: pruebas**

Run: `$C test -p comandos-app --test term_pty`
Expected: PASS.

- [ ] **Step 6: commit**

```bash
git add crates/comandos-app/src/term/mod.rs crates/comandos-app/src/term/settle.rs crates/comandos-app/src/term/pty.rs crates/comandos-app/tests/term_pty.rs
git commit -m "feat(app): PTY de escritorio no bloqueante y enganche a tmux cuando el tamaño se estabiliza

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
