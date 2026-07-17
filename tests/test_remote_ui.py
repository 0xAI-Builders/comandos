#!/usr/bin/env python3
import json
import shutil
import subprocess
from pathlib import Path

import pytest


HTML = Path("dash/index.html").read_text()
SW = Path("dash/sw.js").read_text()
TERM_HTML = Path("dash/term.html").read_text()


def extract_js_function(source, name):
    needles = (f"async function {name}(", f"function {name}(")
    start = None
    for needle in needles:
        try:
            start = source.index(needle)
            break
        except ValueError:
            pass
    if start is None:
        raise ValueError(f"{name} not found")
    brace = source.index("{", start)
    depth = 0
    for i in range(brace, len(source)):
        ch = source[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return source[start : i + 1]
    raise AssertionError(f"Could not extract {name}")


def extract_js_initializer(source, name):
    marker = f"const {name} = "
    start = source.index(marker) + len(marker)
    opener = source[start]
    closer = {"{": "}", "[": "]"}[opener]
    depth = 0
    quote = None
    escaped = False
    for index in range(start, len(source)):
        char = source[index]
        if quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = None
            continue
        if char in "'\"`":
            quote = char
        elif char == opener:
            depth += 1
        elif char == closer:
            depth -= 1
            if depth == 0:
                return source[start:index + 1]
    raise AssertionError(f"Could not extract {name}")


def resize_coordinator_js():
    names = (
        "clearResizeTimers",
        "commitColumnReload",
        "commitHeightFit",
        "coordinateResize",
        "scheduleFit",
        "disposeResizeCoordinator",
    )
    return "\n\n".join(extract_js_function(TERM_HTML, name) for name in names)


def resize_lifecycle_wiring_js():
    statements = (
        "window.addEventListener('pagehide', disposeResizeCoordinator);",
        "ws.addEventListener('close', disposeResizeCoordinator);",
    )
    extracted = []
    for statement in statements:
        matches = [line.strip() for line in TERM_HTML.splitlines()
                   if line.strip() == statement]
        assert matches == [statement]
        extracted.extend(matches)
    return "\n".join(extracted)


def remote_button_state(state, busy=False):
    fn = extract_js_function(HTML, "remoteButtonState")
    script = f"""
{fn}
console.log(JSON.stringify(remoteButtonState({json.dumps(state)}, {str(busy).lower()})));
"""
    out = subprocess.check_output(["node", "-e", script], text=True)
    return json.loads(out)


def run_node_json(script):
    out = subprocess.check_output(["node", "-e", script], text=True)
    return json.loads(out)


def terminal_toolbar_js():
    names = (
        "controlByte",
        "setCtrlArmed",
        "focusTerminal",
        "sendTerminalData",
        "sendToolbarKey",
        "pasteTerminalText",
        "requestClipboardPaste",
        "applyInteractionState",
        "requestInteractionMode",
    )
    return "\n\n".join(extract_js_function(TERM_HTML, name) for name in names)


def run_terminal_toolbar(simulation):
    controller = terminal_toolbar_js()
    return run_node_json(f"""
const sent = [];
const pasted = [];
const posted = [];
let focusCalls = 0;
let ctrlArmed = false;
let pasting = false;
let interactionState = {{known:false, busy:false, selecting:false}};
const TOOLBAR_KEYS = Object.freeze({{
  escape: "\x1b", tab: "\t", left: "\x1b[D", up: "\x1b[A",
  down: "\x1b[B", right: "\x1b[C",
}});
const modeButton = {{
  textContent: "Seleccionar", disabled: false, attrs: {{}},
  classList: {{toggle() {{}}}},
  setAttribute(name, value) {{ this.attrs[name] = value; }},
}};
const ctrlButton = {{classList: {{toggle() {{}}}}, setAttribute() {{}}}};
const dialog = {{shown: 0, showModal() {{ this.shown += 1; }}}};
const pasteText = {{value: ""}};
const document = {{
  querySelector(sel) {{ return sel === '[data-action="ctrl"]' ? ctrlButton : sel === '[data-action="mode"]' ? modeButton : null; }},
  getElementById(id) {{ return id === "paste-dialog" ? dialog : id === "paste-text" ? pasteText : null; }},
}};
const term = {{
  textarea: {{focus() {{ focusCalls += 1; }} }},
  focus() {{ focusCalls += 1; }},
  paste(text) {{ pasted.push(text); }},
}};
const navigator = {{clipboard: {{readText: async () => "line one\\nline two"}}}};
const parent = {{postMessage(message, origin) {{ posted.push({{message, origin}}); }}}};
const location = {{origin: "https://dash.test"}};
function sendInput(data) {{ sent.push(data); }}

{controller}

(async () => {{
{simulation}
}})().catch(error => {{ console.error(error); process.exit(1); }});
""")


def terminal_toolbar_controller_js():
    start = TERM_HTML.index("  const TOOLBAR_KEYS")
    end = TERM_HTML.index("  // Teclado MOVIL", start)
    return extract_js_function(TERM_HTML, "handleTerminalMessage") + "\n" + TERM_HTML[start:end]


def run_terminal_toolbar_lifecycle(simulation):
    controller = terminal_toolbar_controller_js()
    return run_node_json(f"""
const listeners = new Map();
const pasted = [];
const sent = [];
const focusHistory = [];
let active = "opener";

function on(target, type, cb) {{
  const key = `${{target.id}}:${{type}}`;
  const entries = listeners.get(key) || [];
  entries.push(cb); listeners.set(key, entries);
}}
function emit(target, type, event = {{}}) {{
  for (const cb of listeners.get(`${{target.id}}:${{type}}`) || []) cb(event);
}}
function element(id) {{
  return {{
    id, dataset: {{}}, value: "", open: false, disabled: false,
    classList: {{toggle() {{}}, add() {{}}}},
    setAttribute() {{}},
    addEventListener(type, cb) {{ on(this, type, cb); }},
    closest() {{ return this; }},
  }};
}}

const toolbar = element("term-toolbar");
const form = element("paste-form");
const dialogElement = element("paste-dialog");
const pasteText = element("paste-text");
const pasteSubmit = element("paste-submit");
const pasteCancel = element("paste-cancel");
const modeButton = element("mode"); modeButton.dataset.action = "mode";
const ctrlButton = element("ctrl"); ctrlButton.dataset.action = "ctrl";
dialogElement.querySelector = selector => selector === "form" ? form : null;
dialogElement.showModal = () => {{ dialogElement.open = true; active = "paste-text"; }};

const document = {{
  querySelector(selector) {{
    if (selector === '[data-action="mode"]') return modeButton;
    if (selector === '[data-action="ctrl"]') return ctrlButton;
    return null;
  }},
  getElementById(id) {{
    return {{"term-toolbar":toolbar, "paste-dialog":dialogElement,
             "paste-text":pasteText}}[id] || null;
  }},
}};
const window = {{addEventListener(type, cb) {{ on({{id:"window"}}, type, cb); }}}};
const parent = window;
const location = {{origin:"https://dash.test"}};
const term = {{
  textarea: {{focus() {{ active = "terminal"; focusHistory.push(active); }}}},
  focus() {{ active = "terminal"; focusHistory.push(active); }},
  paste(text) {{ pasted.push(text); }},
  attachCustomKeyEventHandler() {{}},
}};
const navigator = {{clipboard: {{readText: async () => ""}}}};
function sendInput(data) {{ sent.push(data); }}
function finishDialog(submitter, canceled = false) {{
  if (submitter) emit(form, "submit", {{submitter}});
  if (canceled) emit(dialogElement, "cancel", {{}});
  active = "opener";
  dialogElement.open = false;
  emit(dialogElement, "close", {{}});
}}

{controller}

(async () => {{
{simulation}
}})().catch(error => {{ console.error(error); process.exit(1); }});
""")


def test_terminal_touch_toolbar_ctrl_clipboard_focus_and_confirmed_mode():
    result = run_terminal_toolbar("""
const keys = ["escape", "tab", "left", "up", "down", "right"].map(name => {
  sendToolbarKey(name);
  return sent.at(-1);
});
setCtrlArmed(true);
sendTerminalData("a");
const ctrlA = sent.at(-1);
setCtrlArmed(true);
sendTerminalData("[");
const ctrlBracket = sent.at(-1);
setCtrlArmed(true);
sendTerminalData("á");
const composition = sent.at(-1);
const ctrlStillArmedAfterComposition = ctrlArmed;
sendTerminalData("hello");
const multiCharacter = sent.at(-1);
await requestClipboardPaste();
navigator.clipboard.readText = async () => { throw new Error("denied"); };
await requestClipboardPaste();
pasteText.value = "manual text";
pasteTerminalText(pasteText.value);
applyInteractionState({known:true, busy:true, selecting:false});
const pendingLabel = modeButton.textContent;
applyInteractionState({known:true, busy:false, selecting:true});
const confirmedLabel = modeButton.textContent;
requestInteractionMode();
console.log(JSON.stringify({
  keys, ctrlA, ctrlBracket, composition, ctrlStillArmedAfterComposition,
  multiCharacter, pasted, sent, focusCalls, dialogShown: dialog.shown,
  pendingLabel, confirmedLabel, posted,
}));
""")

    assert result["keys"] == ["\x1b", "\t", "\x1b[D", "\x1b[A", "\x1b[B", "\x1b[C"]
    assert result["ctrlA"] == "\x01"
    assert result["ctrlBracket"] == "\x1b"
    assert result["composition"] == "á"
    assert result["ctrlStillArmedAfterComposition"] is True
    assert result["multiCharacter"] == "hello"
    assert result["pasted"] == ["line one\nline two", "manual text"]
    assert "line one\nline two" not in result["sent"]
    assert result["dialogShown"] == 1
    assert result["focusCalls"] == 7
    assert result["pendingLabel"] == "Seleccionar"
    assert result["confirmedLabel"] == "Interactuar"
    assert result["posted"] == [{
        "message": {"source": "comandos-term", "type": "interaction-request", "selecting": False},
        "origin": "https://dash.test",
    }]


def test_terminal_toolbar_starts_unknown_and_keeps_touch_panning_native():
    assert "applyInteractionState(interactionState);" in TERM_HTML
    assert "touch-action: pan-x" in TERM_HTML
    assert "if (e.pointerType !== 'touch') e.preventDefault();" in TERM_HTML


def test_selected_ctrl_c_copies_without_sending_sigint():
    controller = "\n\n".join(
        extract_js_function(TERM_HTML, name)
        for name in ("copyTerminalSelection", "handleTerminalKeyEvent")
    )
    script = r"""
const writes = [];
let selection = "SELECT-COPY-KNOWN";
const term = {
  hasSelection() { return selection.length > 0; },
  getSelection() { return selection; },
};
const navigator = {clipboard: {writeText: async text => { writes.push(text); }}};

__CONTROLLER__

(async () => {
  const selectedResult = handleTerminalKeyEvent({
    type:"keydown", key:"c", ctrlKey:true, metaKey:false, altKey:false,
  });
  await Promise.resolve();
  selection = "";
  const plainCtrlCResult = handleTerminalKeyEvent({
    type:"keydown", key:"c", ctrlKey:true, metaKey:false, altKey:false,
  });
  const keyupResult = handleTerminalKeyEvent({
    type:"keyup", key:"c", ctrlKey:true, metaKey:false, altKey:false,
  });
  console.log(JSON.stringify({selectedResult, plainCtrlCResult, keyupResult, writes}));
})().catch(error => { console.error(error); process.exit(1); });
"""
    result = run_node_json(script.replace("__CONTROLLER__", controller))

    assert result == {
        "selectedResult": False,
        "plainCtrlCResult": True,
        "keyupResult": True,
        "writes": ["SELECT-COPY-KNOWN"],
    }
    assert "term.attachCustomKeyEventHandler(handleTerminalKeyEvent);" in TERM_HTML


def test_terminal_paste_focus_follows_actual_dialog_close_lifecycle():
    result = run_terminal_toolbar_lifecycle("""
await requestClipboardPaste();
const emptyClipboard = {active, focusCalls: focusHistory.length, pasted:[...pasted]};

navigator.clipboard.readText = async () => { throw new Error("denied"); };
await requestClipboardPaste();
pasteText.value = "manual text";
finishDialog(pasteSubmit);
const manualSubmit = {active, focusCalls: focusHistory.length, pasted:[...pasted]};

pasteDialog.showModal();
finishDialog(pasteCancel);
const cancelButton = {active, focusCalls: focusHistory.length, pasted:[...pasted]};

pasteDialog.showModal();
finishDialog(null, true);
const escapeCancel = {active, focusCalls: focusHistory.length, pasted:[...pasted]};
console.log(JSON.stringify({emptyClipboard, manualSubmit, cancelButton, escapeCancel, sent}));
""")

    assert result["emptyClipboard"] == {
        "active": "terminal", "focusCalls": 1, "pasted": []
    }
    assert result["manualSubmit"] == {
        "active": "terminal", "focusCalls": 2, "pasted": ["manual text"]
    }
    assert result["cancelButton"] == {
        "active": "terminal", "focusCalls": 3, "pasted": ["manual text"]
    }
    assert result["escapeCancel"] == {
        "active": "terminal", "focusCalls": 4, "pasted": ["manual text"]
    }
    assert result["sent"] == []


def touch_controller_js():
    start = TERM_HTML.index("  const screenEl = () =>")
    end = TERM_HTML.index("  const ta = term.textarea;", start)
    return TERM_HTML[start:end]


def term_interaction_js():
    names = (
        "termInteractionState",
        "postTermState",
        "applyTermInteraction",
        "handleTermFrameMessage",
        "syncTermInteraction",
        "setTermSelectionMode",
        "restoreTermInteraction",
        "sendTermInteractionKeepalive",
        "cleanupTermInteraction",
        "restoreInactiveTermInteractions",
        "restoreAllTermInteractions",
        "handleTermInteractionsPageShow",
    )
    return "\n\n".join(extract_js_function(HTML, name) for name in names)


def run_term_interaction(simulation):
    controller = term_interaction_js()
    return run_node_json(f"""
const termInteraction = new Map();
let termInteractionSeq = 0;
let activeTerm = "ssh-prod";
let termPageHidden = false;
const classNames = new Set();
const postedStates = [];
const frames = {{
  "ssh-prod": {{dataset: {{}}, contentWindow: {{postMessage(message, origin) {{ postedStates.push({{sess:"ssh-prod", message, origin}}); }}}}}},
  "dev": {{dataset: {{}}, contentWindow: {{postMessage(message, origin) {{ postedStates.push({{sess:"dev", message, origin}}); }}}}}},
  "alpha": {{dataset: {{}}, contentWindow: {{postMessage(message, origin) {{ postedStates.push({{sess:"alpha", message, origin}}); }}}}}},
  "beta": {{dataset: {{}}, contentWindow: {{postMessage(message, origin) {{ postedStates.push({{sess:"beta", message, origin}}); }}}}}},
  "compat": {{dataset: {{compat:"1"}}, contentWindow: {{postMessage(message, origin) {{ postedStates.push({{sess:"compat", message, origin}}); }}}}}},
}};
const openTerms = new Map(Object.entries(frames).map(([sess, frame]) => [sess, {{frame}}]));
const location = {{origin:"https://dash.test"}};
const toasts = [];
const keepalives = [];
const tf = (_es, en) => en;
const toast = (message, error) => toasts.push({{message, error: !!error}});
const authToken = () => "test-token";
let keepaliveHook = null;
const fetch = async (path, options) => {{
  const item = {{path, options}};
  keepalives.push(item);
  if(keepaliveHook) keepaliveHook(item);
  return {{ok:true}};
}};
let api;

{controller}

(async () => {{
{simulation}
}})().catch(error => {{ console.error(error); process.exit(1); }});
""")


def run_touch_controller(
    simulation,
    *,
    cells=None,
    viewport_y=0,
    mouse_on=True,
    selecting=False,
    socket_open=True,
):
    """Execute the real iframe touch controller with deterministic I/O."""
    controller = touch_controller_js()
    script = f"""
const listeners = {{}};
const windowListeners = {{}};
const wsListeners = {{}};
const timers = new Map();
const frames = new Map();
const lineReads = [];
const sent = [];
const wheels = [];
const refreshCalls = [];
const scheduledDelays = [];
let nextTimer = 1;
let nextFrame = 1;
let indicator = null;
const cells = {json.dumps(cells or {})};

function setTimeout(cb, delay) {{
  const id = nextTimer++; timers.set(id, cb); scheduledDelays.push(delay); return id;
}}
function clearTimeout(id) {{ timers.delete(id); }}
function runTimers() {{
  const queued = [...timers.values()];
  timers.clear();
  queued.forEach(cb => cb());
}}
function requestAnimationFrame(cb) {{ const id = nextFrame++; frames.set(id, cb); return id; }}
function cancelAnimationFrame(id) {{ frames.delete(id); }}
function flushFrames() {{
  const queued = [...frames.values()];
  frames.clear();
  queued.forEach(cb => cb());
}}

class FakeWheelEvent {{
  constructor(type, init) {{ this.type = type; Object.assign(this, init); }}
}}
const wheelTarget = {{
  dispatchEvent(ev) {{
    if (ev.type === 'wheel') wheels.push({{deltaY: ev.deltaY, clientY: ev.clientY}});
    return true;
  }},
  getBoundingClientRect() {{ return {{left: 0, top: 0, width: 800, height: 480}}; }},
}};
const document = {{
  body: {{appendChild(el) {{ indicator = el; }}}},
  createElement() {{ return {{style: {{display: 'none'}}, textContent: ''}}; }},
  querySelector(sel) {{ return sel === '.xterm-screen' ? wheelTarget : null; }},
  elementFromPoint() {{ return wheelTarget; }},
  addEventListener(type, cb) {{ listeners[type] = cb; }},
}};
const window = {{
  WheelEvent: FakeWheelEvent,
  addEventListener(type, cb) {{ windowListeners[type] = cb; }},
}};
const navigator = {{vibrate() {{}}}};
const interactionState = {{known: {str(selecting).lower()}, busy: false, selecting: {str(selecting).lower()}}};
const term = {{
  cols: 80,
  rows: 24,
  modes: {{mouseTrackingMode: {json.dumps('sgr' if mouse_on else 'none')}}},
  refresh(first, last) {{ refreshCalls.push([first, last]); }},
  onWriteParsed(cb) {{ this.writeParsedHandler = cb; }},
  buffer: {{active: {{
    viewportY: {viewport_y},
    getLine(index) {{
      lineReads.push(index);
      const line = cells[String(index)];
      if (!line) return null;
      return {{getCell(cellIndex) {{
        const value = line[String(cellIndex)];
        return value === undefined ? null : {{getChars() {{ return value; }}}};
      }}}};
    }},
  }}}},
}};
const ws = {{
  readyState: {1 if socket_open else 0},
  addEventListener(type, cb) {{ (wsListeners[type] ||= []).push(cb); }},
}};
function sendInput(data) {{ if (ws.readyState === 1) sent.push(data); }}
function point(col, row, dx = 0, dy = 0, identifier = 7) {{
  return {{
    clientX: (col - 0.5) * 10 + dx,
    clientY: (row - 0.5) * 20 + dy,
    screenX: (col - 0.5) * 10 + dx,
    screenY: (row - 0.5) * 20 + dy,
    identifier,
  }};
}}
function event(touches, changedTouches = touches) {{
  return {{
    touches,
    changedTouches,
    target: wheelTarget,
    prevented: false,
    stopped: false,
    preventDefault() {{ this.prevented = true; }},
    stopPropagation() {{ this.stopped = true; }},
  }};
}}
function touchList(...touches) {{
  const list = {{length: touches.length, item(index) {{ return touches[index] || null; }}}};
  touches.forEach((touch, index) => {{ list[index] = touch; }});
  return list;
}}

{controller}

{simulation}
"""
    return run_node_json(script)


def run_chrome_toolbar_touch_drag():
    chrome = shutil.which("google-chrome") or shutil.which("chromium")
    if not chrome:
        pytest.skip("Chrome/Chromium is not installed")
    term_url = (Path.cwd() / "dash" / "term.html").resolve().as_uri()
    script = f"""
const {{spawn}} = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");
const chrome = {json.dumps(chrome)};
const termUrl = {json.dumps(term_url)};
const port = 9300 + Math.floor(Math.random() * 500);
const profile = fs.mkdtempSync(path.join(os.tmpdir(), "comandos-toolbar-"));
const browser = spawn(chrome, [
  "--headless=new", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
  "--no-first-run", "--no-default-browser-check", "--allow-file-access-from-files",
  `--remote-debugging-port=${{port}}`, `--user-data-dir=${{profile}}`, "about:blank",
], {{stdio:"ignore"}});
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));

async function jsonAt(route) {{
  const response = await fetch(`http://127.0.0.1:${{port}}${{route}}`);
  if (!response.ok) throw new Error(`CDP HTTP ${{response.status}}`);
  return response.json();
}}

async function connect() {{
  let pages;
  for (let attempt = 0; attempt < 100; attempt++) {{
    try {{ pages = await jsonAt("/json/list"); if (pages.length) break; }} catch (_) {{}}
    await delay(50);
  }}
  if (!pages?.length) throw new Error("Chrome CDP did not start");
  const page = pages.find(target => target.type === "page" && target.url === "about:blank") ||
    pages.find(target => target.type === "page");
  if (!page) throw new Error("Chrome page target was not found");
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {{
    ws.addEventListener("open", resolve, {{once:true}});
    ws.addEventListener("error", reject, {{once:true}});
  }});
  let nextId = 1;
  const pending = new Map();
  ws.addEventListener("message", event => {{
    const message = JSON.parse(event.data);
    if (!message.id) return;
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    if (message.error) request.reject(new Error(message.error.message));
    else request.resolve(message.result || {{}});
  }});
  const send = (method, params = {{}}) => new Promise((resolve, reject) => {{
    const id = nextId++;
    pending.set(id, {{resolve, reject}});
    ws.send(JSON.stringify({{id, method, params}}));
  }});
  return {{ws, send}};
}}

(async () => {{
  let connection;
  try {{
    connection = await connect();
    const {{send}} = connection;
    await send("Page.enable");
    await send("Runtime.enable");
    await send("Emulation.setDeviceMetricsOverride", {{
      width:360, height:640, deviceScaleFactor:1, mobile:true,
    }});
    await send("Emulation.setTouchEmulationEnabled", {{enabled:true, maxTouchPoints:1}});
    await send("Page.navigate", {{url:termUrl}});
    let geometry;
    let lastProbe;
    for (let attempt = 0; attempt < 100; attempt++) {{
      try {{
        const evaluated = await send("Runtime.evaluate", {{
          expression:`(() => {{
            const toolbar = document.getElementById("term-toolbar");
            const probe = {{url:location.href, ready:document.readyState,
                           exists:!!toolbar, hidden:toolbar?.hidden,
                           clientWidth:toolbar?.clientWidth || 0,
                           scrollWidth:toolbar?.scrollWidth || 0,
                           innerWidth:window.innerWidth}};
            if (!toolbar || toolbar.hidden || toolbar.scrollWidth <= toolbar.clientWidth)
              return {{probe}};
            toolbar.scrollLeft = 0;
            window.__toolbarTouchMoves = [];
            window.__toolbarPreventCalls = [];
            const nativePreventDefault = Event.prototype.preventDefault;
            Event.prototype.preventDefault = function() {{
              window.__toolbarPreventCalls.push({{
                type:this.type, target:this.target?.id || this.target?.tagName || "",
                pointerType:this.pointerType || "", stack:new Error().stack,
              }});
              return nativePreventDefault.call(this);
            }};
            document.addEventListener("touchmove", event => {{
              window.__toolbarTouchMoves.push(event.defaultPrevented);
            }}, {{passive:true}});
            const rect = toolbar.getBoundingClientRect();
            const keyRect = toolbar.querySelector('[data-key="escape"]').getBoundingClientRect();
            return {{geometry:{{left:rect.left, right:rect.right, top:rect.top, height:rect.height,
                     clientWidth:toolbar.clientWidth, scrollWidth:toolbar.scrollWidth,
                     viewportWidth:innerWidth, documentScrollX:scrollX,
                     tapX:keyRect.left + keyRect.width / 2,
                     tapY:keyRect.top + keyRect.height / 2}}, probe}};
          }})()`, returnByValue:true,
        }});
        lastProbe = evaluated.result?.value?.probe;
        geometry = evaluated.result?.value?.geometry;
        if (geometry) break;
      }} catch (_) {{}}
      await delay(50);
    }}
    if (!geometry) throw new Error(`touch toolbar did not become scrollable: ${{JSON.stringify(lastProbe)}}`);
    await send("Input.dispatchTouchEvent", {{
      type:"touchStart", touchPoints:[{{x:geometry.tapX,y:geometry.tapY,id:2}}],
    }});
    await send("Input.dispatchTouchEvent", {{type:"touchEnd", touchPoints:[]}});
    await delay(100);
    const y = geometry.top + geometry.height / 2;
    const startX = geometry.right - 12;
    const endX = geometry.left + 20;
    await send("Input.dispatchTouchEvent", {{type:"touchStart", touchPoints:[{{x:startX,y,id:1}}]}});
    for (let step = 1; step <= 8; step++) {{
      const x = startX + (endX - startX) * step / 8;
      await send("Input.dispatchTouchEvent", {{type:"touchMove", touchPoints:[{{x,y,id:1}}]}});
      await delay(20);
    }}
    await send("Input.dispatchTouchEvent", {{type:"touchEnd", touchPoints:[]}});
    await delay(150);
    const evaluated = await send("Runtime.evaluate", {{
      expression:`(() => {{
        const toolbar = document.getElementById("term-toolbar");
        return {{scrollLeft:toolbar.scrollLeft,
                 prevented:window.__toolbarTouchMoves,
                 preventCalls:window.__toolbarPreventCalls,
                 activeTag:document.activeElement?.tagName || ""}};
      }})()`, returnByValue:true,
    }});
    const dragResult = evaluated.result.value;
    await send("Runtime.evaluate", {{expression:`(() => {{
      const dialog = document.getElementById("paste-dialog");
      dialog.showModal();
      document.getElementById("paste-text").value = "browser manual";
      dialog.querySelector("form").requestSubmit(document.getElementById("paste-submit"));
    }})()`}});
    await delay(100);
    const manual = (await send("Runtime.evaluate", {{
      expression:`({{activeTag:document.activeElement?.tagName || "",
                    open:document.getElementById("paste-dialog").open}})`, returnByValue:true,
    }})).result.value;
    await send("Runtime.evaluate", {{expression:`(() => {{
      const dialog = document.getElementById("paste-dialog");
      dialog.showModal();
      dialog.querySelector('button[value="cancel"]').click();
    }})()`}});
    await delay(100);
    const cancel = (await send("Runtime.evaluate", {{
      expression:`({{activeTag:document.activeElement?.tagName || "",
                    open:document.getElementById("paste-dialog").open}})`, returnByValue:true,
    }})).result.value;
    await send("Runtime.evaluate", {{expression:`document.getElementById("paste-dialog").showModal()`}});
    await send("Input.dispatchKeyEvent", {{type:"keyDown", key:"Escape", code:"Escape",
      windowsVirtualKeyCode:27, nativeVirtualKeyCode:27}});
    await send("Input.dispatchKeyEvent", {{type:"keyUp", key:"Escape", code:"Escape",
      windowsVirtualKeyCode:27, nativeVirtualKeyCode:27}});
    await delay(100);
    const escape = (await send("Runtime.evaluate", {{
      expression:`({{activeTag:document.activeElement?.tagName || "",
                    open:document.getElementById("paste-dialog").open}})`, returnByValue:true,
    }})).result.value;
    console.log(JSON.stringify({{...geometry, ...dragResult,
      dialogFocus:{{manual, cancel, escape}}}}));
  }} finally {{
    try {{ connection?.ws.close(); }} catch (_) {{}}
    browser.kill("SIGTERM");
    if (browser.exitCode === null) {{
      await Promise.race([
        new Promise(resolve => browser.once("exit", resolve)),
        delay(2000),
      ]);
    }}
    if (browser.exitCode === null) {{
      browser.kill("SIGKILL");
      await new Promise(resolve => browser.once("exit", resolve));
    }}
    for (let attempt = 0; attempt < 10; attempt++) {{
      try {{ fs.rmSync(profile, {{recursive:true, force:true}}); break; }}
      catch (error) {{ if (attempt === 9) throw error; await delay(50); }}
    }}
  }}
}})().catch(error => {{ console.error(error); process.exit(1); }});
"""
    output = subprocess.check_output(["node", "-e", script], text=True, timeout=30)
    return json.loads(output)


def valid_vertical_cells(viewport_y=0, col=40, row=10):
    return {
        viewport_y + visible_row - 1: {col - 1: "│"}
        for visible_row in (row - 1, row, row + 1)
    }


def valid_horizontal_cells(viewport_y=0, col=20, row=12):
    return {
        viewport_y + row - 1: {
            visible_col - 1: "─" for visible_col in (col - 1, col, col + 1)
        }
    }


def run_split_drag(stop_type, flush_before_stop):
    functions = "\n\n".join(
        extract_js_function(HTML, name)
        for name in (
            "splitBounds", "setSplitLeft", "restoreSplitLeft", "initSplitDrag"
        )
    )
    script = f"""
const SPLIT_KEY = "cc-split-left";
{functions}
const splitterListeners = {{}};
const windowListeners = {{}};
const bodyClasses = new Set(["split"]);
const frames = [{{style:{{pointerEvents:""}}}}, {{style:{{pointerEvents:""}}}}];
const cssWrites = [];
const storageWrites = [];
let rectReads = 0;
let released = 0;
let nextFrame = 1;
const frameQueue = new Map();
const panes = {{
  clientWidth: 1200,
  getBoundingClientRect(){{ rectReads += 1; return {{left: 100}}; }},
  style: {{setProperty(_name, value){{ cssWrites.push(value); }}}},
}};
const splitter = {{
  _wired: false,
  addEventListener(type, cb){{ splitterListeners[type] = cb; }},
  setPointerCapture(){{}},
  releasePointerCapture(){{ released += 1; }},
}};
const document = {{
  body: {{classList: {{
    contains(name){{ return bodyClasses.has(name); }},
    add(name){{ bodyClasses.add(name); }},
    remove(name){{ bodyClasses.delete(name); }},
  }}}},
  getElementById(id){{ return id === "splitter" ? splitter : id === "panes" ? panes : null; }},
  querySelectorAll(sel){{ return sel === ".termpane" ? frames : []; }},
}};
const window = {{
  innerWidth: 1200,
  addEventListener(type, cb){{ windowListeners[type] = cb; }},
  removeEventListener(type, cb){{ if(windowListeners[type] === cb) delete windowListeners[type]; }},
}};
const localStorage = {{
  values: new Map(),
  getItem(key){{ return this.values.get(key) || null; }},
  setItem(key, value){{ this.values.set(key, value); storageWrites.push([key, value]); }},
}};
function requestAnimationFrame(cb){{ const id = nextFrame++; frameQueue.set(id, cb); return id; }}
function cancelAnimationFrame(id){{ frameQueue.delete(id); }}
function flushFrames(){{
  const queued = [...frameQueue.values()];
  frameQueue.clear();
  queued.forEach(cb => cb());
}}
const event = (pointerId, clientX, type = "pointermove") => ({{
  type, pointerId, clientX, button: 0,
  preventDefault(){{ this.prevented = true; }},
}});

initSplitDrag();
splitterListeners.pointerdown(event(7, 420));
for(let i = 0; i < 240; i++) windowListeners.pointermove(event(7, 430 + i));
windowListeners.pointermove(event(99, 999));
const cssWritesBeforeFrame = cssWrites.length;
const storageWritesBeforeStop = storageWrites.length;
if({str(flush_before_stop).lower()}) flushFrames();
const cssWritesAfterFrame = cssWrites.length;
const stopX = {"700" if stop_type == "pointerup" else "669"};
windowListeners[{json.dumps(stop_type)}](event(7, stopX, {json.dumps(stop_type)}));
const finalCss = cssWrites.at(-1) || "";
console.log(JSON.stringify({{
  rectReads,
  cssWritesBeforeFrame,
  cssWritesAfterFrame,
  cssWritesAfterStop: cssWrites.length,
  storageWritesBeforeStop,
  storageWritesAfterStop: storageWrites.length,
  storedValue: localStorage.values.get(SPLIT_KEY),
  finalValue: finalCss.replace(/px$/, ""),
  framesRestored: frames.every(frame => frame.style.pointerEvents === ""),
  draggingCleared: !bodyClasses.has("dragging"),
  pendingFrames: frameQueue.size,
  released,
}}));
"""
    return run_node_json(script)


def test_split_drag_coalesces_moves_and_persists_once():
    result = run_split_drag("pointerup", True)

    assert result["rectReads"] == 1
    assert result["cssWritesBeforeFrame"] == 0
    assert result["cssWritesAfterFrame"] == 1
    assert result["cssWritesAfterStop"] == 2  # one newer pointerup coordinate
    assert result["storageWritesBeforeStop"] == 0
    assert result["storageWritesAfterStop"] == 1
    assert result["storedValue"] == result["finalValue"]
    assert result["finalValue"] == "600"  # pointerup's newer x=700 wins
    assert result["framesRestored"] is True
    assert result["draggingCleared"] is True
    assert result["pendingFrames"] == 0
    assert result["released"] == 1


def test_split_drag_cancel_flushes_last_value_and_restores_frames():
    result = run_split_drag("pointercancel", False)

    assert result["rectReads"] == 1
    assert result["cssWritesBeforeFrame"] == 0
    assert result["cssWritesAfterStop"] == 1
    assert result["storageWritesBeforeStop"] == 0
    assert result["storageWritesAfterStop"] == 1
    assert result["storedValue"] == result["finalValue"]
    assert result["finalValue"] == "569"  # unrelated pointerId=99 was ignored
    assert result["framesRestored"] is True
    assert result["draggingCleared"] is True
    assert result["pendingFrames"] == 0


def test_splitter_hit_target_is_wider_than_visible_column():
    assert "body.app.split #splitter::before{content:\"\";position:absolute;inset:0 -12px}" in HTML


def test_split_policy_uses_pointer_class_and_stable_coarse_orientation():
    should_split = extract_js_function(HTML, "shouldSplitLayout")
    result = run_node_json(f"""
{should_split}
const cases = {json.dumps([
    [390, 844, True, False],
    [834, 1112, True, False],
    [1194, 834, True, True],
    [899, 700, False, False],
    [900, 700, False, True],
    [747, 500, True, False],
    [748, 500, True, True],
])};
console.log(JSON.stringify(cases.map(([width, height, coarse, expected]) =>
  shouldSplitLayout(width, height, coarse) === expected)));
""")
    assert result == [True] * 7


def test_portrait_tablet_keyboard_does_not_enable_split_layout():
    functions = "\n\n".join(
        extract_js_function(HTML, name)
        for name in ("currentViewportHeight", "shouldSplitLayout", "applyAppLayout")
    )
    result = run_node_json(f"""
let split = false;
const classList = {{
  toggle(name, enabled) {{ if(name === "split") split = enabled; }},
}};
const document = {{
  documentElement: {{clientHeight: 420}},
  body: {{classList}},
}};
const window = {{
  visualViewport: {{width: 834, height: 420}},
  innerWidth: 834,
  innerHeight: 420,
  screen: {{width: 834, height: 1112, orientation: {{type: "portrait-primary"}}}},
  matchMedia() {{ return {{matches: true}}; }},
}};
const navigator = {{maxTouchPoints: 5}};
function restoreSplitLeft() {{}}
function showView() {{}}
let activeView = "panel";
{functions}
applyAppLayout();
console.log(JSON.stringify({{split}}));
""")
    assert result == {"split": False}


def test_active_tab_reveal_does_not_schedule_for_unchanged_visible_tab():
    reveal = extract_js_function(HTML, "revealActiveTab")
    result = run_node_json(f"""
let activeView = "term:prod";
const scheduled = [];
const calls = [];
function requestAnimationFrame(callback) {{ scheduled.push(callback); }}
const tab = {{
  getBoundingClientRect() {{ return {{left: 20, right: 80}}; }},
  scrollIntoView(options) {{ calls.push(options); }},
}};
const bar = {{
  getBoundingClientRect() {{ return {{left: 0, right: 100}}; }},
  querySelector(selector) {{ return selector === ".apptab.on" ? tab : null; }},
}};
{reveal}
revealActiveTab.lastView = activeView;
revealActiveTab(bar);
scheduled.forEach(callback => callback());
console.log(JSON.stringify({{scheduled: scheduled.length, calls}}));
""")
    assert result == {"scheduled": 0, "calls": []}


def test_active_tab_reveal_schedules_for_unchanged_rebuilt_out_of_bounds_tab():
    reveal = extract_js_function(HTML, "revealActiveTab")
    result = run_node_json(f"""
let activeView = "term:prod";
const scheduled = [];
const calls = [];
function requestAnimationFrame(callback) {{ scheduled.push(callback); }}
const tab = {{
  getBoundingClientRect() {{ return {{left: 120, right: 180}}; }},
  scrollIntoView(options) {{ calls.push(options); }},
}};
const bar = {{
  getBoundingClientRect() {{ return {{left: 0, right: 100}}; }},
  querySelector(selector) {{ return selector === ".apptab.on" ? tab : null; }},
}};
{reveal}
revealActiveTab.lastView = activeView;
revealActiveTab(bar);
scheduled.forEach(callback => callback());
console.log(JSON.stringify({{scheduled: scheduled.length, calls}}));
""")
    assert result == {
        "scheduled": 1,
        "calls": [{"block": "nearest", "inline": "nearest"}],
    }


def test_active_tab_reveal_schedules_for_changed_view():
    reveal = extract_js_function(HTML, "revealActiveTab")
    result = run_node_json(f"""
let activeView = "term:prod";
const scheduled = [];
const calls = [];
function requestAnimationFrame(callback) {{ scheduled.push(callback); }}
const tab = {{
  getBoundingClientRect() {{ return {{left: 120, right: 180}}; }},
  scrollIntoView(options) {{ calls.push(options); }},
}};
const bar = {{
  getBoundingClientRect() {{ return {{left: 0, right: 100}}; }},
  querySelector(selector) {{ return selector === ".apptab.on" ? tab : null; }},
}};
{reveal}
revealActiveTab.lastView = "panel";
revealActiveTab(bar);
scheduled.forEach(callback => callback());
console.log(JSON.stringify({{scheduled: scheduled.length, calls}}));
""")
    assert result == {
        "scheduled": 1,
        "calls": [{"block": "nearest", "inline": "nearest"}],
    }


def test_app_viewport_updates_are_coalesced_per_animation_frame():
    functions = "\n\n".join(
        extract_js_function(HTML, name)
        for name in ("currentViewportHeight", "updateAppViewport", "scheduleAppViewport")
    )
    result = run_node_json(f"""
let appViewportFrame = 0;
let nextFrame = 1;
const frames = new Map();
const writes = [];
const style = {{
  setProperty(name, value) {{ writes.push([name, value]); }},
}};
const document = {{
  documentElement: {{clientHeight: 844, style}},
}};
const window = {{visualViewport: {{height: 420}}, innerHeight: 844}};
function requestAnimationFrame(callback) {{ const id = nextFrame++; frames.set(id, callback); return id; }}
{functions}
for(let i = 0; i < 50; i++) scheduleAppViewport();
const queuedBeforeFlush = frames.size;
[...frames.values()].forEach(callback => callback());
console.log(JSON.stringify({{queuedBeforeFlush, appViewportFrame, writes}}));
""")
    assert result == {
        "queuedBeforeFlush": 1,
        "appViewportFrame": 0,
        "writes": [["--app-height", "420px"]],
    }


def test_webterm_keyboard_resize_settles_once_without_delaying_input():
    coordinator = resize_coordinator_js()
    send_terminal_data = extract_js_function(TERM_HTML, "sendTerminalData")
    script = f"""
let fitFrame = 0;
let heightFitTimer = 0;
let columnReloadTimer = 0;
let fitDeferredForSelection = false;
let resizeDisposed = false;
let fitCalls = 0;
let nextFrame = 1;
let nextTimer = 1;
    let reloads = 0;
    let input = "";
    let ctrlArmed = false;
    let pasting = false;
const frames = new Map();
const timers = new Map();
const scheduledDelays = [];
const sent = [];
const inputAfterEach = [];
const pendingDelaysAtInput = [];
const queuedWorkByInput = [];
const term = {{
  cols: 95,
  rows: 34,
  hasSelection(){{ return false; }},
}};
const ws = {{readyState: 1, send(message){{ sent.push(message); }}}};
let proposed = {{cols: 95, rows: 24}};
const fit = {{
  proposeDimensions(){{ return {{...proposed}}; }},
  fit(){{
    fitCalls += 1;
    term.rows = proposed.rows;
    sendResize({{cols: term.cols, rows: term.rows}});
  }},
}};
const location = {{reload(){{ reloads += 1; }}}};
function requestAnimationFrame(cb){{ const id = nextFrame++; frames.set(id, cb); return id; }}
function cancelAnimationFrame(id){{ frames.delete(id); }}
function setTimeout(cb, delay){{
  const id = nextTimer++; timers.set(id, {{cb, delay}}); scheduledDelays.push(delay); return id;
}}
function clearTimeout(id){{ timers.delete(id); }}
const resizeObserver = {{disconnect(){{}}}};
function sendResize(size){{
  ws.send('1' + JSON.stringify({{columns: size.cols, rows: size.rows}}));
}}
function sendInput(data){{ input += data; }}
{send_terminal_data}
{coordinator}
for(let i = 0; i < 15; i++) {{
  proposed = {{cols: 95, rows: 33 - i}};
  scheduleFit();
  const callbacks = [...frames.values()]; frames.clear(); callbacks.forEach(cb => cb());
  pendingDelaysAtInput.push([...timers.values()].map(timer => timer.delay));
  const framesBeforeInput = frames.size;
  const timersBeforeInput = timers.size;
  sendTerminalData(String.fromCharCode(97 + i));
  inputAfterEach.push(input);
  queuedWorkByInput.push({{
    frames: frames.size - framesBeforeInput,
    timers: timers.size - timersBeforeInput,
  }});
}}
for(const timer of [...timers.values()]) timer.cb();
console.log(JSON.stringify({{
  fitCalls,
  reloads,
  resizeMessages: sent,
  input,
  inputAfterEach,
  pendingDelaysAtInput,
  queuedWorkByInput,
  scheduledDelays,
}}));
"""
    result = run_node_json(script)

    assert result["fitCalls"] == 1
    assert result["reloads"] == 0
    assert result["resizeMessages"] == ['1{"columns":95,"rows":19}']
    assert result["input"] == "abcdefghijklmno"
    assert result["inputAfterEach"] == [
        "abcdefghijklmno"[:index] for index in range(1, 16)
    ]
    assert result["pendingDelaysAtInput"] == [[120]] * 15
    assert result["queuedWorkByInput"] == [{"frames": 0, "timers": 0}] * 15
    assert result["scheduledDelays"].count(120) == 15


def test_webterm_pty_resize_deduplicates_dimensions_and_seeds_auth():
    send_resize = extract_js_function(TERM_HTML, "sendResize")
    script = f"""
let lastResize = "";
const sent = [];
const term = {{cols: 95, rows: 24}};
const ws = {{readyState: 1, send(msg){{ sent.push(msg); }}}};
{send_resize}
for(let i = 0; i < 100; i++) sendResize({{cols:95, rows:24}});
const afterSame = sent.length;
sendResize({{cols:96, rows:24}});
term.cols = 96;
sendResize();
console.log(JSON.stringify({{afterSame, sent, lastResize}}));
"""
    result = run_node_json(script)

    assert result["afterSame"] == 1
    assert result["sent"] == [
        '1{"columns":95,"rows":24}',
        '1{"columns":96,"rows":24}',
    ]
    assert result["lastResize"] == "96x24"
    assert 'lastResize = `${term.cols}x${term.rows}`' in TERM_HTML
    assert "term.onResize(sendResize)" in TERM_HTML
    assert "sendResize();" not in TERM_HTML


def test_webterm_pty_resize_geometry_wiring_auth_and_preferences_integrate():
    geometry_js = TERM_HTML.split("(function () {", 1)[1].split(
        "  const TOOLBAR_KEYS", 1
    )[0]
    prelude = r"""
let fitCalls = 0;
let nextFrame = 1;
const frameQueue = new Map();
const timerQueue = [];
const windowListeners = {};
const elements = new Map();
for (const id of ['dbg', 'err', 'term', 'term-shell', 'term-toolbar']) {
  elements.set(id, {
    classList: {add(){}}, style: {}, textContent: '',
  });
}
globalThis.location = {
  search: '', protocol: 'https:', pathname: '/term/', host: 'host.test',
};
globalThis.navigator = {maxTouchPoints: 1, vibrate(){}};
globalThis.window = {
  ontouchstart: null,
  addEventListener(type, cb){ windowListeners[type] = cb; },
  open(){ throw new Error('link opener should not run in geometry test'); },
};
globalThis.document = {
  title: '', documentElement: {style:{setProperty(){}}}, body: {appendChild(){}, style:{}},
  getElementById(id){ return elements.get(id); },
  createElement(){ return {}; },
};
globalThis.requestAnimationFrame = cb => {
  const id = nextFrame++; frameQueue.set(id, cb); return id;
};
globalThis.setTimeout = cb => { timerQueue.push(cb); return timerQueue.length; };
function sendTerminalData() {}
globalThis.Terminal = class {
  constructor(options){
    this.options = {...options}; this.cols = 80; this.rows = 20; this.selected = false;
  }
  loadAddon(addon){ addon.term = this; }
  open(){}
  write(){}
  hasSelection(){ return this.selected; }
  onData(cb){ this.dataHandler = cb; }
  onResize(cb){ this.resizeHandler = cb; }
  onSelectionChange(cb){ this.selectionHandler = cb; }
};
class FakeFit {
  proposeDimensions(){
    return {cols:this.term.cols, rows:this.term.rows};
  }
  fit(){
    fitCalls += 1;
    this.term.cols = this.term.cols === 80 ? 95 : this.term.cols;
    this.term.rows = this.term.rows === 20 ? 24 : this.term.rows;
    if(this.term.resizeHandler){
      this.term.resizeHandler({cols:this.term.cols, rows:this.term.rows});
    }
  }
}
globalThis.FitAddon = {FitAddon: FakeFit};
globalThis.WebLinksAddon = {WebLinksAddon: class {}};
globalThis.CanvasAddon = {CanvasAddon: class {}};
globalThis.LigaturesWebAddon = class {};
globalThis.WebSocket = class {
  constructor(){
    this.readyState = 0; this.protocol = 'tty'; this.listeners = {}; this.sent = [];
    globalThis.socket = this;
  }
  addEventListener(type, cb){ this.listeners[type] = cb; }
  send(message){ this.sent.push(message); }
};
"""
    simulation = r"""
const fitCallsAfterInitial = fitCalls;
if(typeof windowListeners.resize !== 'function') throw new Error('resize not wired');
if(typeof term.resizeHandler !== 'function') throw new Error('terminal resize not wired');

socket.readyState = 1;
socket.listeners.open();
const auth = JSON.parse(socket.sent[0]);
for(let i = 0; i < 100; i++) windowListeners.resize();
for(let i = 0; i < 100; i++) term.resizeHandler({cols:95, rows:24});
const resizeAfterSeededSame = socket.sent.filter(msg => msg.startsWith('1')).length;

term.cols = 96;
term.resizeHandler({cols:96, rows:24});
socket.listeners.message({data:'2' + JSON.stringify({fontFamily:'Test Mono', fontSize:17})});
for(const cb of timerQueue.splice(0)) cb();
const queuedBeforeFrame = frameQueue.size;
for(const cb of [...frameQueue.values()]) cb();
frameQueue.clear();

const prefsBytes = new TextEncoder().encode(JSON.stringify({fontSize:18}));
const packet = new Uint8Array(prefsBytes.length + 1);
packet[0] = '2'.charCodeAt(0);
packet.set(prefsBytes, 1);
const beforeBinaryPreference = socket.sent.length;
socket.listeners.message({data:packet.buffer});
const queuedAfterBinaryPreference = frameQueue.size;
for(const cb of [...frameQueue.values()]) cb();
frameQueue.clear();

console.log(JSON.stringify({
  fitCallsAfterInitial,
  fitCalls,
  auth,
  resizeAfterSeededSame,
  resizeMessages: socket.sent.filter(msg => msg.startsWith('1')),
  queuedBeforeFrame,
  queuedAfterBinaryPreference,
  beforeBinaryPreference,
  afterBinaryPreference: socket.sent.length,
  fontFamily: term.options.fontFamily,
  fontSize: term.options.fontSize,
}));
"""
    result = run_node_json(prelude + geometry_js + simulation)

    assert result["fitCallsAfterInitial"] == 1
    assert result["auth"]["columns"] == 95
    assert result["auth"]["rows"] == 24
    assert result["resizeAfterSeededSame"] == 0
    assert result["resizeMessages"] == ['1{"columns":96,"rows":24}']
    assert result["queuedBeforeFrame"] == 1
    assert result["queuedAfterBinaryPreference"] == 1
    assert result["beforeBinaryPreference"] == result["afterBinaryPreference"]
    assert result["fitCalls"] == 1
    assert result["fontFamily"] == "Test Mono"
    assert result["fontSize"] == 18


def test_remote_column_resize_reloads_once_after_geometry_stabilizes():
    coordinator = resize_coordinator_js()
    script = f"""
let fitFrame = 0;
let heightFitTimer = 0;
let columnReloadTimer = 0;
let fitDeferredForSelection = false;
let resizeDisposed = false;
let nextFrame = 1;
let nextTimer = 1;
let fitCalls = 0;
let reloads = 0;
const frames = new Map();
const timers = new Map();
const scheduledDelays = [];
const term = {{cols:42, rows:35, hasSelection(){{ return false; }}}};
let width = 390;
const sizes = {{390: {{cols:46, rows:35}}, 430: {{cols:51, rows:35}}}};
const fit = {{
  proposeDimensions(){{ return {{...sizes[width]}}; }},
  fit(){{ fitCalls += 1; }},
}};
const location = {{reload(){{ reloads += 1; }}}};
const requestAnimationFrame = cb => {{
  const id = nextFrame++; frames.set(id, cb); return id;
}};
const cancelAnimationFrame = id => frames.delete(id);
const setTimeout = (cb, delay) => {{
  const id = nextTimer++; timers.set(id, {{cb, delay}}); scheduledDelays.push(delay); return id;
}};
const clearTimeout = id => timers.delete(id);
const resizeObserver = {{disconnect(){{}}}};
const flushFrames = () => {{
  const callbacks = [...frames.values()]; frames.clear(); callbacks.forEach(cb => cb());
}};
const flushTimers = () => {{
  const callbacks = [...timers.values()]; timers.clear(); callbacks.forEach(timer => timer.cb());
}};
{coordinator}

scheduleFit();
flushFrames();
const firstWidth = {{fitCalls, delay:[...timers.values()][0]?.delay, timers:timers.size}};
flushTimers();
term.cols = 46;

width = 430;
scheduleFit();
flushFrames();
flushTimers();
term.cols = 51;

width = 390;
scheduleFit();
flushFrames();
const returnedWidth = {{fitCalls, delay:[...timers.values()][0]?.delay, timers:timers.size}};
flushTimers();
console.log(JSON.stringify({{
  firstWidth,
  returnedWidth,
  reloads,
  fitCalls,
  scheduledDelays,
}}));
"""
    result = run_node_json(script)

    assert result["firstWidth"] == {
        "fitCalls": 0,
        "delay": 180,
        "timers": 1,
    }
    assert result["returnedWidth"] == {
        "fitCalls": 0,
        "delay": 180,
        "timers": 1,
    }
    assert result["reloads"] == 3
    assert result["fitCalls"] == 0
    assert result["scheduledDelays"] == [180, 180, 180]


def test_remote_column_resize_waits_for_active_selection_to_clear():
    assert "function resumeDeferredFitAfterSelection()" in TERM_HTML
    assert "term.onSelectionChange(resumeDeferredFitAfterSelection)" in TERM_HTML
    coordinator = resize_coordinator_js()
    resume_fit = extract_js_function(TERM_HTML, "resumeDeferredFitAfterSelection")
    script = f"""
let fitFrame = 0;
let heightFitTimer = 0;
let columnReloadTimer = 0;
let fitDeferredForSelection = false;
let resizeDisposed = false;
let nextFrame = 1;
let nextTimer = 1;
let fitCalls = 0;
let reloads = 0;
const reloadedCols = [];
let selected = true;
const frames = new Map();
const timers = new Map();
const scheduledDelays = [];
const term = {{
  cols:46,
  rows:35,
  hasSelection(){{ return selected; }},
}};
let proposed = {{cols:51, rows:35}};
const ws = {{readyState:1}};
const fit = {{
  proposeDimensions(){{ return {{...proposed}}; }},
  fit(){{
    fitCalls += 1;
  }},
}};
const location = {{reload(){{ reloads += 1; reloadedCols.push(proposed.cols); }}}};
const requestAnimationFrame = cb => {{
  const id = nextFrame++; frames.set(id, cb); return id;
}};
const cancelAnimationFrame = id => frames.delete(id);
const setTimeout = (cb, delay) => {{
  const id = nextTimer++; timers.set(id, {{cb, delay}}); scheduledDelays.push(delay); return id;
}};
const clearTimeout = id => timers.delete(id);
const resizeObserver = {{disconnect(){{}}}};
const flushFrames = () => {{
  const callbacks = [...frames.values()]; frames.clear(); callbacks.forEach(cb => cb());
}};
const flushTimers = () => {{
  const callbacks = [...timers.values()]; timers.clear(); callbacks.forEach(timer => timer.cb());
}};
{coordinator}
{resume_fit}

scheduleFit();
flushFrames();
const whileSelected = {{
  timers:timers.size,
  reloads,
  deferred:fitDeferredForSelection,
  fitCalls,
}};

selected = false;
for(let i = 0; i < 20; i++) resumeDeferredFitAfterSelection();
const framesAfterClear = frames.size;
flushFrames();
const timerScheduledBeforeSelection = {{
  timers:timers.size,
  delay:[...timers.values()][0]?.delay,
  deferred:fitDeferredForSelection,
  fitCalls,
}};
selected = true;
flushTimers();
const selectionActivatedBeforeExpiry = {{
  timers:timers.size,
  reloads,
  deferred:fitDeferredForSelection,
}};

proposed = {{cols:52, rows:35}};
scheduleFit();
flushFrames();
proposed = {{cols:53, rows:35}};
scheduleFit();
flushFrames();
const latestGeometryWhileSelected = {{
  timers:timers.size,
  reloads,
  deferred:fitDeferredForSelection,
}};

selected = false;
resumeDeferredFitAfterSelection();
flushFrames();
const latestPending = {{
  timers:timers.size,
  delay:[...timers.values()][0]?.delay,
  deferred:fitDeferredForSelection,
}};
flushTimers();

console.log(JSON.stringify({{
  whileSelected,
  framesAfterClear,
  timerScheduledBeforeSelection,
  selectionActivatedBeforeExpiry,
  latestGeometryWhileSelected,
  latestPending,
  reloads,
  reloadedCols,
  fitCalls,
  scheduledDelays,
}}));
"""
    result = run_node_json(script)

    assert result["whileSelected"] == {
        "timers": 0,
        "reloads": 0,
        "deferred": True,
        "fitCalls": 0,
    }
    assert result["framesAfterClear"] == 1
    assert result["timerScheduledBeforeSelection"] == {
        "timers": 1,
        "delay": 180,
        "deferred": False,
        "fitCalls": 0,
    }
    assert result["selectionActivatedBeforeExpiry"] == {
        "timers": 0,
        "reloads": 0,
        "deferred": True,
    }
    assert result["latestGeometryWhileSelected"] == {
        "timers": 0,
        "reloads": 0,
        "deferred": True,
    }
    assert result["latestPending"] == {
        "timers": 1,
        "delay": 180,
        "deferred": False,
    }
    assert result["reloads"] == 1
    assert result["reloadedCols"] == [53]
    assert result["fitCalls"] == 0
    assert result["scheduledDelays"] == [180, 180]


def test_remote_resize_cleanup_cancels_frame_and_settle_timers():
    coordinator = resize_coordinator_js()
    lifecycle_wiring = resize_lifecycle_wiring_js()
    assert "resizeObserver?.observe(document.getElementById('term'))" in TERM_HTML
    script = f"""
function runLifecycleScenario(signal, pending) {{
  let fitFrame = 0;
  let heightFitTimer = 0;
  let columnReloadTimer = 0;
  let fitDeferredForSelection = false;
  let resizeDisposed = false;
  let nextFrame = 1;
  let nextTimer = 1;
  let disconnected = 0;
  const frames = new Map();
  const timers = new Map();
  const eventTarget = () => {{
    const listeners = {{}};
    return {{
      addEventListener(type, callback) {{
        if (!listeners[type]) listeners[type] = [];
        listeners[type].push(callback);
      }},
      dispatchEvent(event) {{
        for (const callback of listeners[event.type] || []) callback(event);
      }},
    }};
  }};
  const window = eventTarget();
  const ws = eventTarget();
  const term = {{cols:95, rows:24, hasSelection(){{ return false; }}}};
  let proposed = {{cols:95, rows:24}};
  const fit = {{proposeDimensions(){{ return {{...proposed}}; }}, fit(){{}}}};
  const location = {{reload(){{}}}};
  const requestAnimationFrame = callback => {{
    const id = nextFrame++; frames.set(id, callback); return id;
  }};
  const cancelAnimationFrame = id => frames.delete(id);
  const setTimeout = (callback, delay) => {{
    const id = nextTimer++; timers.set(id, {{callback, delay}}); return id;
  }};
  const clearTimeout = id => timers.delete(id);
  const resizeObserver = {{disconnect(){{ disconnected += 1; }}}};
  const flushFrames = () => {{
    const callbacks = [...frames.values()]; frames.clear(); callbacks.forEach(callback => callback());
  }};
  {coordinator}
  {lifecycle_wiring}

  if (pending === 'height') {{
    proposed = {{cols:95, rows:23}};
    scheduleFit();
    flushFrames();
    scheduleFit();
  }} else if (pending === 'column') {{
    proposed = {{cols:96, rows:24}};
    scheduleFit();
    flushFrames();
    scheduleFit();
  }} else {{
    scheduleFit();
  }}
  const before = {{
    frames: frames.size,
    delays: [...timers.values()].map(timer => timer.delay),
  }};
  const target = signal === 'pagehide' ? window : ws;
  target.dispatchEvent({{type: signal}});
  return {{
    before,
    after: {{frames: frames.size, timers: timers.size}},
    disposed: resizeDisposed,
    disconnected,
  }};
}}

const results = {{}};
for (const signal of ['pagehide', 'close']) {{
  results[signal] = {{}};
  for (const pending of ['frame', 'height', 'column']) {{
    results[signal][pending] = runLifecycleScenario(signal, pending);
  }}
}}
console.log(JSON.stringify(results));
"""
    result = run_node_json(script)

    for signal in ("pagehide", "close"):
        assert result[signal]["frame"]["before"] == {"frames": 1, "delays": []}
        assert result[signal]["height"]["before"] == {"frames": 1, "delays": [120]}
        assert result[signal]["column"]["before"] == {"frames": 1, "delays": [180]}
        for pending in ("frame", "height", "column"):
            assert result[signal][pending]["after"] == {"frames": 0, "timers": 0}
            assert result[signal][pending]["disposed"] is True
            assert result[signal][pending]["disconnected"] == 1


def test_webterm_data_handler_sends_input_without_resize_work():
    send_terminal_data = extract_js_function(TERM_HTML, "sendTerminalData")
    result = run_node_json(f"""
const sent = [];
let ctrlArmed = false;
let pasting = false;
function sendInput(data) {{ sent.push(data); }}
{send_terminal_data}
sendTerminalData('a');
console.log(JSON.stringify({{sent}}));
""")

    assert result == {"sent": ["a"]}
    assert "term.onData(sendTerminalData)" in TERM_HTML


def test_remote_drawer_controls_are_present():
    assert 'id="btn-remote"' in HTML
    assert 'id="remote"' in HTML
    assert 'id="remote-status"' in HTML
    assert 'id="remote-qr"' in HTML
    assert 'id="remote-dashboard-url"' in HTML
    assert 'id="remote-term-url"' in HTML
    assert 'id="remote-on"' in HTML
    assert 'id="remote-off"' in HTML
    assert 'id="remote-webterm-on"' in HTML
    assert 'id="remote-webterm-off"' in HTML
    assert 'id="remote-open-terminal"' in HTML


def test_remote_ui_calls_backend_endpoints():
    for endpoint in (
        "/remote-state",
        "/remote-on",
        "/remote-off",
        "/remote-webterm-on",
        "/remote-webterm-off",
        "/remote-qr.png",
    ):
        assert endpoint in HTML


def test_remote_polling_slows_down_when_remote_webterm_is_enabled():
    assert "remotePollSeconds()" in HTML
    assert "document.hidden" in HTML


def test_remote_terminal_can_be_opened_from_remote_drawer():
    assert "ensureRemoteTerminalVisible" in HTML
    assert 'remoteAction("/remote-webterm-on"' in HTML
    assert 'ensureRemoteTerminalVisible();' in HTML


def test_open_terminal_button_dismisses_remote_drawer():
    fn = extract_js_function(HTML, "ensureRemoteTerminalVisible")
    assert '$("#remote").classList.remove("open")' in fn


def test_remote_terminal_iframe_does_not_hijack_tmux_wheel_events():
    fn = extract_js_function(HTML, "wireTermFrameScroll")
    assert "scrollLines" not in fn
    assert 'addEventListener("wheel"' not in fn
    assert "WheelEvent" in fn
    assert "elementFromPoint" in fn
    assert "clientX" in fn
    assert "clientY" in fn
    assert "touchstart" in fn
    assert "touchmove" in fn
    assert "frame.dataset.selecting" not in fn
    assert "passive:false" in fn
    style_fn = extract_js_function(HTML, "styleTermFrame")
    assert "touch-action:pan-y" in style_fn
    assert "-webkit-overflow-scrolling:touch" in style_fn
    assert "wireTermFrameScroll(frame)" in HTML


def test_remote_touch_scroll_is_throttled_for_tmux_wheel_ticks():
    fn = extract_js_function(HTML, "wireTermFrameScroll")
    script = f"""
{fn}
const listeners = {{}};
let wheels = [];
const target = {{
  dispatchEvent(ev){{
    if(ev.type === "wheel") wheels.push({{deltaY: ev.deltaY, clientY: ev.clientY}});
    return true;
  }}
}};
class FakeWheelEvent {{
  constructor(type, init){{ this.type = type; Object.assign(this, init); }}
}}
const doc = {{
  body: target,
  addEventListener(type, cb){{ listeners[type] = cb; }},
  elementFromPoint(){{ return target; }},
  querySelector(){{ return target; }}
}};
const frame = {{ dataset: {{}}, contentWindow: {{ WheelEvent: FakeWheelEvent }}, contentDocument: doc }};
wireTermFrameScroll(frame);
const ev = y => ({{
  touches: [{{clientX: 10, clientY: y, screenX: 10, screenY: y}}],
  target,
  preventDefault(){{ this.prevented = true; }},
  stopPropagation(){{ this.stopped = true; }}
}});
listeners.touchstart(ev(100));
listeners.touchmove(ev(80));
const afterSmallMove = wheels.length;
listeners.touchmove(ev(60));
const afterSecondSmallMove = wheels.length;
listeners.touchmove(ev(40));
const afterThreshold = wheels.length;
listeners.touchmove(ev(-90));
console.log(JSON.stringify({{
  afterSmallMove,
  afterSecondSmallMove,
  afterThreshold,
  finalCount: wheels.length,
  deltas: wheels.map(w => w.deltaY)
}}));
"""
    result = run_node_json(script)

    assert result["afterSmallMove"] == 0
    assert result["afterSecondSmallMove"] == 0
    assert result["afterThreshold"] == 1
    assert result["finalCount"] <= 4


def test_terminal_touch_controller_ignores_toolbar_and_dialog_boundaries():
    result = run_touch_controller(
        r"""
function uiTarget(id) {
  return {
    id,
    matches(selector) { return selector.includes(`#${id}`); },
    closest(selector) { return this.matches(selector) ? this : null; },
  };
}
const results = {};
for (const id of ["term-toolbar", "paste-dialog"]) {
  const target = uiTarget(id);
  const start = point(40, 10);
  const startEvent = event([start]);
  startEvent.target = target;
  startEvent.composedPath = () => [target, document.body];
  listeners.touchstart(startEvent);
  const move = point(20, 10);
  const moveEvent = event([move]);
  moveEvent.target = target;
  moveEvent.composedPath = () => [target, document.body];
  listeners.touchmove(moveEvent);
  results[id] = {
    phase: gesture.phase,
    prevented: startEvent.prevented || moveEvent.prevented,
    stopped: startEvent.stopped || moveEvent.stopped,
    timers: timers.size,
    frames: frames.size,
    sent: [...sent],
    wheels: [...wheels],
  };
  listeners.touchend(event([], [move]));
}
const mouseTarget = uiTarget("term-toolbar");
const down = event([]); down.button = 0; down.clientX = 400; down.clientY = 200;
down.target = mouseTarget; down.composedPath = () => [mouseTarget, document.body];
listeners.mousedown(down);
console.log(JSON.stringify({results, paneMouseDrag, mousePrevented:down.prevented}));
""",
        cells=valid_vertical_cells(),
    )

    for boundary in ("term-toolbar", "paste-dialog"):
        assert result["results"][boundary] == {
            "phase": "idle", "prevented": False, "stopped": False,
            "timers": 0, "frames": 0, "sent": [], "wheels": [],
        }
    assert result["paneMouseDrag"] is None
    assert result["mousePrevented"] is False


def test_chrome_touch_drag_scrolls_toolbar_without_terminal_prevent_default():
    result = run_chrome_toolbar_touch_drag()

    assert result["left"] >= 0
    assert result["right"] <= result["viewportWidth"]
    assert result["clientWidth"] <= result["viewportWidth"]
    assert result["documentScrollX"] == 0
    assert result["scrollWidth"] > result["clientWidth"]
    assert result["scrollLeft"] > 100, json.dumps(result, sort_keys=True)
    assert result["prevented"]
    assert not any(result["prevented"])
    assert result["preventCalls"] == []
    assert result["activeTag"] == "TEXTAREA"
    assert result["dialogFocus"] == {
        "manual": {"activeTag": "TEXTAREA", "open": False},
        "cancel": {"activeTag": "TEXTAREA", "open": False},
        "escape": {"activeTag": "TEXTAREA", "open": False},
    }


def test_touch_move_scrolls_when_long_press_does_not_engage():
    def scroll_after(run_timer_first, *, mouse_on=True):
        simulation = f"""
const start = point(20, 10);
listeners.touchstart(event([start]));
if ({str(run_timer_first).lower()}) runTimers();
const moves = [
  point(20, 10, 0, -13),
  point(20, 10, 0, -40),
  point(20, 10, 0, -70),
];
for (const move of moves) listeners.touchmove(event([move]));
listeners.touchend(event([], [moves.at(-1)]));
console.log(JSON.stringify({{
  wheels,
  sent,
  timers: timers.size,
  frames: frames.size,
  indicator: indicator && indicator.style.display,
}}));
"""
        return run_touch_controller(simulation, mouse_on=mouse_on)

    moved_before_long_press = scroll_after(False)
    held_without_a_border = scroll_after(True)
    mouse_tracking_off = scroll_after(False, mouse_on=False)

    for result in (moved_before_long_press, held_without_a_border, mouse_tracking_off):
        assert result["wheels"] == [{"deltaY": 56, "clientY": 120}]
        assert result["sent"] == []
        assert result["timers"] == 0
        assert result["frames"] == 0
        assert result["indicator"] == "none"


def test_touch_resize_survives_transverse_jitter():
    result = run_touch_controller(
        r"""
const start = point(40, 10);
listeners.touchstart(event([start]));
runTimers();
const afterPress = sent.length;
for (let i = 0; i < 100; i++) {
  listeners.touchmove(event([point(43, 10, 0, i % 2 ? 3 : -3)]));
}
const beforeFrame = sent.length;
const queuedFrames = frames.size;
flushFrames();
const afterFrame = sent.length;
const finalTouch = point(44, 10, 0, 2);
listeners.touchend(event([], [finalTouch]));
console.log(JSON.stringify({
  sent,
  afterPress,
  beforeFrame,
  queuedFrames,
  afterFrame,
  remainingFrames: frames.size,
  indicator: indicator.style.display,
}));
""",
        cells=valid_vertical_cells(),
    )

    assert result["afterPress"] == 1
    assert result["beforeFrame"] == 1
    assert result["queuedFrames"] == 1
    assert result["afterFrame"] == 2
    assert result["sent"] == [
        "\x1b[<0;40;10M",
        "\x1b[<32;43;10M",
        "\x1b[<32;44;10M",
        "\x1b[<0;44;10m",
    ]
    assert result["remainingFrames"] == 0
    assert result["indicator"] == "none"


def test_pane_resize_repaints_reflowed_text_after_tmux_redraw():
    assert "let paneRefreshPending = false" in TERM_HTML
    assert "term.onWriteParsed(refreshPaneAfterWrite)" in TERM_HTML
    touch = run_touch_controller(
        r"""
const start = point(40, 10);
listeners.touchstart(event([start]));
runTimers();
const finalTouch = point(45, 10);
listeners.touchmove(event([finalTouch]));
flushFrames();
listeners.touchend(event([], [finalTouch]));
const touchBeforeWrite = {pending: paneRefreshPending, refreshCalls: [...refreshCalls]};
term.writeParsedHandler();
const afterIntermediateWrite = {
  pending: paneRefreshPending,
  refreshCalls: [...refreshCalls],
  timers: timers.size,
};
term.writeParsedHandler();
const afterFinalWrite = {
  pending: paneRefreshPending,
  refreshCalls: [...refreshCalls],
  timers: timers.size,
  delay: scheduledDelays.at(-1),
};
runTimers();
console.log(JSON.stringify({
  touchBeforeWrite,
  afterIntermediateWrite,
  afterFinalWrite,
  touchAfterWrite: {pending: paneRefreshPending, refreshCalls},
}));
""",
        cells=valid_vertical_cells(),
    )

    assert touch["touchBeforeWrite"] == {"pending": True, "refreshCalls": []}
    assert touch["afterIntermediateWrite"] == {
        "pending": True,
        "refreshCalls": [],
        "timers": 1,
    }
    assert touch["afterFinalWrite"] == {
        "pending": True,
        "refreshCalls": [],
        "timers": 1,
        "delay": 180,
    }
    assert touch["touchAfterWrite"] == {
        "pending": False,
        "refreshCalls": [[0, 23]],
    }

    mouse = run_touch_controller(
        r"""
const down = event([]); down.button = 0; down.clientX = 400; down.clientY = 200;
listeners.mousedown(down);
const move = event([]); move.button = 0; move.clientX = 450; move.clientY = 200;
listeners.mousemove(move);
const up = event([]); up.button = 0; up.clientX = 450; up.clientY = 200;
listeners.mouseup(up);
const mouseBeforeWrite = {pending: paneRefreshPending, refreshCalls: [...refreshCalls]};
term.writeParsedHandler();
runTimers();
console.log(JSON.stringify({
  mouseBeforeWrite,
  mouseAfterWrite: {pending: paneRefreshPending, refreshCalls},
}));
""",
        cells=valid_vertical_cells(),
    )

    assert mouse["mouseBeforeWrite"] == {"pending": True, "refreshCalls": []}
    assert mouse["mouseAfterWrite"] == {
        "pending": False,
        "refreshCalls": [[0, 23]],
    }

    non_border = run_touch_controller(
        r"""
const down = event([]); down.button = 0; down.clientX = 100; down.clientY = 200;
listeners.mousedown(down);
const move = event([]); move.button = 0; move.clientX = 180; move.clientY = 200;
listeners.mousemove(move);
const up = event([]); up.button = 0; up.clientX = 180; up.clientY = 200;
listeners.mouseup(up);
console.log(JSON.stringify({pending: paneRefreshPending, timers: timers.size}));
""",
        cells={},
    )

    assert non_border == {"pending": False, "timers": 0}


def test_touch_accepts_non_iterable_touchlist():
    result = run_touch_controller(
        r"""
const start = point(40, 10);
listeners.touchstart(event(touchList(start)));
runTimers();
const moving = point(42, 10);
listeners.touchmove(event(touchList(moving)));
flushFrames();
const finalTouch = point(43, 10);
listeners.touchend(event(touchList(), touchList(finalTouch)));
console.log(JSON.stringify({sent, frames: frames.size, indicator: indicator.style.display}));
""",
        cells=valid_vertical_cells(),
    )

    assert result["sent"] == [
        "\x1b[<0;40;10M",
        "\x1b[<32;42;10M",
        "\x1b[<32;43;10M",
        "\x1b[<0;43;10m",
    ]
    assert result["frames"] == 0
    assert result["indicator"] == "none"


def test_touch_pending_jitter_blocks_native_pan_without_emitting():
    result = run_touch_controller(
        r"""
const start = point(40, 10);
listeners.touchstart(event([start]));
const jitterEvent = event([point(40, 10, 3, 2)]);
listeners.touchmove(jitterEvent);
const beforeTimer = {
  prevented: jitterEvent.prevented,
  stopped: jitterEvent.stopped,
  wheels: [...wheels],
  sent: [...sent],
  timers: timers.size,
};
runTimers();
console.log(JSON.stringify({
  beforeTimer,
  afterTimer: [...sent],
  indicator: indicator.style.display,
}));
""",
        cells=valid_vertical_cells(),
    )

    assert result["beforeTimer"] == {
        "prevented": True,
        "stopped": True,
        "wheels": [],
        "sent": [],
        "timers": 1,
    }
    assert result["afterTimer"] == ["\x1b[<0;40;10M"]
    assert result["indicator"] == "block"


def test_touch_horizontal_resize_locks_vertical_axis():
    result = run_touch_controller(
        r"""
const start = point(20, 12);
listeners.touchstart(event([start]));
runTimers();
for (let i = 0; i < 20; i++) {
  listeners.touchmove(event([point(20, 14, i % 2 ? 3 : -3, 0)]));
}
flushFrames();
const finalTouch = point(20, 15, 2, 0);
listeners.touchend(event([], [finalTouch]));
console.log(JSON.stringify({sent, wheels, frames: frames.size}));
""",
        cells=valid_horizontal_cells(),
    )

    assert result["sent"] == [
        "\x1b[<0;20;12M",
        "\x1b[<32;20;14M",
        "\x1b[<32;20;15M",
        "\x1b[<0;20;15m",
    ]
    assert result["wheels"] == []
    assert result["frames"] == 0


def test_touch_border_lookup_uses_viewport_y():
    viewport_y = 50
    result = run_touch_controller(
        r"""
const start = point(20, 3);
listeners.touchstart(event([start]));
runTimers();
console.log(JSON.stringify({sent, lineReads, indicator: indicator.style.display}));
""",
        cells=valid_horizontal_cells(viewport_y, col=20, row=3),
        viewport_y=viewport_y,
    )

    assert 52 in result["lineReads"]
    assert result["sent"] == ["\x1b[<0;20;3M"]
    assert result["indicator"] == "block"


def test_isolated_border_glyph_does_not_engage():
    pipe = run_touch_controller(
        r"""
listeners.touchstart(event([point(20, 10)]));
runTimers();
console.log(JSON.stringify({sent, indicator: indicator.style.display}));
""",
        cells={9: {19: "|"}},
    )
    dash = run_touch_controller(
        r"""
listeners.touchstart(event([point(20, 10)]));
runTimers();
console.log(JSON.stringify({sent, indicator: indicator.style.display}));
""",
        cells={9: {19: "-"}},
    )

    for result in (pipe, dash):
        assert result["sent"] == []
        assert result["indicator"] == "none"


def run_touch_cancellation(kind):
    action = {
        "second-finger": "listeners.touchstart(event([moving, point(50, 10, 0, 0, 8)]));",
        "touchcancel": "listeners.touchcancel(event([], [moving]));",
        "pagehide": "if (windowListeners.pagehide) windowListeners.pagehide();",
        "ws-close": "ws.readyState = 3; for (const cb of wsListeners.close || []) cb({});",
    }[kind]
    return run_touch_controller(
        f"""
const start = point(40, 10);
listeners.touchstart(event([start]));
runTimers();
const moving = point(42, 10);
listeners.touchmove(event([moving]));
{action}
console.log(JSON.stringify({{
  sent,
  timers: timers.size,
  frames: frames.size,
  indicator: indicator.style.display,
  pagehideWired: typeof windowListeners.pagehide === 'function',
  closeWired: (wsListeners.close || []).length,
}}));
""",
        cells=valid_vertical_cells(),
    )


def test_second_finger_and_touchcancel_release_and_reset():
    for kind in ("second-finger", "touchcancel"):
        result = run_touch_cancellation(kind)
        assert result["sent"] == [
            "\x1b[<0;40;10M",
            "\x1b[<32;42;10M",
            "\x1b[<0;42;10m",
        ]
        assert result["timers"] == 0
        assert result["frames"] == 0
        assert result["indicator"] == "none"


def test_touch_pagehide_and_websocket_close_centralize_cleanup():
    pagehide = run_touch_cancellation("pagehide")
    assert pagehide["pagehideWired"] is True
    assert pagehide["sent"] == [
        "\x1b[<0;40;10M",
        "\x1b[<32;42;10M",
        "\x1b[<0;42;10m",
    ]
    assert pagehide["frames"] == 0
    assert pagehide["indicator"] == "none"

    disconnected = run_touch_cancellation("ws-close")
    assert disconnected["closeWired"] == 1
    assert disconnected["sent"] == ["\x1b[<0;40;10M"]
    assert disconnected["timers"] == 0
    assert disconnected["frames"] == 0
    assert disconnected["indicator"] == "none"


def test_touch_selection_mode_is_not_captured():
    result = run_touch_controller(
        r"""
const startEvent = event([point(40, 10)]);
listeners.touchstart(startEvent);
runTimers();
const moveEvent = event([point(44, 10)]);
listeners.touchmove(moveEvent);
console.log(JSON.stringify({
  sent,
  wheels,
  prevented: startEvent.prevented || moveEvent.prevented,
  timers: timers.size,
  frames: frames.size,
}));
""",
        cells=valid_vertical_cells(),
        selecting=True,
    )

    assert result == {
        "sent": [],
        "wheels": [],
        "prevented": False,
        "timers": 0,
        "frames": 0,
    }


def test_touch_closed_socket_never_engages_resize():
    result = run_touch_controller(
        r"""
listeners.touchstart(event([point(40, 10)]));
runTimers();
const afterLongPress = indicator.style.display;
listeners.touchend(event([], [point(42, 10)]));
console.log(JSON.stringify({sent, afterLongPress, indicator: indicator.style.display}));
""",
        cells=valid_vertical_cells(),
        socket_open=False,
    )

    assert result["sent"] == []
    assert result["afterLongPress"] == "none"
    assert result["indicator"] == "none"


def test_parent_scroll_wiring_skips_owned_terminal_gestures():
    fn = extract_js_function(HTML, "wireTermFrameScroll")
    script = f"""
{fn}
const added = [];
const doc = {{addEventListener(type) {{ added.push(type); }}}};
const win = {{__comandosOwnsTouchGestures: true, document: doc}};
const frame = {{dataset: {{}}, contentWindow: win, contentDocument: doc}};
wireTermFrameScroll(frame);
console.log(JSON.stringify({{added, wired: !!win.__comandosScrollWired}}));
"""
    result = run_node_json(script)

    assert result == {"added": [], "wired": False}
    assert "window.__comandosOwnsTouchGestures = true" in TERM_HTML


def test_remote_terminal_selection_uses_backend_state_not_local_guess():
    assert 'id="term-select-toggle"' not in HTML
    assert "frame.dataset.selecting" not in HTML
    assert "/tmux-mouse" in HTML
    assert "const termInteraction = new Map()" in HTML
    assert "syncTermInteraction" in HTML
    assert "setTermSelectionMode" in HTML
    assert "restoreTermInteraction" in HTML
    assert "postTermState" in HTML
    assert "handleTermFrameMessage" in HTML
    fn = extract_js_function(HTML, "setTermSelectionMode")
    assert 'api("/tmux-mouse"' in fn
    assert "enabled: !selecting" in fn
    assert "termInteractionState(sess)" in fn
    assert "state.temporary = selecting" in fn
    assert "applyTermInteraction(sess)" in fn
    show = extract_js_function(HTML, "showView")
    assert "syncTermInteraction(shown)" in show
    assert "restoreTermInteraction(sess)" in extract_js_function(HTML, "closeTerm")


def test_remote_terminal_selection_sync_click_restore_and_failure_integrate():
    result = run_term_interaction("""
const posts = [];
let failNext = false;
api = async function(path, body) {
  if(!body) return {mouse: "off"};
  posts.push(body);
  if(failNext){ failNext = false; throw new Error("post failed"); }
  return {mouse: body.enabled ? "on" : "off"};
};
await syncTermInteraction("ssh-prod");
const genuineOff = {...termInteraction.get("ssh-prod")};
await setTermSelectionMode("ssh-prod", false);
const interacting = {...termInteraction.get("ssh-prod")};
await setTermSelectionMode("ssh-prod", true);
const temporary = {...termInteraction.get("ssh-prod")};
await restoreTermInteraction("ssh-prod");
const restored = {...termInteraction.get("ssh-prod")};
failNext = true;
await setTermSelectionMode("ssh-prod", true);
const afterFailure = {...termInteraction.get("ssh-prod")};
console.log(JSON.stringify({
  posts, genuineOff, interacting, temporary, restored, afterFailure,
  postedStates,
}));
""")

    assert result["genuineOff"]["mouse"] is False
    assert result["genuineOff"]["temporary"] is False
    assert result["interacting"]["mouse"] is True
    assert result["interacting"]["temporary"] is False
    assert result["temporary"]["mouse"] is False
    assert result["temporary"]["temporary"] is True
    assert result["restored"]["mouse"] is True
    assert result["restored"]["temporary"] is False
    assert result["afterFailure"]["mouse"] is True
    assert result["afterFailure"]["temporary"] is False
    assert [post["enabled"] for post in result["posts"]] == [True, False, True, False]
    assert result["postedStates"][-1]["message"] == {
        "source": "comandos", "type": "interaction-state",
        "known": True, "busy": False, "selecting": False,
    }


def test_terminal_interaction_bridge_validates_source_and_posts_confirmed_state():
    result = run_term_interaction("""
const calls = [];
let finishRequest;
api = async function(_path, body) {
  calls.push(body || null);
  if(body && body.enabled === false)
    return new Promise(resolve => { finishRequest = () => resolve({mouse:"off"}); });
  return {mouse:"on"};
};
const state = termInteractionState("ssh-prod");
state.mouse = true;
const source = frames["ssh-prod"].contentWindow;
const rejectedOrigin = handleTermFrameMessage({
  origin:"https://evil.test", source,
  data:{source:"comandos-term", type:"interaction-request", selecting:true},
});
const rejectedSource = handleTermFrameMessage({
  origin:location.origin, source:{},
  data:{source:"comandos-term", type:"interaction-request", selecting:true},
});
const ready = handleTermFrameMessage({
  origin:location.origin, source,
  data:{source:"comandos-term", type:"ready"},
});
const requested = handleTermFrameMessage({
  origin:location.origin, source,
  data:{source:"comandos-term", type:"interaction-request", selecting:true},
});
await Promise.resolve();
const pending = postedStates.at(-1).message;
finishRequest();
await Promise.resolve();
await Promise.resolve();
const sshAfterOwnRequest = {...termInteraction.get("ssh-prod")};
const devState = termInteractionState("dev");
devState.mouse = true;
const compatState = termInteractionState("compat");
compatState.mouse = true;
api = async function(_path, body) { calls.push(body || null); return {mouse:"off"}; };
const devRequested = handleTermFrameMessage({
  origin:location.origin, source:frames.dev.contentWindow,
  data:{source:"comandos-term", type:"interaction-request", selecting:true},
});
await Promise.resolve();
await Promise.resolve();
const compatRejected = handleTermFrameMessage({
  origin:location.origin, source:frames.compat.contentWindow,
  data:{source:"comandos-term", type:"interaction-request", selecting:true},
});
console.log(JSON.stringify({
  rejectedOrigin, rejectedSource, ready, requested, calls, pending,
  confirmed: postedStates.at(-1).message,
  devRequested, compatRejected, sshAfterOwnRequest,
  devAfterRequest:{...termInteraction.get("dev")},
  compatAfterRequest:{...termInteraction.get("compat")},
}));
""")

    assert result["rejectedOrigin"] is False
    assert result["rejectedSource"] is False
    assert result["ready"] is True
    assert result["requested"] is True
    assert result["calls"] == [
        {"session": "ssh-prod", "enabled": False},
        {"session": "dev", "enabled": False},
    ]
    assert result["pending"] == {
        "source": "comandos", "type": "interaction-state",
        "known": True, "busy": True, "selecting": False,
    }
    assert result["confirmed"] == {
        "source": "comandos", "type": "interaction-state",
        "known": True, "busy": False, "selecting": True,
    }
    assert result["devRequested"] is True
    assert result["compatRejected"] is False
    assert result["sshAfterOwnRequest"]["mouse"] is False
    assert result["devAfterRequest"]["mouse"] is False
    assert result["compatAfterRequest"]["mouse"] is True


def test_named_terminal_message_bridge_executes_theme_and_interaction_paths_independently():
    handler = extract_js_function(TERM_HTML, "handleTerminalMessage")
    apply_theme = extract_js_function(TERM_HTML, "applyTerminalTheme")
    apply_interaction = extract_js_function(TERM_HTML, "applyInteractionState")
    themes = extract_js_initializer(TERM_HTML, "THEMES")
    registration = "window.addEventListener('message', handleTerminalMessage);"
    assert TERM_HTML.count(registration) == 1
    result = run_node_json(f"""
const THEMES = {themes};
let activeTheme = "noche";
let interactionState = {{known:false, busy:false, selecting:false}};
const styles = {{}};
const shell = {{style:{{}}}};
const modeButton = {{
  disabled:false, textContent:"", attributes:{{}}, selected:false,
  classList:{{toggle(_name, value) {{ modeButton.selected = value; }}}},
  setAttribute(name, value) {{ this.attributes[name] = value; }},
}};
const document = {{
  documentElement:{{style:{{setProperty(name, value) {{ styles[name] = value; }}}}}},
  body:{{style:{{}}}},
  getElementById(id) {{ return id === "term-shell" ? shell : null; }},
  querySelector(selector) {{ return selector === '[data-action="mode"]' ? modeButton : null; }},
}};
const parent = {{id:"parent"}};
const location = {{origin:"https://dash.test"}};
const listeners = {{}};
let messageRegistrations = 0;
const window = {{addEventListener(type, callback) {{
  if(type === "message") {{ messageRegistrations += 1; listeners.message = callback; }}
}}}};
let connectionCount = 0;
class WebSocket {{ constructor() {{ connectionCount += 1; this.id = "same-socket"; }} }}
const ws = new WebSocket();
const originalSocket = ws;
const term = {{options:{{theme:THEMES.noche}}}};
function refreshLigatures() {{}}
{apply_interaction}
{apply_theme}
{handler}
{registration}
const interactionAccepted = listeners.message({{
  origin:location.origin, source:parent,
  data:{{source:"comandos", type:"interaction-state", known:true, busy:false, selecting:true}},
}});
const interactionAfterState = {{...interactionState}};
const themeAccepted = listeners.message({{
  origin:location.origin, source:parent,
  data:{{source:"comandos", type:"theme", theme:"bruno"}},
}});
const interactionAfterTheme = {{...interactionState}};
const beforeInvalid = {{
  theme:activeTheme, termTheme:term.options.theme, styles:JSON.stringify(styles),
  interaction:{{...interactionState}}, socket:ws, connectionCount,
}};
const badDataSource = listeners.message({{
  origin:location.origin, source:parent,
  data:{{source:"not-comandos", type:"theme", theme:"noche"}},
}});
const badDataSourceUnchanged = beforeInvalid.theme === activeTheme &&
  beforeInvalid.termTheme === term.options.theme &&
  beforeInvalid.styles === JSON.stringify(styles) &&
  JSON.stringify(beforeInvalid.interaction) === JSON.stringify(interactionState) &&
  beforeInvalid.socket === ws && beforeInvalid.connectionCount === connectionCount;
const badOrigin = listeners.message({{
  origin:"https://evil.test", source:parent,
  data:{{source:"comandos", type:"theme", theme:"noche"}},
}});
const badSource = listeners.message({{
  origin:location.origin, source:{{}},
  data:{{source:"comandos", type:"interaction-state", known:true, selecting:false}},
}});
const unknownType = listeners.message({{
  origin:location.origin, source:parent,
  data:{{source:"comandos", type:"unknown", theme:"noche"}},
}});
const unknownTheme = listeners.message({{
  origin:location.origin, source:parent,
  data:{{source:"comandos", type:"theme", theme:"unknown"}},
}});
console.log(JSON.stringify({{
  messageRegistrations, interactionAccepted, themeAccepted,
  interactionAfterState, interactionAfterTheme,
  modeLabel:modeButton.textContent, modeSelected:modeButton.selected,
  themeIdentity:term.options.theme === THEMES.bruno,
  termTheme:term.options.theme,
  rootBackground:document.documentElement.style.background,
  bodyBackground:document.body.style.background,
  shellBackground:shell.style.background,
  styles,
  badDataSource, badDataSourceUnchanged,
  badOrigin, badSource, unknownType, unknownTheme,
  invalidUnchanged:beforeInvalid.theme === activeTheme &&
    beforeInvalid.termTheme === term.options.theme &&
    beforeInvalid.styles === JSON.stringify(styles) &&
    JSON.stringify(beforeInvalid.interaction) === JSON.stringify(interactionState) &&
    beforeInvalid.socket === ws && beforeInvalid.connectionCount === connectionCount,
  socketSame:ws === originalSocket, socketId:ws.id, connectionCount,
}}));
""")

    assert result == {
        "messageRegistrations": 1,
        "interactionAccepted": True,
        "themeAccepted": True,
        "interactionAfterState": {"known": True, "busy": False, "selecting": True},
        "interactionAfterTheme": {"known": True, "busy": False, "selecting": True},
        "modeLabel": "Interactuar",
        "modeSelected": True,
        "themeIdentity": True,
        "termTheme": {
            "background": "#1A1A1A", "foreground": "#CCCCCC",
            "cursor": "#E4AE49", "cursorAccent": "#1A1A1A",
            "selectionBackground": "#444444", "panel": "#222224",
            "panel2": "#26292B", "line": "#333333", "line2": "#444444",
            "dim": "#AAAAAA", "faint": "#999999", "brand": "#E4AE49",
            "black": "#1A1A1A", "red": "#DA462F", "green": "#73E89A",
            "yellow": "#FAD075", "blue": "#8BC2F9", "magenta": "#D691ED",
            "cyan": "#7DDFF2", "white": "#CCCCCC", "brightBlack": "#666666",
            "brightRed": "#F38172", "brightGreen": "#73E89A",
            "brightYellow": "#FAD075", "brightBlue": "#8BC2F9",
            "brightMagenta": "#D691ED", "brightCyan": "#7DDFF2",
            "brightWhite": "#FFFFFF",
        },
        "rootBackground": "#1A1A1A",
        "bodyBackground": "#1A1A1A",
        "shellBackground": "#1A1A1A",
        "styles": {
            "--term-bg": "#1A1A1A", "--term-panel": "#222224",
            "--term-panel2": "#26292B", "--term-line": "#333333",
            "--term-line2": "#444444", "--term-text": "#CCCCCC",
            "--term-dim": "#AAAAAA", "--term-faint": "#999999",
            "--term-brand": "#E4AE49",
        },
        "badDataSource": False,
        "badDataSourceUnchanged": True,
        "badOrigin": False,
        "badSource": False,
        "unknownType": False,
        "unknownTheme": False,
        "invalidUnchanged": True,
        "socketSame": True,
        "socketId": "same-socket",
        "connectionCount": 1,
    }


def test_remote_terminal_selection_ignores_stale_get_and_keepalive_is_temporary_only():
    result = run_term_interaction("""
let resolveAlpha;
api = async function(path, body) {
  if(body) return {mouse: body.enabled ? "on" : "off"};
  if(path.includes("alpha")) return new Promise(resolve => { resolveAlpha = resolve; });
  return {mouse: "off"};
};
activeTerm = "alpha";
const pending = syncTermInteraction("alpha");
await Promise.resolve();
activeTerm = "beta";
restoreInactiveTermInteractions("beta");
await syncTermInteraction("beta");
resolveAlpha({mouse: "on"});
await pending;
const alphaAfterStale = {...termInteraction.get("alpha")};
const beta = termInteraction.get("beta");
beta.temporary = false;
const dev = termInteractionState("dev");
dev.mouse = false;
dev.temporary = true;
restoreAllTermInteractions();
console.log(JSON.stringify({
  alphaAfterStale,
  beta: {...beta},
  keepaliveSessions: keepalives.map(item => JSON.parse(item.options.body).session),
  keepaliveFlags: keepalives.map(item => item.options.keepalive),
}));
""")

    assert result["alphaAfterStale"]["mouse"] is None
    assert result["beta"]["mouse"] is False
    assert result["beta"]["temporary"] is False
    assert result["keepaliveSessions"] == ["dev"]
    assert result["keepaliveFlags"] == [True]


def test_remote_terminal_selection_persists_across_tab_switches():
    result = run_term_interaction("""
const posts = [];
let finishSelection;
api = async function(_path, body) {
  posts.push(body);
  if(body.enabled === false)
    return new Promise(resolve => { finishSelection = resolve; });
  return {mouse: "on"};
};
const state = termInteractionState("ssh-prod");
state.mouse = true;
const selecting = setTermSelectionMode("ssh-prod", true);
await Promise.resolve();
activeTerm = "beta";
restoreInactiveTermInteractions("beta");
finishSelection({mouse: "off"});
await selecting;
await Promise.resolve();
await Promise.resolve();
restoreInactiveTermInteractions("beta");
await Promise.resolve();
await Promise.resolve();
console.log(JSON.stringify({
  state: {...termInteraction.get("ssh-prod")},
  enabled: posts.map(post => post.enabled),
}));
""")

    assert result["enabled"] == [False]
    assert result["state"]["mouse"] is False
    assert result["state"]["temporary"] is True
    assert result["state"]["restorePending"] is False


def test_remote_terminal_pagehide_compensates_pending_select_and_pageshow_resyncs():
    result = run_term_interaction("""
let finishSelection;
let backendMouse = true;
keepaliveHook = item => {
  backendMouse = JSON.parse(item.options.body).enabled;
};
api = async function(_path, body) {
  if(!body) return {mouse: backendMouse ? "on" : "off"};
  if(body.enabled === false) return new Promise(resolve => {
    finishSelection = () => { backendMouse = false; resolve({mouse: "off"}); };
  });
  backendMouse = true;
  return {mouse: "on"};
};
const state = termInteractionState("ssh-prod");
state.mouse = true;
const selecting = setTermSelectionMode("ssh-prod", true);
await Promise.resolve();
restoreAllTermInteractions();
const immediateKeepalives = keepalives.length;
finishSelection();
await selecting;
await Promise.resolve();
const afterResolutionKeepalives = keepalives.length;
await handleTermInteractionsPageShow();
console.log(JSON.stringify({
  immediateKeepalives,
  afterResolutionKeepalives,
  payloads: keepalives.map(item => JSON.parse(item.options.body)),
  state: {...termInteraction.get("ssh-prod")},
  postedStates,
}));
""")

    assert result["immediateKeepalives"] == 1
    assert result["afterResolutionKeepalives"] == 2
    assert all(payload == {"session": "ssh-prod", "enabled": True}
               for payload in result["payloads"])
    assert result["state"]["mouse"] is True
    assert result["state"]["temporary"] is False
    assert result["state"]["busy"] is False
    assert result["postedStates"][-1]["message"]["selecting"] is False


def test_remote_terminal_initial_pageshow_does_not_resync():
    result = run_term_interaction("""
let gets = 0;
api = async function(_path, body) {
  if(!body) gets += 1;
  return {mouse: "on"};
};
const state = termInteractionState("ssh-prod");
state.mouse = true;
const handled = await handleTermInteractionsPageShow();
console.log(JSON.stringify({
  gets,
  handled,
  mouse: state.mouse,
  seq: state.seq,
}));
""")

    assert result == {"gets": 0, "handled": False, "mouse": True, "seq": 0}


def test_remote_terminal_closed_session_state_is_cleaned_after_restore():
    result = run_term_interaction("""
const posts = [];
api = async function(_path, body) { posts.push(body); return {mouse: "on"}; };
const settled = termInteractionState("dev");
settled.mouse = true;
openTerms.delete("dev");
cleanupTermInteraction("dev");
const settledRemoved = !termInteraction.has("dev");
const temporary = termInteractionState("ssh-prod");
temporary.mouse = false;
temporary.temporary = true;
openTerms.delete("ssh-prod");
await restoreTermInteraction("ssh-prod");
await Promise.resolve();
console.log(JSON.stringify({
  settledRemoved,
  restoredRemoved: !termInteraction.has("ssh-prod"),
  enabled: posts.map(post => post.enabled),
}));
""")

    assert result == {
        "settledRemoved": True,
        "restoredRemoved": True,
        "enabled": [True],
    }


def test_existing_ssh_manager_can_setup_passwordless_key_access():
    assert "/ssh-key-setup" in HTML
    assert "setupSshKey" in HTML
    assert 'button class="key"' in HTML
    assert "row.querySelector(\".key\")" in HTML
    fn = extract_js_function(HTML, "setupSshKey")
    assert 'api("/ssh-key-setup"' in fn
    assert "openInApp(r.session" in fn
    assert "openTerm(r.session" in fn


def test_left_clicking_ssh_chip_opens_a_fresh_ssh_tab():
    assert "/ssh-new-tab" in HTML
    assert "openSshTab" in HTML
    fn = extract_js_function(HTML, "openSshTab")
    assert 'api("/ssh-new-tab"' in fn
    assert "openInApp(r.session" in fn
    assert "openTerm(r.session" in fn
    load_fn = extract_js_function(HTML, "loadSsh")
    assert "openSshTab(h.host)" in load_fn
    assert "connectHost(h.host)" in load_fn


def test_remote_buttons_reflect_actual_backend_state():
    off = remote_button_state({
        "remoteOn": False,
        "primaryHealthy": False,
        "fallbackHealthy": False,
        "webtermReachable": False,
    })
    assert off["remoteOnDisabled"] is False
    assert off["remoteOffDisabled"] is True
    assert off["webtermOnDisabled"] is False
    assert off["webtermOffDisabled"] is True
    assert off["openTerminalDisabled"] is True

    on = remote_button_state({
        "remoteOn": True,
        "primaryHealthy": True,
        "fallbackHealthy": True,
        "webtermReachable": True,
    })
    assert on["remoteOnDisabled"] is True
    assert on["remoteOffDisabled"] is False
    assert on["webtermOnDisabled"] is True
    assert on["webtermOffDisabled"] is False
    assert on["openTerminalDisabled"] is False

    busy = remote_button_state({"remoteOn": True}, busy=True)
    assert all(v is True for k, v in busy.items() if k.endswith("Disabled"))


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


def test_terminal_attach_retries_primary_without_memoizing_fallback():
    functions = "\n\n".join(extract_js_function(HTML, name) for name in (
        "delay",
        "probePrimaryTerm",
        "fallbackTermAvailable",
        "resolveTermBase",
    ))
    result = run_node_json(f"""
const WEBTERM = true;
const TERM_BASE = "https://zion.tail63a117.ts.net/term";
const TERM_FALLBACK_BASE = "https://zion.tail63a117.ts.net:8443";
const TERM_PRIMARY_ATTEMPTS = 3;
const TERM_PRIMARY_RETRY_MS = 400;
const TERM_PRIMARY_PROBE_TIMEOUT_MS = 800;
let primaryProbes = 0;
let fallbackLookups = 0;
const fetch = async () => ({{ok: ++primaryProbes === 4}});
const api = async () => {{
  fallbackLookups += 1;
  return {{fallbackTerminalOn: true}};
}};
const tf = (_es, en) => en;
const toast = () => {{}};
function showTermDegraded() {{}}
function setTimeout(callback) {{ callback(); }}
function clearTimeout() {{}}
class AbortController {{
  constructor() {{ this.signal = {{}}; }}
  abort() {{}}
}}

{functions}

(async () => {{
  const results = [await resolveTermBase(), await resolveTermBase()];
  console.log(JSON.stringify({{results, primaryProbes, fallbackLookups}}));
}})().catch(error => {{ console.error(error); process.exit(1); }});
""")
    assert result == {
        "results": [
            "https://zion.tail63a117.ts.net:8443",
            "https://zion.tail63a117.ts.net/term",
        ],
        "primaryProbes": 4,
        "fallbackLookups": 1,
    }


def test_primary_terminal_probe_aborts_stalled_fetch_and_clears_timeout():
    probe = extract_js_function(HTML, "probePrimaryTerm")
    result = run_node_json(f"""
const TERM_BASE = "https://zion.tail63a117.ts.net/term";
const TERM_PRIMARY_PROBE_TIMEOUT_MS = 800;
const timers = new Map();
const cleared = [];
let nextTimer = 1;
let fetchMode = "stall";
function setTimeout(callback, delay) {{
  const id = nextTimer++;
  timers.set(id, {{callback, delay}});
  return id;
}}
function clearTimeout(id) {{ cleared.push(id); timers.delete(id); }}
class AbortController {{
  constructor() {{
    this.signal = {{aborted: false, listeners: []}};
  }}
  abort() {{
    this.signal.aborted = true;
    this.signal.listeners.forEach(listener => listener());
  }}
}}
function fetch(_url, options) {{
  if(fetchMode === "success") return Promise.resolve({{ok: true}});
  return new Promise((_resolve, reject) => {{
    options.signal.listeners.push(() => reject(new Error("aborted")));
  }});
}}

{probe}

(async () => {{
  const stalled = probePrimaryTerm();
  const [timeoutId, timeout] = [...timers.entries()][0];
  timeout.callback();
  const stalledResult = await stalled;
  fetchMode = "success";
  const successfulResult = await probePrimaryTerm();
  console.log(JSON.stringify({{
    stalledResult,
    successfulResult,
    timeoutDelay: timeout.delay,
    timeoutCleared: cleared.includes(timeoutId),
    activeTimers: timers.size,
    clearCount: cleared.length,
  }}));
}})().catch(error => {{ console.error(error); process.exit(1); }});
""")
    assert result == {
        "stalledResult": False,
        "successfulResult": True,
        "timeoutDelay": 800,
        "timeoutCleared": True,
        "activeTimers": 0,
        "clearCount": 2,
    }


def test_remote_routes_are_never_served_from_stale_shell_cache():
    assert 'const SHELL = "comandos-shell-v2"' in SW
    for endpoint in (
        "/remote-state",
        "/remote-qr.png",
        "/tabs",
        "/tab-history",
    ):
        assert endpoint in SW
    assert "live.some" in SW


def test_dashboard_declares_standard_favicon_to_avoid_remote_404_noise():
    assert '<link rel="icon" href="/icon-192.png">' in HTML




def test_remote_term_has_no_duplicate_mouse_emoji_toggle():
    assert "🖱" not in HTML
    assert "termMouse" not in HTML
    assert "mousetgl" not in HTML
    assert 'api("/tmux-mouse"' in HTML


def test_remote_term_page_fixes_mobile_keys_and_ws():
    term = open("dash/term.html").read()
    assert '<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content">' in term
    # backspace/enter de teclado movil (GBoard no manda keydown)
    assert "beforeinput" in term
    assert "deleteContentBackward" in term
    # ws same-origin cuando se sirve bajo /term
    assert "location.host" in term and "/ws" in term


def test_remote_term_touch_longpress_resizes_panes():
    # Gesto tactil: dejar el dedo (~300ms) y arrastrar = drag de mouse SGR
    # (solo cuando tmux pidio tracking). Verificado E2E: panes 50/50 -> 37/63.
    term = open("dash/term.html").read()
    assert "touchstart" in term and "touchmove" in term and "touchend" in term
    assert "mouseTrackingMode" in term        # sin tracking → scroll normal
    assert "\\x1b[<" in term                  # protocolo SGR press/motion/release
    assert "redimensionando" in term          # indicador visible durante el drag
    assert "navigator.vibrate" in term        # feedback haptico al enganchar
    # v2 smooth: snap al borde leyendo el buffer (│/─ hasta ±2 celdas, eje
    # bloqueado) y motions coalescidos a 1 por frame — verificado E2E en
    # ambos ejes (50/50→40/60 horizontal; 21/21→13/29 vertical)
    assert "snapToBorder" in term
    assert "requestAnimationFrame" in term
    # ligaduras solo desktop: el overlay hacia lag en el resize movil
    assert "IS_TOUCH" in term


def test_webterm_serves_our_index_when_supported():
    src = open("bin/cc-webterm").read()
    assert '--index' in src and 'term.html' in src
    assert '-b /term "${IFLAG[@]}"' in src


def test_tabs_endpoint_is_exact_mirror_no_history_resurrection():
    # tab_labels() mezclaba el HISTORIAL: cualquier sesion viva cuya tab
    # cerraste reaparecia en /tabs (por eso el remoto tenia MAS tabs que el
    # escritorio y las cerradas "no se podian cerrar"). /tabs y /tab-history
    # usan tab_labels (archivo puro); las etiquetas bonitas van aparte.
    src = open("bin/cc-dash").read()
    body = src.split("def tab_labels():", 1)[1].split("def session_labels", 1)[0]
    assert "read_tab_history" not in body        # JAMAS historial en el espejo
    assert "def session_labels" in src           # display separado
    tabs_handler = src.split('self.path.startswith("/tabs")', 1)[1].split("self.path.startswith", 1)[0]
    assert "tab_labels()" in tabs_handler and "session_labels" not in tabs_handler


def test_remote_tabs_have_full_desktop_parity():
    # TODA tab abierta en remoto se registra en el escritorio (openTerm ->
    # /tab-register, mirrored=true) y el + crea terminal en AMBOS lados.
    # Sin esto el remoto acumulaba tabs locales que el escritorio no veia.
    html = open("dash/index.html").read()
    assert 'api("/tab-register", {session: sess, label: label || sess})' in html
    assert 'addTermTab(sess, label, true);' in html
    assert 'api("/tab-new", {})' in html
    assert '"Nueva terminal (se abre también en el escritorio)"' in html
    src = open("bin/cc-dash").read()
    assert '"/tab-register"' in src and '"/tab-new"' in src
    assert "def register_app_tab" in src
    assert "app-tab-open.json" in src


def test_ssh_privacy_note_states_local_only_storage():
    html = open("dash/index.html").read()
    assert "srv-privacy" in html
    assert "~/.ssh/config" in html
    assert "nunca guarda passwords" in html


if __name__ == "__main__":
    test_tabs_endpoint_is_exact_mirror_no_history_resurrection()
    test_ssh_privacy_note_states_local_only_storage()
    test_remote_tabs_have_full_desktop_parity()
    test_remote_term_has_no_duplicate_mouse_emoji_toggle()
    test_remote_term_page_fixes_mobile_keys_and_ws()
    test_remote_term_touch_longpress_resizes_panes()
    test_webterm_serves_our_index_when_supported()
    test_remote_drawer_controls_are_present()
    test_remote_ui_calls_backend_endpoints()
    test_remote_polling_slows_down_when_remote_webterm_is_enabled()
    test_remote_terminal_can_be_opened_from_remote_drawer()
    test_open_terminal_button_dismisses_remote_drawer()
    test_remote_terminal_iframe_does_not_hijack_tmux_wheel_events()
    test_remote_touch_scroll_is_throttled_for_tmux_wheel_ticks()
    test_remote_terminal_selection_uses_backend_state_not_local_guess()
    test_remote_terminal_selection_sync_click_restore_and_failure_integrate()
    test_remote_terminal_selection_ignores_stale_get_and_keepalive_is_temporary_only()
    test_existing_ssh_manager_can_setup_passwordless_key_access()
    test_left_clicking_ssh_chip_opens_a_fresh_ssh_tab()
    test_remote_buttons_reflect_actual_backend_state()
    test_remote_routes_are_never_served_from_stale_shell_cache()
    test_dashboard_declares_standard_favicon_to_avoid_remote_404_noise()
