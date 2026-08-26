# PerezOS Task 8 Implementer Report

## Status

DONE

## What I implemented

- Loaded `core`, `art`, `rig`, `behaviors`, `motion`, `renderer`, and `engine`
  as ordered classic scripts before the dashboard's inline runtime.
- Added one semantic keyboard-accessible `.perezos-stage` button containing one
  transparent `.perezos-canvas`; the stage is rebuilt only when the selected
  session identity changes.
- Added persistent `CENTRO_VIEW` lifecycle state. Same-session status, role,
  model, costume, context pressure, expansion, timestamp, and theme changes now
  update the live controller through `setContext()` without replacing the
  Control Center shell or canvas.
- Made Control Center action handlers read `CENTRO_VIEW.item` at activation
  time so polling cannot leave stale session closures. The async brain panel
  also revalidates the selected session after its request completes.
- Connected pointer movement in stage-local coordinates and native button
  click/Enter/Space activation to `notifyInteraction()`. Accepted activation
  shows one bounded, localized PerezOS phrase.
- Preserved role hats as authored PerezOS props (`visor`, `casco`, `corona`),
  while detector `costume` values retain priority and backend JSON contracts
  remain unchanged.
- Added immediate theme-context refresh with fresh `--brand`, `--panel`, and
  `--line` colors without recreating the controller or resetting its pose.
- Added `cc-axo` read-only one-time migration to `cc-mascot`, truthful
  `sw-mascot` `aria-checked`, and a `.no-mascot` policy that hides only the
  stage and stops engine scheduling.
- Added responsive external CSS for the 256x208 desktop and 180x148 narrow
  stage, transparent pixelated canvas, fixed panel cable anchor, front panel
  occlusion edge, brand focus ring, and no animation keyframes or halo.
- Removed the procedural canvas character, ANSI pixel map, palettes, timers,
  random transform moves, old CSS animations, old labels, and old phrases.
- Bumped the service-worker shell to `comandos-shell-v3` and updated its stale
  dashboard test expectation.
- Renamed only the requested backend comments; `costume` response fields and
  detector values were not changed.
- Added one preallocated, validated accent palette that maps dashboard
  `brand`/`panel`/`line` colors to the rim, deep-shadow bias, cable node, and
  prop markings. The full execution cable retains its authored metal palette.
  Accent-only changes redraw exactly once without rebuilding an atlas or
  replacing the controller, rig, renderer, or Motion state.
- Compiled editable role `matches` entries as case-insensitive regular
  expressions, ignored malformed patterns without throwing, and classified
  current `gpt-5.4`, `gpt-5.5`, `gpt-5.6-sol`, and `glm-4.6` models into their
  authored PerezOS role props.
- Replaced source-shape-only same-session coverage with a behavioral Node
  harness that executes two real `renderCentro()` calls and asserts exact
  canvas/controller identity, one shell construction, and live second context.

## TDD evidence

### RED

Command:

```text
pytest -q tests/perezos/test_integration.py tests/test_dashboard_layout.py
```

Expected result before integration:

```text
9 failed, 8 passed
- PerezOS runtime scripts and stylesheet absent
- semantic persistent canvas absent
- same-session lifecycle/context contract absent
- preference migration absent
- old axolotl runtime and animation artifacts still present
- responsive stage CSS absent
- service-worker shell still v2
```

### GREEN

Command:

```text
pytest -q tests/perezos/test_integration.py tests/test_dashboard_layout.py \
  tests/test_dashboard_cards.py tests/test_terminal_prefs.py \
  tests/test_remote_ui.py::test_remote_routes_are_never_served_from_stale_shell_cache \
  && bash tests/test_js_parses.sh
```

Result:

```text
30 passed
OK
```

### Formal-review remediation RED/GREEN

RED commands:

```text
node --test --test-name-pattern='five dashboard themes|color-only context|dashboard accent changes' \
  tests/perezos/test_renderer.js tests/perezos/test_engine.js
pytest -q tests/perezos/test_integration.py -k 'same_session or real_model_regexes'
```

Observed RED before the production fix:

```text
2 JS failures: theme accents absent; color-only changes skipped as clean
1 Python failure: gpt-5.4 pattern treated as a literal substring
```

The behavioral same-session test passed on the real implementation, while the
reviewer's unconditional-`innerHTML` mutation exited nonzero with
`same-session Control Center shell rebuilt`.

Fresh GREEN after remediation:

```text
3/3 focused JS tests passed
3/3 focused Python tests passed
```

Review follow-up also exercised a RED in which the full cable was incorrectly
theme-colored and the color helper contained a render-time regex literal.
GREEN restores the authored light/dark cable-metal index, bounds theme colors
to the allowed rim/deep-shadow/node/prop pixels, and reuses one module-level
hex pattern covered by the expanded hot-path source audit.

### Local dev-proxy security remediation

The real `devhost` checkpoint reproduced the integration bug before the fix:

```text
http://comandos-perezos.localhost/       -> 403 Host no permitido
http://comandos-perezos.localhost/state  -> 403 Host no permitido
http://127.0.0.1:7383/                   -> 200
http://127.0.0.1:7383/state              -> 200
```

Five incremental RED/GREEN cycles now cover:

- RFC-style `localhost` subdomains and bounded ports, while malformed labels,
  arbitrary suffixes, paths, and invalid ports remain rejected;
- tokenless local proxy access only when the socket peer is loopback, Host is
  `localhost`/`*.localhost`, Origin remains allowed, and every IP in XFF is
  loopback, including IPv4-mapped IPv6;
- duplicate XFF header fields as one security chain, so a non-loopback value in
  any field keeps the token requirement.
- empty, whitespace-only, or partially empty XFF fields as present-but-invalid
  proxy metadata rather than accidentally treating them as a direct request;
- exactly one Host, at most one nonempty Origin, direct tokenless authority
  restricted to localhost or loopback literals, and the 253-character RFC
  hostname boundary.

Remote/tailnet hosts, non-loopback peers, malformed XFF, mixed XFF chains, and
local-looking IP Hosts with XFF still require the existing token. Foreign
Origins still fail before authentication. The server bind remains loopback.

## Verification

- Focused Task 8 dashboard/integration suite: 32/32 passing.
- `bash tests/test_js_parses.sh`: `OK`.
- `node --test tests/perezos/test_*.js`: 199/199 passing (181 top-level
  tests plus nested cases).
- `pytest -q`: 398/398 passing in 105.66 seconds.
- `git diff --check`: passing.
- Formal read-only remediation re-review: `Ready: Yes`, with no remaining
  Critical or Important findings after restoring authored cable metal and
  moving the hex validator pattern out of the render path.
- Local proxy security unit suite: 17/17 passing.
- Remote/security regression suite: 141/141 passing.
- Formal local-proxy security re-review: `Ready: Yes`, with 0 remaining
  Critical or Important findings after covering ambiguous headers, tailnet
  direct access, and the RFC hostname-length boundary.
- Source scan: no old character art/name/runtime/CSS/phrase remains in the
  active dashboard; the single `cc-axo` occurrence is the required read-only
  migration read.

## Files changed

- `dash/perezos/perezos.css` (new)
- `tests/perezos/test_integration.py` (new)
- `dash/index.html`
- `dash/perezos/renderer.js`
- `config/agent-roles.json`
- `dash/sw.js`
- `tests/perezos/test_engine.js`
- `tests/perezos/test_renderer.js`
- `tests/test_dashboard_layout.py`
- `tests/test_remote_ui.py` (direct SW v3 expectation update)
- `bin/cc-dash` (Task 8 backend comment plus localhost proxy security gate)
- `bin/cc-app` (comment only)
- `tests/test_dashboard_security.py` (new)
- `.superpowers/sdd/perezos-task-8-report.md` (new)

## Self-review

- Exactly one mascot controller/canvas is mounted for the current selection.
  No-selection and session-switch paths destroy the previous controller;
  same-session updates preserve its canvas, rig, observers, pose, and memory.
- Interaction listeners are attached only when a new stage is mounted and are
  discarded with that DOM subtree. Engine visibility/resize/intersection/media
  listeners remain owned by the one controller and are released by `destroy()`.
- The canvas is the only moving character DOM node. There are no per-body-part
  elements, CSS motion loops, new network calls, or pointer capture.
- Theme, role, costume, expansion, and status reach the current controller as
  stable primitive context. Detector costume values override role props rather
  than changing their backend meaning.
- Theme accents are six-digit hex values before they can reach Canvas
  `fillStyle`; malformed direct-renderer colors fail closed before touching
  Canvas state. The body remains authored atlas artwork, while five dashboard
  themes share two cached light/dark pages and vary only bounded accent pixels.
- The steady render path compares stable accent strings without creating a new
  palette object or regular expression. A color diff increments
  `accentRevision` once; the next identical frame skips all Canvas operations.
- The same-session regression harness is mutation-sensitive: the exact
  unconditional-`innerHTML` mutation described by review fails on its shell
  construction count.
- Existing session text and actions remain authoritative and visible when the
  mascot preference is off.
- Local proxy trust is conjunctive rather than header-based: a remote or mixed
  XFF chain cannot become tokenless merely by presenting a `*.localhost` Host.
  Tailscale token and Origin anti-CSRF behavior is unchanged.

## Real checkpoint for screenshot

From this worktree, use an unused port so the installed dashboard on 4777 is
not disturbed:

```text
./bin/cc-dash 4788 --no-open
```

Then open `http://127.0.0.1:4788/`, select a real session row, and capture the
Control Center. The endpoint serves this worktree's real `dash/index.html`,
PerezOS modules, and CSS; no preview shim is involved. Narrow verification can
use a 390x844 browser viewport. Stop the checkpoint with Ctrl+C.

## Risks / follow-up

- Task 9 still owns real-browser visual baselines, click/keyboard automation,
  lifecycle leak checks, and 30-second timing/payload enforcement. None of
  those harnesses were added here.
- A browser with neither `ResizeObserver` nor modern optional chaining is
  outside the current dashboard/runtime baseline; supported Chromium/WebKit
  receives the responsive viewport update through the engine observer.
