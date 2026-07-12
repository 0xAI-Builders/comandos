#!/usr/bin/env python3
import ast
import re
import secrets
import time
from pathlib import Path

SRC = Path("bin/cc-dash").read_text()


def load_functions(*names, extra=None):
    tree = ast.parse(SRC)
    funcs = {
        node.name: ast.get_source_segment(SRC, node)
        for node in tree.body
        if isinstance(node, ast.FunctionDef)
    }
    missing = [n for n in names if n not in funcs]
    assert not missing, f"missing helper(s): {', '.join(missing)}"
    ns = dict(extra or {})
    exec("\n\n".join(funcs[n] for n in names), ns)
    return ns


def test_snippet_validate_accepts_normal_input():
    ns = load_functions("snippet_validate")
    assert ns["snippet_validate"]("docker ps", "docker ps -a", ["docker"]) is None


def test_snippet_validate_rejects_empty_name():
    ns = load_functions("snippet_validate")
    assert "name" in ns["snippet_validate"]("", "body", []).lower()


def test_snippet_validate_rejects_long_name():
    ns = load_functions("snippet_validate")
    assert "name" in ns["snippet_validate"]("x" * 81, "body", []).lower()


def test_snippet_validate_rejects_empty_body():
    ns = load_functions("snippet_validate")
    assert "body" in ns["snippet_validate"]("n", "", []).lower()


def test_snippet_validate_rejects_huge_body():
    ns = load_functions("snippet_validate")
    err = ns["snippet_validate"]("n", "x" * 20001, [])
    assert "body" in err.lower()


def test_snippet_validate_rejects_too_many_tags():
    ns = load_functions("snippet_validate")
    err = ns["snippet_validate"]("n", "b", [f"t{i}" for i in range(11)])
    assert "tag" in err.lower()


def test_snippet_validate_rejects_long_tag():
    ns = load_functions("snippet_validate")
    err = ns["snippet_validate"]("n", "b", ["x" * 33])
    assert "tag" in err.lower()


def test_snippet_validate_rejects_non_list_tags():
    ns = load_functions("snippet_validate")
    err = ns["snippet_validate"]("n", "b", "docker,git")
    assert "tag" in err.lower()


def test_snippet_new_produces_valid_shape():
    ns = load_functions("snippet_new", extra={"secrets": secrets, "time": time})
    item = ns["snippet_new"]("docker ps", "docker ps -a", ["docker"])
    assert set(item.keys()) == {"id", "name", "body", "tags", "updated_at"}
    assert re.match(r"^[a-f0-9]{16}$", item["id"])
    assert item["name"] == "docker ps"
    assert item["body"] == "docker ps -a"
    assert item["tags"] == ["docker"]
    assert item["updated_at"] <= int(time.time()) + 1


def test_snippet_update_replaces_fields_and_bumps_ts():
    ns = load_functions("snippet_update", extra={"time": time})
    items = [{"id": "aaaa000000000000", "name": "old", "body": "x",
              "tags": [], "updated_at": 1}]
    new_items, updated = ns["snippet_update"](items, "aaaa000000000000",
                                              "new", "y", ["t"])
    assert updated is not None
    assert updated["name"] == "new"
    assert updated["body"] == "y"
    assert updated["tags"] == ["t"]
    assert updated["updated_at"] > 1
    assert new_items[0] == updated


def test_snippet_update_returns_none_for_missing_id():
    ns = load_functions("snippet_update", extra={"time": time})
    _, updated = ns["snippet_update"]([], "0" * 16, "n", "b", [])
    assert updated is None


def test_snippet_delete_removes_matching_id():
    ns = load_functions("snippet_delete")
    items = [{"id": "aaaa000000000000", "name": "a", "body": "x", "tags": [], "updated_at": 1},
             {"id": "bbbb000000000000", "name": "b", "body": "y", "tags": [], "updated_at": 2}]
    new_items, removed = ns["snippet_delete"](items, "aaaa000000000000")
    assert removed is True
    assert len(new_items) == 1
    assert new_items[0]["id"] == "bbbb000000000000"


def test_snippet_delete_reports_missing():
    ns = load_functions("snippet_delete")
    _, removed = ns["snippet_delete"]([], "0" * 16)
    assert removed is False


def test_read_snippets_drops_malformed(tmp_path, monkeypatch):
    f = tmp_path / "snippets.json"
    f.write_text('[{"id":"aaaa000000000000","name":"ok","body":"x","tags":[],"updated_at":1},'
                 '{"id":"bad","name":"skipped","body":"x","tags":[],"updated_at":1},'
                 '"not a dict",'
                 '{"id":"cccc000000000000","name":"","body":"x","tags":[],"updated_at":1}]')
    ns = load_functions("read_snippets", "snippet_validate", extra={
        "SNIPPETS_FILE": str(f),
        "load_json_file": lambda p, d: __import__("json").load(open(p)) if __import__("os").path.exists(p) else d,
        "SNIPPET_ID_RE": re.compile(r"^[a-f0-9]{16}$"),
    })
    items = ns["read_snippets"]()
    assert [i["id"] for i in items] == ["aaaa000000000000"]
