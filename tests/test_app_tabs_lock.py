"""app-tabs.json se escribe atomico y bajo el mismo candado que cc-dash
(<archivo>.lock, flock exclusivo)."""
import ast
import contextlib
import fcntl
import json
import os
import tempfile
import threading
from pathlib import Path

import pytest

SOURCE = Path(__file__).resolve().parents[1].joinpath("bin", "cc-app").read_text()


def load(names, ns):
    tree = ast.parse(SOURCE)
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in names]
    assert len(nodes) == len(names), names
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<app>", "exec"), ns)
    return ns


def test_file_lock_uses_sibling_lock_file_exclusively(tmp_path):
    ns = load(["_file_lock"], {"_fcntl": fcntl, "contextlib": contextlib, "os": os})
    target = tmp_path / "app-tabs.json"
    with ns["_file_lock"](str(target)):
        other = open(str(target) + ".lock", "a")
        with pytest.raises(BlockingIOError):
            fcntl.flock(other, fcntl.LOCK_EX | fcntl.LOCK_NB)
        other.close()
    other = open(str(target) + ".lock", "a")
    fcntl.flock(other, fcntl.LOCK_EX | fcntl.LOCK_NB)
    other.close()


def test_save_tabs_writes_atomically_under_lock(tmp_path):
    held = []

    @contextlib.contextmanager
    def fake_lock(path):
        held.append(path)
        yield

    target = tmp_path / "app-tabs.json"
    ns = {"_TAB_REORDERING": False, "enforce_tab_order": lambda: None, "_LAYOUT_RESTORE_READY": True,
          "tabs": {"term-1": type("B", (), {"_label": "uno"})()}, "TABS_FILE": str(target),
          "tempfile": tempfile, "os": os, "json": json, "_file_lock": fake_lock,
          "ordered_tab_labels": lambda order, labels: labels, "current_tab_order": lambda: ["term-1"]}
    load(["save_tabs"], ns)["save_tabs"]()
    assert held == [str(target)]
    assert json.loads(target.read_text()) == {"term-1": "uno"}
    assert [p.name for p in tmp_path.iterdir()] == ["app-tabs.json"]


def test_tab_history_is_written_under_lock_without_fixed_tmp_name():
    fn = next(n for n in ast.parse(SOURCE).body if isinstance(n, ast.FunctionDef) and n.name == "archive_tab")
    text = ast.get_source_segment(SOURCE, fn)
    assert "_file_lock(TAB_HISTORY_FILE)" in text
    assert 'TAB_HISTORY_FILE + ".tmp"' not in text
