# Task 3 Report: Dynamic App Shell

## Scope

Replaced the remote app's fixed mobile geometry with a dynamic two-row grid
shell. The change is limited to responsive outer layout, dynamic viewport
height, split-layout policy, and active-tab visibility. It does not add xterm
resize settling and does not start services or interact with tmux sessions.

## Files

- `dash/index.html`
  - Added `interactive-widget=resizes-content`.
  - Uses `--app-height`, scheduled from `visualViewport.height` in one rAF.
  - Replaced absolute narrow-shell positioning with the `#panes` two-row grid.
  - Applies each safe-area inset once on `#panes`.
  - Added coarse/fine split policy, stable coarse orientation handling, and
    resize/orientation listeners.
  - Added nearest-only active-tab reveal behavior.
- `dash/term.html`
  - Added `interactive-widget=resizes-content` to its viewport metadata.
- `tests/test_dashboard_layout.py`
  - Added shell geometry and touch-target regressions.
- `tests/test_remote_ui.py`
  - Added split-policy matrix, physical-portrait keyboard regression,
    active-tab reveal, and viewport scheduler tests.

## TDD Evidence

RED:

```text
pytest -q tests/test_dashboard_layout.py tests/test_remote_ui.py -k 'layout or split or active_tab'
4 failed, 5 passed, 47 deselected
```

The failures were the required missing viewport metadata, 44px tab target,
and absent split-policy functions. A separate active-tab regression then
failed because `revealActiveTab` was absent.

GREEN:

```text
pytest -q tests/test_dashboard_layout.py tests/test_remote_ui.py -k 'layout or split or active_tab'
10 passed, 47 deselected

pytest -q tests/test_dashboard_layout.py tests/test_remote_ui.py
58 passed in 1.29s
```

The portrait regression simulates a coarse device with physical screen
`834x1112`, `portrait-primary`, and visual viewport `834x420`; the layout
remains unsplit.

## Verification

```text
bash tests/test_js_parses.sh
OK

pytest -q
completed successfully (runner returned exit 0)

git diff --check
exit 0
```

## Self-review

- Coarse split is gated by both landscape orientation and a 748px width;
  fine-pointer split starts at 900px.
- Coarse orientation derives from stable `window.screen` / `screen.orientation`
  data, not the keyboard-reduced visual viewport height.
- Resize and rotation call `showView(activeView)` after toggling layout. This
  retains `activeView`, `activeTerm`, and existing `openTerms` iframe objects;
  `ensureFrame` only creates a frame when it is absent.
- `revealActiveTab` uses `scrollIntoView({block:"nearest", inline:"nearest"})`
  only when `activeView` changes, without assigning `scrollLeft`.
- Tabs have a 44px minimum target. The existing splitter pseudo-element keeps
  its 32px total hit target (`8px + 12px` on each side).
- No viewport-scaled font rules were added.

## Concerns

- No live device/browser visual pass was run; the keyboard-orientation case is
  covered deterministically in Node. Validate on target WebKit devices during
  release testing.
- xterm resize settling is intentionally out of scope for this task.

## Fix: active tab after rebuild

`revealActiveTab()` now checks the current active tab against the tabbar
bounds when `activeView` is unchanged. Fully visible tabs do not schedule
another reveal; a rebuilt tab that is partly or fully out of bounds schedules
nearest-only scrolling. The animation-frame callback queries the active tab
again and skips movement when it has become fully visible.

TDD evidence:

```text
pytest -q tests/test_remote_ui.py -k active_tab_reveal
.F.
1 failed, 2 passed, 53 deselected in 0.23s
```

The failing case was the unchanged, rebuilt, out-of-bounds active tab; the
previous unchanged-view guard scheduled no reveal.

```text
pytest -q tests/test_remote_ui.py -k active_tab_reveal
3 passed, 53 deselected in 0.10s

pytest -q tests/test_dashboard_layout.py tests/test_remote_ui.py
60 passed in 1.37s

bash tests/test_js_parses.sh
OK
```
