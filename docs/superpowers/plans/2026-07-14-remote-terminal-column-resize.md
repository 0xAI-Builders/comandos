# Remote Terminal Column Resize Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent wrapped remote-terminal rows from becoming duplicated or interleaved when the iframe column count changes.

**Architecture:** Keep normal xterm fitting for row-only changes. For column changes, debounce and reload only the terminal iframe so a fresh ttyd client attaches to the persistent tmux session at the final geometry.

**Tech Stack:** xterm.js 5.5, ttyd 1.6/1.7 protocol, tmux, vanilla JavaScript, pytest/Node harnesses, Playwright Chromium.

## Global Constraints

- Preserve all tmux sessions, panes, running processes, pane focus, and server-side history.
- Never clear the xterm buffer as a resize workaround.
- Reload at most once after 180 ms of stable column geometry.
- Row-only changes remain live and do not reconnect.
- Publish the verified patch as `v1.5.1`.

---

### Task 1: Column-resize lifecycle

**Files:**
- Modify: `tests/test_remote_ui.py`
- Modify: `dash/term.html`

**Interfaces:**
- Consumes: `FitAddon.proposeDimensions()`, `fit.fit()`, `term.cols`, and the ttyd WebSocket state.
- Produces: `scheduleFit()` behavior that fits row-only changes and debounces `location.reload()` for column changes.

- [ ] **Step 1: Write the failing regression test**

Extend the existing geometry harness with fake timers, `location.reload()`, and
`fit.proposeDimensions()`. Assert that a row-only change calls `fit.fit()`, two
column-change events leave xterm at its old columns, and only the last 180 ms
timer reloads the page.

- [ ] **Step 2: Verify the test fails for the current behavior**

Run:

```bash
pytest -q tests/test_remote_ui.py -k 'column_resize or resize_geometry'
```

Expected: failure because current `scheduleFit()` calls `fit.fit()` for column
changes and never schedules a reload.

- [ ] **Step 3: Implement the minimal lifecycle change**

Add a single pending timer to `dash/term.html`. In the scheduled animation
frame, inspect `fit.proposeDimensions()`. If the connected socket would change
columns, replace the pending 180 ms reload timer and return without fitting.
Otherwise call `fit.fit()` as before.

- [ ] **Step 4: Verify focused and related tests**

Run:

```bash
pytest -q tests/test_remote_ui.py
bash tests/test_js_parses.sh
```

Expected: all tests pass.

### Task 2: Browser regression and release

**Files:**
- Create: `docs/releases/v1.5.1.md`

**Interfaces:**
- Consumes: running `cc-webterm-path.service`, the `/term` route, and a temporary tmux session.
- Produces: verified responsive terminal behavior and published `v1.5.1` tag/release.

- [ ] **Step 1: Restart only webterm services**

Run `bin/cc-webterm` so ttyd reloads `dash/term.html`; leave `cc-app`, tmux, and
all project processes untouched.

- [ ] **Step 2: Run the real browser reproduction**

Attach Chromium to a temporary tmux session, print a unique wrapped line, and
cycle viewport widths `390 -> 430 -> 390`. Compare xterm accessibility rows
with `tmux capture-pane` after each reconnect.

- [ ] **Step 3: Run the full release gate**

Run:

```bash
pytest -q
bash tests/test_js_parses.sh
bash tests/test_platform.sh
git diff --check
```

Expected: every command exits zero.

- [ ] **Step 4: Publish the patch**

Document the fix in `docs/releases/v1.5.1.md`, commit only intended files, push
`main`, create annotated tag `v1.5.1`, push it, and create the GitHub release.
Verify `origin/main`, the tag target, and the public release all point to the
same release commit.
