"""Desktop Resúmenes: the reader sits beside the real VTE terminals, never
over the sidebar; with Ver terminal the terminal stays on the left."""
import ast
from pathlib import Path

SOURCE = Path(__file__).resolve().parents[1].joinpath("bin", "cc-app").read_text()


def load():
    nodes = [n for n in ast.parse(SOURCE).body if isinstance(n, ast.FunctionDef) and n.name == "reader_layout_state"]
    ns = {}
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    return ns["reader_layout_state"]


def test_reader_layout_keeps_terminals_left_and_sidebar_untouched():
    layout = load()
    assert layout(open=False, terminal=False) == {"terminals": True, "reader": False}
    assert layout(open=True, terminal=False) == {"terminals": False, "reader": True}
    assert layout(open=True, terminal=True) == {"terminals": True, "reader": True}
    assert layout(open=False, terminal=True) == {"terminals": True, "reader": False}


def test_header_has_a_resumenes_button_and_the_bridge_handles_reader_messages():
    assert '_icon_btn("sparkles"' in SOURCE
    assert 'd.get("type") == "reader"' in SOURCE
    assert "panel=news" in SOURCE
