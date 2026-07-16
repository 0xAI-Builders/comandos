# Mobile and Tablet Reliability with Bruno Theme Design

**Date:** 2026-07-16
**Status:** Approved

## Objective

Make the remote ComandOS terminal dependable and efficient on small phones and
touch tablets, add a compact terminal key toolbar, and add Bruno as one new
theme across every existing surface. Preserve tmux sessions and the current
desktop and remote workflows.

The separate `Teach this change` learning concept is documented here only as a
future direction. This release must not add learning UI, APIs, storage, prompts,
skills, or background work.

## Confirmed Failure Modes

The investigation reproduced and traced four independent failures:

1. `cc-webterm-path.service`, which owns the preferred same-origin `/term`
   endpoint and custom mobile client, crashed in ttyd 1.6.3 with
   `corrupted double-linked list`. It was a transient systemd unit without a
   restart policy, so `/term` remained at HTTP 502 until manually restarted.
2. Remote health remained green because `webterm_running()` accepted any ttyd
   process. The fallback process on port 4779 was alive even though the primary
   process on port 4780 was gone. Tailscale route configuration was also
   treated as endpoint health.
3. `resolveTermBase()` silently selected the cross-origin fallback. That page
   is ttyd's legacy client, so it does not contain ComandOS's GBoard,
   selection, pane-resize, reflow, or theme behavior.
4. An iPhone-sized browser run produced 15 PTY resize messages during 15
   simulated keyboard-height transitions while input continued. The current
   requestAnimationFrame coalescing limits work per frame, but does not settle
   a multi-frame keyboard animation. The outer narrow layout also depends on
   `100vh` and a hard-coded `top:44px` offset instead of the actual tab-bar and
   visual viewport geometry.

The preferred `/term` service was restored during diagnosis. That operational
restart is not the permanent fix defined below.

## Scope

### Included

- Supervise both ttyd processes and recover automatically after crashes.
- Report primary and fallback health independently and expose a degraded state.
- Make narrow phone and touch-tablet layout follow the visual viewport and safe
  areas without fixed tab-bar offsets.
- Settle virtual-keyboard height changes before resizing xterm and the PTY.
- Preserve the existing stable-width reattach for real column changes.
- Add a touch terminal toolbar with interaction mode, Esc, Ctrl, Tab, arrows,
  and paste.
- Improve active-tab visibility and touch target sizing on phones and tablets.
- Add a Bruno theme to dashboard, GTK chrome, VTE, xterm, and tmux chrome.
- Add deterministic unit, integration, service-recovery, and browser checks.

### Excluded

- Replacing ttyd or implementing a new terminal/WebSocket gateway.
- Caching terminal input or dashboard state.
- Input batching, predictive echo, or speculative local terminal output.
- Replacing tmux, changing session identity, or restoring sessions differently.
- Installing or invoking Matt Pocock's `teach` skill.
- Implementing `Teach this change`, a decision ledger, lessons, quizzes, or
  learning records.
- General dashboard redesign or component restyling beyond responsive fixes and
  the additional theme values.

## Architecture

Keep the current tmux + ttyd + xterm.js architecture. Reliability belongs at
three existing boundaries:

1. **Service lifecycle:** `cc-webterm` starts and supervises the root fallback
   and same-origin path endpoint.
2. **Remote health:** `cc-mobile`, `cc-dash`, and the remote controls distinguish
   route configuration from a responding backend.
3. **Terminal client:** `dash/index.html` owns responsive app layout and
   interaction state; `dash/term.html` owns xterm geometry, touch input, and the
   terminal toolbar.

No new daemon or persistence layer is required.

## Service Lifecycle and Health

### Supervision

When systemd is available, both transient ttyd services must be created with:

- `Restart=on-failure`
- `RestartSec=1s`

Stopping either service explicitly through `cc-webterm off` must remain final;
systemd must not restart a user-stopped unit. The non-systemd `setsid` fallback
retains current behavior.

`cc-webterm` startup succeeds only after both probes respond:

- fallback: `http://127.0.0.1:4779/token`
- primary: `http://127.0.0.1:4780/term/token`

`cc-webterm status` reports `active`, `degraded`, or `off`. `active` requires
both probes, `degraded` means exactly one responds, and `off` means neither
responds. A process match alone is diagnostic evidence, not health.

### Remote state

`cc-dash` must expose the two endpoint-health values separately from the two
Tailscale route-presence values. The existing overall terminal state is healthy
only when the primary endpoint and `/term` route both respond/present. The
fallback is reported independently.

The remote drawer displays:

- `Activo` when the dashboard, primary endpoint, and primary route are healthy.
- `Degradado` when the dashboard works but only the fallback terminal is
  reachable.
- `Apagado` when no terminal endpoint is reachable.

If the primary endpoint fails while the page is open, terminal resolution may
use the fallback for emergency access, but the page must show one degraded
notification and must not present the compatibility client as fully healthy.
Systemd supervision is responsible for restoring the primary endpoint.

## Responsive App Shell

The dashboard and terminal viewport metadata must include
`interactive-widget=resizes-content` while retaining `viewport-fit=cover`.

For narrow mode:

- Use `100dvh` when supported.
- Maintain a CSS `--app-height` fallback from `visualViewport.height` for
  browsers with incorrect dynamic viewport units.
- Replace absolute `top:44px` panel/terminal placement with a two-row grid:
  actual tab-bar height followed by `minmax(0, 1fr)` content.
- Apply top and bottom safe-area insets once; do not duplicate the top inset in
  both the app shell and tab bar.
- Keep the panel and terminal mutually exclusive on phones.
- Keep every primary touch target at least 40 px, with 44 px used when the
  surrounding bar permits it.
- Scroll the newly active tab into the nearest visible horizontal position
  without centering or shifting the entire tab strip unnecessarily.

For tablet mode:

- Portrait remains single-view when a panel plus terminal cannot preserve a
  300 px panel and a 440 px terminal.
- Landscape may use the existing split composition when both minima fit.
- The visual splitter remains narrow, but its touch hit target remains at least
  32 px.
- Rotation must retain the active terminal and session.

No font size may scale with viewport width.

## Terminal Resize Coordinator

`dash/term.html` remains the sole owner of xterm and PTY dimensions.

Geometry observations come from a `ResizeObserver` on the terminal container.
`visualViewport.resize` and `window.resize` are compatibility signals that
schedule the same coordinator; they must not each perform an independent fit.

The coordinator follows these rules:

1. Identical proposed rows and columns produce no work.
2. Height-only changes settle for 120 ms from the latest observation, then call
   `fit.fit()` once. The resulting `term.onResize` sends one deduplicated ttyd
   resize message.
3. A real column change retains the existing 180 ms stabilization window and
   reloads only the terminal iframe once at the final width. It does not resize
   the old xterm instance first.
4. A pending column reload is replaced when a newer width arrives.
5. Active browser selection continues to defer any column reload until the
   selection clears.
6. Page hide, WebSocket close, and disposal cancel pending frame and timer work.
7. Opening or closing the mobile toolbar changes available terminal height and
   follows the same height-only settling path.

This preserves the v1.5.1 wrapped-row guarantee while preventing a virtual
keyboard animation from forcing a tmux redraw on every frame.

Terminal input remains direct. Characters are never delayed behind the resize
settling timer.

## Touch Terminal Toolbar

The toolbar is rendered by the custom terminal page when a coarse pointer or
touch capability is present. It occupies layout space below xterm and above the
bottom safe area; it never overlays terminal cells.

Control order:

`Interact/Select`, `Esc`, `Ctrl`, `Tab`, `Left`, `Up`, `Down`, `Right`, `Paste`

The bar may scroll horizontally on widths that cannot fit all controls. Each
control has stable dimensions, an accessible label, and a tooltip where the
meaning is not visible. Arrow and paste controls use the existing Lucide-style
icon system; Esc, Ctrl, and Tab use their conventional key labels.

### Key behavior

- Esc sends `\x1b`.
- Tab sends `\t`.
- Arrow controls send the standard xterm cursor escape sequences.
- Ctrl is a visible one-shot modifier. When armed, the next eligible single
  ASCII key from the software keyboard is converted to its control byte and
  Ctrl disarms. Composition strings and multi-character input are sent normally
  and do not consume the modifier.
- Toolbar pointer-down handling preserves terminal focus. After a command, the
  xterm helper textarea is refocused without scrolling the outer page.
- Paste uses `navigator.clipboard.readText()` in the secure context and passes
  text through xterm's paste path so bracketed-paste mode is honored. If browser
  permission is denied, a compact paste dialog accepts manual text and performs
  the same xterm paste operation.

### Interaction mode

The existing backend-owned tmux mouse state remains the source of truth.
`Interact/Select` in the toolbar requests a mode change from the parent
dashboard and updates only after the existing `/tmux-mouse` flow confirms it.
It must retain the current pending, failure, tab-switch, page-hide, and restore
semantics. The old floating mode button is removed so it cannot cover terminal
text or disagree with the toolbar.

## Bruno Theme

Add exactly one theme with id `bruno` and display name `Bruno`. Do not change
component geometry, typography, copy, or navigation as part of this theme.

The base mapping follows Bruno's official current dark palette:

| ComandOS role | Value |
| --- | --- |
| `bg` | `#1A1A1A` |
| `panel` / bar | `#222224` |
| `panel2` | `#26292B` |
| `line` | `#333333` |
| `line2` | `#444444` |
| `text` / terminal foreground | `#CCCCCC` |
| `dim` | `#AAAAAA` |
| `faint` | `#999999` |
| `brand` / cursor | `#E4AE49` |
| `waiting` | `#F6AB79` |
| `done` | `#73E89A` |
| `working` / code | `#8BC2F9` |

Source: [Bruno dark theme palette](https://github.com/usebruno/bruno/blob/main/packages/bruno-app/src/themes/dark/dark.js).

The theme must be accepted by the preferences API, added to the dashboard theme
sequence, represented in `bin/cc-app`, applied to VTE and tmux chrome, and
available in `dash/term.html`. Its 16-color ANSI palette is:

`#1A1A1A`, `#DA462F`, `#73E89A`, `#FAD075`, `#8BC2F9`, `#D691ED`,
`#7DDFF2`, `#CCCCCC`, `#666666`, `#F38172`, `#73E89A`, `#FAD075`,
`#8BC2F9`, `#D691ED`, `#7DDFF2`, `#FFFFFF`.

These values map Bruno's official red, green, yellow, blue, purple, cyan, rose,
surface, and text colors into the established VTE palette order.

The dashboard passes the active theme when creating a terminal iframe. Existing
same-origin terminal frames receive a theme message and update xterm options,
page background, cursor, and selection without reconnecting the WebSocket.
This live propagation also fixes the existing mismatch for Termius. A fallback
compatibility iframe may remain visually limited, but it is explicitly marked
degraded as described above.

Update `DESIGN.md` to list all five themes and use `bruno` in every theme
whitelist and validation path. The theme button tooltip and localized toast
must list/name Bruno without adding a new settings flow.

## Error Handling

- A failed service probe is a degraded/off state, never a successful cached
  process state.
- Health probe timeouts are bounded and do not block `/state` or terminal
  input.
- Clipboard denial preserves terminal contents and opens the manual paste
  fallback without logging clipboard text.
- A failed interaction-mode request restores the confirmed mode and leaves key
  controls usable.
- A resize exception clears its scheduled state so a later observation can
  retry.
- A fallback switch does not close, kill, rename, or recreate the tmux session.

## Verification

### Automated

- Shell tests assert both systemd-run commands carry the restart properties and
  both health probes are required for active status.
- Backend tests distinguish route presence, primary health, fallback health,
  degraded status, and full recovery.
- JavaScript harnesses send 15 keyboard-height observations and require one
  final fit/PTY resize, no reload, and no lost input.
- Existing `390 -> 430 -> 390` column tests continue to require one reload per
  stable width and no old-instance fit.
- Toolbar tests cover every byte sequence, one-shot Ctrl, composition input,
  clipboard success/denial, focus retention, and confirmed interaction state.
- Theme tests require `bruno` across the backend whitelist, dashboard CSS and
  sequence, xterm themes, GTK/VTE dictionary, tmux application, and live frame
  propagation.
- The complete Python and shell suites, embedded JavaScript parse checks,
  `bash -n`, Python compilation, and `git diff --check` pass.

### Browser and service checks

Use a disposable tmux session and remove it after the run. Do not type into,
resize, close, or rename a user session.

- Phone viewports: 320x568 and 390x844.
- Tablet viewports: portrait 834x1112 and landscape 1194x834.
- Repeated keyboard-height transitions while typing retain every character and
  produce one stable resize.
- Rotation and width cycles preserve exact tmux capture output without duplicate
  wrapped text.
- Scroll, selection, vertical/horizontal pane resize, every toolbar key, Ctrl,
  and clipboard are exercised with touch input.
- Screenshots and element bounds show no overlap, clipped labels, blank canvas,
  or toolbar-covered terminal rows.
- Kill the primary ttyd process with an attached disposable session; systemd
  restarts it, `/term/token` returns 200 again, the remote state transitions
  degraded -> active, and the tmux session remains alive.
- Check Bruno on dashboard, remote xterm, native GTK/VTE, and tmux chrome.

## Rollout

This is a backward-compatible feature release and should publish as v1.6.0
after all verification gates pass.

Restart `cc-dash`, both webterm services, and the native ComandOS client so the
new backend validation, terminal client, and desktop theme registry load. Record
and compare the full tmux session/window/pane inventory before and after every
restart. Do not restart the tmux server.

## Future Learning Concept: `Teach this change`

This section records the approved brainstorming direction only.

A future feature can expose a project/pane action that opens a dedicated Tutor
pane. Its source material should be the pane PWD, repository root, exact git
diff or commit range, test evidence, and explicit decisions, not an approximate
summary scraped from terminal text.

Teaching state should live outside the source repository at
`~/.local/share/comandos/learning/<project-id>`. The workspace can follow the
stateful structure of Matt Pocock's
[`teach` skill](https://github.com/mattpocock/skills/blob/main/skills/productivity/teach/SKILL.md):
mission, trusted resources, short lessons, reference material, and learning
records. The core interaction should end with retrieval questions and an
explain-back check, because passive summaries create fluency without proving
durable understanding.

A later Decision Ledger could complement teaching by recording the decision,
alternatives rejected, and verification evidence for each meaningful change.
Neither concept is part of v1.6.0.
