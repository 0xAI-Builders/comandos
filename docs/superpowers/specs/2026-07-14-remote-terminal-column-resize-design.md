# Remote Terminal Column Resize Design

**Date:** 2026-07-14
**Status:** Approved

## Problem

The responsive remote terminal fits xterm.js to every iframe resize. When the
column count changes, xterm resizes its alternate buffer immediately and sends
the new PTY geometry to ttyd. tmux then redraws the same screen for the new
geometry. The local resize and the tmux redraw can disagree about wrapped rows,
leaving duplicated, interleaved, or missing fragments on screen.

The defect is deterministic across a `46 -> 51 -> 46` column cycle. The tmux
session remains healthy; only the browser terminal buffer is corrupted.

## Design

Keep the current terminal instance for height-only changes. When the proposed
fit changes columns after the WebSocket is connected, do not resize that xterm
instance. Debounce for 180 ms and reload the terminal iframe once after its
width stabilizes. The new page computes its target geometry before attaching,
and tmux sends one clean screen at that geometry.

Reloading affects only the ttyd client. The tmux session, panes, programs, pane
focus, working directories, and server-side scrollback stay alive. Repeated
resize events replace one pending timer, so dragging a splitter or rotating a
device does not create a reconnect storm.

## Alternatives Rejected

- Freeze the initial columns: stable but wastes space after rotation or split
  changes.
- Clear/reset xterm after each resize: can discard client scrollback and still
  races the tmux redraw.
- Resize on every animation frame: this is the current behavior and reproduces
  the corruption.

## Verification

- A JavaScript harness must prove row-only resize still calls `fit.fit()`.
- A column change must schedule exactly one reload and must not resize xterm.
- A second column-change event must replace the pending timer.
- Real Chromium validation must cycle `390 -> 430 -> 390` pixels against a
  temporary tmux session and show terminal rows identical to `tmux capture-pane`.
- Existing remote UI, touch, parsing, and shell tests must remain green.

## Release

Publish as backward-compatible patch `v1.5.1`. Restart only the user webterm
services so ttyd loads the new custom index; do not restart or kill tmux
sessions.
