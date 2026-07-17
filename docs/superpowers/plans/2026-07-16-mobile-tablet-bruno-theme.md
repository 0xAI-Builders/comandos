# Mobile/Tablet Reliability and Bruno Theme Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make remote terminals reliable and usable on small phones and touch tablets, expose truthful primary/fallback health, and add the Bruno theme across dashboard, xterm, GTK/VTE, and tmux without disrupting existing sessions.

**Architecture:** Keep the existing tmux + ttyd + xterm.js architecture. Add crash recovery and endpoint probes at the service boundary, make `cc-dash` expose route and endpoint health separately, then keep responsive layout in `dash/index.html` and terminal geometry/input in `dash/term.html`. The custom terminal iframe communicates theme and interaction state with its parent through validated same-origin `postMessage` events.

**Tech Stack:** Bash, Python 3 stdlib HTTP server, systemd user transient units, ttyd 1.6/1.7 compatibility, tmux, HTML/CSS/vanilla JavaScript, xterm.js, pytest, Node.js test harnesses, Playwright with system Chrome.

## Global Constraints

- Preserve every existing tmux session, window, pane, PWD, foreground process, and session identity.
- Do not restart the tmux server and do not type into, resize, close, rename, or reattach a user session during verification.
- Do not add input batching, predictive echo, terminal caching, a new daemon, or a persistence layer.
- Terminal input remains direct while resize work settles independently.
- The preferred custom endpoint is `http://127.0.0.1:4780/term`; the compatibility fallback is `http://127.0.0.1:4779`.
- Both systemd transient ttyd units use `Restart=on-failure` and `RestartSec=1s`; an explicit `cc-webterm off` remains final.
- Height-only resize settles for 120 ms; column changes retain the 180 ms reattach window.
- Phone mode shows panel or terminal, never both; coarse-pointer tablet portrait also stays single-view.
- A split requires at least a 300 px panel and a 440 px terminal; fine-pointer desktop retains its 900 px width breakpoint.
- Primary touch targets are at least 40 px, using 44 px in tab and terminal bars; the splitter hit target is at least 32 px.
- Add exactly one theme id, `bruno`, with display name `Bruno`; do not redesign components or change typography/navigation.
- Do not implement `Teach this change`, skills, lessons, quizzes, learning storage, or background learning work in v1.6.0.
- The release target is v1.6.0 only after automated, browser, service-recovery, inventory, and live-theme checks pass.

## File Map

- `bin/cc-webterm`: own ttyd process lifecycle, startup probes, supervision, and `active|degraded|off` CLI status.
- `bin/cc-mobile`: configure only responding terminal routes and report endpoint state instead of process matches.
- `bin/cc-doctor`: diagnose primary and fallback probes independently.
- `bin/cc-dash`: expose bounded endpoint probes and route-aware remote state; accept `bruno` in preferences.
- `dash/index.html`: own the remote shell, tab visibility, split policy, fallback notice, iframe messages, and dashboard theme.
- `dash/term.html`: own xterm geometry, direct terminal input, touch toolbar, paste fallback, and live xterm theme.
- `bin/cc-app`: register Bruno for GTK, VTE, and tmux chrome.
- `DESIGN.md`: document all five themes and the source-of-truth palette roles.
- `tests/test_webterm_service.py`: exercise ttyd supervision and three-state service health with fake commands.
- `tests/test_remote_controls.py`: exercise backend endpoint/route combinations.
- `tests/test_dashboard_layout.py`: assert dynamic viewport, grid shell, safe-area, touch target, and split rules.
- `tests/test_remote_ui.py`: execute the real JavaScript resize, toolbar, bridge, fallback, tab, and theme helpers under Node.
- `tests/test_themes.py`: assert the Bruno palette and all theme registries/surfaces.
- `tests/e2e_mobile_remote.js`: use a disposable tmux session for phone/tablet browser verification.
- `docs/releases/v1.6.0.md`: record behavior, compatibility, and verification evidence.

---

### Task 1: Supervise and Probe Both ttyd Endpoints

**Files:**
- Create: `tests/test_webterm_service.py`
- Modify: `bin/cc-webterm:17-104`
- Modify: `bin/cc-mobile:68-114`
- Modify: `bin/cc-doctor:326-347`

**Interfaces:**
- Consumes: `CC_WEBTERM_PORT` and `CC_WEBTERM_PATH_PORT` environment overrides already supported by `bin/cc-webterm`.
- Produces: shell `probe_url URL`, `health_state`, and `wait_for_endpoint URL`; `cc-webterm status` prints exactly one of `active`, `degraded`, or `off` on its final line.
- Produces: two transient units carrying `--property=Restart=on-failure --property=RestartSec=1s`.

- [ ] **Step 1: Write failing lifecycle and health tests**

Create `tests/test_webterm_service.py` with an executable fake `curl` and assertions that cover all four endpoint combinations:

```python
import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "bin" / "cc-webterm"
SOURCE = SCRIPT.read_text()


def write_executable(path, body):
    path.write_text("#!/bin/sh\n" + body)
    path.chmod(0o755)


def status_with(tmp_path, healthy):
    fake = tmp_path / "bin"
    fake.mkdir()
    write_executable(
        fake / "curl",
        'case "$*" in\n'
        '  *"4779/token"*) [ "$FALLBACK_HEALTHY" = 1 ] ;;\n'
        '  *"4780/term/token"*) [ "$PRIMARY_HEALTHY" = 1 ] ;;\n'
        '  *) exit 1 ;;\n'
        'esac\n',
    )
    env = {
        **os.environ,
        "PATH": f"{fake}:{os.environ['PATH']}",
        "FALLBACK_HEALTHY": "1" if "fallback" in healthy else "0",
        "PRIMARY_HEALTHY": "1" if "primary" in healthy else "0",
    }
    run = subprocess.run(
        [str(SCRIPT), "status"], env=env, text=True,
        capture_output=True, check=True,
    )
    return run.stdout.strip().splitlines()[-1]


def test_status_requires_both_probes_for_active(tmp_path):
    assert status_with(tmp_path, {"primary", "fallback"}) == "active"
    assert status_with(tmp_path, {"primary"}) == "degraded"
    assert status_with(tmp_path, {"fallback"}) == "degraded"
    assert status_with(tmp_path, set()) == "off"


def test_transient_units_restart_only_after_failure():
    assert "--property=Restart=on-failure" in SOURCE
    assert "--property=RestartSec=1s" in SOURCE
    assert SOURCE.count('"${SYSTEMD_RESTART[@]}"') == 2
    assert 'current_state="$(health_state)"' in SOURCE
    assert '[ "$current_state" = active ]' in SOURCE
    assert "systemctl --user stop cc-webterm.service" in SOURCE
    assert "systemctl --user stop cc-webterm-path.service" in SOURCE


def test_mobile_and_doctor_do_not_use_process_matches_as_health():
    mobile = (ROOT / "bin" / "cc-mobile").read_text()
    doctor = (ROOT / "bin" / "cc-doctor").read_text()
    assert '4780/term/token' in mobile and '4779/token' in mobile
    assert 'pgrep -f "ttyd.*cc-webterm-attach"' not in mobile
    section = doctor.split("_section_remote()", 1)[1].split("\n}", 1)[0]
    assert '4780/term/token' in section and '4779/token' in section
    assert "pgrep" not in section
```

- [ ] **Step 2: Run the tests and confirm the old process-based behavior fails**

Run: `pytest -q tests/test_webterm_service.py`

Expected: failures for `active|degraded|off`, missing restart properties, and remaining `pgrep` health checks.

- [ ] **Step 3: Add bounded probe and status helpers to `bin/cc-webterm`**

Insert these helpers after `ok()` and replace the current `status)` case with the shown branch:

```bash
probe_url() {
  curl -fsS --max-time 2 "$1" >/dev/null 2>&1
}

health_state() {
  local primary=0 fallback=0
  probe_url "http://127.0.0.1:$PATH_PORT/term/token" && primary=1
  probe_url "http://127.0.0.1:$PORT/token" && fallback=1
  if [ "$primary" -eq 1 ] && [ "$fallback" -eq 1 ]; then
    printf '%s\n' active
  elif [ "$primary" -eq 1 ] || [ "$fallback" -eq 1 ]; then
    printf '%s\n' degraded
  else
    printf '%s\n' off
  fi
}

wait_for_endpoint() {
  local url="$1" attempt=0
  while [ "$attempt" -lt 20 ]; do
    probe_url "$url" && return 0
    attempt=$((attempt + 1))
    sleep 0.1
  done
  return 1
}

case "${1:-on}" in
  off) stop; ok "Terminal web detenido."; exit 0 ;;
  status) health_state; exit 0 ;;
esac

current_state="$(health_state)"
if [ "$current_state" = active ]; then
  ok "Terminal web ya esta activo."
  exit 0
fi
```

Use the same restart properties on both transient units and require both startup probes:

```bash
SYSTEMD_RESTART=(--property=Restart=on-failure --property=RestartSec=1s)
systemd-run --user --collect --quiet --unit=cc-webterm \
  "${SYSTEMD_RESTART[@]}" "${TTYD_ARGS[@]}" >/dev/null 2>&1
systemd-run --user --collect --quiet --unit=cc-webterm-path \
  "${SYSTEMD_RESTART[@]}" "${TTYD_PATH_ARGS[@]}" >/dev/null 2>&1

if wait_for_endpoint "http://127.0.0.1:$PORT/token" && \
   wait_for_endpoint "http://127.0.0.1:$PATH_PORT/term/token"; then
  ok "Terminal web listo en 127.0.0.1:$PORT (:8443) y 127.0.0.1:$PATH_PORT (/term)."
else
  die "ttyd no arranco por completo. Estado: $(health_state)"
fi
```

Keep `stop()` before startup. `systemctl stop` suppresses `Restart=on-failure`, so explicit off stays off.

- [ ] **Step 4: Replace process checks in `cc-mobile` and `cc-doctor`**

Move `WEBTERM_PORT` and `WEBTERM_PATH_PORT` above the command `case` in `bin/cc-mobile`, then add and use these endpoint predicates for its `status` output, route creation, and final copy:

```bash
primary_webterm_healthy() {
  curl -fsS --max-time 2 "http://127.0.0.1:$WEBTERM_PATH_PORT/term/token" >/dev/null 2>&1
}
fallback_webterm_healthy() {
  curl -fsS --max-time 2 "http://127.0.0.1:$WEBTERM_PORT/token" >/dev/null 2>&1
}
```

Create `/term` only when `primary_webterm_healthy` succeeds, create `:8443` only when `fallback_webterm_healthy` succeeds, and print `active`, `degraded`, or `off` from those two booleans in `cc-mobile status`. The normal Spanish success copy uses `activo`, `degradado`, or `apagado`. In `_section_remote()` use the same two `curl -fsS --max-time 2` probes and emit separate doctor rows named `cc-webterm-primary` and `cc-webterm-fallback`.

- [ ] **Step 5: Verify shell behavior and syntax**

Run: `pytest -q tests/test_webterm_service.py tests/test_remote_controls.py`

Expected: all selected tests pass.

Run: `bash -n bin/cc-webterm bin/cc-mobile bin/cc-doctor`

Expected: exit status 0 with no output.

- [ ] **Step 6: Commit the supervised lifecycle**

```bash
git add bin/cc-webterm bin/cc-mobile bin/cc-doctor tests/test_webterm_service.py
git commit -m "Fix web terminal supervision and health probes"
```

---

### Task 2: Expose Truthful Remote Health and Recover From Fallback

**Files:**
- Modify: `bin/cc-dash:570-631`
- Modify: `dash/index.html:679-684,1994-2111,2514-2537,3218-3283`
- Modify: `tests/test_remote_controls.py:16-30,91-119`
- Modify: `tests/test_remote_ui.py:35-49`

**Interfaces:**
- Consumes: `http_healthy(url: str, timeout: float = 0.4) -> bool`.
- Produces: `webterm_health(probe=None) -> {"primaryHealthy": bool, "fallbackHealthy": bool}`.
- Produces: `remote_status_from_text(serve_text: str, health: dict) -> dict` with `primaryTerminalOn`, `fallbackTerminalOn`, `webtermOn`, `webtermReachable`, `webtermDegraded`, and `terminalState`.
- Produces: `resolveTermBase() -> Promise<string>` that retries primary three times at 400 ms spacing for every new attach, then asks same-origin `/remote-state` whether the cross-origin fallback responds and uses it without memoizing the result.

- [ ] **Step 1: Add failing backend route/health matrix tests**

Update `load_remote_helpers()` to extract `remote_urls`, `remote_status_from_text`, and `webterm_health`, then replace the old boolean status test with:

```python
def test_remote_status_separates_routes_from_endpoint_health():
    ns = load_remote_helpers()
    serve = """
https://zion.tail63a117.ts.net
|-- /     proxy http://127.0.0.1:4777
|-- /term proxy http://127.0.0.1:4780/term
https://zion.tail63a117.ts.net:8443
|-- / proxy http://127.0.0.1:4779
"""
    active = ns["remote_status_from_text"](
        serve, {"primaryHealthy": True, "fallbackHealthy": True})
    degraded = ns["remote_status_from_text"](
        serve, {"primaryHealthy": False, "fallbackHealthy": True})
    off = ns["remote_status_from_text"](
        serve, {"primaryHealthy": False, "fallbackHealthy": False})

    assert active["terminalState"] == "active"
    assert active["primaryTerminalOn"] is True
    assert active["fallbackTerminalOn"] is True
    assert degraded["terminalState"] == "degraded"
    assert degraded["webtermOn"] is False
    assert degraded["webtermReachable"] is True
    assert off["terminalState"] == "off"
    assert off["webtermReachable"] is False


def test_webterm_health_probes_primary_and_fallback_independently():
    ns = load_remote_helpers()
    calls = []

    def probe(url, timeout=0.4):
        calls.append((url, timeout))
        return "4780/term/token" in url

    health = ns["webterm_health"](probe)
    assert health == {"primaryHealthy": True, "fallbackHealthy": False}
    assert [url for url, _timeout in calls] == [
        "http://127.0.0.1:4780/term/token",
        "http://127.0.0.1:4779/token",
    ]
```

- [ ] **Step 2: Add failing UI tests for degraded controls and non-memoized recovery**

Add this control assertion:

```python
def test_degraded_remote_can_restart_primary_and_open_fallback():
    state = remote_button_state({
        "primaryHealthy": False,
        "fallbackHealthy": True,
        "webtermOn": False,
        "webtermReachable": True,
        "terminalState": "degraded",
    })
    assert state["webtermOnDisabled"] is False
    assert state["webtermOffDisabled"] is False
    assert state["openTerminalDisabled"] is False
```

Add a Node harness that extracts `delay`, `probePrimaryTerm`, `fallbackTermAvailable`, and `resolveTermBase`. Make `probePrimaryTerm()` fail three times while `fallbackTermAvailable()` returns true on the first attach, then make primary succeed on the next attach. Assert results `[TERM_FALLBACK_BASE, TERM_BASE]`, four total primary probes, and one same-origin fallback-state lookup. Use fake `setTimeout(callback) { callback(); }` so the test is deterministic.

- [ ] **Step 3: Implement bounded Python probes and route-aware state**

Replace `webterm_running()` with:

```python
def http_healthy(url, timeout=0.4):
    try:
        request = urllib.request.Request(
            url, headers={"Cache-Control": "no-store", "User-Agent": "ComandOS-health"})
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return 200 <= response.status < 400
    except Exception:
        return False


def webterm_health(probe=None):
    check = probe or http_healthy
    return {
        "primaryHealthy": bool(check("http://127.0.0.1:4780/term/token", timeout=0.4)),
        "fallbackHealthy": bool(check("http://127.0.0.1:4779/token", timeout=0.4)),
    }


def remote_status_from_text(serve_text, health):
    remote_on = "proxy http://127.0.0.1:4777" in serve_text
    term_route = "proxy http://127.0.0.1:4780/term" in serve_text
    fallback_route = "proxy http://127.0.0.1:4779" in serve_text
    primary = bool(health.get("primaryHealthy"))
    fallback = bool(health.get("fallbackHealthy"))
    primary_on = primary and term_route
    fallback_on = fallback and fallback_route
    state = "active" if remote_on and primary_on else (
        "degraded" if remote_on and fallback_on else "off")
    return {
        "remoteOn": remote_on,
        "primaryHealthy": primary,
        "fallbackHealthy": fallback,
        "termRouteOn": term_route,
        "fallbackRouteOn": fallback_route,
        "primaryTerminalOn": primary_on,
        "fallbackTerminalOn": fallback_on,
        "webtermOn": primary_on,
        "webtermReachable": primary_on or fallback_on,
        "webtermDegraded": state == "degraded",
        "terminalState": state,
    }
```

Call `remote_status_from_text(serve, webterm_health())` from `remote_state()`. The two probes complete in at most 0.8 seconds and never throw into `/remote-state`.

- [ ] **Step 4: Implement fresh primary probing and one degraded notice**

Remove `termBasePromise` and add:

```javascript
const TERM_PRIMARY_ATTEMPTS = 3;
const TERM_PRIMARY_RETRY_MS = 400;
let termFallbackNotified = false;

function delay(ms){
  return new Promise(resolve => setTimeout(resolve, ms));
}
async function probePrimaryTerm(){
  try{
    const response = await fetch(`${TERM_BASE}/token`, {cache:"no-store"});
    return response.ok;
  }catch(_){
    return false;
  }
}
async function fallbackTermAvailable(){
  try{
    const state = await api("/remote-state");
    return !!state.fallbackTerminalOn;
  }catch(_){
    return false;
  }
}
function showTermDegraded(){
  if(termFallbackNotified) return;
  termFallbackNotified = true;
  toast(tf("Terminal en modo degradado", "Terminal running in degraded mode"), true);
}
async function resolveTermBase(){
  if(!WEBTERM) return "";
  for(let attempt = 0; attempt < TERM_PRIMARY_ATTEMPTS; attempt++){
    if(await probePrimaryTerm()) return TERM_BASE;
    if(attempt + 1 < TERM_PRIMARY_ATTEMPTS) await delay(TERM_PRIMARY_RETRY_MS);
  }
  if(await fallbackTermAvailable()){
    showTermDegraded();
    return TERM_FALLBACK_BASE;
  }
  throw new Error(tf("Ninguna terminal remota responde", "No remote terminal endpoint is responding"));
}
```

In `ensureFrame()`, catch failure, remove the empty iframe, clear `o.frame`, and show the error toast. Set `frame.dataset.compat = base === TERM_BASE ? "0" : "1"` before assigning `src` so later theme/interaction messages skip the compatibility client.

- [ ] **Step 5: Render active, degraded, and off states truthfully**

Make `remoteButtonState()` derive start/stop/open behavior from local endpoint health and reachable routes:

```javascript
function remoteButtonState(st, busy){
  const primary = !!st?.primaryHealthy;
  const fallback = !!st?.fallbackHealthy;
  const anyLocal = primary || fallback;
  const reachable = !!st?.webtermReachable;
  return {
    remoteOnDisabled: !!busy || !!st?.remoteOn,
    remoteOffDisabled: !!busy || !st?.remoteOn,
    webtermOnDisabled: !!busy || (primary && fallback),
    webtermOffDisabled: !!busy || !anyLocal,
    openTerminalDisabled: !!busy || !reachable,
    remoteOnActive: !!st?.remoteOn,
    remoteOffActive: false,
    webtermOnActive: primary && fallback,
    webtermOffActive: false,
    openTerminalActive: reachable,
  };
}
```

Add `.remote-state.degraded` styling with `--waiting`; `renderRemote()` must use `terminalState` to display `Activo`, `Degradado`, or `Apagado`, and must not use route presence as endpoint health.

- [ ] **Step 6: Verify and commit remote health**

Run: `pytest -q tests/test_remote_controls.py tests/test_remote_ui.py tests/test_webterm_service.py`

Expected: all selected tests pass.

Run: `bash tests/test_js_parses.sh`

Expected: both embedded scripts parse successfully.

```bash
git add bin/cc-dash dash/index.html tests/test_remote_controls.py tests/test_remote_ui.py
git commit -m "Fix remote endpoint health and fallback recovery"
```

---

### Task 3: Replace Fixed Mobile Geometry With a Dynamic App Shell

**Files:**
- Modify: `dash/index.html:1-2,43-49,576-670,2461-2511,2616-2627`
- Modify: `dash/term.html:4-6`
- Modify: `tests/test_dashboard_layout.py`
- Modify: `tests/test_remote_ui.py`

**Interfaces:**
- Produces: `currentViewportHeight() -> number`, `scheduleAppViewport() -> void`, and `shouldSplitLayout(width, height, coarse) -> bool`.
- Produces: CSS `--app-height` and a two-row `#panes` grid; no app-specific absolute `top` offset.
- Produces: `revealActiveTab(bar) -> void` using `scrollIntoView({block:"nearest", inline:"nearest"})`.

- [ ] **Step 1: Add failing layout and split-policy tests**

Extend `tests/test_dashboard_layout.py` with:

```python
def test_remote_shell_uses_dynamic_viewport_grid_without_fixed_tab_offset():
    assert "interactive-widget=resizes-content" in CSS
    app = rule("body.app")
    assert "var(--app-height,100dvh)" in app
    panes = rule("body.app #panes")
    assert "grid-template-rows:auto minmax(0,1fr)" in panes
    assert "safe-area-inset-top" in panes
    assert "safe-area-inset-bottom" in panes
    narrow = rule("body.app #view-panel,body.app #term-area")
    assert "top:44px" not in narrow
    assert "position:absolute" not in narrow


def test_remote_touch_targets_have_stable_minimums():
    tab = rule(".apptab")
    assert "min-height:44px" in tab
    splitter = rule("body.app.split #splitter::before")
    assert "inset:0 -12px" in splitter
```

Add a Node test for `shouldSplitLayout()` with the exact matrix:

```javascript
[
  [390, 844, true, false],
  [834, 1112, true, false],
  [1194, 834, true, true],
  [899, 700, false, false],
  [900, 700, false, true],
  [747, 500, true, false],
  [748, 500, true, true],
]
```

- [ ] **Step 2: Run focused tests and confirm fixed `100vh/top:44px` failures**

Run: `pytest -q tests/test_dashboard_layout.py tests/test_remote_ui.py -k 'layout or split or active_tab'`

Expected: new assertions fail against the old absolute shell and width-only media query.

- [ ] **Step 3: Implement dynamic viewport metadata, CSS, and scheduler**

Set both viewport tags to:

```html
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content">
```

Replace narrow app geometry with:

```css
body.app{height:var(--app-height,100dvh);overflow:hidden;display:block}
body.app #panes{
  height:100%;min-height:0;display:grid;
  grid-template-columns:minmax(0,1fr);
  grid-template-rows:auto minmax(0,1fr);
  padding-top:env(safe-area-inset-top);
  padding-bottom:env(safe-area-inset-bottom);
}
body.app #tabbar{grid-column:1;grid-row:1;position:relative;inset:auto;padding:0 8px}
body.app #view-panel,body.app #term-area{
  grid-column:1;grid-row:2;position:relative;inset:auto;min-width:0;min-height:0;
}
body.app #topbar{padding-top:0}
body.app #content{padding-bottom:0}
.apptab{min-height:44px}
```

Keep split grid areas under `@media (min-width:748px)`; the `split` class is the authoritative policy gate. Add:

```javascript
let appViewportFrame = 0;
function currentViewportHeight(){
  return Math.round(window.visualViewport?.height || window.innerHeight || document.documentElement.clientHeight);
}
function updateAppViewport(){
  appViewportFrame = 0;
  document.documentElement.style.setProperty("--app-height", `${currentViewportHeight()}px`);
}
function scheduleAppViewport(){
  if(!appViewportFrame) appViewportFrame = requestAnimationFrame(updateAppViewport);
}
function shouldSplitLayout(width, height, coarse){
  if(coarse) return width > height && width >= 748;
  return width >= 900;
}
function applyAppLayout(){
  const width = Math.round(window.visualViewport?.width || window.innerWidth);
  const height = currentViewportHeight();
  const coarse = window.matchMedia("(pointer:coarse)").matches || navigator.maxTouchPoints > 0;
  document.body.classList.toggle("split", shouldSplitLayout(width, height, coarse));
  restoreSplitLeft();
  showView(activeView);
}
```

Wire `visualViewport.resize`, `window.resize`, and `orientationchange` to both `scheduleAppViewport()` and `applyAppLayout()`. Do not recreate `openTerms`; `showView(activeView)` after toggling `split` retains the active session.

- [ ] **Step 4: Reveal only the newly active tab**

Add and invoke this after `renderTabbar()` appends all tabs:

```javascript
function revealActiveTab(bar){
  if(revealActiveTab.lastView === activeView) return;
  revealActiveTab.lastView = activeView;
  requestAnimationFrame(() => {
    bar.querySelector(".apptab.on")?.scrollIntoView({block:"nearest", inline:"nearest"});
  });
}
```

Do not center the strip and do not assign `scrollLeft` globally.

- [ ] **Step 5: Verify responsive shell and commit**

Run: `pytest -q tests/test_dashboard_layout.py tests/test_remote_ui.py`

Expected: all layout, splitter, tab, scroll, and existing interaction tests pass.

Run: `bash tests/test_js_parses.sh`

Expected: exit status 0.

```bash
git add dash/index.html dash/term.html tests/test_dashboard_layout.py tests/test_remote_ui.py
git commit -m "Fix mobile and tablet viewport layout"
```

---

### Task 4: Settle Keyboard Resize Without Delaying Input

**Files:**
- Modify: `dash/term.html:31-35,123-197,203-282,552-564`
- Modify: `tests/test_remote_ui.py:382-775`

**Interfaces:**
- Produces: `scheduleFit()`, `coordinateResize()`, `commitHeightFit()`, `commitColumnReload()`, and `disposeResizeCoordinator()`.
- Consumes: `fit.proposeDimensions() -> {cols, rows}`, `term.cols`, `term.rows`, `term.hasSelection()`, and `ws.readyState`.
- Invariant: `term.onData(sendTerminalData)` reaches `sendInput()` immediately and never waits on resize timers.

- [ ] **Step 1: Replace resize expectations with the keyboard-animation contract**

Build the Node harness from the real coordinator functions and queue 15 proposed dimensions with the same columns and descending rows. Flush one animation frame after each observation, type one character after each observation, then run timers. Assert:

```python
assert result["fitCalls"] == 1
assert result["reloads"] == 0
assert result["resizeMessages"] == ['1{"columns":95,"rows":19}']
assert result["input"] == "abcdefghijklmno"
assert result["scheduledDelays"].count(120) == 15
```

Retain and update the existing `390 -> 430 -> 390` tests to assert one 180 ms reload at each stable column width and zero `fit.fit()` calls on the old instance. Add a cancellation assertion that `pagehide` and WebSocket close leave no frame, 120 ms timer, or 180 ms timer queued.

- [ ] **Step 2: Run resize tests and observe repeated height fits**

Run: `pytest -q tests/test_remote_ui.py -k 'resize or geometry or column or keyboard'`

Expected: the 15-observation test reports 15 fits/resizes under the old requestAnimationFrame-only coordinator.

- [ ] **Step 3: Implement the two-window resize coordinator**

Replace the existing resize block with this state machine, retaining a synchronous first `fit.fit()` before authentication:

```javascript
let fitFrame = 0;
let heightFitTimer = 0;
let columnReloadTimer = 0;
let fitDeferredForSelection = false;
let resizeDisposed = false;

function clearResizeTimers(){
  if(fitFrame) cancelAnimationFrame(fitFrame);
  if(heightFitTimer) clearTimeout(heightFitTimer);
  if(columnReloadTimer) clearTimeout(columnReloadTimer);
  fitFrame = heightFitTimer = columnReloadTimer = 0;
}
function commitColumnReload(){
  columnReloadTimer = 0;
  if(resizeDisposed) return;
  if(term.hasSelection()){
    fitDeferredForSelection = true;
    return;
  }
  location.reload();
}
function commitHeightFit(){
  heightFitTimer = 0;
  if(resizeDisposed) return;
  try{
    const finalSize = fit.proposeDimensions();
    if(!finalSize) return;
    if(finalSize.cols !== term.cols){
      scheduleFit();
      return;
    }
    if(finalSize.rows !== term.rows) fit.fit();
  }catch(_){
    heightFitTimer = 0;
  }
}
function coordinateResize(){
  fitFrame = 0;
  if(resizeDisposed) return;
  try{
    const next = fit.proposeDimensions();
    if(!next) return;
    if(next.cols === term.cols && next.rows === term.rows){
      if(heightFitTimer) clearTimeout(heightFitTimer);
      if(columnReloadTimer) clearTimeout(columnReloadTimer);
      heightFitTimer = columnReloadTimer = 0;
      return;
    }
    if(next.cols !== term.cols){
      if(heightFitTimer) clearTimeout(heightFitTimer);
      heightFitTimer = 0;
      if(term.hasSelection()){
        fitDeferredForSelection = true;
        if(columnReloadTimer) clearTimeout(columnReloadTimer);
        columnReloadTimer = 0;
        return;
      }
      fitDeferredForSelection = false;
      if(columnReloadTimer) clearTimeout(columnReloadTimer);
      columnReloadTimer = setTimeout(commitColumnReload, 180);
      return;
    }
    if(columnReloadTimer) clearTimeout(columnReloadTimer);
    columnReloadTimer = 0;
    if(heightFitTimer) clearTimeout(heightFitTimer);
    heightFitTimer = setTimeout(commitHeightFit, 120);
  }catch(_){
    clearResizeTimers();
  }
}
function scheduleFit(){
  if(!resizeDisposed && !fitFrame) fitFrame = requestAnimationFrame(coordinateResize);
}
function disposeResizeCoordinator(){
  resizeDisposed = true;
  clearResizeTimers();
  resizeObserver?.disconnect();
}
```

Observe the terminal container once and route compatibility signals through `scheduleFit()`:

```javascript
const resizeObserver = typeof ResizeObserver === "function"
  ? new ResizeObserver(scheduleFit) : null;
resizeObserver?.observe(document.getElementById("term"));
window.addEventListener("resize", scheduleFit);
window.visualViewport?.addEventListener("resize", scheduleFit);
window.addEventListener("pagehide", disposeResizeCoordinator);
ws.addEventListener("close", disposeResizeCoordinator);
```

`resumeDeferredFitAfterSelection()` clears the deferred flag and calls `scheduleFit()` only after selection becomes empty.

- [ ] **Step 4: Keep data delivery independent of resize**

Name the direct path so the toolbar can wrap it in Task 5:

```javascript
function sendTerminalData(data){
  sendInput(data);
}
term.onData(sendTerminalData);
term.onResize(sendResize);
```

No timeout, frame, Promise, or queue belongs in `sendTerminalData()`.

- [ ] **Step 5: Verify resize regressions and commit**

Run: `pytest -q tests/test_remote_ui.py`

Expected: keyboard animation, stable columns, selection deferral, touch pane resize, scrolling, and direct input all pass.

Run: `bash tests/test_js_parses.sh`

Expected: exit status 0.

```bash
git add dash/term.html tests/test_remote_ui.py
git commit -m "Stabilize remote terminal resize on touch devices"
```

---

### Task 5: Add the Touch Toolbar and Parent Interaction Bridge

**Files:**
- Modify: `dash/term.html:31-55,84-145,281-303,555-564`
- Modify: `dash/index.html:662-670,863-866,2226-2268,2514-2537,3595-3600`
- Modify: `tests/test_remote_ui.py:51-118,1271-1465,1635-1670`

**Interfaces:**
- Produces: `controlByte(data: string) -> string|null`, `sendTerminalData(data)`, `sendToolbarKey(name)`, `pasteTerminalText(text)`, `requestClipboardPaste()`, and `focusTerminal()`.
- Produces iframe messages `{source:"comandos-term", type:"ready"|"interaction-request", selecting?: boolean}`.
- Consumes parent messages `{source:"comandos", type:"interaction-state", known, busy, selecting}`.
- Parent validates `event.origin === location.origin` and maps `event.source` to an existing same-origin iframe before changing tmux mouse state.

- [ ] **Step 1: Add failing byte, Ctrl, paste, focus, and bridge tests**

Extract the named toolbar helpers into a Node harness and assert:

```python
assert result["keys"] == [
    "\x1b", "\t", "\x1b[D", "\x1b[A", "\x1b[B", "\x1b[C"]
assert result["ctrlA"] == "\x01"
assert result["ctrlBracket"] == "\x1b"
assert result["composition"] == "á"
assert result["ctrlStillArmedAfterComposition"] is True
assert result["multiCharacter"] == "hello"
assert result["focusCalls"] >= 8
```

Clipboard success must call `term.paste("line one\nline two")` once and never call `sendInput()` directly. Clipboard denial must open `#paste-dialog`, keep terminal contents unchanged, and submit manual text through the same `term.paste()` path. Add parent bridge tests asserting pending state does not visually flip, confirmed API state does flip, stale responses remain ignored, tab switching preserves current semantics, and failure restores the last confirmed mode.

- [ ] **Step 2: Run toolbar tests and confirm controls are absent**

Run: `pytest -q tests/test_remote_ui.py -k 'toolbar or ctrl or clipboard or interaction or selection'`

Expected: missing toolbar/helper/bridge failures while existing backend interaction tests remain green.

- [ ] **Step 3: Add a non-overlay touch toolbar and manual paste dialog**

Change the terminal body to a stable grid:

```html
<div id="term-shell">
  <div id="term"></div>
  <div id="term-toolbar" role="toolbar" aria-label="Controles de terminal" hidden>
    <button type="button" data-action="mode" aria-pressed="false">Seleccionar</button>
    <button type="button" data-key="escape">Esc</button>
    <button type="button" data-action="ctrl" aria-pressed="false">Ctrl</button>
    <button type="button" data-key="tab">Tab</button>
    <button type="button" data-key="left" aria-label="Izquierda" title="Izquierda"><svg aria-hidden="true" viewBox="0 0 24 24"><path d="m15 18-6-6 6-6"/></svg></button>
    <button type="button" data-key="up" aria-label="Arriba" title="Arriba"><svg aria-hidden="true" viewBox="0 0 24 24"><path d="m18 15-6-6-6 6"/></svg></button>
    <button type="button" data-key="down" aria-label="Abajo" title="Abajo"><svg aria-hidden="true" viewBox="0 0 24 24"><path d="m6 9 6 6 6-6"/></svg></button>
    <button type="button" data-key="right" aria-label="Derecha" title="Derecha"><svg aria-hidden="true" viewBox="0 0 24 24"><path d="m9 18 6-6-6-6"/></svg></button>
    <button type="button" data-action="paste" aria-label="Pegar" title="Pegar"><svg aria-hidden="true" viewBox="0 0 24 24"><path d="M15 4h2a2 2 0 0 1 2 2v14H5V6a2 2 0 0 1 2-2h2"/><rect width="6" height="4" x="9" y="2" rx="1"/><path d="m9 14 2 2 4-4"/></svg></button>
  </div>
</div>
<dialog id="paste-dialog">
  <form method="dialog">
    <textarea id="paste-text" aria-label="Texto para pegar"></textarea>
    <div><button value="cancel">Cancelar</button><button id="paste-submit" value="default">Pegar</button></div>
  </form>
</dialog>
```

Use CSS grid rows `minmax(0,1fr) auto`; toolbar buttons are exactly 44 px high with stable minimum widths, the bar scrolls horizontally, and bottom safe area is supplied only by the outer app shell. Set `hidden = !IS_TOUCH` at startup so desktop geometry stays unchanged. The ResizeObserver from Task 4 notices toolbar visibility/height changes.

- [ ] **Step 4: Implement terminal key, Ctrl, clipboard, and focus behavior**

Add:

```javascript
const TOOLBAR_KEYS = Object.freeze({
  escape: "\x1b", tab: "\t", left: "\x1b[D", up: "\x1b[A",
  down: "\x1b[B", right: "\x1b[C",
});
let ctrlArmed = false;
let pasting = false;

function controlByte(data){
  if(typeof data !== "string" || data.length !== 1) return null;
  const code = data.charCodeAt(0);
  if(code === 32) return "\x00";
  if((code >= 64 && code <= 95) || (code >= 97 && code <= 122))
    return String.fromCharCode(code & 31);
  return null;
}
function setCtrlArmed(on){
  ctrlArmed = !!on;
  const button = document.querySelector('[data-action="ctrl"]');
  button?.classList.toggle("on", ctrlArmed);
  button?.setAttribute("aria-pressed", String(ctrlArmed));
}
function focusTerminal(){
  try{ term.textarea?.focus({preventScroll:true}); }catch(_){ term.focus(); }
}
function sendTerminalData(data){
  if(ctrlArmed && !pasting){
    const converted = controlByte(data);
    if(converted !== null){
      setCtrlArmed(false);
      sendInput(converted);
      return;
    }
  }
  sendInput(data);
}
function sendToolbarKey(name){
  const data = TOOLBAR_KEYS[name];
  if(data) sendInput(data);
  focusTerminal();
}
function pasteTerminalText(text){
  if(!text) return false;
  pasting = true;
  try{ term.paste(text); }finally{ pasting = false; }
  focusTerminal();
  return true;
}
async function requestClipboardPaste(){
  try{
    const text = await navigator.clipboard.readText();
    pasteTerminalText(text);
  }catch(_){
    document.getElementById("paste-dialog").showModal();
  }
}
```

Toolbar `pointerdown` calls `preventDefault()` to preserve focus; click dispatches key/action. Do not log clipboard or manual paste text.

- [ ] **Step 5: Move interaction state into a validated message bridge**

Remove `#term-select-toggle`, its CSS, click handler, and `frame.dataset.selecting`. In the parent add `postTermState(sess)` and call it from `applyTermInteraction(sess)`:

```javascript
function postTermState(sess){
  const frame = openTerms.get(sess)?.frame;
  const state = termInteraction.get(sess);
  if(!frame || frame.dataset.compat === "1") return false;
  frame.contentWindow?.postMessage({
    source:"comandos", type:"interaction-state",
    known:typeof state?.mouse === "boolean",
    busy:!!state?.busy,
    selecting:state?.mouse === false,
  }, location.origin);
  return true;
}
function applyTermInteraction(sess){
  postTermState(sess);
}
```

The parent message listener rejects other origins/sources, handles `ready` by sending current state, and handles `interaction-request` by calling `setTermSelectionMode(sess, !!data.selecting)`. In `term.html`, maintain `interactionState`, update the mode button only from confirmed parent messages, and make `selectingText()` return `interactionState.selecting`.

- [ ] **Step 6: Verify toolbar behavior and commit**

Run: `pytest -q tests/test_remote_ui.py tests/test_remote_controls.py`

Expected: every key byte, one-shot Ctrl, composition, paste success/denial, focus, selection, stale response, restore, touch scroll, and pane resize test passes.

Run: `bash tests/test_js_parses.sh`

Expected: exit status 0.

```bash
git add dash/index.html dash/term.html tests/test_remote_ui.py
git commit -m "Add touch terminal controls and interaction bridge"
```

---

### Task 6: Add Bruno Across Every Theme Surface

**Files:**
- Create: `tests/test_themes.py`
- Modify: `bin/cc-app:35-64,2153-2212`
- Modify: `bin/cc-dash:2354-2360`
- Modify: `dash/index.html:13-42,91-94,823,2514-2537,3030-3087`
- Modify: `dash/term.html:31-49,64-145`
- Modify: `DESIGN.md:1-35`

**Interfaces:**
- Produces theme id `bruno` in every whitelist/registry.
- Produces `applyTerminalTheme(name)` in the iframe and `broadcastTermTheme(name)` in the parent.
- Consumes `{source:"comandos", type:"theme", theme:string}` for same-origin live updates.
- Bruno ANSI order is exactly black, red, green, yellow, blue, magenta, cyan, white, then eight bright values from the approved specification.

- [ ] **Step 1: Write a failing cross-surface palette test**

Create `tests/test_themes.py`:

```python
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
APP = (ROOT / "bin" / "cc-app").read_text()
DASH = (ROOT / "bin" / "cc-dash").read_text()
INDEX = (ROOT / "dash" / "index.html").read_text()
TERM = (ROOT / "dash" / "term.html").read_text()
DESIGN = (ROOT / "DESIGN.md").read_text()

ANSI = [
    "#1A1A1A", "#DA462F", "#73E89A", "#FAD075",
    "#8BC2F9", "#D691ED", "#7DDFF2", "#CCCCCC",
    "#666666", "#F38172", "#73E89A", "#FAD075",
    "#8BC2F9", "#D691ED", "#7DDFF2", "#FFFFFF",
]


def test_bruno_registry_and_palette_exist_on_every_surface():
    for source in (APP, DASH, INDEX, TERM, DESIGN):
        assert "bruno" in source.lower()
    for color in ANSI:
        assert color in APP
        assert color in TERM
    for color in ("#1A1A1A", "#222224", "#26292B", "#333333",
                  "#444444", "#CCCCCC", "#AAAAAA", "#999999",
                  "#E4AE49", "#F6AB79", "#73E89A", "#8BC2F9"):
        assert color in INDEX


def test_all_five_theme_ids_are_documented_and_accepted():
    ids = ("noche", "dia", "calido", "termius", "bruno")
    for theme in ids:
        assert theme in DESIGN.lower()
        assert theme in DASH
        assert theme in INDEX
        assert theme in TERM


def test_same_origin_frames_receive_live_theme_without_reconnect():
    assert "function broadcastTermTheme" in INDEX
    assert 'type:"theme"' in INDEX
    assert "function applyTerminalTheme" in TERM
    assert "term.options.theme" in TERM
    assert "location.reload()" not in INDEX.split("function broadcastTermTheme", 1)[1].split("}", 1)[0]
```

- [ ] **Step 2: Run the palette test and confirm Bruno is absent**

Run: `pytest -q tests/test_themes.py`

Expected: all three tests fail on the missing `bruno` registry and palette.

- [ ] **Step 3: Register Bruno in GTK/VTE/tmux and backend preferences**

Add to `bin/cc-app`:

```python
PAL_BRUNO = [
    "#1A1A1A", "#DA462F", "#73E89A", "#FAD075", "#8BC2F9", "#D691ED",
    "#7DDFF2", "#CCCCCC", "#666666", "#F38172", "#73E89A", "#FAD075",
    "#8BC2F9", "#D691ED", "#7DDFF2", "#FFFFFF",
]
```

Add this theme entry:

```python
"bruno": dict(
    fg="#CCCCCC", bg="#1A1A1A", cursor="#E4AE49", pal=PAL_BRUNO,
    bar="#222224", dim="#AAAAAA", text="#CCCCCC", brand="#E4AE49",
    line="#333333", line2="#444444", faint="#999999"),
```

Add `"bruno"` to the `/prefs-set` tuple in `bin/cc-dash`. Existing `apply_theme()` and `_apply_tmux_theme()` then apply Bruno live to GTK CSS, all VTE terminals, and tmux status/borders.

- [ ] **Step 4: Add dashboard and xterm theme objects**

Add the dashboard role mapping exactly:

```css
:root[data-theme="bruno"]{
  --bg:#1A1A1A;--panel:#222224;--panel2:#26292B;--line:#333333;--line2:#444444;
  --text:#CCCCCC;--dim:#AAAAAA;--faint:#999999;--brand:#E4AE49;
  --waiting:#F6AB79;--done:#73E89A;--working:#8BC2F9;
  --glow:#1A1A1A;--code:#8BC2F9;--shadow:rgba(0,0,0,.5);
}
```

Add `bruno` and the currently missing `termius` to `dash/term.html`. The Bruno xterm object must include `background`, `foreground`, `cursor`, `cursorAccent`, `selectionBackground`, and all 16 xterm ANSI keys populated from `PAL_BRUNO`.

- [ ] **Step 5: Propagate theme live to existing custom frames**

Pass `theme=${encodeURIComponent(curTheme)}` when first assigning the iframe URL. Add:

```javascript
function broadcastTermTheme(name){
  for(const {frame} of openTerms.values()){
    if(!frame || frame.dataset.compat === "1") continue;
    frame.contentWindow?.postMessage(
      {source:"comandos", type:"theme", theme:name}, location.origin);
  }
}
```

Call `broadcastTermTheme(name)` at the end of `applyTheme()`. In the iframe:

```javascript
let activeTheme = THEMES[theme] ? theme : "noche";
function applyTerminalTheme(name){
  activeTheme = THEMES[name] ? name : "noche";
  const cfg = THEMES[activeTheme];
  term.options.theme = cfg;
  document.documentElement.style.background = cfg.background;
  document.body.style.background = cfg.background;
  document.getElementById("term-shell").style.background = cfg.background;
}
```

The iframe message listener validates `event.origin === location.origin`, calls `applyTerminalTheme(data.theme)`, and never reloads or reconnects the WebSocket.

- [ ] **Step 6: Update theme navigation and documentation**

Set:

```javascript
const THEME_SEQ = ["noche", "dia", "calido", "termius", "bruno"];
const THEME_ICON = {noche:"moon", dia:"sun", calido:"flame", termius:"sprout", bruno:"zap"};
```

Add the Lucide `zap` path to the existing `ICON` map, add localized display labels `Bruno`, and update the theme button tooltip. Update `DESIGN.md` from three themes to five and include the approved Bruno roles and ANSI source-of-truth statement.

- [ ] **Step 7: Verify all theme surfaces and commit**

Run: `pytest -q tests/test_themes.py tests/test_terminal_prefs.py tests/test_remote_ui.py`

Expected: all theme, preference, bridge, and terminal tests pass.

Run: `python3 -m py_compile bin/cc-app bin/cc-dash`

Expected: exit status 0.

Run: `bash tests/test_js_parses.sh`

Expected: exit status 0.

```bash
git add bin/cc-app bin/cc-dash dash/index.html dash/term.html DESIGN.md tests/test_themes.py tests/test_remote_ui.py
git commit -m "Add Bruno theme across ComandOS"
```

---

### Task 7: Browser Regression Harness, Recovery Check, and v1.6.0 Release

**Files:**
- Create: `tests/e2e_mobile_remote.js`
- Create: `docs/releases/v1.6.0.md`
- Modify only when a failing gate exposes a release-blocking defect: files already listed in Tasks 1-6 and their focused tests.

**Interfaces:**
- Consumes: `CC_REMOTE_BASE`, `CC_REMOTE_TOKEN`, global `playwright`, `/usr/bin/google-chrome`, and a disposable tmux session.
- Produces: screenshots under `/tmp/comandos-v160-e2e/`, exact tmux marker capture, viewport bounds JSON, and cleanup in `finally`.
- Produces: annotated git tag `v1.6.0` and GitHub release from `docs/releases/v1.6.0.md` only after every gate passes.

- [ ] **Step 1: Write the disposable Playwright harness**

Create `tests/e2e_mobile_remote.js` with this lifecycle:

```javascript
const {chromium} = require("playwright");
const {execFileSync} = require("node:child_process");
const fs = require("node:fs");

const base = process.env.CC_REMOTE_BASE;
const token = process.env.CC_REMOTE_TOKEN;
if(!base || !token) throw new Error("CC_REMOTE_BASE and CC_REMOTE_TOKEN are required");
const session = `comandos-e2e-${process.pid}`;
const output = "/tmp/comandos-v160-e2e";
const animated = "abcdefghijklmno";
const marker = `mobile-input-${animated}`;

function tmux(...args){
  return execFileSync("tmux", args, {encoding:"utf8"});
}
async function api(path, body){
  const response = await fetch(`${base}${path}`, {
    method:"POST",
    headers:{"Content-Type":"application/json", "X-Comandos-Token":token},
    body:JSON.stringify(body),
  });
  if(!response.ok) throw new Error(`${path}: ${response.status}`);
  return response.json();
}
async function openTerminal(page){
  await page.goto(`${base}/?token=${encodeURIComponent(token)}`, {waitUntil:"networkidle"});
  await page.locator(".apptab .lbl", {hasText:session}).click();
  const frame = page.frameLocator("iframe.termpane.on");
  await frame.locator(".xterm-helper-textarea").waitFor({state:"attached"});
  return frame;
}

(async () => {
  fs.mkdirSync(output, {recursive:true});
  tmux("new-session", "-d", "-s", session, "-x", "100", "-y", "30");
  await api("/tab-register", {session, label:session});
  const browser = await chromium.launch({
    executablePath:"/usr/bin/google-chrome", headless:true,
  });
  try{
    const phone = await browser.newContext({
      viewport:{width:390,height:844}, hasTouch:true, isMobile:true,
    });
    const page = await phone.newPage();
    const frame = await openTerminal(page);
    await frame.locator(".xterm-helper-textarea").focus();
    await page.keyboard.type("mobile-input-");
    for(let i = 0; i < 15; i++){
      await page.setViewportSize({width:390, height:844 - i * 12});
      await page.keyboard.type(animated[i]);
    }
    await page.keyboard.press("Enter");
    await page.waitForTimeout(500);
    await page.setViewportSize({width:390,height:844});
    await page.screenshot({path:`${output}/phone-390x844.png`, fullPage:true});
    const phoneBounds = await page.evaluate(() => ({
      viewport:[innerWidth, innerHeight],
      tabbar:document.querySelector("#tabbar").getBoundingClientRect().toJSON(),
      terminal:document.querySelector("#term-area").getBoundingClientRect().toJSON(),
    }));
    if(phoneBounds.tabbar.bottom > phoneBounds.terminal.top)
      throw new Error(`phone overlap: ${JSON.stringify(phoneBounds)}`);
    await phone.close();

    for(const view of [
      {name:"phone-320x568", width:320, height:568},
      {name:"tablet-834x1112", width:834, height:1112},
      {name:"tablet-1194x834", width:1194, height:834},
    ]){
      const context = await browser.newContext({
        viewport:{width:view.width,height:view.height}, hasTouch:true, isMobile:true,
      });
      const checkPage = await context.newPage();
      await openTerminal(checkPage);
      await checkPage.screenshot({path:`${output}/${view.name}.png`, fullPage:true});
      const geometry = await checkPage.evaluate(() => ({
        split:document.body.classList.contains("split"),
        canvas:[...document.querySelectorAll("iframe.termpane.on")].length,
        body:document.body.getBoundingClientRect().toJSON(),
      }));
      if(geometry.canvas !== 1 || geometry.body.width <= 0 || geometry.body.height <= 0)
        throw new Error(`${view.name}: ${JSON.stringify(geometry)}`);
      if(view.width < view.height && geometry.split)
        throw new Error(`${view.name}: portrait touch view split unexpectedly`);
      if(view.width > view.height && !geometry.split)
        throw new Error(`${view.name}: landscape touch view did not split`);
      await context.close();
    }

    const capture = tmux("capture-pane", "-p", "-t", `${session}:0.0`, "-S", "-80");
    if(!capture.includes(marker)) throw new Error(`marker missing from tmux: ${capture}`);
    console.log(JSON.stringify({session, marker, output, ok:true}));
  } finally {
    await browser.close();
    await api("/tab-close", {session}).catch(() => {});
    try{ tmux("kill-session", "-t", `=${session}`); }catch(_){ }
  }
})().catch(error => { console.error(error); process.exit(1); });
```

Before the phone screenshot, assert nine toolbar controls are visible and every control is at least 40 px high. Start `cat -v` only inside the disposable pane, click `Esc`, `Tab`, and all four arrow controls, arm `Ctrl`, type `a`, then use `tmux capture-pane` to assert the emitted control notation before sending Ctrl+C. Stub `navigator.clipboard.readText()` once with `line one\nline two` and once with a rejected promise; assert the first value reaches the disposable pane and the second opens `#paste-dialog`, then submit `manual paste` there. Generate 200 numbered lines inside `session`, drag vertically over `.xterm-screen`, and assert `.xterm-viewport.scrollTop` changes. Grant clipboard read/write permission, switch to Select through `[data-action="mode"]`, drag across visible text, press Ctrl+C, and assert the browser clipboard contains selected terminal text. Split only `session:0`, perform one long-press border drag in each axis, verify both disposable pane geometries changed, then join and remove only the added disposable pane before continuing.

- [ ] **Step 2: Run every automated gate**

Run: `pytest -q`

Expected: all Python tests pass with no failures or errors.

Run: `for test_file in tests/*.sh; do bash "$test_file"; done`

Expected: every shell fixture and embedded JavaScript parse test exits 0.

Run: `bash -n bin/cc-webterm bin/cc-mobile bin/cc-doctor && python3 -m py_compile bin/cc-app bin/cc-dash bin/cc_usage.py`

Expected: exit status 0 with no syntax output.

Run: `git diff --check`

Expected: exit status 0 with no whitespace errors.

- [ ] **Step 3: Record the pre-restart tmux inventory**

Run:

```bash
tmux list-panes -a -F '#{session_name}|#{window_index}|#{pane_index}|#{pane_id}|#{pane_current_path}|#{pane_current_command}|#{pane_pid}' | sort > /tmp/comandos-v160-before.txt
```

Expected: a non-empty inventory when user sessions exist. Do not alter its listed panes.

- [ ] **Step 4: Restart only ComandOS services and native client**

Run:

```bash
systemctl --user restart cc-dash.service
bin/cc-webterm off
bin/cc-webterm
old_app_pid=$(pgrep -f 'python3 .*/cc-app$' | head -1)
[ -z "$old_app_pid" ] || kill "$old_app_pid"
nohup "$HOME/.local/bin/cc-app" >/tmp/cc-app-v160.log 2>&1 &
```

Expected: `/prefs`, `/term/token`, and `:4779/token` each return HTTP 200; the new native process remains alive. Do not run `tmux kill-server` or restart a tmux service.

- [ ] **Step 5: Prove service crash recovery with a disposable session**

Create and register `comandos-recovery-v160`, attach it through the custom remote iframe, then record the primary unit PID:

```bash
before_pid=$(systemctl --user show -p MainPID --value cc-webterm-path.service)
systemctl --user kill --signal=ABRT cc-webterm-path.service
for attempt in $(seq 1 30); do
  curl -fsS --max-time 1 http://127.0.0.1:4780/term/token >/dev/null && break
  sleep 0.2
done
after_pid=$(systemctl --user show -p MainPID --value cc-webterm-path.service)
test "$before_pid" != "$after_pid"
curl -fsS http://127.0.0.1:4780/term/token >/dev/null
tmux has-session -t '=comandos-recovery-v160'
```

Expected: the unit transitions failed/restarting -> active in seconds, PID changes, remote state returns `degraded -> active`, and the disposable tmux session remains alive. Unregister and kill only `comandos-recovery-v160` afterward.

- [ ] **Step 6: Run phone/tablet browser checks and inspect pixels**

Derive the private base and token without printing the token to logs, then run:

```bash
NODE_PATH="$(npm root -g)" CC_REMOTE_BASE="https://$(tailscale status --json | jq -r '.Self.DNSName' | sed 's/\.$//')" CC_REMOTE_TOKEN="$(cat "$HOME/.claude/hooks/dash-token")" node tests/e2e_mobile_remote.js
```

Expected: JSON ends with `"ok":true`; the disposable marker is exact; screenshots exist for 320x568, 390x844, 834x1112, and 1194x834.

Open each screenshot with `view_image` and verify nonblank terminal pixels, one visible active terminal, no tab/content overlap, no clipped labels, no toolbar-covered rows, and correct portrait/landscape composition. Use Playwright element bounds to confirm each toolbar control is at least 40 px high and the tab controls are 44 px high.

- [ ] **Step 7: Verify Bruno live without reconnecting sessions**

In the disposable browser terminal, record the iframe WebSocket object count and tmux client count, cycle to Bruno, and assert the iframe remains loaded while computed values become `rgb(26, 26, 26)` background and `rgb(228, 174, 73)` brand/cursor. In the native client, select Bruno and inspect GTK/VTE plus tmux `status-style`, `pane-active-border-style`, and `message-style`; changing theme must not change the inventory or pane PIDs.

- [ ] **Step 8: Compare the post-restart inventory byte-for-byte**

Run:

```bash
tmux list-panes -a -F '#{session_name}|#{window_index}|#{pane_index}|#{pane_id}|#{pane_current_path}|#{pane_current_command}|#{pane_pid}' | sort > /tmp/comandos-v160-after.txt
diff -u /tmp/comandos-v160-before.txt /tmp/comandos-v160-after.txt
```

Expected: no diff after disposable test sessions are removed.

- [ ] **Step 9: Request final code review and fix only evidenced findings**

Invoke `superpowers:requesting-code-review` against the merge base of the v1.6.0 work. Re-run each focused test for an accepted finding, then repeat Steps 2, 6, and 8. Invoke `superpowers:verification-before-completion` before claiming the release is ready.

- [ ] **Step 10: Write and commit release evidence**

Create `docs/releases/v1.6.0.md` with sections `Fixed`, `Mobile and tablet`, `Bruno theme`, `Compatibility`, and `Verification`. Record exact pytest count, shell gate results, four viewport results, systemd PID recovery, and identical tmux inventories; state that `Teach this change` was intentionally not implemented.

```bash
git add tests/e2e_mobile_remote.js docs/releases/v1.6.0.md
git commit -m "Document and verify ComandOS v1.6.0"
```

- [ ] **Step 11: Publish v1.6.0 only from a clean verified branch**

Run:

```bash
git status --short --branch
git log --oneline origin/main..HEAD
git push origin main
git tag -a v1.6.0 -m "ComandOS v1.6.0"
git push origin v1.6.0
gh release create v1.6.0 --title "ComandOS v1.6.0" --notes-file docs/releases/v1.6.0.md
```

Expected: only known user-owned untracked paths may remain; all v1.6.0 commits are on `origin/main`; tag and GitHub release resolve to the verified commit.
