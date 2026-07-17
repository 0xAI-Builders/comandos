# Task 6 Report: Bruno Across Every Theme Surface

## Commits and Scope

- Supplied base: `9e3906e`
- Task 6 implementation: `d88e6fe Add Bruno theme across ComandOS`
- Initial verification report: `4ce8f9a Document Bruno theme verification`
- Reviewer follow-up: `842198a Strengthen Bruno theme integration coverage`
- Reviewer evidence report: `b1220f2 Document Task 6 review follow-up`
- Coverage closure: `b0619e2 Close Bruno theme coverage gaps`
- Branch: `feature/v1.6.0-mobile-bruno`

No services were restarted, no tmux sessions were touched, and no release or
Teach/skills work was performed.

Bruno remains the fifth and only new theme. The approved dashboard roles and
all 16 ANSI associations are exact. The accepted theme set is exactly
`noche`, `dia`, `calido`, `termius`, and `bruno` in the native registry,
custom xterm registry, dashboard sequence, and backend preference whitelist.

## Reviewer Follow-up Decisions

The iframe now has one named `handleTerminalMessage` listener. The handler
validates same origin, parent source, ComandOS message source, recognized type,
and recognized theme before dispatching either Task 5 interaction state or
Task 6 theme state. This is the only production change in `842198a`; it
consolidates the two already validated paths without changing frame, terminal,
session, or WebSocket lifecycle.

The real `ensureFrame` helper is executed with realistic DOM, `openTerms`, and
`resolveTermBase` stubs. Its initial primary iframe URL is asserted exactly as
`/term/?arg=dev%20%2F%20blue&theme=bruno`, and repeated calls retain both the
same frame object and the same session-map entry.

The existing ligature strategy remains dispose and recreate through the
addon's public constructor and `dispose()` API. The follow-up does not modify
`assets/xterm/addon-ligatures-web.js` and does not write private `_fg` or `_bg`
fields. A delayed-font harness now loads the actual addon class and proves
that rapid Noche -> Bruno -> Dia -> Bruno changes leave exactly one live
overlay, one resize listener, and one render listener. Late callbacks from
disposed addons cannot restore their overlays, and the WebSocket identity and
constructor count remain unchanged.

The pre-review implementation remains covered: `styleTermFrame` replaces its
injected important background and scrollbar colors on every live change,
uses `color-scheme:light` for Day and dark for the other themes, and themes the
paste dialog container, textarea, and both action buttons through roles.

The final coverage closure is test-only. It parses the exact
`:root[data-theme="bruno"]` block into a declaration map and compares the full
15-property set and values, including `--glow:#1A1A1A`, `--code:#8BC2F9`, and
`--shadow:rgba(0,0,0,.5)`. This replaces the previous global substring checks,
so missing, extra, duplicate, or wrong declarations fail.

The production-backed named listener test now evaluates the real custom xterm
`THEMES` object, asserts the complete Bruno xterm object and all nine terminal
role properties, and rejects `data.source != "comandos"`. The rejection is
asserted to leave the active theme, xterm theme object, role map, Task 5
interaction state, WebSocket identity, and constructor count unchanged. The
copied two-theme direct-handler test was removed after these assertions passed
against production unchanged.

## TDD Evidence

### RED

1. Initial Task 6 contract: `pytest -q tests/test_themes.py` reported
   `4 failed` before Bruno and the theme helpers existed.
2. Pre-review contract: `pytest -q tests/test_themes.py` reported
   `3 failed, 4 passed` for Day's stale dark color scheme, missing ligature
   lifecycle support, and unthemed paste dialog action buttons.
3. Reviewer follow-up contract:
   `pytest -q tests/test_themes.py tests/test_remote_ui.py -k 'theme or ligature or ensure_frame or named_terminal_message_bridge'`
   reported `1 failed, 10 passed, 64 deselected`. The failure proved the real
   page did not yet register exactly one named unified bridge listener.
4. Coverage-closure contract:
   `pytest -q tests/test_themes.py::test_bruno_dashboard_css_block_is_exact_and_all_ids_are_documented`
   reported `1 failed` because the selector-scoped declaration parser did not
   exist. After adding the parser, the same test reported `1 passed in 0.03s`.

### GREEN

1. The same reviewer-focused command reported
   `11 passed, 64 deselected in 0.35s` after the listener consolidation.
2. The expanded production-backed named-listener test reported
   `1 passed in 0.16s` without a production change.
3. Fresh focused verification:
   `pytest -q tests/test_themes.py tests/test_terminal_prefs.py tests/test_remote_ui.py`
   reported `82 passed in 3.67s`.
4. Fresh full verification: `pytest -q` reported
   `308 passed in 43.08s`.
5. `python3 -m py_compile bin/cc-app bin/cc-dash` exited `0`.
6. `bash tests/test_js_parses.sh` exited `0` with `OK`.
7. `git diff --check` exited `0`.

The focused and full totals each decreased by one because the copied duplicate
test was removed; no behavioral coverage was dropped.

The tests evaluate the real JavaScript theme objects, parse the real Python
registries and backend whitelist, execute the real parent/iframe helpers, load
the real ligature addon asynchronously, and execute the real native
`apply_theme` and `_apply_tmux_theme` functions. Native fakes observe exact
Bruno foreground, background, cursor, 16-color palette, GTK CSS, and tmux
status, border, activity, and message role values on every terminal/provider.

## Changed Files

Task 6 as a whole owns and changed:

- `bin/cc-app`
- `bin/cc-dash`
- `dash/index.html`
- `dash/term.html`
- `DESIGN.md`
- `tests/test_themes.py`
- `tests/test_remote_ui.py`
- `.superpowers/sdd/task-6-report.md`

Reviewer follow-up `842198a` changed only:

- `dash/term.html`
- `tests/test_themes.py`
- `tests/test_remote_ui.py`

Coverage closure `b0619e2` changed only:

- `tests/test_themes.py`
- `tests/test_remote_ui.py`

No addon asset or unrelated file was modified.

## Remaining Live Risks

- The native behavioral harness executes the production GTK/VTE/tmux apply
  functions with observable fakes, but it does not drive a real GTK main loop,
  VTE widget, or tmux server. Live native visual validation remains necessary.
- The parent and iframe helpers run against realistic JavaScript stubs, not an
  actual WebKitGTK instance. WebKit message scheduling, injected stylesheet
  timing, and scrollbar rendering can still differ from the harness.
- The delayed-font test uses the actual ligature addon lifecycle with a fake
  canvas and font object. Real font parsing and glyph painting should still be
  checked during rapid desktop theme changes.
- iOS WebKit and Android Chromium still need visual checks for all five themes,
  especially toolbar/dialog legibility, safe-area layout, and scrollbar color.
