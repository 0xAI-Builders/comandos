#!/usr/bin/env python3
import ast
import re
from pathlib import Path


SRC = Path("bin/cc-app").read_text()


def load_url_helpers():
    tree = ast.parse(SRC)
    funcs = {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }
    needed = ("normalize_wrapped_url_text", "url_from_wrapped_text")
    missing = [name for name in needed if name not in funcs]
    assert not missing, f"missing helper(s): {', '.join(missing)}"

    url_re = re.search(r'^URL_RE\s*=\s*(.+)$', SRC, re.M)
    assert url_re, "missing URL_RE"
    ns = {"re": re}
    exec(f"URL_RE = {url_re.group(1)}\n" + "\n\n".join(funcs[name] for name in needed), ns)
    return ns


def test_wrapped_url_text_is_joined_before_opening():
    ns = load_url_helpers()
    text = "abre https://example.com/a/very/\nlong/path?x=1&y=2 ahora"

    assert ns["normalize_wrapped_url_text"](text) == (
        "abre https://example.com/a/very/long/path?x=1&y=2 ahora"
    )


def test_wrapped_url_can_be_selected_from_first_or_second_visual_line():
    ns = load_url_helpers()
    text = "abre https://example.com/a/very/\nlong/path?x=1&y=2 ahora"
    full = "https://example.com/a/very/long/path?x=1&y=2"

    assert ns["url_from_wrapped_text"](text, "https://example.com/a/very/") == full
    assert ns["url_from_wrapped_text"](text, "long/path?x=1&y=2") == full
    assert ns["url_from_wrapped_text"](text, "") == full


if __name__ == "__main__":
    test_wrapped_url_text_is_joined_before_opening()
    test_wrapped_url_can_be_selected_from_first_or_second_visual_line()
