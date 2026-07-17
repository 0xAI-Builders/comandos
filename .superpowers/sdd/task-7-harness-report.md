# Task 7 Browser Harness Report

## Scope

Implemented and hardened `tests/e2e_mobile_remote.js`, added permanent offline
coverage in `tests/test_e2e_harness.py`, and added an explicitly restricted
ephemeral tab-close path in `bin/cc-dash` with regression coverage in
`tests/test_usage_dash.py`. The final gate exercised the real dashboard, both
ttyd endpoints, the Tailscale route, system Chrome, the native GTK/VTE client,
and one nonce-owned disposable tmux session.

## Safety Model

- `new-session` is marked attempted before invocation, creates a 256-bit
  random `COMANDOS_E2E_OWNER_NONCE` session environment variable, and requests
  exact `$session_id|@window_id|%pane_id` output with `-P -F`.
- Cleanup probes the exact `=session` name and nonce. Once `$session_id` is
  known, session options and cleanup target that immutable ID. A timed-out
  creation can be removed by name only when its nonce matches; a pre-existing
  collision is rejected and never killed.
- Send, split, pane-kill, mouse-mode, and restore paths re-probe ownership
  immediately before mutation. `capture-pane -J` targets only the recorded
  primary pane and preserves wrapped reader output as one logical line.
- Registration uses four explicit states: `not-attempted`, `uncertain`,
  `confirmed`, and `rejected`. Confirmed registrations are always closed;
  uncertain registrations close only while nonce ownership remains proven;
  rejected registrations are never closed.
- E2E cleanup calls `/tab-close` with `ephemeral: true`. The backend accepts
  that flag only for `comandos-e2e-*`, removes the mirrored/native tab, and
  skips the user's Recents history. Normal tab closing still records history.
- The dashboard token is used only in the initial URL and authenticated API
  headers. The URL query is removed before screenshots, and diagnostics redact
  raw and URL-encoded token forms.

## Behavioral Coverage

- Four touch contexts cover `320x568`, `390x844`, `834x1112`, and
  `1194x834`, including expected single/split policy, positive geometry,
  toolbar/tab target sizes, overlap checks, and screenshots.
- The init script wraps the real xterm constructor and records its actual
  public buffer. Geometry and selection locate known markers in that buffer,
  then require foreground pixels inside the exact marker canvas region. Cursor
  pixels elsewhere cannot satisfy the check.
- Height-only viewport changes verify ordered input while iframe identity,
  load count, and frame/page WebSocket creation counts remain stable.
- Raw tty readers enter raw mode before announcing READY and restore termios in
  `finally`. Toolbar bytes are exact. Clipboard success applies xterm's
  `LF -> CR` paste normalization, and manual paste uses an exact fixed-length
  reader.
- Cancel, Escape, and empty clipboard each run a three-second absolute raw tty
  observation. The UI action must finish with at least 500 ms remaining, then
  the reader continues to its deadline and must report zero bytes.
- Touch toolbar pan, terminal scroll, text selection/copy, and both pane-resize
  axes operate only on the disposable session. Because tmux uses xterm's
  alternate buffer, scroll is proven through tmux copy-mode and a positive
  `scroll_position`, then explicitly returned to live mode. Every CDP contact
  ends or cancels from `finally`.
- Selected `Ctrl+C` is intercepted by xterm, writes the exact selection to the
  browser clipboard, and does not send SIGINT. With no selection, the shortcut
  remains terminal input. Pane resize targets the immutable created pane ID and
  is independent of the user's tmux `base-index`.
- Every intermediate theme click waits for both dashboard state and terminal
  CSS propagation. On a phone it reaches the visible control through the
  `Panel` tab and returns to the same terminal instead of force-clicking a
  hidden element. Bruno/Day/Bruno checks preserve iframe, socket, and tmux
  client counts.
- Playwright loads lazily. Normal Node resolution is preferred; when
  `NODE_PATH` is unset, the runtime resolves `npm root -g` explicitly. Import
  performs no Playwright discovery or subprocess call.

## Offline Verification

```text
$ pytest -q tests/test_usage_dash.py
20 passed in 0.06s

$ pytest -q tests/test_e2e_harness.py
9 passed in 0.14s

$ pytest -q
320 passed in 43.83s

$ bash tests/test_codex_adapters.sh
(exit 0)

$ bash tests/test_doctor.sh
ok

$ bash tests/test_js_parses.sh
OK

$ bash tests/test_platform.sh
ok

$ bash -n bin/cc-webterm bin/cc-mobile bin/cc-doctor
(exit 0)

$ python3 -m py_compile bin/cc-app bin/cc-dash bin/cc_usage.py
(exit 0)

$ node --check tests/e2e_mobile_remote.js
(exit 0)

$ node -e / call loadPlaywright() and require its resolved package version /
1.58.2

$ git diff --check
(exit 0)
```

The Chrome toolbar fixture now waits for Chrome to exit and retries profile
cleanup, eliminating its prior `ENOTEMPTY` teardown race. The first complete
post-live run exposed one missing `attachCustomKeyEventHandler` method on a Node
test double (`319 passed, 1 failed`); the isolated regression then passed and
the fresh complete run above passed all 320 tests.

## Live Findings And Fixes

- tmux 3.2a returned no session ID for the original ownership query. The probe
  now resolves exact `session_name|session_id` pairs through `list-sessions`;
  the one harness-owned orphan from that failed attempt was nonce-verified and
  removed without touching user sessions.
- Iframe `load` does not bubble to `window`; instrumentation now attaches a
  direct listener to every inserted iframe through a `MutationObserver`.
- A zero-byte tty result was previously treated as a false polling result. The
  parser now distinguishes a valid empty hex payload from a missing marker.
- The touch toolbar was wider than the phone document, shifting the page left.
  The terminal shell now constrains every grid child to the viewport and keeps
  horizontal overflow inside the toolbar itself.
- The original scroll assertion assumed xterm normal-buffer history. Live
  diagnostics proved `bufferType=alternate`, `rows=bufferLength=37`, and mouse
  tracking `drag`; the corrected check observed tmux `scroll_position=30`.
- xterm consumed selected `Ctrl+C`, cleared the selection, and emitted no copy
  event. The production key handler now copies the exact selected marker while
  preserving ordinary `Ctrl+C` when no selection exists.
- Resize assumed pane index zero, but the live tmux configuration starts at
  index one. The harness now uses the exact pane ID returned by `new-session`.

## Live Verification

```text
Remote E2E: ok=true, disposable session comandos-e2e-307694
Viewports: 320x568 single, 390x844 single, 834x1112 single,
           1194x834 split; one active frame in every viewport
Toolbar pan: scrollLeft=130; no canceled touchmove
Terminal history: live -> copy-mode scroll_position=30 -> live
Selection copy: SELECT-COPY-307694 copied exactly
Pane resize: horizontal 23->27 columns; vertical 17->21 rows
Theme: Bruno -> Day -> Bruno; iframe/socket identity unchanged;
       tmux clients 1 -> 1
Screenshots: all four exact dimensions, nonblank standard deviation
             0.0541 through 0.0837, visually inspected with no overlap
Native Bruno: 1600x880, nonblank standard deviation 0.1298
Crash recovery: primary ttyd PID 210393 -> 217733; disposable pane survived
Final services: dashboard 208148, fallback ttyd 288780,
                primary ttyd 288782, native app 320254
Remote state: active, both routes healthy/reachable, not degraded, QR available
Pane inventory: 33 lines, before/after SHA-256
                8664bed266f404e0360727c9d3f94b36fcf3319336df99b04e0c0a409685b10b
Session inventory: 15 lines, before/after SHA-256
                   46edf4980827f5cd0f19807762aa45bb5cbe0b8c0f607bacc81d1a1c34dc5f47
Cleanup: no comandos-e2e session, mirror entry, Recents entry, or event file
```
