import ast
import json
import re
import subprocess
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
ANSI_KEYS = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue",
    "brightMagenta", "brightCyan", "brightWhite",
]
THEME_IDS = {"noche", "dia", "calido", "termius", "bruno"}
BRUNO_ROLES = {
    "bg": "#1A1A1A", "panel": "#222224", "panel2": "#26292B",
    "line": "#333333", "line2": "#444444", "text": "#CCCCCC",
    "dim": "#AAAAAA", "faint": "#999999", "brand": "#E4AE49",
    "waiting": "#F6AB79", "done": "#73E89A", "working": "#8BC2F9",
}


def extract_js_function(source, name):
    for prefix in ("async function ", "function "):
        start = source.find(f"{prefix}{name}(")
        if start >= 0:
            break
    else:
        raise AssertionError(f"{name} not found")
    brace = source.index("{", start)
    depth = 0
    for index in range(brace, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[start:index + 1]
    raise AssertionError(f"unterminated {name}")


def run_node_json(script):
    return json.loads(subprocess.check_output(["node", "-e", script], text=True))


def extract_js_initializer(source, name):
    match = re.search(rf"\b(?:const|let|var)\s+{re.escape(name)}\s*=\s*", source)
    assert match, f"{name} initializer not found"
    start = match.end()
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
    raise AssertionError(f"unterminated {name} initializer")


def eval_js_initializer(source, name):
    initializer = extract_js_initializer(source, name)
    return run_node_json(
        f"const value = {initializer}; console.log(JSON.stringify(value));"
    )


def exec_python_assignments(source, names):
    wanted = set(names)
    selected = []
    for node in ast.parse(source).body:
        if not isinstance(node, ast.Assign):
            continue
        targets = {target.id for target in node.targets if isinstance(target, ast.Name)}
        if targets & wanted:
            selected.append(node)
    module = ast.fix_missing_locations(ast.Module(body=selected, type_ignores=[]))
    namespace = {}
    exec(compile(module, "<theme-assignments>", "exec"), namespace)
    return namespace


def backend_theme_whitelist():
    for node in ast.walk(ast.parse(DASH)):
        if not isinstance(node, ast.Compare) or len(node.ops) != 1:
            continue
        call = node.left
        if not (isinstance(node.ops[0], ast.In) and isinstance(call, ast.Call)
                and isinstance(call.func, ast.Attribute) and call.func.attr == "get"
                and len(call.args) == 1 and isinstance(call.args[0], ast.Constant)
                and call.args[0].value == "theme"):
            continue
        return set(ast.literal_eval(node.comparators[0]))
    raise AssertionError("theme preference whitelist not found")


def native_theme_namespace():
    names = {"PALETTE", "PAL_DIA", "PAL_CALIDO", "PAL_BRUNO", "THEMES"}
    return exec_python_assignments(APP, names)


def native_apply_namespace():
    assignments = {"PALETTE", "PAL_DIA", "PAL_CALIDO", "PAL_BRUNO", "THEMES", "APP_CSS"}
    functions = {"theme_css", "_build_hb_css", "apply_theme", "_apply_tmux_theme"}
    selected = []
    for node in ast.parse(APP).body:
        if isinstance(node, ast.Assign):
            targets = {target.id for target in node.targets if isinstance(target, ast.Name)}
            if targets & assignments:
                selected.append(node)
        elif isinstance(node, ast.FunctionDef) and node.name in functions:
            selected.append(node)
    module = ast.fix_missing_locations(ast.Module(body=selected, type_ignores=[]))
    namespace = {}
    exec(compile(module, "<native-theme-apply>", "exec"), namespace)
    return namespace


def test_every_theme_registry_accepts_exactly_the_five_approved_ids():
    term_themes = eval_js_initializer(TERM, "THEMES")
    native_themes = native_theme_namespace()["THEMES"]
    dashboard_sequence = eval_js_initializer(INDEX, "THEME_SEQ")

    assert set(term_themes) == THEME_IDS
    assert set(native_themes) == THEME_IDS
    assert dashboard_sequence == ["noche", "dia", "calido", "termius", "bruno"]
    assert backend_theme_whitelist() == THEME_IDS


def test_bruno_xterm_ansi_associations_and_native_palette_are_exact():
    term_bruno = eval_js_initializer(TERM, "THEMES")["bruno"]
    native_bruno = native_theme_namespace()["THEMES"]["bruno"]

    assert [key for key in term_bruno if key in ANSI_KEYS] == ANSI_KEYS
    assert [(key, term_bruno[key]) for key in ANSI_KEYS] == list(zip(ANSI_KEYS, ANSI))
    assert {key: term_bruno[key] for key in (
        "background", "foreground", "cursor", "cursorAccent", "selectionBackground"
    )} == {
        "background": "#1A1A1A", "foreground": "#CCCCCC", "cursor": "#E4AE49",
        "cursorAccent": "#1A1A1A", "selectionBackground": "#444444",
    }
    assert native_bruno["pal"] == ANSI
    assert native_bruno == {
        "fg": "#CCCCCC", "bg": "#1A1A1A", "cursor": "#E4AE49", "pal": ANSI,
        "bar": "#222224", "dim": "#AAAAAA", "text": "#CCCCCC",
        "brand": "#E4AE49", "line": "#333333", "line2": "#444444",
        "faint": "#999999",
    }


def test_bruno_dashboard_roles_are_exact_and_all_ids_are_documented():
    for role, color in BRUNO_ROLES.items():
        assert f"--{role}:{color}" in INDEX
    for theme in THEME_IDS:
        assert theme in DESIGN.lower()


def test_real_parent_theme_broadcast_skips_compat_and_missing_frames():
    broadcast = extract_js_function(INDEX, "broadcastTermTheme")
    result = run_node_json(f"""
const posts = [];
const styled = [];
const location = {{origin:"https://dash.test"}};
function styleTermFrame(frame) {{ styled.push(frame); }}
const openTerms = new Map([
  ["current", {{frame:{{dataset:{{compat:"0"}}, contentWindow:{{postMessage(message, origin){{posts.push(["current", message, origin]);}}}}}}}}],
  ["compat", {{frame:{{dataset:{{compat:"1"}}, contentWindow:{{postMessage(){{throw new Error("compat");}}}}}}}}],
  ["missing", {{frame:null}}],
  ["windowless", {{frame:{{dataset:{{compat:"0"}}}}}}],
]);
{broadcast}
broadcastTermTheme("bruno");
console.log(JSON.stringify({{posts, styled:styled.length}}));
""")
    assert result == {"posts": [["current", {
        "source": "comandos", "type": "theme", "theme": "bruno",
    }, "https://dash.test"]], "styled": 2}


def test_real_ensure_frame_keeps_identity_and_passes_initial_bruno_theme():
    ensure_frame = extract_js_function(INDEX, "ensureFrame")
    result = run_node_json(f"""
const created = [];
const appended = [];
const interactions = [];
const TERM_BASE = "/term";
const curTheme = "bruno";
const session = "dev / blue";
const entry = {{tab:{{}}, frame:null}};
const openTerms = new Map([[session, entry]]);
const area = {{appendChild(frame) {{ appended.push(frame); }}}};
const document = {{
  createElement(tag) {{
    const frame = {{
      tag, dataset:{{}}, className:"", src:"", removed:false,
      listeners:{{}}, classList:{{toggle() {{}}}},
      addEventListener(type, callback) {{ this.listeners[type] = callback; }},
      remove() {{ this.removed = true; }},
    }};
    created.push(frame);
    return frame;
  }},
  getElementById(id) {{ return id === "term-area" ? area : null; }},
}};
function styleTermFrame() {{}}
function wireTermFrameScroll() {{}}
function wireTermFrameShortcuts() {{}}
function applyTermInteraction(value) {{ interactions.push(value); }}
function resolveTermBase() {{ return Promise.resolve(TERM_BASE); }}
function toast(message) {{ throw new Error(message); }}
{ensure_frame}
(async () => {{
  ensureFrame(session);
  const initialFrame = entry.frame;
  await Promise.resolve();
  await Promise.resolve();
  ensureFrame(session);
  console.log(JSON.stringify({{
    created:created.length,
    appended:appended.length,
    sameEntryFrame:entry.frame === initialFrame,
    sameSessionEntry:openTerms.get(session) === entry,
    sameAppendedFrame:appended[0] === initialFrame,
    src:initialFrame.src,
    compat:initialFrame.dataset.compat,
    interactions,
    removed:initialFrame.removed,
  }}));
}})().catch(error => {{ console.error(error); process.exit(1); }});
""")
    assert result == {
        "created": 1,
        "appended": 1,
        "sameEntryFrame": True,
        "sameSessionEntry": True,
        "sameAppendedFrame": True,
        "src": "/term/?arg=dev%20%2F%20blue&theme=bruno",
        "compat": "0",
        "interactions": ["dev / blue"],
        "removed": False,
    }


def test_real_native_apply_theme_recolors_every_vte_gtk_provider_and_tmux_role():
    namespace = native_apply_namespace()

    class Provider:
        def __init__(self):
            self.loaded = []

        def load_from_data(self, data):
            self.loaded.append(data)

    class Terminal:
        def __init__(self):
            self.colors = []
            self.cursors = []

        def set_colors(self, foreground, background, palette):
            self.colors.append((foreground, background, palette))

        def set_color_cursor(self, cursor):
            self.cursors.append(cursor)

    class Page:
        def __init__(self, terminal):
            self._term = terminal

    class Notebook:
        def __init__(self, pages):
            self.pages = pages

        def get_n_pages(self):
            return len(self.pages)

        def get_nth_page(self, index):
            return self.pages[index]

    terminals = [Terminal(), Terminal()]
    providers = [Provider(), Provider(), Provider()]
    tmux_calls = []
    namespace.update({
        "THEME": namespace["THEMES"]["noche"],
        "_prov": providers[0],
        "_HB_CSS": providers[1],
        "_PANED_CSS": providers[2],
        "nb": Notebook([Page(terminal) for terminal in terminals]),
        "rgba": lambda color: color,
        "tmuxc": lambda *args: tmux_calls.append(args),
    })

    namespace["apply_theme"]("bruno")
    bruno = namespace["THEMES"]["bruno"]

    assert namespace["THEME"] == bruno
    for terminal in terminals:
        assert terminal.colors == [("#CCCCCC", "#1A1A1A", ANSI)]
        assert terminal.cursors == ["#E4AE49"]

    assert providers[0].loaded == [namespace["theme_css"](bruno).encode()]
    assert providers[1].loaded == [namespace["_build_hb_css"]()]
    assert b"#1A1A1A" in providers[0].loaded[0]
    assert b"#E4AE49" in providers[1].loaded[0]
    assert b"border-left:1px solid #333333" in providers[2].loaded[0]
    assert tmux_calls == [
        ("set-option", "-g", "status-style", "bg=#222224,fg=#AAAAAA"),
        ("set-option", "-g", "window-status-style", "bg=#222224,fg=#AAAAAA"),
        ("set-option", "-g", "window-status-current-style", "bg=#222224,fg=#CCCCCC,bold"),
        ("set-option", "-g", "window-status-activity-style", "bg=#222224,fg=#E4AE49,bold"),
        ("set-option", "-g", "pane-active-border-style", "#{?pane_in_mode,fg=yellow,fg=#E4AE49}"),
        ("set-option", "-g", "pane-border-style", "fg=#333333"),
        ("set-option", "-g", "message-style", "bg=#E4AE49,fg=#222224"),
    ]


def test_real_frame_style_replaces_important_colors_after_theme_change():
    style_frame = extract_js_function(INDEX, "styleTermFrame")
    result = run_node_json(f"""
let colors = {{bg:"#0A0D13", line2:"#2E3852", brand:"#8B7CFF"}};
const inserted = {{}};
const style = {{id:"", textContent:""}};
const doc = {{
  head: {{appendChild(node) {{ inserted[node.id] = node; }}}},
  getElementById(id) {{ return inserted[id] || null; }},
  createElement() {{ return style; }},
}};
const frame = {{dataset:{{compat:"0"}}, contentDocument:doc}};
const document = {{documentElement:{{dataset:{{theme:"noche"}}}}}};
function getComputedStyle() {{
  return {{getPropertyValue(name) {{
    return name === "--bg" ? colors.bg : name === "--line2" ? colors.line2 : colors.brand;
  }}}};
}}
{style_frame}
styleTermFrame(frame);
const first = style.textContent;
colors = {{bg:"#1A1A1A", line2:"#444444", brand:"#E4AE49"}};
styleTermFrame(frame);
const bruno = style.textContent;
document.documentElement.dataset.theme = "dia";
colors = {{bg:"#F7F8FB", line2:"#C3CAD8", brand:"#5B4BD6"}};
styleTermFrame(frame);
console.log(JSON.stringify({{first, bruno, day:style.textContent, styles:Object.keys(inserted).length}}));
""")
    assert result["styles"] == 1
    assert "#0A0D13!important" in result["first"]
    assert "color-scheme:dark" in result["first"]
    assert "#1A1A1A!important" in result["bruno"]
    assert "#444444" in result["bruno"]
    assert "#E4AE49" in result["bruno"]
    assert "#0A0D13" not in result["bruno"]
    assert "color-scheme:light" in result["day"]


def test_real_ligature_addon_survives_out_of_order_font_callbacks_during_rapid_themes():
    install = extract_js_function(TERM, "installLigatures")
    refresh = extract_js_function(TERM, "refreshLigatures")
    apply = extract_js_function(TERM, "applyTerminalTheme")
    themes = extract_js_initializer(TERM, "THEMES")
    addon_path = json.dumps(str(ROOT / "assets" / "xterm" / "addon-ligatures-web.js"))
    result = run_node_json(f"""
const delayedFonts = [];
const timers = new Map();
const animationFrames = [];
let nextTimer = 1;
const resizeHandlers = new Set();
const renderHandlers = new Set();
const children = [];
const context = {{
  setTransform() {{}}, clearRect() {{}}, fillRect() {{}}, beginPath() {{}},
  moveTo() {{}}, lineTo() {{}}, bezierCurveTo() {{}}, quadraticCurveTo() {{}},
  closePath() {{}}, fill() {{}},
}};
const host = {{
  clientWidth:800, clientHeight:480,
  appendChild(node) {{ node.parentNode = this; children.push(node); }},
  removeChild(node) {{
    const index = children.indexOf(node);
    if(index >= 0) children.splice(index, 1);
    node.parentNode = null;
  }},
}};
const shell = {{style:{{}}}};
const styles = {{}};
const document = {{
  documentElement: {{style:{{setProperty(name, value) {{ styles[name] = value; }}}}}},
  body: {{style:{{}}}},
  createElement(tag) {{
    if(tag !== "canvas") throw new Error(`unexpected element ${{tag}}`);
    return {{className:"", style:{{cssText:""}}, parentNode:null,
             getContext() {{ return context; }}}};
  }},
  getElementById(id) {{ return id === "term-shell" ? shell : null; }},
}};
const window = {{
  addEventListener(type, callback) {{ if(type === "resize") resizeHandlers.add(callback); }},
  removeEventListener(type, callback) {{ if(type === "resize") resizeHandlers.delete(callback); }},
}};
const browserGlobal = {{
  devicePixelRatio:1,
  opentype:{{load(_url, callback) {{ delayedFonts.push(callback); }}}},
  setTimeout(callback) {{ const id = nextTimer++; timers.set(id, callback); return id; }},
  clearTimeout(id) {{ timers.delete(id); }},
  requestAnimationFrame(callback) {{ animationFrames.push(callback); }},
}};
globalThis.document = document;
globalThis.window = window;
globalThis.self = browserGlobal;
const {{LigaturesWebAddon}} = require({addon_path});
const THEMES = {themes};
let activeTheme = "noche";
let ligatures = null;
const IS_TOUCH = false;
const debug = false;
const dbg = () => {{}};
const loaded = [];
const term = {{
  options:{{theme:THEMES.noche}}, cols:80, rows:24,
  element:{{querySelector(selector) {{ return selector === ".xterm-screen" ? host : null; }}}},
  buffer:{{active:{{viewportY:0, getLine() {{ return null; }}}}}},
  loadAddon(addon) {{ loaded.push(addon); addon.activate(this); }},
  onRender(callback) {{
    renderHandlers.add(callback);
    return {{dispose() {{ renderHandlers.delete(callback); }}}};
  }},
}};
let connectionCount = 0;
class WebSocket {{ constructor() {{ connectionCount += 1; this.id = "same-socket"; }} }}
const ws = new WebSocket();
const originalSocket = ws;
{install}
{refresh}
{apply}
installLigatures(THEMES.noche);
const nocheAddon = ligatures;
applyTerminalTheme("bruno");
const firstBrunoAddon = ligatures;
applyTerminalTheme("dia");
const diaAddon = ligatures;
applyTerminalTheme("bruno");
const finalBrunoAddon = ligatures;
const fakeFont = {{
  glyphs:{{length:0, get() {{ return null; }}}},
  tables:{{head:{{unitsPerEm:2048}}, hhea:{{ascender:1900, descender:-500}}}},
  unitsPerEm:2048,
}};
for(const index of [3, 0, 2, 1]) delayedFonts[index](null, fakeFont);
while(timers.size) {{
  const callbacks = [...timers.values()];
  timers.clear();
  callbacks.forEach(callback => callback());
}}
while(animationFrames.length) animationFrames.shift()();
console.log(JSON.stringify({{
  delayedCallbacks:delayedFonts.length,
  overlays:children.length,
  overlayClass:children[0]?.className,
  finalOwnsOverlay:finalBrunoAddon._overlay === children[0],
  oldOverlays:[nocheAddon, firstBrunoAddon, diaAddon].map(addon => addon._overlay),
  resizeHandlers:resizeHandlers.size,
  renderHandlers:renderHandlers.size,
  loadedAddons:loaded.length,
  liveAddons:loaded.filter(addon => addon._overlay !== null).length,
  activeTheme,
  finalBackground:term.options.theme.background,
  socketSame:ws === originalSocket,
  socketId:ws.id,
  connectionCount,
}}));
""")
    assert result["delayedCallbacks"] == 4
    assert result["overlays"] == 1
    assert result["overlayClass"] == "xterm-ligatures-overlay"
    assert result["finalOwnsOverlay"] is True
    assert result["oldOverlays"] == [None, None, None]
    assert result["resizeHandlers"] == 1
    assert result["renderHandlers"] == 1
    assert result["loadedAddons"] == 4
    assert result["liveAddons"] == 1
    assert result["activeTheme"] == "bruno"
    assert result["finalBackground"] == "#1A1A1A"
    assert result["socketSame"] is True
    assert result["socketId"] == "same-socket"
    assert result["connectionCount"] == 1


def test_paste_dialog_action_buttons_use_terminal_theme_roles():
    rule = re.search(r"#paste-dialog button\s*\{([^}]*)\}", TERM, re.S)
    assert rule
    assert "background:var(--term-panel2)" in rule.group(1)
    assert "border:1px solid var(--term-line2)" in rule.group(1)
    assert "color:var(--term-text)" in rule.group(1)


def test_real_iframe_theme_application_and_validation_preserve_connection_state():
    apply = extract_js_function(TERM, "applyTerminalTheme")
    handle = extract_js_function(TERM, "handleTerminalMessage")
    result = run_node_json(f"""
const THEMES = {{
  noche: {{background:"#0A0D13", foreground:"#EAF0FB", cursor:"#FFAE1A", panel:"#111720", panel2:"#182130", line:"#222A3A", line2:"#2E3852", dim:"#9AA6BF", faint:"#5E6980", brand:"#8B7CFF"}},
  bruno: {{background:"#1A1A1A", foreground:"#CCCCCC", cursor:"#E4AE49", panel:"#222224", panel2:"#26292B", line:"#333333", line2:"#444444", dim:"#AAAAAA", faint:"#999999", brand:"#E4AE49"}},
}};
let activeTheme = "noche";
const styles = {{}};
const shell = {{style:{{}}}};
const document = {{
  documentElement: {{style:{{setProperty(name, value){{styles[name] = value;}}}}}},
  body: {{style:{{}}}},
  getElementById(id) {{ return id === "term-shell" ? shell : null; }},
}};
const term = {{options:{{theme:THEMES.noche}}}};
const parent = {{}};
const location = {{origin:"https://dash.test"}};
const ws = {{id:"same-socket"}};
let connectionCount = 1;
let interactionState = {{known:true, busy:false, selecting:true}};
function refreshLigatures() {{}}
{apply}
{handle}
const before = {{ws, connectionCount, interactionState:{{...interactionState}}}};
const accepted = handleTerminalMessage({{origin:location.origin, source:parent, data:{{source:"comandos", type:"theme", theme:"bruno"}}}});
const after = {{ws, connectionCount, interactionState:{{...interactionState}}, theme:term.options.theme, styles, body:document.body.style.background, shell:shell.style.background}};
const badOrigin = handleTerminalMessage({{origin:"https://evil.test", source:parent, data:{{source:"comandos", type:"theme", theme:"noche"}}}});
const badSource = handleTerminalMessage({{origin:location.origin, source:{{}}, data:{{source:"comandos", type:"theme", theme:"noche"}}}});
const unknown = handleTerminalMessage({{origin:location.origin, source:parent, data:{{source:"comandos", type:"theme", theme:"unknown"}}}});
console.log(JSON.stringify({{accepted, beforeSocket:before.ws.id, afterSocket:after.ws.id, beforeCount:before.connectionCount, afterCount:after.connectionCount, beforeInteraction:before.interactionState, afterInteraction:after.interactionState, theme:after.theme, styles:after.styles, body:after.body, shell:after.shell, badOrigin, badSource, unknown, activeTheme}}));
""")
    assert result["accepted"] is True
    assert result["beforeSocket"] == result["afterSocket"] == "same-socket"
    assert result["beforeCount"] == result["afterCount"] == 1
    assert result["beforeInteraction"] == result["afterInteraction"] == {
        "known": True, "busy": False, "selecting": True,
    }
    assert result["theme"]["background"] == "#1A1A1A"
    assert result["styles"] == {
        "--term-bg": "#1A1A1A", "--term-panel": "#222224",
        "--term-panel2": "#26292B", "--term-line": "#333333",
        "--term-line2": "#444444", "--term-text": "#CCCCCC",
        "--term-dim": "#AAAAAA", "--term-faint": "#999999",
        "--term-brand": "#E4AE49",
    }
    assert result["body"] == result["shell"] == "#1A1A1A"
    assert result["badOrigin"] is result["badSource"] is result["unknown"] is False
    assert result["activeTheme"] == "bruno"
    assert "location.reload()" not in extract_js_function(TERM, "applyTerminalTheme")
