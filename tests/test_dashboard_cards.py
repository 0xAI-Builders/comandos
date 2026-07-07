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


def test_codex_done_with_detail_renders_as_actionable_card():
    run_node(textwrap.dedent(f"""
        const assert = require("assert");
        {js_function("itemKind")}

        assert.equal(itemKind({{status: "waiting", agent: "codex"}}), "card");
        assert.equal(itemKind({{status: "done", agent: "codex", detail: "Final answer"}}), "card");
        assert.equal(itemKind({{status: "done", agent: "codex", last: "Final answer"}}), "card");
        assert.equal(itemKind({{status: "done", agent: "claude", detail: "Final answer"}}), "row");
        assert.equal(itemKind({{status: "working", agent: "codex"}}), "row");
    """))


def test_card_container_visibility_uses_rendered_card_count_not_only_waiting():
    assert 'let cardCount = 0;' in HTML
    assert 'cardCount++' in HTML
    assert '$("#urgent-wrap").classList.toggle("hidden", cardCount === 0)' in HTML


if __name__ == "__main__":
    test_codex_done_with_detail_renders_as_actionable_card()
    test_card_container_visibility_uses_rendered_card_count_not_only_waiting()
