# Fase 4 — `comandos-app`: escritorio GTK en Rust con `comandos-term` nativo — plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** sustituir `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` (Python, GTK3 + WebKit2GTK 2.50 + VTE 0.68, 9 019 líneas) por el binario Rust `comandos-app`, con terminal propio sobre `comandos-term`, idéntico en píxeles, con menos memoria y sin omitir ninguna función: al reiniciar la app se ven exactamente las mismas pestañas, splits, cuentas y conversaciones, el RSS medido es menor o igual y la vuelta atrás es un solo comando. La parte GTK de `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-notifyd` **no** está en esta fase: ya es `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-notifyd` con su propio cutover (`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/verification/cutover-notifyd.md`); `comandos-app` solo le habla por HTTP en el 4778, igual que el Python.

**Architecture:** un crate nuevo `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app` (binario `comandos-app`; despacho por `argv[0]`: `cc-app` → live, `comandos-app` → sandbox, `cc-notifyd`/`comandos-notifyd` → `Entry::Notifyd` y adaptador de T19 al crate existente). Tres modos que el tipo hace cumplir: **sandbox** (servidor tmux propio con socket `-S` dentro de su raíz temporal, hooks temporales, tablero solo en la banda 7200–7399), **shadow** (servidor tmux del usuario con clientes `attach -f read-only,ignore-size`, estado real en solo lectura: cero escrituras en tmux, en `/home/someguy/.claude/hooks`, en el tablero o en la presencia, y no consume los archivos IPC) y **live** (el sustituto real: mismos archivos, mismo candado de instancia única que el Python, así que nunca corren los dos). La lógica que no necesita GTK (tmux, archivos de estado, reanudación, plan de restauración, colores y CSS, codificación de teclado y ratón, mensajes del puente, comandos IPC, geometría de superposiciones) vive en módulos puros con pruebas sin pantalla y oráculo Python; la capa GTK es fina. El terminal es un `gtk::DrawingArea` pintado con cairo + pango sobre el modelo de `comandos-term`, con PTY propio (`pty-process`) vigilado por el bucle de GLib: un solo hilo de interfaz, sin VTE. La paridad de píxeles la vigila `cargo xtask png-diff` sobre capturas de `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` y de `comandos-app` tomadas con `cargo xtask app-shot` exclusivamente en el runtime Linux remoto de macmini, una vez comprobada su disponibilidad.

**Tech Stack:** Rust 1.96 (edition 2024). Desde T1: gtk-rs GTK3 `gtk = "=0.18.2"` (feature `v3_24`), `gdk = "=0.18.2"`, `glib = "=0.18.5"`, `gio = "=0.18.4"`, `cairo-rs = "=0.18.5"`, `pango = "=0.18.3"`, `pangocairo = "=0.18.0"`, `gdk-pixbuf = "=0.18.5"`, `pty-process = "=0.5.3"` (API `blocking`), `async-channel = "=2.5.0"`, `ureq = "=3.4.2"` (`default-features = false`, solo HTTP a 127.0.0.1), `regex = "=1.13.1"`, `nix = "=0.31.3"` (`fs`, `signal`, `process`, `user`), `serde_json` y `sha2` (workspace), `comandos-core`, `comandos-runtime` (`pane_snapshot::PaneInspector`). Desde T4: `comandos-term` (Fase 3). Desde T7: `webkit2gtk = "=2.0.2"` (WebKit2GTK 4.1, `features = ["v2_40"]`) y `javascriptcore-rs = "=1.1.2"` (arrastran `soup3`). En `xtask` (T7): `png = "=0.18.1"`.

**Spec:** `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§2 reglas de oro, §3.2, §4.1, §4.4–4.5 scope de systemd por pane, §5 sombra y cutover, §6 `xtask png-diff` e `import -window`, §7 fila «4», §8 riesgo «Bindings GTK3», Enmienda 4). Spike del terminal: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/research/2026-10-04-spike-terminal-rust.md`. Planes de la Fase 3: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/superpowers/plans/2026-10-04-fase-3a-comandos-term.md` y `-3b-comandos-web.md`. Oráculo: `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` y `/home/someguy/codebase/0xJesus/ComandOS/lib/{tmux_snapshot,agent_stop,tmux_clipboard,session_tabs,gtk_tabstrip,gtk_workspace,workspace_layout,work_marks,workspace_state,pane_snapshot}.py`; las líneas citadas son las de `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` en `main` `38c9167` (9 019 líneas, último cambio del archivo `3f0a01a`). Preflight de este plan: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/.superpowers/sdd/2026-10-04-fase-4-app-gtk/preflight.md` (hallazgos F01-F38, rulings R0-R9), aplicado entero.

**Precondición:** las Fases 2 y 3 están fusionadas en `main` (T4 en adelante usan `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-term`; hoy vive en `migration/rust-fase3`) y `comandos dash` sirve el 4777. El paquete de desarrollo `libwebkit2gtk-4.1-dev` debe estar instalado antes de T7: hoy solo están las bibliotecas de ejecución (`libwebkit2gtk-4.1-0 2.50.4`) y `libgtk-3-dev 3.24.33`. **Lo instala Jesús** (`sudo apt install libwebkit2gtk-4.1-dev`); ningún agente usa `sudo`. T1–T6 no lo necesitan.

**Estado comprobado (2026-10-05):** T1/T2 integradas en ca9cbdb; T3 implementada en e8ff79f y ronda de review 15f7216 aprobada. Las pruebas nativas no completan puertas visuales; T4-T20 describen trabajo pendiente.

---

## Rulings del controlador que fijan este plan

1. **Ninguna sesión se rompe ni se interrumpe.** Ningún paso reinicia el servidor tmux del usuario, ni `tmux.service`, ni la app Python en marcha, ni escribe en una sesión del usuario fuera del modo **live** ya activado por Jesús. Toda llamada a tmux de este crate y de sus pruebas lleva `-S <socket>` explícito (`TmuxCtl` y `TestTmux`); `TMUX_TMPDIR` solo no basta (tmux 3.2a lo ignora si el directorio no existe y cae en `/tmp/tmux-1000/default`). `kill-server` y `kill-session` no existen en la API mutante; el sandbox nunca apunta al servidor `default` del usuario. Las pruebas matan solo su servidor, primero `kill-server` con su `-S` y después borran el directorio.
2. **Cutover reversible y en manos de Jesús**: shadow con paridad, después todas las puertas, después un cambio reversible de `/home/someguy/.local/bin/cc-app` que reinicia solo la app, y solo con Jesús presente y su OK en el chat. La vuelta es `comandos install --rollback cc-app`. Nunca se reinicia cc-app sin él. T20 no toca `cc-notifyd`.
3. **Comentarios en español, identificadores en inglés**; `unsafe_code = "forbid"` heredado del workspace; sin `unwrap`/`expect`/indexado con `[]` fuera de pruebas (`#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]` en `lib.rs`, R9); `cargo clippy -D warnings`; `cargo fmt`; versiones exactas `=x.y.z`; cero archivos Python o bash nuevos (los oráculos Python se pasan como texto a `python3 -c`, como en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-runtime/tests/hook_claude_parity.rs`).
4. **Bindings GTK sin `unsafe`**: todo lo que este plan necesita existe en la API segura de gtk-rs 0.18 / webkit2gtk-rs 2.0 (`DrawingArea::connect_draw`, `glib::source::unix_fd_add_local`, `UserContentManager::connect_script_message_received`, `WebsiteDataManager::builder`, `gtk::IMMulticontext`). Ningún punto de este plan necesita `unsafe`; si una tarea encuentra un hueco que solo se cubre con `unsafe`, **se detiene** y lo informa con el símbolo exacto.
5. **Independencia para worktrees paralelos**: cada tarea declara de qué tareas depende; las de la 4b añaden su módulo y **una sola línea** de registro en `ui/app.rs` (sección «Registro de rebanadas»). `ui/mod.rs` y `pub mod ui;` los crea T7 (la primera que los necesita); `term/mod.rs` y `pub mod term;` los crea T4. El controlador integra `lib.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/Cargo.toml`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/Cargo.lock` y la línea base de `app-drift` en secuencia (F31/F34).
6. **Toda verificación de navegador/GTK ocurre en macmini.** La prohibición actual de Jesús prevalece sobre el permiso histórico de Xvfb. No se lanzan GTK, Chrome/Chromium, Playwright, Puppeteer ni Xvfb en este Linux local. Browser research y capturas web usan chrome-bg remoto; una app HTTP local se expone con cc-browser-expose. GTK3/WebKitGTK necesita un runtime Linux remoto en macmini y su pantalla configurada y verificada: un Chrome remoto o macOS nativo no sustituyen ese runtime. T7 verifica esa capacidad antes de cualquier puerta visual. Si falta, se registra exactamente la capacidad ausente y se continúa con pruebas nativas sin afirmar paridad visual. Nunca hay fallback local ni ventanas en la sesión personal.
7. **La paridad de píxeles tiene dueño**: `cargo xtask png-diff` (T7, Rust, mismo criterio que `/home/someguy/codebase/0xJesus/ComandOS/tools/png_diff.py`: canal de luminancia de la diferencia > 24, pasa con ≤ 0,1 % de píxeles distintos) y las puertas de píxeles de T12 contra capturas de `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` tomadas con `app-shot`.

## Global Constraints

- Todo se compila y prueba con
  `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target-fase4 nice -n 10 cargo <cmd> -j2` (abreviado `$C <cmd>`; directorio propio de la fase, F36). Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j2 -- -D warnings` y las pruebas de los paquetes tocados.
- Edition 2024, `rust-version = "1.96"`, `unsafe_code = "forbid"` (workspace).
- Commits con `git add <rutas>` explícitas; mensajes `feat(app): …`, `feat(xtask): …`, `feat(install): …`, `docs(verification): …` en español, línea en blanco y atribución del arnés real, si existe. El plan: `git add -f docs/superpowers/plans/2026-10-04-fase-4-app-gtk.md`.
- Puertos: nunca 4777–4782 en pruebas. Los tableros de prueba usan la banda de `devhost` 7200–7399 (por omisión 7311). Nunca 3000, 5173, 8000, 8080 ni el rango efímero 32768–60999 para puertos fijos.
- Rutas reales que el modo **live** usa, idénticas a las del Python (`/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` línea entre paréntesis): `/home/someguy/.claude/hooks/app-tabs.json` (1900, con candado `app-tabs.json.lock` compartido con `cc-dash`), `app-tabs-history.json` (1901), `app-tabs-snapshot.json` (2469), `app-sessions-v2.json` (2634), `app-tab-active.json` (4942), `app-tab-models.json` (4943), `app-extension-shelf.json` (5448), `app-layout.json` (7492), `app-pane-position.json` (8013), `app-focus.json` (8745), `app-tab-close.json` (8780), `app-tab-open.json` (8805), `app-command.json` (8982), `snippets.json` (443), `acp-panes.json` (2764). WebKit: datos en `/home/someguy/.local/share/comandos/` y caché en `/home/someguy/.cache/comandos/` (el Python fija `GLib.set_prgname("comandos")`, línea 192).
- JSON escrito con la codificación de `json.dump` del Python (orden de inserción, `ensure_ascii`, separadores `", "` y `": "`): `comandos_core::json::response_dumps`. `serde_json` del workspace lleva `preserve_order`.
- Ventana live: `WM_CLASS` `("comandos","comandos")`, título `ComandOS`, `prgname` `comandos`, `GDK_BACKEND=x11` (el `.desktop` ya lo exporta; la app lo fija con `gdk::set_allowed_backends("x11")` antes de `gtk::init`).
- Candado de instancia única live: `$XDG_RUNTIME_DIR/cc-app-<DISPLAY con '/'→'_' y sin ':'>.lock` (fallback `/tmp/comandos-<uid>/` y `/tmp`), el **mismo archivo** que el Python (líneas 43–73): si uno corre, el otro activa la ventana con `wmctrl -a ComandOS` (plazo 5 s) y sale.
- Sombra: título `Sombra de la app (Rust)` y `WM_CLASS` `("sombra-app-rs","sombra-app-rs")`; sandbox: `Sandbox de la app (Rust)` y `sandbox-app-rs`. **Ninguno contiene `comandos`** sin distinguir mayúsculas, porque `cc-dash` y el Python enfocan con `wmctrl -x -a comandos` y `wmctrl -a ComandOS`, que comparan por subcadena.
- Idioma de la interfaz: `CC_LANG` de `/home/someguy/.claude/hooks/cc-notify.conf` con las reglas de `read_conf` de `cc-dash`; si no, `es` cuando `$LANG` empieza por `es` (`_ui_lang`, 177; diferencia documentada en T1).
- Escrituras de archivos solo a través de `guard::WriteGuard` (T3); `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/clippy.toml` prohíbe `std::fs::{write, rename, remove_file, create_dir_all, File::create, OpenOptions::open}` fuera de `guard.rs`, y los procesos se lanzan solo desde `tmux.rs` y `proc.rs` (`std::process::Command::new` también prohibido fuera de ellos).

## Review Focus

1. **La sombra no escribe nunca en el tmux del usuario ni lo redimensiona.** Un cliente que se engancha a una sesión real con otro tamaño encoge la sesión para todos sus clientes (incidente «escribe sola», 1-oct). Esperado: los clientes de la sombra llevan `read-only,ignore-size`, el tamaño de la sesión no cambia, y cualquier verbo de tmux fuera de la lista de lectura devuelve `TmuxError::ShadowRefused`. Pruebas `shadow_attach_keeps_session_size` y `shadow_refuses_every_mutating_verb` (Tarea 3).
2. **Ningún camino llega al servidor tmux `default` del usuario desde el sandbox ni desde las pruebas.** Esperado: `TmuxCtl` solo se construye con `from_config`, el socket del sandbox vive bajo su raíz temporal, `kill-server`/`kill-session` no pasan por `mutate` y el único borrado de sesión es el tipado `kill_owned_session`. Pruebas `sandbox_socket_never_user_default`, `mutate_refuses_kill_verbs`, `abbreviated_verbs_are_refused` y `owned_kill_needs_proof` (Tarea 3).
3. **Arranque con la pantalla bloqueada o antes de que GTK dé tamaño.** El terminal mide 1 px y un `attach` a 80×24 encoge la sesión. Esperado: el primer attach usa el tamaño que la sesión ya tiene en tmux y solo cambia al tamaño real cuando la asignación se estabiliza (250 ms sin cambios, tope 1 500 ms). Pruebas `settle_waits_for_quiet_allocation`, `attach_before_allocation_uses_tmux_size` y `pty_attach_keeps_session_size_until_settled` (Tarea 5).
4. **Dos escritores sobre los archivos de estado** (Python y sombra a la vez; live y `cc-dash`). Esperado: en sombra `WriteGuard` rechaza toda escritura en `/home/someguy/.claude/hooks` y la sombra no consume los IPC; en live `app-tabs.json` se escribe bajo `flock` de `app-tabs.json.lock` con renombrado atómico; un arranque parcial nunca sobrescribe un snapshot completo. Pruebas `shadow_guard_refuses_hooks_writes`, `guard_refuses_dotdot_and_symlink_escape` (T3), `tabs_write_holds_shared_lock` y `shadow_does_not_consume_ipc` (T9), `no_snapshot_before_restore_finishes` (T11).
5. **Reanudación de conversaciones**: una pestaña muerta con Codex o Claude nunca debe reanudar otra conversación (`--last`, `--continue`), y los `-c` de Codex son de configuración. Esperado: mismos comandos que el Python, byte a byte. Prueba diferencial `resume_matches_python_oracle` (T11).
6. **Texto no ASCII y método de entrada**: `ñandú 漢字 🚀` tecleado (incluido IME) y pegado llega intacto a tmux; los caracteres anchos ocupan dos celdas y se pintan una vez. Pruebas `ime_commit_reaches_pty` (T6), `wide_cells_paint_once` (T4).
7. **Píxeles**: cada pantalla del Python capturada en el arnés tiene su gemela Rust a ≤ 0,1 %. Puertas `pixel_gate_*` de T12 y de cada rebanada 4b.

Además (cubiertos en sus tareas): `localStorage` del tablero conservado entre Python y Rust (T7), presión de memoria (oomd) antes del cutover (T20), mensajes del puente malformados ignorados sin pánico (T13), salidas de tmux de más de 64 KiB sin bloqueo (T3).

---

## Decisión de framework

**Decisión: gtk-rs GTK3 0.18 sin framework de arquitectura (sin relm), con WebKit2GTK 4.1 vía `webkit2gtk = "=2.0.2"`.** El estado de la app es un `Rc<App>` con `RefCell` por dominio y módulos puros probados aparte.

| Criterio | gtk4-rs 0.11 + relm4 0.11 | gtk4-rs 0.11 sin framework | **gtk-rs GTK3 0.18 sin framework** |
|---|---|---|---|
| WebKit embebible en Ubuntu 22.04 | **No**: el tablero necesita `webkitgtk-6.0`, que jammy no ofrece (solo hay `webkit2gtk-4.0/4.1`, ligados a GTK3) | **No**, mismo motivo | **Sí**: `libwebkit2gtk-4.1-0 2.50.4` instalada; `webkit2gtk 2.0.2` la envuelve |
| GTK disponible | 4.6.9 solo en ejecución, sin `-dev` | igual | 3.24.33 con `-dev` |
| RSS de una ventana vacía (a medir en la Tarea 7) | mayor: el renderizador GL de GSK reserva contexto y texturas | igual | menor: cairo por software, sin contexto GL |
| Arranque | inicialización GL añadida | igual | sin GL |
| Riesgo de bindings | relm4 añade macros y su propio ciclo de vida | mantenido activamente | gtk-rs GTK3 marcado sin mantenimiento (`RUSTSEC-2024-0411` a `-0420`; `RUSTSEC-2024-0429` sobre `glib::VariantStrIter`, que este código no usa); API estable; el binding delicado es WebKit (spec §8) |
| Píxeles iguales a `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` | otro motor de temas y otro CSS | igual | **el mismo GTK 3.24 y el mismo CSS**: la cabecera, las pestañas y los popovers salen idénticos |

Notas:
- Existe `gtk 0.19.0` (GTK3 sobre glib 0.22), pero `webkit2gtk 2.0.2` exige `gtk ^0.18` y `javascriptcore-rs =1.1`; mezclar glib 0.18 y 0.22 en el mismo proceso no es posible con tipos compartidos.
- La Tarea 7 es la **puerta**: si la app mínima no alcanza el presupuesto de RSS o el puente de WebKit falla, se para y se informa antes del port completo (spec §8).

## Blanco móvil: `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` cambia mientras se porta

Otras sesiones editan `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` en el checkout principal. Reglas:

1. La Tarea 2 creó `cargo xtask app-drift`, que guarda un hash por definición (`def`/`class`, anidadas incluidas, decoradores dentro) de `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` y `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-notifyd` en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/verification/app-drift-baseline.json` (459 definiciones de cc-app y 55 de cc-notifyd en `38c9167`).
2. **Cada tarea de port empieza** con `cargo xtask app-drift /home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` (lectura del checkout principal, sin tocarlo). Las funciones de su alcance (Apéndice A) que salgan como `changed`/`added` se portan en su versión nueva; la tarea lo anota en su commit y entrega al controlador la lista para `--accept <nombre>…` (el controlador integra la línea base en secuencia, F34).
3. Los oráculos Python leen `COMANDOS_CC_APP_ORACLE` si está definido (ruta al `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` del checkout principal) y si no el `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` del worktree; ver `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/support/oracle.rs` (T3).
4. La Tarea 20 (cutover) exige `app-drift` vacío contra el checkout principal el mismo día del cambio.

## Interfaz real de `comandos-term` (Fase 3)

La Fase 3 entrega `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-term` (modelo VT sobre `alacritty_terminal 0.26`, renderizado a tiras, glifos de caja, codificación de entrada, selección y enlaces). Esta fase usa **esa API tal cual** y la re-exporta desde `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/engine.rs` (F22); el resto de `comandos-app` importa de `crate::term::engine`, nunca de `comandos_term` directamente. No se usa `comandos-term-web` (depende de `wasm-bindgen`/`web-sys` sin condición); su planificador de pintado y su parpadeo de 600 ms se reescriben para escritorio en T6.

```rust
// comandos_term (Fase 3, `migration/rust-fase3`): lo que consume el escritorio.
pub mod engine {
    pub struct GridSize { pub cols: u16, pub rows: u16 }
    pub struct Palette { pub fg: [u8; 3], pub bg: [u8; 3], pub cursor: [u8; 3],
                         pub cursor_accent: [u8; 3], pub selection: [u8; 3], pub ansi: [[u8; 3]; 256] }
    impl Palette { pub fn xterm_default(fg, bg, cursor, cursor_accent, selection: [u8; 3]) -> Palette; }
    pub struct Engine;
    impl Engine {
        pub fn new(size: GridSize, scrollback: usize, palette: Palette) -> Engine;
        pub fn with_cursor_blink(size: GridSize, scrollback: usize, palette: Palette, blink: bool) -> Engine;
        pub fn advance(&mut self, bytes: &[u8], now_ms: f64);
        pub fn next_deadline_ms(&self) -> Option<f64>;      // fin de CSI ? 2026 h
        pub fn tick(&mut self, now_ms: f64) -> bool;
        pub fn resize(&mut self, size: GridSize, cell_px: (u16, u16));
        pub fn set_palette(&mut self, palette: Palette);
        pub fn drain(&mut self) -> Drained;                 // respuestas, título, portapapeles, timbre
        pub fn take_damage(&mut self) -> Damage;            // Full | Lines(Vec<usize>)
        pub fn take_damage_into(&mut self, lines: &mut Vec<usize>) -> bool; // true = todo
        pub fn scroll_display(&mut self, lines: i32);
        pub fn display_offset(&self) -> usize;
        pub fn history_len(&self) -> usize;
        pub fn modes(&self) -> Modes;
    }
    pub struct Drained { pub replies: Vec<u8>, pub title: Option<String>, pub clipboard: Option<String>,
                         pub clipboard_target: Option<ClipboardTarget>, pub bell: bool }
    pub enum ClipboardTarget { Clipboard, Selection }
    pub enum MouseMode { Off, Click, Drag, Motion }
    pub struct Modes { pub app_cursor: bool, pub app_keypad: bool, pub bracketed_paste: bool,
                       pub mouse: MouseMode, pub sgr_mouse: bool, pub alt_screen: bool,
                       pub alternate_scroll: bool, pub focus_events: bool, pub cursor_visible: bool }
    pub enum Damage { Full, Lines(Vec<usize>) }
    pub const MAX_REPLY_BYTES: usize;    // 64 KiB
    pub const MAX_TITLE_BYTES: usize;    // 4 KiB
    pub const MAX_CLIPBOARD_BYTES: usize; // 1 MiB
}
pub mod render {
    pub struct Style { pub fg: [u8; 3], pub bg: [u8; 3], pub bold: bool, pub italic: bool, pub dim: bool,
                       pub dim_fg: Option<[u8; 3]>, pub underline: Underline, pub underline_color: Option<[u8; 3]>,
                       pub strike: bool, pub hidden: bool, pub inverse: bool, pub default_bg: bool }
    pub enum Underline { None, Single, Double, Curly, Dotted, Dashed }
    pub enum RunKind { Text, Wide, Box(char) }
    pub struct Run { pub col: u16, pub cells: u16, pub text: String, pub style: Style, pub kind: RunKind, pub link: Option<u32> }
    pub struct RowRender { pub line: usize, pub bg_runs: Vec<(u16, u16, [u8; 3])>, pub runs: Vec<Run>, pub links: Vec<String> }
    pub struct RenderOpts { pub bold_is_bright: bool, pub min_contrast: f32 }   // Default = xterm.js (true, 1.0)
    pub use alacritty_terminal::vte::ansi::CursorShape;
    pub struct CursorView { pub line: usize, pub col: u16, pub shape: CursorShape, pub visible: bool, pub wide: bool }
    pub fn render_row(engine: &Engine, line: usize, palette: &Palette, opts: &RenderOpts) -> RowRender;
    pub fn render_row_into(/* misma entrada, reutiliza el RowRender */);
    pub fn cursor(engine: &Engine) -> CursorView;
}
pub mod glyphs {
    pub struct CellMetrics { pub cell_w: f64, pub cell_h: f64, pub dpr: f64, pub font_size: f64 }
    pub enum DrawOp { FillRect { x, y, w, h: f64 }, FillPattern { mask: &'static [&'static [u8]] }, ClipCell,
                      BeginPath, MoveTo { x, y: f64 }, LineTo { x, y: f64 },
                      CurveTo { x1, y1, x2, y2, x, y: f64 }, Stroke { line_width: f64 }, Fill }
    pub fn draw_ops(c: char, metrics: &CellMetrics, out: &mut Vec<DrawOp>) -> bool; // false = no es glifo de caja
}
pub mod input {
    pub struct KeyInput<'a> { pub key: &'a str, pub code: &'a str, pub key_code: u32,
                              pub ctrl: bool, pub alt: bool, pub shift: bool, pub meta: bool }
    pub enum KeyAction { Send(Vec<u8>), ScrollPage(i32), None }
    pub fn encode_key(k: &KeyInput, m: &Modes) -> KeyAction;     // semántica keyCode de xterm.js
    pub fn encode_paste(text: &str, m: &Modes) -> Vec<u8>;
    pub enum Button { Left, Middle, Right, WheelUp, WheelDown, None }
    pub enum MouseKind { Press, Release, Move }
    pub fn encode_mouse(b: Button, kind: MouseKind, col: u16, row: u16, mods: (bool, bool, bool), m: &Modes) -> Option<Vec<u8>>;
    pub enum WheelAction { Mouse(Vec<u8>), Arrows(Vec<u8>), Scroll(i32) }
    pub fn wheel(lines: i32, col: u16, row: u16, mods: (bool, bool, bool), m: &Modes) -> WheelAction;
    pub fn focus(on: bool, m: &Modes) -> Option<&'static [u8]>;
    pub struct Ime;  // start, end(data), input(input_type, data, is_composing), forget_commit, keydown_ignored(key_code)
}
pub mod select {
    pub type Point = (i32, u16);  // (fila absoluta, columna)
    pub enum SelectMode { Simple, Word, Line, Block }
    pub struct Selection { pub anchor: Point, pub head: Point, pub mode: SelectMode }
    pub struct UrlSpan { pub start: Point, pub end: Point, pub url: String }
    pub fn selection_bounds(engine: &Engine, s: &Selection) -> Option<(Point, Point)>;
    pub fn selected_text(engine: &Engine, s: &Selection) -> String;
    pub fn word_at(engine: &Engine, point: Point) -> Option<(Point, Point)>;
    pub fn find_urls(engine: &Engine, line: i32) -> Vec<UrlSpan>;
}
```

Diferencias VTE ↔ xterm.js que esta fase corrige en el escritorio (el Python usa VTE 0.68): `RenderOpts { bold_is_bright: false, min_contrast: 1.0 }` (VTE no aclara la negrita salvo `set_bold_is_bright`, que el Python no llama); el tenue (SGR 2) de VTE no es la mezcla al 50 % de xterm.js sino `vte_dim` (T4), calibrado contra píxeles de VTE en T12; la tecla de GDK se traduce al `KeyInput` de xterm.js en T6 con una prueba diferencial contra los bytes que VTE manda al PTY. Si `comandos-term` cambia una firma, solo cambia `term/engine.rs` y los módulos `term/*` que la usen; no se modifica `comandos-term` desde esta fase.

## Presupuesto y línea base de RSS

Medido el 2026-10-04 con `ps -o rss` (solo lectura) sobre la app viva, 22,5 h de actividad:

| Proceso | RSS hoy | Hilos | Objetivo Rust | Aceptación |
|---|---|---|---|---|
| `cc-app` (python3) → `comandos-app` | 130 MB | 84 | ≤ 60 MB con 20 pestañas adjuntas tras 1 h | ≤ 130 MB |
| `WebKitWebProcess` (tablero) | 146 MB | 63 | ≤ 146 MB (mismo motor 2.50) | ≤ 146 MB |
| `WebKitNetworkProcess` | 885 MB | 12 | ≤ 250 MB tras el experimento de la Tarea 20 | ≤ 885 MB |
| Arranque a ventana visible | sin medir | — | ≤ 600 ms | medido y anotado |

Las mediciones Rust y Python se toman con `cargo xtask rss` dentro de `cargo xtask app-shot --measure` en el runtime Linux remoto verificado de macmini, T7 y se guardan en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/verification/rss.jsonl` (formato existente), resumidas en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/verification/cutover-app.md`.

## Estructura de archivos

```
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/Cargo.toml                                    (miembro crates/comandos-app)                        T1 ✔
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/Cargo.toml                (+comandos-term T4, +webkit T7)                      T1 ✔, T4, T7
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/clippy.toml               (disallowed-methods de E/S y procesos)              T3
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/main.rs               (despacho argv[0])                                   T1 ✔, T7, T19
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/lib.rs                (módulos + deny de clippy R9)                        T1 ✔, T3…
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/config.rs             (RunMode, AppConfig, SocketLabel, parse_args, ui_lang) T1 ✔
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/guard.rs              (WriteGuard canónico)                                T3
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/tmux.rs               (TmuxCtl, TmuxError, READ_VERBS, MUTATE_VERBS)       T3
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/proc.rs               (procesos externos con plazo: wmctrl, xdg-open, systemd-run) T3
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/jobs.rs               (hilos de trabajo → bucle de GLib)                   T3
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/mod.rs                                                                 T4
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/engine.rs        (re-exporta comandos-term; TermEngine)              T4
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/paint.rs         (paleta VTE, plan de pintado por fila)               T4
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/settle.rs        (Settle, initial_size)                               T5
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/pty.rs           (PtySession)                                         T5
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/keys.rs          (gdk → KeyInput)                                     T6
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/links.rs         (url_from_wrapped_text, PATH_RE, URL_RE)             T6
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/schedule.rs      (planificador de pintado y parpadeo de escritorio)   T6
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/view.rs          (TermView: DrawingArea + PTY + entrada + selección)  T6
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/mod.rs                                                                   T7
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/app.rs             (App, run, «Registro de rebanadas»)                  T7
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/window.rs          (ventana, cabecera vacía, Paned, candado)            T7
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/webview.rs         (tablero, datos por modo, puente «centro»)           T7
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/layout_dump.rs        (LayoutDump)                                         T7
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/dash_client.rs        (DashClient, http_post, dash_call)                   T8
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/poll.rs               (PollLoop, PollUpdate)                               T8
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/theme.rs              (THEMES, theme_css, header_css, button_style_css, prefs) T8
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/state_files.rs        (lectura/escritura con flock)                        T9
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/tabs.rs               (TabRegistry puro: orden, favoritos, archivo)       T9
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/tabstrip.rs        (TabStripNotebook)                                   T9
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/tab_label.rs                                                             T9
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/confirm.rs         (diálogo que solo cierra con Sí/No)                  T9
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ipc.rs                (IpcRequest, monitores, no consume en sombra)        T9
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/workspace_view.rs     (prune, shape, split_paths, dock_target)             T10
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/workspace.rs       (GroupPage, WorkspaceView, arrastre y acople)        T10
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/resume.rs             (sane_flags, resume_command, exact_resume_command)   T11
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/snapshot.rs           (snapshot de pestañas y de layouts)                  T11
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/restore.rs            (RestorePlan y ejecutor)                             T11
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/{bridge,app_commands,presence}.rs                                        T13
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/{header,marks,hourglass}.rs                                              T14
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/{keys,switcher,help}.rs, src/agent_stop.rs                               T15
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/{menu,clipboard,snippets}.rs, src/clipboard_bridge.rs                    T16
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/{overlays,accounts,extensions}.rs, src/overlay_geom.rs                   T17
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/{side,mosaic,reader,modals}.rs                                           T18
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/notify.rs             (POST al 4778 de comandos-notifyd)                   T19
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/support/{mod,oracle,tmux}.rs                                              T3
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/*.rs, tests/gtk_smoke.rs (harness = false)                               T1 ✔, T3–T19
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/src/app_drift.rs                                                                              T2 ✔
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/src/{png_diff,app_shot,isolate}.rs                                                            T7
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/src/app_layout.rs                                                                             T12
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-cli/src/install.rs, crates/comandos-cli/src/install/release.rs (binario app)       T20
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/verification/app-drift-baseline.json                                                           T2 ✔
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/verification/app-shots/<escenario>/{python,rust,diff}.png                                      T7, T12–T18
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/verification/cutover-app.md                                                                    T12, T20
```

Orden y paralelismo (dependencias entre paréntesis):

- **4a, cimientos:** T1/T2 integradas; T3 aprobada; T4 y T5 pueden avanzar en paralelo después de sus prerrequisitos, T8 independiente tras T1; T6 consume T3/T4/T5; T7 consume T3/T6/T8; T9 consume T3/T7/T8; T10 después de T9; T11 después de T10; T12 verifica T6-T11.
- **4b, rebanadas:** T13 fija el puente; T14/T15 pueden avanzar en paralelo; T16 consume T15; T17 consume T14/T16; T18 consume T17/T10. T19 depende de T1/T8 y puede avanzar independientemente. T20 exige T1-T19 y todas las puertas remotas.

---

## 4a — Cimientos y app mínima

### Task 1: crate `comandos-app`, versiones fijadas, despacho y modos  ✔ `e63a6af`, `6a1a466`, `84f5684`

**Depende de:** nada. **Paralelizable:** no (las demás lo necesitan). **Estado:** implementada y aprobada, incluidas rondas `6a1a466`, `84f5684`, `079a42d`; integrada en `ca9cbdb`. Esta sección describe lo construido y aplica F03, F10, F18, F19, F20, F21 y el ruling «sandbox siempre con socket propio».

**Files:**
- Modify: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/Cargo.toml` (miembro `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app`)
- Create: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/Cargo.toml`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/{main,lib,config}.rs`
- Test: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/config.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/gtk_smoke.rs` (`harness = false`)

**Interfaces:**
- Consumes: nada del crate.
- Produces (`comandos_app::config`):
  - `enum RunMode { Sandbox, Shadow, Live }`.
  - `enum Entry { App { default_live: bool }, Notifyd }` y `fn resolve_entry(argv0: &str) -> Entry` (`cc-app` → `App { default_live: true }`; `cc-notifyd`/`comandos-notifyd` → `Notifyd`; el resto → sandbox).
  - `struct SocketLabel` con `SocketLabel::new(raw: &str) -> Result<SocketLabel, String>` (no vacía, ≤ 64, distinta de `default`, sin `.` inicial, `[A-Za-z0-9._-]`) y `as_str()`.
  - `enum TmuxServer<'a> { User, Private(&'a SocketLabel) }`.
  - `struct AppConfig` con campos privados y un `enum Mode` privado `{ Sandbox { socket: SocketLabel, root: PathBuf }, Shadow { socket: Option<SocketLabel> }, Live { socket: Option<SocketLabel> } }`: **un sandbox sin socket propio no es representable**. Accesores: `mode() -> RunMode`, `tmux_server() -> TmuxServer<'_>`, `tmux_socket() -> Option<&str>`, `sandbox_root() -> Option<&Path>`, `home()`, `hooks_dir()`, `dash_url() -> Option<&str>` (`None` en sandbox sin `--dash-url`), `web_data_dir()`, `web_cache_dir()`, `runtime_dir()`, `repo_root() -> Option<&Path>`, `writes_allowed() -> bool` (falso solo en sombra), `lock_file_name(display: &str) -> String`, `wm_class() -> &'static str`, `title() -> &'static str`, `layout_dump_path() -> PathBuf`.
  - `fn parse_args(args: &[String], default_live: bool, env: &dyn Fn(&str) -> Option<String>) -> Result<AppConfig, String>`; opciones `--mode sandbox|shadow|live`, `--tmux-socket NOMBRE`, `--hooks-dir DIR`, `--dash-url http://127.0.0.1:PUERTO`, `--repo DIR`.
  - `sandbox_temp(&self) -> Option<&Path>` (`84f5684`).
  - `fn ui_lang(hooks_dir: &Path, lang_env: Option<&str>) -> &'static str`.
- Comportamiento fijado:
  - `lock_file_name`: `DISPLAY` vacío → `x`; `/` → `_`; se quitan los `:` (F19, igual que `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app:43`); live `cc-app-<d>.lock`, sombra `sombra-app-rs-<d>.lock`, sandbox `comandos-app-sbx-<d>.lock`.
  - Sandbox: raíz `resolve($XDG_RUNTIME_DIR)/comandos-app-sbx` (o bajo `/tmp/comandos-<uid>`), socket por omisión `comandos-app-sbx` (la etiqueta no empieza por `.` ni `-`), hooks en `<raíz>/hooks`, datos WebKit en `<raíz>/{data,cache}`. **`parse_args` toca el disco en sandbox** (`84f5684`): crea con `mkdir` 0700 (un nivel) el `runtime_dir`, la raíz y, si hace falta, `<raíz>/tmp`, y comprueba con `lstat` que son directorios reales del `getuid()` sin escritura de grupo ni otros; los errores de argumentos salen antes de tocar nada. Por eso las pruebas del sandbox usan `HOME`, `XDG_RUNTIME_DIR` y `TMPDIR` de fixture. **Lista de permitidos** (`6a1a466`, `84f5684`): la raíz, `--hooks-dir` y los datos y la caché web se resuelven (`canonicalize` del ancestro existente más profundo + cola léxica) y tienen que quedar bajo `sandbox_root()` o `sandbox_temp()`; se rechazan `..` en la parte que no existe, enlaces colgantes y enlaces que salen. Ninguna ancla (raíz o `TMPDIR`) puede ser, contener ni estar dentro de `/home/someguy/.claude`, `/home/someguy/.claude-accounts`, `$CLAUDE_CONFIG_DIR`, `/home/someguy/.codex`, `/home/someguy/.config/comandos`, `/home/someguy/.local/share/comandos`, `/home/someguy/.cache/comandos`, `/home/someguy/.local/state` ni `/home/someguy/.ssh`, ni contener una casa (con el `HOME` del entorno y el de passwd). Un `TMPDIR` rechazado se sustituye por `<raíz>/tmp` privado, nunca `/tmp`. `sandbox_temp()` devuelve ese temporal validado: **los temporales del sandbox van ahí, nunca a `std::env::temp_dir()`** (F10). `--dash-url` solo en la banda 7200–7399 (F20).
  - Sombra: `writes_allowed()` es `false`: la sombra no escribe estado (archivos de hooks, tmux, tablero). Live: WebKit en `/home/someguy/.local/share/comandos` y `/home/someguy/.cache/comandos`; sombra en `$XDG_RUNTIME_DIR/comandos-app-shadow/{data,cache}` (rutas que T7 **no** usa: la sombra corre WebKit efímero).
  - Sombra y live: `--dash-url` o `COMANDOS_DASH_URL` (vacío cuenta como no definido, como `os.environ.get(...) or DEFAULT` en `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app:86`), por omisión `http://127.0.0.1:4777`; solo `http://127.0.0.1:PUERTO`. `hooks_dir` es absoluto en todos los modos.
  - `layout_dump_path()`: live `<runtime>/comandos-app-layout.json`, sombra `<runtime>/comandos-app-shadow-layout.json`, sandbox `sandbox_root()/layout.json`.
  - `repo_root`: `--repo`, `COMANDOS_APP_REPO` o el destino de `<hooks>/dash/index.html` dos niveles arriba.
  - `ui_lang`: lee `CC_LANG` de `<hooks>/cc-notify.conf` con `comandos_runtime::providers::read_conf` (el port de `read_conf` de `cc-dash`: la última asignación gana, comillas emparejadas fuera, `\r` suelto incluido). **Diferencia documentada**: `_ui_lang` del Python (177) pregunta a `GET /conf` y, si el tablero no responde, cae en `$LANG`; leer el archivo da el mismo resultado que `/conf` cuando los dos procesos tienen el mismo `LANG` y funciona sin tablero.
  - Quien use la configuración lo hace por los accesores, nunca por los campos (son privados).

**Cargo.toml del crate (lo construido):** `[lints] workspace = true`; `[[bin]] comandos-app`; `[[test]] gtk_smoke` con `harness = false`; dependencias exactamente las de «Tech Stack» desde T1 (`nix` con `fs, signal, process, user`; sin `webkit2gtk`, `javascriptcore-rs` ni `soup3`, que llegan en T7, F03).

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/main.rs`: `--version` imprime `comandos-app <versión>`; `Entry::App` y `Entry::Notifyd` salen con 2 y un aviso hasta que T7 y T19 sustituyen cada brazo. `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/lib.rs`: `pub mod config;`. `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/gtk_smoke.rs`: enlaza GTK (lee `gtk::major_version()…` sin inicializar) y, si falta `COMANDOS_GTK_TEST_DISPLAY`, imprime el aviso y pasa.

- [x] **Step 1: pruebas que fallan** — `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/config.rs`: `entry_follows_argv0`, `bare_comandos_app_is_sandbox`, `cc_app_defaults_to_live_with_real_paths`, `shadow_never_looks_like_the_real_app` (título y `WM_CLASS` sin `comandos`), `dash_url_must_be_loopback`, `unknown_flag_is_usage_error`, `lock_file_name_matches_python` (diferencial contra `python3 -c` con `DISPLAY=":0/x"`), `sandbox_dash_url_stays_in_devhost_band`, `sandbox_always_carries_a_private_socket` (`--tmux-socket default` es error; sin la opción, `tmux_server()` es `Private("comandos-app-sbx")`), `sandbox_refuses_real_hooks_dir` (también vía `..` y enlace simbólico), `ui_lang_reads_conf_then_lang`; añadidas en `6a1a466`: `sandbox_writes_only_inside_its_roots` (fixtures, nunca el `/home/someguy/.claude` real: hooks reales con `HOME` en otro sitio, subdirectorio inexistente tras un enlace, `link/../x`, `/home/someguy/.claude`, `~`, `/home/someguy/.config/comandos`, enlace colgante, `TMPDIR` que contiene el `HOME`), `only_shadow_forbids_writes`, `dash_env_rules`, `missing_home_and_dash_led_socket_are_errors`; en `84f5684`: raíz enlazada (a estado del fixture y a otro sitio), raíz con modo 0770/0775/0757/1777, raíz que es archivo, runtime con 0770, runtime dentro de estado, `TMPDIR` sobre estado (igual, dentro, contiene, enlace, `CLAUDE_CONFIG_DIR`, casa, `/`), `HOME` falso con la casa de passwd y otro dueño con metadatos inyectados (`private_dir_verdict`).
- [x] **Step 2: correrlas** — `$C test -p comandos-app --test config` → FAIL (no existe el crate).
- [x] **Step 3: implementar** `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/Cargo.toml` del workspace, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/{Cargo.toml, src/main.rs, src/lib.rs, src/config.rs, tests/gtk_smoke.rs}` como se describe arriba.
- [x] **Step 4: pruebas** — `$C test -p comandos-app` → 15 PASS en `config`, `gtk_smoke` pasa con aviso; `$C clippy -p comandos-app --all-targets -- -D warnings` limpio.
- [x] **Step 5: commits** — `e63a6af` `feat(app): crate comandos-app con modos sandbox/sombra/live y despacho por argv[0]`; `6a1a466` `fix(app): el sandbox solo escribe dentro de su raíz o del temporal (lista de permitidos)`; `84f5684` `fix(app): las anclas del sandbox se validan aparte y nunca pisan estado real`.

Pendiente de T1 que se asigna a otras tareas, sin reabrirla:
- **T3**: el tmux del sandbox usa `-S <sandbox_root()>/tmux/<etiqueta>`; `TmuxCtl::from_config` es el único constructor y lee solo accesores. `WriteGuard` toma sus raíces de los accesores (`sandbox_root()` y `sandbox_temp()` en sandbox) y vuelve a resolver la ruta **en el momento de escribir** (recorrido `openat` con `O_NOFOLLOW` desde la raíz permitida, archivo final con `O_NOFOLLOW | O_EXCL` en un temporal y `renameat` dentro del mismo descriptor de directorio) para cerrar la ventana TOCTOU entre `parse_args` y la escritura. Los temporales del sandbox van a `sandbox_temp()`, nunca a `std::env::temp_dir()`.
- **T7**: vuelve a comprobar la raíz del sandbox (0700, del usuario, `lstat`) justo antes de usarla, porque `parse_args` la creó en otro momento; el candado del sandbox vive dentro de la raíz (`sandbox_root()/<lock_file_name>`); la sombra usa un `WebsiteDataManager` **efímero** (nada persiste: ni `localStorage` ni caché) y no usa `web_data_dir()`/`web_cache_dir()`.
- **T8**: el `DashClient` de la sombra solo envía `GET`.
- **T19**: `Entry::Notifyd` delega en la entrada pública del crate comandos-notifyd existente.

### Task 2: `cargo xtask app-drift` — hash por función de `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app`  ✔ `49f2de0`, `01c3a1b`, `218aaa5`

**Depende de:** nada (solo `xtask`). **Paralelizable:** sí, con T3, T4, T8. **Estado:** implementada en `migration/rust-fase4-drift`; esta sección se corrige para describir lo construido (F15, F16, F17).

**Files:**
- Create: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/src/app_drift.rs`
- Modify: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/src/main.rs` (subcomando `app-drift`), `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/Cargo.toml` (`sha2.workspace = true`)
- Create: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/verification/app-drift-baseline.json`
- Test: pruebas unitarias en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/src/app_drift.rs`

**Interfaces:**
- Produces: `cargo xtask app-drift [--write-baseline] [--accept NOMBRE…] [--baseline RUTA] CC_APP [CC_NOTIFYD]`; `app_drift::{Def { qualname: String, hash: String }, Drift { added, removed, changed: Vec<String> }, defs(src: &str) -> Vec<Def>, checked_defs(src: &str) -> Result<Vec<Def>, ()>, diff(base: &[Def], now: &[Def]) -> Drift, Opts { baseline: PathBuf, write: bool, accept: Vec<String>, paths: Vec<String> }, parse_args(args: &[String]) -> Result<Opts, String>, run(baseline: &Path, paths: &[String], write: bool, accept: &[String]) -> Result<i32, String>}`.
- Códigos de salida: 0 sin deriva (o con `--write-baseline`), 1 con deriva, 2 si un archivo no se lee, la línea base está corrupta (salvo con `--write-baseline`), el escaneo acaba en estado inválido (dentro de una cadena, con paréntesis abiertos o con barra final) o falta el valor de `--baseline`/`--accept` o las rutas. El mensaje de subcomando desconocido lista `rss, parity, poll, app-drift`.

- [x] **Step 1: pruebas que fallan**

Al final de `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/src/app_drift.rs`, módulo `tests` con 25 pruebas. Las tres primeras fijan la idea:
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
Las otras 22: `repeated_qualname_gets_suffix` (una segunda definición con el mismo nombre calificado sale como `nombre#2`), `added_and_removed_are_reported`, `accept_only_touches_named_defs`, `run_baseline_roundtrip_in_tempdir`, `missing_input_is_an_error_not_drift`, `corrupt_baseline_is_an_error_unless_rewriting`, `baseline_keys_are_sorted_and_stable`, `decorator_is_part_of_the_hash`, `multiline_decorator_arguments_are_hashed`, `column_zero_triple_quoted_body_is_hashed`, `hash_and_blank_lines_inside_strings_count`, `single_quoted_strings_prefixes_and_escapes`, `backslash_continuation_inside_single_quoted_string`, `multiline_signature_closing_at_def_indent`, `def_under_if_is_not_nested_in_previous_def`, `nested_function_after_statement_in_body`, `def_keyword_inside_a_string_is_not_a_def`, `crlf_tabs_async_and_empty_input`, `nested_quotes_inside_fstring_fields`, `one_liners_and_trailing_dedent`, `unbalanced_scan_state_at_eof_is_an_error`, `options_without_value_are_usage_errors`.

- [x] **Step 2: correrlas** — `$C test -p xtask app_drift` → FAIL (`defs` no existe).

- [x] **Step 3: implementar** (lo construido; sustituye al esbozo por sangría de la versión anterior del plan)

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/src/app_drift.rs` (815 líneas con las pruebas):
- Un escáner de líneas (`struct Scanner`) sigue el estado léxico de Python: cadenas de una y de tres comillas con prefijos (`r`, `b`, `u`, `f`, `rb`…), escapes, f-strings con comillas anidadas dentro de `{…}`, comentarios, paréntesis/corchetes/llaves abiertos y continuaciones con barra. Solo las líneas cuyo inicio está fuera de cadena y de paréntesis pueden abrir una definición (`def`, `async def`, `class`) o cerrarla por sangría.
- El cuerpo de cada definición empieza en su primer decorador y acaba antes de la primera línea lógica con código a sangría ≤ la de la definición (las líneas dentro de cadenas multilínea no cierran nada). El hash es SHA-256 del texto normalizado (sin líneas vacías ni comentarios de línea completa **fuera** de cadenas; dentro de cadenas cuentan), recortado a 12 hex.
- Nombres calificados por pila de definiciones abiertas; las definiciones bajo un `if` a nivel de módulo no se anidan en la anterior; un nombre calificado repetido recibe sufijo `#n`.
- `checked_defs` devuelve `Err(())` si el escaneo acaba en estado inválido; `run` lo convierte en «estado de escaneo inválido al final de <archivo>» (salida 2) en lugar de hashear.
- `accepted(base, now, accept)` avanza solo las definiciones nombradas; las demás conservan el hash anterior; los archivos que no se pasan en esta llamada conservan su entrada. Las claves se guardan ordenadas (`BTreeMap`). Si un `--accept` no coincide con nada, se avisa por stderr.

En `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/src/main.rs`:
```rust
Some("app-drift") => {
    // Errores de uso con salida 2, antes de leer nada.
    let o = app_drift::parse_args(&args[1..]).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        exit(2);
    });
    match app_drift::run(&o.baseline, &o.paths, o.write, &o.accept) {
        Ok(code) => exit(code),
        Err(e) => {
            eprintln!("error: {e}");
            exit(2);
        }
    }
}
```

- [x] **Step 4: pruebas y línea base**

Run: `$C test -p xtask app_drift` → 25 PASS.

Run:
```bash
cargo xtask app-drift --write-baseline bin/cc-app bin/cc-notifyd
cargo xtask app-drift /home/someguy/codebase/0xJesus/ComandOS/bin/cc-app /home/someguy/codebase/0xJesus/ComandOS/bin/cc-notifyd
```
Expected: línea base con 459 definiciones de `cc-app` y 55 de `cc-notifyd`; el segundo comando, contra `main` en `38c9167`, sale con 0 (sin deriva).

- [x] **Step 5: commits** — `49f2de0` `feat(xtask): app-drift, hash por función de cc-app y cc-notifyd para portar un blanco móvil`; `01c3a1b` `fix(xtask): app-drift con escáner exacto de cadenas, decoradores y firmas multilínea`; `218aaa5` `fix(xtask): app-drift rechaza un escaneo inválido al final y los argumentos sin valor`.

### Task 3: tmux con `-S` y guardia de modo, escrituras vigiladas, procesos con plazo y trabajos fuera del hilo de interfaz

**Depende de:** T1. **Paralelizable:** sí, con T2, T4, T8, T19.

Aplica F05, F07, F08, F09, F10, F11, F12, F13, F14, F38, R9 y los rulings de tmux del controlador.

**Files:**
- Create: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/clippy.toml`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/{tmux,guard,proc,jobs}.rs`
- Modify: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/lib.rs` (módulos y `deny` de clippy)
- Create: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/support/{mod,oracle,tmux}.rs`
- Test: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/{tmux_guard,write_guard,proc_jobs}.rs`

**Interfaces:**
- Consumes: `config::{AppConfig, RunMode, TmuxServer}` (solo accesores).
- Produces:
  - `proc::{ProcSpec, ProcOutput, ProcError, run(spec: &ProcSpec) -> Result<ProcOutput, ProcError>, spawn_detached(program: &str, args: &[OsString]) -> Result<(), ProcError>, OUTPUT_CAP}`; `ProcSpec { program: String, args: Vec<OsString>, stdin: Option<Vec<u8>>, env: Vec<(String, OsString)>, clear_env: bool, env_remove: Vec<String>, cwd: Option<PathBuf>, timeout: Duration }`; `ProcOutput { code: Option<i32>, stdout: Vec<u8>, stderr: Vec<u8>, timed_out: bool, truncated: bool }`.
  - `tmux::{TmuxCtl, TmuxError, TmuxOut, OwnedSession, READ_VERBS, MUTATE_VERBS, TMUX_TIMEOUT, check_read_args(args: &[&str]) -> Result<(), TmuxError>}`:
    - `TmuxCtl::from_config(cfg: &AppConfig, env: &dyn Fn(&str) -> Option<String>) -> Result<TmuxCtl, TmuxError>` (único constructor).
    - `socket_path(&self) -> &Path`, `mode(&self) -> RunMode`.
    - `read(&self, args: &[&str]) -> Result<TmuxOut, TmuxError>`, `mutate(&self, args: &[&str]) -> Result<TmuxOut, TmuxError>`, `mutate_with_stdin(&self, args: &[&str], stdin: &[u8]) -> Result<TmuxOut, TmuxError>`.
    - `idle_scratch(&self, session: &str) -> Result<Option<OwnedSession>, TmuxError>`, `new_placeholder_session(&self, args: &[&str]) -> Result<(TmuxOut, Option<OwnedSession>), TmuxError>`, `kill_owned_session(&self, owned: OwnedSession) -> Result<TmuxOut, TmuxError>`.
    - `attach_argv(&self, session: &str) -> Vec<String>`, `window_size(&self, session: &str) -> Option<(u16, u16)>`, `prepare_socket_dir(&self, guard: &WriteGuard) -> Result<(), GuardError>`.
    - `TmuxOut { code: i32, stdout: String, stderr: String }` con `ok()`; `TmuxError { ShadowRefused(String), Forbidden(String), BadArgs(String), Spawn(String), Timeout(String) }`.
  - `guard::{WriteGuard, GuardError}`: `WriteGuard::from_config(cfg: &AppConfig, display: &str) -> WriteGuard` (raíces por accesores: sandbox `sandbox_root()` y `sandbox_temp()`, ya resueltas por `parse_args`; live `hooks_dir()` y `runtime_dir()` con su ruta canónica; sombra ninguna); `write_atomic(&self, path: &Path, bytes: &[u8], tmp_prefix: &str) -> Result<(), GuardError>`; `append_if_exists(&self, path: &Path, bytes: &[u8]) -> Result<bool, GuardError>`; `remove_file(&self, path: &Path) -> Result<(), GuardError>`; `create_dir_all(&self, path: &Path, mode: u32) -> Result<(), GuardError>`; `open_lock(&self, path: &Path) -> Result<std::fs::File, GuardError>`; `check(&self, path: &Path) -> Result<(), GuardError>`. `GuardError { Shadow(PathBuf), Outside(PathBuf), Escape(PathBuf), Io(PathBuf, String) }`.
  - `jobs::Jobs`: `Jobs::new(workers: usize, name: &str) -> Jobs`; `spawn<T: Send + 'static>(&self, work: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) + 'static)`; `spawn_loop(name: &str, body: impl FnOnce() + Send + 'static) -> std::io::Result<()>`; `to_main<T: Send + 'static>(done: impl FnOnce(T) + 'static) -> async_channel::Sender<T>`.
  - Constantes: `TMUX_TIMEOUT = 5 s` (`tmuxc`, línea 414), `OUTPUT_CAP = 16 MiB`.
  - Test support: `support::tmux::TestTmux::{for_mode(mode: RunMode) -> Option<Fixture>, new_session(&self, name: &str, cols: u16, rows: u16), session_size(&self, name: &str) -> (u16, u16), raw(&self, args: &[&str]) -> std::process::Output, socket(&self) -> &Path}`; `Fixture { tmux: TestTmux, config: AppConfig, ctl: TmuxCtl, guard: WriteGuard, env: Vec<(String, String)> }`; `support::oracle::{cc_app_path() -> PathBuf, python_eval(prelude_defs: &[&str], expr: &str) -> String}`.

Reglas que esta tarea fija para todo el crate:
- **`-S` siempre.** `TmuxCtl` pasa `-S <socket>` en cada llamada (y en el `attach` del PTY). Socket: sandbox `<sandbox_root()>/tmux/<etiqueta>` (directorio `<raíz>/tmux` con modo 0700, creado con `prepare_socket_dir`); sombra/live sin etiqueta `<TMUX_TMPDIR o /tmp>/tmux-<uid>/default` con la regla de tmux 3.2a (si `TMUX_TMPDIR` no es un directorio existente, `/tmp`); sombra/live con etiqueta, el mismo directorio con el nombre de la etiqueta. El sandbox nunca resuelve al socket `default` del usuario (comprobado en `from_config`).
- **Verbos por lista blanca exacta, sin abreviaturas.** tmux acepta prefijos únicos (`kill-ser`) y alias (`ls`): por eso `read` solo admite nombres completos de `READ_VERBS` y `mutate` solo de `MUTATE_VERBS`. `kill-server` y `kill-session` no están en ninguna; el único borrado de sesión es `kill_owned_session(OwnedSession)`, y `OwnedSession` solo la crean `idle_scratch` (sesión `term-*` cuyos panes son todos `zsh`/`bash`/`sh`/`fish`, regla de `close_tab`, 3383–3388) y `new_placeholder_session` (la sesión temporal que crea la restauración, que `restore_session` puede borrar si falla antes de arrancar un agente, `/home/someguy/codebase/0xJesus/ComandOS/lib/tmux_snapshot.py:206–211`).
- **Lecturas que no mutan** (F09): `display-message` y `capture-pane` exigen `-p`; `save-buffer` solo con destino `-`; ningún argumento puede contener `#(` (formato que ejecuta un comando); los `-t` tienen que ser un objetivo válido (`=sesión`, `=sesión:`, `=sesión:ventana`, `=sesión:^`, `%N`, `@N`, `$N` o `sesión`); `list-panes -s -t =sesión` es válido (F07).
- **Sombra**: `mutate`, `mutate_with_stdin` y `kill_owned_session` devuelven `ShadowRefused` sin lanzar nada; `attach_argv` añade `-f read-only,ignore-size`.
- **Salida grande sin bloqueo** (F08): `proc::run` lee stdout y stderr en hilos a la vez y escribe stdin desde otro, con tope `OUTPUT_CAP` y plazo; una salida de más de 64 KiB nunca bloquea al hijo.
- **Escrituras**: solo por `WriteGuard` (clippy `disallowed-methods`); la ruta se vuelve a resolver al escribir, componente a componente con `openat(O_DIRECTORY | O_NOFOLLOW)` desde la raíz canónica, el temporal se crea con `O_CREAT | O_EXCL | O_NOFOLLOW` y se renombra con `renameat` en el mismo descriptor de directorio. Un `..`, un `.` o un enlace en el recorrido es `Escape`.
- **Procesos**: solo `proc.rs` llama a `std::process::Command::new` (`#[allow(clippy::disallowed_methods)]` en esas dos llamadas, con su motivo). `tmux.rs` lanza tmux a través de `proc::run`.

### Contratos definitivos de T3, implementación Codex 2026-10-05

Estos contratos corrigen los ejemplos de código de abajo. La implementación e8ff79f y ronda 15f7216 son la fuente ejecutable para T3, no los bloques históricos. ProcSpec añade clear_env; sandbox usa /dev/null, HOME privado y env_clear. El token placeholder proviene de la propia respuesta de creación con -P/-F internos, nunca de una segunda consulta por nombre. El código implementado está en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/`.

- Live permite únicamente los archivos enumerados en Global Constraints, los candados de instancia y el log opt-in. No permite escribir todo hooks ni todo XDG_RUNTIME_DIR. Sombra conserva únicamente candado y layout propios. El recorrido de padres usa descriptores sin seguir enlaces, incluso para archivos de la lista cerrada. El prefijo del temporal debe ser un único componente.
- OwnedSession tiene campos privados ligados al socket, pid del servidor y session_id. Se revalida la identidad antes de borrar y se apunta por session_id. Para sesiones term-* se repite la comprobación de shell ocioso. El constructor de placeholders rechaza -A, abreviaturas de opciones y -s duplicado, para no reclamar una sesión preexistente. La observación de shell ocioso y el borrado siguen siendo dos operaciones; se requiere revisión de esta carrera al integrar el cierre de pestañas.
- La lectura analiza opciones de display-message/capture-pane para exigir un -p real y rechazar -I y agrupaciones desconocidas. Se rechazan separadores de listas tmux y formatos #( en lectura y mutación. Los llamadores que necesitan insertar texto con punto y coma lo transmiten por stdin y buffers, no como lista de comandos.
- Jobs usa mínimo dos trabajadores. to_main devuelve al MainContext thread-default; cae en el global si no hay contexto específico. Los cierres done siguen sin Send y pueden capturar Rc/widgets.
- run usa tuberías no bloqueantes con plazo, lectores concurrentes, escritor concurrente y grupo de procesos propio. Mata únicamente ese grupo al vencer el plazo y recoge al hijo. La salida retenida conserva el tope de 16 MiB por flujo. spawn_detached separa grupo POSIX y recoge al hijo, pero no hace setsid; el ejemplo anterior que afirmaba equivalencia con start_new_session era incorrecto.
- El soporte de tmux usa socket privado con -S, configuración /dev/null, entorno limpio y /bin/sh explícito; no carga perfiles ni credenciales del usuario. Todas las pruebas son nativas y sin pantalla. Prohibidos GTK local, Xvfb local y navegador local según la instrucción actual de Jesús, que prevalece sobre los rulings históricos.
- Verificación autorizada con -j2 y target-fase4; no se necesita ejecutar la app ni cambiar servicios.

- [ ] **Step 1: soporte de pruebas**

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/support/mod.rs`:
```rust
//! Soporte compartido de las pruebas de integración de `comandos-app`.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
pub mod oracle;
pub mod tmux;
```

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/support/tmux.rs`:
```rust
//! Servidor tmux de prueba: siempre con `-S` a un socket dentro de un directorio
//! temporal propio. Nunca toca `/tmp/tmux-<uid>/default`. Al soltarse hace
//! `kill-server` con su `-S` y DESPUÉS borra el directorio (CLAUDE.md del repo).
use comandos_app::config::{AppConfig, RunMode, parse_args};
use comandos_app::guard::WriteGuard;
use comandos_app::tmux::TmuxCtl;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

pub struct TestTmux {
    dir: PathBuf,
    socket: PathBuf,
}

pub struct Fixture {
    pub tmux: TestTmux,
    pub config: AppConfig,
    pub ctl: TmuxCtl,
    pub guard: WriteGuard,
    pub env: Vec<(String, String)>,
}

fn have_tmux() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .is_ok_and(|o| o.status.success())
}

impl TestTmux {
    /// Configuración, `TmuxCtl` y servidor de prueba coherentes para `mode`.
    /// Sandbox: socket `<dir>/run/comandos-app-sbx/tmux/t`. Sombra y live: etiqueta
    /// `t` con `TMUX_TMPDIR=<dir>/tt`, socket `<dir>/tt/tmux-<uid>/t`.
    /// `None` (la prueba se salta) si no hay tmux.
    pub fn for_mode(mode: RunMode) -> Option<Fixture> {
        Self::build(mode, true)
    }

    pub fn cold_for_mode(mode: RunMode) -> Option<Fixture> {
        Self::build(mode, false)
    }

    fn build(mode: RunMode, anchor: bool) -> Option<Fixture> {
        if !have_tmux() {
            eprintln!("tmux no está instalado: prueba saltada");
            return None;
        }
        let dir = std::env::temp_dir().join(format!(
            "comandos-app-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        for sub in ["home/.claude/hooks", "run", "tmp", "tt"] {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir.join(sub))
                .unwrap();
        }
        let env: Vec<(String, String)> = vec![
            ("HOME".into(), dir.join("home").display().to_string()),
            (
                "XDG_RUNTIME_DIR".into(),
                dir.join("run").display().to_string(),
            ),
            ("TMPDIR".into(), dir.join("tmp").display().to_string()),
            ("TMUX_TMPDIR".into(), dir.join("tt").display().to_string()),
        ];
        let lookup = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let mut args = vec![
            "--mode".to_string(),
            match mode {
                RunMode::Sandbox => "sandbox",
                RunMode::Shadow => "shadow",
                RunMode::Live => "live",
            }
            .to_string(),
        ];
        args.extend(["--tmux-socket".to_string(), "t".to_string()]);
        if mode != RunMode::Sandbox {
            args.extend([
                "--hooks-dir".to_string(),
                dir.join("home/.claude/hooks").display().to_string(),
            ]);
        }
        let config = parse_args(&args, false, &lookup).unwrap();
        let guard = WriteGuard::from_config(&config, ":99");
        let ctl = TmuxCtl::from_config(&config, &lookup).unwrap();
        if mode == RunMode::Sandbox {
            ctl.prepare_socket_dir(&guard).unwrap();
        }
        let socket = ctl.socket_path().to_path_buf();
        assert!(
            socket.starts_with(&dir),
            "el socket de prueba {} tiene que vivir en {}",
            socket.display(),
            dir.display()
        );
        if let Some(parent) = socket.parent() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .unwrap();
        }
        let tmux = TestTmux { dir, socket };
        // Sesión ancla para que el servidor no se apague al cerrar la última de la prueba.
        if anchor {
            tmux.new_session("__keep", 80, 24);
        }
        Some(Fixture {
            tmux,
            config,
            ctl,
            guard,
            env,
        })
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn raw(&self, args: &[&str]) -> Output {
        Command::new("tmux")
            .arg("-S")
            .arg(&self.socket)
            .args(["-f", "/dev/null"])
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.dir.join("home"))
            .env("SHELL", "/bin/sh")
            .env("TERM", "xterm-256color")
            .output()
            .unwrap()
    }

    pub fn new_session(&self, name: &str, cols: u16, rows: u16) {
        let out = self.raw(&[
            "new-session",
            "-d",
            "-s",
            name,
            "-x",
            &cols.to_string(),
            "-y",
            &rows.to_string(),
            "/bin/sh",
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    pub fn session_size(&self, name: &str) -> (u16, u16) {
        let out = self.raw(&[
            "display-message",
            "-p",
            "-t",
            &format!("={name}:"),
            "#{window_width} #{window_height}",
        ]);
        let text = String::from_utf8_lossy(&out.stdout);
        let mut it = text
            .split_whitespace()
            .map(|v| v.parse::<u16>().unwrap_or(0));
        (it.next().unwrap_or(0), it.next().unwrap_or(0))
    }
}

impl Drop for TestTmux {
    fn drop(&mut self) {
        // Primero kill-server con NUESTRO -S; después borrar el directorio.
        let _ = self.raw(&["kill-server"]);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
```

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/support/oracle.rs`:
```rust
//! Oráculo Python: carga funciones y constantes de `bin/cc-app` con `ast` (sin
//! ejecutar el módulo, que abriría GTK) y evalúa una expresión. Cero archivos
//! Python nuevos: el programa va como texto a `python3 -c`.
use std::path::PathBuf;
use std::process::Command;

/// `COMANDOS_CC_APP_ORACLE` (checkout principal) o el `bin/cc-app` del worktree.
pub fn cc_app_path() -> PathBuf {
    std::env::var_os("COMANDOS_CC_APP_ORACLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bin/cc-app"))
}

const LOADER: &str = r#"
import ast, json, sys, os, re, shlex
src = open(sys.argv[1]).read()
tree = ast.parse(src)
want = set(json.loads(sys.argv[2]))
ns = {"os": os, "re": re, "json": json, "shlex": shlex, "ES": True}
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.realpath(sys.argv[1]))), "lib"))
for node in tree.body:
    names = []
    if isinstance(node, (ast.FunctionDef, ast.ClassDef)):
        names = [node.name]
    elif isinstance(node, ast.Assign):
        names = [t.id for t in node.targets if isinstance(t, ast.Name)]
    if want & set(names):
        exec(compile(ast.Module([node], []), "cc-app", "exec"), ns)
print(json.dumps(eval(sys.argv[3], ns), ensure_ascii=False))
"#;

/// Define en un espacio de nombres solo los nombres de `defs` (funciones, clases o
/// asignaciones de nivel de módulo, en el orden del archivo) y devuelve el JSON de
/// `expr`. Si `defs` necesita otro nombre, se añade a la lista.
pub fn python_eval(defs: &[&str], expr: &str) -> String {
    let out = Command::new("python3")
        .arg("-c")
        .arg(LOADER)
        .arg(cc_app_path())
        .arg(serde_json::to_string(defs).unwrap())
        .arg(expr)
        .output()
        .expect("python3");
    assert!(
        out.status.success(),
        "oráculo: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}
```

- [ ] **Step 2: pruebas que fallan**

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/tmux_guard.rs`:
```rust
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
mod support;
use comandos_app::config::RunMode;
use comandos_app::tmux::{MUTATE_VERBS, READ_VERBS, TmuxError, check_read_args};
use support::tmux::TestTmux;

#[test]
fn sandbox_socket_never_user_default() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    let uid = nix::unistd::getuid().as_raw();
    let user_default = std::path::PathBuf::from(format!("/tmp/tmux-{uid}/default"));
    assert_ne!(f.ctl.socket_path(), user_default);
    assert!(
        f.ctl
            .socket_path()
            .starts_with(f.config.sandbox_root().unwrap())
    );
    assert!(f.ctl.socket_path().ends_with("tmux/t"));
}

#[test]
fn every_call_carries_explicit_socket() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    f.tmux.new_session("s1", 100, 30);
    // Si TmuxCtl no pasara -S, este has-session iría al servidor por omisión y fallaría.
    assert!(f.ctl.read(&["has-session", "-t", "=s1"]).unwrap().ok());
    let argv = f.ctl.attach_argv("s1");
    assert_eq!(argv[0], "/bin/sh");
    assert!(
        argv[2].contains(&format!("-S '{}'", f.ctl.socket_path().display())),
        "{argv:?}"
    );
}

#[test]
fn mutate_refuses_kill_verbs() {
    let Some(f) = TestTmux::for_mode(RunMode::Live) else {
        return;
    };
    f.tmux.new_session("victim", 80, 24);
    for args in [
        ["kill-server"].as_slice(),
        &["kill-session", "-t", "=victim"],
    ] {
        assert!(
            matches!(f.ctl.mutate(args), Err(TmuxError::Forbidden(_))),
            "{args:?}"
        );
        assert!(
            matches!(f.ctl.read(args), Err(TmuxError::Forbidden(_))),
            "{args:?}"
        );
    }
    assert!(
        f.ctl.read(&["has-session", "-t", "=victim"]).unwrap().ok(),
        "la sesión sigue viva"
    );
    assert!(!MUTATE_VERBS.contains(&"kill-session") && !READ_VERBS.contains(&"kill-server"));
}

#[test]
fn abbreviated_verbs_are_refused() {
    let Some(f) = TestTmux::for_mode(RunMode::Live) else {
        return;
    };
    for verb in [
        "kill-ser", "kill-ses", "killw", "ls", "show", "lsp", "send", "display",
    ] {
        assert!(
            matches!(f.ctl.mutate(&[verb]), Err(TmuxError::Forbidden(_))),
            "{verb}"
        );
        assert!(
            matches!(f.ctl.read(&[verb]), Err(TmuxError::Forbidden(_))),
            "{verb}"
        );
    }
}

#[test]
fn read_args_must_not_mutate() {
    assert!(
        check_read_args(&["display-message", "-t", "=s:", "#{pane_id}"]).is_err(),
        "sin -p"
    );
    assert!(check_read_args(&["display-message", "-p", "-t", "=s:", "#{pane_id}"]).is_ok());
    assert!(
        check_read_args(&["capture-pane", "-t", "%3"]).is_err(),
        "sin -p crea un buffer"
    );
    assert!(check_read_args(&["capture-pane", "-p", "-t", "%3"]).is_ok());
    assert!(check_read_args(&["save-buffer", "/tmp/x"]).is_err());
    assert!(check_read_args(&["save-buffer", "-"]).is_ok());
    assert!(check_read_args(&["display-message", "-p", "#(rm -rf ~)"]).is_err());
    assert!(
        check_read_args(&[
            "list-panes",
            "-s",
            "-t",
            "=sess",
            "-F",
            "#{pane_current_command}"
        ])
        .is_ok()
    );
    assert!(check_read_args(&["list-panes", "-t", "=s; kill-server", "-F", "x"]).is_err());
    for target in [
        "=s",
        "=s:",
        "=s:claude",
        "=s:^",
        "=s:2",
        "%12",
        "@3",
        "$4",
        "term-12-3",
    ] {
        assert!(
            check_read_args(&["display-message", "-p", "-t", target, "x"]).is_ok(),
            "{target}"
        );
    }
}

#[test]
fn shadow_refuses_every_mutating_verb() {
    let Some(f) = TestTmux::for_mode(RunMode::Shadow) else {
        return;
    };
    f.tmux.new_session("s1", 80, 24);
    for verb in MUTATE_VERBS {
        assert!(
            matches!(
                f.ctl.mutate(&[verb, "-t", "=s1"]),
                Err(TmuxError::ShadowRefused(_))
            ),
            "{verb}"
        );
    }
    assert!(matches!(
        f.ctl
            .mutate_with_stdin(&["load-buffer", "-b", "x", "-"], b"hola"),
        Err(TmuxError::ShadowRefused(_))
    ));
    assert!(
        f.ctl
            .read(&["list-sessions", "-F", "#{session_name}"])
            .unwrap()
            .stdout
            .contains("s1")
    );
}

#[test]
fn shadow_attach_keeps_session_size() {
    let Some(f) = TestTmux::for_mode(RunMode::Shadow) else {
        return;
    };
    f.tmux.new_session("big", 163, 44);
    let argv = f.ctl.attach_argv("big");
    assert!(
        argv[2].contains("attach -f read-only,ignore-size -t '=big'"),
        "{argv:?}"
    );
    // Cliente real de 80×24 a través de `script` (un PTY): la sesión no encoge.
    let mut child = std::process::Command::new("script")
        .args([
            "-qfec",
            &format!("stty cols 80 rows 24; {}", argv[2]),
            "/dev/null",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert_eq!(f.tmux.session_size("big"), (163, 44));
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn owned_kill_needs_proof() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    f.tmux.new_session("term-9-1", 80, 24);
    f.tmux
        .raw(&["send-keys", "-t", "=term-9-1:", "sleep 300", "Enter"]);
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(
        f.ctl.idle_scratch("term-9-1").unwrap().is_none(),
        "corre algo: no se mata"
    );
    f.tmux.new_session("term-9-2", 80, 24);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let owned = f
        .ctl
        .idle_scratch("term-9-2")
        .unwrap()
        .expect("shell ocioso");
    assert!(f.ctl.kill_owned_session(owned).unwrap().ok());
    assert!(
        !f.ctl
            .read(&["has-session", "-t", "=term-9-2"])
            .unwrap()
            .ok()
    );
    assert!(
        f.ctl.idle_scratch("proyecto").unwrap().is_none(),
        "solo term-*"
    );
}

#[test]
fn placeholder_can_be_removed_by_its_creator() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    let (out, owned) = f
        .ctl
        .new_placeholder_session(&[
            "new-session",
            "-d",
            "-P",
            "-F",
            "#{pane_id}",
            "-s",
            "ph",
            "sleep",
            "2147483647",
        ])
        .unwrap();
    assert!(out.ok());
    assert!(
        f.ctl
            .kill_owned_session(owned.expect("token"))
            .unwrap()
            .ok()
    );
}

#[test]
fn large_output_never_blocks() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    let big = "x".repeat(1 << 20); // 1 MiB por stdin y de vuelta por stdout
    assert!(
        f.ctl
            .mutate_with_stdin(&["load-buffer", "-b", "big", "-"], big.as_bytes())
            .unwrap()
            .ok()
    );
    let started = std::time::Instant::now();
    let out = f.ctl.read(&["show-buffer", "-b", "big"]).unwrap();
    assert_eq!(out.stdout.len(), 1 << 20);
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
}

#[test]
fn window_size_ignores_tiny_sessions() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    f.tmux.new_session("ok", 120, 40);
    f.tmux.new_session("tiny", 10, 3);
    assert_eq!(f.ctl.window_size("ok"), Some((120, 40)));
    assert_eq!(
        f.ctl.window_size("tiny"),
        None,
        "cols >= 20 y rows >= 5 (_tmux_window_size)"
    );
    assert_eq!(f.ctl.window_size("nope"), None);
}

#[test]
fn tmux_command_chains_are_refused_before_execution() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    for args in [
        vec!["list-sessions", ";", "kill-server"],
        vec!["capture-pane", "-p", ";kill-server"],
        vec!["display-message", "-p", "-I"],
    ] {
        assert!(f.ctl.read(&args).is_err());
    }
    assert!(
        f.ctl
            .mutate(&["new-session", "-d", ";", "kill-server"])
            .is_err()
    );
    assert!(f.ctl.read(&["has-session", "-t", "=__keep"]).unwrap().ok());
}

#[test]
fn owned_token_cannot_kill_a_different_server() {
    let first = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    let second = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    let (_, token) = first
        .ctl
        .new_placeholder_session(&["new-session", "-d", "-s", "ph", "sleep", "30"])
        .unwrap();
    second.tmux.new_session("ph", 80, 24);
    assert!(second.ctl.kill_owned_session(token.unwrap()).is_err());
    assert!(second.ctl.read(&["has-session", "-t", "=ph"]).unwrap().ok());
}

#[test]
fn reads_preserve_sessions_buffers_and_clients() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.tmux.new_session("s", 80, 24);
    f.ctl
        .mutate_with_stdin(&["load-buffer", "-b", "fixture", "-"], b"hello")
        .unwrap();
    let snapshot =
        || ["list-sessions", "list-buffers", "list-clients"].map(|verb| f.tmux.raw(&[verb]).stdout);
    let before = snapshot();
    let cases = [
        vec!["has-session", "-t", "=s"],
        vec!["list-sessions"],
        vec!["list-windows", "-t", "=s"],
        vec!["list-panes", "-s", "-t", "=s"],
        vec!["list-clients"],
        vec!["list-buffers"],
        vec!["display-message", "-p", "-t", "=s:", "#{pane_id}"],
        vec!["show-options", "-t", "s"],
        vec!["show-buffer", "-b", "fixture"],
        vec!["save-buffer", "-b", "fixture", "-"],
        vec!["capture-pane", "-p", "-t", "=s:"],
        vec!["show-environment", "-t", "=s"],
    ];
    for args in cases {
        assert!(f.ctl.read(&args).unwrap().ok(), "{args:?}");
        assert_eq!(snapshot(), before, "{args:?}");
    }
}

#[test]
fn stale_token_does_not_kill_a_recreated_session() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    let (_, token) = f
        .ctl
        .new_placeholder_session(&["new-session", "-d", "-s", "ph", "sleep", "30"])
        .unwrap();
    f.tmux.raw(&["kill-session", "-t", "=ph"]);
    f.tmux.new_session("ph", 80, 24);
    assert!(f.ctl.kill_owned_session(token.unwrap()).is_err());
    assert!(f.ctl.read(&["has-session", "-t", "=ph"]).unwrap().ok());
}

#[test]
fn placeholder_cannot_claim_an_existing_session() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.tmux.new_session("existing", 80, 24);
    for args in [
        vec!["new-session", "-Ad", "-s", "existing"],
        vec!["new-session", "-A", "-d", "-s", "existing"],
        vec!["new-session", "-d", "-s", "existing", "-s", "other"],
    ] {
        assert!(f.ctl.new_placeholder_session(&args).is_err());
    }
    assert!(
        f.ctl
            .read(&["has-session", "-t", "=existing"])
            .unwrap()
            .ok()
    );
}

#[test]
fn cold_sandbox_child() {
    let Some(marker) = std::env::var_os("COMANDOS_COLD_MARKER") else {
        return;
    };
    let f = TestTmux::cold_for_mode(RunMode::Sandbox).unwrap();
    assert!(
        f.ctl
            .mutate(&["new-session", "-d", "-s", "cold"])
            .unwrap()
            .ok()
    );
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert!(
        !std::path::Path::new(&marker).exists(),
        "se cargó la configuración personal sintética"
    );
    let home = f.ctl.read(&["show-environment", "-g", "HOME"]).unwrap();
    assert_eq!(
        home.stdout.trim(),
        format!(
            "HOME={}",
            f.config.sandbox_root().unwrap().join("home").display()
        )
    );
    assert!(
        !f.ctl
            .read(&["show-environment", "-g", "PERSONAL_SECRET"])
            .unwrap()
            .ok()
    );
    let shell = f
        .ctl
        .read(&[
            "display-message",
            "-p",
            "-t",
            "=cold:",
            "#{pane_current_command}",
        ])
        .unwrap();
    assert_eq!(shell.stdout.trim(), "sh");
}

#[test]
fn first_sandbox_server_does_not_load_personal_config_or_environment() {
    let outer = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    let marker = outer.config.home().join("config-was-loaded");
    std::fs::write(
        outer.config.home().join(".tmux.conf"),
        format!("run-shell 'touch {}'\n", marker.display()),
    )
    .unwrap();
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cold_sandbox_child", "--nocapture"])
        .env_clear()
        .env("HOME", outer.config.home())
        .env("SHELL", "/bin/sh")
        .env("PATH", "/usr/bin:/bin")
        .env("TERM", "xterm-256color")
        .env("PERSONAL_SECRET", "fake-value-never-inherited")
        .env("COMANDOS_COLD_MARKER", &marker)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn placeholder_identity_is_from_creation_even_if_replaced_by_hook() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    let hook = "set-hook -gu after-new-session; kill-session -t =ph; new-session -d -s ph sleep 30";
    let configured = f.tmux.raw(&["set-hook", "-g", "after-new-session", hook]);
    assert!(configured.status.success());
    let (out, token) = f
        .ctl
        .new_placeholder_session(&[
            "new-session",
            "-d",
            "-P",
            "-F",
            "#{pane_id}",
            "-s",
            "ph",
            "sleep",
            "30",
        ])
        .unwrap();
    assert!(out.ok());
    assert!(
        out.stdout.trim().starts_with('%'),
        "se conserva el formato del llamador"
    );
    assert!(
        f.ctl.kill_owned_session(token.unwrap()).is_err(),
        "el token no puede reclamar la sustituta"
    );
    assert!(f.ctl.read(&["has-session", "-t", "=ph"]).unwrap().ok());
}

#[test]
fn placeholder_keeps_the_callers_print_contract() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    for (name, print, format, expected) in [
        ("silent", false, None, ""),
        ("default", true, None, "default:\n"),
        (
            "custom",
            true,
            Some("#{session_name}|#{pane_id}"),
            "custom|",
        ),
    ] {
        let mut args = vec!["new-session", "-d", "-s", name];
        if print {
            args.push("-P");
        }
        if let Some(format) = format {
            args.extend(["-F", format]);
        }
        args.extend(["sleep", "30"]);
        let (out, owned) = f.ctl.new_placeholder_session(&args).unwrap();
        if format.is_some() {
            assert!(out.stdout.starts_with(expected));
        } else {
            assert_eq!(out.stdout, expected);
        }
        assert!(f.ctl.kill_owned_session(owned.unwrap()).unwrap().ok());
    }
    let argv = f.ctl.attach_argv("__keep");
    assert!(argv[2].contains("env -i HOME="));
    assert!(argv[2].contains("-f /dev/null attach"));
}
```

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/write_guard.rs`:
```rust
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
mod support;
use comandos_app::config::RunMode;
use comandos_app::guard::GuardError;
use support::tmux::TestTmux;

#[test]
fn shadow_guard_refuses_hooks_writes() {
    let Some(f) = TestTmux::for_mode(RunMode::Shadow) else {
        return;
    };
    let target = f.config.hooks_dir().join("app-tabs.json");
    assert!(matches!(
        f.guard.write_atomic(&target, b"[]", "app-tabs."),
        Err(GuardError::Shadow(_))
    ));
    assert!(matches!(
        f.guard.remove_file(&target),
        Err(GuardError::Shadow(_))
    ));
    assert!(!target.exists());
    // Lo único que la sombra escribe: su candado y su volcado de diseño.
    assert!(
        f.guard
            .write_atomic(&f.config.layout_dump_path(), b"{}", "layout.")
            .is_ok()
    );
}

#[test]
fn guard_writes_atomically_inside_roots() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    let path = f.config.hooks_dir().join("sub/app-tabs.json");
    f.guard
        .create_dir_all(path.parent().unwrap(), 0o700)
        .unwrap();
    f.guard
        .write_atomic(&path, "[\"ñandú\"]".as_bytes(), "app-tabs.")
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[\"ñandú\"]");
    let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "ningún temporal queda atrás");
}

#[test]
fn guard_refuses_dotdot_and_symlink_escape() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    let hooks = f.config.hooks_dir().to_path_buf();
    std::fs::create_dir_all(&hooks).unwrap();
    let outside = f.config.home().join("fuera");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, hooks.join("enlace")).unwrap();
    assert!(matches!(
        f.guard
            .write_atomic(&hooks.join("../../../home/x.json"), b"1", "x."),
        Err(GuardError::Escape(_) | GuardError::Outside(_))
    ));
    assert!(matches!(
        f.guard
            .write_atomic(&hooks.join("enlace/x.json"), b"1", "x."),
        Err(GuardError::Escape(_))
    ));
    assert!(!outside.join("x.json").exists());
    assert!(matches!(
        f.guard.write_atomic(
            &f.config.home().join(".claude/hooks/app-tabs.json"),
            b"1",
            "x."
        ),
        Err(GuardError::Outside(_))
    ));
}

#[test]
fn guard_rechecks_at_write_time() {
    // TOCTOU: la ruta era válida al construir la guarda; después alguien cambia
    // un directorio intermedio por un enlace que sale. La escritura lo detecta.
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else {
        return;
    };
    let dir = f.config.hooks_dir().join("cambia");
    f.guard.create_dir_all(&dir, 0o700).unwrap();
    f.guard
        .write_atomic(&dir.join("a.json"), b"1", "a.")
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let outside = f.config.home().join("robo");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, &dir).unwrap();
    assert!(matches!(
        f.guard.write_atomic(&dir.join("a.json"), b"2", "a."),
        Err(GuardError::Escape(_))
    ));
    assert!(!outside.join("a.json").exists());
}

#[test]
fn temporary_prefix_cannot_escape_the_parent() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let path = f.config.hooks_dir().join("app-tabs.json");
    for prefix in ["../evil", "/tmp/evil", "x/y", ".", "..", ""] {
        assert!(matches!(
            f.guard.write_atomic(&path, b"x", prefix),
            Err(GuardError::Escape(_))
        ));
    }
}

#[test]
fn live_guard_has_a_closed_file_list() {
    let f = TestTmux::for_mode(RunMode::Live).unwrap();
    let allowed = f.config.hooks_dir().join("app-tabs.json");
    f.guard.write_atomic(&allowed, b"[]", "tabs.").unwrap();
    for path in [
        f.config.hooks_dir().join("unrelated"),
        f.config.runtime_dir().join("other-service.sock"),
    ] {
        assert!(matches!(
            f.guard.write_atomic(&path, b"x", "x."),
            Err(GuardError::Outside(_))
        ));
    }
}

#[test]
fn final_symlink_is_never_followed() {
    let f = TestTmux::for_mode(RunMode::Sandbox).unwrap();
    f.guard.create_dir_all(f.config.hooks_dir(), 0o700).unwrap();
    let outside = f.config.home().join("outside");
    std::fs::write(&outside, "original").unwrap();
    let path = f.config.hooks_dir().join("app-tabs.json");
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(f.guard.append_if_exists(&path, b"x").is_err());
    assert!(f.guard.open_lock(&path).is_err());
    f.guard
        .write_atomic(&path, b"replacement", "tabs.")
        .unwrap();
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "original");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "replacement");
}
```

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/proc_jobs.rs`:
```rust
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
use comandos_app::jobs::Jobs;
use comandos_app::proc::{ProcSpec, run};
use std::time::{Duration, Instant};

fn spec(program: &str, args: &[&str], timeout_ms: u64) -> ProcSpec {
    ProcSpec {
        program: program.into(),
        args: args.iter().map(Into::into).collect(),
        stdin: None,
        env: Vec::new(),
        clear_env: false,
        env_remove: Vec::new(),
        cwd: None,
        timeout: Duration::from_millis(timeout_ms),
    }
}

#[test]
fn stdout_and_stderr_over_64k_do_not_deadlock() {
    // 1 MiB a stderr ANTES de escribir stdout: con lecturas en serie se bloquea.
    let s = spec(
        "sh",
        &[
            "-c",
            "head -c 1048576 /dev/zero >&2; head -c 1048576 /dev/zero",
        ],
        5000,
    );
    let out = run(&s).unwrap();
    assert_eq!((out.stdout.len(), out.stderr.len()), (1 << 20, 1 << 20));
    assert_eq!(out.code, Some(0));
}

#[test]
fn timeout_kills_the_child() {
    let started = Instant::now();
    let out = run(&spec("sleep", &["30"], 300)).unwrap();
    assert!(out.timed_out && out.code.is_none());
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn stdin_is_fed_concurrently() {
    let mut s = spec("cat", &[], 5000);
    s.stdin = Some(vec![b'y'; 1 << 20]);
    assert_eq!(run(&s).unwrap().stdout.len(), 1 << 20);
}

#[test]
fn jobs_deliver_on_the_main_context_two_at_a_time() {
    let ctx = glib::MainContext::new();
    let _guard = ctx.acquire().unwrap();
    let main_loop = glib::MainLoop::new(Some(&ctx), false);
    ctx.with_thread_default(|| {
        let jobs = Jobs::new(2, "prueba");
        let main_thread = std::thread::current().id();
        let done = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let started = Instant::now();
        for i in 0..4 {
            let done = done.clone();
            let ml = main_loop.clone();
            jobs.spawn(
                move || {
                    std::thread::sleep(Duration::from_millis(200));
                    i
                },
                move |v| {
                    assert_eq!(
                        std::thread::current().id(),
                        main_thread,
                        "done en el hilo de GLib"
                    );
                    done.borrow_mut().push(v);
                    if done.borrow().len() == 4 {
                        ml.quit();
                    }
                },
            );
        }
        main_loop.run();
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(390) && elapsed < Duration::from_millis(700),
            "{elapsed:?}"
        );
        let mut got = done.borrow().clone();
        got.sort_unstable();
        assert_eq!(got, [0, 1, 2, 3]);
    })
    .unwrap();
}

#[test]
fn descendants_cannot_hold_the_timeout_open() {
    let started = Instant::now();
    let out = run(&spec("sh", &["-c", "sleep 30 & wait"], 150)).unwrap();
    assert!(out.timed_out);
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn output_is_capped_while_the_rest_is_drained() {
    let out = run(&spec("head", &["-c", "17000000", "/dev/zero"], 5000)).unwrap();
    assert_eq!(out.stdout.len(), comandos_app::proc::OUTPUT_CAP);
    assert!(out.truncated);
    assert_eq!(out.code, Some(0));
}
```

- [ ] **Step 3: correrlas**

Run: `$C test -p comandos-app --test tmux_guard --test write_guard --test proc_jobs`
Expected: FAIL (no existen `tmux`, `guard`, `proc`, `jobs`).

- [ ] **Step 4: `clippy.toml` y `lib.rs`**

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/clippy.toml`:
```toml
# E/S de archivos solo por guard::WriteGuard; procesos solo por proc.rs (F11).
disallowed-methods = [
  { path = "std::fs::write", reason = "escribir solo con guard::WriteGuard" },
  { path = "std::fs::rename", reason = "escribir solo con guard::WriteGuard" },
  { path = "std::fs::remove_file", reason = "borrar solo con guard::WriteGuard" },
  { path = "std::fs::remove_dir", reason = "borrar solo con guard::WriteGuard" },
  { path = "std::fs::remove_dir_all", reason = "borrar solo con guard::WriteGuard" },
  { path = "std::fs::create_dir", reason = "crear solo con guard::WriteGuard" },
  { path = "std::fs::create_dir_all", reason = "crear solo con guard::WriteGuard" },
  { path = "std::fs::File::create", reason = "escribir solo con guard::WriteGuard" },
  { path = "std::fs::OpenOptions::open", reason = "abrir para escribir solo con guard::WriteGuard" },
  { path = "std::process::Command::new", reason = "procesos solo desde proc.rs" },
]
```

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/lib.rs`:
```rust
//! Escritorio GTK de ComandOS: tablero WebKit, pestañas de terminal sobre tmux y popups.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#![cfg_attr(
    test,
    allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]
pub mod config;
pub mod guard;
pub mod jobs;
pub mod proc;
pub mod tmux;
```

- [ ] **Step 5: implementar `proc.rs`**

```rust
//! Único sitio donde se lanzan procesos (clippy `disallowed-methods`). Lee stdout
//! y stderr a la vez y escribe stdin desde otro hilo: una salida de más de 64 KiB
//! (el búfer de una tubería) nunca bloquea al hijo (F08).
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Tope de lo que se guarda de cada salida.
pub const OUTPUT_CAP: usize = 16 << 20;

#[derive(Debug, Clone)]
pub struct ProcSpec {
    pub program: String,
    pub args: Vec<OsString>,
    pub stdin: Option<Vec<u8>>,
    pub env: Vec<(String, OsString)>,
    pub clear_env: bool,
    pub env_remove: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct ProcOutput {
    /// `None` si murió por señal o por el plazo.
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub truncated: bool,
}

#[derive(Debug)]
pub enum ProcError {
    Spawn(String),
}

fn command(program: &str) -> Command {
    // Única llamada permitida (con `spawn_detached`).
    #[allow(clippy::disallowed_methods)]
    Command::new(program)
}

fn drain(mut src: impl Read, deadline: Instant) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    let mut truncated = false;
    loop {
        if Instant::now() >= deadline {
            break;
        }
        match src.read(&mut buf) {
            Ok(0) => break,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
            Ok(n) => {
                let room = OUTPUT_CAP.saturating_sub(out.len());
                out.extend_from_slice(buf.get(..n.min(room)).unwrap_or_default());
                truncated |= n > room;
            }
        }
    }
    (out, truncated)
}

pub fn run(spec: &ProcSpec) -> Result<ProcOutput, ProcError> {
    use std::os::unix::process::CommandExt;
    let deadline = Instant::now()
        .checked_add(spec.timeout)
        .ok_or_else(|| ProcError::Spawn("plazo fuera de rango".into()))?;
    let mut cmd = command(&spec.program);
    cmd.process_group(0);
    cmd.args(&spec.args)
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if spec.clear_env {
        cmd.env_clear();
    }
    for key in &spec.env_remove {
        cmd.env_remove(key);
    }
    for (key, value) in &spec.env {
        cmd.env(key, value);
    }
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| ProcError::Spawn(format!("{}: {e}", spec.program)))?;
    let group = nix::unistd::Pid::from_raw(
        i32::try_from(child.id()).map_err(|e| ProcError::Spawn(e.to_string()))?,
    );
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    fn nonblocking(fd: &impl std::os::fd::AsFd) -> Result<(), ProcError> {
        use nix::fcntl::{FcntlArg, OFlag, fcntl};
        let flags = fcntl(fd, FcntlArg::F_GETFL).map_err(|e| ProcError::Spawn(e.to_string()))?;
        fcntl(
            fd,
            FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
        )
        .map(|_| ())
        .map_err(|e| ProcError::Spawn(e.to_string()))
    }
    let prepared = (|| {
        if let Some(p) = &stdin {
            nonblocking(p)?;
        }
        if let Some(p) = &stdout {
            nonblocking(p)?;
        }
        if let Some(p) = &stderr {
            nonblocking(p)?;
        }
        Ok::<(), ProcError>(())
    })();
    if let Err(error) = prepared {
        let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    std::thread::scope(|scope| {
        if let (Some(mut pipe), Some(data)) = (stdin, spec.stdin.as_deref()) {
            scope.spawn(move || {
                let mut rest = data;
                while !rest.is_empty() && Instant::now() < deadline {
                    match pipe.write(rest) {
                        Ok(0) => break,
                        Ok(n) => rest = rest.get(n..).unwrap_or_default(),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(2))
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
            });
        }
        let out = stdout.map(|p| scope.spawn(move || drain(p, deadline)));
        let err = stderr.map(|p| scope.spawn(move || drain(p, deadline)));
        let mut result = ProcOutput::default();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    result.code = status.code();
                    break;
                }
                Ok(None) if Instant::now() >= deadline => {
                    let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
                    let _ = child.kill();
                    let _ = child.wait();
                    result.timed_out = true;
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                Err(_) => {
                    let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
        // También cierra los pipes heredados por nietos cuando el líder termina.
        let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
        if let Some((bytes, cut)) = out.and_then(|h| h.join().ok()) {
            result.stdout = bytes;
            result.truncated |= cut;
        }
        if let Some((bytes, cut)) = err.and_then(|h| h.join().ok()) {
            result.stderr = bytes;
            result.truncated |= cut;
        }
        Ok(result)
    })
}

/// Lanza sin esperar (`xdg-open`, `nautilus`); un hilo recoge al hijo para que no
/// quede zombi. Usa un grupo de procesos separado; no crea una sesión POSIX.
pub fn spawn_detached(program: &str, args: &[OsString]) -> Result<(), ProcError> {
    use std::os::unix::process::CommandExt;
    let mut cmd = command(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = cmd
        .spawn()
        .map_err(|e| ProcError::Spawn(format!("{program}: {e}")))?;
    std::thread::Builder::new()
        .name("comandos-reap".into())
        .spawn(move || {
            let _ = child.wait();
        })
        .map(|_| ())
        .map_err(|e| ProcError::Spawn(e.to_string()))
}
```

- [ ] **Step 6: implementar `tmux.rs`**

```rust
//! Toda llamada a tmux de la app pasa por aquí, siempre con `-S <socket>`
//! explícito: nunca se cae por descuido en el servidor `default` del usuario.
//! El modo decide qué se puede hacer: la sombra solo lee.
use crate::config::{AppConfig, RunMode, TmuxServer};
use crate::guard::{GuardError, WriteGuard};
use crate::proc::{ProcSpec, run};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Plazo de `tmuxc` (`bin/cc-app:414`).
pub const TMUX_TIMEOUT: Duration = Duration::from_secs(5);

/// Verbos que no cambian nada (con las reglas de `check_read_args`).
pub const READ_VERBS: &[&str] = &[
    "has-session",
    "list-sessions",
    "list-windows",
    "list-panes",
    "list-clients",
    "list-buffers",
    "display-message",
    "show-options",
    "show-buffer",
    "save-buffer",
    "capture-pane",
    "show-environment",
];

/// Verbos que la app usa para cambiar tmux (solo fuera de la sombra). Ni
/// `kill-server` ni `kill-session`: el único borrado es `kill_owned_session`.
pub const MUTATE_VERBS: &[&str] = &[
    "new-session",
    "new-window",
    "split-window",
    "send-keys",
    "select-pane",
    "select-window",
    "select-layout",
    "resize-pane",
    "resize-window",
    "set-option",
    "load-buffer",
    "paste-buffer",
    "delete-buffer",
    "respawn-pane",
    "move-window",
];

/// Shells cuya presencia exclusiva hace «ociosa» una sesión `term-*` (`close_tab`, 3386).
const IDLE_SHELLS: &[&str] = &["zsh", "bash", "sh", "fish"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxError {
    ShadowRefused(String),
    Forbidden(String),
    BadArgs(String),
    Spawn(String),
    Timeout(String),
}

/// Como `subprocess.CompletedProcess` del Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxOut {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl TmuxOut {
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

/// Prueba de que una sesión se puede borrar. Solo la crean `idle_scratch` y
/// `new_placeholder_session`; no se puede construir desde fuera.
#[derive(Debug)]
pub struct OwnedSession {
    name: String,
    socket: PathBuf,
    identity: String,
    idle: bool,
}

#[derive(Debug, Clone)]
pub struct TmuxCtl {
    mode: RunMode,
    socket: PathBuf,
    sandbox_home: Option<PathBuf>,
}

fn valid_session_name(name: &str) -> bool {
    // SESSION_RE de bin/cc-app:3572 (el mismo que cc-dash).
    !name.is_empty()
        && name.len() <= 80
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn valid_target(t: &str) -> bool {
    let id = |p: char| {
        t.strip_prefix(p)
            .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
    };
    if id('%') || id('@') || id('$') {
        return true;
    }
    let body = t.strip_prefix('=').unwrap_or(t);
    let (sess, rest) = match body.split_once(':') {
        Some((s, r)) => (s, Some(r)),
        None => (body, None),
    };
    valid_session_name(sess)
        && rest.is_none_or(|r| {
            r.is_empty()
                || r == "^"
                || r.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        })
}

/// Reglas de lectura (F07, F09): sin `#(`, `-p` obligatorio donde sin él se
/// muta, `save-buffer` solo a stdout, objetivos `-t` válidos.
pub fn check_read_args(args: &[&str]) -> Result<(), TmuxError> {
    let verb = *args
        .first()
        .ok_or_else(|| TmuxError::BadArgs("sin verbo".into()))?;
    if !READ_VERBS.contains(&verb) {
        return Err(TmuxError::Forbidden(verb.to_string()));
    }
    if let Some(bad) = args.iter().find(|a| a.contains("#(")) {
        return Err(TmuxError::BadArgs(format!(
            "formato que ejecuta comandos: {bad}"
        )));
    }
    reject_command_chain(args)?;
    if matches!(verb, "display-message" | "capture-pane") {
        let mut printed = false;
        let mut it = args.iter().skip(1);
        while let Some(arg) = it.next() {
            match *arg {
                "-p" => printed = true,
                "-t" | "-c" | "-F" | "-b" | "-S" | "-E" => {
                    it.next()
                        .ok_or_else(|| TmuxError::BadArgs(format!("{arg} sin valor")))?;
                }
                "-a" | "-v" | "-l" | "-C" | "-e" | "-J" | "-N" | "-P" | "-q" => {}
                other if other.starts_with('-') => {
                    return Err(TmuxError::BadArgs(format!(
                        "opción de lectura desconocida: {other}"
                    )));
                }
                _ => break,
            }
        }
        if !printed {
            return Err(TmuxError::BadArgs(format!("{verb} sin -p")));
        }
    }
    if verb == "save-buffer" && args.last() != Some(&"-") {
        return Err(TmuxError::BadArgs("save-buffer solo a stdout (-)".into()));
    }
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if *a == "-t" {
            let target = it
                .next()
                .ok_or_else(|| TmuxError::BadArgs("-t sin valor".into()))?;
            if !valid_target(target) {
                return Err(TmuxError::BadArgs(format!("objetivo no válido: {target}")));
            }
        }
    }
    Ok(())
}

// tmux interpreta separadores y llaves como listas de comandos incluso sin shell.
fn reject_command_chain(args: &[&str]) -> Result<(), TmuxError> {
    if args
        .iter()
        .any(|a| a.contains(';') || matches!(*a, "{" | "}"))
    {
        return Err(TmuxError::BadArgs("lista de comandos tmux".into()));
    }
    Ok(())
}

/// `TMUX_TMPDIR` como lo usa tmux 3.2a: si no es un directorio existente, `/tmp`.
fn tmux_tmpdir(env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    env("TMUX_TMPDIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

impl TmuxCtl {
    /// Único constructor: lee el modo y el servidor de la configuración.
    pub fn from_config(
        cfg: &AppConfig,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<TmuxCtl, TmuxError> {
        let uid = nix::unistd::getuid().as_raw();
        let user_dir = tmux_tmpdir(env).join(format!("tmux-{uid}"));
        let socket = match (cfg.mode(), cfg.tmux_server(), cfg.sandbox_root()) {
            (RunMode::Sandbox, TmuxServer::Private(label), Some(root)) => {
                root.join("tmux").join(label.as_str())
            }
            (RunMode::Sandbox, _, _) => {
                return Err(TmuxError::Forbidden("sandbox sin socket propio".into()));
            }
            (_, TmuxServer::User, _) => user_dir.join("default"),
            (_, TmuxServer::Private(label), _) => user_dir.join(label.as_str()),
        };
        if cfg.mode() == RunMode::Sandbox
            && (socket.starts_with(&user_dir) || socket.starts_with(format!("/tmp/tmux-{uid}")))
        {
            return Err(TmuxError::Forbidden(format!(
                "el socket del sandbox no puede vivir con los del usuario: {}",
                socket.display()
            )));
        }
        Ok(TmuxCtl {
            mode: cfg.mode(),
            socket,
            sandbox_home: cfg.sandbox_root().map(|root| root.join("home")),
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    pub fn mode(&self) -> RunMode {
        self.mode
    }

    /// Crea `<raíz>/tmux` (0700, como tmux exige) en sandbox; en otro modo no hace nada.
    pub fn prepare_socket_dir(&self, guard: &WriteGuard) -> Result<(), GuardError> {
        match (self.mode, self.socket.parent()) {
            (RunMode::Sandbox, Some(dir)) => {
                guard.create_dir_all(dir, 0o700)?;
                if let Some(home) = &self.sandbox_home {
                    guard.create_dir_all(home, 0o700)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn exec(&self, args: &[&str], stdin: Option<&[u8]>) -> Result<TmuxOut, TmuxError> {
        let mut argv: Vec<OsString> = vec!["-S".into(), self.socket.clone().into_os_string()];
        if self.mode == RunMode::Sandbox {
            argv.extend(["-f".into(), "/dev/null".into()]);
        }
        argv.extend(args.iter().map(OsString::from));
        let spec = ProcSpec {
            program: "tmux".into(),
            args: argv,
            stdin: stdin.map(<[u8]>::to_vec),
            env: self.sandbox_home.as_ref().map_or_else(Vec::new, |home| {
                vec![
                    ("HOME".into(), home.clone().into_os_string()),
                    ("PATH".into(), "/usr/bin:/bin".into()),
                    ("SHELL".into(), "/bin/sh".into()),
                    ("TERM".into(), "xterm-256color".into()),
                    (
                        "XDG_CONFIG_HOME".into(),
                        home.join(".config").into_os_string(),
                    ),
                ]
            }),
            clear_env: self.mode == RunMode::Sandbox,
            env_remove: vec!["TMUX".into(), "TMUX_PANE".into()],
            cwd: None,
            timeout: TMUX_TIMEOUT,
        };
        let out = run(&spec).map_err(|e| TmuxError::Spawn(format!("{e:?}")))?;
        if out.timed_out {
            return Err(TmuxError::Timeout(args.join(" ")));
        }
        Ok(TmuxOut {
            code: out.code.unwrap_or(1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    pub fn read(&self, args: &[&str]) -> Result<TmuxOut, TmuxError> {
        check_read_args(args)?;
        self.exec(args, None)
    }

    fn check_mutate(&self, args: &[&str]) -> Result<(), TmuxError> {
        let verb = *args
            .first()
            .ok_or_else(|| TmuxError::BadArgs("sin verbo".into()))?;
        if !MUTATE_VERBS.contains(&verb) {
            return Err(TmuxError::Forbidden(verb.to_string()));
        }
        if self.mode == RunMode::Shadow {
            return Err(TmuxError::ShadowRefused(verb.to_string()));
        }
        reject_command_chain(args)?;
        if let Some(bad) = args.iter().find(|a| a.contains("#(")) {
            return Err(TmuxError::BadArgs(format!(
                "formato que ejecuta comandos: {bad}"
            )));
        }
        Ok(())
    }

    pub fn mutate(&self, args: &[&str]) -> Result<TmuxOut, TmuxError> {
        self.check_mutate(args)?;
        self.exec(args, None)
    }

    /// `load-buffer -` con el texto por stdin (`snip_paste`, F38).
    pub fn mutate_with_stdin(&self, args: &[&str], stdin: &[u8]) -> Result<TmuxOut, TmuxError> {
        self.check_mutate(args)?;
        self.exec(args, Some(stdin))
    }

    /// `Some` solo si `session` es `term-*` y todos sus panes son shells ociosos.
    pub fn idle_scratch(&self, session: &str) -> Result<Option<OwnedSession>, TmuxError> {
        if !session.starts_with("term-") || !valid_session_name(session) {
            return Ok(None);
        }
        let target = format!("={session}");
        let out = self.read(&[
            "list-panes",
            "-s",
            "-t",
            &target,
            "-F",
            "#{pane_current_command}",
        ])?;
        let cmds: Vec<&str> = out.stdout.split_whitespace().collect();
        if out.ok() && !cmds.is_empty() && cmds.iter().all(|c| IDLE_SHELLS.contains(c)) {
            self.owned(session, true).map(Some)
        } else {
            Ok(None)
        }
    }

    fn owned(&self, name: &str, idle: bool) -> Result<OwnedSession, TmuxError> {
        let target = format!("={name}:");
        let out = self.read(&[
            "display-message",
            "-p",
            "-t",
            &target,
            "#{pid}:#{session_id}",
        ])?;
        if !out.ok() || out.stdout.trim().is_empty() {
            return Err(TmuxError::BadArgs("la sesión ya no existe".into()));
        }
        Ok(OwnedSession {
            name: name.into(),
            socket: self.socket.clone(),
            identity: out.stdout.trim().into(),
            idle,
        })
    }

    /// `new-session` de la restauración: devuelve la prueba de propiedad si tmux
    /// la creó (el nombre sale del `-s` de `args`).
    pub fn new_placeholder_session(
        &self,
        args: &[&str],
    ) -> Result<(TmuxOut, Option<OwnedSession>), TmuxError> {
        if args.first() != Some(&"new-session") {
            return Err(TmuxError::BadArgs(
                "new_placeholder_session solo crea sesiones".into(),
            ));
        }
        self.check_mutate(args)?;
        // -P/-F internos capturan identidad en la misma creación. Nunca buscar
        // por nombre después: un hook puede reemplazar la sesión antes de volver.
        let mut options = args.iter().skip(1);
        let mut session = None;
        let mut caller_print = false;
        let mut internal_format_added = false;
        let mut caller_format = "#{session_name}:";
        let mut argv: Vec<String> = vec!["new-session".into()];
        while let Some(arg) = options.next() {
            match *arg {
                "-P" => caller_print = true,
                "-F" => {
                    caller_format = options
                        .next()
                        .ok_or_else(|| TmuxError::BadArgs("-F sin valor".into()))?
                }
                "-d" | "-E" => argv.push((*arg).into()),
                "-s" => {
                    let name = options
                        .next()
                        .filter(|n| valid_session_name(n))
                        .ok_or_else(|| TmuxError::BadArgs("new-session sin -s válido".into()))?;
                    if session.replace((*name).to_string()).is_some() {
                        return Err(TmuxError::BadArgs("-s duplicado".into()));
                    }
                    argv.extend([(*arg).to_string(), (*name).to_string()]);
                }
                "-x" | "-y" | "-n" | "-c" | "-e" => {
                    let value = options
                        .next()
                        .ok_or_else(|| TmuxError::BadArgs(format!("{arg} sin valor")))?;
                    argv.extend([(*arg).to_string(), (*value).to_string()]);
                }
                other if other.starts_with('-') => {
                    return Err(TmuxError::BadArgs(format!(
                        "opción no permitida de placeholder: {other}"
                    )));
                }
                _ => {
                    // Las opciones internas van antes del programa y sus argumentos.
                    argv.extend([
                        "-P".into(),
                        "-F".into(),
                        format!("#{{pid}}:#{{session_id}}|{caller_format}"),
                    ]);
                    internal_format_added = true;
                    argv.push((*arg).into());
                    argv.extend(options.map(|value| (*value).to_string()));
                    break;
                }
            }
        }
        if !internal_format_added {
            argv.extend([
                "-P".into(),
                "-F".into(),
                format!("#{{pid}}:#{{session_id}}|{caller_format}"),
            ]);
        }
        let name = session.ok_or_else(|| TmuxError::BadArgs("new-session sin -s válido".into()))?;
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        let mut out = self.exec(&refs, None)?;
        let owned = if out.ok() {
            let (identity, requested) = out.stdout.split_once('|').ok_or_else(|| {
                TmuxError::BadArgs(format!(
                    "new-session no entregó identidad: {:?}",
                    out.stdout
                ))
            })?;
            let valid = identity.split_once(':').is_some_and(|(pid, id)| {
                !pid.is_empty()
                    && pid.bytes().all(|b| b.is_ascii_digit())
                    && id.starts_with('$')
                    && valid_target(id)
            });
            if !valid {
                return Err(TmuxError::BadArgs("identidad de creación inválida".into()));
            }
            let token = OwnedSession {
                name,
                socket: self.socket.clone(),
                identity: identity.into(),
                idle: false,
            };
            out.stdout = if caller_print {
                requested.to_string()
            } else {
                String::new()
            };
            Some(token)
        } else {
            None
        };
        Ok((out, owned))
    }

    pub fn kill_owned_session(&self, owned: OwnedSession) -> Result<TmuxOut, TmuxError> {
        if self.mode == RunMode::Shadow {
            return Err(TmuxError::ShadowRefused("kill-session".into()));
        }
        if owned.socket != self.socket {
            return Err(TmuxError::Forbidden("token de otro servidor".into()));
        }
        let current = self.owned(&owned.name, owned.idle)?;
        if current.identity != owned.identity {
            return Err(TmuxError::Forbidden(
                "sesión reemplazada desde la prueba de propiedad".into(),
            ));
        }
        if owned.idle && self.idle_scratch(&owned.name)?.is_none() {
            return Err(TmuxError::Forbidden(
                "la sesión dejó de estar ociosa".into(),
            ));
        }
        let target = owned
            .identity
            .split_once(':')
            .map(|(_, id)| id)
            .filter(|id| valid_target(id))
            .ok_or_else(|| TmuxError::BadArgs("identidad de sesión inválida".into()))?;
        self.exec(&["kill-session", "-t", target], None)
    }

    /// Mismo comando que `open_tab` (3611) con `-S`; la sombra se engancha en
    /// solo lectura y sin cambiar el tamaño de la sesión.
    pub fn attach_argv(&self, session: &str) -> Vec<String> {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        let flags = if self.mode == RunMode::Shadow {
            "-f read-only,ignore-size "
        } else {
            ""
        };
        let sandbox_prefix = self.sandbox_home.as_ref().map_or_else(String::new, |home| format!(
            "env -i HOME={} PATH=/usr/bin:/bin SHELL=/bin/sh TERM=xterm-256color XDG_CONFIG_HOME={} ",
            quote(&home.display().to_string()), quote(&home.join(".config").display().to_string())
        ));
        let isolated_config = if self.mode == RunMode::Sandbox {
            "-f /dev/null "
        } else {
            ""
        };
        let attach = format!(
            "{sandbox_prefix}tmux -S {} {isolated_config}attach {flags}-t {} || {{ echo '[sesion terminada — cierra esta pestana con la x]'; exec cat; }}",
            quote(&self.socket.display().to_string()),
            quote(&format!("={session}")),
        );
        vec!["/bin/sh".into(), "-c".into(), attach]
    }

    /// `_tmux_window_size` (749): tamaño que la sesión ya tiene, si es razonable.
    pub fn window_size(&self, session: &str) -> Option<(u16, u16)> {
        let target = format!("={session}:");
        let out = self
            .read(&[
                "display-message",
                "-p",
                "-t",
                &target,
                "#{window_width} #{window_height}",
            ])
            .ok()?;
        let mut it = out.stdout.split_whitespace().map(str::parse::<u16>);
        let (cols, rows) = (it.next()?.ok()?, it.next()?.ok()?);
        (cols >= 20 && rows >= 5).then_some((cols, rows))
    }
}
```

- [ ] **Step 7: implementar `guard.rs`**

```rust
//! Toda escritura de archivos de la app. La ruta se vuelve a resolver en el
//! momento de escribir, componente a componente desde una raíz canónica con
//! `O_NOFOLLOW`: un enlace o un `..` metido después de `parse_args` no saca la
//! escritura de su raíz (TOCTOU). La sombra solo escribe su candado y su volcado.
use crate::config::{AppConfig, RunMode};
use nix::fcntl::{OFlag, open, openat, renameat};
use nix::sys::stat::{Mode, mkdirat};
use nix::unistd::unlinkat;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub enum GuardError {
    Shadow(PathBuf),
    Outside(PathBuf),
    Escape(PathBuf),
    Io(PathBuf, String),
}

#[derive(Debug, Clone)]
pub struct WriteGuard {
    mode: RunMode,
    /// (ruta configurada, ruta canónica).
    roots: Vec<(PathBuf, PathBuf)>,
    /// Archivos sueltos permitidos (candados, volcado de la sombra, log opt-in).
    files: Vec<PathBuf>,
}

const DIR_FLAGS: OFlag = OFlag::O_DIRECTORY
    .union(OFlag::O_NOFOLLOW)
    .union(OFlag::O_CLOEXEC);

fn io(path: &Path, e: impl std::fmt::Display) -> GuardError {
    GuardError::Io(path.to_path_buf(), e.to_string())
}

fn normal_parts(rel: &Path, whole: &Path) -> Result<Vec<OsString>, GuardError> {
    rel.components()
        .map(|c| match c {
            Component::Normal(n) => Ok(n.to_os_string()),
            _ => Err(GuardError::Escape(whole.to_path_buf())),
        })
        .collect()
}

/// Abre `dir` desde `/` sin seguir ningún enlace.
fn open_dir_nofollow(dir: &Path) -> Result<OwnedFd, GuardError> {
    let mut fd = open("/", DIR_FLAGS, Mode::empty()).map_err(|e| io(dir, e))?;
    let rel = dir
        .strip_prefix("/")
        .map_err(|_| GuardError::Escape(dir.to_path_buf()))?;
    for part in normal_parts(rel, dir)? {
        fd = openat(&fd, part.as_os_str(), DIR_FLAGS, Mode::empty())
            .map_err(|_| GuardError::Escape(dir.to_path_buf()))?;
    }
    Ok(fd)
}

impl WriteGuard {
    /// `display` decide el nombre del candado de instancia única (T7).
    pub fn from_config(cfg: &AppConfig, display: &str) -> WriteGuard {
        let lock = cfg.lock_file_name(display);
        let uid = nix::unistd::getuid().as_raw();
        let mut files = Vec::new();
        match cfg.mode() {
            RunMode::Shadow => {
                files.push(cfg.runtime_dir().join(&lock));
                files.push(cfg.layout_dump_path());
            }
            RunMode::Live => {
                // Candados de _base del Python (52): XDG_RUNTIME_DIR, /tmp/comandos-<uid>, /tmp.
                files.push(cfg.runtime_dir().join(&lock));
                files.push(PathBuf::from(format!("/tmp/comandos-{uid}")).join(&lock));
                files.push(PathBuf::from("/tmp").join(&lock));
                // Log opt-in de snip_log (472): solo se anexa si ya existe.
                files.push(PathBuf::from("/tmp/cc-app-snip.log"));
                for name in [
                    "app-tabs.json",
                    "app-tabs.json.lock",
                    "app-tabs-history.json",
                    "app-tabs-snapshot.json",
                    "app-sessions-v2.json",
                    "app-tab-active.json",
                    "app-tab-models.json",
                    "app-extension-shelf.json",
                    "app-layout.json",
                    "app-pane-position.json",
                    "app-focus.json",
                    "app-tab-close.json",
                    "app-tab-open.json",
                    "app-command.json",
                    "snippets.json",
                    "acp-panes.json",
                ] {
                    files.push(cfg.hooks_dir().join(name));
                }
            }
            RunMode::Sandbox => {}
        }
        let roots = match cfg.mode() {
            // Ya resueltas y validadas por parse_args (0700, del usuario, fuera de estado real).
            RunMode::Sandbox => [cfg.sandbox_root(), cfg.sandbox_temp()]
                .into_iter()
                .flatten()
                .map(|p| (p.to_path_buf(), p.to_path_buf()))
                .collect(),
            RunMode::Shadow => Vec::new(),
            RunMode::Live => Vec::new(),
        };
        WriteGuard {
            mode: cfg.mode(),
            roots,
            files,
        }
    }

    /// (descriptor del directorio padre, nombre final) tras recorrer sin enlaces.
    fn resolve_parent(
        &self,
        path: &Path,
        create_dirs: bool,
    ) -> Result<(OwnedFd, OsString), GuardError> {
        if path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(GuardError::Escape(path.to_path_buf()));
        }
        let name = path
            .file_name()
            .ok_or_else(|| GuardError::Escape(path.to_path_buf()))?
            .to_os_string();
        let parent = path
            .parent()
            .ok_or_else(|| GuardError::Escape(path.to_path_buf()))?;
        if self.files.iter().any(|f| f == path) {
            return Ok((open_dir_nofollow(parent)?, name));
        }
        if self.mode == RunMode::Shadow {
            return Err(GuardError::Shadow(path.to_path_buf()));
        }
        let (configured, canonical) = self
            .roots
            .iter()
            .find(|(conf, canon)| parent.starts_with(conf) || parent.starts_with(canon))
            .ok_or_else(|| GuardError::Outside(path.to_path_buf()))?;
        let rel = parent
            .strip_prefix(configured)
            .or_else(|_| parent.strip_prefix(canonical))
            .map_err(|_| GuardError::Outside(path.to_path_buf()))?;
        let mut fd = open_dir_nofollow(canonical)?;
        for part in normal_parts(rel, path)? {
            fd = match openat(&fd, part.as_os_str(), DIR_FLAGS, Mode::empty()) {
                Ok(next) => next,
                Err(nix::errno::Errno::ENOENT) if create_dirs => {
                    mkdirat(&fd, part.as_os_str(), Mode::from_bits_truncate(0o700))
                        .map_err(|e| io(path, e))?;
                    openat(&fd, part.as_os_str(), DIR_FLAGS, Mode::empty())
                        .map_err(|_| GuardError::Escape(path.to_path_buf()))?
                }
                Err(_) => return Err(GuardError::Escape(path.to_path_buf())),
            };
        }
        Ok((fd, name))
    }

    pub fn check(&self, path: &Path) -> Result<(), GuardError> {
        self.resolve_parent(path, false).map(|_| ())
    }

    /// Temporal `<prefijo><aleatorio>.tmp` en el mismo directorio + `renameat`
    /// (como `tempfile.mkstemp` + `os.replace` del Python).
    pub fn write_atomic(
        &self,
        path: &Path,
        bytes: &[u8],
        tmp_prefix: &str,
    ) -> Result<(), GuardError> {
        if !plain_name(OsStr::new(tmp_prefix)) {
            return Err(GuardError::Escape(path.to_path_buf()));
        }
        let (dir, name) = self.resolve_parent(path, false)?;
        let mut rnd = [0u8; 8];
        getrandom::fill(&mut rnd).map_err(|e| io(path, e))?;
        let tmp: OsString = format!(
            "{tmp_prefix}{}.tmp",
            rnd.iter().map(|b| format!("{b:02x}")).collect::<String>()
        )
        .into();
        let flags =
            OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_WRONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        let fd = openat(
            &dir,
            tmp.as_os_str(),
            flags,
            Mode::from_bits_truncate(0o600),
        )
        .map_err(|e| io(path, e))?;
        let mut file = std::fs::File::from(fd);
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        let renamed = written.map_err(|e| io(path, e)).and_then(|()| {
            renameat(&dir, tmp.as_os_str(), &dir, name.as_os_str()).map_err(|e| io(path, e))
        });
        if renamed.is_err() {
            let _ = unlinkat(
                &dir,
                tmp.as_os_str(),
                nix::unistd::UnlinkatFlags::NoRemoveDir,
            );
        }
        renamed
    }

    /// Anexa solo si el archivo ya existe (`snip_log`, 495). `Ok(false)` si no existe.
    pub fn append_if_exists(&self, path: &Path, bytes: &[u8]) -> Result<bool, GuardError> {
        let (dir, name) = self.resolve_parent(path, false)?;
        let flags = OFlag::O_WRONLY | OFlag::O_APPEND | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        match openat(&dir, name.as_os_str(), flags, Mode::empty()) {
            Ok(fd) => std::fs::File::from(fd)
                .write_all(bytes)
                .map(|()| true)
                .map_err(|e| io(path, e)),
            Err(nix::errno::Errno::ENOENT) => Ok(false),
            Err(e) => Err(io(path, e)),
        }
    }

    /// Consumir un archivo IPC (`on_app_command`, 8968). Nunca en sombra.
    pub fn remove_file(&self, path: &Path) -> Result<(), GuardError> {
        let (dir, name) = self.resolve_parent(path, false)?;
        unlinkat(
            &dir,
            name.as_os_str(),
            nix::unistd::UnlinkatFlags::NoRemoveDir,
        )
        .map_err(|e| io(path, e))
    }

    /// `os.makedirs(..., exist_ok=True)` dentro de una raíz, sin seguir enlaces.
    pub fn create_dir_all(&self, path: &Path, mode: u32) -> Result<(), GuardError> {
        let (dir, name) = self.resolve_parent(path, true)?;
        match mkdirat(&dir, name.as_os_str(), Mode::from_bits_truncate(mode)) {
            Ok(()) | Err(nix::errno::Errno::EEXIST) => {}
            Err(e) => return Err(io(path, e)),
        }
        openat(&dir, name.as_os_str(), DIR_FLAGS, Mode::empty())
            .map(|_| ())
            .map_err(|_| GuardError::Escape(path.to_path_buf()))
    }

    /// Abre (creándolo) un archivo de candado para `flock`.
    pub fn open_lock(&self, path: &Path) -> Result<std::fs::File, GuardError> {
        let (dir, name) = self.resolve_parent(path, false)?;
        let flags = OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        openat(
            &dir,
            name.as_os_str(),
            flags,
            Mode::from_bits_truncate(0o600),
        )
        .map(std::fs::File::from)
        .map_err(|e| io(path, e))
    }
}

/// `true` si `name` es un nombre de componente sin separadores (para los llamadores
/// que componen nombres de archivo a partir de datos externos).
pub fn plain_name(name: &OsStr) -> bool {
    let s = name.as_encoded_bytes();
    !s.is_empty() && !s.contains(&b'/') && s != b"." && s != b".."
}
```
`getrandom` entra como dependencia del crate (`getrandom.workspace = true` en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/Cargo.toml`).

- [ ] **Step 8: implementar `jobs.rs`**

```rust
//! Trabajo fuera del hilo de GTK. `Jobs` es un grupo de hilos fijos para tareas
//! cortas (HTTP ≤ 15 s, tmux ≤ 5 s); los bucles largos (`poll_state_loop`,
//! `ws_poll_loop`, `tmux_clipboard_loop`, esperas de cuenta) van en su propio hilo
//! con `spawn_loop`. El resultado vuelve al bucle de GLib por `async_channel`.
use std::sync::{Arc, Mutex, mpsc};

type Work = Box<dyn FnOnce() + Send + 'static>;

#[derive(Clone)]
pub struct Jobs {
    tx: mpsc::Sender<Work>,
}

impl Jobs {
    pub fn new(workers: usize, name: &str) -> Jobs {
        let (tx, rx) = mpsc::channel::<Work>();
        let rx = Arc::new(Mutex::new(rx));
        for i in 0..workers.max(2) {
            let rx = Arc::clone(&rx);
            let _ = std::thread::Builder::new()
                .name(format!("comandos-{name}-{i}"))
                .spawn(move || {
                    loop {
                        let next = match rx.lock() {
                            Ok(guard) => guard.recv(),
                            Err(_) => break,
                        };
                        match next {
                            Ok(work) => work(),
                            Err(_) => break,
                        }
                    }
                });
        }
        Jobs { tx }
    }

    /// `work` en un hilo del grupo; `done(resultado)` después, en el hilo de GLib.
    pub fn spawn<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(T) + 'static,
    ) {
        let back = to_main(done);
        let _ = self.tx.send(Box::new(move || {
            let _ = back.send_blocking(work());
        }));
    }
}

/// Canal hacia el hilo de GLib: lo que se envíe llega a `done` una vez.
pub fn to_main<T: Send + 'static>(done: impl FnOnce(T) + 'static) -> async_channel::Sender<T> {
    let (tx, rx) = async_channel::bounded::<T>(1);
    glib::MainContext::ref_thread_default().spawn_local(async move {
        if let Ok(value) = rx.recv().await {
            done(value);
        }
    });
    tx
}

/// Hilo propio con nombre para un bucle largo (equivale a `threading.Thread(daemon=True)`).
pub fn spawn_loop(name: &str, body: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name(format!("comandos-{name}"))
        .spawn(body)
        .map(|_| ())
}
```

- [ ] **Step 9: pruebas**

Run: `$C test -p comandos-app --test tmux_guard --test write_guard --test proc_jobs`
Expected: PASS (las de tmux se saltan con aviso si `tmux` no está; en esta máquina está). Después `$C clippy -p comandos-app --all-targets -j2 -- -D warnings`: limpio, y `grep -rn "Command::new" crates/comandos-app/src` solo devuelve `proc.rs`.

- [ ] **Step 10: commit**

```bash
git add crates/comandos-app/clippy.toml crates/comandos-app/Cargo.toml crates/comandos-app/src/lib.rs \
  crates/comandos-app/src/tmux.rs crates/comandos-app/src/guard.rs \
  crates/comandos-app/src/proc.rs crates/comandos-app/src/jobs.rs crates/comandos-app/tests/support \
  crates/comandos-app/tests/tmux_guard.rs crates/comandos-app/tests/write_guard.rs \
  crates/comandos-app/tests/proc_jobs.rs
git commit -m "feat(app): tmux con -S y lista blanca, escrituras que se resuelven al escribir, procesos con plazo y trabajos a GLib
"
```

### Task 4: motor del terminal sobre `comandos-term`, colores de VTE y plan de pintado por fila

**Depende de:** T1 y Fase 3 fusionada. **Paralelizable:** sí, con T2, T3, T8, T19.

Reescrita sobre la API real de `comandos-term` (F04, F06, F22, F23, F26). No pinta nada todavía: produce un plan de operaciones por fila, puro y probado sin pantalla, que T6 ejecuta con cairo + pango.

**Files:**
- Modify: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/Cargo.toml` (`comandos-term = { path = "../comandos-term" }`, `alacritty_terminal = { version = "=0.26.0", default-features = false }`), `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/lib.rs` (`pub mod term;`)
- Create: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/{mod,engine,paint}.rs`
- Test: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/term_paint.rs`

**Interfaces:**
- Consumes: `comandos_term::{engine, render, glyphs, input, select}` (ver «Interfaz real de `comandos-term`»).
- Produces:
  - `term::engine`: re-exportaciones (`Engine`, `GridSize`, `Palette`, `Modes`, `MouseMode`, `Damage`, `Drained`, `ClipboardTarget`, `RenderOpts`, `RowRender`, `Run`, `RunKind`, `Style`, `Underline`, `CursorShape`, `CursorView`, `CellMetrics`, `DrawOp`, `draw_ops`, `KeyInput`, `KeyAction`, `encode_key`, `encode_paste`, `Button`, `MouseKind`, `encode_mouse`, `WheelAction`, `wheel`, `focus`, `Ime`, `Point`, `SelectMode`, `Selection`, `UrlSpan`, `selected_text`, `selection_bounds`, `word_at`, `find_urls`) y `TermEngine`:
    - `TermEngine::new(cols: u16, rows: u16, scrollback: usize, palette: Palette, blink: bool, epoch: Instant) -> TermEngine`.
    - `feed(&mut self, bytes: &[u8], now: Instant)`, `tick(&mut self, now: Instant) -> bool`, `next_deadline(&self) -> Option<Instant>`.
    - `take_dirty(&mut self, lines: &mut Vec<usize>) -> bool` (`true` = repintar todo).
    - `render_line(&self, line: usize, out: &mut RowRender)` (con `VTE_OPTS`), `cursor(&self) -> CursorView`, `modes(&self) -> Modes`, `drain(&mut self) -> Drained`.
    - `resize(&mut self, cols: u16, rows: u16, cell_px: (u16, u16))`, `set_palette(&mut self, palette: Palette)`, `palette(&self) -> &Palette`, `scroll(&mut self, lines: i32)`, `size(&self) -> (u16, u16)`, `engine(&self) -> &Engine`.
    - `fg_is_rgb(&self, line: usize, col: u16) -> bool`.
  - Constantes: `SCROLLBACK = 10_000` (`make_term`, 795), `MOSAIC_SCROLLBACK = 400` (celdas del mosaico, `_mosaic_term`), `VTE_OPTS: RenderOpts = { bold_is_bright: false, min_contrast: 1.0 }`.
  - `term::paint`:
    - `hex_rgb(h: &str) -> Option<[u8; 3]>`.
    - `palette_from_theme(fg: &str, bg: &str, cursor: &str, pal16: &[&str]) -> Option<Palette>`.
    - `vte_dim(c: [u8; 3]) -> [u8; 3]`.
    - `CellGeom { cell_w: f64, cell_h: f64, origin_x: f64, origin_y: f64, dpr: f64, font_size: f64 }`.
    - `enum PaintOp { Rect { x: f64, y: f64, w: f64, h: f64, rgb: [u8; 3] }, Text { col: u16, x: f64, y: f64, cells: u16, text: String, rgb: [u8; 3], bold: bool, italic: bool }, Glyph { x: f64, y: f64, ops: Vec<DrawOp>, rgb: [u8; 3] }, Line { x: f64, y: f64, w: f64, rgb: [u8; 3], kind: LineKind } }`.
    - `enum LineKind { Under(Underline), Strike }`.
    - `plan_row(row: &RowRender, line: usize, geom: &CellGeom, fg_is_rgb: &dyn Fn(u16) -> bool, out: &mut Vec<PaintOp>)`.

Reglas de color (VTE 0.68 tal como lo configura `make_term`, 789–813):
- Paleta: los 16 colores del tema (`set_colors(fg, bg, pal)`), cubo 6×6×6 y grises de xterm para 16–255 (`Palette::xterm_default` y se pisan 0–15); cursor del tema, `cursor_accent` = fondo.
- Negrita sin aclarar (`bold_is_bright` falso: el Python no llama a `set_bold_is_bright`).
- Tenue (SGR 2): `vte_dim` = cada canal × 2 / 3 (la «fórmula mágica de xterm» de VTE en `rgb_from_index`), **solo** si el color no es RGB directo; con RGB directo, sin cambio. `Style.fg` de `comandos-term` ya trae la mezcla al 50 % de xterm.js: el plan usa `style.dim_fg` (el color sin mezclar) como base. La calibración contra píxeles de VTE es la puerta `pixel_gate_terminal_attrs` de T12.
- Fondo por omisión (`style.default_bg`): no se emite `Rect`; lo pinta T6 con la opacidad del tema (`TERM_OPACITY`). Fondos explícitos: `Rect` opaco.

- [ ] **Step 1: pruebas que fallan**

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/term_paint.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::term::engine::{RowRender, TermEngine, Underline};
use comandos_app::term::paint::{CellGeom, LineKind, PaintOp, palette_from_theme, plan_row, vte_dim};
use std::time::{Duration, Instant};

const PAL: [&str; 16] = [
    "#232B3E", "#FF6B6B", "#2EE59D", "#FFAE1A", "#7AA5FF", "#B49CFF", "#4DD0E1", "#EAF0FB",
    "#5E6980", "#FF8F8F", "#6FF0BC", "#FFC55C", "#A3C0FF", "#CDBBFF", "#86E3F0", "#FFFFFF",
];

fn engine(cols: u16, rows: u16) -> (TermEngine, Instant) {
    let epoch = Instant::now();
    let pal = palette_from_theme("#EAF0FB", "#0A0D13", "#FFAE1A", &PAL).unwrap();
    (TermEngine::new(cols, rows, 100, pal, false, epoch), epoch)
}

fn geom() -> CellGeom {
    CellGeom { cell_w: 8.0, cell_h: 19.0, origin_x: 10.0, origin_y: 22.0, dpr: 1.0, font_size: 13.0 }
}

fn plan(t: &TermEngine, line: usize) -> Vec<PaintOp> {
    let mut row = RowRender::default();
    t.render_line(line, &mut row);
    let mut ops = Vec::new();
    plan_row(&row, line, &geom(), &|col| t.fg_is_rgb(line, col), &mut ops);
    ops
}

fn texts(ops: &[PaintOp]) -> Vec<(String, u16, u16, [u8; 3], bool)> {
    ops.iter()
        .filter_map(|o| match o {
            PaintOp::Text { col, cells, text, rgb, bold, .. } => Some((text.clone(), *col, *cells, *rgb, *bold)),
            _ => None,
        })
        .collect()
}

#[test]
fn palette_from_theme_hex() {
    let p = palette_from_theme("#EAF0FB", "#0A0D13", "#FFAE1A", &PAL).unwrap();
    assert_eq!(p.ansi[1], [0xFF, 0x6B, 0x6B]);
    assert_eq!(p.ansi[16], [0, 0, 0]);
    assert_eq!(p.ansi[231], [255, 255, 255]);
    assert_eq!(p.ansi[232], [8, 8, 8]);
    assert_eq!((p.fg, p.bg, p.cursor, p.cursor_accent), ([0xEA, 0xF0, 0xFB], [0x0A, 0x0D, 0x13], [0xFF, 0xAE, 0x1A], [0x0A, 0x0D, 0x13]));
    assert!(palette_from_theme("#EAF0FB", "nope", "#FFAE1A", &PAL).is_none());
}

#[test]
fn ascii_runs_merge() {
    let (mut t, e) = engine(20, 2);
    t.feed(b"hola mundo", e);
    let got = texts(&plan(&t, 0));
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!((got[0].0.trim_end(), got[0].1), ("hola mundo", 0));
}

#[test]
fn wide_cells_paint_once() {
    let (mut t, e) = engine(20, 2);
    t.feed("a漢b🚀".as_bytes(), e);
    let got = texts(&plan(&t, 0));
    let wide: Vec<_> = got.iter().filter(|(s, ..)| s.contains('漢')).collect();
    assert_eq!(wide.len(), 1, "{got:?}");
    assert_eq!((wide[0].1, wide[0].2), (1, 2), "columna 1, dos celdas");
    let rocket: Vec<_> = got.iter().filter(|(s, ..)| s.contains('🚀')).collect();
    assert_eq!((rocket.len(), rocket[0].2), (1, 2));
    assert!(got.iter().any(|(s, col, ..)| s.starts_with('b') && *col == 3));
}

#[test]
fn box_chars_become_draw_ops() {
    let (mut t, e) = engine(10, 1);
    t.feed("─│┼".as_bytes(), e);
    let ops = plan(&t, 0);
    let glyphs = ops.iter().filter(|o| matches!(o, PaintOp::Glyph { .. })).count();
    assert_eq!(glyphs, 3);
    assert!(texts(&ops).iter().all(|(s, ..)| !s.contains(['─', '│', '┼'])));
}

#[test]
fn bg_runs_become_rects_but_default_bg_does_not() {
    let (mut t, e) = engine(10, 1);
    t.feed(b"\x1b[41m  \x1b[0m  ", e);
    let rects: Vec<_> = plan(&t, 0)
        .into_iter()
        .filter_map(|o| match o {
            PaintOp::Rect { x, y, w, h, rgb } => Some((x, y, w, h, rgb)),
            _ => None,
        })
        .collect();
    assert_eq!(rects, [(10.0, 22.0, 16.0, 19.0, [0xFF, 0x6B, 0x6B])]);
}

#[test]
fn dim_uses_vte_two_thirds_except_rgb() {
    let (mut t, e) = engine(10, 2);
    t.feed(b"\x1b[2mx\x1b[0m\r\n\x1b[2;38;2;200;100;50my", e);
    assert_eq!(texts(&plan(&t, 0))[0].3, vte_dim([0xEA, 0xF0, 0xFB]));
    assert_eq!(vte_dim([0xEA, 0xF0, 0xFB]), [0x9C, 0xA0, 0xA7]);
    assert_eq!(texts(&plan(&t, 1))[0].3, [200, 100, 50]);
}

#[test]
fn bold_is_not_bright_like_vte() {
    let (mut t, e) = engine(10, 1);
    t.feed(b"\x1b[1;31mx", e);
    let got = texts(&plan(&t, 0));
    assert_eq!((got[0].3, got[0].4), ([0xFF, 0x6B, 0x6B], true));
}

#[test]
fn underline_and_strike_become_lines() {
    let (mut t, e) = engine(10, 1);
    t.feed(b"\x1b[4mu\x1b[24;9ms", e);
    let kinds: Vec<LineKind> = plan(&t, 0)
        .into_iter()
        .filter_map(|o| match o {
            PaintOp::Line { kind, .. } => Some(kind),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, [LineKind::Under(Underline::Single), LineKind::Strike]);
}

#[test]
fn sync_update_defers_damage() {
    let (mut t, e) = engine(10, 2);
    let mut lines = Vec::new();
    let _ = t.take_dirty(&mut lines);
    lines.clear();
    t.feed(b"\x1b[?2026habc", e);
    assert!(t.next_deadline().is_some());
    assert!(!t.take_dirty(&mut lines) && lines.is_empty(), "retenido: nada que pintar");
    t.feed(b"\x1b[?2026l", e + Duration::from_millis(5));
    assert!(t.take_dirty(&mut lines) || lines.contains(&0));
    assert!(t.next_deadline().is_none());
}
```

- [ ] **Step 2: correrlas**

Run: `$C test -p comandos-app --test term_paint`
Expected: FAIL (no existe `term`).

- [ ] **Step 3: `term/mod.rs` y `term/engine.rs`**

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/mod.rs`:
```rust
//! Terminal de escritorio sobre `comandos-term`: motor, pintado, PTY y widget.
pub mod engine;
pub mod paint;
```

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/engine.rs`:
```rust
//! Única puerta a `comandos-term` (F22): el resto del crate importa de aquí. Si la
//! Fase 3 cambia una firma, solo cambia este archivo y quien use lo cambiado.
pub use comandos_term::engine::{ClipboardTarget, Damage, Drained, Engine, GridSize, Modes, MouseMode, Palette};
pub use comandos_term::glyphs::{CellMetrics, DrawOp, draw_ops};
pub use comandos_term::input::{
    Button, Ime, KeyAction, KeyInput, MouseKind, WheelAction, encode_key, encode_mouse, encode_paste, focus, wheel,
};
pub use comandos_term::render::{
    CursorShape, CursorView, RenderOpts, RowRender, Run, RunKind, Style, Underline, cursor, render_row_into,
};
pub use comandos_term::select::{
    Point, SelectMode, Selection, UrlSpan, find_urls, selected_text, selection_bounds, word_at,
};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Line;
use alacritty_terminal::vte::ansi::Color;
use std::time::{Duration, Instant};

/// `set_scrollback_lines(10000)` de `make_term` (795).
pub const SCROLLBACK: usize = 10_000;
/// Celdas del mosaico (`_mosaic_term`).
pub const MOSAIC_SCROLLBACK: usize = 400;
/// VTE: la negrita no aclara el color; sin ajuste de contraste.
pub const VTE_OPTS: RenderOpts = RenderOpts { bold_is_bright: false, min_contrast: 1.0 };

pub struct TermEngine {
    engine: Engine,
    epoch: Instant,
    palette: Palette,
    cols: u16,
    rows: u16,
}

impl TermEngine {
    pub fn new(cols: u16, rows: u16, scrollback: usize, palette: Palette, blink: bool, epoch: Instant) -> TermEngine {
        let engine = Engine::with_cursor_blink(GridSize { cols, rows }, scrollback, palette.clone(), blink);
        TermEngine { engine, epoch, palette, cols, rows }
    }

    fn ms(&self, now: Instant) -> f64 {
        now.saturating_duration_since(self.epoch).as_secs_f64() * 1000.0
    }

    pub fn feed(&mut self, bytes: &[u8], now: Instant) {
        let ms = self.ms(now);
        self.engine.advance(bytes, ms);
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        let ms = self.ms(now);
        self.engine.tick(ms)
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        let ms = self.engine.next_deadline_ms()?;
        Some(self.epoch + Duration::from_secs_f64((ms / 1000.0).max(0.0)))
    }

    pub fn take_dirty(&mut self, lines: &mut Vec<usize>) -> bool {
        self.engine.take_damage_into(lines)
    }

    pub fn render_line(&self, line: usize, out: &mut RowRender) {
        render_row_into(&self.engine, line, &self.palette, &VTE_OPTS, out);
    }

    pub fn cursor(&self) -> CursorView {
        cursor(&self.engine)
    }

    pub fn modes(&self) -> Modes {
        self.engine.modes()
    }

    pub fn drain(&mut self) -> Drained {
        self.engine.drain()
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell_px: (u16, u16)) {
        self.cols = cols;
        self.rows = rows;
        self.engine.resize(GridSize { cols, rows }, cell_px);
    }

    pub fn set_palette(&mut self, palette: Palette) {
        self.engine.set_palette(palette.clone());
        self.palette = palette;
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    pub fn scroll(&mut self, lines: i32) {
        self.engine.scroll_display(lines);
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// El color de primer plano de la celda es RGB directo (VTE no lo atenúa).
    pub fn fg_is_rgb(&self, line: usize, col: u16) -> bool {
        let grid = self.engine.term().grid();
        let offset = i32::try_from(self.engine.display_offset()).unwrap_or(0);
        let Ok(line) = i32::try_from(line) else { return false };
        let point = Line(line - offset);
        if usize::from(col) >= grid.columns() || line >= i32::try_from(grid.screen_lines()).unwrap_or(0) {
            return false;
        }
        grid.display_iter().find(|cell| cell.point.line == point && cell.point.column.0 == usize::from(col))
            .is_some_and(|cell| matches!(cell.cell.fg, Color::Spec(_)))
    }
}
```
Las firmas se comprobaron en comandos-term y alacritty_terminal 0.26.0. fg_is_rgb usa Grid::display_iter e Indexed::{point,cell}, sin indexado directo. T4 debe validar selección/scrollback y RGB directo antes de pintar.

- [ ] **Step 4: `term/paint.rs`**

```rust
//! Plan de pintado por fila, sin cairo: rectángulos de fondo, tiras de texto,
//! glifos de caja (U+2500–U+259F y Powerline, de `comandos-term::glyphs`) y
//! líneas de subrayado/tachado, en píxeles del widget. T6 lo ejecuta.
use crate::term::engine::{CellMetrics, DrawOp, Palette, RowRender, RunKind, Underline, draw_ops};

pub fn hex_rgb(h: &str) -> Option<[u8; 3]> {
    let digits = h.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| digits.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok());
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// `set_colors(fg, bg, pal)` + `set_color_cursor(cursor)` de `make_term`.
pub fn palette_from_theme(fg: &str, bg: &str, cursor: &str, pal16: &[&str]) -> Option<Palette> {
    let (fg, bg, cursor) = (hex_rgb(fg)?, hex_rgb(bg)?, hex_rgb(cursor)?);
    let mut p = Palette::xterm_default(fg, bg, cursor, bg, fg);
    for (slot, hex) in p.ansi.iter_mut().zip(pal16.iter().take(16)) {
        *slot = hex_rgb(hex)?;
    }
    Some(p)
}

/// Atenuado de VTE (`rgb_from_index`): 2/3 de cada canal en 16 bits.
pub fn vte_dim(c: [u8; 3]) -> [u8; 3] {
    c.map(|v| {
        let wide = u32::from(v) * 257 * 2 / 3;
        u8::try_from(wide / 257).unwrap_or(u8::MAX)
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellGeom {
    pub cell_w: f64,
    pub cell_h: f64,
    /// Esquina superior izquierda de la celda (0, 0) dentro del widget (márgenes de `make_term`).
    pub origin_x: f64,
    pub origin_y: f64,
    pub dpr: f64,
    pub font_size: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Under(Underline),
    Strike,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaintOp {
    Rect { x: f64, y: f64, w: f64, h: f64, rgb: [u8; 3] },
    Text { col: u16, x: f64, y: f64, cells: u16, text: String, rgb: [u8; 3], bold: bool, italic: bool },
    Glyph { x: f64, y: f64, ops: Vec<DrawOp>, rgb: [u8; 3] },
    Line { x: f64, y: f64, w: f64, rgb: [u8; 3], kind: LineKind },
}

pub fn plan_row(row: &RowRender, line: usize, g: &CellGeom, fg_is_rgb: &dyn Fn(u16) -> bool, out: &mut Vec<PaintOp>) {
    let y = g.origin_y + g.cell_h * f64::from(u32::try_from(line).unwrap_or(u32::MAX));
    let x_of = |col: u16| g.origin_x + g.cell_w * f64::from(col);
    for &(col, cells, rgb) in &row.bg_runs {
        out.push(PaintOp::Rect { x: x_of(col), y, w: g.cell_w * f64::from(cells), h: g.cell_h, rgb });
    }
    let metrics = CellMetrics { cell_w: g.cell_w * g.dpr, cell_h: g.cell_h * g.dpr, dpr: g.dpr, font_size: g.font_size };
    for run in &row.runs {
        if run.style.hidden {
            continue;
        }
        let base = run.style.dim_fg.unwrap_or(run.style.fg);
        let rgb = if run.style.dim && !fg_is_rgb(run.col) { vte_dim(base) } else if run.style.dim { base } else { run.style.fg };
        match run.kind {
            RunKind::Box(c) => {
                let mut ops = Vec::new();
                if draw_ops(c, &metrics, &mut ops) {
                    out.push(PaintOp::Glyph { x: x_of(run.col), y, ops, rgb });
                    continue;
                }
                out.push(PaintOp::Text { col: run.col, x: x_of(run.col), y, cells: run.cells, text: run.text.clone(), rgb, bold: run.style.bold, italic: run.style.italic });
            }
            RunKind::Text | RunKind::Wide => {
                out.push(PaintOp::Text { col: run.col, x: x_of(run.col), y, cells: run.cells, text: run.text.clone(), rgb, bold: run.style.bold, italic: run.style.italic });
            }
        }
        let w = g.cell_w * f64::from(run.cells);
        let line_rgb = run.style.underline_color.unwrap_or(rgb);
        if run.style.underline != Underline::None {
            out.push(PaintOp::Line { x: x_of(run.col), y, w, rgb: line_rgb, kind: LineKind::Under(run.style.underline) });
        }
        if run.style.strike {
            out.push(PaintOp::Line { x: x_of(run.col), y, w, rgb, kind: LineKind::Strike });
        }
    }
}
```

`#[derive(Clone)]` de `Palette` existe en `comandos-term` (la paleta se copia al cambiar de tema); si no lo tuviera, `TermEngine` guarda solo la suya y llama a `set_palette` con otra construida igual.

- [ ] **Step 5: pruebas**

Run: `$C test -p comandos-app --test term_paint`
Expected: 9 PASS.

- [ ] **Step 6: commit**

```bash
git add crates/comandos-app/Cargo.toml crates/comandos-app/src/lib.rs crates/comandos-app/src/term crates/comandos-app/tests/term_paint.rs
git commit -m "feat(app): motor de terminal sobre comandos-term con colores de VTE y plan de pintado por fila
"
```

### Task 5: PTY de escritorio y enganche cuando el tamaño se estabiliza

**Depende de:** T3. **Paralelizable:** sí con T4; el controlador integra las declaraciones de term/mod.rs.

Aplica F27 (firma de `initial_size`), F28 (`stty size` y attach con PTY sin tamaño), F29 (`Drop`), F30 (prueba con tmux real antes/después).

**Files:**
- Create: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/{settle,pty}.rs`
- Modify: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/mod.rs`
- Test: `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/term_pty.rs`

**Interfaces:**
- Consumes: `tmux::TmuxCtl::{attach_argv, window_size}`; `support::tmux::TestTmux`.
- Produces:
  - `term::settle::Settle::{new(quiet_ms: u64, cap_ms: u64, start_ms: u64) -> Settle, on_alloc(&mut self, now_ms: u64), due(&self, now_ms: u64) -> bool, fired(&mut self) -> bool, next_check_ms(&self, now_ms: u64) -> Option<u64>}` (`fired` devuelve `true` solo la primera vez).
  - `term::settle::initial_size((w, h): (i32, i32), cell_w: f64, cell_h: f64, (pad_x, pad_y): (f64, f64), tmux: Option<(u16, u16)>) -> (u16, u16)`: `w <= 1` = «sin tamaño de GTK» → `tmux` o 80×24; si no, `cols = ⌊(w − pad_x) / cell_w⌋` (mín. 2) y `rows = ⌊(h − pad_y) / cell_h⌋` (mín. 1).
  - `term::pty::PtySession::{spawn(argv: &[String], cols: u16, rows: u16, cwd: &Path) -> Result<PtySession, PtyError>, read_chunk(&mut self, buf: &mut [u8]) -> ReadOutcome, write(&mut self, bytes: &[u8]) -> Result<(), PtyError>, flush_pending(&mut self) -> Result<bool, PtyError>, has_pending(&self) -> bool, resize(&self, cols: u16, rows: u16) -> Result<(), PtyError>, raw_fd(&self) -> i32, pid(&self) -> u32, child_tty(&self) -> Option<String>, try_reap(&mut self) -> Option<i32>}`; `impl Drop for PtySession` (cuelga el grupo con `SIGHUP`, como cerrar la ventana, y recoge al hijo en un hilo); `ReadOutcome { Data(usize), WouldBlock, Closed }`; `PtyError { Open(String), Spawn(String), Io(String) }`.
  - Constantes: `SETTLE_QUIET_MS = 250`, `SETTLE_CAP_MS = 1500` (`_spawn_when_settled`, 760), `RESPAWN_MS = 800` (`child-exited`, 884).
- Entorno del hijo: el del proceso, sin `TMUX`/`TMUX_PANE`/`NO_COLOR`, con `TERM=xterm-256color`, `COLORTERM=truecolor` y `VTE_VERSION=6800` (lo que VTE 0.68 exporta en `spawn_sync`); `cwd` = `$HOME` (`make_term`, 869).

- [ ] **Step 1: pruebas que fallan**

`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/term_pty.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::disallowed_methods)]
mod support;
use comandos_app::config::RunMode;
use comandos_app::term::pty::{PtySession, ReadOutcome};
use comandos_app::term::settle::{Settle, initial_size};
use std::time::{Duration, Instant};
use support::tmux::TestTmux;

fn read_until(p: &mut PtySession, needle: &str, within: Duration) -> String {
    let mut buf = [0u8; 4096];
    let mut got = Vec::new();
    let end = Instant::now() + within;
    while Instant::now() < end {
        match p.read_chunk(&mut buf) {
            ReadOutcome::Data(n) => got.extend_from_slice(&buf[..n]),
            ReadOutcome::WouldBlock => std::thread::sleep(Duration::from_millis(5)),
            ReadOutcome::Closed => break,
        }
        if String::from_utf8_lossy(&got).contains(needle) {
            break;
        }
    }
    String::from_utf8_lossy(&got).into_owned()
}

#[test]
fn settle_waits_for_quiet_allocation() {
    let mut s = Settle::new(250, 1500, 0);
    s.on_alloc(10);
    s.on_alloc(100); // 174 → 168 → 163 columnas al arrancar
    s.on_alloc(200);
    assert!(!s.due(449));
    assert!(s.due(450));
    assert!(s.fired());
    assert!(!s.fired(), "solo una vez");
    assert_eq!(s.next_check_ms(500), None);
}

#[test]
fn settle_cap_fires_even_while_resizing() {
    let mut s = Settle::new(250, 1500, 0);
    for t in (0..1600).step_by(100) {
        s.on_alloc(t);
        if t < 1500 {
            assert!(!s.due(t), "{t}");
        }
    }
    assert!(s.due(1500));
    assert_eq!(Settle::new(250, 1500, 0).next_check_ms(10), Some(1500));
}

#[test]
fn attach_before_allocation_uses_tmux_size() {
    // Asignación de 1 px (pantalla bloqueada): manda el tamaño que tmux ya tiene.
    assert_eq!(initial_size((1, 1), 8.0, 18.0, (20.0, 30.0), Some((163, 44))), (163, 44));
    // Sin dato de tmux: 80×24, como VTE.
    assert_eq!(initial_size((1, 822), 8.0, 18.0, (20.0, 30.0), None), (80, 24));
    // Con tamaño real: (1300 − 20) / 8 = 160 columnas, (822 − 30) / 18 = 44 filas.
    assert_eq!(initial_size((1300, 822), 8.0, 18.0, (20.0, 30.0), Some((10, 10))), (160, 44));
    // Ventana diminuta: mínimos 2×1.
    assert_eq!(initial_size((10, 10), 8.0, 18.0, (20.0, 30.0), None), (2, 1));
}

#[test]
fn pty_roundtrip_utf8_and_stty_resize() {
    let home = std::env::temp_dir();
    let mut p = PtySession::spawn(&["/bin/sh".into(), "-c".into(), r#"stty size; while IFS= read -r line; do printf '%s\n' "$line"; stty size; done"#.into()], 100, 30, &home).unwrap();
    assert!(read_until(&mut p, "30 100", Duration::from_secs(3)).contains("30 100"), "tamaño ANTES del primer byte");
    p.write("ñandú 漢字 🚀\n".as_bytes()).unwrap();
    assert!(read_until(&mut p, "🚀", Duration::from_secs(3)).contains("ñandú 漢字 🚀"));
    p.resize(120, 40).unwrap();
    p.write(b"resize-check\n").unwrap();
    assert!(read_until(&mut p, "40 120", Duration::from_secs(3)).contains("40 120"), "resize observado en el MISMO PTY");
}

#[test]
fn pty_attach_keeps_session_size_until_settled() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else { return };
    f.tmux.new_session("s1", 163, 44);
    let before = f.tmux.session_size("s1");
    // GTK aún no dio tamaño: el primer attach usa el de tmux, no 80×24.
    let (c, r) = initial_size((1, 1), 8.0, 18.0, (20.0, 30.0), f.ctl.window_size("s1"));
    assert_eq!((c, r), (163, 44));
    let p = PtySession::spawn(&f.ctl.attach_argv("s1"), c, r, &std::env::temp_dir()).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(p.child_tty().is_some_and(|tty| tty.starts_with("/dev/pts/")));
    assert_eq!(f.tmux.session_size("s1"), before, "el attach no encoge la sesión");
    // Cuando la asignación se estabiliza, un solo cambio al tamaño real.
    p.resize(120, 40).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(f.tmux.session_size("s1"), (120, 40));
    drop(p);
    std::thread::sleep(Duration::from_millis(300));
    let clients = f.tmux.raw(&["list-clients", "-F", "#{client_tty}"]);
    assert!(String::from_utf8_lossy(&clients.stdout).trim().is_empty(), "Drop cuelga el cliente");
}
```

- [ ] **Step 2: correrlas**

Run: `$C test -p comandos-app --test term_pty`
Expected: FAIL (no existen `settle` ni `pty`).

- [ ] **Step 3: `settle.rs`**

```rust
//! `_spawn_when_settled` (760): engancha cuando la asignación deja de cambiar
//! (`quiet_ms`) o, como tope, a los `cap_ms`. Así tmux redimensiona una sola vez
//! (al arrancar la ventana se acomoda en pasos 174→168→163 columnas).
pub const SETTLE_QUIET_MS: u64 = 250;
pub const SETTLE_CAP_MS: u64 = 1500;
/// `child-exited` → `GLib.timeout_add(800, spawn)` (884).
pub const RESPAWN_MS: u64 = 800;

#[derive(Debug, Clone)]
pub struct Settle {
    quiet: u64,
    cap_at: u64,
    last_alloc: Option<u64>,
    done: bool,
}

impl Settle {
    pub fn new(quiet_ms: u64, cap_ms: u64, start_ms: u64) -> Settle {
        Settle { quiet: quiet_ms, cap_at: start_ms.saturating_add(cap_ms), last_alloc: None, done: false }
    }

    pub fn on_alloc(&mut self, now_ms: u64) {
        self.last_alloc = Some(now_ms);
    }

    pub fn due(&self, now_ms: u64) -> bool {
        !self.done
            && (now_ms >= self.cap_at || self.last_alloc.is_some_and(|t| now_ms >= t.saturating_add(self.quiet)))
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
        let quiet = self.last_alloc.map_or(self.cap_at, |t| t.saturating_add(self.quiet));
        Some(quiet.min(self.cap_at).max(now_ms.saturating_add(1)))
    }
}

/// Tamaño del primer attach (`make_term.spawn`, 861–866): con la asignación real si
/// existe; si GTK aún no dio tamaño (1 px, también con la pantalla bloqueada), el
/// que la sesión ya tiene en tmux; si no, 80×24 como VTE.
pub fn initial_size(
    (w, h): (i32, i32),
    cell_w: f64,
    cell_h: f64,
    (pad_x, pad_y): (f64, f64),
    tmux: Option<(u16, u16)>,
) -> (u16, u16) {
    if w <= 1 {
        return tmux.unwrap_or((80, 24));
    }
    let cells = |px: i32, pad: f64, cell: f64, min: f64| -> u16 {
        let n = ((f64::from(px) - pad) / cell.max(1.0)).floor().max(min).min(f64::from(u16::MAX));
        // n es entero y está en [min, u16::MAX]: el `as` es exacto.
        n as u16
    };
    (cells(w, pad_x, cell_w, 2.0), cells(h, pad_y, cell_h, 1.0))
}
```

- [ ] **Step 4: `pty.rs`**

```rust
//! PTY por pestaña con `pty-process` (API bloqueante, sin `unsafe` aquí). El
//! descriptor maestro se pone no bloqueante y lo vigila el bucle de GLib (T6).
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::path::Path;
use std::process::Child;

#[derive(Debug)]
pub enum PtyError {
    Open(String),
    Spawn(String),
    Io(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOutcome {
    Data(usize),
    WouldBlock,
    Closed,
}

pub struct PtySession {
    pty: pty_process::blocking::Pty,
    child: Option<Child>,
    pending: VecDeque<u8>,
}

impl PtySession {
    pub fn spawn(argv: &[String], cols: u16, rows: u16, cwd: &Path) -> Result<PtySession, PtyError> {
        let (program, args) = argv.split_first().ok_or_else(|| PtyError::Spawn("argv vacío".into()))?;
        let (pty, pts) = pty_process::blocking::open().map_err(|e| PtyError::Open(e.to_string()))?;
        // El tamaño va ANTES del attach: tmux nunca ve 80×24 por accidente.
        pty.resize(pty_process::Size::new(rows, cols)).map_err(|e| PtyError::Open(e.to_string()))?;
        let child = pty_process::blocking::Command::new(program)
            .args(args)
            .current_dir(cwd)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("NO_COLOR")
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("VTE_VERSION", "6800")
            .spawn(pts)
            .map_err(|e| PtyError::Spawn(e.to_string()))?;
        let flags = fcntl(pty.as_fd(), FcntlArg::F_GETFL).map_err(|e| PtyError::Io(e.to_string()))?;
        fcntl(pty.as_fd(), FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK))
            .map_err(|e| PtyError::Io(e.to_string()))?;
        Ok(PtySession { pty, child: Some(child), pending: VecDeque::new() })
    }

    pub fn raw_fd(&self) -> i32 {
        self.pty.as_raw_fd()
    }

    pub fn pid(&self) -> u32 {
        self.child.as_ref().map_or(0, Child::id)
    }

    pub fn read_chunk(&mut self, buf: &mut [u8]) -> ReadOutcome {
        match self.pty.read(buf) {
            Ok(0) => ReadOutcome::Closed,
            Ok(n) => ReadOutcome::Data(n),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => ReadOutcome::WouldBlock,
            // EIO: el hijo cerró el esclavo.
            Err(_) => ReadOutcome::Closed,
        }
    }

    /// Escribe lo que quepa; el resto queda para `flush_pending` (vigilado con `IO_OUT`).
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), PtyError> {
        self.pending.extend(bytes);
        self.flush_pending().map(|_| ())
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// `true` si quedó todo escrito.
    pub fn flush_pending(&mut self) -> Result<bool, PtyError> {
        while !self.pending.is_empty() {
            let (front, _) = self.pending.as_slices();
            match self.pty.write(front) {
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

    /// TTY del hijo (`/proc/<pid>/fd/0`): identifica el cliente tmux de la pestaña (875).
    pub fn child_tty(&self) -> Option<String> {
        std::fs::read_link(format!("/proc/{}/fd/0", self.pid())).ok().map(|p| p.to_string_lossy().into_owned())
    }

    pub fn try_reap(&mut self) -> Option<i32> {
        let status = self.child.as_mut()?.try_wait().ok().flatten()?;
        self.child = None;
        Some(status.code().unwrap_or(-1))
    }
}

impl Drop for PtySession {
    /// Como cerrar la ventana de VTE: `SIGHUP` al grupo del hijo (es líder de sesión,
    /// `pty-process` hace `setsid`) y se recoge en un hilo para no dejar zombis.
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Ok(pid) = i32::try_from(child.id()) {
                let _ = killpg(Pid::from_raw(pid), Signal::SIGHUP);
            }
            let _ = std::thread::Builder::new().name("comandos-pty-reap".into()).spawn(move || {
                let _ = child.wait();
            });
        }
    }
}
```

`term/mod.rs` añade `pub mod pty;` y `pub mod settle;`.

- [ ] **Step 5: pruebas**

Run: `$C test -p comandos-app --test term_pty`
Expected: 5 PASS.

- [ ] **Step 6: commit**

```bash
git add crates/comandos-app/src/term/mod.rs crates/comandos-app/src/term/settle.rs crates/comandos-app/src/term/pty.rs crates/comandos-app/tests/term_pty.rs
git commit -m "feat(app): PTY de escritorio no bloqueante que cuelga al soltarse y enganche a tmux cuando el tamaño se estabiliza
"
```



## Correcciones vinculantes de T4/T5 antes de implementar

Las APIs de T4 coinciden con `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/crates/comandos-term/src/engine.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/crates/comandos-term/src/render.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/crates/comandos-term/src/input.rs` y `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase3/crates/comandos-term/src/glyphs.rs`. No existen Terminal, CellView ni TermColor de la versión inicial. Se usa Engine, RowRender, Run y DrawOp, sin duplicar motores de teclado/ratón/selección/enlaces. Hex RGB exige seis dígitos hexadecimales; la prueba incluye `#+12345` como rechazo. Render no repite espacios finales de ancho doble; term/links adapta coordenadas y texto envuelto, no reimplementa select::find_urls.

T5 crea su transporte independientemente de T4 y añade `spawn_with_env(argv: &[String], cols: u16, rows: u16, cwd: &Path, env: &[(String, String)], clear_env: bool) -> Result<PtySession, PtyError>`. `spawn` delega con env vacío y clear_env falso. Las pruebas usan clear_env verdadero, HOME de fixture, PATH /usr/bin:/bin y SHELL /bin/sh. La API segura pty_process::blocking::Command tiene env_clear/envs, verificado en la dependencia 0.5.3. Los entornos de fixture se aplican también al attach. No se heredan perfiles ni credenciales del usuario. La configuración no bloqueante ocurre antes de spawn, para que un fallo de fcntl no deje un hijo vivo. La cola pendiente se limita a 1 MiB con error explícito y prueba de backpressure. Drop envía HUP únicamente al grupo del hijo de PTY y el recolector privado escala a TERM/KILL tras plazos acotados; nunca espera indefinidamente ni mata tmux servidor. Se verifica stty size antes y después de resize en el mismo PTY, y que un cliente retirado no mata la sesión.

T3 incluye prueba del primer new-session sin servidor prearrancado: sandbox debe iniciar con configuración /dev/null, HOME privado, shell explícito y entorno sin credenciales. Un fixture que arranca el servidor antes de llamar al adaptador no demuestra ese contrato. La ronda de código 15f7216 incluye aislamiento y regresiones rojo/verde; su re-review está aprobada.

## Convenciones para las tareas restantes

Las rutas de archivos de T6-T20 están ancladas explícitamente en el worktree `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4`. El implementador adapta únicamente ese prefijo si trabaja en otro worktree, sin cambiar el archivo lógico. Los archivos declarados Create son entregables futuros, no archivos cuya existencia se afirma hoy. Los binarios de fixture son auxiliares Rust del test harness; no se crean scripts Python/bash y no se ejecutan agentes reales. El oráculo es `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app`, leído por AST sin arrancar su bootstrap GTK. Todas las tareas entregan lista de funciones de Apéndice A aceptables para app-drift; únicamente el controlador modifica la línea base.

Cada tarea termina con pruebas nativas del paquete, clippy del workspace con -j2, fmt y commit explícito. Las pruebas visuales son entregables separados: un exit 0 nativo nunca se presenta como paridad visual. Una puerta remota ausente queda registrada como pendiente con su motivo; no se convierte en prueba ignorada que permita cutover. Las tareas 4b se pueden construir en ramas independientes tras T13, pero el controlador integra declaraciones de módulos, dependencias y registros en orden T14-T18.

### Task 6: widget de terminal GTK, entrada y calendario de pintado

**Depende de:** T3, T4, T5. **Entrega:** terminal nativo completo conectado a PTY y motor existente.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/view.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/keys.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/links.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/schedule.rs` y `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/term_view_model.rs`. Modify `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/term/mod.rs`.

**Interfaces:** consume TermEngine, PaintOp/CellGeom, PtySession/ReadOutcome, Settle/initial_size y las reexportaciones de input/select de T4. Produce `TermView::new(options: TermOptions) -> Result<TermView, TermViewError>`, `widget(&self) -> &gtk::DrawingArea`, `feed(&self, bytes: &[u8]) -> Result<(), PtyError>`, `resize(&self, cols: u16, rows: u16) -> Result<(), PtyError>`, `selection_text(&self) -> Option<String>`, `paste(&self, text: &str) -> Result<(), PtyError>`, `set_font_scale(&self, scale: f64)`, `set_preferences(&self, prefs: &Value)`, `set_palette(&self, palette: Palette)`, `session(&self) -> Option<&str>`, `child_tty(&self) -> Option<String>` y callbacks de título/salida/timbre. `TermOptions` contiene argv, cwd, session, palette, scrollback, preferences y mode. `PaintSchedule::{damage(now_ms, lines), next_delay_ms(now_ms), painted(now_ms), blink_due(now_ms)}` es puro. `normalize_wrapped_url_text(&str) -> String`, `url_from_wrapped_text(text: &str, fragment: &str, row: Option<usize>, col: Option<usize>) -> Option<String>` conservan las reglas Python.

Prueba inicial concreta en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/term_view_model.rs`, junto a los casos detallados en los pasos. Los tipos deben derivar PartialEq/Debug donde estas aserciones lo requieren.

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::term::links::normalize_wrapped_url_text;
#[test]
fn plain_utf8_url_text_stays_intact() {
    assert_eq!(normalize_wrapped_url_text("https://example.org/ñandú"), "https://example.org/ñandú");
}
```

- [ ] Leer make_term y helpers de entrada/enlaces del Apéndice A. Escribir tests de modelos sin GTK: daño repetido coalescido, synchronized-update hasta deadline, UTF-8 de IME una sola vez, key mapping GDK a KeyInput y fragmentos de URL envueltos. La prueba de doble commit debe alimentar Ime::start/end/input, no comparar el mismo encode consigo mismo.
- [ ] Ejecutar `$C test -p comandos-app --test term_view_model`; comprobar fallo por APIs ausentes antes de implementar.
- [ ] Conectar unix_fd_add_local a IN/HUP/ERR; leer hasta WouldBlock con presupuesto por iteración, avanzar Engine, devolver Drained.replies al PTY y tratar título/clipboard/timbre por callbacks. Registrar OUT solo mientras has_pending. Mantener SourceId y eliminar todas las fuentes al soltar TermView. HUP no provoca bucle ocupado ni respawn infinito.
- [ ] Medir fuente con pango antes de initial_size y attach; usar quiet 250 ms/cap 1500 ms. Pintar Rect, Text, Glyph y Line con cairo/pangocairo; traducir todas las variantes DrawOp por match exhaustivo y aplicar DPR una sola vez. Cursor, blink, selección, scrollback, OSC8 y detectores existentes se dibujan usando engine/select, sin duplicarlos. IMMulticontext procesa preedit/commit; IME no vuelve a enviar key-press. Ratón usa encode_mouse/wheel/focus; SSH conserva coalescing y límites de on_ssh_scroll.
- [ ] Ejecutar tests nativos y entregar casos remotos `ime_commit_reaches_pty`, `terminal_mouse_and_focus`, `cursor_blink_stops_when_unfocused`, `term_fd_sources_drop` a T12. Ninguno inicia GTK en local. Commit `feat(app): terminal GTK sobre motor y PTY nativos`.

### Task 7: aplicación mínima, WebKit y arnés remoto de evidencia

**Depende de:** T3, T6, T8. **Entrega:** app sandbox mínima y herramientas de captura/diff, sin registrar todavía funciones 4b.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/mod.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/app.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/window.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/webview.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/layout_dump.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/src/app_shot.rs` y `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/src/png_diff.rs`. Modify manifest, lib/main de comandos-app y main/manifest de xtask en ese mismo worktree.

**Interfaces:** `ui::app::run(args: &[String], default_live: bool) -> ExitCode`; `App` se conserva en Rc y posee configuración, tmux, guard, jobs, DashClient, Window, Paned, Notebook, WebView y `install_handler(name: &'static str, callback: Rc<dyn Fn(&Value) -> bool>)`. `dispatch(&self, message: &Value) -> bool` recorre handlers; false significa mensaje sin consumidor, nunca acción silenciosa. T13 decide el formato. `webview::create(cfg: &AppConfig) -> Result<WebView, WebError>`, `dashboard_uri(base: Option<&str>, version: &str) -> Option<String>`, `layout_dump::capture(window: &gtk::Window) -> Value` con geometrías/celdas/widget names para T12. `png_diff::compare(reference: &Path, candidate: &Path) -> Result<DiffSummary, DiffError>` con dimensiones, píxeles comparados, diferentes y porcentaje. `xtask app-shot --remote macmini --manifest <ruta absoluta de fixture> --output <ruta absoluta local> [--measure]` solo orquesta capacidad remota verificada y transfiere artifacts; no acepta perfil local de GTK.

Prueba inicial concreta en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/app_shell_model.rs`, junto a los casos detallados en los pasos. Los tipos deben derivar PartialEq/Debug donde estas aserciones lo requieren.

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::webview::dashboard_uri;
#[test]
fn disconnected_dashboard_has_no_live_url() {
    assert!(dashboard_uri(None, "fixture-v1").is_none());
    let uri = dashboard_uri(Some("http://127.0.0.1:7311"), "fixture-v1").expect("fixture");
    assert!(uri.contains("app=1") && uri.contains("anwin=1") && uri.contains("fixture-v1"));
}
```

- [ ] Añadir webkit2gtk 2.0.2 v2_40/javascriptcore-rs 1.1.2 y png 0.18.1 donde corresponde. Comprobar pkg-config antes de compilar; paquete de desarrollo faltante se informa, sin sudo automático. Crear tests nativos de URI app=1/anwin=1/version, lock por DISPLAY, aislamiento de website-data-manager y png-diff con PNGs sintéticos de 1 píxel/dimensiones diferentes/umbral 24 y 0,1 %. `$C test -p xtask png_diff` y `$C test -p comandos-app --test app_shell_model` deben fallar inicialmente y luego pasar.
- [ ] Crear window con WM_CLASS/título por config, backend X11, visual RGBA, mismos tamaños/márgenes/headerbar del bootstrap Python y lock open_lock+flock. Una instancia existente activa solo su clase según modo y sale; la prueba usa runner falso, nunca wmctrl en el escritorio real. El cierre libera fuentes y lock y recoge únicamente clientes de PTY propios.
- [ ] Sandbox usa datos web en raíces privadas y fixture HTTP 7311. Shadow usa WebsiteDataManager efímero, no copia ni abre datos web personales con escritura; no permite POST ni JS del dashboard con efectos reales. Live conserva los directorios WebKit del Python para localStorage; se compara con copia de perfil en fixture, no con el perfil vivo. Se mantiene user-content handler centro, NotificationPermissionRequest permitido y otras solicitudes sin aprobación automática, WebAudio, console messages y retry load-failed cada 2 s con fuente cancelable.
- [ ] Registrar en ui/mod únicamente los módulos existentes; reservar marcadores independientes `registro T13` a `registro T18` en app, sin stubs que simulen funciones. La app mínima muestra panel web fixture y una sesión privada existente, usa Jobs para recibir PollUpdate y no arranca procesos de agentes.
- [ ] Implementar app-shot como orquestador Rust remoto: preflight debe confirmar runtime Linux GTK3/WebKitGTK accesible en macmini, pantalla aislada, herramienta de captura, tmux privado, fixture HTTP, Python con GI/VTE del oráculo y toolchain que compila el binario para ese runtime. Verificar que todos esos procesos corren allí, antes de lanzar ninguno. macOS/Chrome solo no bastan. Si falta una capacidad devolver exit 2 con nombre concreto y cero procesos locales. No instalar/configurar VM ni declarar runtime disponible por asumirlo. Las capturas web usan chrome-bg; app-shot recoge GTK de la pantalla remota configurada.
- [ ] Medir app mínima más WebKitWebProcess y WebKitNetworkProcess por separado, incluidos procesos hijos, y registrar resultados y entorno reproducible. Puerta T7 exige puente correcto y presupuesto RSS de esta sección; si no hay runtime remoto, puerta pendiente y no habilita T20. Commit `feat(app): shell GTK y arnés de paridad exclusivamente remoto`.

### Task 8: cliente del tablero, polling puro y temas compartidos

**Depende de:** T1. **Paralelizable:** con T3/T4. **Entrega:** datos y preferencias sin dependencia de GLib.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/dash_client.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/poll.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/theme.rs` y tests `dash_client`, `poll`, `theme` en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/`.

**Interfaces:** `DashClient::new(base: Option<&str>, mode: RunMode) -> Result<DashClient, DashError>`, `get(&self, path: &str, timeout: Duration) -> Result<Value, DashError>`, `post(&self, path: &str, payload: &Value, timeout: Duration) -> Result<(u16, Value), DashError>`. None es desconectado y devuelve Disconnected sin abrir socket; Shadow post devuelve ShadowRefused sin red. `PollUpdate` enum `State(Value)`, `Prefs { value: Value, favorite_generation: u64 }`, `Notices { revision: String, badge: u64 }`, `Workspace(Value)`, `Marks(Value)`. `Poller::start(client: DashClient, generation: Arc<AtomicU64>) -> (Poller, mpsc::Receiver<PollUpdate>)`, `stop(&self)`; token de cancelación compartido y shutdown con plazos. Ningún widget/Rc/GLib en Poller. `ThemeTokens` conserva claves y 16 colores hex en String; `themes_from_file(raw: Option<&[u8]>) -> Value`, `tokens(themes: &Value, name: &Value) -> Option<Tokens>` se reutilizan del crate comandos-notifyd, sin reescribir tablas. `desktop_theme(name: &str, themes: &Value) -> Option<ThemeTokens>`, `theme_css(&ThemeTokens) -> String`, `header_css(&ThemeTokens) -> String`, `button_style_css(style: &str, theme: &ThemeTokens) -> String` pertenecen al escritorio y se comparan contra cc-app, no contra CSS de popups.

Prueba inicial concreta en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/dash_client.rs`, junto a los casos detallados en los pasos. Los tipos deben derivar PartialEq/Debug donde estas aserciones lo requieren.

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::{config::RunMode, dash_client::DashClient};
#[test]
fn disconnected_client_never_defaults_to_production() {
    let client = DashClient::new(None, RunMode::Sandbox).expect("config");
    assert!(client.get("/prefs", std::time::Duration::from_millis(50)).is_err());
}
```

- [ ] Escribir fixture HTTP Rust ligado exclusivamente a 127.0.0.1:7311, con cola de solicitudes y respuestas guionadas. Tests prueban None sin conexión, Shadow POST sin solicitud recibida, body/status de 409 y errores HTTP, unicode/JSON en orden Python, URL/Host de loopback y timeout. `$C test -p comandos-app --test dash_client --test poll --test theme` debe fallar antes del port y pasar después.
- [ ] Portar GET /state y /prefs cada 3 s con timeout 3 s, /workspace cada 2 s, /notices/watch rev+wait=25 con timeout 35 s/backoff 2 s, y fetch de marcas cada 5 s. Los bucles largos son hilos separados; no ocupan Jobs. T7 realiza el puente mpsc a MainContext. Coalescer State/Prefs por tipo y mantener badge de Notices como autoridad; /state no sobrescribe campana. Inyectar reloj y duraciones de test para comprobar orden/cancelación sin sleeps de 35 s.
- [ ] Llevar generation de favoritos en Prefs para descartar respuestas anteriores a un toggle. Aplicar únicamente diferencias de claves _LIVE_PREF_KEYS, incluyendo fuente/cursor/padding/opacidad/tema/tabs_layout. GET /conf permite reconciliar idioma con T1; desconectado conserva fallback LANG.
- [ ] Usar config/themes y parser de notifyd para tokens de tema compartidos; portar al escritorio las paletas ANSI que el archivo de temas no trae. Comparar las 9 THEMES y cinco estilos de botones contra AST del Python. CSS de cabecera no usa build_css de popup porque sus selectores difieren. Commit `feat(app): polling puro, preferencias y temas del tablero`.

### Task 9: pestañas, persistencia compartida y eventos IPC

**Depende de:** T3, T7, T8. **Entrega:** registro de pestañas y notebook con estado compatible.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/state_files.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/tabs.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ipc.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/tabstrip.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/tab_label.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/confirm.rs`; tests `tabs`, `state_files`, `ipc` en el directorio absoluto de tests anterior.

**Interfaces:** `TabRecord { key: String, label: String, favorite: bool, kind: TabKind }`, `TabKind { Session, Local, Web }`; `TabRegistry::{from_json(&Value) -> Result<Self, TabsError>, ordered_keys() -> Vec<String>, insert(TabRecord), rename(key: &str, label: &str), archive(key: &str, reason: &str), apply_favorites(&Value, generation: u64), to_json() -> Value}`. `StateFiles::new(config: AppConfig, guard: WriteGuard)`, `read(name: &str) -> Result<Value, StateError>`, `write_tabs(value: &Value) -> Result<(), StateError>` con lock app-tabs.json.lock, y `write(name: &str, value: &Value) -> Result<(), StateError>`. `IpcRequest { kind: IpcKind, payload: Value, path: PathBuf }`, `IpcKind { Focus, TabClose, TabOpen, Command }`, `read_request(path: &Path) -> Result<IpcRequest, IpcError>` y `consume(&self, request: &IpcRequest) -> Result<(), GuardError>` nunca consume en Shadow. `TabStripNotebook` adapta el GtkNotebook existente de App y expone focus/reorder/remove/overview por key.

Prueba inicial concreta en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/tabs.rs`, junto a los casos detallados en los pasos. Los tipos deben derivar PartialEq/Debug donde estas aserciones lo requieren.

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::tabs::TabRegistry;
#[test]
fn malformed_tabs_never_become_a_snapshot() {
    assert!(TabRegistry::from_json(&serde_json::json!("broken")).is_err());
}
```

- [ ] Escribir pruebas con HOME/runtime temporales de orden, favoritos, rename vacío, archivo corrupto/ausente, historial y notificación duplicada Gio. `tabs_write_holds_shared_lock` usa dos procesos Rust del fixture: mientras uno retiene flock, el otro no completa la escritura; después JSON exacto sin pérdida y sin temporal sobrante. `shadow_does_not_consume_ipc` comprueba hash/mtime/contenido sin borrar archivos.
- [ ] Ejecutar `$C test -p comandos-app --test tabs --test state_files --test ipc` en rojo. Portar modelos desde session_tabs y lib/gtk_tabstrip; mantener orden compartido y favoritos, no confundir orden de widgets con el doc workspace. Serializar con response_dumps, lock común y WriteGuard. Solo los nombres de archivos de Global Constraints son aceptados por StateFiles.
- [ ] Conectar monitors CREATED/CHANGES_DONE_HINT; leer documento completo, validar payload, encolar acción una sola vez y consumir solo en live/sandbox. Sombra puede observar pero no enfocar ni cerrar ni consumir IPC del usuario. Retirar monitores al salir. Los callbacks evitan bloquear MainContext y no procesan archivos mientras restore no esté listo.
- [ ] Implementar etiqueta renombrable, botón favorito, dots, confirmación must-answer sin cierre por Escape/clic exterior, local tab/quick terminal/open_tab/open_xterm y estado cerrado. Nuevas sesiones sandbox usan shell fixture y cliente desconectado, nunca http_ensure real. cierre usa tokens propios y no mata agentes sin confirmación y comprobación de identidad. Tests remotos `tabstrip_rename_reorder_favorite`, `confirm_only_yes_no`, `close_busy_tab_cancel_preserves_session`. Commit `feat(app): pestañas y estado IPC compatibles con Python`.

### Task 10: grupos, splits de workspace y arrastre compartido

**Depende de:** T9. **Entrega:** composición GTK del documento compartido y operaciones de docking.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/workspace_view.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/workspace.rs` y `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/workspace_view.rs`.

**Interfaces:** `prune(tree: &Value, present: &BTreeSet<String>) -> Value`, `shape(tree: &Value) -> Value`, `split_paths(tree: &Value) -> Vec<(Vec<usize>, f64)>`, `dock_target(layout: &Value, x: f64, y: f64, moved: &BTreeSet<String>) -> Option<DockTarget>`, `DockTarget { group: String, tab: String, edge: DockEdge }`, `DockEdge { Left, Right, Top, Bottom, Center }`. `WorkspaceView::apply(&self, doc: &Value)`, `select(&self, key: &str) -> bool`, `commit(&self, doc: &Value, focus: Option<&str>)`. Reutiliza comandos_core::workspace::layout::{move_tab,detach_tab,resize_split,move_tab_group,sort_groups,restore_order} con sus firmas reales, no reescribe transformaciones ya portadas.

Prueba inicial concreta en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/workspace_view.rs`, junto a los casos detallados en los pasos. Los tipos deben derivar PartialEq/Debug donde estas aserciones lo requieren.

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::workspace_view::dock_target;
#[test]
fn no_layout_has_no_dock_target() {
    assert!(dock_target(&serde_json::json!([]), 10.0, 10.0, &std::collections::BTreeSet::new()).is_none());
}
```

- [ ] Comparar modelos prune/shape/split_paths/dock_target con lib/gtk_workspace por oráculo; casos sin grupo, hoja ausente, árbol colapsado, origen movido que no puede ser destino, borde/centro y columnas estrechas. Comprobar tray_rects/tray_at, half/nearest y autoscroll de tabstrip en límites. `$C test -p comandos-app --test workspace_view` debe demostrar rojo/verde.
- [ ] Construir GroupPage y Paned por árbol, reutilizar TermViews existentes sin detach/reattach innecesario, preservar foco de hoja y proporciones; freeze notificaciones mientras se aplica doc. Los cambios de tamaño envían ratio por path de split, no índices visuales transitorios.
- [ ] Portar drag state idle/pressed/lifted/docking/cancelled, coordenadas root→layer, ghost/lifted/trays, grab, escape/cancel, botón soltado fuera y desplazamiento en borde. Ninguna operación local destruye el widget de sesión hasta confirmar transición.
- [ ] POST /workspace con requestId de 12 bytes aleatorios, expectedRevision y document; aplicar respuesta del servidor como autoridad. En 409 aplicar current, restaurar foco válido y notificar. Bloquear nuevas publicaciones mientras posting; no perder actualizaciones que llegan durante restore. Shadow solo dibuja doc y simula preview, no publica. Pruebas remotas `workspace_drag_between_groups`, `workspace_conflict_keeps_remote_state`, `workspace_split_drag_cancel`. Commit `feat(app): workspace GTK con docking y revisiones compartidas`.

### Task 11: identificación de conversaciones, snapshots y restauración exacta

**Depende de:** T3, T9, T10. **Entrega:** arranque/restauración que conserva identidad y layouts.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/resume.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/snapshot.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/restore.rs`; tests `resume`, `snapshot`, `restore` en el directorio absoluto de tests de comandos-app.

**Interfaces:** `sane_flags(raw: &Value, agent: &str) -> Vec<String>`, `resume_command(snapshot: &Value) -> Option<String>`, `exact_resume_command(pane: &Value) -> Option<String>`; `RestorePlan::build(tabs: &Value, snapshots: &Value, present: &BTreeSet<String>, mode: RunMode) -> Result<Self, RestoreError>`, `RestoreAction { AttachExisting, CreatePlaceholder, RestoreLayout, ResumeExact, ShowAmbiguity }` con key/label/snapshot asociados; `RestoreCoordinator::{default() -> Self, begin(), ready() -> bool, finish(result: RestoreResult)}`; `capture_session(tmux: &TmuxCtl, name: &str, inspector: &PaneInspector) -> Result<Value, SnapshotError>`, `carry_resume_ids(captured: Value, previous: &Value) -> Value`, `carry_pane_keys(captured: Value, previous: &Value) -> Value`.

Prueba inicial concreta en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/restore.rs`, junto a los casos detallados en los pasos. Los tipos deben derivar PartialEq/Debug donde estas aserciones lo requieren.

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::restore::RestoreCoordinator;
#[test]
fn startup_blocks_snapshots_until_restore_finishes() {
    let mut coordinator = RestoreCoordinator::default();
    coordinator.begin();
    assert!(!coordinator.ready());
}
```

- [ ] Crear oráculos AST de _sane_flags/resume_command/exact_resume_command y lib/tmux_snapshot sobre datos temporales. Casos Claude con UUID, Codex con rollout exacto y -c de configuración, etiquetas/cwd con comillas/Unicode, pane_id remapeado, proceso reciclado y falta/ambigüedad real de identity. No usar --last ni --continue para adivinar conversación. `$C test -p comandos-app --test resume --test snapshot --test restore` rojo/verde.
- [ ] Reutilizar comandos_runtime::pane_snapshot::PaneInspector y comandos_core::workspace::snapshot::{check_snapshot,layout_checksum,leaf_ids}. Portar remap/capture/carry validando checksum/layout y correspondencia por pane key/id exacto. La lectura de procesos va por una interfaz inyectable; tests usan árboles /proc ficticios, no procesos reales de agentes.
- [ ] Restaurar primero sesiones existentes sin comandos de agentes; para una ausente, crear placeholder tipado, recrear split/cwd/tamaño, mapear nuevos pane ids, enviar únicamente comandos exactos con buffers stdin del adaptador tmux y Enter, nunca concatenar command chains. Las pruebas usan runner Rust grabador y no invocan claude/codex/ssh. Aislar lanzamientos nuevos de agente en scopes transitorios propios por panel mediante interfaz ScopeLauncher con argv explícito; tests verifican argumentos y error, sin systemd real.
- [ ] Puerta RestoreCoordinator inhibe snapshots/escritura de app-tabs hasta que todas las operaciones terminen o queden como error visible. En sombra no crea placeholders, no reanuda, no cambia layout ni guarda snapshots; solo adjunta a existentes según selección manual del usuario. snapshot_layouts 5 s y snapshot_tabs 30 s se serializan, no pisan estado completo con arranque parcial; cierre espera únicamente trabajos propios con plazo.
- [ ] `no_snapshot_before_restore_finishes`, `resume_matches_python_oracle`, `restore_existing_never_sends_resume`, `failed_restore_cleans_only_owned_placeholder`, `snapshot_ids_survive_pane_remap`, `scope_launch_is_per_pane` son obligatorias. Entregar fixture mixto de agentes/splits sin ejecutar agentes para T12. Commit `feat(app): restauración exacta de sesiones y conversaciones`.

### Task 12: puertas de paridad de los cimientos y layout

**Depende de:** T6-T11 y puerta remota T7 disponible. **Entrega:** evidencia reproducible antes de integrar superficies 4b.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/src/app_layout.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/verification/app-layout.json` y `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/verification/app-pixel-gates.md`; capturas remotas se transfieren al directorio local absoluto `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/verification/app-captures/`.

**Interfaces:** `cargo xtask app-layout --reference <ruta absoluta> --candidate <ruta absoluta>` compara documentos LayoutDump normalizados; `cargo xtask app-shot --remote macmini --manifest <ruta absoluta> --output <ruta absoluta>` produce PNG, dump, procesos y manifiesto de entorno, con procedencia remota. `cargo xtask png-diff --reference <ruta absoluta> --candidate <ruta absoluta>` devuelve exit 0 solo ≤0,1 %, exit 1 por diferencia y 2 por entrada/capacidad ausente.

El comparador puro expone `compare_layout(reference: &Value, candidate: &Value) -> Result<LayoutDiff, LayoutError>` y `LayoutDiff::matches() -> bool` para que el CLI y tests usen la misma regla. El crate xtask expone sus módulos de comparadores mediante lib.rs sin inicializar GTK.

Prueba inicial concreta que debe fallar antes de la implementación:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::disallowed_methods)]
use xtask::app_layout::compare_layout;
#[test]
fn a_missing_tab_is_not_an_ignorable_layout_difference() {
    let reference = serde_json::json!({"tabs":[{"key":"fixture","selected":true}]});
    let candidate = serde_json::json!({"tabs":[]});
    assert!(!compare_layout(&reference, &candidate).expect("valid documents").matches());
}
```

- [ ] Implementar prueba nativa de comparación de dumps: mismo orden/splits/tamaños/labels/selected ids, números finitos y tolerancia geométrica explícita de redondeo ≤1 píxel CSS. Ignorar solo timestamps/IDs efímeros declarados, nunca contenido/pestañas. Comprobar error en tamaño/tema/fuente distintos.
- [ ] En remoto usar dos copias privadas del mismo estado y las mismas sesiones privadas, fixture de tablero, fuente, DPR, tema, dimensiones y animaciones en instante fijado. No ejecutar oracle y candidato sobre el perfil real, ni hacer attach al tmux del usuario. Capturar una vez asentados carga, restore y layout; la espera tiene deadline/error.
- [ ] `pixel_gate_shell`, `pixel_gate_tabs_workspace`, `pixel_gate_terminal_attrs`, `pixel_gate_terminal_utf8_ime`, `pixel_gate_terminal_mouse_copy`, `pixel_gate_restore` producen diff por región y total. La región dashboard exige mismo DOM/render sin cambios; terminal compara ANSI16/256/RGB, dim/bold, wide/emoji/combining, caja/Powerline, underline/tachado/cursor/selection/scrollback y sincronización. No ajustar umbrales tras ver fallo para aprobarlo; investigar VTE→motor y corregirlo.
- [ ] Registrar memoria/procesos/CPU de reposo y tiempos de arranque de ambos sobre carga idéntica. Conservar no solo resumen sino comandos, hashes, entorno y archivos originales. Puertas ausentes fallan cerradas para cutover. `$C test -p xtask app_layout` verifica comparador local puro; UI se valida únicamente por el runtime remoto. Commit `docs(verification): puertas de paridad de terminal y workspace`.

## 4b: superficies de escritorio sobre contratos consolidados

### Task 13: puente centro, dispatcher de app y presencia

**Depende de:** T12 para ejecución visual; modelos puros pueden avanzar tras T9/T10/T11. **Entrega:** mensajes y acciones con registro único para T14-T18.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/bridge.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/app_commands.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/presence.rs`; tests `bridge`, `app_commands`, `presence` en el directorio absoluto de tests.

**Interfaces:** `BridgeMsg` enum `Extensions { session, pane, harness }`, `Reader { action, id, on }`, `ButtonStyle(String)`, `HeaderAction(String)`, `SidebarTerm(Value)`, `LeftPanel(String)`, `ChainModal(Value)`, `Theme(String)`, `OpenUrl { url, label, modal: bool }`, `Rename { session, label }`, `OpenSession { session, window, label: Option<String> }`. Campos no tipados aparte son String/bool indicados; `parse_bridge(raw: &str) -> Result<BridgeMsg, BridgeError>` con prioridad de ramas idéntica a on_msg. `AppCommand { name: String, args: Value }`, `parse_command(&Value) -> Result<AppCommand, CommandError>`. `Presence::payload(now_ms: u64, interaction: bool, visible: bool, device_id: &str, mode: RunMode) -> Option<Value>`. Cada rebanada entrega `install(app: &Rc<App>)`; App guarda closures con Weak<App>, no ciclos Rc. Dispatcher rechaza comando desconocido y registra fallo explícito si la rebanada requerida no está instalada.

Prueba inicial concreta en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/bridge.rs`, junto a los casos detallados en los pasos. Los tipos deben derivar PartialEq/Debug donde estas aserciones lo requieren.

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::bridge::parse_bridge;
#[test]
fn malformed_bridge_never_reaches_ui_dispatch() {
    for raw in ["{", "null", "[]", "1"] { assert!(parse_bridge(raw).is_err()); }
}
```

- [ ] Comparar parse_bridge con todas las ramas de on_msg y errores JSON/tipos. Limitar URL a 2000 chars http/https, labels 60 web-tab/80 modal, reader id regex fecha@hora; mensaje sin session válida no toca tmux. Casos con varias claves comprueban prioridad, no ejecutar dos acciones. `$C test -p comandos-app --test bridge --test app_commands --test presence` rojo/verde.
- [ ] Registrar todas las 30 claves de APP_COMMANDS copiadas del AST; no eliminar acciones porque el consumidor aún se integra en otra rebanada. Splits/kill/side/mosaic/snippets/window/fonts/reorder llaman handlers; quit y ventana solo en fixture remoto. Adaptadores nativos usan TmuxCtl/guard/DashClient y políticas de modo; no hay Command/fs directos en handlers.
- [ ] Presencia manda /presence cada 30 s, interacción real de teclado/clic con debounce 5 s, visible desde iconified; motion/heartbeats no se declaran interacción. workspace/client conserva deviceId/activeTabId y espera focus_ready del restore. Shadow devuelve None para todo POST y no informa actividad ni focus. Fixture comprueba 0 solicitudes de Shadow.
- [ ] Adoptar JS→Rust de UserContentManager centro sin usar eval de datos sin quoting; Rust→JS usa JSON de argumentos y funciones conocidas. Destruir fuentes/monitores/handlers al cerrar. Pruebas remotas `bridge_all_messages`, `app_commands_all_actions`, `presence_interaction_vs_heartbeat`; pixel_gate_bridge con dashboard fixture. Commit `feat(app): puente del tablero, comandos y presencia por modo`.

### Task 14: cabecera, marcas, indicadores y reloj de arena

**Depende de:** T13, T8. **Entrega:** cabecera visual completa y estado de sesiones.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/header.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/marks.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/hourglass.rs`; tests `header_models`, `marks`, `hourglass` en el directorio absoluto de tests.

**Interfaces:** `header::install(app: &Rc<App>)`, `marks::adopt(payload: &Value)`, `indicator_display(mark: &Value, state: &str) -> Value`, `hourglass_sheet_frames(width: u32, height: u32, scale: f64) -> Vec<(u32,u32,u32,u32)>`, `hourglass_frame(block: &Value, now_ms: f64, flip_start_ms: Option<f64>) -> usize`; payloads/icon names/semántica se comparan contra Python.

Prueba inicial concreta que debe fallar antes de la implementación:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::disallowed_methods)]
use comandos_app::ui::hourglass::hourglass_frame;
#[test]
fn hourglass_clock_and_flip_have_distinct_frames() {
    let block = serde_json::json!({});
    assert_eq!(hourglass_frame(&block, 0.0, None), 0);
    assert_eq!(hourglass_frame(&block, 0.0, Some(0.0)), 21);
    assert_eq!(hourglass_frame(&block, 110.0, Some(0.0)), 22);
}
```

- [ ] Tests nativos de indicador por mark/state/finished/animations, frame de AI, tint SVG, porcentajes/hora y límites de spritesheet. Comparar hourglass_frame contra oráculo con reloj congelado antes/durante/después del flip. Reutilizar reglas comandos_core::work_marks, no portar otra copia de catálogo.
- [ ] Portar todos los HEADER_ACTIONS, cinco estilos de botones, Lucide/themed icons, favorite/status marks, menú de marca por sesión/pane, sticker/AI animation, hourglass polling/tick y badge de notices. Pausar animaciones ocultas, reutilizar pixbufs acotados por tema/DPR y retirar ticks al destruir, sin caché infinita.
- [ ] GET marks/acciones POST pasan por T8 Jobs y guard de modo. Una respuesta atrasada no sobrescribe selección reciente. Minimizar/maximizar/restaurar se conecta solo a la Window propia. `$C test -p comandos-app --test header_models --test marks --test hourglass`; remoto `pixel_gate_header_marks_hourglass` en los nueve temas y estilos; fixture verifica menú y badge sincronizado. Commit `feat(app): cabecera, marcas e indicadores con reloj de arena`.

### Task 15: atajos, switcher y parada segura de agentes

**Depende de:** T13, T9. **Entrega:** navegación/ayuda y Ctrl+C semántico sin matar procesos ajenos.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/keys.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/switcher.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/help.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/agent_stop.rs`; tests `keys`, `switcher`, `agent_stop` en el directorio absoluto de tests.

**Interfaces:** `fuzzy_score(query: &str, candidate: &str) -> i32`, `CtrlCAction { Copy, SendInterrupt, Cleanup }`, `ctrl_c_action(has_selection: bool, copy_context: bool, now: f64, last: f64, window: f64) -> CtrlCAction`; `ProcessRecord { pid: u32, parent: u32, start_time: u64, argv: Vec<String> }`, `StopPlan::build(root: u32, records: &[ProcessRecord], client_state: &Value) -> StopPlan`. Execution uses injected `ProcessReader` and `SignalSink` that require unchanged pid/start_time. Pane destruction has separate `PaneCloseIntent` proof binding socket/session/pane identity and user confirmation; no general kill-pane string appended to a command list.

Prueba inicial concreta en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/agent_stop.rs`, junto a los casos detallados en los pasos. Los tipos deben derivar PartialEq/Debug donde estas aserciones lo requieren.

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::agent_stop::{ctrl_c_action, CtrlCAction};
#[test]
fn selection_uses_copy_before_stop() {
    assert_eq!(ctrl_c_action(true, false, 1.0, 0.9, 0.4), CtrlCAction::Copy);
}
```

- [ ] Portar todas las branches de on_key, MRU/cycle, fuzzy matching/switcher/overview y help rows. Test selection Ctrl+C copia; sin selección primer Ctrl+C interrumpe; doble pulsación dentro de ventana aplica cleanup solo si foreground y client state lo permiten; copy-mode/prefix bloquean cleanup. lib/agent_stop es oráculo de árbol/proceso, no se carga toda la app.
- [ ] Pruebas ficticias de descendientes detached, PID reciclado, parent cambiado, shell no agente, nombres Claude/Codex/Gemini/agy/opencode y process race entre leer y señalar. SignalSink grabador exige cero señales a registros cambiados. Test de ejecución real, si necesario, crea únicamente helper Rust propio con identificador de nacimiento y grupo privado; nunca lee/señala procesos reales de agentes ni usa pkill.
- [ ] Limitar cleanup al agente foreground y descendientes demostrados; no matar servidor tmux, shells no asociados ni todos los hijos de la app. Shadow no ejecuta señales, kill pane ni teclas mutantes de automatización. Cierre de pane usa adaptador tipado y confirmación, con prueba de identity antes de actuar y error visible si cambió.
- [ ] `$C test -p comandos-app --test keys --test switcher --test agent_stop`; remoto `switcher_mru_overview_keyboard`, `help_escape_and_backdrop`, `ctrl_c_selection_then_interrupt`, `pixel_gate_switcher_help`. Commit `feat(app): atajos, switcher y parada de agentes por identidad`.

### Task 16: menús de terminal, portapapeles y snippets

**Depende de:** T13, T6, T15. **Entrega:** copiar/pegar/exportar/revelar y editor de snippets.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/menu.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/clipboard.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/snippets.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/clipboard_bridge.rs`; tests `clipboard_bridge`, `snippets`, `menu_actions` en el directorio absoluto de tests.

**Interfaces:** `newest_auto(listing: &str) -> Option<String>`, `Snippet { name: String, tags: Vec<String>, body: String }`, `filter_snippets(items: &[Snippet], query: &str) -> Vec<usize>`, `paste_snippet(session: &str, text: &str, tmux: &TmuxCtl) -> Result<(), ClipboardError>`; `MenuAction` enum por acciones reales Python y `clean_local_path(url: &str) -> Option<PathBuf>`. GtkClipboard vive únicamente en MainContext; watcher tmux entrega String por hilo cancelable y nunca consume buffer de usuario en Shadow.

Prueba inicial concreta en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/snippets.rs`, junto a los casos detallados en los pasos. Los tipos deben derivar PartialEq/Debug donde estas aserciones lo requieren.

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::snippets::{filter_snippets, Snippet};
#[test]
fn filter_matches_text_without_changing_body() {
    let items = vec![Snippet { name: "ñandú".into(), tags: vec!["rust".into()], body: "printf x; printf y".into() }];
    assert_eq!(filter_snippets(&items, "ñandú"), vec![0]);
    assert_eq!(items[0].body, "printf x; printf y");
}
```

- [ ] Tests oráculo de newest_auto/snippets/filter/clean_local_path, URL envuelta/OSC8, paths file:// y percent encoding; rechazo de esquema/comando ejecutable no permitido. Pegar UTF-8 y texto con ';', newline, emoji y secuencias de shell via mutate_with_stdin load-buffer, paste-buffer y delete-buffer propio; no new-session shell ni expansión interpolada. Se limpia buffer solo si la app lo creó.
- [ ] Portar copy VTE→selection_text, copy tmux copy-mode, exit_copy_mode, clipboard→PTY encode_paste, auto clipboard watcher, reply export/copy y notification feedback. OSC52 respeta modo y límite del motor; no automatizar lectura/escritura del clipboard personal desde tests locales.
- [ ] SnippetsDialog conserva búsqueda, lista/preview, editar/guardar/borrar, tags/name/body, flash/error y envío a active-session exacta. snip_load/snip_save usa StateFiles y JSON Python; log solo append_if_exists y ningún dump de credenciales. Menú terminal conserva split/harness/paste/copy/export/reveal/mark; abrir URL/revelar usa argv de proc::spawn_detached y runner falso en tests, nunca shell interpolado.
- [ ] `$C test -p comandos-app --test clipboard_bridge --test snippets --test menu_actions`; remoto `clipboard_unicode_bracketed_paste`, `snippet_crud_and_send`, `terminal_context_menu_all_actions`, `pixel_gate_snippets_menu`. Commit `feat(app): menús, portapapeles y snippets compatibles`.

### Task 17: overlays de panes, cuentas y extensiones

**Depende de:** T13, T14, T16. **Entrega:** barras de modelo/cuenta/harness, marcos y gutters interactivos.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/overlays.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/accounts.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/extensions.rs`; tests `overlay_geometry`, `account_flow`, `extension_bridge` en el directorio absoluto de tests.

**Interfaces:** `PaneGeometry { id: String, left: u16, top: u16, width: u16, height: u16 }`, `pane_frames(rows: &[PaneGeometry], geom: &CellGeom, focused: bool) -> Vec<Value>`, `gutter_target(geometry: &[PaneGeometry], pane: &str, orientation: &str, cell: f64) -> Option<Value>`, `shell_pill_xy(rows: &BTreeMap<String, (f64,f64,f64,f64)>, pane_id: &str, margins: (f64,f64), pane_origin: (u16,u16), cell: (f64,f64)) -> (i32,i32)` con coordenadas/márgenes exactos del oráculo. `AccountFlow { operation_id: String, alias: String, stage: AccountStage, attempts: u16 }`, `AccountStage { Loading, AwaitingLogin, Switching, Complete, Failed }`, `adopt_response(&mut self, operation_id: &str, response: &Value) -> bool`. `shelf_height(total: f64, saved: Option<f64>) -> f64` y `extension_message(raw: &str) -> Result<Value, BridgeError>`.

Prueba inicial concreta en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/account_flow.rs`, junto a los casos detallados en los pasos. Los tipos deben derivar PartialEq/Debug donde estas aserciones lo requieren.

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::accounts::{AccountFlow, AccountStage};
#[test]
fn previous_account_operation_cannot_complete_current_one() {
    let mut flow = AccountFlow { operation_id: "new".into(), alias: "fixture".into(), stage: AccountStage::Switching, attempts: 0 };
    assert!(!flow.adopt_response("old", &serde_json::json!({"stage":"complete"})));
    assert_eq!(flow.stage, AccountStage::Switching);
}
```

- [ ] Oráculos de pill/card/gutter/half/grip/pane_frames con pane grids de tmux y métricas pango, focus/no-focus, niveles DPR y clipping. Tests prueban que resize no usa geometría antigua y que offset/márgenes/DPR no se cuentan dos veces. Un grip solo cambia el split vecino elegido, sin salto al pane incorrecto.
- [ ] Portar overlays/model badges/key controls/shell pill/AI card/active frames, prefix watch, gutters drag/release y refresh models. Snapshot models se escribe solo por guard y después de restore. Coalescer geometría con un trabajo en vuelo por box; respuestas viejas se descartan si pane/session cambiaron.
- [ ] Cuenta popover conserva login/switch/list/quota y barra porcentual; operación se identifica por operation_id+alias, espera hasta 120 intentos con backoff y cancelación cuando se destruye popover. No aceptar respuesta de operación anterior ni anunciar Complete antes de estado final. Fixtures HTTP con credenciales ficticias; ningún login real ni claves/alias privados en evidencia.
- [ ] Shelf extensiones conserva height/save debounce, abrir/cerrar, WebView/handler propio y notices toggle. El perfil es por modo de T7 y el mensaje no puede actuar sobre otro pane. Shadow muestra datos sin login/switch/POST ni persistir shelf. `$C test -p comandos-app --test overlay_geometry --test account_flow --test extension_bridge`; remoto `gutter_drag_focus`, `account_switch_stale_operation`, `extension_shelf_messages`, `pixel_gate_pane_cards_accounts_extensions`. Commit `feat(app): overlays, cuentas y extensiones por pane`.

### Task 18: panel lateral, mosaico, reader y modales web

**Depende de:** T13, T17, T10. **Entrega:** todas las disposiciones y vistas auxiliares del escritorio.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/side.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/mosaic.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/reader.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/ui/modals.rs`; tests `side_layout`, `mosaic`, `reader` en el directorio absoluto de tests.

**Interfaces:** `reader_layout_state(open: bool, terminal: bool) -> Value`, `SideState::from_json(&Value) -> Result<Self, LayoutError>`, `SideState::to_json() -> Value`, `MosaicState::{open(sessions: &[Value]), close(), zoom(session: &str), unzoom()}` puro. `ModalKind { Url, Chains, Analytics, Help, Snippets }`; `show_modal_panel(app: &Rc<App>, panel: &gtk::Widget, dismissable: bool, on_close: Rc<dyn Fn()>)`. Las APIs install de T13 conectan mensajes Reader/SidebarTerm/LeftPanel/ChainModal/OpenUrl y APP_COMMANDS de mosaico/layout.

Prueba inicial concreta en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/reader.rs`, junto a los casos detallados en los pasos. Los tipos deben derivar PartialEq/Debug donde estas aserciones lo requieren.

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::reader::reader_layout_state;
#[test]
fn reader_with_terminal_keeps_both_regions() {
    assert_eq!(reader_layout_state(true, true), serde_json::json!({"terminals":true,"reader":true}));
    assert_eq!(reader_layout_state(true, false), serde_json::json!({"terminals":false,"reader":true}));
}
```

- [ ] Tests de reader open/terminal combinations, side hidden/collapsed/pinned/share/tab order y límite de cmds_h 0..4000; shell/sidebar ownership y sesión activa no se pierden por cambiar tabs. Portar all side_* del Apéndice A, drag header, share/save debounce, arrows/wheel y left-panel snapshot/tween; retirar fuentes y capturas tras transición.
- [ ] Mosaico lista mismas sesiones/orden, TermViews con scrollback 400, resize sin encoger sesiones fuera del modo autorizado y zoom/unzoom conservando session. No crear un nuevo agente por celda ni adjuntar a sesiones reales en tests. Anular generación del fill si se cierra antes de terminar; evitar widgets/fuentes vivos al abrir/cerrar repetidamente.
- [ ] Layout row/rows, sort_menu/sort_tabs y cycle_page operan sobre workspace compartido cuando existe; no ordenar solo notebook y revertirse con polling. Layout/pane-position conserva valores Python, lock/guard y persist=false respetado. Web tabs/modal/chains/analytics tienen controles de cerrar/Escape/backdrop según dismissable, handlers propios y permisos de T7; al cerrar se destruye WebView y fuentes de timeout.
- [ ] `$C test -p comandos-app --test side_layout --test mosaic --test reader`; remoto `side_drag_collapse_pin_share`, `mosaic_reopen_zoom_release`, `reader_terminal_toggle`, `chain_analytics_modal_close`, `pixel_gate_side_mosaic_reader_modals`. Registrar recuento de clientes privados antes/después de 20 ciclos, debe volver al valor inicial. Commit `feat(app): panel lateral, mosaico, reader y modales`.

### Task 19: adaptador al notifyd existente y API compartida

**Depende de:** T1, T8. **Paralelizable:** con tareas 4b, integración controlada de main/manifest. **Entrega:** Entry::Notifyd funciona reutilizando el crate de Fase 2f, sin otro servidor de popups.

**Files:** Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/src/notifyd.rs` y `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/notifyd_adapter.rs`; modify `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-notifyd/src/lib.rs`, `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-notifyd/src/main.rs`, y extraer configuración/entry existentes a `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-notifyd/src/entry.rs`. No crear notifyd/popup, markdown, stack ni otro http server en comandos-app.

**Interfaces:** `comandos_notifyd::entry::run(args: &[OsString]) -> ExitCode`, `entry::parse_options(args: &[OsString]) -> Result<Options, String>` conserva exactamente flags y defaults del binario existente, y `comandos_app::notifyd::run(args: &[String]) -> ExitCode` convierte argumentos y delega. Los main de ambos binarios llaman una misma entry. Extraer funciones existentes, sin reinterpretar sus contratos. Se mantienen http::serve, popup::Context y módulos model/notice/actions/markup/stack/position/sweep/theme/dash existentes.

Prueba inicial concreta en `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-app/tests/notifyd_adapter.rs`, junto a los casos detallados en los pasos. Los tipos deben derivar PartialEq/Debug donde estas aserciones lo requieren.

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[test]
fn notifyd_adapter_rejects_unknown_args_before_gtk() {
    let args = vec![std::ffi::OsString::from("--unknown-fixture")];
    assert!(comandos_notifyd::entry::parse_options(&args).is_err());
}
```

- [ ] Capturar argv/help/errores del notifyd actual con pruebas parse-only, y extraer entry sin cambiar comportamiento. El test nunca llama run con argumentos que arranquen GTK; usar parse_options y error de opción inválida para probar delegación. `--headless` se verifica con fixture HTTP propio 7312, hooks temporales, dash fixture 7311 y socket privado explícito, nunca default 4778 ni estado real.
- [ ] Exponer Options solo al nivel requerido y añadir origen de configuración a APIs de tema compartidas de T8. `Entry::Notifyd` no se elimina ni se convierte en rechazo. En producción su configuración sigue siendo la del notifyd existente; no activar dos listeners ni reinstalar symlink de cc-notifyd en T20.
- [ ] `$C test -p comandos-notifyd -p comandos-app -j2`, regresiones markup/theme/position/stack/actions sin pantalla y `notifyd_adapter_uses_existing_entry`. Remoto solo si se requiere popup integration, `pixel_gate_notifyd_adapter` usa fixture que ya era referencia de 2f. Commit `refactor(notifyd): exponer entrada compartida para el adaptador GTK`.

### Task 20: aceptación completa, empaquetado y cutover reversible de cc-app

**Depende de:** T1-T19, revisión independiente y todas las puertas remotas. **Entrega:** candidato instalable, evidencia y rollback; el cambio live queda para la autorización concreta de Jesús presente.

**Files:** Modify `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-cli/src/install.rs` y `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-cli/src/install/release.rs`; tests `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-cli/tests/install.rs` y `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-cli/tests/install_release.rs`. Create `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/verification/cutover-app.md` y `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/docs/verification/app-function-coverage.json`. El paquete de release sigue el esquema versionado ya aprobado, no inventa otro layout ni modifica servicios activos durante desarrollo.

**Interfaces:** `comandos install --link cc-app --dry-run` produce cambios concretos y ruta absoluta de rollback, sin escribir enlaces/servicios reales; `comandos install --rollback cc-app --dry-run` muestra inversa. T20 añade --dry-run y --stage-app <ruta absoluta del binario> a la API actual --stage/--link/--rollback; esas dos flags todavía no existen. --link cc-app debe usar release de comandos-app, no el binario headless comandos. Los tests usan --home con raíz ficticia y nunca systemctl real. Se conserva el dispatcher cc-app→live y comandos-app→sandbox, notifyd sin tocar.

El fixture Rust InstallFixture crea HOME privado con enlace anterior y binario candidato generado por el helper de pruebas existente. Expone new, old_cc_app_symlink, stage_app_candidate, cc_app_path e install; install añade --home del fixture y nunca usa HOME personal. El resultado tiene code/stdout. No ejecuta systemctl.

Prueba inicial concreta que debe fallar antes de la implementación:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::disallowed_methods)]
#[test]
fn cc_app_dry_run_keeps_the_existing_link() {
    let fixture = InstallFixture::new();
    fixture.old_cc_app_symlink();
    fixture.stage_app_candidate();
    let before = std::fs::read_link(fixture.cc_app_path()).expect("old link");
    let result = fixture.install(&["--link", "cc-app", "--dry-run"]);
    assert_eq!(result.code, 0);
    assert_eq!(std::fs::read_link(fixture.cc_app_path()).expect("same link"), before);
    assert!(result.stdout.contains("rollback"));
}
```

- [ ] Verificar cobertura por qualified-name del Apéndice A contra AST y app-drift del checkout principal. Toda función tiene implementación/evidencia o reutilización exacta referenciada; ninguna queda omitida por llamarse helper/anidada. Reconciliar definiciones nuevas/cambiadas y fixtures; baseline solo la acepta el controlador tras review. Cero deriva no aceptada es requisito el mismo día del cutover.
- [ ] Ejecutar pruebas Rust de paquetes tocados, clippy workspace all-targets -j2, fmt, cargo deny/audit según política de avisos documentada. Compilar release reproducible y registrar hash/version/dependencias. No bloquear con avisos conocidos aceptados sin diferenciar regresiones nuevas; tampoco silenciarlos sin explicación.
- [ ] Repetir todas las pixel_gate_* remotas, fuente/DPR/nueve temas/cinco estilos, 20 pestañas y mezcla de splits/conversaciones, restore parcial/cerradas/ambigüedad, IPC/bridge, cuentas ficticias, copy/IME, reader/mosaico y teardown. Comparar memoria de app tras 1 h ≤130 MB, WebProcess ≤146 MB y NetworkProcess ≤885 MB; objetivos 60/146/250 se registran por separado. Medir arranque visible/CPU/procesos, sin afirmar objetivo 600 ms cumplido sin dato.
- [ ] Experimento NetworkProcess usa copias de perfil y mismos ciclos load/reload, establece si data-manager/contexts/cachés multiplican procesos, corrige ciclo de vida si RSS crece y vuelve a medir. Oomd usa fixture con ScopeLauncher y procesos propios en runtime remoto aislado; comprobar que matar scope de un panel preserva app y tmux y demás panes. No cambiar límites/cgroups/systemd de las sesiones reales.
- [ ] Preparar candidato/rollback completos y revisar dry-run en sandbox. Cutover no reinicia tmux ni services de terceros. Tras solicitud concreta de Jesús presente, respaldar enlace cc-app vigente, activar release y reiniciar solo app, comparar pestañas/splits/cuentas/conversaciones antes/después y comprobar rollback de un comando. No ejecutar este paso por completar el plan. Fallo en cualquiera de las puertas deja candidato disponible y cutover pendiente con causa exacta.
- [ ] Guardar evidencia remota transferida con equipo y rutas originales absolutas, hashes y comandos; jamás presentar ruta macmini como local. Commit de preparación `feat(install): candidato GTK con rollback de cc-app y evidencia completa`.


## Apéndice A: cobertura exhaustiva de cc-app

La tabla asigna las 459 definiciones AST, incluidas clases y funciones anidadas, a una tarea responsable. Es cobertura del plan, no prueba de implementación terminada. Fuente comprobada `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4-plan/bin/cc-app`, 9019 líneas. La referencia de ejecución es `/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app`; toda definición nueva o cambiada detectada por app-drift necesita fila/evidencia antes de aceptar el baseline. Las clases y cada método/closure tienen fila propia.

| Tarea | Definiciones | Puerta de aceptación |
|---|---:|---|
| T1 | 1 | config/ui_lang |
| T3 | 3 | tmux_guard/proc_jobs |
| T5 | 4 | term_pty/settle |
| T6 | 19 | term_view_model y terminal remoto T12 |
| T7 | 9 | app_shell_model/pixel_gate_shell |
| T8 | 18 | dash_client/poll/theme |
| T9 | 51 | tabs/state_files/ipc/tabstrip remoto |
| T10 | 49 | workspace_view y drag remoto |
| T11 | 23 | resume/snapshot/restore |
| T13 | 17 | bridge/app_commands/presence |
| T14 | 42 | header_models/marks/hourglass |
| T15 | 34 | keys/switcher/agent_stop |
| T16 | 47 | clipboard_bridge/snippets/menu_actions |
| T17 | 67 | overlay_geometry/account_flow/extension_bridge |
| T18 | 73 | side_layout/mosaic/reader |
| T19 | 2 | notifyd_adapter existente |

Cada fila siguiente toma sus pruebas del resumen anterior. Las anidadas comparten tarea con su propietario y deben aparecer en la lista de aceptación entregada al controlador. Las líneas fijan el oráculo, no son instrucciones de parche ciego sobre el checkout principal.

| Qualified name | Línea AST | Tarea |
|---|---:|---|
| `_NullLock` | 58 | T7 |
| `_NullLock.write` | 59 | T7 |
| `_NullLock.flush` | 60 | T7 |
| `_term_line_scale` | 170 | T6 |
| `_ui_lang` | 177 | T1 |
| `_shelf_separator_css` | 348 | T8 |
| `theme_css` | 362 | T8 |
| `fetch_prefs` | 376 | T8 |
| `fetch_pref_theme` | 386 | T8 |
| `rgba` | 408 | T8 |
| `tmuxc` | 414 | T3 |
| `http_post` | 421 | T8 |
| `http_ensure` | 438 | T8 |
| `snip_load` | 446 | T16 |
| `snip_save` | 457 | T16 |
| `_snip_env_snapshot` | 475 | T16 |
| `snip_log` | 495 | T16 |
| `snip_paste` | 509 | T16 |
| `normalize_wrapped_url_text` | 547 | T6 |
| `url_from_wrapped_text` | 553 | T6 |
| `url_from_wrapped_text.joined` | 571 | T6 |
| `cell_at_event` | 601 | T6 |
| `url_at_event` | 614 | T6 |
| `ssh_host_from_session` | 644 | T6 |
| `terminal_grid_at_event` | 662 | T6 |
| `_ssh_scroll_amount` | 676 | T6 |
| `_flush_ssh_scroll` | 696 | T6 |
| `on_ssh_scroll` | 714 | T6 |
| `_bg_rgba` | 740 | T6 |
| `_tmux_window_size` | 749 | T5 |
| `_spawn_when_settled` | 760 | T5 |
| `_spawn_when_settled.go` | 768 | T5 |
| `_spawn_when_settled.on_alloc` | 779 | T5 |
| `make_term` | 789 | T6 |
| `make_term._on_drop` | 829 | T6 |
| `make_term.spawn` | 849 | T6 |
| `feed` | 899 | T6 |
| `new_session_dialog` | 906 | T9 |
| `new_session_dialog.done` | 926 | T9 |
| `new_session_dialog.on_response` | 937 | T9 |
| `term_session` | 956 | T6 |
| `pane_at` | 969 | T16 |
| `copy_mode_pane` | 994 | T16 |
| `select_pane_at_event` | 1012 | T16 |
| `copy_text_to_clipboard` | 1020 | T16 |
| `copy_vte_selection` | 1032 | T16 |
| `copy_tmux_selection_to_clipboard` | 1044 | T16 |
| `notify_popup` | 1058 | T19 |
| `notify_popup.send` | 1061 | T19 |
| `export_claude_reply` | 1074 | T16 |
| `export_claude_reply.work` | 1077 | T16 |
| `copy_claude_reply` | 1089 | T16 |
| `copy_claude_reply.found` | 1092 | T16 |
| `copy_claude_reply.work` | 1099 | T16 |
| `exit_copy_mode` | 1118 | T16 |
| `open_url` | 1130 | T16 |
| `_clean_local_path` | 1176 | T16 |
| `reveal_in_file_manager` | 1184 | T16 |
| `link_actions_popover` | 1214 | T16 |
| `link_actions_popover.row` | 1239 | T16 |
| `link_actions_popover.row.go` | 1249 | T16 |
| `on_term_release` | 1276 | T16 |
| `on_term_button` | 1298 | T16 |
| `on_term_button.item` | 1330 | T16 |
| `on_term_button.split` | 1344 | T16 |
| `on_term_button.do_copy` | 1362 | T16 |
| `_build_hb_css` | 1460 | T8 |
| `button_style_css` | 1810 | T8 |
| `apply_button_style` | 1875 | T8 |
| `tab_hb` | 1909 | T9 |
| `notebook_pages` | 1917 | T9 |
| `current_tab_order` | 1922 | T9 |
| `enforce_tab_order` | 1934 | T9 |
| `_paint_favorite_button` | 1954 | T9 |
| `refresh_favorite_buttons` | 1976 | T9 |
| `apply_tab_favorites` | 1994 | T9 |
| `toggle_tab_favorite` | 2009 | T9 |
| `toggle_tab_favorite.finish` | 2018 | T9 |
| `toggle_tab_favorite.persist` | 2031 | T9 |
| `work_mark_row` | 2050 | T14 |
| `_work_mark_pixbuf` | 2055 | T14 |
| `ai_frame_pixbuf` | 2064 | T14 |
| `_indicator_pixbuf` | 2076 | T14 |
| `_set_indicator_frame` | 2093 | T14 |
| `tab_indicator_display` | 2104 | T14 |
| `paint_tab_mark` | 2111 | T14 |
| `_paint_tab_sticker` | 2134 | T14 |
| `_animations_enabled` | 2152 | T14 |
| `_indicator_animate` | 2159 | T14 |
| `_indicator_tick` | 2167 | T14 |
| `_paint_all_tab_marks` | 2207 | T14 |
| `apply_work_marks` | 2213 | T14 |
| `adopt_work_mark` | 2221 | T14 |
| `start_work_marks_fetch` | 2227 | T14 |
| `start_work_marks_fetch.run` | 2232 | T14 |
| `set_work_mark` | 2244 | T14 |
| `set_work_mark.run` | 2251 | T14 |
| `append_work_mark_menu` | 2274 | T14 |
| `_mark_menu_item` | 2309 | T14 |
| `tab_indicator_menu` | 2333 | T14 |
| `ordered_tab_labels` | 2365 | T9 |
| `_file_lock` | 2377 | T9 |
| `save_tabs` | 2387 | T9 |
| `on_tab_reordered` | 2409 | T9 |
| `saved_tab_label` | 2413 | T9 |
| `archive_tab` | 2424 | T9 |
| `_which_cli` | 2484 | T11 |
| `_sane_flags` | 2499 | T11 |
| `_pid_alive` | 2536 | T11 |
| `_proc_flags` | 2544 | T11 |
| `_claude_by_tmux_session` | 2555 | T11 |
| `_ppid_map` | 2587 | T11 |
| `_codex_info_for_session` | 2604 | T11 |
| `snapshot_layouts` | 2639 | T11 |
| `exact_resume_command` | 2670 | T11 |
| `restore_saved_layout` | 2680 | T11 |
| `prepare_local_terminal` | 2688 | T11 |
| `finish_startup_restore` | 2694 | T11 |
| `finish_startup_restore.ready` | 2695 | T11 |
| `snapshot_tabs` | 2712 | T11 |
| `load_tab_snapshot` | 2785 | T11 |
| `resume_command` | 2794 | T11 |
| `_send_resume` | 2838 | T11 |
| `_restore_one` | 2848 | T11 |
| `restore_tabs` | 2888 | T11 |
| `restore_tabs.finish` | 2893 | T11 |
| `restore_tabs.open_xterm` | 2909 | T11 |
| `restore_tabs.open_restored` | 2917 | T11 |
| `restore_tabs.work` | 2925 | T11 |
| `_apply_tab_rename_css` | 2960 | T9 |
| `tab_label` | 2987 | T9 |
| `tab_label._open_state_menu` | 2999 | T9 |
| `tab_label.begin_rename` | 3063 | T9 |
| `tab_label.begin_rename.finish` | 3088 | T9 |
| `tab_label.begin_rename.on_key` | 3102 | T9 |
| `tab_label.on_click` | 3113 | T9 |
| `update_dots` | 3142 | T9 |
| `apply_terminal_prefs` | 3154 | T6 |
| `_queue_poll_update` | 3203 | T8 |
| `_drain_poll_updates` | 3217 | T8 |
| `notices_watch_loop` | 3231 | T8 |
| `poll_state_loop` | 3246 | T8 |
| `_confirm_must_answer` | 3283 | T9 |
| `_confirm_must_answer._finish` | 3325 | T9 |
| `_confirm_must_answer.on_key` | 3339 | T9 |
| `close_tab` | 3352 | T9 |
| `current_cwd` | 3391 | T9 |
| `_create_term_session` | 3408 | T9 |
| `new_local_tab` | 3432 | T9 |
| `new_local_tab.done` | 3436 | T9 |
| `new_local_tab.work` | 3444 | T9 |
| `request_quick_terminal` | 3462 | T9 |
| `open_quick_terminal` | 3476 | T9 |
| `open_quick_terminal.done` | 3481 | T9 |
| `open_quick_terminal.work` | 3489 | T9 |
| `_ensure_cc_webterm` | 3498 | T9 |
| `open_xterm_tab` | 3512 | T9 |
| `open_xterm_tab.build` | 3528 | T9 |
| `open_xterm_tab.work` | 3559 | T9 |
| `select_win` | 3575 | T9 |
| `open_tab` | 3593 | T9 |
| `set_tab_label` | 3625 | T9 |
| `fuzzy_score` | 3633 | T15 |
| `close_on_click_outside` | 3651 | T15 |
| `close_on_click_outside._arm` | 3661 | T15 |
| `close_on_click_outside._release_grab` | 3675 | T15 |
| `close_on_click_outside._kill` | 3683 | T15 |
| `close_on_click_outside._dismiss` | 3688 | T15 |
| `close_on_click_outside.on_press` | 3701 | T15 |
| `close_on_click_outside.on_key` | 3713 | T15 |
| `close_on_click_outside._tick` | 3719 | T15 |
| `open_switcher` | 3740 | T15 |
| `open_switcher._close` | 3750 | T15 |
| `open_switcher.candidates` | 3765 | T15 |
| `open_switcher.refill` | 3782 | T15 |
| `open_switcher.go` | 3814 | T15 |
| `open_switcher.close_selected` | 3821 | T15 |
| `open_switcher.move_sel` | 3830 | T15 |
| `open_switcher.on_sw_key` | 3839 | T15 |
| `tab_page_label` | 3864 | T15 |
| `open_tabs_overview` | 3874 | T15 |
| `open_tabs_overview._close` | 3880 | T15 |
| `open_tabs_overview.focus_page` | 3898 | T15 |
| `open_tabs_overview.refill` | 3907 | T15 |
| `open_tabs_overview.refill.on_close` | 3932 | T15 |
| `open_tabs_overview.go` | 3943 | T15 |
| `rename_tab` | 3954 | T9 |
| `apply_theme` | 3987 | T8 |
| `_apply_tmux_theme` | 4044 | T8 |
| `ws_widget` | 4094 | T10 |
| `ws_tab_label` | 4098 | T10 |
| `ws_header` | 4110 | T10 |
| `ws_forget` | 4127 | T10 |
| `ws_select` | 4142 | T10 |
| `ws_apply` | 4152 | T10 |
| `_ws_restore_focus` | 4176 | T10 |
| `report_presence` | 4196 | T13 |
| `_presence_window_state` | 4208 | T13 |
| `_ws_save_focus` | 4217 | T13 |
| `ws_poll_loop` | 4228 | T8 |
| `tab_reorder` | 4240 | T10 |
| `ws_commit` | 4256 | T10 |
| `ws_commit.work` | 4265 | T10 |
| `ws_commit.work.done` | 4281 | T10 |
| `_dash_call` | 4296 | T8 |
| `_in_background` | 4312 | T3 |
| `_in_background.run` | 4314 | T3 |
| `close_group_guarded` | 4320 | T10 |
| `close_group_guarded.confirm` | 4328 | T10 |
| `close_group_guarded.report` | 4346 | T10 |
| `close_split_guarded` | 4357 | T10 |
| `close_split_guarded.confirm` | 4361 | T10 |
| `close_split_guarded.report` | 4376 | T10 |
| `ws_resized` | 4383 | T10 |
| `_ws_member_focused` | 4396 | T10 |
| `_ws_source_of` | 4408 | T10 |
| `ws_drag_source` | 4420 | T10 |
| `_ws_press` | 4430 | T10 |
| `_ws_layer_point` | 4439 | T10 |
| `_ws_layer_point_root` | 4444 | T10 |
| `_ws_pointer_move` | 4458 | T10 |
| `_ws_release` | 4475 | T10 |
| `_ws_window_motion` | 4484 | T10 |
| `_ws_window_release` | 4496 | T10 |
| `_ws_grab` | 4505 | T10 |
| `_ws_button_still_down` | 4518 | T10 |
| `ws_cancel_drag` | 4527 | T10 |
| `_ws_moved_ids` | 4535 | T10 |
| `ws_strip_edge_step` | 4543 | T10 |
| `_ws_edge_tick` | 4552 | T10 |
| `_ws_source_page` | 4595 | T10 |
| `_ws_drag_begin` | 4606 | T10 |
| `_ws_drag_end` | 4626 | T10 |
| `_ws_rect` | 4636 | T10 |
| `_ws_layout` | 4644 | T10 |
| `ws_tray_rects` | 4678 | T10 |
| `ws_tray_at` | 4686 | T10 |
| `_ws_target` | 4692 | T10 |
| `_ws_doc_index` | 4721 | T10 |
| `_ws_finish` | 4733 | T10 |
| `_ws_draw_trays` | 4756 | T10 |
| `_ws_rounded` | 4781 | T10 |
| `_ws_draw_ghost` | 4791 | T10 |
| `_ws_draw_lifted` | 4856 | T10 |
| `_ws_draw` | 4873 | T10 |
| `tmux_clipboard_loop` | 4920 | T16 |
| `_report_active_tab` | 4946 | T13 |
| `_dash_js` | 4973 | T13 |
| `open_ai_session_here` | 4986 | T13 |
| `_esc` | 5009 | T17 |
| `_svg_image` | 5017 | T17 |
| `_attach_model_bar` | 5031 | T17 |
| `_pane_geometry` | 5067 | T17 |
| `_shell_pill` | 5091 | T17 |
| `_card_button` | 5117 | T17 |
| `_motor_badge` | 5142 | T17 |
| `_pane_card_ai` | 5162 | T17 |
| `_pane_pill` | 5176 | T17 |
| `_pane_card_keys` | 5239 | T17 |
| `_account_bar_class` | 5253 | T17 |
| `_acct_awaiting` | 5263 | T17 |
| `_wait_account_switch` | 5270 | T17 |
| `_account_popover` | 5294 | T17 |
| `_account_popover.login` | 5318 | T17 |
| `_account_popover.login.work` | 5323 | T17 |
| `_account_popover.login.work.done` | 5325 | T17 |
| `_account_popover.switch` | 5339 | T17 |
| `_account_popover.switch.fail` | 5344 | T17 |
| `_account_popover.switch.work` | 5351 | T17 |
| `_account_popover.switch.work.done` | 5361 | T17 |
| `_account_popover.fill` | 5374 | T17 |
| `_account_popover.load` | 5429 | T17 |
| `_extension_pill` | 5440 | T17 |
| `_shelf_height` | 5452 | T17 |
| `_load_shelf_height` | 5459 | T17 |
| `_save_shelf_height` | 5467 | T17 |
| `_queue_shelf_height_save` | 5486 | T17 |
| `_toggle_notices_shelf` | 5501 | T17 |
| `_shelf_box_sync` | 5524 | T17 |
| `_close_extension_shelf` | 5532 | T17 |
| `_extension_message` | 5542 | T17 |
| `_open_extension_shelf` | 5554 | T17 |
| `_draw_pane_frames` | 5590 | T17 |
| `_shell_pill_xy` | 5656 | T17 |
| `_pill_row_y` | 5665 | T17 |
| `_visible_boxes` | 5674 | T17 |
| `_box_focused` | 5685 | T17 |
| `_gutter_at` | 5700 | T17 |
| `_gutter_neighbor` | 5711 | T17 |
| `_gutter_target` | 5733 | T17 |
| `_gutter_half` | 5744 | T17 |
| `_gutter_measure` | 5751 | T17 |
| `_grip_cursor` | 5771 | T17 |
| `_grip_rect` | 5783 | T17 |
| `_sync_grips` | 5791 | T17 |
| `_gutter_hot` | 5831 | T17 |
| `_grip_enter` | 5840 | T17 |
| `_grip_leave` | 5846 | T17 |
| `_box_cell` | 5852 | T17 |
| `_focus_frame_now` | 5861 | T17 |
| `_tmux_prefix` | 5888 | T17 |
| `_prefix_watch` | 5906 | T17 |
| `_gutter_send` | 5921 | T17 |
| `_gutter_send.done` | 5937 | T17 |
| `_term_click` | 5947 | T17 |
| `_grip_to_term` | 5954 | T17 |
| `_gutter_press` | 5961 | T17 |
| `_gutter_motion` | 5992 | T17 |
| `_gutter_release` | 6006 | T17 |
| `_pane_frames` | 6020 | T17 |
| `_card_rect` | 6057 | T17 |
| `_reposition_pills` | 6068 | T17 |
| `_pane_geo_soon` | 6114 | T17 |
| `_pane_geo_soon.run` | 6119 | T17 |
| `_place_pills` | 6134 | T17 |
| `_refresh_tab_models` | 6225 | T17 |
| `_track_mru` | 6307 | T15 |
| `_mru_toggle` | 6320 | T15 |
| `_cycle_tab` | 6329 | T15 |
| `_themed_icon_image` | 6335 | T14 |
| `_icon_btn` | 6351 | T14 |
| `show_new_tab_menu` | 6382 | T14 |
| `show_new_tab_menu.add` | 6387 | T14 |
| `_open_wizard` | 6404 | T14 |
| `_dash_click` | 6430 | T14 |
| `_dash_click.go` | 6431 | T14 |
| `_panel_popover` | 6447 | T14 |
| `_panel_popover.go` | 6450 | T14 |
| `hourglass_sheet_frames` | 6485 | T14 |
| `hourglass_frame` | 6491 | T14 |
| `_hourglass_frames` | 6506 | T14 |
| `_hourglass_poll` | 6522 | T14 |
| `_hourglass_poll.run` | 6528 | T14 |
| `_hourglass_adopt` | 6540 | T14 |
| `_hourglass_tick` | 6551 | T14 |
| `_hourglass_button` | 6566 | T14 |
| `set_notif_badge` | 6601 | T14 |
| `_toggle_max` | 6613 | T14 |
| `_tween` | 6650 | T14 |
| `_tween._tick` | 6655 | T14 |
| `_mosaic_sessions` | 6670 | T18 |
| `_mosaic_close` | 6688 | T18 |
| `_mosaic_open` | 6713 | T18 |
| `_mosaic_open._side_toggle` | 6731 | T18 |
| `_mosaic_open._fill` | 6793 | T18 |
| `_mosaic_open._fill._cell_focus_in` | 6801 | T18 |
| `_mosaic_open._fill._cell_focus_out` | 6821 | T18 |
| `_mosaic_open._fill._cell_press` | 6826 | T18 |
| `_mosaic_term` | 6851 | T18 |
| `_shrink_cell` | 6876 | T18 |
| `_mosaic_zoom` | 6893 | T18 |
| `_mosaic_zoom._zoom_out` | 6912 | T18 |
| `_mosaic_zoom._zoom_key` | 6927 | T18 |
| `_mosaic_toggle` | 6940 | T18 |
| `_dbg_js_file` | 6969 | T18 |
| `_sort_tabs` | 6999 | T18 |
| `_sort_tabs.work` | 7000 | T18 |
| `_sort_tabs.done` | 7003 | T18 |
| `_sort_menu` | 7013 | T18 |
| `_cycle_page` | 7043 | T18 |
| `apply_tabs_layout` | 7077 | T18 |
| `apply_tabs_layout._save` | 7088 | T18 |
| `open_web_modal` | 7120 | T18 |
| `open_chain_modal` | 7165 | T18 |
| `_chain_modal_msg` | 7199 | T18 |
| `open_analytics_modal` | 7219 | T18 |
| `open_web_tab` | 7246 | T18 |
| `on_msg` | 7269 | T13 |
| `on_perm` | 7354 | T7 |
| `_side_head_button` | 7399 | T18 |
| `_side_term_action` | 7414 | T18 |
| `_side_scroll_by` | 7422 | T18 |
| `_side_sync_arrows` | 7427 | T18 |
| `_side_tabs_wheel` | 7439 | T18 |
| `_layout_load` | 7495 | T18 |
| `_layout_save` | 7508 | T18 |
| `_dash_js_quiet` | 7518 | T18 |
| `_side_term_box` | 7526 | T18 |
| `_side_pin_pos` | 7544 | T18 |
| `_side_apply_pin` | 7561 | T18 |
| `_side_keep_layout` | 7569 | T18 |
| `_side_term_apply_share` | 7577 | T18 |
| `_side_term_collapse` | 7587 | T18 |
| `_side_term_keep_collapsed` | 7597 | T18 |
| `_side_tab_widget` | 7603 | T18 |
| `_side_reveal_tab` | 7631 | T18 |
| `_side_head_render` | 7642 | T18 |
| `_side_tabs_from_web` | 7667 | T18 |
| `_side_term_show` | 7679 | T18 |
| `_side_term_save_share` | 7719 | T18 |
| `_side_paned_moved` | 7728 | T18 |
| `_side_head_press` | 7741 | T18 |
| `_side_head_motion` | 7749 | T18 |
| `_side_head_release` | 7758 | T18 |
| `_side_term_focused` | 7771 | T18 |
| `_left_fx_clear` | 7788 | T18 |
| `_left_fx_snapshot` | 7799 | T18 |
| `_left_fx_run` | 7812 | T18 |
| `_left_fx_run.draw` | 7834 | T18 |
| `_left_fx_run.step` | 7866 | T18 |
| `_left_panel_set` | 7873 | T18 |
| `_left_panel_toggle` | 7904 | T18 |
| `_left_btn_paint` | 7908 | T18 |
| `reader_layout_state` | 7918 | T18 |
| `_reader_apply` | 7923 | T18 |
| `reader_open` | 7938 | T18 |
| `reader_close` | 7952 | T18 |
| `reader_toggle` | 7957 | T18 |
| `reader_terminal` | 7961 | T18 |
| `_load_pane_position` | 8017 | T7 |
| `_init_pane_position` | 8025 | T7 |
| `_save_pane_position` | 8036 | T7 |
| `_hb_press` | 8061 | T18 |
| `show_modal_panel` | 8089 | T18 |
| `show_modal_panel._close` | 8107 | T18 |
| `show_modal_panel._on_backdrop_press` | 8134 | T18 |
| `show_modal_panel._key_handler` | 8140 | T18 |
| `show_modal_panel._restack` | 8166 | T18 |
| `cur_term` | 8185 | T16 |
| `SnippetsDialog` | 8190 | T16 |
| `SnippetsDialog.__init__` | 8200 | T16 |
| `SnippetsDialog._filter` | 8282 | T16 |
| `SnippetsDialog._render_list` | 8297 | T16 |
| `SnippetsDialog._on_row` | 8316 | T16 |
| `SnippetsDialog._clear_right` | 8320 | T16 |
| `SnippetsDialog._render_preview` | 8324 | T16 |
| `SnippetsDialog._edit` | 8357 | T16 |
| `SnippetsDialog._save_from_editor` | 8387 | T16 |
| `SnippetsDialog._delete` | 8410 | T16 |
| `SnippetsDialog._close` | 8417 | T16 |
| `SnippetsDialog._flash` | 8428 | T16 |
| `SnippetsDialog._flash._clear` | 8437 | T16 |
| `SnippetsDialog._send` | 8443 | T16 |
| `_active_snippets_session` | 8456 | T16 |
| `open_snippets_dialog` | 8469 | T16 |
| `open_snippets_dialog._do_close` | 8475 | T16 |
| `show_help` | 8560 | T15 |
| `show_help._close` | 8566 | T15 |
| `show_help.on_help_key` | 8606 | T15 |
| `_agent_cleanup_worker` | 8613 | T15 |
| `_arm_agent_cleanup` | 8620 | T15 |
| `handle_ctrl_c` | 8635 | T15 |
| `on_key` | 8650 | T15 |
| `on_focus_request` | 8748 | T9 |
| `raise_main_window` | 8763 | T7 |
| `raise_main_window._drop` | 8771 | T7 |
| `on_tab_close_request` | 8783 | T9 |
| `on_tab_open_request` | 8808 | T9 |
| `_cur_page` | 8835 | T13 |
| `_cur_term` | 8839 | T13 |
| `_page_for` | 8844 | T13 |
| `_cur_pane_id` | 8852 | T13 |
| `_kill_cur_pane` | 8865 | T13 |
| `_kill_cur_pane.work` | 8875 | T13 |
| `_select_pane_cmd` | 8887 | T13 |
| `_split_cmd` | 8892 | T13 |
| `_side_toggle` | 8919 | T13 |
| `set_font_scale` | 8923 | T6 |
| `on_app_command` | 8964 | T13 |


### Bootstrap, constantes y bibliotecas fuera de def/class

| Capacidad | Dueño | Puerta |
|---|---|---|
| Single-instance/flock, argv0, HOME/runtime, WM_CLASS/title, X11/RGBA, show_all/no_show_all, foco inicial y destroy | T1/T7 | shell_model/lock/pixel_gate_shell, sombra sin activar ventana real |
| 9 THEMES, paletas ANSI, fuente, padding, opacidad y estilos GTK | T4/T6/T8/T14 | tokens/CSS/paleta exactos y pixel_gate_terminal_attrs/header |
| HEADER_ACTIONS: quickTerminal/newSession/sortMenu/notices/chains/analytics | T13/T14/T18 | todas las claves del AST tienen handler y prueba |
| APP_COMMANDS: split/split_ssh/kill_pane/toggle_window/select_pane/next_tab/prev_tab/mru_toggle/focus_page/tab_reorder/mosaic/mosaic_zoom/side_panel/terminals_visible/reload_dashboard/font_scale/open_switcher/tabs_overview/help/snippets/new_local_tab/open_xterm_tab/open_wizard/start_ai_here/copy_selection/paste_clipboard/copy_reply/window/paned_position/quit | T13 y rebanada destinataria | 30 claves y efectos de fixture, ningún no-op |
| Centro handler, load-failed, permission-request, WebAudio y perfiles/localStorage | T7/T13/T18 | puente/carga/permisos y copia de perfiles remotos |
| state/prefs/notices/workspace/marks, timers snapshot/startup/hourglass/presence | T8/T11/T14/T13 | reloj inyectado, teardown, Shadow sin POST/consume |
| Todos los documentos tabs/history/snapshot/sessions/active/models/shelf/layout/pane-position, IPC/snippets | T9/T11/T17/T18/T16 | JSON/locks y lista cerrada Global Constraints |
| Scope por agente/pane, cleanup de descendientes y ciclo PTY | T5/T11/T15/T20 | identidad/scopes de fixture, ningún proceso ajeno señalado |

| Biblioteca Python del oráculo, ruta absoluta | Tarea y reutilización |
|---|---|
| `/home/someguy/codebase/0xJesus/ComandOS/lib/gtk_tabstrip.py` | T9 comportamiento GtkNotebook, sin Python en runtime Rust |
| `/home/someguy/codebase/0xJesus/ComandOS/lib/gtk_workspace.py` | T10 geometría prune/shape/split_paths/dock_target y UI |
| `/home/someguy/codebase/0xJesus/ComandOS/lib/workspace_layout.py` | T10 reutiliza comandos_core::workspace::layout |
| `/home/someguy/codebase/0xJesus/ComandOS/lib/workspace_state.py` | T10 consume documento/revisión del servidor, sin otro store |
| `/home/someguy/codebase/0xJesus/ComandOS/lib/session_tabs.py` | T9 ordered_tab_keys/favorites diferencial |
| `/home/someguy/codebase/0xJesus/ComandOS/lib/tmux_snapshot.py` | T11 capture/remap/carry/restore/checksum |
| `/home/someguy/codebase/0xJesus/ComandOS/lib/pane_snapshot.py` | T11 reutiliza comandos_runtime::pane_snapshot::PaneInspector |
| `/home/someguy/codebase/0xJesus/ComandOS/lib/agent_stop.py` | T15 reglas y todas sus defs, efectos tipados por identidad |
| `/home/someguy/codebase/0xJesus/ComandOS/lib/tmux_clipboard.py` | T16 newest_auto diferencial y watcher cancelable |
| `/home/someguy/codebase/0xJesus/ComandOS/lib/work_marks.py` | T14 reglas comandos_core::work_marks y representación GTK |

## Orden de integración y capacidades pendientes

T1/T2 integradas y T3 ronda 15f7216 aprobada. T4 requiere comandos-term disponible en integración; T5 avanza sin T4 y T8 sin T3. Luego T6, T7, T9, T10, T11 y T12. T13 fija mensajes/dispatcher antes de T14-T18; T19 puede avanzar contra notifyd existente, coordinando manifest/main. T20 exige todas las puertas y cobertura verificada contra drift.

Capacidades externas no comprobadas por el plan: paquete de desarrollo WebKitGTK para T7 y runtime Linux GTK/WebKitGTK/oráculo VTE remotos sobre macmini con pantalla aislada y captura. No se afirma que estén instalados; Chrome remoto no los sustituye. Las tareas nativas continúan, pero paridad visual, RSS con UI y cutover permanecen cerrados mientras falte evidencia. La carrera shell ocioso frente a kill y la diferencia setsid de spawn_detached deben resolverse en consumidores T9/T15 y T16 antes de acciones reales.
