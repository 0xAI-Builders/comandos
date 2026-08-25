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


def test_all_session_states_render_as_uniform_rows():
    run_node(textwrap.dedent(f"""
        const assert = require("assert");
        {js_function("itemKind")}

        assert.equal(itemKind({{status: "waiting", agent: "codex"}}), "row");
        assert.equal(itemKind({{status: "done", agent: "codex", detail: "Final answer"}}), "row");
        assert.equal(itemKind({{status: "done", agent: "codex", last: "Final answer"}}), "row");
        assert.equal(itemKind({{status: "done", agent: "claude", detail: "Final answer"}}), "row");
        assert.equal(itemKind({{status: "working", agent: "codex"}}), "row");
    """))


def test_renderer_uses_one_row_container_and_inline_expansion():
    render = js_function("render")
    row = js_function("rowEl")

    assert "const container = rows;" in render
    assert "rowEl(it)" in render
    assert "ROW_ORDER = {waiting:-1" in HTML
    assert "style.order =" in render
    assert 'class="rxp"' in row
    assert 'class="xp hidden"' in row
    assert 'el.classList.toggle("open")' in row


def test_cards_and_urgent_section_are_removed():
    assert "cardCount" not in HTML
    assert "#urgent-wrap" not in HTML
    assert 'id="urgent-wrap"' not in HTML
    assert "cardEl" not in HTML


if __name__ == "__main__":
    test_all_session_states_render_as_uniform_rows()
    test_renderer_uses_one_row_container_and_inline_expansion()
    test_cards_and_urgent_section_are_removed()
