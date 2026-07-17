# Task 6 Report: Bruno Across Every Theme Surface

## Implementation

- Implementation commit: `d88e6fe Add Bruno theme across ComandOS`
- Branch: `feature/v1.6.0-mobile-bruno`
- Base supplied for this task: `9e3906e`
- No services were restarted and no tmux sessions were touched.

Bruno is registered as the fifth and only new theme in GTK/VTE/tmux,
dashboard preferences, dashboard CSS, custom xterm, navigation, and design
documentation. The approved dashboard roles and all 16 ANSI values are
asserted exactly.

Existing custom terminal frames receive `{source:"comandos",type:"theme"}`
only when they are non-compat frames. The iframe validates origin, source,
message source, type, and theme id before applying a theme. The live path
changes `term.options.theme`, terminal role CSS, root/body/shell backgrounds,
and the parent-injected scrollbar/background CSS without reloading,
replacing a frame, or reconnecting a WebSocket.

The desktop ligature overlay is recreated through its public constructor
options and `dispose()` method when a live theme changes. No private
`_fg`/`_bg` fields or addon asset API were changed. Touch terminals do not
create the overlay, so their live theme path has no overlay work.

## TDD Evidence

### RED

1. Initial Task 6 contract: `pytest -q tests/test_themes.py` reported
   `4 failed` because Bruno and the theme helpers were absent.
2. Pre-review additions: `pytest -q tests/test_themes.py` reported
   `3 failed, 4 passed` for Day still forced to `color-scheme:dark`, missing
   ligature lifecycle helpers, and unthemed paste dialog action buttons.

### GREEN

1. Focused theme, preference, and remote UI verification:
   `pytest -q tests/test_themes.py tests/test_terminal_prefs.py tests/test_remote_ui.py`
   reported `80 passed in 2.94s`.
2. `python3 -m py_compile bin/cc-app bin/cc-dash` exited `0`.
3. `bash tests/test_js_parses.sh` exited `0` with `OK`.
4. Full collection reported `306 tests collected`. The execution runner
   terminates a direct `pytest -q` before its summary (it reached the snippets
   boundary at about 52% with no pytest failure). The same collected suite was
   therefore run in ordered cohorts: `160 passed in 28.45s` and
   `146 passed in 17.25s`, totaling `306 passed`.

Focused Node harnesses execute the real parent broadcaster, `styleTermFrame`,
iframe theme apply/message helpers, and ligature lifecycle helpers. They prove
Bruno updates xterm and terminal UI roles, parent style replacement updates
stale `!important` values, Day becomes light, invalid messages do nothing,
and WebSocket identity/count plus Task 5 interaction state remain unchanged.

## Changed Files

- `bin/cc-app`
- `bin/cc-dash`
- `dash/index.html`
- `dash/term.html`
- `DESIGN.md`
- `tests/test_themes.py`
- `tests/test_remote_ui.py`
- `.superpowers/sdd/task-6-report.md`

## Remaining Live Risks

- GTK/VTE/tmux live recoloring relies on existing `apply_theme()` and
  `_apply_tmux_theme()` behavior. Registry and syntax coverage pass, but no
  live native GTK session was driven during this task.
- WebKit may schedule an iframe load/style update differently from Chromium.
  The initial URL carries the selected theme and the parent updates the
  existing injected stylesheet, but live Day/Bruno rendering should be checked
  in WebKitGTK on a real device.
- The ligature overlay is recreated without reconnecting the socket. Its font
  load is asynchronous, so desktop visual validation should confirm no stale
  overlay is visible during rapid repeated theme changes.
- Mobile touch terminals intentionally skip the ligature overlay. Verify the
  toolbar, paste dialog buttons, scrollbar, and safe-area layout for all five
  themes on iOS WebKit and Android Chromium.
