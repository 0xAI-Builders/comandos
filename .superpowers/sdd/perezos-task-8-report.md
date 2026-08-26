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

## Verification

- Focused Task 8 dashboard/integration suite: 30/30 passing.
- `bash tests/test_js_parses.sh`: `OK`.
- `node --test tests/perezos/test_*.js`: 196/196 passing (178 top-level
  tests plus nested cases).
- `pytest -q`: 396/396 passing in 105.17 seconds.
- `git diff --check`: passing.
- Source scan: no old character art/name/runtime/CSS/phrase remains in the
  active dashboard; the single `cc-axo` occurrence is the required read-only
  migration read.

## Files changed

- `dash/perezos/perezos.css` (new)
- `tests/perezos/test_integration.py` (new)
- `dash/index.html`
- `dash/sw.js`
- `tests/test_dashboard_layout.py`
- `tests/test_remote_ui.py` (direct SW v3 expectation update)
- `bin/cc-dash` (comment only)
- `bin/cc-app` (comment only)
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
- Existing session text and actions remain authoritative and visible when the
  mascot preference is off.

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
