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


def test_bruno_registry_palette_and_roles_are_exact_on_every_surface():
    for source in (APP, DASH, INDEX, TERM, DESIGN):
        assert "bruno" in source.lower()
    palette = APP.split("PAL_BRUNO = [", 1)[1].split("]", 1)[0]
    assert re.findall(r'#[0-9A-F]{6}', palette) == ANSI
    assert '"bruno": dict(' in APP
    for color in ANSI:
        assert color in TERM
    for role, color in BRUNO_ROLES.items():
        assert f"--{role}:{color}" in INDEX
    assert re.search(r"foreground:\s*['\"]#CCCCCC", TERM)
    assert re.search(r"background:\s*['\"]#1A1A1A", TERM)
    assert re.search(r"cursor:\s*['\"]#E4AE49", TERM)


def test_all_five_theme_ids_are_accepted_and_documented():
    for theme in ("noche", "dia", "calido", "termius", "bruno"):
        assert theme in DESIGN.lower()
        assert theme in DASH
        assert theme in INDEX
        assert theme in TERM
    assert '("noche", "dia", "calido", "termius", "bruno")' in DASH
    assert 'const THEME_SEQ = ["noche", "dia", "calido", "termius", "bruno"]' in INDEX
    assert 'bruno:"zap"' in INDEX


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


def test_live_theme_recreates_desktop_ligature_overlay_without_reconnecting():
    install = extract_js_function(TERM, "installLigatures")
    refresh = extract_js_function(TERM, "refreshLigatures")
    apply = extract_js_function(TERM, "applyTerminalTheme")
    result = run_node_json(f"""
const THEMES = {{
  noche: {{background:"#0A0D13", foreground:"#EAF0FB", cursor:"#FFAE1A", panel:"#121722", panel2:"#161C29", line:"#222A3A", line2:"#2E3852", dim:"#9AA6BF", faint:"#5E6980", brand:"#8B7CFF"}},
  bruno: {{background:"#1A1A1A", foreground:"#CCCCCC", cursor:"#E4AE49", panel:"#222224", panel2:"#26292B", line:"#333333", line2:"#444444", dim:"#AAAAAA", faint:"#999999", brand:"#E4AE49"}},
}};
let activeTheme = "noche";
const created = [];
let disposed = 0;
class LigaturesWebAddon {{
  constructor(options) {{ this.options = options; created.push(options); }}
  dispose() {{ disposed += 1; }}
}}
const oldOverlay = {{dispose() {{ disposed += 1; }}}};
let ligatures = oldOverlay;
const IS_TOUCH = false;
const debug = false;
const dbg = () => {{}};
const term = {{options:{{theme:THEMES.noche}}, loadAddon() {{}}}};
const styles = {{}};
const shell = {{style:{{}}}};
const rootStyle = {{setProperty(name, value) {{ styles[name] = value; }}}};
const document = {{
  documentElement: {{style:rootStyle}},
  body: {{style:{{}}}},
  getElementById(id) {{ return id === "term-shell" ? shell : null; }},
}};
const ws = {{id:"same-socket"}};
let connectionCount = 1;
{install}
{refresh}
{apply}
applyTerminalTheme("bruno");
console.log(JSON.stringify({{disposed, created, activeTheme, ws:ws.id, connectionCount, theme:term.options.theme}}));
""")
    assert result["disposed"] == 1
    assert result["created"] == [{
        "fontUrl": "../assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-Regular.ttf",
        "fontSize": 14,
        "foreground": "#CCCCCC",
        "background": "#1A1A1A",
        "debug": False,
    }]
    assert result["activeTheme"] == "bruno"
    assert result["theme"]["background"] == "#1A1A1A"
    assert result["ws"] == "same-socket"
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
