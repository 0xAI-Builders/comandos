#!/usr/bin/env python3
"""Robustez de cc-dash: validadores, escrituras atomicas y ruta de peticiones.
Todo corre contra tmp dirs y monkeypatch; nunca contra ~/.claude ni tmux real."""
import importlib.machinery
import importlib.util
import sys
from pathlib import Path

import pytest


@pytest.fixture(scope="module")
def dash():
    bin_dir = str(Path("bin").resolve())
    if bin_dir not in sys.path:
        sys.path.insert(0, bin_dir)
    loader = importlib.machinery.SourceFileLoader(
        "cc_dash_hardening_under_test", str(Path("bin/cc-dash").resolve()))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


@pytest.mark.parametrize("name,value", [
    ("SESSION_RE", "demo"),
    ("PANE_RE", "%12"),
    ("SNIPPET_ID_RE", "0123456789abcdef"),
    ("SSH_HOST_RE", "prod"),
    ("SSH_HOSTNAME_RE", "203.0.113.10"),
    ("SSH_USER_RE", "root"),
    ("SSH_PATH_RE", "~/.ssh/id_ed25519"),
])
def test_validators_reject_trailing_newline_even_with_match(dash, name, value):
    pattern = getattr(dash, name)
    assert pattern.match(value)
    # `$` acepta un "\n" final: con .match() eso colaba "demo\n" como sesion.
    assert not pattern.match(value + "\n")
    assert not pattern.fullmatch(value + "\n")
