# Spike: Rust terminal engine (WASM + canvas) replacing xterm.js + ttyd

Date: 2026-10-04. Everything under `.spike/term/` is throwaway and git-ignored (`.spike/.gitignore` = `*`).

## Recommendation

Use **`alacritty_terminal` 0.26.0** as the VT model, compiled to `wasm32-unknown-unknown` with a
two-line source patch. Render with a **Canvas 2D renderer written in Rust** that repaints only the
lines alacritty's damage tracking reports, batches same-style ASCII runs into one `fillText`, and
draws wide or non-ASCII glyphs one at a time at their cell origin. Feed it from a **single-threaded
tokio bridge** that gives each WebSocket its own PTY running `tmux attach`.

It works end to end: colours, attributes, cursor, resize, mouse, UTF-8 input and output, and tmux
scrollback are all correct in the remote browser. The remaining work (see "Gaps against xterm.js")
does not block the architecture.

## What was built

| Part | Path | Size |
|---|---|---|
| PTY<->WebSocket bridge (tokio, tokio-tungstenite 0.24, pty-process 0.5) | `bridge/src/main.rs` | 152 lines |
| Load-test client for RSS numbers | `bridge/src/bin/wsclient.rs` | 35 lines |
| WASM terminal (wasm-bindgen 0.2.129, web-sys, alacritty_terminal) | `web/src/lib.rs` | ~900 lines |
| Patched alacritty_terminal 0.26.0 | `vendor/alacritty_terminal/` | 2-line diff + Cargo cfg |
| Isolated tmux config (`tmux -L comandos-spike`) | `spike-tmux.conf` | |
| Build script (cargo, wasm-bindgen, fonts) | `build-web.sh` | |

Protocol on `/ws`: client->server binary = keyboard bytes; text `R <cols> <rows>` = resize;
server->client binary = raw PTY output. The same port serves `web/dist` over HTTP. The only JS is
the `wasm-bindgen` loader plus a one-line `import init ...; init()` in `index.html`.

## Terminal-model crate evaluation

| Crate | wasm32-unknown-unknown | Notes |
|---|---|---|
| `alacritty_terminal` 0.26.0, `default-features = false` | Fails unpatched; builds after a 2-line patch | **Chosen.** |
| `termwiz` 0.23.3 (WezTerm) | Fails | `getrandom`/`uuid` need wasm features; after adding them `mac_address` fails (no `os` module). `wezterm-term` is not on crates.io (only forks such as `zeughaus-wezterm-term`). Stopped. |
| `vt100` 0.16.2 | Builds cleanly (68 KB probe) | Pure and small, but no damage tracking, no synchronized updates (2026), no OSC 52 events, no cursor shape, fewer underline styles. Fallback only. |

### alacritty_terminal compile errors and fixes

1. `error: polling does not support this target OS`. `polling`, `libc` and `home` are
   unconditional deps used only by the `tty` (PTY spawn) and `event_loop` modules, which a
   browser does not need. Fix (vendored, via `[patch.crates-io]`):
   - `src/lib.rs`: `#[cfg(not(target_arch = "wasm32"))]` on `pub mod event_loop;` and `pub mod tty;`.
   - `Cargo.toml`: move `home`, `libc`, `polling` under `[target."cfg(not(target_arch = \"wasm32\"))".dependencies]`.
   Afterwards `cargo tree --target wasm32-unknown-unknown -i libc` is empty. Small enough to
   upstream as a `wasm` cfg or keep as a vendored fork.
2. Runtime trap found by reading the source (not hit in testing): vte's default `StdSyncHandler`
   calls `std::time::Instant::now()` on `CSI ? 2026 h` (synchronized update), which panics on
   wasm32-unknown-unknown. Fix: `Processor<T: Timeout>` is generic; the spike supplies `WasmSync`
   on `js_sys::Date::now()` and calls `stop_sync` when the deadline passes. Claude Code emits mode
   2026, so production needs this.

Features: `alacritty_terminal` `default-features = false` (no serde); `vte` `std` + `ansi` (pinned by the crate).

## Measurements

### WASM size (release: `opt-level="s"`, `lto=true`, `codegen-units=1`, `panic="abort"`, strip)

**`wasm-opt` is not installed; numbers are without it.**

| File | Raw | gzip -9 |
|---|---|---|
| `spike_web_bg.wasm` | 191,562 B (187 KB) | 75,426 B (74 KB) |
| `spike_web.js` (generated glue) | 35,537 B | 6,412 B |

(Parser-only probe at default opt-level 3, no LTO: 268 KB.) The bundled JetBrains Mono Nerd Font
TTFs (4 x 2.4 MB) outweigh the engine; production should serve subset woff2.

### Bridge RSS (`ps -o rss`, current-thread tokio, release, 849 KB binary)

| State | RSS | Threads |
|---|---|---|
| Idle, no sockets | 2,504 KB | 1 |
| 1 WebSocket | 2,688 KB | 1-2 |
| 5 WebSockets | 3,180 KB | 1 |
| 5 WebSockets while `seq 1 300000` streams to all | 3,244 KB | 1 |
| 1 browser socket after a ~1.5 MB burst to the remote browser | 10,540 KB | 1 |

The last row is allocator high-water: the remote browser drained the burst slower than tmux
produced it and the bridge has no backpressure policy yet. Each `tmux attach` client costs a
separate ~4.5 MB RSS; the spike tmux server used 6.7 MB.

### Latency and throughput (remote HeadlessChrome on the Mac mini via the `cc-browser-expose` SSH tunnel)

- **Key echo round trip** (browser WS -> tunnel -> bridge -> tmux -> zsh echo -> back), 20 samples:
  min 3.5 ms, **median 5.2 ms**, max 8.0 ms, plus one animation frame to paint. Typing feels immediate.
- **Parser+model throughput in the browser** (`?bench`, 1.87 MB of 256-colour/truecolour SGR text with CJK): 103-109 ms, **~17-18 MB/s**.
- **Full-screen redraw:** 5k cells (106x47) 9.8 ms; 19k cells (158x120) 25 ms (cold first frames, CPU-only headless browser).
- **Live session:** `seq 1 500000` took 0.21 s at the shell (tmux absorbs output); the browser parsed
  the resulting ~1.5 MB in 328 ms and painted only 24 frames thanks to `requestAnimationFrame`
  coalescing; UI stayed responsive. Session totals: average frame 5.0 ms (2,341 lines repainted over
  141 frames), worst frame 57.7 ms.

### Resize

Works. Viewport 1280x800@1x -> 820x560@2x: tmux client went 160x44 -> 96x30 and redrew; HiDPI text
is sharp (backing store scaled by `devicePixelRatio`).

## Visual verification (chrome-bg screenshots, remote browser)

| Check | Result |
|---|---|
| `ls --color` | Correct colours and alignment |
| 256-colour table, truecolour gradients | Exact, cell-aligned |
| bold/italic/underline/strike/inverse/dim, bold-as-bright | Correct |
| Cursor (block #FFAE1A, glyph in bg colour) | Correct; beam/underline shapes implemented, untested |
| `htop -d 5` | Meters, colours, inverse header, F-key bar all correct |
| UTF-8 output: CJK/emoji take 2 cells (`\|` after 4 wide chars aligns with one after 8 ASCII) | Correct |
| UTF-8 input `ñandú 漢字 🚀` | Reached tmux intact (needed the hidden-textarea fix below) |
| `seq 1 500` + wheel | Wheel sends SGR mouse to tmux -> copy-mode scroll through tmux history |
| Mouse click | `cat -v` under `?1000h?1006h` received `^[[<0;54;10M^[[<0;54;10m` (correct SGR press/release, right cell). An htop header click did not change sort; bytes did reach tmux, not investigated further |
| Status bar #121722, bg #0A0D13, text #EAF0FB | Correct |

Caveat about the remote browser: it is HeadlessChrome on Linux with almost no fonts (every family,
`monospace` included, resolved to one proportional font; no CJK/emoji fonts). Consequences:
- First render was misaligned. Fix: bundle the webfont ComandOS already ships
  (`assets/fonts/JetBrainsMono`) via `@font-face`, await `document.fonts.load(...)` from Rust, then
  re-measure (same as `term.html` today; correct for production anyway).
- CJK/emoji show as tofu there, with correct 2-cell width; desktop Chrome on macOS would use system fallbacks.

## Architecture notes for the real implementation

- **Scrollback lives in tmux.** tmux puts the outer terminal on the alt screen and enables mouse
  reporting, so local alacritty history barely fills. The wheel must speak SGR mouse to tmux when
  `MOUSE_MODE` is set (spike does), arrow keys under `ALT_SCREEN`+`ALTERNATE_SCROLL`, local scroll
  otherwise. A local 10k-line history only matters for non-tmux sessions.
- **Terminal replies go back over the socket.** alacritty emits `PtyWrite` (DA/DSR), `ColorRequest`
  (OSC 10/11), `TextAreaSizeRequest`; the spike's `EventListener` queues and flushes them after each
  `advance`. OSC 52 `ClipboardStore` is wired to `navigator.clipboard.writeText` (tmux copy-mode ->
  system clipboard; not exercised).
- **Keyboard needs a hidden `<textarea>`.** `keydown` alone dropped `ñ ú 漢字` typed via CDP
  `insertText` (the IME path). Spike keeps `keydown` for keys/control codes and adds a 1x1
  transparent textarea whose `input`/`compositionend` events carry text; `keydown` is ignored while
  composing (keyCode 229). Same design as xterm.js.
- **Damage-driven Canvas 2D is enough at these sizes.** `term.damage()` plus old/new cursor lines
  pick the rows to repaint. Keep the renderer behind a trait so a WebGL glyph-atlas backend can be
  added for huge/busy panes.
- **Bridge needs backpressure.** Bound the per-socket queue (pause PTY reads or coalesce), like
  ttyd's flow control; see the 10.5 MB high-water.
- **Isolation details.** Bridge strips `TMUX`/`TMUX_PANE` before spawning (required when launched
  inside another tmux), sets `TERM=xterm-256color`, `COLORTERM=truecolor`; the tmux config adds
  `terminal-features xterm-256color:RGB` so truecolour passes through tmux 3.2a.

## Gaps against xterm.js to close later

1. **Selection/copy in the renderer:** absent. alacritty has a `selection` module; needs wiring
   (drag -> `Selection`, highlight, Cmd/Ctrl+C) for when tmux mouse is off. With tmux mouse on, copy
   goes through tmux + OSC 52.
2. **Hyperlinks (OSC 8):** stored per cell by alacritty (`cell.hyperlink()`), not rendered/clickable; no regex URL detection.
3. **Ligatures:** none; ligating fonts would break alignment inside a `fillText` run. Needs per-glyph drawing or shaping.
4. **IME:** committed text works; no composition preview, textarea does not follow the cursor (candidate window at 0,0).
5. **Smooth scrolling / inertia:** line-quantized; tmux decides scroll speed.
6. **Box-drawing/powerline glyphs:** come from the font, so vertical lines gap between rows; xterm.js draws them in code (`customGlyphs`).
7. **Massive output:** fine at 1.5 MB, but every run is a `fillText`; add an `OffscreenCanvas` glyph atlas or WebGL for giant panes / 60 fps full-screen animation. Bridge backpressure missing.
8. **Accessibility and search:** no screen-reader DOM mirror, no find-in-buffer.
9. **Images (sixel/iTerm2/kitty):** not in alacritty; WezTerm's model has them but does not compile to wasm.
10. **Not ported from term.html:** reconnect logic, ttyd auth token, themes, font prefs.
11. **Keyboard protocols:** kitty keyboard / modifyOtherKeys not encoded (alacritty tracks the kitty mode flags).
12. **Bell, focus reporting (`?1004`), cursor blink:** not implemented.

## Reproduce

```sh
cd .spike/term
CARGO_TARGET_DIR=../target nice -n 10 cargo build -j 6 --release --manifest-path bridge/Cargo.toml
./build-web.sh                                   # needs wasm-bindgen-cli 0.2.129 (installed)
SPIKE_PORT=7230 ../target/release/spike-bridge & # devhost: http://spike-term.localhost -> 7230
~/.local/bin/cc-browser-expose start 7230        # chrome-bg new_page http://127.0.0.1:7230/?stats
../target/release/wsclient 5 10                  # 5 sockets for 10 s (RSS)
# http://127.0.0.1:7230/?bench -> in-browser parser benchmark (window.__spikeBench)
tmux -L comandos-spike kill-server; kill %1; ~/.local/bin/cc-browser-expose stop 7230
```

Leftovers: devhost entry `spike-term` (port 7230) still registered (`devhost rm spike-term` removes it);
`wasm-bindgen-cli` 0.2.129 installed in `~/.cargo/bin`. tmux server, bridge and SSH forward are stopped.
