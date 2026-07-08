#!/usr/bin/env python3
import ast
import re
from pathlib import Path


SRC = Path("bin/cc-dash").read_text()


def load_remote_helpers():
    tree = ast.parse(SRC)
    funcs = {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }
    needed = ("remote_urls", "remote_status_from_text")
    missing = [name for name in needed if name not in funcs]
    assert not missing, f"missing helper(s): {', '.join(missing)}"

    ns = {"re": re}
    exec("\n\n".join(funcs[name] for name in needed), ns)
    return ns


def test_remote_urls_include_existing_access_token():
    ns = load_remote_helpers()

    urls = ns["remote_urls"]("zion.tail63a117.ts.net", "abc123")

    assert urls["dashboard"] == "https://zion.tail63a117.ts.net/?token=abc123"
    assert urls["terminal"] == "https://zion.tail63a117.ts.net/term?token=abc123"
    assert urls["terminalFallback"] == "https://zion.tail63a117.ts.net:8443/?token=abc123"


def test_remote_status_detects_dashboard_and_terminal_routes():
    ns = load_remote_helpers()
    serve = """
https://zion.tail63a117.ts.net (tailnet only)
|-- /     proxy http://127.0.0.1:4777
|-- /term proxy http://127.0.0.1:4780/term

https://zion.tail63a117.ts.net:8443 (tailnet only)
|-- / proxy http://127.0.0.1:4779
"""

    status = ns["remote_status_from_text"](serve, True)

    assert status["remoteOn"] is True
    assert status["webtermOn"] is True
    assert status["termRouteOn"] is True
    assert status["fallbackRouteOn"] is True


if __name__ == "__main__":
    test_remote_urls_include_existing_access_token()
    test_remote_status_detects_dashboard_and_terminal_routes()
