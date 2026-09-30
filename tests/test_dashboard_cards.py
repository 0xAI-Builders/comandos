#!/usr/bin/env python3
import re
import subprocess
import textwrap
from pathlib import Path


HTML = Path("dash/index.html").read_text()


def js_function(name: str) -> str:
    match = re.search(rf"function {re.escape(name)}\([^)]*\)\{{", HTML)
    assert match, f"missing JS function: {name}"
    start = match.start()
    depth = 0
    for pos in range(match.end() - 1, len(HTML)):
        ch = HTML[pos]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return HTML[start:pos + 1]
    raise AssertionError(f"unterminated JS function: {name}")


def run_node(script: str):
    subprocess.run(["node", "-e", script], check=True, text=True)


def test_session_rows_left_the_sidebar():
    # S2: the left panel is the command sidebar; the session rows (and their
    # renderer) are gone. render() keeps counters and the tab bar refresh.
    assert 'id="rows"' not in HTML
    for gone in ("function rowEl(", "function itemKind(", "ROW_ORDER", "function wireActions("):
        assert gone not in HTML, gone
    render = js_function("render")
    assert "counts.waiting" in render and "renderTabbar();" in render


def test_cards_and_urgent_section_are_removed():
    assert "cardCount" not in HTML
    assert "#urgent-wrap" not in HTML
    assert 'id="urgent-wrap"' not in HTML
    # el renderer legacy de cards no existe; "cardEl" suelto como nombre de
    # variable local (p.ej. en notificaciones) no es el renderer
    assert "function cardEl" not in HTML
    assert "const cardEl" not in HTML


if __name__ == "__main__":
    test_session_rows_left_the_sidebar()
    test_cards_and_urgent_section_are_removed()
