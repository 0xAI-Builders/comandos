# Terminal Links and Selection Reliability Design

**Date:** 2026-07-15
**Status:** Approved

## Goal

Make terminal links and text selection predictable in the native desktop app
and in the remote web terminal on desktop and touch devices. Preserve the
v1.5.1 wrapped-row fix and all tmux sessions and processes.

## Confirmed Failures

### Native desktop

`on_term_release()` opens a URL only when VTE reports no selection. A selection
that existed before the click therefore blocks the link; VTE then clears that
selection as part of normal click handling. The result is one click that both
does nothing and removes the highlight.

Every primary-button press also selects the tmux pane immediately. A drag meant
to create a VTE selection can therefore trigger a tmux redraw before the drag
finishes.

### Remote web terminal

The v1.5.1 column-resize fix reloads the terminal iframe after width changes.
That protects wrapped rows, but a reload destroys an active xterm selection.
This was reproduced in Brave with an active selection followed by a
`110 -> 115` column change.

Selection mode is currently described as a toggle but is restored
automatically when another ComandOS tab becomes active. That makes the visible
contract and the actual behavior disagree.

The existing remote link handler is not the cause: a real headless Brave test
opened the exact terminal URL in a new page.

## Desktop Design

Treat a primary-button gesture as one transaction:

1. On press, record coordinates, URL, pane, and whether a selection existed.
   Do not select the pane yet.
2. On release, classify movement under five pixels as a clean click.
3. A clean click selects the target pane. If it began over a URL, open that URL
   even if an older selection still exists.
4. A drag does not select the pane and never opens a URL. VTE remains free to
   finish and retain the selection.
5. Ctrl+click remains a direct URL command.

Open HTTP(S) URLs through `Gio.AppInfo.launch_default_for_uri()` on native
Linux, which honors the registered Brave default. Keep the current macOS and
WSL launchers and retain `xdg-open` as the Linux fallback. Launcher failures
must return `False` rather than disappearing silently; successful dispatch
returns `True`.

## Remote Design

Before scheduling a column-change reload, check `term.hasSelection()`.

- If no selection exists, retain the v1.5.1 debounced reload unchanged.
- If a selection exists, remember that a column fit is pending and do not
  resize or reload.
- Listen to `term.onSelectionChange()`. When the selection becomes empty,
  re-run the normal fit decision. If the width returned to its original value,
  no reload is needed; otherwise one 180 ms debounced reload occurs.

Selection mode remains active for an open terminal when switching ComandOS
tabs. It is restored to interactive tmux mouse mode only when the user presses
**Interact**, closes that terminal tab, or the dashboard actually unloads.
This matches the visible toggle and avoids state changing behind the user.

The remote URL handler remains unchanged because Brave and Chrome-compatible
anchor activation already works and changing it would add popup risk without
addressing the reproduced desktop defect.

## State and Failure Handling

- No tmux session, pane, process, working directory, or history may be killed.
- A pending column fit is local to one iframe and is cleared after it is
  resolved.
- Closing a remote tab and `pagehide` still restore tmux mouse mode so a dead
  browser cannot leave the session in selection mode unintentionally.
- Failed desktop browser dispatch falls back once; it must not open duplicate
  tabs.
- Existing touch scroll and long-press pane resize behavior remains disabled
  while selection mode is active.

## Verification

### Automated

- Desktop clean URL clicks open with and without a pre-existing VTE selection.
- Desktop drags neither select a tmux pane nor open a URL.
- Clean non-URL clicks still select the correct split pane.
- Linux URI dispatch prefers GIO and falls back to `xdg-open` exactly once.
- Remote column changes do not reload while `term.hasSelection()` is true.
- Clearing selection resumes the pending fit and produces at most one reload.
- Remote selection mode survives a ComandOS tab switch and is still restored
  on close/pagehide.
- Existing remote touch, wrapping, links, desktop tabs, and JavaScript parsing
  suites remain green.

### Real browser

- Brave remote click opens the exact URL in a new tab.
- Brave selection survives a width change while highlighted, then the deferred
  resize completes after selection is cleared.
- Chromium repeats the same remote selection and link checks.
- Native desktop link dispatch resolves to the registered Brave desktop
  handler.

## Release

After the complete test and browser gates pass, restart only the ComandOS
desktop client and user webterm services. Publish a backward-compatible patch
release; do not include unrelated untracked artifacts.
