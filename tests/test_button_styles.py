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
    for bid, icon in (("btn-usage", "database"), ("btn-remote", "smartphone"), ("btn-settings", "settings")):
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
