# Task 7 Browser Harness Report

## Scope

Implemented and hardened `tests/e2e_mobile_remote.js`, added permanent offline
coverage in `tests/test_e2e_harness.py`, and added an explicitly restricted
ephemeral tab-close path in `bin/cc-dash` with regression coverage in
`tests/test_usage_dash.py`. No live browser, service, API, or tmux command was
run while building or reviewing this harness.

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
  axes operate only on the disposable session. Every CDP contact ends or
  cancels from `finally`.
- Every intermediate theme click waits for both dashboard state and terminal
  CSS propagation. Bruno/Day/Bruno checks preserve iframe, socket, and tmux
  client counts.
- Playwright loads lazily. Normal Node resolution is preferred; when
  `NODE_PATH` is unset, the runtime resolves `npm root -g` explicitly. Import
  performs no Playwright discovery or subprocess call.

## Offline Verification

```text
$ pytest -q tests/test_usage_dash.py
20 passed in 0.06s

$ pytest -q tests/test_e2e_harness.py
5 passed in 0.14s

$ pytest -q
315 passed in 41.90s

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

The first complete pytest attempt reached `314 passed` plus one teardown-only
failure: an existing Chrome test produced its valid result and then hit
`ENOTEMPTY` while deleting a still-flushing temporary profile. The isolated
test immediately passed (`1 passed in 1.43s`), and the fresh complete run above
passed all 315 tests.

## Live Status

Not run by design. The next gate is a read-only re-review of this corrected
code. Only after approval may the operator merge, inventory existing panes,
restart the three scoped services/processes, exercise crash recovery, and run
the harness against the real Tailscale URL.
