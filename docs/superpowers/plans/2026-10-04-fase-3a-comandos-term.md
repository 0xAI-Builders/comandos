# Fase 3a — `comandos-term`: motor, puente PTY y renderizador canvas — plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** sustituir ttyd (4779/4780) y xterm.js 5.5.0 + addons por una terminal en Rust: un puente PTY↔WebSocket dentro de `comandos dash` y una página de terminal WASM que pinta en canvas, con el mismo aspecto, la misma entrada (teclado físico, GBoard, IME, ratón, gestos) y menos memoria por terminal; sin tocar jamás el servidor tmux del usuario salvo para `attach`.

**Architecture:** `comandos-term` (crate puro, host y wasm): motor VT (`alacritty_terminal` 0.26 vendorizado con parche wasm), protocolo con dos dialectos (`tty` de ttyd y `comandos.term.v1`), codificación de entrada y render puro a «tiras» de celdas. `comandos-server::dash::term`: puente PTY (`pty-process`) con lista cerrada de comandos tmux y contrapresión, rutas `/term/*` y oyente de compatibilidad para 4780/4779. `comandos-term-web` (cdylib): canvas 2D por daños, teclado/IME/ratón y la carcasa de la página (port de `dash/term.html`). Dos cutovers: paso 1 (A4, el puente sustituye a ttyd con xterm.js intacto) y paso 2 (A11, la página nueva sustituye a xterm.js).

**Tech Stack:** ver el índice. Específico: `alacritty_terminal = "=0.26.0"` (parcheado), `vte = "=0.15.0"` (lo fija alacritty), `pty-process = { version = "=0.5.3", features = ["async"] }`, `tokio-tungstenite = "=0.30.0"`, `web-sys = "=0.3.106"` (rasgos `CanvasRenderingContext2d`, `HtmlCanvasElement`, `HtmlTextAreaElement`, `KeyboardEvent`, `InputEvent`, `CompositionEvent`, `MouseEvent`, `WheelEvent`, `PointerEvent`, `TouchEvent`, `WebSocket`, `BinaryType`, `MessageEvent`, `FontFaceSet`, `FontFace`, `ResizeObserver`, `Clipboard`, `Navigator`, `Performance`).

**Spec e índice:** `docs/superpowers/plans/2026-10-04-fase-3-term-y-web.md` (Rulings, Global Constraints —en especial la **regla tmux**—, Decisiones D4–D7, Review Focus 4 y 5, tareas comunes T1–T4). Spike: `docs/research/2026-10-04-spike-terminal-rust.md` (su código en `.spike/term/` se perdió; los números y el diseño valen). Inventario §5 (protocolo de ttyd, URLs, `cc-webterm-attach`).

## Hechos del sistema actual que fija este plan (leídos el 2026-10-04)

- `bin/cc-webterm` arranca dos ttyd 1.6.3 como unidades transitorias: `cc-webterm` (`-p 4779`, UI embebida) y `cc-webterm-path` (`-p 4780 -b /term -I dash/term.html`); comunes: `-i 127.0.0.1 --url-arg`, `-t theme={"background":"#0A0D13","foreground":"#EAF0FB","cursor":"#FFAE1A","selectionBackground":"#2E3852"}`, `-t fontSize=11`, `-t fontFamily=Ubuntu Sans Mono, JetBrainsMono Nerd Font Mono, JetBrainsMono Nerd Font, JetBrains Mono, DejaVu Sans Mono, monospace`, `-t rendererType=canvas`, `-t scrollback=10000 -t cursorBlink=true -t disableLeaveAlert=true -t disableResizeOverlay=true`, comando `cc-webterm-attach`.
- `bin/cc-webterm-attach <token> <sesión>`: compara el token recortado con `H/dash-token` recortado («Acceso denegado al terminal de ComandOS.» y sale 1); con tmux ≥ 3.2 añade `-f active-pane`; sesión válida `^[A-Za-z0-9._-]{1,80}$` y existente → `exec tmux attach -t =<sesión>`; si no, «La sesion '<s>' ya no existe.» y un selector numerado de `list-sessions -F #{session_name}` sin `local|hub|control`, o `$SHELL`.
- `tailscale serve`: `/` → 4777, `/term` → `127.0.0.1:4780/term`, `:8443/` → 4779. Este plan **no cambia** `tailscale serve`: el oyente de compatibilidad ocupa los mismos puertos.
- `dash/term.html`: `new WebSocket(<base>/ws?arg=<token>&arg=<sesión>, ['tty'])`, `binaryType='arraybuffer'`, primer mensaje `{"AuthToken":"","columns":C,"rows":R}`, entrada `'0'+bytes` binario, resize `'1'+JSON` texto, salida `'0'|'1'|'2'`; aplica `prefs.fontFamily`/`prefs.fontSize` del `'2'` (así la letra efectiva es 11 px); reconexión 250 ms·2ⁿ hasta 4 s; cola de entrada 4096 B / 15 s; `postMessage` con el padre (`ready`, `user-interaction`, `pane-selected`; recibe `theme`, `button-style`, sesión); llama a `/terminal-panes`, `/tab-models`, `/terminal-history`, `/tmux-scroll`, `/accounts`, `/account/switch`, `/account/add`, `/model/status`, `/workspace/client`.
- `dash/index.html`: remoto (`https:` no loopback) usa `${origin}/term/?auth=<token>&arg=<sesión>&theme=<tema>` y sondea `${origin}/term/token`; tras 3 fallos, `https://<host>:8443/?arg=<token>&arg=<sesión>`. En `cc-app` (loopback) la barra lateral usa `http://127.0.0.1:4779/?arg=<token>&arg=<sesión>` (UI embebida de ttyd). `bin/cc-app:3512-3567` (pestaña experimental) y `bin/cc-app-mac:94-95` usan `ws://127.0.0.1:4779/ws` y `http://127.0.0.1:4779/token`.
- `config/terminal-replies.conf` traga las respuestas DA2 de VTE 0.68 y de xterm.js (`ESC [ > 0;276;0 c`); xterm.js responde DA1 `ESC [ ? 1;2c`. alacritty responde `ESC [ ? 6c` y `ESC [ > 0;2600;1c`.

## Estructura de archivos

```
Cargo.toml                                        ([patch.crates-io] alacritty_terminal; miembros)      A1, A6
vendor/alacritty_terminal/                        (0.26.0 + parche wasm; LICENSE-APACHE)                A1
crates/comandos-term/Cargo.toml
crates/comandos-term/src/lib.rs
crates/comandos-term/src/engine.rs                (Engine, GridSize, Palette, Drained, Modes)            A1
crates/comandos-term/src/clock.rs                 (ClockSync: Timeout de vte sobre reloj inyectado)      A1
crates/comandos-term/src/proto.rs                 (Dialect, ClientMsg, ServerMsg, TtyPrefs)              A2
crates/comandos-term/src/render.rs                (Style, Run, RowRender, CursorView, resolve_color)     A5
crates/comandos-term/src/glyphs.rs                (BoxOp, box_ops: dibujo de U+2500–259F y powerline)    A5
crates/comandos-term/src/input.rs                 (KeyInput, encode_key, encode_mouse, paste, wheel)     A7
crates/comandos-term/src/select.rs                (Selection por celdas, texto copiado, links)           A8
crates/comandos-term/tests/{engine,proto,render,input,select}.rs
crates/comandos-server/src/dash/term/mod.rs       (TermRoutes, TermMode)                                 A4
crates/comandos-server/src/dash/term/attach.rs    (AttachCommand, TmuxTarget, valid_session)             A3
crates/comandos-server/src/dash/term/bridge.rs    (run_bridge, Outbox, contrapresión)                    A3
crates/comandos-server/src/dash/term/replay.rs    (fuente de grabaciones para la sombra)                 A3
crates/comandos-server/src/dash/term/compat.rs    (oyente 4780/4779, perfiles path/plain)                A4, A11
crates/comandos-server/tests/support/private_tmux.rs (PrivateTmux con Drop: kill-server -S y luego rm)  A3
crates/comandos-server/tests/term_{bridge,routes,compat}.rs
crates/comandos-cli/src/webterm.rs                (port de bin/cc-webterm)                               A4
crates/comandos-cli/src/webterm_attach.rs         (port de bin/cc-webterm-attach)                        A4
crates/comandos-cli/tests/webterm.rs
crates/comandos-term-web/                         (cdylib: canvas.rs, metrics.rs, keyboard.rs, mouse.rs,
                                                    links.rs, connection.rs, page.rs, remote.rs)          A6–A10
xtask/src/term_bench.rs, xtask/src/term_record.rs, xtask/web/replays/*.bin, xtask/web/shots.json (term-*) A12
docs/verification/cutover-term.md, docs/verification/fase3/term-*.json                                    A4, A11, A12
```

---

### Task A1: Motor VT (`comandos-term::engine`) sobre `alacritty_terminal` vendorizado

**Independiente:** sí (de todo; solo necesita el workspace).

**Files:**
- Create: `vendor/alacritty_terminal/` (copia de `~/.cargo/registry/src/*/alacritty_terminal-0.26.0/` con el parche), `crates/comandos-term/{Cargo.toml,src/lib.rs,src/engine.rs,src/clock.rs}`
- Modify: `Cargo.toml` (miembro `crates/comandos-term`, `[patch.crates-io]`)
- Test: `crates/comandos-term/tests/engine.rs`

**Interfaces:**
- Produces:
  - `pub struct GridSize { pub cols: u16, pub rows: u16 }` (implementa `alacritty_terminal::grid::Dimensions`).
  - `pub struct Palette { pub fg: [u8; 3], pub bg: [u8; 3], pub cursor: [u8; 3], pub cursor_accent: [u8; 3], pub selection: [u8; 3], pub ansi: [[u8; 3]; 256] }` con `Palette::xterm_default(fg, bg, cursor, cursor_accent, selection) -> Palette` (los 16 de xterm.js 5.5 + cubo 6×6×6 + grises).
  - `pub struct Engine`; `Engine::new(size: GridSize, scrollback: usize, palette: Palette) -> Engine`; `advance(&mut self, bytes: &[u8], now_ms: f64)`; `tick(&mut self, now_ms: f64) -> bool` (cierra una actualización sincronizada vencida; `true` si hubo cambio); `next_deadline_ms(&self) -> Option<f64>`; `resize(&mut self, size: GridSize, cell_px: (u16, u16))`; `drain(&mut self) -> Drained`; `modes(&self) -> Modes`; `take_damage(&mut self) -> Damage`; `scroll_display(&mut self, lines: i32)`; `display_offset(&self) -> usize`; `history_len(&self) -> usize`; `term(&self) -> &Term<Collector>` (lectura para render y selección).
  - `pub struct Drained { pub replies: Vec<u8>, pub title: Option<String>, pub clipboard: Option<String>, pub bell: bool }`.
  - `pub struct Modes { pub app_cursor: bool, pub app_keypad: bool, pub bracketed_paste: bool, pub mouse: MouseMode, pub sgr_mouse: bool, pub alt_screen: bool, pub alternate_scroll: bool, pub focus_events: bool, pub cursor_visible: bool }`; `pub enum MouseMode { Off, Click, Drag, Motion }`.
  - `pub enum Damage { Full, Lines(Vec<usize>) }` (índices de línea de pantalla).
  - Constantes `DA1_REPLY = b"\x1b[?1;2c"`, `DA2_REPLY = b"\x1b[>0;276;0c"` (las de xterm.js 5.5.0).

- [ ] **Step 1: Vendorizar con el parche.** Copiar el crate del registro a `vendor/alacritty_terminal/` (incluye `LICENSE-APACHE`). Parche (lo mínimo que el spike validó):
  - `src/lib.rs`: `#[cfg(not(target_arch = "wasm32"))]` delante de `pub mod event_loop;` y de `pub mod tty;`.
  - `Cargo.toml`: mover `home`, `libc` y `polling` a `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`.
  - Añadir `vendor/alacritty_terminal/COMANDOS-PATCH.md` (tres líneas: origen, versión, las dos modificaciones) y en el `Cargo.toml` raíz:

```toml
[patch.crates-io]
alacritty_terminal = { path = "vendor/alacritty_terminal" }
```

`crates/comandos-term/Cargo.toml`:

```toml
[package]
name = "comandos-term"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
alacritty_terminal = { version = "=0.26.0", default-features = false }

[lints]
workspace = true
```

- [ ] **Step 2: Prueba que falla**

```rust
// crates/comandos-term/tests/engine.rs
use alacritty_terminal::{index::{Column, Line, Point}, term::cell::Flags, vte::ansi::{Color, Rgb}};
use comandos_term::engine::{DA1_REPLY, DA2_REPLY, Damage, Engine, GridSize, MouseMode, Palette};

fn engine(cols: u16, rows: u16) -> Engine {
    let p = Palette::xterm_default([0xEA, 0xF0, 0xFB], [0x0A, 0x0D, 0x13], [0xFF, 0xAE, 0x1A], [0x0A, 0x0D, 0x13], [0x2E, 0x38, 0x52]);
    Engine::new(GridSize { cols, rows }, 10_000, p)
}

fn cell(e: &Engine, line: i32, col: usize) -> alacritty_terminal::term::cell::Cell {
    e.term().grid()[Point::new(Line(line), Column(col))].clone()
}

#[test]
fn truecolor_bold_text_lands_in_the_grid() {
    let mut e = engine(20, 3);
    e.advance(b"\x1b[1;38;2;255;0;0mA", 0.0);
    let c = cell(&e, 0, 0);
    assert_eq!(c.c, 'A');
    assert_eq!(c.fg, Color::Spec(Rgb { r: 255, g: 0, b: 0 }));
    assert!(c.flags.contains(Flags::BOLD));
}

#[test]
fn cjk_takes_two_cells() {
    let mut e = engine(20, 3);
    e.advance("漢x".as_bytes(), 0.0);
    assert!(cell(&e, 0, 0).flags.contains(Flags::WIDE_CHAR));
    assert!(cell(&e, 0, 1).flags.contains(Flags::WIDE_CHAR_SPACER));
    assert_eq!(cell(&e, 0, 2).c, 'x');
}

#[test]
fn da1_and_da2_replies_match_xterm_js() {
    // Review Focus 5: tmux 3.2a + config/terminal-replies.conf esperan la DA2 de xterm.js.
    let mut e = engine(20, 3);
    e.advance(b"\x1b[c\x1b[>c", 0.0);
    let mut want = DA1_REPLY.to_vec();
    want.extend_from_slice(DA2_REPLY);
    assert_eq!(e.drain().replies, want);
    assert_eq!(DA2_REPLY, b"\x1b[>0;276;0c");
}

#[test]
fn osc_title_and_osc52_are_events() {
    let mut e = engine(20, 3);
    e.advance(b"\x1b]0;hola\x07\x1b]52;c;aG9sYQ==\x07", 0.0);
    let d = e.drain();
    assert_eq!(d.title.as_deref(), Some("hola"));
    assert_eq!(d.clipboard.as_deref(), Some("hola"));
}

#[test]
fn synchronized_update_waits_for_end_or_deadline() {
    // Claude Code emite CSI ? 2026 h; sin reloj inyectado, vte llamaría a Instant::now (pánico en wasm).
    let mut e = engine(20, 3);
    let _ = e.take_damage();
    e.advance(b"\x1b[?2026hX", 0.0);
    assert_eq!(cell(&e, 0, 0).c, ' ', "retenido durante la actualización sincronizada");
    assert!(e.next_deadline_ms().is_some());
    assert!(!e.tick(50.0));
    assert!(e.tick(1_000.0), "el plazo vencido vuelca lo retenido");
    assert_eq!(cell(&e, 0, 0).c, 'X');
    e.advance(b"\x1b[?2026hY\x1b[?2026l", 2_000.0);
    assert_eq!(cell(&e, 0, 1).c, 'Y', "el fin explícito vuelca sin esperar");
}

#[test]
fn scrollback_is_bounded_to_ten_thousand_lines() {
    let mut e = engine(10, 5);
    let lines: String = (0..10_100).map(|i| format!("{i}\r\n")).collect();
    e.advance(lines.as_bytes(), 0.0);
    assert_eq!(e.history_len(), 10_000);
}

#[test]
fn modes_follow_dec_private_sequences() {
    let mut e = engine(10, 5);
    e.advance(b"\x1b[?1h\x1b[?2004h\x1b[?1002h\x1b[?1006h\x1b[?1049h\x1b[?1004h", 0.0);
    let m = e.modes();
    assert!(m.app_cursor && m.bracketed_paste && m.sgr_mouse && m.alt_screen && m.focus_events);
    assert_eq!(m.mouse, MouseMode::Drag);
}

#[test]
fn damage_reports_only_touched_lines() {
    let mut e = engine(10, 5);
    let _ = e.take_damage();
    e.advance(b"\x1b[3;1Hz", 0.0);
    match e.take_damage() {
        Damage::Lines(lines) => assert!(lines.contains(&2) && !lines.contains(&0), "{lines:?}"),
        Damage::Full => panic!("se esperaba daño parcial"),
    }
}
```

- [ ] **Step 3:** `$C test -p comandos-term --test engine` → FAIL (no compila).
- [ ] **Step 4: Implementación**

```rust
// crates/comandos-term/src/clock.rs
//! Reloj de las actualizaciones sincronizadas (CSI ? 2026). vte llama a
//! `Instant::now()` en su temporizador por defecto, que en wasm32 entra en
//! pánico; aquí el tiempo lo pone quien llama (`Engine::advance/tick`).
use alacritty_terminal::vte::ansi::Timeout;
use std::{cell::Cell, time::Duration};

thread_local! {
    static NOW_MS: Cell<f64> = const { Cell::new(0.0) };
}

pub(crate) fn set_now(now_ms: f64) { NOW_MS.with(|n| n.set(now_ms)); }
fn now() -> f64 { NOW_MS.with(Cell::get) }

#[derive(Default)]
pub struct ClockSync { deadline_ms: Option<f64> }

impl ClockSync {
    pub fn deadline_ms(&self) -> Option<f64> { self.deadline_ms }
}

impl Timeout for ClockSync {
    fn set_timeout(&mut self, duration: Duration) {
        self.deadline_ms = Some(now() + duration.as_secs_f64() * 1000.0);
    }
    fn clear_timeout(&mut self) { self.deadline_ms = None; }
    fn pending_timeout(&self) -> bool { self.deadline_ms.is_some_and(|d| now() < d) }
}
```

```rust
// crates/comandos-term/src/engine.rs (núcleo; la paleta xterm_default rellena
// 0–15 con los colores de xterm.js 5.5 DEFAULT_ANSI_COLORS, 16–231 con el cubo
// [0,95,135,175,215,255] y 232–255 con grises 8+10·i)
use crate::clock::{ClockSync, set_now};
use alacritty_terminal::{
    event::{Event, EventListener, WindowSize},
    grid::{Dimensions, Scroll},
    term::{Config, Term, TermDamage, TermMode},
    vte::ansi::{Processor, Rgb},
};
use std::{cell::Cell, rc::Rc};

pub const DA1_REPLY: &[u8] = b"\x1b[?1;2c";
pub const DA2_REPLY: &[u8] = b"\x1b[>0;276;0c";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize { pub cols: u16, pub rows: u16 }

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize { usize::from(self.rows) }
    fn screen_lines(&self) -> usize { usize::from(self.rows) }
    fn columns(&self) -> usize { usize::from(self.cols) }
}

/// Recolector de eventos: `send_event(&self)` exige mutabilidad interior;
/// `Cell<Vec<_>>` con take/set no puede entrar en pánico (RefCell sí).
#[derive(Clone, Default)]
pub struct Collector(Rc<Cell<Vec<Event>>>);

impl EventListener for Collector {
    fn send_event(&self, event: Event) {
        let mut v = self.0.take();
        // Cota: un flujo hostil no hace crecer la cola sin límite.
        if v.len() < 4096 { v.push(event); }
        self.0.set(v);
    }
}

pub struct Engine {
    term: Term<Collector>,
    parser: Processor<ClockSync>,
    events: Collector,
    palette: Palette,
    cell_px: (u16, u16),
    size: GridSize,
}

impl Engine {
    pub fn new(size: GridSize, scrollback: usize, palette: Palette) -> Engine {
        let events = Collector::default();
        let config = Config { scrolling_history: scrollback, ..Config::default() };
        let term = Term::new(config, &size, events.clone());
        Engine { term, parser: Processor::new(), events, palette, cell_px: (8, 16), size }
    }

    pub fn advance(&mut self, bytes: &[u8], now_ms: f64) {
        set_now(now_ms);
        self.parser.advance(&mut self.term, bytes);
    }

    pub fn next_deadline_ms(&self) -> Option<f64> { self.parser.sync_timeout().deadline_ms() }

    pub fn tick(&mut self, now_ms: f64) -> bool {
        set_now(now_ms);
        match self.next_deadline_ms() {
            Some(d) if now_ms >= d => { self.parser.stop_sync(&mut self.term); true }
            _ => false,
        }
    }

    pub fn resize(&mut self, size: GridSize, cell_px: (u16, u16)) {
        self.size = size;
        self.cell_px = cell_px;
        self.term.resize(size);
    }

    pub fn drain(&mut self) -> Drained {
        let mut out = Drained::default();
        for event in self.events.0.take() {
            match event {
                Event::PtyWrite(text) => out.replies.extend_from_slice(rewrite_reply(text.as_bytes())),
                Event::ColorRequest(index, format) => {
                    let rgb = self.palette.color_for_request(index);
                    out.replies.extend_from_slice(format(Rgb { r: rgb[0], g: rgb[1], b: rgb[2] }).as_bytes());
                }
                Event::TextAreaSizeRequest(format) => {
                    let ws = WindowSize {
                        num_lines: self.size.rows, num_cols: self.size.cols,
                        cell_width: self.cell_px.0, cell_height: self.cell_px.1,
                    };
                    out.replies.extend_from_slice(format(ws).as_bytes());
                }
                Event::Title(t) => out.title = Some(t),
                Event::ResetTitle => out.title = Some(String::new()),
                Event::ClipboardStore(_, text) => out.clipboard = Some(text),
                Event::Bell => out.bell = true,
                _ => {}
            }
        }
        out
    }

    pub fn take_damage(&mut self) -> Damage {
        let damage = match self.term.damage() {
            TermDamage::Full => Damage::Full,
            TermDamage::Partial(it) => Damage::Lines(it.map(|l| l.line).collect()),
        };
        self.term.reset_damage();
        damage
    }

    pub fn scroll_display(&mut self, lines: i32) { self.term.scroll_display(Scroll::Delta(lines)); }
    pub fn display_offset(&self) -> usize { self.term.grid().display_offset() }
    pub fn history_len(&self) -> usize { self.term.grid().history_size() }
    pub fn term(&self) -> &Term<Collector> { &self.term }

    pub fn modes(&self) -> Modes {
        let m = *self.term.mode();
        let mouse = if m.contains(TermMode::MOUSE_MOTION) { MouseMode::Motion }
            else if m.contains(TermMode::MOUSE_DRAG) { MouseMode::Drag }
            else if m.contains(TermMode::MOUSE_REPORT_CLICK) { MouseMode::Click }
            else { MouseMode::Off };
        Modes {
            app_cursor: m.contains(TermMode::APP_CURSOR), app_keypad: m.contains(TermMode::APP_KEYPAD),
            bracketed_paste: m.contains(TermMode::BRACKETED_PASTE), mouse,
            sgr_mouse: m.contains(TermMode::SGR_MOUSE), alt_screen: m.contains(TermMode::ALT_SCREEN),
            alternate_scroll: m.contains(TermMode::ALTERNATE_SCROLL),
            focus_events: m.contains(TermMode::FOCUS_IN_OUT), cursor_visible: m.contains(TermMode::SHOW_CURSOR),
        }
    }
}

/// alacritty contesta DA1 `?6c` y DA2 `>0;<versión>;1c`; tmux y
/// config/terminal-replies.conf esperan las de xterm.js (Review Focus 5).
fn rewrite_reply(reply: &[u8]) -> &[u8] {
    if reply == b"\x1b[?6c" { return DA1_REPLY; }
    if reply.starts_with(b"\x1b[>0;") && reply.ends_with(b";1c") { return DA2_REPLY; }
    reply
}
```

`Drained`, `Modes`, `MouseMode`, `Damage` y `Palette` (con `color_for_request(index: usize) -> [u8; 3]`: 0–255 → `ansi`, 256 → `fg`, 257 → `bg`, 258 → `cursor`, otro → `fg`) se declaran en el mismo módulo con `#[derive(Debug, Clone, Default, PartialEq)]` donde aplique. Si la API real de la versión vendorizada usa otros nombres (`TermDamage::Partial` iterando `LineDamageBounds`, `Processor::stop_sync` con firma distinta), se usa la real sin cambiar las interfaces públicas de arriba y se anota en el commit.

- [ ] **Step 5:** `$C test -p comandos-term --test engine` → PASS.
- [ ] **Step 6: Compila en wasm y sin libc.**

```bash
$C check -p comandos-term --target wasm32-unknown-unknown
$C tree -p comandos-term --target wasm32-unknown-unknown -i libc   # salida vacía
```

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock vendor/alacritty_terminal crates/comandos-term
git commit -m "feat(term): motor VT sobre alacritty_terminal 0.26 vendorizado, reloj inyectado y respuestas DA de xterm.js

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task A2: Protocolo de terminal con dos dialectos (`comandos-term::proto`)

**Independiente:** sí.

**Files:**
- Create: `crates/comandos-term/src/proto.rs`
- Modify: `crates/comandos-term/src/lib.rs`, `crates/comandos-term/Cargo.toml` (`serde_json.workspace = true`)
- Test: `crates/comandos-term/tests/proto.rs`

**Interfaces:**
- Produces:
  - `pub enum Dialect { Tty, V1 }` con `Dialect::from_protocol(p: Option<&str>) -> Option<Dialect>` (`"tty"` → `Tty`, `"comandos.term.v1"` → `V1`); `pub const PROTOCOLS: &[&str] = &["comandos.term.v1", "tty"]`.
  - `pub struct Init { pub cols: u16, pub rows: u16, pub session: Option<String> }`.
  - `pub enum ClientMsg { Input(Vec<u8>), Resize { cols: u16, rows: u16 }, Pause, Resume }`.
  - `pub fn parse_init(dialect: Dialect, frame: &[u8]) -> Result<Init, ProtoError>`; `pub fn parse_client(frame: &[u8]) -> Result<ClientMsg, ProtoError>` (igual en ambos dialectos; frames texto o binario); `pub fn tty_args(query: &str) -> Vec<String>` (los `arg` repetidos, decodificados, en orden).
  - `pub fn output(bytes: &[u8]) -> Vec<u8>` (`'0'`+bytes), `pub fn title(text: &str) -> Vec<u8>` (`'1'`+texto), `pub fn prefs(json: &str) -> Vec<u8>` (`'2'`+json).
  - `pub const TTY_PREFS: &str` — el JSON exacto que ttyd 1.6.3 manda con las opciones `-t` de `bin/cc-webterm` (lo fija el Step 6 contra un ttyd real).
  - `pub fn tty_title(command: &str, host: &str) -> String` (`"<command> (<host>)"`, formato de ttyd).
  - Límites: tamaño `cols ∈ 2..=1000`, `rows ∈ 1..=500` (si no, `ProtoError::BadSize`); frame de entrada ≤ 64 KiB.

- [ ] **Step 1: Prueba que falla**

```rust
// crates/comandos-term/tests/proto.rs
use comandos_term::proto::*;

#[test]
fn tty_init_is_raw_json_without_prefix() {
    let init = parse_init(Dialect::Tty, br#"{"AuthToken":"","columns":96,"rows":30}"#).unwrap();
    assert_eq!((init.cols, init.rows, init.session), (96, 30, None));
}

#[test]
fn v1_init_carries_session() {
    let init = parse_init(Dialect::V1, br#"{"v":1,"cols":80,"rows":24,"session":"term-1-2"}"#).unwrap();
    assert_eq!(init.session.as_deref(), Some("term-1-2"));
    assert!(parse_init(Dialect::V1, br#"{"v":2,"cols":80,"rows":24}"#).is_err());
}

#[test]
fn sizes_out_of_range_are_rejected() {
    assert!(matches!(parse_init(Dialect::Tty, br#"{"columns":0,"rows":30}"#), Err(ProtoError::BadSize)));
    assert!(matches!(parse_init(Dialect::Tty, br#"{"columns":80,"rows":9999}"#), Err(ProtoError::BadSize)));
}

#[test]
fn client_messages_parse_from_text_or_binary_frames() {
    assert_eq!(parse_client(b"0ls\r").unwrap(), ClientMsg::Input(b"ls\r".to_vec()));
    assert_eq!(parse_client(br#"1{"columns":120,"rows":40}"#).unwrap(), ClientMsg::Resize { cols: 120, rows: 40 });
    assert_eq!(parse_client(b"2").unwrap(), ClientMsg::Pause);
    assert_eq!(parse_client(b"3").unwrap(), ClientMsg::Resume);
    assert!(parse_client(b"9").is_err());
    assert!(parse_client(&[b'0'; 70_000]).is_err());
}

#[test]
fn tty_args_keep_order_and_decode() {
    assert_eq!(tty_args("arg=t%20k&x=1&arg=term-1"), vec!["t k".to_string(), "term-1".to_string()]);
}

#[test]
fn server_frames_have_ttyd_prefixes() {
    assert_eq!(output(b"hi"), b"0hi".to_vec());
    assert_eq!(title("cc-webterm-attach (host)"), b"1cc-webterm-attach (host)".to_vec());
    assert_eq!(&prefs("{}")[..1], b"2");
}
```

- [ ] **Step 2:** `$C test -p comandos-term --test proto` → FAIL.
- [ ] **Step 3: Implementación** (`serde_json::Value` para leer, nunca indexado):

```rust
// crates/comandos-term/src/proto.rs (núcleo)
use serde_json::Value;

pub const PROTOCOLS: &[&str] = &["comandos.term.v1", "tty"];
const MAX_INPUT: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect { Tty, V1 }

#[derive(Debug, PartialEq, Eq)]
pub enum ProtoError { BadJson, BadSize, BadVersion, Unknown, TooLarge }

#[derive(Debug, PartialEq, Eq)]
pub enum ClientMsg { Input(Vec<u8>), Resize { cols: u16, rows: u16 }, Pause, Resume }

#[derive(Debug, PartialEq, Eq)]
pub struct Init { pub cols: u16, pub rows: u16, pub session: Option<String> }

impl Dialect {
    pub fn from_protocol(p: Option<&str>) -> Option<Dialect> {
        match p { Some("tty") => Some(Dialect::Tty), Some("comandos.term.v1") => Some(Dialect::V1), _ => None }
    }
}

fn size(v: &Value, cols_key: &str, rows_key: &str) -> Result<(u16, u16), ProtoError> {
    let get = |k: &str| v.get(k).and_then(Value::as_u64).and_then(|n| u16::try_from(n).ok());
    match (get(cols_key), get(rows_key)) {
        (Some(c), Some(r)) if (2..=1000).contains(&c) && (1..=500).contains(&r) => Ok((c, r)),
        _ => Err(ProtoError::BadSize),
    }
}

pub fn parse_init(dialect: Dialect, frame: &[u8]) -> Result<Init, ProtoError> {
    let v: Value = serde_json::from_slice(frame).map_err(|_| ProtoError::BadJson)?;
    match dialect {
        Dialect::Tty => { let (cols, rows) = size(&v, "columns", "rows")?; Ok(Init { cols, rows, session: None }) }
        Dialect::V1 => {
            if v.get("v").and_then(Value::as_u64) != Some(1) { return Err(ProtoError::BadVersion); }
            let (cols, rows) = size(&v, "cols", "rows")?;
            Ok(Init { cols, rows, session: v.get("session").and_then(Value::as_str).map(str::to_string) })
        }
    }
}

pub fn parse_client(frame: &[u8]) -> Result<ClientMsg, ProtoError> {
    if frame.len() > MAX_INPUT + 1 { return Err(ProtoError::TooLarge); }
    let (cmd, rest) = frame.split_first().ok_or(ProtoError::Unknown)?;
    match cmd {
        b'0' => Ok(ClientMsg::Input(rest.to_vec())),
        b'1' => {
            let v: Value = serde_json::from_slice(rest).map_err(|_| ProtoError::BadJson)?;
            let (cols, rows) = size(&v, "columns", "rows")?;
            Ok(ClientMsg::Resize { cols, rows })
        }
        b'2' => Ok(ClientMsg::Pause),
        b'3' => Ok(ClientMsg::Resume),
        _ => Err(ProtoError::Unknown),
    }
}

fn prefixed(prefix: u8, body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(body.len() + 1);
    v.push(prefix);
    v.extend_from_slice(body);
    v
}
pub fn output(bytes: &[u8]) -> Vec<u8> { prefixed(b'0', bytes) }
pub fn title(text: &str) -> Vec<u8> { prefixed(b'1', text.as_bytes()) }
pub fn prefs(json: &str) -> Vec<u8> { prefixed(b'2', json.as_bytes()) }
pub fn tty_title(command: &str, host: &str) -> String { format!("{command} ({host})") }
```

`tty_args` reutiliza la decodificación `%XX`/`+` de `comandos_core::dashboard_access::query_pairs(query, true)` (ya portada y probada) y filtra `arg`.

- [ ] **Step 4:** `$C test -p comandos-term --test proto` → PASS.
- [ ] **Step 5: Prueba diferencial contra ttyd real (fija `TTY_PREFS` y el título).** `crates/comandos-server/tests/term_ttyd_frames.rs` (vive en el servidor porque usa `PrivateTmux` de A3; si A3 aún no existe, esta prueba se añade al cerrar A3). Se salta con aviso si `ttyd` no está en el PATH. Levanta `ttyd -p <puerto 73xx libre> -i 127.0.0.1 --url-arg <los -t de bin/cc-webterm> tmux -S <dir>/tmux-1000/default attach -t =t1` sobre un `PrivateTmux` con la sesión `t1`, se conecta con `tokio-tungstenite` (subprotocolo `tty`), manda el init y guarda las dos primeras tramas: la `'1'` (título) y la `'2'` (preferencias). Comprueba `frames[1] == prefs(TTY_PREFS)` y que el título sea `tty_title(<argv0 que ttyd muestra>, <hostname>)`; al terminar mata ttyd y el `PrivateTmux` hace `kill-server -S` y borra el directorio. Primera ejecución: el implementador copia el JSON capturado a `TTY_PREFS` (literal, en el orden que lo manda ttyd) y lo comitea; la prueba lo vigila desde entonces.
- [ ] **Step 6: Commit**

```bash
git add crates/comandos-term/src/proto.rs crates/comandos-term/src/lib.rs crates/comandos-term/Cargo.toml crates/comandos-term/tests/proto.rs Cargo.lock
git commit -m "feat(term): protocolo de terminal con dialectos tty (ttyd 1.6.3) y comandos.term.v1

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task A3: Puente PTY↔WebSocket con lista cerrada de comandos tmux y contrapresión

**Depende de:** A2 (y T3 para el tipo `WsStream`; si T3 no está, el puente se prueba con un par de canales en memoria y se cablea en A4).

**Files:**
- Create: `crates/comandos-server/src/dash/term/{mod.rs,attach.rs,bridge.rs,replay.rs}`, `crates/comandos-server/tests/support/private_tmux.rs`
- Modify: `crates/comandos-server/src/dash/mod.rs` (`pub mod term;`), `crates/comandos-server/Cargo.toml` (`comandos-term`, `pty-process`)
- Test: `crates/comandos-server/tests/term_bridge.rs`

**Interfaces:**
- Consumes: `comandos_term::proto::*` (A2); `native::tmux::{Tmux, Program, private_socket}` (2b); `comandos_core::dashboard_access::token_matches` y `token::load_token`.
- Produces:
  - `pub enum TmuxTarget { User, Private(PathBuf) }` — `User` no pasa `-S` (mismo servidor que hoy usa ttyd: el del entorno del frente); `Private(dir)` pasa `-S <dir>/tmux-<uid>/default`.
  - `pub enum AttachCommand { Version, HasSession(String), ListSessionNames, Attach { session: String, active_pane: bool } }` con `fn args(&self) -> Vec<String>` — **lista cerrada**: no existe variante que mate, cree o configure nada.
  - `pub fn valid_session(name: &str) -> bool` (`^[A-Za-z0-9._-]{1,80}$`).
  - `pub enum Source { Pty { target: TmuxTarget, session: Option<String> }, Replay(PathBuf) }`.
  - `pub struct BridgeLimits { pub outbox_bytes: usize, pub read_chunk: usize }` (por omisión 1 MiB y 64 KiB).
  - `pub async fn run_bridge<S>(socket: S, dialect: Dialect, init: Init, source: Source, limits: BridgeLimits, shutdown: watch::Receiver<bool>)` donde `S: Stream<Item = Result<Message, WsError>> + Sink<Message> + Unpin`.
  - Test support: `PrivateTmux::start(sessions: &[&str]) -> Option<PrivateTmux>` (None si no hay tmux), `PrivateTmux::target() -> TmuxTarget`, `PrivateTmux::capture(session) -> String`, `PrivateTmux::clients(session) -> usize`; `Drop`: `kill-server` con **su** `-S`, luego borra el directorio.

- [ ] **Step 1: Soporte de pruebas `PrivateTmux`** (regla tmux del índice; único lugar de las pruebas que lanza tmux):

```rust
// crates/comandos-server/tests/support/private_tmux.rs
//! Servidor tmux de usar y tirar. Siempre con `-S` propio: nunca toca el
//! servidor del usuario. Drop: kill-server con ESTE socket y luego borra.
use comandos_server::dash::{native::tmux::private_socket, term::attach::TmuxTarget};
use std::{path::PathBuf, process::Command};

pub struct PrivateTmux { dir: PathBuf, socket: PathBuf }

impl PrivateTmux {
    pub fn start(sessions: &[&str]) -> Option<PrivateTmux> {
        if Command::new("tmux").arg("-V").output().is_err() {
            eprintln!("aviso: sin tmux, prueba omitida");
            return None;
        }
        let dir = std::env::temp_dir().join(format!("comandos-term-test-{}-{}", std::process::id(), rand_suffix()));
        let socket = private_socket(&dir); // crea <dir>/tmux-<uid>/ con 0700
        let me = PrivateTmux { dir, socket };
        for s in sessions {
            let ok = me.tmux(&["-f", "/dev/null", "new-session", "-d", "-s", s, "-x", "80", "-y", "24"]).status.success();
            assert!(ok, "no se pudo crear la sesión privada {s}");
        }
        Some(me)
    }
    pub fn tmux(&self, args: &[&str]) -> std::process::Output {
        let socket = self.socket.to_string_lossy().to_string();
        Command::new("tmux").arg("-S").arg(&socket).args(args)
            .env_remove("TMUX").env_remove("TMUX_PANE").output().expect("tmux")
    }
    pub fn target(&self) -> TmuxTarget { TmuxTarget::Private(self.dir.clone()) }
    pub fn capture(&self, session: &str) -> String {
        String::from_utf8_lossy(&self.tmux(&["capture-pane", "-p", "-t", &format!("={session}")]).stdout).into_owned()
    }
    pub fn clients(&self, session: &str) -> usize {
        String::from_utf8_lossy(&self.tmux(&["list-clients", "-t", &format!("={session}")]).stdout).lines().count()
    }
}

impl Drop for PrivateTmux {
    fn drop(&mut self) {
        // Orden de CLAUDE.md: primero kill-server con el -S propio, después borrar.
        let _ = self.tmux(&["kill-server"]);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn rand_suffix() -> u64 {
    let mut b = [0u8; 8];
    let _ = getrandom::fill(&mut b);
    u64::from_le_bytes(b)
}
```

- [ ] **Step 2: Prueba que falla**

```rust
// crates/comandos-server/tests/term_bridge.rs
mod support;
use comandos_server::dash::term::{attach::{AttachCommand, TmuxTarget, valid_session}, bridge::{BridgeLimits, Source, run_bridge}};
use comandos_term::proto::{Dialect, Init, output};
use futures_util::{SinkExt, StreamExt};
use support::{private_tmux::PrivateTmux, ws_pair};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

#[test]
fn live_commands_are_attach_or_has_session_only() {
    // Regla tmux: el puente solo sabe pedir esto; ninguna variante es de alcance de servidor destructivo.
    let all = [
        AttachCommand::Version,
        AttachCommand::HasSession("s".into()),
        AttachCommand::ListSessionNames,
        AttachCommand::Attach { session: "s".into(), active_pane: true },
    ];
    for cmd in all {
        let args = cmd.args().join(" ");
        for banned in ["kill", "new-session", "set", "source", "unlink", "respawn", "detach -a", "server"] {
            assert!(!args.contains(banned), "{args}");
        }
    }
    assert_eq!(AttachCommand::Attach { session: "s".into(), active_pane: true }.args(), ["attach", "-f", "active-pane", "-t", "=s"]);
}

#[test]
fn session_names_are_validated() {
    assert!(valid_session("term-12-3") && valid_session("a.b_c"));
    assert!(!valid_session("") && !valid_session("x;y") && !valid_session(&"a".repeat(81)) && !valid_session("=s"));
}

#[tokio::test(flavor = "current_thread")]
async fn bridge_attaches_with_client_size_types_and_detaches_on_close() {
    let Some(tmux) = PrivateTmux::start(&["t1"]) else { return };
    let (server, mut client) = ws_pair().await;
    let (_stop, shutdown) = tokio::sync::watch::channel(false);
    let init = Init { cols: 100, rows: 30, session: Some("t1".into()) };
    let task = tokio::spawn(run_bridge(server, Dialect::Tty, init,
        Source::Pty { target: tmux.target(), session: Some("t1".into()) }, BridgeLimits::default(), shutdown));
    // El cliente tmux nace con el tamaño del navegador (no encoge la sesión).
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(tmux.clients("t1"), 1);
    let size = String::from_utf8_lossy(&tmux.tmux(&["list-clients", "-F", "#{client_width}x#{client_height}"]).stdout).trim().to_string();
    assert_eq!(size, "100x30");
    client.send(Message::binary(b"0echo puente-ok\r".to_vec())).await.unwrap();
    let mut seen = Vec::new();
    while let Ok(Some(Ok(msg))) = tokio::time::timeout(Duration::from_secs(3), client.next()).await {
        seen.extend_from_slice(&msg.into_data());
        if String::from_utf8_lossy(&seen).contains("puente-ok") { break; }
    }
    assert!(tmux.capture("t1").contains("puente-ok"));
    drop(client);
    let _ = tokio::time::timeout(Duration::from_secs(3), task).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(tmux.clients("t1"), 0, "cerrar el socket desengancha el cliente");
    assert!(tmux.tmux(&["has-session", "-t", "=t1"]).status.success(), "la sesión sigue viva");
}

#[tokio::test(flavor = "current_thread")]
async fn resize_message_resizes_the_pty() {
    let Some(tmux) = PrivateTmux::start(&["t1"]) else { return };
    let (server, mut client) = ws_pair().await;
    let (_stop, shutdown) = tokio::sync::watch::channel(false);
    tokio::spawn(run_bridge(server, Dialect::Tty, Init { cols: 80, rows: 24, session: Some("t1".into()) },
        Source::Pty { target: tmux.target(), session: Some("t1".into()) }, BridgeLimits::default(), shutdown));
    tokio::time::sleep(Duration::from_millis(300)).await;
    client.send(Message::text(r#"1{"columns":120,"rows":40}"#)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let size = String::from_utf8_lossy(&tmux.tmux(&["list-clients", "-F", "#{client_width}x#{client_height}"]).stdout).trim().to_string();
    assert_eq!(size, "120x40");
}

#[tokio::test(flavor = "current_thread")]
async fn slow_reader_never_buffers_more_than_the_outbox() {
    let Some(tmux) = PrivateTmux::start(&["t1"]) else { return };
    let (server, client) = ws_pair().await; // el cliente no lee
    let (_stop, shutdown) = tokio::sync::watch::channel(false);
    let limits = BridgeLimits { outbox_bytes: 64 * 1024, read_chunk: 16 * 1024 };
    let stats = comandos_server::dash::term::bridge::stats();
    tokio::spawn(run_bridge(server, Dialect::Tty, Init { cols: 80, rows: 24, session: Some("t1".into()) },
        Source::Pty { target: tmux.target(), session: Some("t1".into()) }, limits, shutdown));
    tmux.tmux(&["send-keys", "-t", "=t1", "seq 1 300000", "Enter"]);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(stats.max_outbox_bytes() <= 64 * 1024 + 16 * 1024, "{}", stats.max_outbox_bytes());
    drop(client);
}

#[tokio::test(flavor = "current_thread")]
async fn missing_session_shows_message_and_picker_without_attach() {
    let Some(tmux) = PrivateTmux::start(&["t1", "local"]) else { return };
    let (server, mut client) = ws_pair().await;
    let (_stop, shutdown) = tokio::sync::watch::channel(false);
    tokio::spawn(run_bridge(server, Dialect::Tty, Init { cols: 80, rows: 24, session: Some("nope".into()) },
        Source::Pty { target: tmux.target(), session: Some("nope".into()) }, BridgeLimits::default(), shutdown));
    let mut text = Vec::new();
    while let Ok(Some(Ok(m))) = tokio::time::timeout(Duration::from_secs(2), client.next()).await {
        text.extend_from_slice(&m.into_data());
        if String::from_utf8_lossy(&text).contains("Numero") { break; }
    }
    let s = String::from_utf8_lossy(&text);
    assert!(s.contains("La sesion 'nope' ya no existe."), "{s}");
    assert!(s.contains("1) t1") && !s.contains(") local"), "{s}");
}

#[tokio::test(flavor = "current_thread")]
async fn replay_source_streams_a_recording_and_never_spawns() {
    let dir = tempfile_dir();
    let rec = dir.join("r.bin");
    std::fs::write(&rec, b"\x1b[31mrojo\x1b[0m\r\n").unwrap();
    let (server, mut client) = ws_pair().await;
    let (_stop, shutdown) = tokio::sync::watch::channel(false);
    tokio::spawn(run_bridge(server, Dialect::V1, Init { cols: 80, rows: 24, session: None },
        Source::Replay(rec), BridgeLimits::default(), shutdown));
    let mut all = Vec::new();
    while let Ok(Some(Ok(m))) = tokio::time::timeout(Duration::from_millis(500), client.next()).await { all.push(m.into_data()); }
    assert!(all.iter().any(|f| f.as_ref() == output(b"\x1b[31mrojo\x1b[0m\r\n").as_slice()));
}

fn tempfile_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("comandos-replay-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}
```

`support::ws_pair()` abre un `TcpListener` en `127.0.0.1:0`, acepta con `tokio_tungstenite::accept_async` y conecta con `connect_async`; devuelve `(servidor, cliente)`.

- [ ] **Step 3:** `$C test -p comandos-server --test term_bridge` → FAIL.
- [ ] **Step 4: `attach.rs`**

```rust
// crates/comandos-server/src/dash/term/attach.rs
//! Lo único que el puente pide a tmux. Enum cerrado: añadir una variante que
//! mate, cree o reconfigure algo rompe `live_commands_are_attach_or_has_session_only`.
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxTarget { User, Private(PathBuf) }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachCommand { Version, HasSession(String), ListSessionNames, Attach { session: String, active_pane: bool } }

impl AttachCommand {
    pub fn args(&self) -> Vec<String> {
        match self {
            AttachCommand::Version => vec!["-V".into()],
            AttachCommand::HasSession(s) => vec!["has-session".into(), "-t".into(), format!("={s}")],
            AttachCommand::ListSessionNames => vec!["list-sessions".into(), "-F".into(), "#{session_name}".into()],
            AttachCommand::Attach { session, active_pane } => {
                let mut v = vec!["attach".to_string()];
                if *active_pane { v.extend(["-f".into(), "active-pane".into()]); }
                v.extend(["-t".into(), format!("={session}")]);
                v
            }
        }
    }
}

impl TmuxTarget {
    /// Prefijo `-S <socket>` solo para servidores privados (pruebas, sombra).
    pub fn prefix(&self) -> Vec<String> {
        match self {
            TmuxTarget::User => Vec::new(),
            TmuxTarget::Private(dir) => vec!["-S".into(), crate::dash::native::tmux::private_socket(dir).to_string_lossy().into_owned()],
        }
    }
}

pub fn valid_session(name: &str) -> bool {
    (1..=80).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// `tmux -V` ≥ 3.2 → `-f active-pane` (un teléfono que elige split no mueve el
/// panel activo del escritorio).
pub fn supports_active_pane(version_stdout: &str) -> bool {
    let digits: Vec<u32> = version_stdout.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty()).take(2).filter_map(|s| s.parse().ok()).collect();
    matches!(digits.as_slice(), [major, minor] if *major > 3 || (*major == 3 && *minor >= 2))
}
```

- [ ] **Step 5: `bridge.rs`** — diseño (código completo; aquí el núcleo):

```rust
// crates/comandos-server/src/dash/term/bridge.rs (núcleo)
//! Un PTY por WebSocket. El PTY nace con el tamaño del cliente antes del
//! attach (la sesión no se encoge). Contrapresión: si la cola hacia el socket
//! supera `outbox_bytes`, se deja de leer del PTY hasta que baje (tmux
//! retiene); `'2'`/`'3'` del cliente pausan/reanudan la lectura (ttyd).
use super::attach::{AttachCommand, TmuxTarget, supports_active_pane, valid_session};
use comandos_term::proto::{self, ClientMsg, Dialect, Init};
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use std::{path::PathBuf, sync::atomic::{AtomicUsize, Ordering}};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, sync::watch};
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

pub struct BridgeLimits { pub outbox_bytes: usize, pub read_chunk: usize }
impl Default for BridgeLimits { fn default() -> Self { Self { outbox_bytes: 1 << 20, read_chunk: 64 << 10 } } }

pub enum Source { Pty { target: TmuxTarget, session: Option<String> }, Replay(PathBuf) }

static MAX_OUTBOX: AtomicUsize = AtomicUsize::new(0);
pub struct Stats;
impl Stats { pub fn max_outbox_bytes(&self) -> usize { MAX_OUTBOX.load(Ordering::Relaxed) } }
pub fn stats() -> Stats { MAX_OUTBOX.store(0, Ordering::Relaxed); Stats }

fn pty_command(target: &TmuxTarget, cmd: &AttachCommand, size: (u16, u16)) -> pty_process::Command {
    let mut c = pty_process::Command::new("tmux");
    c.args(target.prefix()).args(cmd.args())
        .env_remove("TMUX").env_remove("TMUX_PANE")
        .env("TERM", "xterm-256color").env("COLORTERM", "truecolor");
    let _ = size; // el tamaño se fija en el PTY antes de `spawn`
    c
}
```

Flujo de `run_bridge` (implementar exactamente así):
1. `Source::Replay(path)`: lee el archivo en bloques de `read_chunk`, envía `proto::output(bloque)` por bloque respetando la cola, y después espera al cierre del socket o a `shutdown`; descarta la entrada del cliente. Nunca lanza procesos.
2. `Source::Pty`: si `session` es `Some(s)` con `valid_session(s)` y `tmux <prefix> has-session -t =s` (vía `tokio::process::Command`, plazo 2 s, `TMUX`/`TMUX_PANE` quitados) sale con 0 → `cmd = Attach { session: s, active_pane: supports_active_pane(<stdout de tmux -V>) }`. Si no: se envía `proto::output("La sesion '<s>' ya no existe.\r\n\r\n")` cuando había nombre válido, y se entra en el **selector**: `ListSessionNames`, filtrar `local|hub|control`, si no hay ninguna `"No hay sesiones. Crea una:  ccx nombre\r\n\r\n"` y PTY con `$SHELL` (`/bin/sh` si no hay); si hay, se escribe «Sesiones de ComandOS:\r\n» + `"  %2d) %s\r\n"` por sesión + «\r\nNumero (o Enter para shell libre): » y se leen dígitos de la entrada del cliente con eco local hasta `\r`; número válido → `Attach` de esa; si no, `$SHELL`. (Textos literales de `bin/cc-webterm-attach`.)
3. `pty_process::open()` → `pty.resize(Size::new(init.rows, init.cols))` → `spawn` del comando con `pts`. Bucle `select!`: lectura del PTY (`read_chunk`, solo si `queued < outbox_bytes` y no pausado) → `sink.feed(Message::binary(proto::output(..)))`, `queued += n`, `MAX_OUTBOX = max(MAX_OUTBOX, queued)`; `sink.flush()` completado → `queued = 0`; mensaje del cliente → `parse_client`: `Input` → escribir en el PTY, `Resize` → `pty.resize`, `Pause/Resume` → bandera; error de protocolo → cerrar con código 1003; cierre del socket, `shutdown` o fin del hijo → salir.
4. Al salir: soltar el PTY (el cliente `tmux attach` recibe SIGHUP y se desengancha; la sesión sigue), `child.kill()` si aún vive y `child.wait()` con plazo de 2 s. **Nunca** se manda otro comando a tmux.
5. Al abrir, antes del primer `output`, se envían `proto::title(tty_title("cc-webterm-attach", <hostname>))` y `proto::prefs(TTY_PREFS)` (D5): xterm.js aplica tamaño y familia, como con ttyd.

- [ ] **Step 6:** `$C test -p comandos-server --test term_bridge` → PASS (las pruebas con tmux se omiten con aviso si no hay tmux; en esta máquina corren).
- [ ] **Step 7: Commit**

```bash
git add crates/comandos-server/src/dash/term crates/comandos-server/src/dash/mod.rs crates/comandos-server/Cargo.toml crates/comandos-server/tests/term_bridge.rs crates/comandos-server/tests/support Cargo.lock
git commit -m "feat(dash): puente PTY de la terminal con comandos tmux cerrados, contrapresión y fuente de grabaciones

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task A4: Paso 1 — `/term/*` en el frente con el protocolo de ttyd, oyente de compatibilidad 4780 y `comandos webterm`

**Depende de:** T3, A3.

**Files:**
- Create: `crates/comandos-server/src/dash/term/compat.rs`, `crates/comandos-cli/src/webterm.rs`, `crates/comandos-cli/src/webterm_attach.rs`, `docs/verification/cutover-term.md`
- Modify: `crates/comandos-server/src/dash/term/mod.rs`, `crates/comandos-server/src/dash/mod.rs` (`DashConfig.term: TermMode`, `.webterm_compat: Vec<u16>`, `.shadow_readonly: bool`, `.term_replay_dir: Option<PathBuf>`; `build` pasa `websocket: Some(term::ws_route(..))`), `crates/comandos-server/src/dash/router.rs` (`/term` antes de `classify`), `crates/comandos-cli/src/dispatch.rs` (alias `cc-webterm` → `webterm`, `cc-webterm-attach` → `webterm-attach`)
- Test: `crates/comandos-server/tests/term_routes.rs`, `crates/comandos-server/tests/term_compat.rs`, `crates/comandos-cli/tests/webterm.rs`

**Interfaces:**
- Consumes: `run_bridge`, `Source`, `TmuxTarget` (A3); `WsRoute`, `WsRequest` (T3); `access::token_matches`, `token::load_token`.
- Produces:
  - `pub enum TermMode { Off, Ttyd, Native }` (`COMANDOS_DASH_TERM=off|ttyd|native`, por omisión `off` hasta el cutover; `--term <modo>`).
  - Rutas (con `TermMode::Ttyd` o `Native`): `GET /term/` y `/term/?…` → página de terminal (`Ttyd`: `dash/term.html` del disco, como `ttyd -I`; `Native`: la página de A9, A11); `GET /term/token` → `{"token":""}` (`application/json`, igual que ttyd sin `-c`); `GET /term/ws` (upgrade, subprotocolos `tty` y, con `Native`, `comandos.term.v1`). Con `Off`, `/term*` se comporta como hoy (reenvío al Python, que no la conoce → su 404).
  - Autenticación: dialecto `tty` → primer `arg` recortado comparado con `H/dash-token` recortado con `token_matches` (cierre con mensaje «Acceso denegado al terminal de ComandOS.\r\n» como trama `'0'` y código 1008); `v1` → la puerta del tablero ya pasó (la ruta no es pública).
  - `pub async fn serve_compat(port: u16, profile: CompatProfile, state: Arc<DashState>, shutdown)`; `pub enum CompatProfile { Path, Plain }` — `Path` (4780): solo `/term/`, `/term/token`, `/term/ws`, todo lo demás 404 (paridad con `ttyd -b /term`); `Plain` (4779, A11): `/`, `/token`, `/ws`.
  - Flags: `--webterm-compat 4780[,4779]` (`COMANDOS_DASH_WEBTERM_COMPAT`); `--shadow-readonly`; `--term-replay-dir DIR` (con `shadow_readonly`, `/term/ws` usa `Source::Replay(DIR/<sesión>.bin)` o `DIR/default.bin`).
  - `comandos webterm [on|off|status]` (port de `bin/cc-webterm`): consulta `GET http://127.0.0.1:4777/term/token` con plazo 0,4 s; si responde 200 con `{"token":""}` y la cabecera `X-Comandos-Term: <modo>`, no lanza el ttyd de 4780 (ni el de 4779 si el modo es `native` y el oyente 4779 está activo); lanza el resto con `systemd-run --user --collect --unit=cc-webterm …` con los mismos argumentos de hoy; crea/borra `H/webterm-enabled` (0600) como hoy. `comandos webterm-attach <token> <sesión>` (port de `bin/cc-webterm-attach`, mismos textos y códigos de salida, `exec` de tmux con `std::os::unix::process::CommandExt::exec`).

- [ ] **Step 1: Prueba que falla — rutas.** `term_routes.rs` levanta el frente de prueba (soporte existente `support::front`) con `TermMode::Ttyd`, un HOME temporal con `dash-token` = `t0k` y `TmuxTarget::Private` de un `PrivateTmux` con sesión `t1` (el frente de prueba acepta `term_target` en `NativeOptions`; producción usa `User`). Casos:
  - `GET /term/token` → 200 `{"token":""}`, cabecera `X-Comandos-Term: ttyd`.
  - `GET /term/?auth=t0k&arg=t1` → 200 `text/html` con los bytes de `dash/term.html` del `dash_dir` de prueba.
  - WS `/term/ws?arg=t0k&arg=t1` con subprotocolo `tty` y Origin del frente: tras el init recibe `'1'…`, `'2'+TTY_PREFS` y salida; `0echo ruta-ok\r` aparece en `capture("t1")`.
  - WS con `arg=malo` → primera trama `0Acceso denegado al terminal de ComandOS.\r\n` y cierre; `PrivateTmux::clients("t1") == 0`.
  - WS desde Origin ajeno → 403 (puerta).
  - Con `TermMode::Off`, `GET /term/token` se reenvía (el heredado de prueba responde su 404).
- [ ] **Step 2: Prueba que falla — compatibilidad.** `term_compat.rs`: `serve_compat` en un puerto 73xx con `Path`: `/term/token` 200, `/term/ws` funciona con `tty`, `/assets/x.js` y `/` → 404 (como `ttyd -b /term`); una petición con `Host: <host tailscale de la lista permitida>` pasa la puerta igual que en 4777. Prueba `compat_holds_port_so_ttyd_cannot_double_bind`: con el oyente activo, enlazar el mismo puerto falla con `AddrInUse` (documenta por qué `comandos webterm` no debe lanzar ttyd en ese puerto).
- [ ] **Step 3: Prueba que falla — CLI.** `crates/comandos-cli/tests/webterm.rs` con un `Runner` inyectado (registra los comandos en vez de ejecutarlos; nunca llama a `systemd-run` ni `tmux` reales):
  - `webterm on` con frente en modo `ttyd` (servidor de prueba que responde `/term/token` con la cabecera) → solo un `systemd-run --unit=cc-webterm … ttyd -p 4779 …` con los `-t` literales de `bin/cc-webterm`; ningún `cc-webterm-path`.
  - con frente `native` y 4779 atendido → ningún `systemd-run`; `webterm-enabled` creado con 0600.
  - con frente apagado → los dos `systemd-run` de hoy (paridad con el bash).
  - `webterm off` → `systemctl --user stop cc-webterm.service cc-webterm-path.service` y borra `webterm-enabled`.
  - `webterm-attach` con token incorrecto → stdout «Acceso denegado al terminal de ComandOS.», código 1; con sesión válida → el `Runner` recibe `exec tmux attach [-f active-pane] -t =t1` (sin `-S`: es producción).
- [ ] **Step 4:** `$C test -p comandos-server --test term_routes --test term_compat && $C test -p comandos-cli --test webterm` → FAIL.
- [ ] **Step 5: Implementación.** `term::ws_route(state)` construye el `WsRoute` de T3: `accepts("/term/ws")` → `Some(&["tty"])` en `Ttyd`, `Some(&["comandos.term.v1", "tty"])` en `Native`, `None` en `Off`. El manejador lee el init (plazo 10 s, si no cierra 1002), resuelve la sesión (`tty`: segundo `arg`; `v1`: `init.session`), elige `Source` (`shadow_readonly` → `Replay`; si no → `Pty { target: state.term_target.clone(), session }`) y llama a `run_bridge`. `router::classify_with` comprueba primero `term::route(method, path)` (antes que `native::route`) cuando `TermMode != Off`. `serve_compat` reutiliza `comandos_server::serve` con un `Config` cuyo manejador HTTP solo atiende las rutas del perfil y cuyo `websocket` es el mismo `ws_route` con la ruta reescrita (`/ws` → `/term/ws` en `Plain`). El ensamblado arranca los oyentes de compatibilidad dentro de `serve_with` con el mismo `shutdown`. CLI: `webterm.rs` y `webterm_attach.rs` con un rasgo `Runner { fn run(&self, program: &str, args: &[String]) -> io::Result<Output>; fn exec(&self, program: &str, args: &[String]) -> io::Error }` y la implementación real `SystemRunner`.
- [ ] **Step 6:** Pruebas de los Steps 1–3 → PASS; `$C test -p comandos-server -p comandos-cli` → PASS.
- [ ] **Step 7: Procedimiento de cutover del paso 1** en `docs/verification/cutover-term.md` (lo ejecuta el controlador):

````markdown
# Cutover de la terminal web

## Paso 1 — el puente Rust sustituye a ttyd en 4780 (xterm.js no cambia)

### 0. Previos
```sh
systemctl --user is-active cc-dash.service cc-dash-legacy.service cc-webterm.service cc-webterm-path.service
~/.local/share/comandos/bin/comandos install --releases          # anotar la actual
tailscale serve status                                           # debe seguir igual al final
```
### 1. Sombra (nunca contra sesiones reales)
```sh
cargo run -p xtask -- term-record --suite base --out ~/.local/share/comandos/term-replays   # A12, tmux privado
~/.local/share/comandos/bin/comandos dash 4782 --no-open --shadow-readonly --no-usage-effects \
  --term ttyd --term-replay-dir ~/.local/share/comandos/term-replays &
cc-browser-expose start 4782
cargo run -p xtask -- shots pair --base http://127.0.0.1:4782 --suite term-xterm --out docs/verification/fase3/term-paso1
```
`term-xterm` compara, en el mismo navegador del Mac, la página servida por un ttyd de prueba (tmux privado) y la servida por el frente en sombra, con las mismas grabaciones: debe quedar en 0 recortes fuera de tolerancia.
### 2. Activar
```sh
NEW=<ruta al binario nuevo>; "$NEW" install --stage
~/.local/share/comandos/bin/comandos install --link cc-webterm
~/.local/share/comandos/bin/comandos install --link cc-webterm-attach
mkdir -p ~/.config/systemd/user/cc-dash.service.d
printf '[Service]\nEnvironment=COMANDOS_DASH_TERM=ttyd\nEnvironment=COMANDOS_DASH_WEBTERM_COMPAT=4780\n' > ~/.config/systemd/user/cc-dash.service.d/term.conf
systemctl --user stop cc-webterm-path.service
systemctl --user daemon-reload && systemctl --user restart cc-dash.service
```
`systemctl --user restart cc-dash.service` reinicia solo el frente: tmux, `cc-app` y las sesiones no se tocan; las terminales remotas abiertas se reconectan solas (backoff de `term.html`).
### 3. Verificar
- Teléfono (tailnet): abrir una pestaña de terminal; teclear, Backspace de GBoard, rotar, historial, «Paneles».
- `curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:4780/term/token` → 200; `ss -ltnp | grep 4780` → el proceso es `comandos`.
- Pss del frente con 4 terminales remotas abiertas (`grep Pss /proc/<pid>/smaps_rollup`) ≤ antes + 4 × 256 KiB.
### 4. Revertir
```sh
rm ~/.config/systemd/user/cc-dash.service.d/term.conf && systemctl --user daemon-reload && systemctl --user restart cc-dash.service
~/.local/share/comandos/bin/comandos install --rollback cc-webterm
~/.local/share/comandos/bin/comandos install --rollback cc-webterm-attach
~/.local/bin/cc-webterm                                      # vuelve a levantar los dos ttyd
```
````

- [ ] **Step 8: Commit**

```bash
git add crates/comandos-server/src/dash crates/comandos-server/tests/term_routes.rs crates/comandos-server/tests/term_compat.rs crates/comandos-cli/src crates/comandos-cli/tests/webterm.rs docs/verification/cutover-term.md
git commit -m "feat(dash): /term con el protocolo de ttyd, oyente de compatibilidad 4780 y comandos webterm en Rust

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task A5: Render puro — tiras de celdas, colores, cursor y glifos de dibujo

**Depende de:** A1. **Independiente de** A3/A4.

**Files:**
- Create: `crates/comandos-term/src/render.rs`, `crates/comandos-term/src/glyphs.rs`
- Test: `crates/comandos-term/tests/render.rs`

**Interfaces:**
- Consumes: `Engine`, `Palette` (A1).
- Produces:
  - `pub struct Style { pub fg: [u8; 3], pub bg: [u8; 3], pub bold: bool, pub italic: bool, pub dim: bool, pub underline: Underline, pub strike: bool, pub hidden: bool }`; `pub enum Underline { None, Single, Double, Curly, Dotted, Dashed }`.
  - `pub enum RunKind { Text, Wide, Box(char) }`.
  - `pub struct Run { pub col: u16, pub cells: u16, pub text: String, pub style: Style, pub kind: RunKind, pub link: Option<u32> }` (`link`: índice en la tabla de hipervínculos de la fila).
  - `pub struct RowRender { pub line: usize, pub bg_runs: Vec<(u16, u16, [u8; 3])>, pub runs: Vec<Run>, pub links: Vec<String> }`.
  - `pub struct RenderOpts { pub bold_is_bright: bool, pub min_contrast: f32 }` (`bold_is_bright = true` como xterm.js 5.5 `drawBoldTextInBrightColors`; `min_contrast = 1.0`, el valor por omisión de xterm.js).
  - `pub fn render_row(engine: &Engine, line: usize, palette: &Palette, opts: &RenderOpts) -> RowRender`.
  - `pub struct CursorView { pub line: usize, pub col: u16, pub shape: CursorShape, pub visible: bool, pub wide: bool }`; `pub fn cursor(engine: &Engine) -> CursorView`.
  - `glyphs::BoxOp { HLine { y: f32, x0: f32, x1: f32, w: f32 }, VLine { x: f32, y0: f32, y1: f32, w: f32 }, Rect { x: f32, y: f32, w: f32, h: f32, alpha: f32 }, Tri { pts: [(f32, f32); 3] } }` en unidades de celda (0..1); `glyphs::box_ops(c: char) -> Option<&'static [BoxOp]>` para U+2500–U+259F y U+E0B0–U+E0B3 (los que xterm.js dibuja con `customGlyphs: true`).

- [ ] **Step 1: Prueba que falla**

```rust
// crates/comandos-term/tests/render.rs
use comandos_term::{engine::{Engine, GridSize, Palette}, glyphs::{BoxOp, box_ops}, render::*};

fn eng() -> (Engine, Palette) {
    let p = Palette::xterm_default([0xEA, 0xF0, 0xFB], [0x0A, 0x0D, 0x13], [0xFF, 0xAE, 0x1A], [0x0A, 0x0D, 0x13], [0x2E, 0x38, 0x52]);
    (Engine::new(GridSize { cols: 20, rows: 4 }, 100, p.clone()), p)
}
const O: RenderOpts = RenderOpts { bold_is_bright: true, min_contrast: 1.0 };

#[test]
fn same_style_ascii_is_one_run() {
    let (mut e, p) = eng();
    e.advance(b"abc\x1b[31mde", 0.0);
    let r = render_row(&e, 0, &p, &O);
    assert_eq!(r.runs.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), ["abc", "de"]);
    assert_eq!(r.runs[1].style.fg, p.ansi[1]);
}

#[test]
fn bold_named_color_becomes_bright() {
    let (mut e, p) = eng();
    e.advance(b"\x1b[1;34mX", 0.0);
    assert_eq!(render_row(&e, 0, &p, &O).runs[0].style.fg, p.ansi[12]);
}

#[test]
fn inverse_swaps_and_default_bg_is_omitted() {
    let (mut e, p) = eng();
    e.advance(b"a\x1b[7mb", 0.0);
    let r = render_row(&e, 0, &p, &O);
    assert_eq!(r.runs[1].style.fg, p.bg);
    assert_eq!(r.bg_runs, vec![(1, 1, p.fg)], "solo se pinta el fondo que no es el de la terminal");
}

#[test]
fn wide_glyphs_are_their_own_two_cell_run() {
    let (mut e, p) = eng();
    e.advance("a漢b".as_bytes(), 0.0);
    let r = render_row(&e, 0, &p, &O);
    let wide = r.runs.iter().find(|x| x.text == "漢").unwrap();
    assert_eq!((wide.col, wide.cells, &wide.kind), (1, 2, &RunKind::Wide));
}

#[test]
fn box_drawing_is_drawn_not_typed() {
    let (mut e, p) = eng();
    e.advance("─│█".as_bytes(), 0.0);
    let r = render_row(&e, 0, &p, &O);
    assert!(r.runs.iter().all(|x| matches!(x.kind, RunKind::Box(_))));
    assert!(matches!(box_ops('─').unwrap(), [BoxOp::HLine { y, .. }] if (*y - 0.5).abs() < 1e-6));
    assert!(matches!(box_ops('█').unwrap(), [BoxOp::Rect { w, h, .. }] if *w == 1.0 && *h == 1.0));
    assert!(box_ops('a').is_none());
}

#[test]
fn osc8_links_are_attached_to_runs() {
    let (mut e, p) = eng();
    e.advance(b"\x1b]8;;https://ej.mx\x1b\\ver\x1b]8;;\x1b\\ fin", 0.0);
    let r = render_row(&e, 0, &p, &O);
    let link = r.runs.iter().find(|x| x.text == "ver").unwrap().link.unwrap();
    assert_eq!(r.links[link as usize], "https://ej.mx");
}

#[test]
fn cursor_reports_shape_and_position() {
    let (mut e, _) = eng();
    e.advance(b"ab\x1b[5 q", 0.0);
    let c = cursor(&e);
    assert_eq!((c.line, c.col, c.visible), (0, 2, true));
    assert_eq!(c.shape, CursorShape::Beam);
}
```

- [ ] **Step 2:** `$C test -p comandos-term --test render` → FAIL.
- [ ] **Step 3: Implementación.** `render_row` recorre `term.grid()[Line(line - display_offset)]` celda a celda: salta `WIDE_CHAR_SPACER`; resuelve `fg`/`bg` (`Color::Named` → `palette` con `Foreground/Background/Cursor`, `Indexed(i)` → `ansi[i]`, `Spec(rgb)`), aplica `bold_is_bright` solo a índices 0–7 (named o indexed), `dim` como xterm.js (mezcla 50 % hacia el fondo), `INVERSE` intercambia, `HIDDEN` deja `fg = bg`; agrupa en la misma `Run` mientras coinciden estilo, enlace y tipo `Text` y el carácter es ASCII imprimible o latino de ancho 1; `WIDE_CHAR` y cualquier carácter con `zerowidth()` empiezan una `Run` propia; `box_ops(c).is_some()` → `RunKind::Box(c)` de una celda. `bg_runs` agrupa celdas consecutivas con el mismo fondo distinto de `palette.bg`. `glyphs.rs` es una tabla `match` de `&'static [BoxOp]` (líneas finas `w = 1/12` de celda y gruesas `w = 1/6`, como el `customGlyphs` de xterm.js 5.5; bloques U+2580–U+259F como `Rect` con `alpha` 0.25/0.5/0.75 para los sombreados; powerline E0B0–E0B3 como `Tri`). `CursorShape` es el de `alacritty_terminal::vte::ansi::CursorShape` (`Block`, `Underline`, `Beam`, `HollowBlock`, `Hidden`) reexportado.
- [ ] **Step 4:** `$C test -p comandos-term --test render` → PASS; `$C check -p comandos-term --target wasm32-unknown-unknown`.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-term/src/render.rs crates/comandos-term/src/glyphs.rs crates/comandos-term/src/lib.rs crates/comandos-term/tests/render.rs
git commit -m "feat(term): render puro a tiras de celdas, colores de xterm.js, cursor y glifos de dibujo

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task A6: Renderizador canvas 2D por daños (`comandos-term-web`)

**Depende de:** A5, T4. **Paralela a** A7.

**Files:**
- Create: `crates/comandos-term-web/{Cargo.toml,src/lib.rs,src/metrics.rs,src/canvas.rs,src/paint.rs}`
- Modify: `Cargo.toml` (miembro)
- Test: `crates/comandos-term-web/src/metrics.rs` (pruebas unitarias en host de la aritmética), `crates/comandos-term-web/tests/web.rs` (`wasm-bindgen-test`, corre en el Chrome del Mac por `xtask term-bench --wasm-tests`, A12)

**Interfaces:**
- Consumes: `render_row`, `cursor`, `box_ops`, `Engine` (A1, A5).
- Produces:
  - `metrics::CellMetrics { pub css_w: f64, pub css_h: f64, pub dev_w: u32, pub dev_h: u32, pub dev_char_h: u32, pub char_top: u32, pub dpr: f64 }`; `metrics::from_measure(char_w_css: f64, char_h_css: f64, dpr: f64, line_height: f64, letter_spacing: f64) -> CellMetrics` con la aritmética de `addon-canvas` 0.7 (`dev char w = floor(w·dpr)`, `dev char h = ceil(h·dpr)`, `dev cell h = floor(dev char h · lineHeight)`, `char top = lineHeight == 1 ? 0 : round((cell h − char h)/2)`, `dev cell w = dev char w + round(letterSpacing)`, CSS = dev/dpr).
  - `metrics::fit(parent_w: f64, parent_h: f64, scrollbar_w: f64, m: &CellMetrics) -> (u16, u16)` (FitAddon 0.10: `cols = max(2, floor((w − scrollbar)/css_w))`, `rows = max(1, floor(h/css_h))`).
  - `trait Painter { fn clear_rows(&mut self, rows: &[usize]); fn paint_row(&mut self, row: &RowRender, m: &CellMetrics); fn paint_cursor(&mut self, c: &CursorView, under: Option<&Run>, m: &CellMetrics); fn resize(&mut self, cols: u16, rows: u16, m: &CellMetrics); }` y `canvas::Canvas2d` que lo implementa (un `<canvas>` por terminal, almacén de respaldo escalado por `devicePixelRatio`).
  - `paint::Scheduler`: acumula daños y pinta en `requestAnimationFrame` (uno en vuelo como máximo); sin daños no pide cuadro (presupuesto de CPU en reposo).
  - `#[wasm_bindgen] pub struct WebTerm` (la usa A9): `WebTerm::new(host: HtmlElement, opts: JsValue) -> Result<WebTerm, JsValue>`, `write(&mut self, bytes: &[u8])`, `resize_to_fit(&mut self) -> JsValue` (`{cols, rows}`), `set_theme(&mut self, theme: JsValue)`, `set_font(&mut self, family: &str, size: f64)`, `take_replies(&mut self) -> Vec<u8>`, `dimensions(&self) -> JsValue`.

- [ ] **Step 1: Prueba que falla (host)**

```rust
// en crates/comandos-term-web/src/metrics.rs
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_addon_canvas_arithmetic_at_dpr_1_and_2() {
        // Ubuntu Sans Mono 11 px: char 6.6 × 13 css (medidas de ejemplo).
        let m1 = from_measure(6.6, 13.0, 1.0, 1.2, 0.0);
        assert_eq!((m1.dev_w, m1.dev_char_h, m1.dev_h, m1.char_top), (6, 13, 15, 1));
        let m2 = from_measure(6.6, 13.0, 2.0, 1.2, 0.0);
        assert_eq!((m2.dev_w, m2.dev_char_h, m2.dev_h), (13, 26, 31));
        assert!((m2.css_h - 15.5).abs() < 1e-9);
    }
    #[test]
    fn fit_mirrors_fitaddon() {
        let m = from_measure(6.6, 13.0, 1.0, 1.2, 0.0);
        assert_eq!(fit(800.0, 450.0, 10.0, &m), (131, 30));
        assert_eq!(fit(5.0, 5.0, 0.0, &m), (2, 1));
    }
}
```

- [ ] **Step 2:** `$C test -p comandos-term-web --lib` → FAIL.
- [ ] **Step 3: Implementación.** `metrics.rs` con la aritmética de arriba (`f64::floor/ceil/round` convertidos con `as u32` tras `max(0.0)`, sin pánico). Medición en el navegador (`metrics::measure(document, family, size) -> Result<(f64, f64), JsValue>`): `await document.fonts().load("<size>px <family>")` y luego un `<span>` oculto dentro de un contenedor `.xterm` con `"W".repeat(32)` y la fuente aplicada, `getBoundingClientRect` → `w/32`, `h` (estrategia DOM de xterm.js 5.5). `scrollbar_w`: un `div.xterm-viewport` oculto con `overflow-y: scroll` dentro del mismo `.xterm` (`offsetWidth − clientWidth`), de modo que el CSS de `term.html` que estiliza la barra aplica igual que con xterm.js. `Canvas2d::paint_row`: limpia la franja de la fila con el fondo de la terminal, pinta `bg_runs` como `fillRect` en píxeles de dispositivo, después cada `Run`: `Text` → un `fillText` por tira con `font = "<italic> <bold> <dev size>px <family>"` en la base `char_top + ascent`; `Wide` → `fillText` centrado en 2 celdas; `Box` → `box_ops` a `fillRect`/`moveTo…fill` en enteros de dispositivo (las líneas verticales se unen entre filas); subrayados y tachado con `fillRect` de 1 px de dispositivo (curvo: `quadraticCurveTo` por celda). Cursor: `Block` → rectángulo `cursor` y el glifo debajo en `cursor_accent`; `Underline`/`Beam` → barra de `max(1, dpr)` px; sin foco → `HollowBlock`. Parpadeo: temporizador de 600 ms que repinta solo la fila del cursor, detenido sin foco o con `cursorBlink: false`.
- [ ] **Step 4:** `$C test -p comandos-term-web --lib` → PASS; `cargo run -p xtask -- web-build --crate comandos-term-web --check-budget` → ≤ 250 KiB gzip.
- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/comandos-term-web
git commit -m "feat(term): renderizador canvas 2D por daños con la aritmética de celdas de xterm.js

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task A7: Entrada — teclado, IME, GBoard, pegado, ratón y rueda

**Depende de:** A1 (host); la parte web, de A6. **Paralela a** A6 en su parte de host.

**Files:**
- Create: `crates/comandos-term/src/input.rs`, `crates/comandos-term-web/src/keyboard.rs`, `crates/comandos-term-web/src/mouse.rs`
- Test: `crates/comandos-term/tests/input.rs`

**Interfaces:**
- Consumes: `Modes`, `MouseMode` (A1).
- Produces:
  - `pub struct KeyInput<'a> { pub key: &'a str, pub code: &'a str, pub ctrl: bool, pub alt: bool, pub shift: bool, pub meta: bool }`.
  - `pub enum KeyAction { Send(Vec<u8>), ScrollPage(i32), None }`; `pub fn encode_key(k: &KeyInput, m: &Modes) -> KeyAction` (tabla de `Keyboard.evaluateKeyboardEvent` de xterm.js 5.5 para Linux).
  - `pub fn encode_paste(text: &str, m: &Modes) -> Vec<u8>` (`\r?\n` → `\r`; con `bracketed_paste`, `ESC[200~ … ESC[201~` y sin `ESC[201~` interno).
  - `pub enum Button { Left, Middle, Right, WheelUp, WheelDown, None }`, `pub enum MouseKind { Press, Release, Move }`; `pub fn encode_mouse(b: Button, kind: MouseKind, col: u16, row: u16, mods: (bool, bool, bool), m: &Modes) -> Option<Vec<u8>>` (SGR con `sgr_mouse`; X10 si no, con `col, row ≤ 223`).
  - `pub enum WheelAction { Mouse(Vec<u8>), Arrows(Vec<u8>), Scroll(i32) }`; `pub fn wheel(lines: i32, col: u16, row: u16, m: &Modes) -> WheelAction` (con ratón activo → SGR a tmux; `alt_screen && alternate_scroll` → flechas `ESC[A`/`ESC[B` × líneas (`ESCOA` con `app_cursor`); si no → desplazamiento local).
  - `pub fn focus(on: bool, m: &Modes) -> Option<&'static [u8]>` (`ESC[I`/`ESC[O` con `focus_events`).
  - `pub fn beforeinput(input_type: &str, data: Option<&str>) -> Option<Vec<u8>>` (`deleteContentBackward` → `0x7f`, `insertLineBreak`/`insertParagraph` → `\r`, `insertText` → datos; el resto `None`).
  - `pub struct Ime { composing: bool, last_commit: Option<String> }` (estado puro del IME, `Default`): `start(&mut self)`, `end(&mut self, data: &str) -> Option<Vec<u8>>` (entrega el texto compuesto), `input(&mut self, input_type: &str, data: Option<&str>, is_composing: bool) -> Option<Vec<u8>>` (ignora todo lo que llega con `is_composing` y el `insertText` que repite el último texto comprometido justo después de `compositionend`; el resto delega en `beforeinput`), `keydown_ignored(&self, key_code: u32) -> bool` (`true` con 229 o componiendo).

- [ ] **Step 1: Prueba que falla**

```rust
// crates/comandos-term/tests/input.rs
use comandos_term::{engine::{Modes, MouseMode}, input::*};

fn k(key: &str) -> KeyInput<'_> { KeyInput { key, code: "", ctrl: false, alt: false, shift: false, meta: false } }
fn send(a: KeyAction) -> Vec<u8> { match a { KeyAction::Send(v) => v, other => panic!("{other:?}") } }

#[test]
fn xterm_js_key_table() {
    let n = Modes::default();
    let app = Modes { app_cursor: true, ..Modes::default() };
    let cases: &[(KeyInput, &Modes, &[u8])] = &[
        (k("ArrowUp"), &n, b"\x1b[A"), (k("ArrowUp"), &app, b"\x1bOA"),
        (KeyInput { shift: true, ..k("ArrowUp") }, &n, b"\x1b[1;2A"),
        (KeyInput { alt: true, ..k("ArrowUp") }, &n, b"\x1b[1;3A"),
        (KeyInput { ctrl: true, ..k("ArrowRight") }, &n, b"\x1b[1;5C"),
        (k("Home"), &n, b"\x1b[H"), (k("Home"), &app, b"\x1bOH"), (k("End"), &n, b"\x1b[F"),
        (k("PageUp"), &n, b"\x1b[5~"), (k("Delete"), &n, b"\x1b[3~"), (k("Insert"), &n, b"\x1b[2~"),
        (k("F1"), &n, b"\x1bOP"), (k("F4"), &n, b"\x1bOS"), (k("F5"), &n, b"\x1b[15~"), (k("F12"), &n, b"\x1b[24~"),
        (k("Backspace"), &n, b"\x7f"), (KeyInput { ctrl: true, ..k("Backspace") }, &n, b"\x08"),
        (KeyInput { alt: true, ..k("Backspace") }, &n, b"\x1b\x7f"),
        (k("Tab"), &n, b"\t"), (KeyInput { shift: true, ..k("Tab") }, &n, b"\x1b[Z"),
        (k("Enter"), &n, b"\r"), (KeyInput { alt: true, ..k("Enter") }, &n, b"\x1b\r"), (k("Escape"), &n, b"\x1b"),
        (KeyInput { ctrl: true, ..k("c") }, &n, b"\x03"), (KeyInput { ctrl: true, ..k(" ") }, &n, b"\x00"),
        (KeyInput { ctrl: true, ..k("[") }, &n, b"\x1b"), (KeyInput { ctrl: true, ..k("\\") }, &n, b"\x1c"),
        (KeyInput { ctrl: true, ..k("]") }, &n, b"\x1d"), (KeyInput { ctrl: true, ..k("/") }, &n, b"\x1f"),
        (KeyInput { alt: true, ..k("a") }, &n, b"\x1ba"), (k("ñ"), &n, "ñ".as_bytes()),
    ];
    for (key, modes, want) in cases {
        assert_eq!(send(encode_key(key, modes)), *want, "{}", key.key);
    }
}

#[test]
fn shift_pageup_scrolls_locally() {
    assert!(matches!(encode_key(&KeyInput { shift: true, ..k("PageUp") }, &Modes::default()), KeyAction::ScrollPage(-1)));
}

#[test]
fn paste_normalises_newlines_and_brackets() {
    let br = Modes { bracketed_paste: true, ..Modes::default() };
    assert_eq!(encode_paste("a\nb\r\nc", &Modes::default()), b"a\rb\rc");
    assert_eq!(encode_paste("x\x1b[201~y", &br), b"\x1b[200~xy\x1b[201~");
}

#[test]
fn sgr_and_x10_mouse() {
    let sgr = Modes { mouse: MouseMode::Click, sgr_mouse: true, ..Modes::default() };
    assert_eq!(encode_mouse(Button::Left, MouseKind::Press, 53, 9, (false, false, false), &sgr).unwrap(), b"\x1b[<0;54;10M");
    assert_eq!(encode_mouse(Button::Left, MouseKind::Release, 53, 9, (false, false, false), &sgr).unwrap(), b"\x1b[<0;54;10m");
    assert!(encode_mouse(Button::Left, MouseKind::Move, 1, 1, (false, false, false), &sgr).is_none(), "Click no informa movimiento");
    let x10 = Modes { mouse: MouseMode::Click, ..Modes::default() };
    assert_eq!(encode_mouse(Button::Left, MouseKind::Press, 0, 0, (false, false, false), &x10).unwrap(), b"\x1b[M !!");
    assert!(encode_mouse(Button::Left, MouseKind::Press, 300, 0, (false, false, false), &x10).is_none());
}

#[test]
fn wheel_policy_follows_the_spike() {
    let tmux_mouse = Modes { mouse: MouseMode::Drag, sgr_mouse: true, ..Modes::default() };
    assert!(matches!(wheel(-1, 0, 0, &tmux_mouse), WheelAction::Mouse(v) if v == b"\x1b[<64;1;1M"));
    let alt = Modes { alt_screen: true, alternate_scroll: true, ..Modes::default() };
    assert!(matches!(wheel(2, 0, 0, &alt), WheelAction::Arrows(v) if v == b"\x1b[B\x1b[B"));
    assert!(matches!(wheel(3, 0, 0, &Modes::default()), WheelAction::Scroll(3)));
}

#[test]
fn gboard_beforeinput_bytes() {
    // Review Focus 4: los teclados en pantalla no mandan keydown.
    assert_eq!(beforeinput("deleteContentBackward", None).unwrap(), b"\x7f");
    assert_eq!(beforeinput("insertLineBreak", None).unwrap(), b"\r");
    assert_eq!(beforeinput("insertText", Some("ñandú")).unwrap(), "ñandú".as_bytes());
    assert!(beforeinput("formatBold", None).is_none());
}

#[test]
fn ime_commit_once() {
    // Review Focus 4: Chrome manda compositionend y después input con el mismo texto.
    let mut ime = Ime::default();
    ime.start();
    assert!(ime.keydown_ignored(229));
    assert!(ime.input("insertCompositionText", Some("漢"), true).is_none());
    assert_eq!(ime.end("漢字").unwrap(), "漢字".as_bytes());
    assert!(ime.input("insertText", Some("漢字"), false).is_none(), "no se envía dos veces");
    assert_eq!(ime.input("insertText", Some("a"), false).unwrap(), b"a");
}
```

- [ ] **Step 2:** `$C test -p comandos-term --test input` → FAIL.
- [ ] **Step 3: Implementación** de `input.rs` como tabla `match` (`modifier_param = 1 + shift + 2·alt + 4·ctrl`; con parámetro las teclas de cursor usan `ESC[1;<p><X>` y las de `~` usan `ESC[<n>;<p>~`; Ctrl+letra → `letra & 0x1f`; Alt+carácter → `ESC` + carácter; Meta se ignora en Linux como xterm.js con `macOptionIsMeta: false`). Web: `keyboard.rs` instala un `<textarea>` oculto de 1×1 (atributos `autocorrect=off autocapitalize=off spellcheck=false`, `aria-label="Entrada de la terminal"`) que sigue la celda del cursor (el candidato del IME aparece junto al cursor, cubre el hueco 4 del spike); `keydown` → `encode_key` salvo `isComposing`/`keyCode 229`; `compositionstart`/`compositionend`/`input`/`beforeinput` pasan por `Ime` (texto compuesto **una sola vez**); `beforeinput` → `beforeinput()` con `preventDefault` solo si devolvió bytes; `paste` → `encode_paste`. `mouse.rs`: celda = `floor((x − left)/css_w)`, `floor((y − top)/css_h)`; rueda acumulada en líneas (`deltaMode` píxel → `/css_h`), una petición en vuelo como máximo cuando va a `/tmux-scroll` (A10).
- [ ] **Step 4:** `$C test -p comandos-term --test input` → PASS.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-term/src/input.rs crates/comandos-term/tests/input.rs crates/comandos-term-web/src/keyboard.rs crates/comandos-term-web/src/mouse.rs crates/comandos-term-web/src/lib.rs
git commit -m "feat(term): codificación de teclado, pegado, ratón y rueda compatible con xterm.js; IME y GBoard en web

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task A8: Selección, copia, enlaces (OSC 8 y URL envueltas) y ligaduras

**Depende de:** A6, A7. **Paralela a** A9.

**Files:**
- Create: `crates/comandos-term/src/select.rs`, `crates/comandos-term-web/src/links.rs`
- Test: `crates/comandos-term/tests/select.rs`

**Interfaces:**
- Produces:
  - `pub struct Selection { pub anchor: (i32, u16), pub head: (i32, u16), pub mode: SelectMode }` (`Simple`, `Word`, `Line`; filas absolutas con historia negativa); `pub fn selected_text(engine: &Engine, s: &Selection) -> String` (une filas envueltas sin salto, recorta espacios finales como xterm.js `translateToString(true)`).
  - `pub fn find_urls(engine: &Engine, line: i32) -> Vec<UrlSpan>`; `pub struct UrlSpan { pub start: (i32, u16), pub end: (i32, u16), pub url: String }` — regex `https?://[^\s'"<>\])]+` sobre el bloque de filas llenas contiguas (la misma regla del proveedor multi-fila de `term.html`: fila llena = probable envoltura dura de tmux) más OSC 8 de las celdas.
  - `pub fn ligature_runs(text: &str) -> Vec<(usize, usize)>` (rangos de las secuencias candidatas de `assets/xterm/addon-ligatures-web.js`, búsqueda voraz de mayor a menor longitud).

- [ ] **Step 1: Prueba que falla**

```rust
// crates/comandos-term/tests/select.rs
use comandos_term::{engine::{Engine, GridSize, Palette}, select::*};

fn eng(cols: u16) -> Engine {
    Engine::new(GridSize { cols, rows: 5 }, 100, Palette::xterm_default([255; 3], [0; 3], [255; 3], [0; 3], [80; 3]))
}

#[test]
fn wrapped_rows_copy_without_newline_and_trailing_spaces_are_trimmed() {
    let mut e = eng(10);
    e.advance(b"0123456789abc   \r\nfin", 0.0);
    let s = Selection { anchor: (0, 0), head: (2, 2), mode: SelectMode::Simple };
    assert_eq!(selected_text(&e, &s), "0123456789abc\nfin");
}

#[test]
fn url_split_by_tmux_hard_wrap_is_one_link() {
    let mut e = eng(20);
    e.advance(b"ver https://ejemplo.mx/un/camino/largo ok", 0.0);
    let urls = find_urls(&e, 1);
    assert_eq!(urls.len(), 1);
    assert_eq!(urls[0].url, "https://ejemplo.mx/un/camino/largo");
    assert_eq!((urls[0].start.0, urls[0].end.0), (0, 1));
}

#[test]
fn ligature_candidates_are_greedy() {
    assert_eq!(ligature_runs("a <=> b != c"), vec![(2, 5), (8, 10)]);
    assert_eq!(ligature_runs("===>"), vec![(0, 4)]);
}
```

- [ ] **Step 2:** `$C test -p comandos-term --test select` → FAIL.
- [ ] **Step 3: Implementación.** `select.rs` sobre `term.grid()` (sin usar el módulo `selection` de alacritty para no depender de su semántica de palabras; `Word` usa los separadores de xterm.js 5.5: espacio, `()[]{}'"`). `ligature_runs`: lista literal `CANDIDATES` copiada de `addon-ligatures-web.js` (es un dato), ordenada por longitud. Web: arrastre con el ratón cuando `Modes.mouse == Off` o con Shift (xterm.js); con ratón de tmux activo la copia sigue por tmux + OSC 52 (`Drained.clipboard` → `navigator.clipboard.writeText`). Ctrl+Shift+C / menú contextual copian; un clic sobre enlace con Ctrl abre (`window.open(url, "_blank", "noopener,noreferrer")` vía la función `openWebLink` que porta A9). **Ligaduras:** solo sin táctil (igual que `term.html`, `IS_TOUCH` las desactiva); las tiras que contienen rangos de `ligature_runs` se pintan con la cara `comandos-liga` (`FontFace("comandos-liga", url(../assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-Regular.ttf))`, cargada desde Rust) y `fillText` de la secuencia completa centrada en sus celdas; diferencia aceptada documentada (D10 del índice): rasterización del navegador frente a los trazados de opentype.js, ±1 px de antialias en esas celdas (la suite `term-liga` usa `channel: 48`).
- [ ] **Step 4:** `$C test -p comandos-term --test select` → PASS.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-term/src/select.rs crates/comandos-term/tests/select.rs crates/comandos-term-web/src/links.rs crates/comandos-term-web/src/lib.rs
git commit -m "feat(term): selección y copia, enlaces OSC 8 y URL envueltas por tmux, ligaduras de escritorio

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task A9: Carcasa de la página de terminal (port de `dash/term.html`, parte común)

**Depende de:** A6, A7, B1 (`comandos-web-dom`, `comandos-web-view`), B3 (inventario de `term.html`). **Paralela a** A8.

**Files:**
- Create: `crates/comandos-term-web/src/{page.rs,connection.rs,theme.rs,parent.rs}`, `crates/comandos-web-view/src/term_page.rs`
- Test: `crates/comandos-term-web/src/connection.rs` (unitarias de host), `crates/comandos-web-view/tests/term_page.rs`

**Interfaces:**
- Consumes: `WebTerm` (A6), entrada (A7), `comandos_web_dom::{dom, api, storage, i18n, log}` (B1), `proto` (A2).
- Produces:
  - `term_page::shell(params: &TermParams) -> Markup`: el DOM de `term.html` fuera de los `<script>` (la misma estructura `#term-shell`, `#term`, `#term-toolbar`, `#dbg`, el overlay de error, la vista de historial, en el mismo orden, con las mismas clases e ids que lista el inventario B3), más los `<link>` de `buttons.css` y el `<style>` de `term.html` copiado tal cual; **sin** `xterm.css` ni scripts de xterm.
  - `connection::Reconnect { attempt: u32 }` con `next_delay_ms(&mut self) -> u32` (250·2ⁿ, tope 4000, `attempt` tope 5) y `connected(&mut self)`; `connection::InputQueue::new(max_bytes: usize, max_age_ms: f64)` con `push(&mut self, data: &[u8], now: f64) -> bool` y `drain(&mut self, now: f64) -> Drained { items, stale }` (4096 B / 15 s, como `createInputQueue`); `connection::AckTracker` (vacía el compositor solo cuando la terminal responde tras el envío, como `createAckTracker`).
  - Dialecto: la página nueva usa `comandos.term.v1` (`new WebSocket(<base>/ws, ["comandos.term.v1"])`, init con `session` = `arg`); si el servidor contesta con `tty` (A4 en modo `ttyd`), cae al dialecto `tty` con `arg` en la URL.
  - API con el padre (`postMessage`, mismo origen): envía `{source:'comandos-term', type:'ready'}`, `user-interaction`, `pane-selected`; recibe y aplica `theme`, `button-style` y cambio de sesión exactamente como `handleTerminalMessage` de `term.html`.
  - Temas: los mismos objetos `THEMES` de `term.html` (`noche`, `dia`, `calido`, … copiados literal a `theme.rs`) y el mismo efecto en las variables CSS `--term-*`.

- [ ] **Step 1: Prueba que falla (host)**

```rust
// en crates/comandos-term-web/src/connection.rs
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backoff_matches_term_html() {
        let mut r = Reconnect::default();
        let seq: Vec<u32> = (0..7).map(|_| r.next_delay_ms()).collect();
        assert_eq!(seq, vec![250, 500, 1000, 2000, 4000, 4000, 4000]);
        r.connected();
        assert_eq!(r.next_delay_ms(), 250);
    }
    #[test]
    fn input_queue_is_bounded_in_bytes_and_age() {
        let mut q = InputQueue::new(4096, 15_000.0);
        assert!(q.push(&[b'a'; 4000], 0.0));
        assert!(!q.push(&[b'b'; 200], 1.0), "se rechaza lo que no cabe; el borrador se conserva");
        let d = q.drain(20_000.0);
        assert!(d.stale && d.items.len() == 1);
    }
}
```

`crates/comandos-web-view/tests/term_page.rs`: `dom_diff::normalize` (de `xtask`, usado como dev-dependency) del `shell(..)` frente al HTML de `dash/term.html` con los `<script>` y el `<link>` de `xterm.css` quitados por `scraper` → iguales.

- [ ] **Step 2:** `$C test -p comandos-term-web --lib && $C test -p comandos-web-view --test term_page` → FAIL.
- [ ] **Step 3: Implementación.** `page.rs` (`#[wasm_bindgen] pub fn boot_term(k: &str)`): lee la consulta (`auth`, `arg`, `theme`, `btn`, `debug`, `ws`), aplica el estilo de botones heredado del padre (try/catch de otro origen como `term.html`), monta `WebTerm` en `#term` con `fontFamily` y `fontSize: 14`, `lineHeight: 1.2`, `letterSpacing: 0`, `cursorBlink: true`, `scrollback: 10000` (los de `term.html`) y aplica después las preferencias `'2'` del servidor (11 px), conecta, reconecta con `Reconnect`, reenvía tamaño con `ResizeObserver` (`fit` + `'1'+JSON` solo si cambió), escribe `[conexión restablecida]` / `[conexión cerrada · <código>; reconectando]` en gris como `term.html`, aplaza el ajuste mientras hay selección. Todo texto visible sale del inventario B3 (mismas cadenas en español e inglés).
- [ ] **Step 4:** pruebas de host → PASS; e2e en el Mac (A12) cubre la conexión real.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-term-web/src crates/comandos-web-view/src/term_page.rs crates/comandos-web-view/src/lib.rs crates/comandos-web-view/tests/term_page.rs
git commit -m "feat(term): página de terminal en Rust — carcasa, conexión con reconexión y cola, temas y API con el padre

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task A10: Controles remotos de la terminal (barra, compositor, historial, paneles, gestos)

**Depende de:** A9 y B5 (`comandos_web_dom::drafts`, port de `dash/device-drafts.js`).

**Files:**
- Create: `crates/comandos-term-web/src/remote.rs`, `crates/comandos-web-view/src/term_remote.rs`
- Test: `crates/comandos-web-view/tests/term_remote.rs`, e2e en `xtask/web/shots.json` (suites `term-remote-*`) y `xtask term-bench --e2e remote` (A12)

**Interfaces:**
- Consumes: `connection::*`, `WebTerm` (A9, A6); rutas existentes `/terminal-panes`, `/terminal-history`, `/tmux-scroll`, `/tab-models`, `/accounts`, `/account/switch`, `/account/add`, `/model/status`, `/workspace/client` (mismos cuerpos y cabecera `X-Comandos-Token` que `term.html`).
- Produces: el comportamiento de `docs/remote-terminal-input.md` y `docs/remote-session-controls.md` (sección «Terminal remota») sin cambios: `⌫ Borrar` (envía `0x7f`, no se encola desconectado), Esc, flechas, Ctrl+C, pegar; **Seleccionar/Historial** (copia estable del texto del panel, `Volver`, `Copiar`, `Todo`, selector `Inicio/Fin`, cuatro flechas que nunca envían teclas, letra mínima 16 px en táctil, hasta 2000 líneas, `Actualizar`, `Final`); **Paneles** (activar split, `Cerrar` con confirmación del panel concreto); compositor nativo (borrador persistente vía `device-drafts`, `Insertar`/`Enviar`, `Teclado directo`); gesto de pulsación larga ~300 ms + arrastre → ratón SGR solo con ratón de tmux activo, con «⇕ redimensionando» y `navigator.vibrate`; scroll remoto con una petición `/tmux-scroll` en vuelo como máximo y acumulador acotado; selector de cuenta/modelo de la barra. `HAS_REMOTE_CONTROLS = IS_TOUCH || (protocolo ≠ file: && arg presente)` y, en el perfil `plain` (A11), siempre falso.

- [ ] **Step 1: Prueba que falla.** `term_remote.rs` compara con `dom_diff` el HTML de la barra (`#term-toolbar`) y de la vista de historial que genera `term_remote::toolbar(..)`/`history_view(..)` con el que construye `term.html` (capturado una vez por `xtask web-inventory --page term.html --dump toolbar,history` en `xtask/web/fixtures/term/*.html`, B3).
- [ ] **Step 2:** `$C test -p comandos-web-view --test term_remote` → FAIL.
- [ ] **Step 3: Implementación** en `remote.rs`, por bloques del inventario B3 de `term.html` (`term.html:1088-1700` compositor y teclado móvil, `1640-1880` historial, `1875-1970` paneles, `2140-2330` cuentas y modelo), con las mismas cadenas, los mismos tiempos (300 ms de pulsación, una petición de scroll en vuelo) y las mismas rutas. Los borradores usan `comandos_web_dom::drafts` (B5), con las mismas claves de `localStorage` que `dash/device-drafts.js`; así el borrador escrito con la página vieja aparece en la nueva.
- [ ] **Step 4:** prueba de vista → PASS. E2e en el Mac (con el frente de prueba, `PrivateTmux` y fixtures; nunca contra 4777) a 320, 390, 844 y 1400 px: Backspace remoto llega como `0x7f` a la sesión privada, el borrador sobrevive a una reconexión forzada, la vista de historial no envía teclas, «Paneles → Cerrar» confirma el panel elegido aunque cambie el foco. Comando: `cargo run -p xtask -- term-bench --e2e remote --out docs/verification/fase3/term-remote`.
- [ ] **Step 5: Commit**

```bash
git add crates/comandos-term-web/src/remote.rs crates/comandos-web-view/src/term_remote.rs crates/comandos-web-view/tests/term_remote.rs xtask/web/fixtures/term
git commit -m "feat(term): controles remotos de la terminal en Rust (barra, compositor, historial, paneles, gestos)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task A12: Banco y paridad de la terminal (`xtask term-record`, `term-bench`, suites `term-*`)

**Depende de:** A8–A10, T2. **Se ejecuta antes que A11.**

**Files:**
- Create: `xtask/src/term_record.rs`, `xtask/src/term_bench.rs`, `xtask/web/replays/` (grabaciones), entradas `term-*` en `xtask/web/shots.json`, `docs/verification/fase3/term-reference.json`
- Modify: `xtask/src/main.rs`

**Interfaces:**
- Consumes: `PrivateTmux` (lógica equivalente en `xtask` con `private_tmux(dir)`), `mcp::Client`, `shots`, `png_diff` (T2), frente de prueba (A4) y página nueva (A9–A10).
- Produces:
  - `cargo run -p xtask -- term-record --suite base --out DIR`: en un tmux **privado** (`private_tmux(dir)`, `-S` propio; `Drop` mata ese servidor y borra el directorio) crea una sesión 160×44, ejecuta cada guion y graba la salida del cliente `tmux attach` (por un PTY propio) en `DIR/<nombre>.bin`: `ls --color=always /usr`, tabla de 256 colores y degradado truecolor (`printf`), atributos (negrita, cursiva, subrayado, tachado, inverso, tenue), UTF-8 (CJK, emoji, `ñandú`), `htop -d 5` 3 s, `vim -u NONE` sobre un archivo de ejemplo, `seq 1 500000`, dibujo de cajas y powerline, una ráfaga con `CSI ? 2026 h/l`. Las grabaciones de TUIs de agentes se obtienen con `--live-readonly <sesión>:<panel>` (lo usa solo el controlador): una única ejecución de `tmux capture-pane -p -e -J -S -2000 -t <panel>` **de solo lectura** sobre el servidor del usuario (comando de lectura de un panel, sin `kill`, `set` ni `attach`), guardada como `agent-<n>.bin`.
  - `cargo run -p xtask -- term-bench --base URL [--wasm-tests] [--e2e remote] --out DIR`: para cada variante (`xterm` = `term.html` + puente en modo `ttyd`; `native` = página nueva) en el mismo navegador del Mac: eco de tecla (20 muestras; el arnés inyecta por `evaluate_script` un oyente que marca `performance.now()` al enviar `0x` + marcador y al ver el marcador pintado), rendimiento de análisis (1,87 MB del banco del spike), peor cuadro con `seq 1 500000`, memoria por iframe con 10 000 líneas de historia, cuadros por segundo en reposo (0 esperado), `cols/rows` y dimensiones de celda en el mismo contenedor (`fit_matches_xterm`, `cell_metrics_match_xterm`: los de xterm.js se leen con `term._core._renderService.dimensions`). Guarda `DIR/bench.json` y compara con los presupuestos del índice; código 1 si alguno falla.
  - Suites `term-xterm`, `term-native`, `term-liga`, `term-remote-*`: misma grabación en ambas variantes, a 1400/844/390/320 px y dpr 1/2, recortes `#term canvas, .xterm-screen` (el canvas completo) y la barra; `channel: 24`, `ratio: 0.002` (`term-liga`: `channel: 48`).

- [ ] **Step 1: Prueba que falla.** `xtask/tests/term_record.rs`: con `tmux` presente, `term-record --suite mini` (solo `printf hola`) crea `mini/printf.bin` que contiene `hola`; después de terminar, el directorio del socket privado no existe y `tmux -S <ese socket> has-session` falla (el servidor privado murió y no quedó nada).
- [ ] **Step 2:** `$C test -p xtask --test term_record` → FAIL.
- [ ] **Step 3: Implementación** de `term_record.rs` (PTY con `pty-process`, `-S` privado, guiones como `&[(&str, &str, u64)]` nombre/comando/segundos) y `term_bench.rs` (orquesta `mcp::Client`; las funciones JS de medición son cadenas de una línea en el código Rust, Ruling 5 del índice).
- [ ] **Step 4:** `$C test -p xtask --test term_record` → PASS.
- [ ] **Step 5: Medición de referencia y de la variante nueva** (controlador, con frente de prueba en un puerto 73xx, `--term-replay-dir` y `cc-browser-expose`): `term-bench` y `shots pair --suite term-native`; resultados a `docs/verification/fase3/term-reference.json` y `term-native.json`. **Criterio para pasar a A11:** 0 recortes fuera de tolerancia en `term-native` y `term-remote-*`, todos los presupuestos cumplidos.
- [ ] **Step 6: Commit**

```bash
git add xtask/src/term_record.rs xtask/src/term_bench.rs xtask/src/main.rs xtask/tests/term_record.rs xtask/web/shots.json xtask/web/replays docs/verification/fase3/term-reference.json docs/verification/fase3/term-native.json
git commit -m "feat(xtask): grabaciones en tmux privado, banco de latencia/memoria y paridad visual de la terminal

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task A11: Paso 2 — la página `comandos-term` en `/term/`, perfil `plain` en 4779 y retirada de ttyd

**Depende de:** A12 en verde, B2 (compositor y registro: la página de terminal es el componente `term`).

**Files:**
- Modify: `crates/comandos-server/src/dash/term/{mod.rs,compat.rs}`, `crates/comandos-web/components.json` (componente `term`), `crates/comandos-cli/src/webterm.rs`, `docs/verification/cutover-term.md` (sección «Paso 2»)
- Test: `crates/comandos-server/tests/term_compat.rs` (casos `plain`), `xtask/web/shots.json` (`term-plain`)

**Interfaces:**
- Consumes: `CompatProfile::Plain`, `TermMode::Native` (A4), compositor `web::compose_term` y registro (B2), `boot_term` (A9).
- Produces:
  - Con el componente `term` activo (`H/comandos-web.json`, D2), `GET /term/` sirve `compose_term()`: `term_page::shell` + `boot.js` del artefacto `comandos_term_web` + compuerta; si no está activo, `dash/term.html` (paso 1). El modo `TermMode::Native` admite ambos dialectos, así una pestaña vieja con xterm.js y una nueva conviven durante el corte.
  - Perfil `Plain` en 4779: `GET /` y `/?arg=<token>&arg=<sesión>` → la página nueva con `embed=plain` (sin barra remota, fuente de las preferencias de ttyd: 11 px; es lo que ve hoy la barra lateral de `cc-app` con la UI de ttyd); `GET /token` → `{"token":""}`; `GET /ws` → dialecto `tty`; además sirve desde `dash_dir`, solo en este perfil, `/assets/*`, `/buttons.css` y `/device-drafts.js` para el respaldo `web=off` (la página xterm.js con sus rutas `../assets/…` resueltas en 4779) (lo usan `bin/cc-app:3512` y `bin/cc-app-mac:94` hasta las Fases 4 y 5).
  - `comandos webterm on` con frente `native` y 4779 atendido: no lanza ningún ttyd.

- [ ] **Step 1: Prueba que falla.** `term_compat.rs`: `serve_compat(4779-de-prueba, Plain)` → `GET /token` 200; `GET /?arg=t0k&arg=t1` → HTML con `data-embed="plain"`; WS `/ws` con `tty` y `arg` válido hace eco en la sesión privada; `GET /term/` → 404 en ese perfil. `compat_8443_origin_allowed`: petición con `Host: <host del tailnet>:8443` y `Origin: https://<host>:8443` pasa la puerta (la usa el respaldo de `index.html`).
- [ ] **Step 2:** `$C test -p comandos-server --test term_compat` → FAIL.
- [ ] **Step 3: Implementación** de lo anterior; la página `plain` es la misma `boot_term` con `HAS_REMOTE_CONTROLS = false` y sin iframe padre.
- [ ] **Step 4:** PASS; `shots pair --suite term-plain` en el Mac compara la UI embebida de un ttyd 1.6.3 de prueba (tmux privado, mismos `-t`) con el perfil `plain` del frente de prueba: 0 recortes fuera de tolerancia.
- [ ] **Step 5: Procedimiento «Paso 2»** en `cutover-term.md`:

````markdown
## Paso 2 — página comandos-term en /term/ y 4779 sin ttyd

### 1. Sombra
```sh
~/.local/share/comandos/bin/comandos web set term shadow
```
En el teléfono: abrir el tablero con `?web=shadow` y una pestaña «+» (terminal nueva) — solo esa pestaña usa la página nueva; las demás siguen con xterm.js. Usarla un día completo.
### 2. Activar
```sh
printf '[Service]\nEnvironment=COMANDOS_DASH_TERM=native\nEnvironment=COMANDOS_DASH_WEBTERM_COMPAT=4780,4779\n' > ~/.config/systemd/user/cc-dash.service.d/term.conf
systemctl --user stop cc-webterm.service
systemctl --user daemon-reload && systemctl --user restart cc-dash.service
~/.local/share/comandos/bin/comandos web set term on
```
`cc-app` sigue intacta: su barra lateral recarga sus iframes de 4779 al reconectar y ve la página `plain`.
### 3. Verificar
`ss -ltnp | grep -E '4779|4780'` → ambos `comandos`; `pgrep -x ttyd` → vacío; barra lateral de `cc-app`, PWA y respaldo `:8443` abren terminales; memoria por terminal según `term-bench`.
### 4. Revertir
```sh
~/.local/share/comandos/bin/comandos web set term off          # vuelve xterm.js en /term/, sin reinicio
printf '[Service]\nEnvironment=COMANDOS_DASH_TERM=ttyd\nEnvironment=COMANDOS_DASH_WEBTERM_COMPAT=4780\n' > ~/.config/systemd/user/cc-dash.service.d/term.conf
systemctl --user daemon-reload && systemctl --user restart cc-dash.service && ~/.local/bin/cc-webterm   # ttyd vuelve a 4779
```
````

- [ ] **Step 6: Commit**

```bash
git add crates/comandos-server/src/dash/term crates/comandos-server/tests/term_compat.rs crates/comandos-web/components.json crates/comandos-cli/src/webterm.rs xtask/web/shots.json docs/verification/cutover-term.md
git commit -m "feat(dash): /term sirve comandos-term, perfil plain en 4779 y ttyd retirado

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

## Riesgos propios de 3a

- **Métricas de celda distintas de xterm.js** (la causa más probable de un desfase de 1 px que arrastra toda la pantalla): se comparan por `evaluate_script` contra `term._core._renderService.dimensions` en A12 antes que cualquier captura.
- **El ratón de tmux y el scroll local** se mezclan (spike, «Scrollback lives in tmux»): la política está fijada en `wheel()` con prueba y el e2e remoto la ejerce.
- **Grabaciones de agentes**: la única lectura del servidor del usuario es `capture-pane` de solo lectura bajo `--live-readonly`, ejecutada por el controlador; nada más del plan toca ese servidor salvo el attach de producción.
- **WebKitGTK de `cc-app`** al cargar la página `plain` en 4779: si su WASM fallara, la compuerta (D3) recarga con `web=off`, que en `plain` sirve la página xterm.js; la barra lateral nunca queda en blanco.
