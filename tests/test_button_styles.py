"""Grill 29-sep: header buttons share one size and a 3D style chosen in
Ajustes → Apariencia (sutil by default; arcade, tecla, pixel, consola)."""
import ast
import importlib.machinery
import importlib.util
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
STYLES = ("sutil", "arcade", "tecla", "pixel", "consola")
INDEX = (ROOT / "dash" / "index.html").read_text()
CSS = (ROOT / "dash" / "workspace.css").read_text()
APP = (ROOT / "bin" / "cc-app").read_text()


@pytest.fixture(scope="module")
def dash():
    sys.path.insert(0, str(ROOT / "bin"))
    loader = importlib.machinery.SourceFileLoader("cc_dash_buttons", str(ROOT / "bin" / "cc-dash"))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


def test_button_style_pref_defaults_to_sutil_and_only_accepts_known_styles(dash, tmp_path, monkeypatch):
    monkeypatch.setattr(dash, "PREFS_PATH", str(tmp_path / "prefs.json"))
    assert dash.read_prefs()["button_style"] == "sutil"
    for style in STYLES:
        dash.update_prefs({"button_style": style})
        assert dash.read_prefs()["button_style"] == style
    dash.update_prefs({"button_style": "<script>"})
    assert dash.read_prefs()["button_style"] == "consola"


def test_web_offers_every_style_and_header_buttons_are_icons_of_one_size():
    assert 'id="button-style-gallery"' in INDEX
    for style in STYLES:
        assert f'"{style}"' in INDEX
        assert f'html[data-btn-style="{style}"]' in CSS or style == "sutil"
    for bid, icon in (("btn-usage", "database"), ("btn-remote", "smartphone"), ("btn-settings", "settings"),
                      ("btn-servers", "server"), ("btn-terminal", "terminal"), ("btn-newsess", "plus"), ("btn-menu", "menu")):
        tag = INDEX.split(f'id="{bid}"')[1].split("</button>")[0]
        assert f'data-icon="{icon}"' in tag and 'class="hdr-lbl"' in tag
    assert (ROOT / "dash" / "icons" / "smartphone.svg").is_file()
    assert "--hdr-key" in CSS


def test_desktop_header_css_exists_for_every_style_and_presses_down():
    nodes = [n for n in ast.parse(APP).body if isinstance(n, ast.FunctionDef) and n.name == "button_style_css"]
    assert nodes, "cc-app defines button_style_css"
    ns = {}
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    theme = {"bg": "#0A0D13", "fg": "#E6EDF5", "line": "#2E3852", "panel": "#101824", "accent": "#8B7CFF"}
    outs = {s: ns["button_style_css"](s, theme) for s in STYLES}
    assert len(set(outs.values())) == len(STYLES), "each style looks different"
    assert all(b":active" in css and b"min-width" in css for css in outs.values())
    assert ns["button_style_css"]("nope", theme) == outs["sutil"]


BUTTONS_CSS = ROOT / "dash" / "buttons.css"
TERM = (ROOT / "dash" / "term.html").read_text()
EXT_HTML = (ROOT / "dash" / "extensions.html").read_text()
EXT_JS = (ROOT / "dash" / "extensions.js").read_text()


def test_every_web_button_gets_the_style_on_every_page():
    css = BUTTONS_CSS.read_text()
    assert ":is(button,input[type=button],input[type=submit],input[type=reset])" in css
    for style in STYLES:
        assert f'[data-btn-style="{style}"]' in css
    assert ":active" in css and ":disabled" in css
    for page in (INDEX, TERM, EXT_HTML):
        assert "buttons.css" in page, "every page loads the shared button layer"
    assert "button-style" in TERM and "button-style" in INDEX, "the terminal iframe follows the chosen style"
    assert "button_style" in EXT_JS, "the extensions page reads the shared pref"


def test_desktop_styles_every_button_but_window_controls():
    nodes = [n for n in ast.parse(APP).body if isinstance(n, ast.FunctionDef) and n.name == "button_style_css"]
    ns = {}
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    theme = {"bg": "#0A0D13", "fg": "#E6EDF5", "line": "#2E3852", "line2": "#3a4560", "bar": "#101824", "text": "#fff", "dim": "#999"}
    for style in STYLES:
        css = ns["button_style_css"](style, theme)
        assert b"button:not(.cc-key):not(.cc-winctl)" in css
    assert 'add_class("cc-winctl")' in APP


def test_ai_sprites_pref_defaults_to_the_comandos_bot_and_only_accepts_shipped_sets(dash, tmp_path, monkeypatch):
    """Grill 30-sep: the tab semáforo character is chosen in Ajustes → Apariencia."""
    monkeypatch.setattr(dash, "PREFS_PATH", str(tmp_path / "prefs.json"))
    assert dash.read_prefs()["ai_sprites"] == "comandos"
    dash.update_prefs({"ai_sprites": "kit"})
    assert dash.read_prefs()["ai_sprites"] == "kit"
    dash.update_prefs({"ai_sprites": "../etc"})
    assert dash.read_prefs()["ai_sprites"] == "kit"
    assert 'id="ai-set-gallery"' in INDEX and "applyAiSet(p.ai_sprites" in INDEX
    app = (ROOT / "bin" / "cc-app").read_text()
    assert 'd.get("aiSprites") in work_mark_state.AI_SETS' in app and "def apply_ai_sprites" in app


def test_desktop_header_actions_live_in_the_web_header_like_the_remote():
    """Fix 2 (grill 30-sep): the GTK window bar keeps brand, ‹ › and window controls;
    Terminal, Nueva sesión, Ordenar, Analytics, Remoto, Resúmenes, campana, Pomodoro
    and Ajustes are the same web buttons as in the remote, left of the terminals."""
    app = (ROOT / "bin" / "cc-app").read_text()
    for widget in ("_settings_btn", "_notif_wrap", "_news_btn", "_pomo_btn", "_plus", "_quick_term_btn", "_sort_btn"):
        assert f"_headerbar.pack_end({widget})" not in app, widget
    for widget in ("_close", "_maximize", "_minimize"):
        assert f"_headerbar.pack_end({widget})" in app, widget
    assert 'HEADER_ACTIONS = {"quickTerminal": open_quick_terminal, "newSession": _open_wizard,' in app
    assert 'd.get("headerAction") in HEADER_ACTIONS' in app
    assert "body.inapp .hdr-primary{display:none}" not in INDEX
    assert "body.inapp header.hdr-ordered :is(#btn-terminal,#btn-newsess){display:none}" not in CSS
    # Prototipo aprobado: Terminal · Nueva sesión · Ordenar a la derecha de la barra de pestañas GTK;
    # en la app la cabecera web es la rejilla de 4 columnas sin ☰, contadores ni búsqueda.
    assert "nb.set_action_widget(_strip_actions, Gtk.PackType.END)" in app
    assert "for _b in (_quick_term_btn, _plus, _sort_btn, _tab_next):" in app
    assert "body.inapp header.hdr-ordered{display:grid;grid-template-columns:repeat(3,minmax(0,1fr))" in CSS
    assert "body.inapp header.hdr-ordered :is(#btn-menu,.counts,#btn-terminal,#btn-newsess,#btn-switch,#btn-snippets){display:none!important}" in CSS
    assert 'id="btn-sort"' not in INDEX
    for action in ("quickTerminal", "newSession"):
        assert f'toApp("{action}")' in INDEX


def test_desktop_bell_opens_the_notices_shelf_under_the_terminals():
    """30-sep: on the desktop the bell opens the notices drawer in the shelf under the
    terminals (like MCPs · Skills), through index.html?panel=notices, not in the sidebar."""
    app = (ROOT / "bin" / "cc-app").read_text()
    assert '"notices": lambda *_: _toggle_notices_shelf()' in app and "def _toggle_notices_shelf" in app
    assert 'view.load_uri(f"{BASE_URL}/?panel=notices&app=1&v={_DASH_V}")' in app
    assert "_shelf_paned.pack2(_shelf_box, False, False)" in app
    assert 'toApp("notices")' in INDEX and 'if(ONLY_PANEL==="notices") return;' in INDEX
    assert 'html[data-only-panel="notices"] body.only-panel #notices{display:flex!important' in INDEX
    assert 'html[data-only-panel="notices"] body.only-panel #notices .nt-body{flex:1 1 auto;height:auto!important' in INDEX, 'fills the shelf'
