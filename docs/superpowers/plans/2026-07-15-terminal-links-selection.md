# Terminal Links and Selection Reliability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make URL clicks and text selection reliable in the native app and remote terminal without changing tmux sessions or the v1.5.1 wrapped-row protection.

**Architecture:** Classify each native primary-button gesture only on release, so clean clicks select panes/open links while drags remain owned by VTE. In the web terminal, postpone a column-changing iframe reload while xterm has an active selection, then resume the existing debounce after selection clears. Keep remote selection mode attached to each open session until the user explicitly restores interaction or closes/unloads it.

**Tech Stack:** Python 3, GTK 3/VTE, PyGObject GIO, xterm.js, vanilla JavaScript, tmux, pytest, Node.js, Playwright/Chromium, Brave.

## Global Constraints

- Preserve every tmux session, pane, process, working directory, and history.
- Preserve v1.5.1's 180 ms reload for real remote column changes.
- Preserve Ctrl+click, remote touch scroll, long-press pane resize, close/pagehide cleanup, and the existing web link handler.
- Do not add dependencies or modify `.playwright-mcp/`, `json`, or `subprocess`.
- A failed native Linux GIO launch falls back to `xdg-open` at most once and never opens duplicate tabs.

---

## File Map

- `bin/cc-app`: native URL dispatch and primary-button gesture transaction.
- `dash/term.html`: xterm fit/reload deferral while selected text exists.
- `dash/index.html`: per-session remote selection-mode lifetime.
- `tests/test_desktop_links.py`: isolated native event and launcher regression tests.
- `tests/test_remote_ui.py`: Node simulations for resize and interaction state.
- `tests/test_webterm_links.py`: unchanged-handler regression coverage.
- `docs/releases/v1.5.2.md`: patch release behavior and verification.

### Task 1: Native Link Dispatch and Gesture Classification

**Files:**
- Modify: `tests/test_desktop_links.py`
- Modify: `bin/cc-app:665-726`

**Interfaces:**
- Consumes: `url_at_event(term, event, match)`, `select_pane_at_event(term, event)`, `copy_vte_selection(term)`.
- Produces: `open_url(url: str) -> bool`; `term._primary_press` tuple `(x, y, url, pane_id)` valid until release.

- [ ] **Step 1: Add failing event and launcher tests**

Extend the AST test harness so `on_term_button`, `on_term_release`, and `open_url` execute with fake GTK/VTE events. Add tests equivalent to:

```python
def test_clean_url_click_opens_despite_preexisting_selection():
    term = FakeTerm(url="https://example.com", selected=True)
    ns["on_term_button"](term, Event(1, 10, 20))
    assert pane_calls == []
    ns["on_term_release"](term, Event(1, 11, 21))
    assert pane_calls == [(term, release_event)]
    assert opened == ["https://example.com"]


def test_primary_drag_neither_opens_url_nor_selects_pane():
    term = FakeTerm(url="https://example.com", selected=False)
    ns["on_term_button"](term, Event(1, 10, 20))
    ns["on_term_release"](term, Event(1, 30, 20))
    assert pane_calls == []
    assert opened == []


def test_linux_open_url_prefers_gio_then_falls_back_once():
    assert gio_success_ns["open_url"]("https://example.com") is True
    assert gio_calls == ["https://example.com"]
    assert popen_calls == []
    assert gio_failure_ns["open_url"]("https://example.com") is True
    assert popen_calls == [["xdg-open", "https://example.com"]]
```

Also assert a clean non-link release calls `select_pane_at_event` once, and Ctrl+click still opens immediately and returns `True`.

- [ ] **Step 2: Run the native tests and confirm RED**

Run:

```bash
pytest -q tests/test_desktop_links.py
```

Expected: failures show pane selection still occurs on press, old selection blocks the URL, drag selects a pane, and Linux dispatch does not call GIO or return a status.

- [ ] **Step 3: Implement the minimum native behavior**

Change `open_url` to return a boolean. Keep macOS `open` and WSL `wslview`; on native Linux call:

```python
try:
    Gio.AppInfo.launch_default_for_uri(url, None)
    return True
except Exception:
    try:
        subprocess.Popen(["xdg-open", url], start_new_session=True,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return True
    except Exception:
        return False
```

Change primary press to resolve `pane_id` through `pane_at(term, event)` and store `(event.x, event.y, url, pane_id)` in `term._primary_press` without selecting it. On release, clear that state and classify `abs(dx) < 5 and abs(dy) < 5`; only a clean click with a recorded pane calls `tmuxc("select-pane", "-t", pane_id)`, and a clean URL click calls `open_url(url)` regardless of `term.get_has_selection()`. Continue copying a completed VTE selection and returning `False` so VTE receives normal mouse handling.

- [ ] **Step 4: Verify GREEN and focused regressions**

Run:

```bash
pytest -q tests/test_desktop_links.py tests/test_desktop_tabs.py tests/test_desktop_launcher.py
python3 -m py_compile bin/cc-app
```

Expected: all tests pass and `cc-app` compiles.

- [ ] **Step 5: Commit native behavior**

```bash
git add bin/cc-app tests/test_desktop_links.py
git commit -m "Fix native terminal links and selection gestures"
```

### Task 2: Defer Remote Column Reload During Selection

**Files:**
- Modify: `tests/test_remote_ui.py`
- Modify: `dash/term.html:147-177`

**Interfaces:**
- Consumes: xterm `term.hasSelection()` and `term.onSelectionChange(callback)`.
- Produces: `fitDeferredForSelection: boolean`; `resumeDeferredFitAfterSelection() -> void`; existing `scheduleFit() -> void` resumes deferred work.

- [ ] **Step 1: Add a failing selection-aware resize simulation**

Add a Node simulation that extracts `scheduleFit`, supplies `term.hasSelection()`, and records timers/reloads:

```javascript
let selected = true;
const term = {
  cols: 46, rows: 35,
  hasSelection(){ return selected; },
};
let proposed = {cols: 51, rows: 35};
scheduleFit();
flushFrames();
const whileSelected = {timers: timers.size, reloads, deferred: fitDeferredForSelection};
selected = false;
resumeDeferredFitAfterSelection();
flushFrames();
const afterClear = {timers: timers.size, deferred: fitDeferredForSelection};
```

Assert no timer/reload while selected, one 180 ms timer after clearing, and no reload when `proposed.cols` returns to `term.cols` before the selection clears. Assert repeated selection-change/resize events still coalesce.

- [ ] **Step 2: Run the resize tests and confirm RED**

Run:

```bash
pytest -q tests/test_remote_ui.py -k 'column_resize or selection_aware'
```

Expected: the new test fails because `scheduleFit` starts a reload timer despite active selection and no `onSelectionChange` handler exists.

- [ ] **Step 3: Implement deferred fitting**

Near the existing fit state add:

```javascript
let fitDeferredForSelection = false;
```

Inside the animation-frame callback, after proposing dimensions but before scheduling a column reload:

```javascript
if (next && ws.readyState === 1 && next.cols !== term.cols && term.hasSelection()) {
  fitDeferredForSelection = true;
  if (columnReloadTimer) {
    clearTimeout(columnReloadTimer);
    columnReloadTimer = 0;
  }
  return;
}
fitDeferredForSelection = false;
```

Define and wire selection clearing once:

```javascript
function resumeDeferredFitAfterSelection() {
  if (fitDeferredForSelection && !term.hasSelection()) scheduleFit();
}
term.onSelectionChange(resumeDeferredFitAfterSelection);
```

Keep the current reload branch and row-only `fit.fit()` behavior unchanged.

- [ ] **Step 4: Verify GREEN and webterm regressions**

Run:

```bash
pytest -q tests/test_remote_ui.py tests/test_webterm_links.py
bash tests/test_js_parses.sh
```

Expected: remote UI/link tests pass and the repository's JavaScript syntax harness parses both dashboard scripts.

- [ ] **Step 5: Commit selection-safe resize**

```bash
git add dash/term.html tests/test_remote_ui.py
git commit -m "Preserve remote terminal selection during resize"
```

### Task 3: Persist Remote Selection Mode Across Tabs

**Files:**
- Modify: `tests/test_remote_ui.py`
- Modify: `dash/index.html:2386-2399,2540-2556`

**Interfaces:**
- Consumes: `setTermSelectionMode`, `restoreTermInteraction`, `restoreAllTermInteractions`, `closeTerm`.
- Produces: tab switching that does not mutate another open terminal's `temporary` selection state.

- [ ] **Step 1: Change the regression expectation before production code**

Replace the current tab-switch test with a persistence test. First assert that the extracted `showView` source does not call `restoreInactiveTermInteractions`; this assertion fails against v1.5.1. Then run the existing async selection simulation without invoking restoration when `activeTerm` changes:

```javascript
const selecting = setTermSelectionMode("ssh-prod", true);
await Promise.resolve();
activeTerm = "beta";
finishSelection({mouse: "off"});
await selecting;
```

Assert POST flags are `[false]`, `ssh-prod.mouse === false`, and `ssh-prod.temporary === true`. Retain separate assertions that `closeTerm("ssh-prod")` and `restoreAllTermInteractions()` POST/keepalive `enabled:true`.

- [ ] **Step 2: Run the interaction tests and confirm RED**

Run:

```bash
pytest -q tests/test_remote_ui.py -k 'terminal_selection'
```

Expected: tab-switch persistence fails because `showView` calls `restoreInactiveTermInteractions` and posts `enabled:true`.

- [ ] **Step 3: Remove only tab-switch restoration**

Remove `restoreInactiveTermInteractions(shown)` from `showView`. Delete `restoreInactiveTermInteractions` if it has no remaining call sites and remove it from the test harness extraction list. Do not change `restoreTermInteraction` calls in `closeTerm`, the **Interact** toggle path, `restoreAllTermInteractions`, `pagehide`, or `pageshow` resynchronization.

- [ ] **Step 4: Verify GREEN and state-machine regressions**

Run:

```bash
pytest -q tests/test_remote_ui.py
```

Expected: every interaction state, close, pagehide, pageshow, touch, tab, and remote UI test passes.

- [ ] **Step 5: Commit persistent selection mode**

```bash
git add dash/index.html tests/test_remote_ui.py
git commit -m "Keep remote selection mode across tab switches"
```

### Task 4: End-to-End Verification and Patch Release

**Files:**
- Create: `docs/releases/v1.5.2.md`
- Modify only if required by existing release automation: release metadata discovered in repository scripts.

**Interfaces:**
- Consumes: Tasks 1-3 and existing release workflow.
- Produces: verified `v1.5.2` tag/release and restarted ComandOS clients/services.

- [ ] **Step 1: Run the complete automated suite**

```bash
pytest -q
bash tests/test_platform.sh
git diff --check
```

Expected: all tests pass, platform checks pass, and no whitespace errors exist.

- [ ] **Step 2: Run real browser checks before restarting anything**

Against a disposable/live remote terminal in Brave and Chromium:

1. Open an exact `https://example.com/comandos-link-test` link and assert the new page URL matches exactly.
2. Select a unique marker, resize from one column count to another, and assert the selection remains while highlighted.
3. Clear selection and assert one deferred reattach completes with uncorrupted wrapped text.
4. Enable **Seleccionar**, switch ComandOS tabs, return, and assert the mode/button remains active.
5. Repeat the selection/scroll path with a touch viewport.

Do not close or kill existing tmux sessions during these checks.

- [ ] **Step 3: Verify native Brave resolution and gesture behavior**

```bash
xdg-settings get default-web-browser
gio mime x-scheme-handler/https
```

Expected: both resolve to the registered Brave desktop handler. In the running app, clean-click a URL, drag-select text over a split, copy it, and click each split; assert the browser opens once, selection remains/copies, and keyboard focus follows the clean pane click.

- [ ] **Step 4: Write and commit release notes**

Create `docs/releases/v1.5.2.md` describing the two user-facing fixes, preserved behavior, exact automated counts, real-browser results, and restart commands. Then:

```bash
git add docs/releases/v1.5.2.md
git commit -m "Document v1.5.2 terminal interaction fixes"
```

- [ ] **Step 5: Restart only ComandOS surfaces**

Confirm `~/.local/bin/cc-app` resolves to this checkout. Restart `cc-webterm.service`/path through `bin/cc-webterm`, then stop the single current native `cc-app` process gracefully and relaunch it with `bin/cc-centro`. Verify services are active and the app process points at this checkout. Do not restart tmux, `cc-dash`, or unrelated user services.

- [ ] **Step 6: Publish the patch after final verification**

Re-run focused smoke tests against the restarted surfaces, push `main`, create signed/annotated tag `v1.5.2` according to the repository's existing release convention, push the tag, and create the GitHub release from `docs/releases/v1.5.2.md`. Confirm local `HEAD`, `origin/main`, and `v1.5.2` resolve to the intended release commit.
