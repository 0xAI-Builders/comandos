# Task 4 Report: Stabilize Remote Terminal Resize

## Implementation Commit

- `768f5eaa8785ebe1514f0899d84e8a78e0b54ffe` - `Stabilize remote terminal resize on touch devices`

## RED Evidence

Before changing `dash/term.html`, ran:

```text
pytest -q tests/test_remote_ui.py -k 'resize or geometry or column or keyboard'
```

Result: `1 failed, 11 passed, 44 deselected`. The new keyboard-animation
regression failed as intended: the previous requestAnimationFrame-only path
made `15` fits where the contract requires `1`.

## GREEN Evidence

After implementing the two-window coordinator, the focused command reported:

```text
14 passed, 44 deselected
```

Coverage includes 15 height-only observations settling to one fit and PTY
resize, 180 ms reloads for stable `390 -> 430 -> 390` column changes without
fitting the old terminal, selection deferral retaining the latest width,
disposal of queued resize work, and direct terminal input delivery.

## Full Verification

The direct `pytest -q` command was started twice, but this runner truncates
the foreground process near its 30 second session limit before a final summary.
All 292 collected tests were then verified in bounded groups:

```text
pytest -q <dashboard, desktop, remote groups>  139 passed in 24.93s
pytest -q <snippets, status, prefs groups>      63 passed in 13.68s
pytest -q <usage, webterm groups>               90 passed in 1.21s
bash tests/test_js_parses.sh                    OK
```

Total: `292 passed`.

## Files Changed

- `dash/term.html`
- `tests/test_remote_ui.py`
- `.superpowers/sdd/task-4-report.md`

## Remaining Live-Device Risks

- Verify Android Gboard and iOS Safari keyboard animations against the real
  ResizeObserver cadence and pixel rounding from xterm FitAddon.
- Verify selecting terminal text through a tablet orientation change preserves
  the selection until the deferred column reload executes.
- Verify pagehide and an actual ttyd socket close cancel resize work on real
  browser lifecycle implementations.
