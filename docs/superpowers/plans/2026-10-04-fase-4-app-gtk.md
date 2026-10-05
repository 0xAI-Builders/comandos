# Fase 4 — `comandos-app`: escritorio GTK en Rust con `comandos-term` nativo — plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** sustituir `bin/cc-app` (Python, GTK3 + WebKit2GTK 2.50 + VTE 0.68, 9 019 líneas) por el binario Rust `comandos-app`, con terminal propio sobre `comandos-term`, idéntico en píxeles, con menos memoria y sin omitir ninguna función: al reiniciar la app se ven exactamente las mismas pestañas, splits, cuentas y conversaciones, el RSS medido es menor o igual y la vuelta atrás es un solo comando. La parte GTK de `bin/cc-notifyd` **no** está en esta fase: ya es `crates/comandos-notifyd` con su propio cutover (`docs/verification/cutover-notifyd.md`); `comandos-app` solo le habla por HTTP en el 4778, igual que el Python.

**Architecture:** un crate nuevo `crates/comandos-app` (binario `comandos-app`; despacho por `argv[0]`: `cc-app` → live, `comandos-app` → sandbox, `cc-notifyd`/`comandos-notifyd` → rechazo con aviso, porque los popups son de `comandos-notifyd`). Tres modos que el tipo hace cumplir: **sandbox** (servidor tmux propio con socket `-S` dentro de su raíz temporal, hooks temporales, tablero solo en la banda 7200–7399), **shadow** (servidor tmux del usuario con clientes `attach -f read-only,ignore-size`, estado real en solo lectura: cero escrituras en tmux, en `~/.claude/hooks`, en el tablero o en la presencia, y no consume los archivos IPC) y **live** (el sustituto real: mismos archivos, mismo candado de instancia única que el Python, así que nunca corren los dos). La lógica que no necesita GTK (tmux, archivos de estado, reanudación, plan de restauración, colores y CSS, codificación de teclado y ratón, mensajes del puente, comandos IPC, geometría de superposiciones) vive en módulos puros con pruebas sin pantalla y oráculo Python; la capa GTK es fina. El terminal es un `gtk::DrawingArea` pintado con cairo + pango sobre el modelo de `comandos-term`, con PTY propio (`pty-process`) vigilado por el bucle de GLib: un solo hilo de interfaz, sin VTE. La paridad de píxeles la vigila `cargo xtask png-diff` sobre capturas de `bin/cc-app` y de `comandos-app` tomadas con `cargo xtask app-shot` dentro de un namespace aislado con Xvfb propio.

**Tech Stack:** Rust 1.96 (edition 2024). Desde T1: gtk-rs GTK3 `gtk = "=0.18.2"` (feature `v3_24`), `gdk = "=0.18.2"`, `glib = "=0.18.5"`, `gio = "=0.18.4"`, `cairo-rs = "=0.18.5"`, `pango = "=0.18.3"`, `pangocairo = "=0.18.0"`, `gdk-pixbuf = "=0.18.5"`, `pty-process = "=0.5.3"` (API `blocking`), `async-channel = "=2.5.0"`, `ureq = "=3.4.2"` (`default-features = false`, solo HTTP a 127.0.0.1), `regex = "=1.13.1"`, `nix = "=0.31.3"` (`fs`, `signal`, `process`, `user`), `serde_json` y `sha2` (workspace), `comandos-core`, `comandos-runtime` (`pane_snapshot::PaneInspector`). Desde T4: `comandos-term` (Fase 3). Desde T7: `webkit2gtk = "=2.0.2"` (WebKit2GTK 4.1, `features = ["v2_40"]`) y `javascriptcore-rs = "=1.1.2"` (arrastran `soup3`). En `xtask` (T7): `png = "=0.18.1"`.

**Spec:** `docs/superpowers/specs/2026-10-04-comandos-rust-complete-design.md` (§2 reglas de oro, §3.2, §4.1, §4.4–4.5 scope de systemd por pane, §5 sombra y cutover, §6 `xtask png-diff` e `import -window`, §7 fila «4», §8 riesgo «Bindings GTK3», Enmienda 4). Spike del terminal: `docs/research/2026-10-04-spike-terminal-rust.md`. Planes de la Fase 3: `docs/superpowers/plans/2026-10-04-fase-3a-comandos-term.md` y `-3b-comandos-web.md`. Oráculo: `bin/cc-app` y `lib/{tmux_snapshot,agent_stop,tmux_clipboard,session_tabs,gtk_tabstrip,gtk_workspace,workspace_layout,work_marks,workspace_state,pane_snapshot}.py`; las líneas citadas son las de `bin/cc-app` en `main` `38c9167` (9 019 líneas, último cambio del archivo `3f0a01a`). Preflight de este plan: `fase4-preflight.md` (hallazgos F01–F38, rulings R0–R9), aplicado entero.

**Precondición:** las Fases 2 y 3 están fusionadas en `main` (T4 en adelante usan `crates/comandos-term`; hoy vive en `migration/rust-fase3`) y `comandos dash` sirve el 4777. El paquete de desarrollo `libwebkit2gtk-4.1-dev` debe estar instalado antes de T7: hoy solo están las bibliotecas de ejecución (`libwebkit2gtk-4.1-0 2.50.4`) y `libgtk-3-dev 3.24.33`. **Lo instala Jesús** (`sudo apt install libwebkit2gtk-4.1-dev`); ningún agente usa `sudo`. T1–T6 no lo necesitan.

**Estado al reescribir este plan (2026-10-05):** T1 implementada en `migration/rust-fase4` (`e63a6af`, en revisión); T2 implementada en `migration/rust-fase4-drift` (`49f2de0`, `01c3a1b`, `218aaa5`). Sus secciones describen lo construido.

---

## Rulings del controlador que fijan este plan

1. **Ninguna sesión se rompe ni se interrumpe.** Ningún paso reinicia el servidor tmux del usuario, ni `tmux.service`, ni la app Python en marcha, ni escribe en una sesión del usuario fuera del modo **live** ya activado por Jesús. Toda llamada a tmux de este crate y de sus pruebas lleva `-S <socket>` explícito (`TmuxCtl` y `TestTmux`); `TMUX_TMPDIR` solo no basta (tmux 3.2a lo ignora si el directorio no existe y cae en `/tmp/tmux-1000/default`). `kill-server` y `kill-session` no existen en la API mutante; el sandbox nunca apunta al servidor `default` del usuario. Las pruebas matan solo su servidor, primero `kill-server` con su `-S` y después borran el directorio.
2. **Cutover reversible y en manos de Jesús**: shadow con paridad, después todas las puertas, después un cambio reversible de `~/.local/bin/cc-app` que reinicia solo la app, y solo con Jesús presente y su OK en el chat. La vuelta es `comandos install --rollback cc-app`. Nunca se reinicia cc-app sin él. T20 no toca `cc-notifyd`.
3. **Comentarios en español, identificadores en inglés**; `unsafe_code = "forbid"` heredado del workspace; sin `unwrap`/`expect`/indexado con `[]` fuera de pruebas (`#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]` en `lib.rs`, R9); `cargo clippy -D warnings`; `cargo fmt`; versiones exactas `=x.y.z`; cero archivos Python o bash nuevos (los oráculos Python se pasan como texto a `python3 -c`, como en `crates/comandos-runtime/tests/hook_claude_parity.rs`).
4. **Bindings GTK sin `unsafe`**: todo lo que este plan necesita existe en la API segura de gtk-rs 0.18 / webkit2gtk-rs 2.0 (`DrawingArea::connect_draw`, `glib::source::unix_fd_add_local`, `UserContentManager::connect_script_message_received`, `WebsiteDataManager::builder`, `gtk::IMMulticontext`). Ningún punto de este plan necesita `unsafe`; si una tarea encuentra un hueco que solo se cubre con `unsafe`, **se detiene** y lo informa con el símbolo exacto.
5. **Independencia para worktrees paralelos**: cada tarea declara de qué tareas depende; las de la 4b añaden su módulo y **una sola línea** de registro en `ui/app.rs` (sección «Registro de rebanadas»). `ui/mod.rs` y `pub mod ui;` los crea T7 (la primera que los necesita); `term/mod.rs` y `pub mod term;` los crea T4. El controlador integra `lib.rs`, `Cargo.toml`, `Cargo.lock` y la línea base de `app-drift` en secuencia (F31/F34).
6. **Xvfb solo para medir y verificar sin pantalla.** Se autoriza Xvfb únicamente para medir memoria/RSS de WebKitGTK y para la verificación sin pantalla de la app (capturas de paridad). Siempre dentro de `cargo xtask app-shot`, que corre en un namespace propio (`unshare -Urnm`: red, montajes y usuario) con `tmpfs` sobre `/tmp`, así que ni la app Rust ni el oráculo Python pueden ver el socket X del usuario (`/tmp/.X11-unix/X0` queda tapado y el socket abstracto vive en otro namespace de red) ni su servidor tmux. Nunca se abre una ventana en la pantalla real. Nada de navegador local ni Playwright. Las pruebas GTK de `tests/gtk_smoke.rs` corren solo si `COMANDOS_GTK_TEST_DISPLAY` está definido y apunta a un Xvfb del arnés.
7. **La paridad de píxeles tiene dueño**: `cargo xtask png-diff` (T7, Rust, mismo criterio que `tools/png_diff.py`: canal de luminancia de la diferencia > 24, pasa con ≤ 0,1 % de píxeles distintos) y las puertas de píxeles de T12 contra capturas de `bin/cc-app` tomadas con `app-shot`.

## Global Constraints

- Todo se compila y prueba con
  `CARGO_TARGET_DIR=/home/someguy/codebase/0xJesus/ComandOS/.build/target-fase4 nice -n 10 cargo <cmd> -j 6` (abreviado `$C <cmd>`; directorio propio de la fase, F36). Antes de cada commit: `$C fmt --all -- --check`, `$C clippy --workspace --all-targets -j 6 -- -D warnings` y las pruebas de los paquetes tocados.
- Edition 2024, `rust-version = "1.96"`, `unsafe_code = "forbid"` (workspace).
- Commits con `git add <rutas>` explícitas; mensajes `feat(app): …`, `feat(xtask): …`, `feat(install): …`, `docs(verification): …` en español, línea en blanco y `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. El plan: `git add -f docs/superpowers/plans/2026-10-04-fase-4-app-gtk.md`.
- Puertos: nunca 4777–4782 en pruebas. Los tableros de prueba usan la banda de `devhost` 7200–7399 (por omisión 7311). Nunca 3000, 5173, 8000, 8080 ni el rango efímero 32768–60999 para puertos fijos.
- Rutas reales que el modo **live** usa, idénticas a las del Python (`bin/cc-app` línea entre paréntesis): `~/.claude/hooks/app-tabs.json` (1900, con candado `app-tabs.json.lock` compartido con `cc-dash`), `app-tabs-history.json` (1901), `app-tabs-snapshot.json` (2469), `app-sessions-v2.json` (2634), `app-tab-active.json` (4942), `app-tab-models.json` (4943), `app-extension-shelf.json` (5448), `app-layout.json` (7492), `app-pane-position.json` (8013), `app-focus.json` (8745), `app-tab-close.json` (8780), `app-tab-open.json` (8805), `app-command.json` (8982), `snippets.json` (443), `acp-panes.json` (2764). WebKit: datos en `~/.local/share/comandos/` y caché en `~/.cache/comandos/` (el Python fija `GLib.set_prgname("comandos")`, línea 192).
- JSON escrito con la codificación de `json.dump` del Python (orden de inserción, `ensure_ascii`, separadores `", "` y `": "`): `comandos_core::json::response_dumps`. `serde_json` del workspace lleva `preserve_order`.
- Ventana live: `WM_CLASS` `("comandos","comandos")`, título `ComandOS`, `prgname` `comandos`, `GDK_BACKEND=x11` (el `.desktop` ya lo exporta; la app lo fija con `gdk::set_allowed_backends("x11")` antes de `gtk::init`).
- Candado de instancia única live: `$XDG_RUNTIME_DIR/cc-app-<DISPLAY con '/'→'_' y sin ':'>.lock` (fallback `/tmp/comandos-<uid>/` y `/tmp`), el **mismo archivo** que el Python (líneas 43–73): si uno corre, el otro activa la ventana con `wmctrl -a ComandOS` (plazo 5 s) y sale.
- Sombra: título `Sombra de la app (Rust)` y `WM_CLASS` `("sombra-app-rs","sombra-app-rs")`; sandbox: `Sandbox de la app (Rust)` y `sandbox-app-rs`. **Ninguno contiene `comandos`** sin distinguir mayúsculas, porque `cc-dash` y el Python enfocan con `wmctrl -x -a comandos` y `wmctrl -a ComandOS`, que comparan por subcadena.
- Idioma de la interfaz: `CC_LANG` de `~/.claude/hooks/cc-notify.conf` con las reglas de `read_conf` de `cc-dash`; si no, `es` cuando `$LANG` empieza por `es` (`_ui_lang`, 177; diferencia documentada en T1).
- Escrituras de archivos solo a través de `guard::WriteGuard` (T3); `crates/comandos-app/clippy.toml` prohíbe `std::fs::{write, rename, remove_file, create_dir_all, File::create, OpenOptions::open}` fuera de `guard.rs`, y los procesos se lanzan solo desde `tmux.rs` y `proc.rs` (`std::process::Command::new` también prohibido fuera de ellos).

## Review Focus

1. **La sombra no escribe nunca en el tmux del usuario ni lo redimensiona.** Un cliente que se engancha a una sesión real con otro tamaño encoge la sesión para todos sus clientes (incidente «escribe sola», 1-oct). Esperado: los clientes de la sombra llevan `read-only,ignore-size`, el tamaño de la sesión no cambia, y cualquier verbo de tmux fuera de la lista de lectura devuelve `TmuxError::ShadowRefused`. Pruebas `shadow_attach_keeps_session_size` y `shadow_refuses_every_mutating_verb` (Tarea 3).
2. **Ningún camino llega al servidor tmux `default` del usuario desde el sandbox ni desde las pruebas.** Esperado: `TmuxCtl` solo se construye con `from_config`, el socket del sandbox vive bajo su raíz temporal, `kill-server`/`kill-session` no pasan por `mutate` y el único borrado de sesión es el tipado `kill_owned_session`. Pruebas `sandbox_socket_never_user_default`, `mutate_refuses_kill_verbs`, `abbreviated_verbs_are_refused` y `owned_kill_needs_proof` (Tarea 3).
3. **Arranque con la pantalla bloqueada o antes de que GTK dé tamaño.** El terminal mide 1 px y un `attach` a 80×24 encoge la sesión. Esperado: el primer attach usa el tamaño que la sesión ya tiene en tmux y solo cambia al tamaño real cuando la asignación se estabiliza (250 ms sin cambios, tope 1 500 ms). Pruebas `settle_waits_for_quiet_allocation`, `attach_before_allocation_uses_tmux_size` y `pty_attach_keeps_session_size_until_settled` (Tarea 5).
4. **Dos escritores sobre los archivos de estado** (Python y sombra a la vez; live y `cc-dash`). Esperado: en sombra `WriteGuard` rechaza toda escritura en `~/.claude/hooks` y la sombra no consume los IPC; en live `app-tabs.json` se escribe bajo `flock` de `app-tabs.json.lock` con renombrado atómico; un arranque parcial nunca sobrescribe un snapshot completo. Pruebas `shadow_guard_refuses_hooks_writes`, `guard_refuses_dotdot_and_symlink_escape` (T3), `tabs_write_holds_shared_lock` y `shadow_does_not_consume_ipc` (T9), `no_snapshot_before_restore_finishes` (T11).
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
| Píxeles iguales a `bin/cc-app` | otro motor de temas y otro CSS | igual | **el mismo GTK 3.24 y el mismo CSS**: la cabecera, las pestañas y los popovers salen idénticos |

Notas:
- Existe `gtk 0.19.0` (GTK3 sobre glib 0.22), pero `webkit2gtk 2.0.2` exige `gtk ^0.18` y `javascriptcore-rs =1.1`; mezclar glib 0.18 y 0.22 en el mismo proceso no es posible con tipos compartidos.
- La Tarea 7 es la **puerta**: si la app mínima no alcanza el presupuesto de RSS o el puente de WebKit falla, se para y se informa antes del port completo (spec §8).

## Blanco móvil: `bin/cc-app` cambia mientras se porta

Otras sesiones editan `bin/cc-app` en el checkout principal. Reglas:

1. La Tarea 2 creó `cargo xtask app-drift`, que guarda un hash por definición (`def`/`class`, anidadas incluidas, decoradores dentro) de `bin/cc-app` y `bin/cc-notifyd` en `docs/verification/app-drift-baseline.json` (459 definiciones de cc-app y 55 de cc-notifyd en `38c9167`).
2. **Cada tarea de port empieza** con `cargo xtask app-drift /home/someguy/codebase/0xJesus/ComandOS/bin/cc-app` (lectura del checkout principal, sin tocarlo). Las funciones de su alcance (Apéndice A) que salgan como `changed`/`added` se portan en su versión nueva; la tarea lo anota en su commit y entrega al controlador la lista para `--accept <nombre>…` (el controlador integra la línea base en secuencia, F34).
3. Los oráculos Python leen `COMANDOS_CC_APP_ORACLE` si está definido (ruta al `bin/cc-app` del checkout principal) y si no el `bin/cc-app` del worktree; ver `tests/support/oracle.rs` (T3).
4. La Tarea 20 (cutover) exige `app-drift` vacío contra el checkout principal el mismo día del cambio.

## Interfaz real de `comandos-term` (Fase 3)

La Fase 3 entrega `crates/comandos-term` (modelo VT sobre `alacritty_terminal 0.26`, renderizado a tiras, glifos de caja, codificación de entrada, selección y enlaces). Esta fase usa **esa API tal cual** y la re-exporta desde `crates/comandos-app/src/term/engine.rs` (F22); el resto de `comandos-app` importa de `crate::term::engine`, nunca de `comandos_term` directamente. No se usa `comandos-term-web` (depende de `wasm-bindgen`/`web-sys` sin condición); su planificador de pintado y su parpadeo de 600 ms se reescriben para escritorio en T6.

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

Las mediciones Rust y Python se toman con `cargo xtask rss` dentro de `cargo xtask app-shot --measure` (Xvfb aislado, T7) y se guardan en `docs/verification/rss.jsonl` (formato existente), resumidas en `docs/verification/cutover-app.md`.

## Estructura de archivos

```
Cargo.toml                                    (miembro crates/comandos-app)                        T1 ✔
crates/comandos-app/Cargo.toml                (+comandos-term T4, +webkit T7)                      T1 ✔, T4, T7
crates/comandos-app/clippy.toml               (disallowed-methods de E/S y procesos)              T3
crates/comandos-app/src/main.rs               (despacho argv[0])                                   T1 ✔, T7, T19
crates/comandos-app/src/lib.rs                (módulos + deny de clippy R9)                        T1 ✔, T3…
crates/comandos-app/src/config.rs             (RunMode, AppConfig, SocketLabel, parse_args, ui_lang) T1 ✔
crates/comandos-app/src/guard.rs              (WriteGuard canónico)                                T3
crates/comandos-app/src/tmux.rs               (TmuxCtl, TmuxError, READ_VERBS, MUTATE_VERBS)       T3
crates/comandos-app/src/proc.rs               (procesos externos con plazo: wmctrl, xdg-open, systemd-run) T3
crates/comandos-app/src/jobs.rs               (hilos de trabajo → bucle de GLib)                   T3
crates/comandos-app/src/term/mod.rs                                                                 T4
crates/comandos-app/src/term/engine.rs        (re-exporta comandos-term; TermEngine)              T4
crates/comandos-app/src/term/paint.rs         (paleta VTE, plan de pintado por fila)               T4
crates/comandos-app/src/term/settle.rs        (Settle, initial_size)                               T5
crates/comandos-app/src/term/pty.rs           (PtySession)                                         T5
crates/comandos-app/src/term/keys.rs          (gdk → KeyInput)                                     T6
crates/comandos-app/src/term/links.rs         (url_from_wrapped_text, PATH_RE, URL_RE)             T6
crates/comandos-app/src/term/schedule.rs      (planificador de pintado y parpadeo de escritorio)   T6
crates/comandos-app/src/term/view.rs          (TermView: DrawingArea + PTY + entrada + selección)  T6
crates/comandos-app/src/ui/mod.rs                                                                   T7
crates/comandos-app/src/ui/app.rs             (App, run, «Registro de rebanadas»)                  T7
crates/comandos-app/src/ui/window.rs          (ventana, cabecera vacía, Paned, candado)            T7
crates/comandos-app/src/ui/webview.rs         (tablero, datos por modo, puente «centro»)           T7
crates/comandos-app/src/layout_dump.rs        (LayoutDump)                                         T7
crates/comandos-app/src/dash_client.rs        (DashClient, http_post, dash_call)                   T8
crates/comandos-app/src/poll.rs               (PollLoop, PollUpdate)                               T8
crates/comandos-app/src/theme.rs              (THEMES, theme_css, header_css, button_style_css, prefs) T8
crates/comandos-app/src/state_files.rs        (lectura/escritura con flock)                        T9
crates/comandos-app/src/tabs.rs               (TabRegistry puro: orden, favoritos, archivo)       T9
crates/comandos-app/src/ui/tabstrip.rs        (TabStripNotebook)                                   T9
crates/comandos-app/src/ui/tab_label.rs                                                             T9
crates/comandos-app/src/ui/confirm.rs         (diálogo que solo cierra con Sí/No)                  T9
crates/comandos-app/src/ipc.rs                (IpcRequest, monitores, no consume en sombra)        T9
crates/comandos-app/src/workspace_view.rs     (prune, shape, split_paths, dock_target)             T10
crates/comandos-app/src/ui/workspace.rs       (GroupPage, WorkspaceView, arrastre y acople)        T10
crates/comandos-app/src/resume.rs             (sane_flags, resume_command, exact_resume_command)   T11
crates/comandos-app/src/snapshot.rs           (snapshot de pestañas y de layouts)                  T11
crates/comandos-app/src/restore.rs            (RestorePlan y ejecutor)                             T11
crates/comandos-app/src/ui/{bridge,app_commands,presence}.rs                                        T13
crates/comandos-app/src/ui/{header,marks,hourglass}.rs                                              T14
crates/comandos-app/src/ui/{keys,switcher,help}.rs, src/agent_stop.rs                               T15
crates/comandos-app/src/ui/{menu,clipboard,snippets}.rs, src/clipboard_bridge.rs                    T16
crates/comandos-app/src/ui/{overlays,accounts,extensions}.rs, src/overlay_geom.rs                   T17
crates/comandos-app/src/ui/{side,mosaic,reader,modals}.rs                                           T18
crates/comandos-app/src/notify.rs             (POST al 4778 de comandos-notifyd)                   T19
crates/comandos-app/tests/support/{mod,oracle,tmux}.rs                                              T3
crates/comandos-app/tests/*.rs, tests/gtk_smoke.rs (harness = false)                               T1 ✔, T3–T19
xtask/src/app_drift.rs                                                                              T2 ✔
xtask/src/{png_diff,app_shot,isolate}.rs                                                            T7
xtask/src/app_layout.rs                                                                             T12
crates/comandos-cli/src/install.rs, crates/comandos-cli/src/install/release.rs (binario app)       T20
docs/verification/app-drift-baseline.json                                                           T2 ✔
docs/verification/app-shots/<escenario>/{python,rust,diff}.png                                      T7, T12–T18
docs/verification/cutover-app.md                                                                    T12, T20
```

Orden y paralelismo (dependencias entre paréntesis):

- **4a — cimientos y app mínima**: T1 ✔ → {T2 ✔, T3, T4, T8, T19} en paralelo → T5 (T3, T4) → T6 (T4, T5) → T7 (T3, T6, T8) → T9 (T7) → {T10, T11} en paralelo (T9) → T12 (T9, T10, T11).
- **4b — paridad por rebanadas**, todas después de T12 y entre sí en paralelo salvo lo indicado: T13; T14 (T13); T15 (T13); T16 (T13, T19); T17 (T13); T18 (T13, T14). T20 al final.

---

## 4a — Cimientos y app mínima

### Task 1: crate `comandos-app`, versiones fijadas, despacho y modos  ✔ `e63a6af`, `6a1a466`

**Depende de:** nada. **Paralelizable:** no (las demás lo necesitan). **Estado:** implementada en `migration/rust-fase4` (`e63a6af` y la ronda 1 de revisión `6a1a466`). Esta sección describe lo construido y aplica F03, F10, F18, F19, F20, F21 y el ruling «sandbox siempre con socket propio».

**Files:**
- Modify: `Cargo.toml` (miembro `crates/comandos-app`)
- Create: `crates/comandos-app/Cargo.toml`, `crates/comandos-app/src/{main,lib,config}.rs`
- Test: `crates/comandos-app/tests/config.rs`, `crates/comandos-app/tests/gtk_smoke.rs` (`harness = false`)

**Interfaces:**
- Consumes: nada del crate.
- Produces (`comandos_app::config`):
  - `enum RunMode { Sandbox, Shadow, Live }`.
  - `enum Entry { App { default_live: bool }, Notifyd }` y `fn resolve_entry(argv0: &str) -> Entry` (`cc-app` → `App { default_live: true }`; `cc-notifyd`/`comandos-notifyd` → `Notifyd`; el resto → sandbox).
  - `struct SocketLabel` con `SocketLabel::new(raw: &str) -> Result<SocketLabel, String>` (no vacía, ≤ 64, distinta de `default`, sin `.` inicial, `[A-Za-z0-9._-]`) y `as_str()`.
  - `enum TmuxServer<'a> { User, Private(&'a SocketLabel) }`.
  - `struct AppConfig` con campos privados y un `enum Mode` privado `{ Sandbox { socket: SocketLabel, root: PathBuf }, Shadow { socket: Option<SocketLabel> }, Live { socket: Option<SocketLabel> } }`: **un sandbox sin socket propio no es representable**. Accesores: `mode() -> RunMode`, `tmux_server() -> TmuxServer<'_>`, `tmux_socket() -> Option<&str>`, `sandbox_root() -> Option<&Path>`, `home()`, `hooks_dir()`, `dash_url() -> Option<&str>` (`None` en sandbox sin `--dash-url`), `web_data_dir()`, `web_cache_dir()`, `runtime_dir()`, `repo_root() -> Option<&Path>`, `writes_allowed() -> bool` (falso solo en sombra), `lock_file_name(display: &str) -> String`, `wm_class() -> &'static str`, `title() -> &'static str`, `layout_dump_path() -> PathBuf`.
  - `fn parse_args(args: &[String], default_live: bool, env: &dyn Fn(&str) -> Option<String>) -> Result<AppConfig, String>`; opciones `--mode sandbox|shadow|live`, `--tmux-socket NOMBRE`, `--hooks-dir DIR`, `--dash-url http://127.0.0.1:PUERTO`, `--repo DIR`.
  - `fn ui_lang(hooks_dir: &Path, lang_env: Option<&str>) -> &'static str`.
- Comportamiento fijado:
  - `lock_file_name`: `DISPLAY` vacío → `x`; `/` → `_`; se quitan los `:` (F19, igual que `bin/cc-app:43`); live `cc-app-<d>.lock`, sombra `sombra-app-rs-<d>.lock`, sandbox `comandos-app-sbx-<d>.lock`.
  - Sandbox: raíz `$XDG_RUNTIME_DIR/comandos-app-sbx` (o `/tmp/comandos-<uid>/comandos-app-sbx`), socket por omisión `comandos-app-sbx` (la etiqueta no empieza por `.` ni `-`), hooks en `<raíz>/hooks`, datos WebKit en `<raíz>/{data,cache}`. **Lista de permitidos** (`6a1a466`): la raíz, `--hooks-dir` y los datos y la caché web se resuelven (`canonicalize` del ancestro existente más profundo + cola léxica) y tienen que quedar bajo `sandbox_root()` o bajo `TMPDIR` (por omisión `/tmp`); se rechazan `..` en la parte que no existe, enlaces colgantes y enlaces que salen; una raíz que sea `/` o contenga el `HOME` no cuenta (F10). `--dash-url` solo en la banda 7200–7399 (F20).
  - Sombra: `writes_allowed()` es `false`: la sombra no escribe estado (archivos de hooks, tmux, tablero). Live: WebKit en `~/.local/share/comandos` y `~/.cache/comandos`; sombra en `$XDG_RUNTIME_DIR/comandos-app-shadow/{data,cache}` (rutas que T7 **no** usa: la sombra corre WebKit efímero).
  - Sombra y live: `--dash-url` o `COMANDOS_DASH_URL` (vacío cuenta como no definido, como `os.environ.get(...) or DEFAULT` en `bin/cc-app:86`), por omisión `http://127.0.0.1:4777`; solo `http://127.0.0.1:PUERTO`. `hooks_dir` es absoluto en todos los modos.
  - `layout_dump_path()`: live `<runtime>/comandos-app-layout.json`, sombra `<runtime>/comandos-app-shadow-layout.json`, sandbox `sandbox_root()/layout.json`.
  - `repo_root`: `--repo`, `COMANDOS_APP_REPO` o el destino de `<hooks>/dash/index.html` dos niveles arriba.
  - `ui_lang`: lee `CC_LANG` de `<hooks>/cc-notify.conf` con `comandos_runtime::providers::read_conf` (el port de `read_conf` de `cc-dash`: la última asignación gana, comillas emparejadas fuera, `\r` suelto incluido). **Diferencia documentada**: `_ui_lang` del Python (177) pregunta a `GET /conf` y, si el tablero no responde, cae en `$LANG`; leer el archivo da el mismo resultado que `/conf` cuando los dos procesos tienen el mismo `LANG` y funciona sin tablero.
  - Quien use la configuración lo hace por los accesores, nunca por los campos (son privados).

**Cargo.toml del crate (lo construido):** `[lints] workspace = true`; `[[bin]] comandos-app`; `[[test]] gtk_smoke` con `harness = false`; dependencias exactamente las de «Tech Stack» desde T1 (`nix` con `fs, signal, process, user`; sin `webkit2gtk`, `javascriptcore-rs` ni `soup3`, que llegan en T7, F03).

`src/main.rs`: `--version` imprime `comandos-app <versión>`; `Entry::App` y `Entry::Notifyd` salen con 2 y un aviso hasta que T7 y T19 sustituyen cada brazo. `src/lib.rs`: `pub mod config;`. `tests/gtk_smoke.rs`: enlaza GTK (lee `gtk::major_version()…` sin inicializar) y, si falta `COMANDOS_GTK_TEST_DISPLAY`, imprime el aviso y pasa.

- [x] **Step 1: pruebas que fallan** — `tests/config.rs`: `entry_follows_argv0`, `bare_comandos_app_is_sandbox`, `cc_app_defaults_to_live_with_real_paths`, `shadow_never_looks_like_the_real_app` (título y `WM_CLASS` sin `comandos`), `dash_url_must_be_loopback`, `unknown_flag_is_usage_error`, `lock_file_name_matches_python` (diferencial contra `python3 -c` con `DISPLAY=":0/x"`), `sandbox_dash_url_stays_in_devhost_band`, `sandbox_always_carries_a_private_socket` (`--tmux-socket default` es error; sin la opción, `tmux_server()` es `Private("comandos-app-sbx")`), `sandbox_refuses_real_hooks_dir` (también vía `..` y enlace simbólico), `ui_lang_reads_conf_then_lang`; añadidas en `6a1a466`: `sandbox_writes_only_inside_its_roots` (fixtures, nunca el `~/.claude` real: hooks reales con `HOME` en otro sitio, subdirectorio inexistente tras un enlace, `link/../x`, `~/.claude`, `~`, `~/.config/comandos`, enlace colgante, `TMPDIR` que contiene el `HOME`), `only_shadow_forbids_writes`, `dash_env_rules`, `missing_home_and_dash_led_socket_are_errors`.
- [x] **Step 2: correrlas** — `$C test -p comandos-app --test config` → FAIL (no existe el crate).
- [x] **Step 3: implementar** `Cargo.toml` del workspace, `crates/comandos-app/{Cargo.toml, src/main.rs, src/lib.rs, src/config.rs, tests/gtk_smoke.rs}` como se describe arriba.
- [x] **Step 4: pruebas** — `$C test -p comandos-app` → 15 PASS en `config`, `gtk_smoke` pasa con aviso; `$C clippy -p comandos-app --all-targets -- -D warnings` limpio.
- [x] **Step 5: commits** — `e63a6af` `feat(app): crate comandos-app con modos sandbox/sombra/live y despacho por argv[0]`; `6a1a466` `fix(app): el sandbox solo escribe dentro de su raíz o del temporal (lista de permitidos)`.

Pendiente de T1 que se asigna a otras tareas, sin reabrirla:
- **T3**: el tmux del sandbox usa `-S <sandbox_root()>/tmux/<etiqueta>`; `TmuxCtl::from_config` es el único constructor y lee solo accesores. `WriteGuard` vuelve a resolver la ruta **en el momento de escribir** (recorrido `openat` con `O_NOFOLLOW` desde la raíz permitida, creación con `O_EXCL`, `renameat` dentro del mismo descriptor de directorio) para cerrar la ventana TOCTOU entre `parse_args` y la escritura; T3 añade a `AppConfig` el accesor `write_roots()` con las raíces ya resueltas.
- **T7**: crea la raíz del sandbox con modo 0700 y comprueba que es del usuario; el candado del sandbox vive dentro de la raíz; la sombra usa un `WebsiteDataManager` **efímero** (nada persiste: ni `localStorage` ni caché).
- **T8**: el `DashClient` de la sombra solo envía `GET`.
- **T19**: `Entry::Notifyd` deja de ser «pendiente» y pasa a rechazo explícito.

### Task 2: `cargo xtask app-drift` — hash por función de `bin/cc-app`  ✔ `49f2de0`, `01c3a1b`, `218aaa5`

**Depende de:** nada (solo `xtask`). **Paralelizable:** sí, con T3, T4, T8. **Estado:** implementada en `migration/rust-fase4-drift`; esta sección se corrige para describir lo construido (F15, F16, F17).

**Files:**
- Create: `xtask/src/app_drift.rs`
- Modify: `xtask/src/main.rs` (subcomando `app-drift`), `xtask/Cargo.toml` (`sha2.workspace = true`)
- Create: `docs/verification/app-drift-baseline.json`
- Test: pruebas unitarias en `xtask/src/app_drift.rs`

**Interfaces:**
- Produces: `cargo xtask app-drift [--write-baseline] [--accept NOMBRE…] [--baseline RUTA] CC_APP [CC_NOTIFYD]`; `app_drift::{Def { qualname: String, hash: String }, Drift { added, removed, changed: Vec<String> }, defs(src: &str) -> Vec<Def>, checked_defs(src: &str) -> Result<Vec<Def>, ()>, diff(base: &[Def], now: &[Def]) -> Drift, Opts { baseline: PathBuf, write: bool, accept: Vec<String>, paths: Vec<String> }, parse_args(args: &[String]) -> Result<Opts, String>, run(baseline: &Path, paths: &[String], write: bool, accept: &[String]) -> Result<i32, String>}`.
- Códigos de salida: 0 sin deriva (o con `--write-baseline`), 1 con deriva, 2 si un archivo no se lee, la línea base está corrupta (salvo con `--write-baseline`), el escaneo acaba en estado inválido (dentro de una cadena, con paréntesis abiertos o con barra final) o falta el valor de `--baseline`/`--accept` o las rutas. El mensaje de subcomando desconocido lista `rss, parity, poll, app-drift`.

- [x] **Step 1: pruebas que fallan**

Al final de `xtask/src/app_drift.rs`, módulo `tests` con 25 pruebas. Las tres primeras fijan la idea:
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

`xtask/src/app_drift.rs` (815 líneas con las pruebas):
- Un escáner de líneas (`struct Scanner`) sigue el estado léxico de Python: cadenas de una y de tres comillas con prefijos (`r`, `b`, `u`, `f`, `rb`…), escapes, f-strings con comillas anidadas dentro de `{…}`, comentarios, paréntesis/corchetes/llaves abiertos y continuaciones con barra. Solo las líneas cuyo inicio está fuera de cadena y de paréntesis pueden abrir una definición (`def`, `async def`, `class`) o cerrarla por sangría.
- El cuerpo de cada definición empieza en su primer decorador y acaba antes de la primera línea lógica con código a sangría ≤ la de la definición (las líneas dentro de cadenas multilínea no cierran nada). El hash es SHA-256 del texto normalizado (sin líneas vacías ni comentarios de línea completa **fuera** de cadenas; dentro de cadenas cuentan), recortado a 12 hex.
- Nombres calificados por pila de definiciones abiertas; las definiciones bajo un `if` a nivel de módulo no se anidan en la anterior; un nombre calificado repetido recibe sufijo `#n`.
- `checked_defs` devuelve `Err(())` si el escaneo acaba en estado inválido; `run` lo convierte en «estado de escaneo inválido al final de <archivo>» (salida 2) en lugar de hashear.
- `accepted(base, now, accept)` avanza solo las definiciones nombradas; las demás conservan el hash anterior; los archivos que no se pasan en esta llamada conservan su entrada. Las claves se guardan ordenadas (`BTreeMap`). Si un `--accept` no coincide con nada, se avisa por stderr.

En `xtask/src/main.rs`:
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
- Create: `crates/comandos-app/clippy.toml`, `crates/comandos-app/src/{tmux,guard,proc,jobs}.rs`
- Modify: `crates/comandos-app/src/lib.rs` (módulos y `deny` de clippy), `crates/comandos-app/src/config.rs` (accesor `write_roots`)
- Create: `crates/comandos-app/tests/support/{mod,oracle,tmux}.rs`
- Test: `crates/comandos-app/tests/{tmux_guard,write_guard,proc_jobs}.rs`

**Interfaces:**
- Consumes: `config::{AppConfig, RunMode, TmuxServer}` (solo accesores).
- Produces:
  - `config::AppConfig::write_roots(&self) -> Vec<(PathBuf, PathBuf)>`: pares (ruta configurada, ruta canónica) donde la app puede escribir. Sandbox: las raíces ya resueltas por `parse_args` (`sandbox_root()` y `TMPDIR`). Live: `(hooks_dir, canonicalize(hooks_dir))` y `(runtime_dir, canonicalize(runtime_dir))`. Sombra: vacío.
  - `proc::{ProcSpec, ProcOutput, ProcError, run(spec: &ProcSpec) -> Result<ProcOutput, ProcError>, spawn_detached(program: &str, args: &[OsString]) -> Result<(), ProcError>, OUTPUT_CAP}`; `ProcSpec { program: String, args: Vec<OsString>, stdin: Option<Vec<u8>>, env: Vec<(String, OsString)>, env_remove: Vec<String>, cwd: Option<PathBuf>, timeout: Duration }`; `ProcOutput { code: Option<i32>, stdout: Vec<u8>, stderr: Vec<u8>, timed_out: bool, truncated: bool }`.
  - `tmux::{TmuxCtl, TmuxError, TmuxOut, OwnedSession, READ_VERBS, MUTATE_VERBS, TMUX_TIMEOUT, check_read_args(args: &[&str]) -> Result<(), TmuxError>}`:
    - `TmuxCtl::from_config(cfg: &AppConfig, env: &dyn Fn(&str) -> Option<String>) -> Result<TmuxCtl, TmuxError>` (único constructor).
    - `socket_path(&self) -> &Path`, `mode(&self) -> RunMode`.
    - `read(&self, args: &[&str]) -> Result<TmuxOut, TmuxError>`, `mutate(&self, args: &[&str]) -> Result<TmuxOut, TmuxError>`, `mutate_with_stdin(&self, args: &[&str], stdin: &[u8]) -> Result<TmuxOut, TmuxError>`.
    - `idle_scratch(&self, session: &str) -> Result<Option<OwnedSession>, TmuxError>`, `new_placeholder_session(&self, args: &[&str]) -> Result<(TmuxOut, Option<OwnedSession>), TmuxError>`, `kill_owned_session(&self, owned: OwnedSession) -> Result<TmuxOut, TmuxError>`.
    - `attach_argv(&self, session: &str) -> Vec<String>`, `window_size(&self, session: &str) -> Option<(u16, u16)>`, `prepare_socket_dir(&self, guard: &WriteGuard) -> Result<(), GuardError>`.
    - `TmuxOut { code: i32, stdout: String, stderr: String }` con `ok()`; `TmuxError { ShadowRefused(String), Forbidden(String), BadArgs(String), Spawn(String), Timeout(String) }`.
  - `guard::{WriteGuard, GuardError}`: `WriteGuard::from_config(cfg: &AppConfig, display: &str) -> WriteGuard`; `write_atomic(&self, path: &Path, bytes: &[u8], tmp_prefix: &str) -> Result<(), GuardError>`; `append_if_exists(&self, path: &Path, bytes: &[u8]) -> Result<bool, GuardError>`; `remove_file(&self, path: &Path) -> Result<(), GuardError>`; `create_dir_all(&self, path: &Path, mode: u32) -> Result<(), GuardError>`; `open_lock(&self, path: &Path) -> Result<std::fs::File, GuardError>`; `check(&self, path: &Path) -> Result<(), GuardError>`. `GuardError { Shadow(PathBuf), Outside(PathBuf), Escape(PathBuf), Io(PathBuf, String) }`.
  - `jobs::Jobs`: `Jobs::new(workers: usize, name: &str) -> Jobs`; `spawn<T: Send + 'static>(&self, work: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) + 'static)`; `spawn_loop(name: &str, body: impl FnOnce() + Send + 'static) -> std::io::Result<()>`; `to_main<T: Send + 'static>(done: impl FnOnce(T) + 'static) -> async_channel::Sender<T>`.
  - Constantes: `TMUX_TIMEOUT = 5 s` (`tmuxc`, línea 414), `OUTPUT_CAP = 16 MiB`.
  - Test support: `support::tmux::TestTmux::{for_mode(mode: RunMode) -> Option<Fixture>, new_session(&self, name: &str, cols: u16, rows: u16), session_size(&self, name: &str) -> (u16, u16), raw(&self, args: &[&str]) -> std::process::Output, socket(&self) -> &Path}`; `Fixture { tmux: TestTmux, config: AppConfig, ctl: TmuxCtl, guard: WriteGuard, env: Vec<(String, String)> }`; `support::oracle::{cc_app_path() -> PathBuf, python_eval(prelude_defs: &[&str], expr: &str) -> String}`.

Reglas que esta tarea fija para todo el crate:
- **`-S` siempre.** `TmuxCtl` pasa `-S <socket>` en cada llamada (y en el `attach` del PTY). Socket: sandbox `<sandbox_root()>/tmux/<etiqueta>` (directorio `<raíz>/tmux` con modo 0700, creado con `prepare_socket_dir`); sombra/live sin etiqueta `<TMUX_TMPDIR o /tmp>/tmux-<uid>/default` con la regla de tmux 3.2a (si `TMUX_TMPDIR` no es un directorio existente, `/tmp`); sombra/live con etiqueta, el mismo directorio con el nombre de la etiqueta. El sandbox nunca resuelve al socket `default` del usuario (comprobado en `from_config`).
- **Verbos por lista blanca exacta, sin abreviaturas.** tmux acepta prefijos únicos (`kill-ser`) y alias (`ls`): por eso `read` solo admite nombres completos de `READ_VERBS` y `mutate` solo de `MUTATE_VERBS`. `kill-server` y `kill-session` no están en ninguna; el único borrado de sesión es `kill_owned_session(OwnedSession)`, y `OwnedSession` solo la crean `idle_scratch` (sesión `term-*` cuyos panes son todos `zsh`/`bash`/`sh`/`fish`, regla de `close_tab`, 3383–3388) y `new_placeholder_session` (la sesión temporal que crea la restauración, que `restore_session` puede borrar si falla antes de arrancar un agente, `lib/tmux_snapshot.py:206–211`).
- **Lecturas que no mutan** (F09): `display-message` y `capture-pane` exigen `-p`; `save-buffer` solo con destino `-`; ningún argumento puede contener `#(` (formato que ejecuta un comando); los `-t` tienen que ser un objetivo válido (`=sesión`, `=sesión:`, `=sesión:ventana`, `=sesión:^`, `%N`, `@N`, `$N` o `sesión`); `list-panes -s -t =sesión` es válido (F07).
- **Sombra**: `mutate`, `mutate_with_stdin` y `kill_owned_session` devuelven `ShadowRefused` sin lanzar nada; `attach_argv` añade `-f read-only,ignore-size`.
- **Salida grande sin bloqueo** (F08): `proc::run` lee stdout y stderr en hilos a la vez y escribe stdin desde otro, con tope `OUTPUT_CAP` y plazo; una salida de más de 64 KiB nunca bloquea al hijo.
- **Escrituras**: solo por `WriteGuard` (clippy `disallowed-methods`); la ruta se vuelve a resolver al escribir, componente a componente con `openat(O_DIRECTORY | O_NOFOLLOW)` desde la raíz canónica, el temporal se crea con `O_CREAT | O_EXCL | O_NOFOLLOW` y se renombra con `renameat` en el mismo descriptor de directorio. Un `..`, un `.` o un enlace en el recorrido es `Escape`.
- **Procesos**: solo `proc.rs` llama a `std::process::Command::new` (`#[allow(clippy::disallowed_methods)]` en esas dos llamadas, con su motivo). `tmux.rs` lanza tmux a través de `proc::run`.

- [ ] **Step 1: soporte de pruebas**

`crates/comandos-app/tests/support/mod.rs`:
```rust
//! Soporte compartido de las pruebas de integración de `comandos-app`.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::disallowed_methods)]
pub mod oracle;
pub mod tmux;
```

`crates/comandos-app/tests/support/tmux.rs`:
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
    Command::new("tmux").arg("-V").output().is_ok_and(|o| o.status.success())
}

impl TestTmux {
    /// Configuración, `TmuxCtl` y servidor de prueba coherentes para `mode`.
    /// Sandbox: socket `<dir>/run/comandos-app-sbx/tmux/t`. Sombra y live: etiqueta
    /// `t` con `TMUX_TMPDIR=<dir>/tt`, socket `<dir>/tt/tmux-<uid>/t`.
    /// `None` (la prueba se salta) si no hay tmux.
    pub fn for_mode(mode: RunMode) -> Option<Fixture> {
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
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir.join(sub)).unwrap();
        }
        let env: Vec<(String, String)> = vec![
            ("HOME".into(), dir.join("home").display().to_string()),
            ("XDG_RUNTIME_DIR".into(), dir.join("run").display().to_string()),
            ("TMPDIR".into(), dir.join("tmp").display().to_string()),
            ("TMUX_TMPDIR".into(), dir.join("tt").display().to_string()),
        ];
        let lookup = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let mut args = vec!["--mode".to_string(), match mode {
            RunMode::Sandbox => "sandbox",
            RunMode::Shadow => "shadow",
            RunMode::Live => "live",
        }
        .to_string()];
        args.extend(["--tmux-socket".to_string(), "t".to_string()]);
        if mode != RunMode::Sandbox {
            args.extend(["--hooks-dir".to_string(), dir.join("home/.claude/hooks").display().to_string()]);
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
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(parent).unwrap();
        }
        let tmux = TestTmux { dir, socket };
        // Sesión ancla para que el servidor no se apague al cerrar la última de la prueba.
        tmux.new_session("__keep", 80, 24);
        Some(Fixture { tmux, config, ctl, guard, env })
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
            .env_remove("TMUX")
            .output()
            .unwrap()
    }

    pub fn new_session(&self, name: &str, cols: u16, rows: u16) {
        let out = self.raw(&[
            "new-session", "-d", "-s", name, "-x", &cols.to_string(), "-y", &rows.to_string(),
        ]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    pub fn session_size(&self, name: &str) -> (u16, u16) {
        let out = self.raw(&[
            "display-message", "-p", "-t", &format!("={name}:"), "#{window_width} #{window_height}",
        ]);
        let text = String::from_utf8_lossy(&out.stdout);
        let mut it = text.split_whitespace().map(|v| v.parse::<u16>().unwrap_or(0));
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

`crates/comandos-app/tests/support/oracle.rs`:
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
    assert!(out.status.success(), "oráculo: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim_end().to_string()
}
```

- [ ] **Step 2: pruebas que fallan**

`crates/comandos-app/tests/tmux_guard.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::disallowed_methods)]
mod support;
use comandos_app::config::RunMode;
use comandos_app::tmux::{MUTATE_VERBS, READ_VERBS, TmuxError, check_read_args};
use support::tmux::TestTmux;

#[test]
fn sandbox_socket_never_user_default() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else { return };
    let uid = nix::unistd::getuid().as_raw();
    let user_default = std::path::PathBuf::from(format!("/tmp/tmux-{uid}/default"));
    assert_ne!(f.ctl.socket_path(), user_default);
    assert!(f.ctl.socket_path().starts_with(f.config.sandbox_root().unwrap()));
    assert!(f.ctl.socket_path().ends_with("tmux/t"));
}

#[test]
fn every_call_carries_explicit_socket() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else { return };
    f.tmux.new_session("s1", 100, 30);
    // Si TmuxCtl no pasara -S, este has-session iría al servidor por omisión y fallaría.
    assert!(f.ctl.read(&["has-session", "-t", "=s1"]).unwrap().ok());
    let argv = f.ctl.attach_argv("s1");
    assert_eq!(argv[0], "/bin/sh");
    assert!(argv[2].contains(&format!("-S '{}'", f.ctl.socket_path().display())), "{argv:?}");
}

#[test]
fn mutate_refuses_kill_verbs() {
    let Some(f) = TestTmux::for_mode(RunMode::Live) else { return };
    f.tmux.new_session("victim", 80, 24);
    for args in [["kill-server"].as_slice(), &["kill-session", "-t", "=victim"]] {
        assert!(matches!(f.ctl.mutate(args), Err(TmuxError::Forbidden(_))), "{args:?}");
        assert!(matches!(f.ctl.read(args), Err(TmuxError::Forbidden(_))), "{args:?}");
    }
    assert!(f.ctl.read(&["has-session", "-t", "=victim"]).unwrap().ok(), "la sesión sigue viva");
    assert!(!MUTATE_VERBS.contains(&"kill-session") && !READ_VERBS.contains(&"kill-server"));
}

#[test]
fn abbreviated_verbs_are_refused() {
    let Some(f) = TestTmux::for_mode(RunMode::Live) else { return };
    for verb in ["kill-ser", "kill-ses", "killw", "ls", "show", "lsp", "send", "display"] {
        assert!(matches!(f.ctl.mutate(&[verb]), Err(TmuxError::Forbidden(_))), "{verb}");
        assert!(matches!(f.ctl.read(&[verb]), Err(TmuxError::Forbidden(_))), "{verb}");
    }
}

#[test]
fn read_args_must_not_mutate() {
    assert!(check_read_args(&["display-message", "-t", "=s:", "#{pane_id}"]).is_err(), "sin -p");
    assert!(check_read_args(&["display-message", "-p", "-t", "=s:", "#{pane_id}"]).is_ok());
    assert!(check_read_args(&["capture-pane", "-t", "%3"]).is_err(), "sin -p crea un buffer");
    assert!(check_read_args(&["capture-pane", "-p", "-t", "%3"]).is_ok());
    assert!(check_read_args(&["save-buffer", "/tmp/x"]).is_err());
    assert!(check_read_args(&["save-buffer", "-"]).is_ok());
    assert!(check_read_args(&["display-message", "-p", "#(rm -rf ~)"]).is_err());
    assert!(check_read_args(&["list-panes", "-s", "-t", "=sess", "-F", "#{pane_current_command}"]).is_ok());
    assert!(check_read_args(&["list-panes", "-t", "=s; kill-server", "-F", "x"]).is_err());
    for target in ["=s", "=s:", "=s:claude", "=s:^", "=s:2", "%12", "@3", "$4", "term-12-3"] {
        assert!(check_read_args(&["display-message", "-p", "-t", target, "x"]).is_ok(), "{target}");
    }
}

#[test]
fn shadow_refuses_every_mutating_verb() {
    let Some(f) = TestTmux::for_mode(RunMode::Shadow) else { return };
    f.tmux.new_session("s1", 80, 24);
    for verb in MUTATE_VERBS {
        assert!(
            matches!(f.ctl.mutate(&[verb, "-t", "=s1"]), Err(TmuxError::ShadowRefused(_))),
            "{verb}"
        );
    }
    assert!(matches!(
        f.ctl.mutate_with_stdin(&["load-buffer", "-b", "x", "-"], b"hola"),
        Err(TmuxError::ShadowRefused(_))
    ));
    assert!(f.ctl.read(&["list-sessions", "-F", "#{session_name}"]).unwrap().stdout.contains("s1"));
}

#[test]
fn shadow_attach_keeps_session_size() {
    let Some(f) = TestTmux::for_mode(RunMode::Shadow) else { return };
    f.tmux.new_session("big", 163, 44);
    let argv = f.ctl.attach_argv("big");
    assert!(argv[2].contains("attach -f read-only,ignore-size -t '=big'"), "{argv:?}");
    // Cliente real de 80×24 a través de `script` (un PTY): la sesión no encoge.
    let mut child = std::process::Command::new("script")
        .args(["-qfec", &format!("stty cols 80 rows 24; {}", argv[2]), "/dev/null"])
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
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else { return };
    f.tmux.new_session("term-9-1", 80, 24);
    f.tmux.raw(&["send-keys", "-t", "=term-9-1:", "sleep 300", "Enter"]);
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(f.ctl.idle_scratch("term-9-1").unwrap().is_none(), "corre algo: no se mata");
    f.tmux.new_session("term-9-2", 80, 24);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let owned = f.ctl.idle_scratch("term-9-2").unwrap().expect("shell ocioso");
    assert!(f.ctl.kill_owned_session(owned).unwrap().ok());
    assert!(!f.ctl.read(&["has-session", "-t", "=term-9-2"]).unwrap().ok());
    assert!(f.ctl.idle_scratch("proyecto").unwrap().is_none(), "solo term-*");
}

#[test]
fn placeholder_can_be_removed_by_its_creator() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else { return };
    let (out, owned) = f
        .ctl
        .new_placeholder_session(&["new-session", "-d", "-P", "-F", "#{pane_id}", "-s", "ph", "sleep", "2147483647"])
        .unwrap();
    assert!(out.ok());
    assert!(f.ctl.kill_owned_session(owned.expect("token")).unwrap().ok());
}

#[test]
fn large_output_never_blocks() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else { return };
    let big = "x".repeat(1 << 20); // 1 MiB por stdin y de vuelta por stdout
    assert!(f.ctl.mutate_with_stdin(&["load-buffer", "-b", "big", "-"], big.as_bytes()).unwrap().ok());
    let started = std::time::Instant::now();
    let out = f.ctl.read(&["show-buffer", "-b", "big"]).unwrap();
    assert_eq!(out.stdout.len(), 1 << 20);
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
}

#[test]
fn window_size_ignores_tiny_sessions() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else { return };
    f.tmux.new_session("ok", 120, 40);
    f.tmux.new_session("tiny", 10, 3);
    assert_eq!(f.ctl.window_size("ok"), Some((120, 40)));
    assert_eq!(f.ctl.window_size("tiny"), None, "cols >= 20 y rows >= 5 (_tmux_window_size)");
    assert_eq!(f.ctl.window_size("nope"), None);
}
```

`crates/comandos-app/tests/write_guard.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::disallowed_methods)]
mod support;
use comandos_app::config::RunMode;
use comandos_app::guard::GuardError;
use support::tmux::TestTmux;

#[test]
fn shadow_guard_refuses_hooks_writes() {
    let Some(f) = TestTmux::for_mode(RunMode::Shadow) else { return };
    let target = f.config.hooks_dir().join("app-tabs.json");
    assert!(matches!(f.guard.write_atomic(&target, b"[]", "app-tabs."), Err(GuardError::Shadow(_))));
    assert!(matches!(f.guard.remove_file(&target), Err(GuardError::Shadow(_))));
    assert!(!target.exists());
    // Lo único que la sombra escribe: su candado y su volcado de diseño.
    assert!(f.guard.write_atomic(&f.config.layout_dump_path(), b"{}", "layout.").is_ok());
}

#[test]
fn guard_writes_atomically_inside_roots() {
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else { return };
    let path = f.config.hooks_dir().join("sub/app-tabs.json");
    f.guard.create_dir_all(path.parent().unwrap(), 0o700).unwrap();
    f.guard.write_atomic(&path, "[\"ñandú\"]".as_bytes(), "app-tabs.").unwrap();
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
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else { return };
    let hooks = f.config.hooks_dir().to_path_buf();
    std::fs::create_dir_all(&hooks).unwrap();
    let outside = f.config.home().join("fuera");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, hooks.join("enlace")).unwrap();
    assert!(matches!(
        f.guard.write_atomic(&hooks.join("../../../home/x.json"), b"1", "x."),
        Err(GuardError::Escape(_) | GuardError::Outside(_))
    ));
    assert!(matches!(
        f.guard.write_atomic(&hooks.join("enlace/x.json"), b"1", "x."),
        Err(GuardError::Escape(_))
    ));
    assert!(!outside.join("x.json").exists());
    assert!(matches!(
        f.guard.write_atomic(&f.config.home().join(".claude/hooks/app-tabs.json"), b"1", "x."),
        Err(GuardError::Outside(_))
    ));
}

#[test]
fn guard_rechecks_at_write_time() {
    // TOCTOU: la ruta era válida al construir la guarda; después alguien cambia
    // un directorio intermedio por un enlace que sale. La escritura lo detecta.
    let Some(f) = TestTmux::for_mode(RunMode::Sandbox) else { return };
    let dir = f.config.hooks_dir().join("cambia");
    f.guard.create_dir_all(&dir, 0o700).unwrap();
    f.guard.write_atomic(&dir.join("a.json"), b"1", "a.").unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let outside = f.config.home().join("robo");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, &dir).unwrap();
    assert!(matches!(f.guard.write_atomic(&dir.join("a.json"), b"2", "a."), Err(GuardError::Escape(_))));
    assert!(!outside.join("a.json").exists());
}
```

`crates/comandos-app/tests/proc_jobs.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::disallowed_methods)]
use comandos_app::jobs::Jobs;
use comandos_app::proc::{ProcSpec, run};
use std::time::{Duration, Instant};

fn spec(program: &str, args: &[&str], timeout_ms: u64) -> ProcSpec {
    ProcSpec {
        program: program.into(),
        args: args.iter().map(Into::into).collect(),
        stdin: None,
        env: Vec::new(),
        env_remove: Vec::new(),
        cwd: None,
        timeout: Duration::from_millis(timeout_ms),
    }
}

#[test]
fn stdout_and_stderr_over_64k_do_not_deadlock() {
    // 1 MiB a stderr ANTES de escribir stdout: con lecturas en serie se bloquea.
    let s = spec("sh", &["-c", "head -c 1048576 /dev/zero >&2; head -c 1048576 /dev/zero"], 5000);
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
    let ctx = glib::MainContext::default();
    let _guard = ctx.acquire().unwrap();
    let main_loop = glib::MainLoop::new(Some(&ctx), false);
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
                assert_eq!(std::thread::current().id(), main_thread, "done en el hilo de GLib");
                done.borrow_mut().push(v);
                if done.borrow().len() == 4 {
                    ml.quit();
                }
            },
        );
    }
    main_loop.run();
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(390) && elapsed < Duration::from_millis(700), "{elapsed:?}");
    let mut got = done.borrow().clone();
    got.sort_unstable();
    assert_eq!(got, [0, 1, 2, 3]);
}
```

- [ ] **Step 3: correrlas**

Run: `$C test -p comandos-app --test tmux_guard --test write_guard --test proc_jobs`
Expected: FAIL (no existen `tmux`, `guard`, `proc`, `jobs`).

- [ ] **Step 4: `clippy.toml` y `lib.rs`**

`crates/comandos-app/clippy.toml`:
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

`crates/comandos-app/src/lib.rs`:
```rust
//! Escritorio GTK de ComandOS: tablero WebKit, pestañas de terminal sobre tmux y popups.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
pub mod config;
pub mod guard;
pub mod jobs;
pub mod proc;
pub mod tmux;
```

- [ ] **Step 5: `write_roots` en `config.rs`**

En `config.rs`, `Mode::Sandbox` guarda las raíces ya resueltas (`roots: Vec<PathBuf>`) que hoy calcula `sandbox_write_roots` dentro de `parse_args`, y se añade:
```rust
    /// Pares (ruta como la ven los llamadores, ruta canónica) donde la app puede
    /// escribir. La sombra no tiene ninguno; `guard::WriteGuard` añade aparte su
    /// candado y su volcado de diseño.
    pub fn write_roots(&self) -> Vec<(PathBuf, PathBuf)> {
        match &self.mode {
            Mode::Sandbox { roots, .. } => roots.iter().map(|r| (r.clone(), r.clone())).collect(),
            Mode::Shadow { .. } => Vec::new(),
            Mode::Live { .. } => [&self.hooks_dir, &self.runtime_dir]
                .into_iter()
                .filter_map(|p| std::fs::canonicalize(p).ok().map(|c| (p.clone(), c)))
                .collect(),
        }
    }
```
En el brazo `RunMode::Sandbox` de `parse_args`: `(Mode::Sandbox { socket, root, roots }, …)`. Prueba añadida a `tests/config.rs`:
```rust
#[test]
fn write_roots_follow_the_mode() {
    let base = scratch("roots");
    let env = env_owned(vec![
        ("HOME", p(&base.join("home"))),
        ("XDG_RUNTIME_DIR", p(&base.join("run"))),
        ("TMPDIR", p(&base.join("tmp"))),
    ]);
    std::fs::create_dir_all(base.join("home/.claude/hooks")).unwrap();
    std::fs::create_dir_all(base.join("run")).unwrap();
    std::fs::create_dir_all(base.join("tmp")).unwrap();
    let sbx = parse_args(&args(&["--mode", "sandbox"]), false, &env).unwrap();
    let roots: Vec<_> = sbx.write_roots().into_iter().map(|(_, c)| c).collect();
    assert!(roots.iter().any(|r| r.ends_with("comandos-app-sbx")));
    assert!(roots.iter().all(|r| !r.starts_with(base.join("home"))));
    let shadow = parse_args(&args(&["--mode", "shadow"]), false, &env).unwrap();
    assert!(shadow.write_roots().is_empty());
    let live = parse_args(&args(&["--mode", "live"]), false, &env).unwrap();
    assert_eq!(live.write_roots().len(), 2);
}
```

- [ ] **Step 6: implementar `proc.rs`**

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

fn drain(mut src: impl Read) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    let mut truncated = false;
    loop {
        match src.read(&mut buf) {
            Ok(0) | Err(_) => break,
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
    let mut cmd = command(&spec.program);
    cmd.args(&spec.args)
        .stdin(if spec.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in &spec.env_remove {
        cmd.env_remove(key);
    }
    for (key, value) in &spec.env {
        cmd.env(key, value);
    }
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    let mut child = cmd.spawn().map_err(|e| ProcError::Spawn(format!("{}: {e}", spec.program)))?;
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    std::thread::scope(|scope| {
        if let (Some(mut pipe), Some(data)) = (stdin, spec.stdin.as_deref()) {
            scope.spawn(move || {
                let _ = pipe.write_all(data);
            });
        }
        let out = stdout.map(|p| scope.spawn(move || drain(p)));
        let err = stderr.map(|p| scope.spawn(move || drain(p)));
        let deadline = Instant::now() + spec.timeout;
        let mut result = ProcOutput::default();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    result.code = status.code();
                    break;
                }
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    result.timed_out = true;
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                Err(_) => break,
            }
        }
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
/// quede zombi. Equivale a `subprocess.Popen(..., start_new_session=True)`.
pub fn spawn_detached(program: &str, args: &[OsString]) -> Result<(), ProcError> {
    use std::os::unix::process::CommandExt;
    let mut cmd = command(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = cmd.spawn().map_err(|e| ProcError::Spawn(format!("{program}: {e}")))?;
    std::thread::Builder::new()
        .name("comandos-reap".into())
        .spawn(move || {
            let _ = child.wait();
        })
        .map(|_| ())
        .map_err(|e| ProcError::Spawn(e.to_string()))
}
```

- [ ] **Step 7: implementar `tmux.rs`**

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
    "has-session", "list-sessions", "list-windows", "list-panes", "list-clients",
    "list-buffers", "display-message", "show-options", "show-buffer", "save-buffer",
    "capture-pane", "show-environment",
];

/// Verbos que la app usa para cambiar tmux (solo fuera de la sombra). Ni
/// `kill-server` ni `kill-session`: el único borrado es `kill_owned_session`.
pub const MUTATE_VERBS: &[&str] = &[
    "new-session", "new-window", "split-window", "send-keys", "select-pane",
    "select-window", "select-layout", "resize-pane", "resize-window", "set-option",
    "load-buffer", "paste-buffer", "delete-buffer", "respawn-pane", "move-window",
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
}

#[derive(Debug, Clone)]
pub struct TmuxCtl {
    mode: RunMode,
    socket: PathBuf,
}

fn valid_session_name(name: &str) -> bool {
    // SESSION_RE de bin/cc-app:3572 (el mismo que cc-dash).
    !name.is_empty()
        && name.len() <= 80
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
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
                || r.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        })
}

/// Reglas de lectura (F07, F09): sin `#(`, `-p` obligatorio donde sin él se
/// muta, `save-buffer` solo a stdout, objetivos `-t` válidos.
pub fn check_read_args(args: &[&str]) -> Result<(), TmuxError> {
    let verb = *args.first().ok_or_else(|| TmuxError::BadArgs("sin verbo".into()))?;
    if !READ_VERBS.contains(&verb) {
        return Err(TmuxError::Forbidden(verb.to_string()));
    }
    if let Some(bad) = args.iter().find(|a| a.contains("#(")) {
        return Err(TmuxError::BadArgs(format!("formato que ejecuta comandos: {bad}")));
    }
    let has = |flag: &str| args.iter().skip(1).any(|a| *a == flag);
    match verb {
        "display-message" | "capture-pane" if !has("-p") => {
            return Err(TmuxError::BadArgs(format!("{verb} sin -p")));
        }
        "save-buffer" if args.last() != Some(&"-") => {
            return Err(TmuxError::BadArgs("save-buffer solo a stdout (-)".into()));
        }
        _ => {}
    }
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if *a == "-t" {
            let target = it.next().ok_or_else(|| TmuxError::BadArgs("-t sin valor".into()))?;
            if !valid_target(target) {
                return Err(TmuxError::BadArgs(format!("objetivo no válido: {target}")));
            }
        }
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
    pub fn from_config(cfg: &AppConfig, env: &dyn Fn(&str) -> Option<String>) -> Result<TmuxCtl, TmuxError> {
        let uid = nix::unistd::getuid().as_raw();
        let user_dir = tmux_tmpdir(env).join(format!("tmux-{uid}"));
        let socket = match (cfg.mode(), cfg.tmux_server(), cfg.sandbox_root()) {
            (RunMode::Sandbox, TmuxServer::Private(label), Some(root)) => root.join("tmux").join(label.as_str()),
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
        Ok(TmuxCtl { mode: cfg.mode(), socket })
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
            (RunMode::Sandbox, Some(dir)) => guard.create_dir_all(dir, 0o700),
            _ => Ok(()),
        }
    }

    fn exec(&self, args: &[&str], stdin: Option<&[u8]>) -> Result<TmuxOut, TmuxError> {
        let mut argv: Vec<OsString> = vec!["-S".into(), self.socket.clone().into_os_string()];
        argv.extend(args.iter().map(OsString::from));
        let spec = ProcSpec {
            program: "tmux".into(),
            args: argv,
            stdin: stdin.map(<[u8]>::to_vec),
            env: Vec::new(),
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
        let verb = *args.first().ok_or_else(|| TmuxError::BadArgs("sin verbo".into()))?;
        if !MUTATE_VERBS.contains(&verb) {
            return Err(TmuxError::Forbidden(verb.to_string()));
        }
        if self.mode == RunMode::Shadow {
            return Err(TmuxError::ShadowRefused(verb.to_string()));
        }
        if let Some(bad) = args.iter().find(|a| a.contains("#(")) {
            return Err(TmuxError::BadArgs(format!("formato que ejecuta comandos: {bad}")));
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
        let out = self.read(&["list-panes", "-s", "-t", &target, "-F", "#{pane_current_command}"])?;
        let cmds: Vec<&str> = out.stdout.split_whitespace().collect();
        Ok((out.ok() && !cmds.is_empty() && cmds.iter().all(|c| IDLE_SHELLS.contains(c)))
            .then(|| OwnedSession { name: session.to_string() }))
    }

    /// `new-session` de la restauración: devuelve la prueba de propiedad si tmux
    /// la creó (el nombre sale del `-s` de `args`).
    pub fn new_placeholder_session(&self, args: &[&str]) -> Result<(TmuxOut, Option<OwnedSession>), TmuxError> {
        if args.first() != Some(&"new-session") {
            return Err(TmuxError::BadArgs("new_placeholder_session solo crea sesiones".into()));
        }
        let name = args
            .iter()
            .position(|a| *a == "-s")
            .and_then(|i| args.get(i + 1))
            .filter(|n| valid_session_name(n))
            .ok_or_else(|| TmuxError::BadArgs("new-session sin -s válido".into()))?
            .to_string();
        let out = self.mutate(args)?;
        let owned = out.ok().then(|| OwnedSession { name });
        Ok((out, owned))
    }

    pub fn kill_owned_session(&self, owned: OwnedSession) -> Result<TmuxOut, TmuxError> {
        if self.mode == RunMode::Shadow {
            return Err(TmuxError::ShadowRefused("kill-session".into()));
        }
        let target = format!("={}", owned.name);
        self.exec(&["kill-session", "-t", &target], None)
    }

    /// Mismo comando que `open_tab` (3611) con `-S`; la sombra se engancha en
    /// solo lectura y sin cambiar el tamaño de la sesión.
    pub fn attach_argv(&self, session: &str) -> Vec<String> {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        let flags = if self.mode == RunMode::Shadow { "-f read-only,ignore-size " } else { "" };
        let attach = format!(
            "tmux -S {} attach {flags}-t {} || {{ echo '[sesion terminada — cierra esta pestana con la x]'; exec cat; }}",
            quote(&self.socket.display().to_string()),
            quote(&format!("={session}")),
        );
        vec!["/bin/sh".into(), "-c".into(), attach]
    }

    /// `_tmux_window_size` (749): tamaño que la sesión ya tiene, si es razonable.
    pub fn window_size(&self, session: &str) -> Option<(u16, u16)> {
        let target = format!("={session}:");
        let out = self
            .read(&["display-message", "-p", "-t", &target, "#{window_width} #{window_height}"])
            .ok()?;
        let mut it = out.stdout.split_whitespace().map(str::parse::<u16>);
        let (cols, rows) = (it.next()?.ok()?, it.next()?.ok()?);
        (cols >= 20 && rows >= 5).then_some((cols, rows))
    }
}
```

- [ ] **Step 8: implementar `guard.rs`**

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

const DIR_FLAGS: OFlag = OFlag::O_DIRECTORY.union(OFlag::O_NOFOLLOW).union(OFlag::O_CLOEXEC);

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
    let rel = dir.strip_prefix("/").map_err(|_| GuardError::Escape(dir.to_path_buf()))?;
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
            }
            RunMode::Sandbox => {}
        }
        WriteGuard { mode: cfg.mode(), roots: cfg.write_roots(), files }
    }

    /// (descriptor del directorio padre, nombre final) tras recorrer sin enlaces.
    fn resolve_parent(&self, path: &Path, create_dirs: bool) -> Result<(OwnedFd, OsString), GuardError> {
        if path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
            return Err(GuardError::Escape(path.to_path_buf()));
        }
        let name = path.file_name().ok_or_else(|| GuardError::Escape(path.to_path_buf()))?.to_os_string();
        let parent = path.parent().ok_or_else(|| GuardError::Escape(path.to_path_buf()))?;
        if self.files.iter().any(|f| f == path) {
            return Ok((open_dir_nofollow(&std::fs::canonicalize(parent).map_err(|e| io(path, e))?)?, name));
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
                    mkdirat(&fd, part.as_os_str(), Mode::from_bits_truncate(0o700)).map_err(|e| io(path, e))?;
                    openat(&fd, part.as_os_str(), DIR_FLAGS, Mode::empty()).map_err(|_| GuardError::Escape(path.to_path_buf()))?
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
    pub fn write_atomic(&self, path: &Path, bytes: &[u8], tmp_prefix: &str) -> Result<(), GuardError> {
        let (dir, name) = self.resolve_parent(path, false)?;
        let mut rnd = [0u8; 8];
        getrandom::fill(&mut rnd).map_err(|e| io(path, e))?;
        let tmp: OsString = format!("{tmp_prefix}{}.tmp", rnd.iter().map(|b| format!("{b:02x}")).collect::<String>()).into();
        let flags = OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_WRONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        let fd = openat(&dir, tmp.as_os_str(), flags, Mode::from_bits_truncate(0o600)).map_err(|e| io(path, e))?;
        let mut file = std::fs::File::from(fd);
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        let renamed = written
            .map_err(|e| io(path, e))
            .and_then(|()| renameat(&dir, tmp.as_os_str(), &dir, name.as_os_str()).map_err(|e| io(path, e)));
        if renamed.is_err() {
            let _ = unlinkat(&dir, tmp.as_os_str(), nix::unistd::UnlinkatFlags::NoRemoveDir);
        }
        renamed
    }

    /// Anexa solo si el archivo ya existe (`snip_log`, 495). `Ok(false)` si no existe.
    pub fn append_if_exists(&self, path: &Path, bytes: &[u8]) -> Result<bool, GuardError> {
        let (dir, name) = self.resolve_parent(path, false)?;
        let flags = OFlag::O_WRONLY | OFlag::O_APPEND | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        match openat(&dir, name.as_os_str(), flags, Mode::empty()) {
            Ok(fd) => std::fs::File::from(fd).write_all(bytes).map(|()| true).map_err(|e| io(path, e)),
            Err(nix::errno::Errno::ENOENT) => Ok(false),
            Err(e) => Err(io(path, e)),
        }
    }

    /// Consumir un archivo IPC (`on_app_command`, 8968). Nunca en sombra.
    pub fn remove_file(&self, path: &Path) -> Result<(), GuardError> {
        let (dir, name) = self.resolve_parent(path, false)?;
        unlinkat(&dir, name.as_os_str(), nix::unistd::UnlinkatFlags::NoRemoveDir).map_err(|e| io(path, e))
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
        openat(&dir, name.as_os_str(), flags, Mode::from_bits_truncate(0o600))
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
`getrandom` entra como dependencia del crate (`getrandom.workspace = true` en `crates/comandos-app/Cargo.toml`).

- [ ] **Step 9: implementar `jobs.rs`**

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
        for i in 0..workers.max(1) {
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
    glib::MainContext::default().spawn_local(async move {
        if let Ok(value) = rx.recv().await {
            done(value);
        }
    });
    tx
}

/// Hilo propio con nombre para un bucle largo (equivale a `threading.Thread(daemon=True)`).
pub fn spawn_loop(name: &str, body: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    std::thread::Builder::new().name(format!("comandos-{name}")).spawn(body).map(|_| ())
}
```

- [ ] **Step 10: pruebas**

Run: `$C test -p comandos-app --test tmux_guard --test write_guard --test proc_jobs --test config`
Expected: PASS (las de tmux se saltan con aviso si `tmux` no está; en esta máquina está). Después `$C clippy -p comandos-app --all-targets -j 6 -- -D warnings`: limpio, y `grep -rn "Command::new" crates/comandos-app/src` solo devuelve `proc.rs`.

- [ ] **Step 11: commit**

```bash
git add crates/comandos-app/clippy.toml crates/comandos-app/Cargo.toml crates/comandos-app/src/lib.rs \
  crates/comandos-app/src/config.rs crates/comandos-app/src/tmux.rs crates/comandos-app/src/guard.rs \
  crates/comandos-app/src/proc.rs crates/comandos-app/src/jobs.rs crates/comandos-app/tests/support \
  crates/comandos-app/tests/tmux_guard.rs crates/comandos-app/tests/write_guard.rs \
  crates/comandos-app/tests/proc_jobs.rs crates/comandos-app/tests/config.rs
git commit -m "feat(app): tmux con -S y lista blanca, escrituras que se resuelven al escribir, procesos con plazo y trabajos a GLib

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 4: motor del terminal sobre `comandos-term`, colores de VTE y plan de pintado por fila

**Depende de:** T1 y Fase 3 fusionada. **Paralelizable:** sí, con T2, T3, T8, T19.

Reescrita sobre la API real de `comandos-term` (F04, F06, F22, F23, F26). No pinta nada todavía: produce un plan de operaciones por fila, puro y probado sin pantalla, que T6 ejecuta con cairo + pango.

**Files:**
- Modify: `crates/comandos-app/Cargo.toml` (`comandos-term = { path = "../comandos-term" }`, `alacritty_terminal = { version = "=0.26.0", default-features = false }`), `crates/comandos-app/src/lib.rs` (`pub mod term;`)
- Create: `crates/comandos-app/src/term/{mod,engine,paint}.rs`
- Test: `crates/comandos-app/tests/term_paint.rs`

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

`crates/comandos-app/tests/term_paint.rs`:
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

`crates/comandos-app/src/term/mod.rs`:
```rust
//! Terminal de escritorio sobre `comandos-term`: motor, pintado, PTY y widget.
pub mod engine;
pub mod paint;
```

`crates/comandos-app/src/term/engine.rs`:
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
use alacritty_terminal::index::{Column, Line};
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
        matches!(grid[point][Column(usize::from(col))].fg, Color::Spec(_))
    }
}
```
(Si `alacritty_terminal 0.26` nombra distinto `grid.screen_lines()`/`columns()` o el campo `fg`, se ajusta solo esta función; `comandos-term` usa los mismos tipos en `render.rs`.)

- [ ] **Step 4: `term/paint.rs`**

```rust
//! Plan de pintado por fila, sin cairo: rectángulos de fondo, tiras de texto,
//! glifos de caja (U+2500–U+259F y Powerline, de `comandos-term::glyphs`) y
//! líneas de subrayado/tachado, en píxeles del widget. T6 lo ejecuta.
use crate::term::engine::{CellMetrics, DrawOp, Palette, RowRender, RunKind, Underline, draw_ops};

pub fn hex_rgb(h: &str) -> Option<[u8; 3]> {
    let digits = h.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
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

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 5: PTY de escritorio y enganche cuando el tamaño se estabiliza

**Depende de:** T3, T4. **Paralelizable:** no.

Aplica F27 (firma de `initial_size`), F28 (`stty size` y attach con PTY sin tamaño), F29 (`Drop`), F30 (prueba con tmux real antes/después).

**Files:**
- Create: `crates/comandos-app/src/term/{settle,pty}.rs`
- Modify: `crates/comandos-app/src/term/mod.rs`
- Test: `crates/comandos-app/tests/term_pty.rs`

**Interfaces:**
- Consumes: `tmux::TmuxCtl::{attach_argv, window_size}`; `support::tmux::TestTmux`.
- Produces:
  - `term::settle::Settle::{new(quiet_ms: u64, cap_ms: u64, start_ms: u64) -> Settle, on_alloc(&mut self, now_ms: u64), due(&self, now_ms: u64) -> bool, fired(&mut self) -> bool, next_check_ms(&self, now_ms: u64) -> Option<u64>}` (`fired` devuelve `true` solo la primera vez).
  - `term::settle::initial_size((w, h): (i32, i32), cell_w: f64, cell_h: f64, (pad_x, pad_y): (f64, f64), tmux: Option<(u16, u16)>) -> (u16, u16)`: `w <= 1` = «sin tamaño de GTK» → `tmux` o 80×24; si no, `cols = ⌊(w − pad_x) / cell_w⌋` (mín. 2) y `rows = ⌊(h − pad_y) / cell_h⌋` (mín. 1).
  - `term::pty::PtySession::{spawn(argv: &[String], cols: u16, rows: u16, cwd: &Path) -> Result<PtySession, PtyError>, read_chunk(&mut self, buf: &mut [u8]) -> ReadOutcome, write(&mut self, bytes: &[u8]) -> Result<(), PtyError>, flush_pending(&mut self) -> Result<bool, PtyError>, has_pending(&self) -> bool, resize(&self, cols: u16, rows: u16) -> Result<(), PtyError>, raw_fd(&self) -> i32, pid(&self) -> u32, child_tty(&self) -> Option<String>, try_reap(&mut self) -> Option<i32>}`; `impl Drop for PtySession` (cuelga el grupo con `SIGHUP`, como cerrar la ventana, y recoge al hijo en un hilo); `ReadOutcome { Data(usize), WouldBlock, Closed }`; `PtyError { Open(String), Spawn(String), Io(String) }`.
  - Constantes: `SETTLE_QUIET_MS = 250`, `SETTLE_CAP_MS = 1500` (`_spawn_when_settled`, 760), `RESPAWN_MS = 800` (`child-exited`, 884).
- Entorno del hijo: el del proceso, sin `TMUX`/`TMUX_PANE`/`NO_COLOR`, con `TERM=xterm-256color`, `COLORTERM=truecolor` y `VTE_VERSION=6800` (lo que VTE 0.68 exporta en `spawn_sync`); `cwd` = `$HOME` (`make_term`, 869).

- [ ] **Step 1: pruebas que fallan**

`crates/comandos-app/tests/term_pty.rs`:
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
    let mut p = PtySession::spawn(&["/bin/sh".into(), "-c".into(), "stty size; cat".into()], 100, 30, &home).unwrap();
    assert!(read_until(&mut p, "30 100", Duration::from_secs(3)).contains("30 100"), "tamaño ANTES del primer byte");
    p.write("ñandú 漢字 🚀\n".as_bytes()).unwrap();
    assert!(read_until(&mut p, "🚀", Duration::from_secs(3)).contains("ñandú 漢字 🚀"));
    p.resize(120, 40).unwrap();
    p.write(b"\x04").unwrap();
    let mut q = PtySession::spawn(&["/bin/sh".into(), "-c".into(), "sleep 0.2; stty size".into()], 120, 40, &home).unwrap();
    assert!(read_until(&mut q, "40 120", Duration::from_secs(3)).contains("40 120"));
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

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

