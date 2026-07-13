#!/usr/bin/env python3
import json
import subprocess
from pathlib import Path


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


def touch_controller_js():
    start = TERM_HTML.index("  const screenEl = () =>")
    end = TERM_HTML.index("  const ta = term.textarea;", start)
    return TERM_HTML[start:end]


def term_interaction_js():
    names = (
        "termInteractionState",
        "applyTermInteraction",
        "updateTermSelectButton",
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
const button = {{
  disabled: false,
  textContent: "",
  title: "",
  attrs: {{}},
  classList: {{toggle(name, on) {{ if(on) classNames.add(name); else classNames.delete(name); }}}},
  setAttribute(name, value) {{ this.attrs[name] = value; }},
}};
const frames = {{
  "ssh-prod": {{dataset: {{}}}},
  "dev": {{dataset: {{}}}},
  "alpha": {{dataset: {{}}}},
  "beta": {{dataset: {{}}}},
}};
const openTerms = new Map(Object.entries(frames).map(([sess, frame]) => [sess, {{frame}}]));
const document = {{getElementById(id) {{ return id === "term-select-toggle" ? button : null; }}}};
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
let nextTimer = 1;
let nextFrame = 1;
let indicator = null;
const cells = {json.dumps(cells or {})};

function setTimeout(cb) {{ const id = nextTimer++; timers.set(id, cb); return id; }}
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
const frameElement = {{dataset: {json.dumps({'selecting': '1'} if selecting else {})}}};
const term = {{
  cols: 80,
  rows: 24,
  modes: {{mouseTrackingMode: {json.dumps('sgr' if mouse_on else 'none')}}},
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


def test_webterm_fit_is_coalesced_per_animation_frame():
    schedule_fit = extract_js_function(TERM_HTML, "scheduleFit")
    script = f"""
let fitFrame = 0;
let fitCalls = 0;
let nextFrame = 1;
const queued = new Map();
const fit = {{fit(){{ fitCalls += 1; }}}};
function requestAnimationFrame(cb){{ const id = nextFrame++; queued.set(id, cb); return id; }}
{schedule_fit}
for(let i = 0; i < 100; i++) scheduleFit();
const beforeFlush = fitCalls;
const queuedBeforeFlush = queued.size;
const callbacks = [...queued.values()]; queued.clear(); callbacks.forEach(cb => cb());
console.log(JSON.stringify({{beforeFlush, queuedBeforeFlush, fitCalls, fitFrame}}));
"""
    result = run_node_json(script)

    assert result == {
        "beforeFlush": 0,
        "queuedBeforeFlush": 1,
        "fitCalls": 1,
        "fitFrame": 0,
    }
    assert "window.addEventListener('resize', scheduleFit)" in TERM_HTML


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
        "// Teclado MOVIL", 1
    )[0]
    prelude = r"""
let fitCalls = 0;
let nextFrame = 1;
const frameQueue = new Map();
const timerQueue = [];
const windowListeners = {};
const elements = new Map();
for (const id of ['dbg', 'err', 'term']) {
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
  title: '', body: {appendChild(){}},
  getElementById(id){ return elements.get(id); },
  createElement(){ return {}; },
};
globalThis.requestAnimationFrame = cb => {
  const id = nextFrame++; frameQueue.set(id, cb); return id;
};
globalThis.setTimeout = cb => { timerQueue.push(cb); return timerQueue.length; };
globalThis.Terminal = class {
  constructor(options){ this.options = {...options}; this.cols = 80; this.rows = 20; }
  loadAddon(addon){ addon.term = this; }
  open(){}
  write(){}
  onData(cb){ this.dataHandler = cb; }
  onResize(cb){ this.resizeHandler = cb; }
};
class FakeFit {
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
    assert result["fitCalls"] == 3
    assert result["fontFamily"] == "Test Mono"
    assert result["fontSize"] == 18


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
    assert "frame.dataset.selecting" in fn
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
    assert 'id="term-select-toggle"' in HTML
    assert "/tmux-mouse" in HTML
    assert "const termInteraction = new Map()" in HTML
    assert "syncTermInteraction" in HTML
    assert "setTermSelectionMode" in HTML
    assert "restoreTermInteraction" in HTML
    fn = extract_js_function(HTML, "setTermSelectionMode")
    assert 'api("/tmux-mouse"' in fn
    assert "enabled: !selecting" in fn
    assert "termInteractionState(sess)" in fn
    assert "state.temporary = selecting" in fn
    assert "applyTermInteraction(sess)" in fn
    show = extract_js_function(HTML, "showView")
    assert "syncTermInteraction(shown)" in show
    assert "updateTermSelectButton();" in show
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
const genuineLabel = button.textContent;
await setTermSelectionMode("ssh-prod", false);
const interacting = {...termInteraction.get("ssh-prod")};
const interactLabel = button.textContent;
await setTermSelectionMode("ssh-prod", true);
const temporary = {...termInteraction.get("ssh-prod")};
const temporaryLabel = button.textContent;
await restoreTermInteraction("ssh-prod");
const restored = {...termInteraction.get("ssh-prod")};
failNext = true;
await setTermSelectionMode("ssh-prod", true);
const afterFailure = {...termInteraction.get("ssh-prod")};
console.log(JSON.stringify({
  posts, genuineOff, genuineLabel, interacting, interactLabel,
  temporary, temporaryLabel, restored, afterFailure,
  selecting: frames["ssh-prod"].dataset.selecting || "",
  disabled: button.disabled,
}));
""")

    assert result["genuineOff"]["mouse"] is False
    assert result["genuineOff"]["temporary"] is False
    assert result["genuineLabel"] == "Interact"
    assert result["interacting"]["mouse"] is True
    assert result["interacting"]["temporary"] is False
    assert result["interactLabel"] == "Select"
    assert result["temporary"]["mouse"] is False
    assert result["temporary"]["temporary"] is True
    assert result["temporaryLabel"] == "Interact"
    assert result["restored"]["mouse"] is True
    assert result["restored"]["temporary"] is False
    assert result["afterFailure"]["mouse"] is True
    assert result["afterFailure"]["temporary"] is False
    assert result["selecting"] == ""
    assert result["disabled"] is False
    assert [post["enabled"] for post in result["posts"]] == [True, False, True, False]


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


def test_remote_terminal_selection_finishing_after_tab_switch_is_restored():
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
console.log(JSON.stringify({
  state: {...termInteraction.get("ssh-prod")},
  enabled: posts.map(post => post.enabled),
}));
""")

    assert result["enabled"] == [False, True]
    assert result["state"]["mouse"] is True
    assert result["state"]["temporary"] is False


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
  selecting: frames["ssh-prod"].dataset.selecting || "",
  label: button.textContent,
}));
""")

    assert result["immediateKeepalives"] == 1
    assert result["afterResolutionKeepalives"] == 2
    assert all(payload == {"session": "ssh-prod", "enabled": True}
               for payload in result["payloads"])
    assert result["state"]["mouse"] is True
    assert result["state"]["temporary"] is False
    assert result["state"]["busy"] is False
    assert result["selecting"] == ""
    assert result["label"] == "Select"


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
    off = remote_button_state({"remoteOn": False, "webtermOn": False})
    assert off["remoteOnDisabled"] is False
    assert off["remoteOffDisabled"] is True
    assert off["webtermOnDisabled"] is False
    assert off["webtermOffDisabled"] is True
    assert off["openTerminalDisabled"] is True

    on = remote_button_state({"remoteOn": True, "webtermOn": True})
    assert on["remoteOnDisabled"] is True
    assert on["remoteOffDisabled"] is False
    assert on["webtermOnDisabled"] is True
    assert on["webtermOffDisabled"] is False
    assert on["openTerminalDisabled"] is False

    busy = remote_button_state({"remoteOn": True, "webtermOn": False}, busy=True)
    assert all(v is True for k, v in busy.items() if k.endswith("Disabled"))


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
    assert '<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">' in term
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
