#!/usr/bin/env python3
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import operator_catalog as cat  # noqa: E402

DASH = (ROOT / "bin" / "cc-dash").read_text()
HTML = (ROOT / "dash" / "index.html").read_text()
HTML += (ROOT / "dash" / "workspace.js").read_text()
HTML += (ROOT / "dash" / "pomodoro.js").read_text()
TERM = (ROOT / "dash" / "term.html").read_text()
APP = (ROOT / "bin" / "cc-app").read_text()


def test_catalog_names_are_unique_snake_case():
    names = [t.name for t in cat.CATALOG]
    assert len(names) == len(set(names))
    for n in names:
        assert re.fullmatch(r"[a-z][a-z0-9_]{2,48}", n), n


def test_every_tool_has_group_description_and_valid_target():
    kinds = {"api", "ui", "app", "local"}
    for t in cat.CATALOG:
        assert t.group in cat.GROUPS, t.name
        assert len(t.description) >= 12, t.name
        assert t.target["kind"] in kinds, t.name
        for r in t.required:
            assert r in t.params, (t.name, r)
        if t.destructive:
            assert "confirm" in t.params and "confirm" in t.required, t.name


def test_api_targets_point_to_real_cc_dash_paths():
    for t in cat.CATALOG:
        if t.target["kind"] != "api":
            continue
        path = t.target["path"]
        assert f'"{path}"' in DASH, (t.name, path)


def test_no_tool_targets_the_retired_operator_routes():
    # S4: /operator* answers 410; set_chat_model and operator_action_results left.
    for t in cat.CATALOG:
        assert not str(t.target.get("path", "")).startswith("/operator"), t.name


def test_ui_targets_point_to_real_selectors_or_functions():
    for t in cat.CATALOG:
        if t.target["kind"] != "ui":
            continue
        op = t.target["op"]
        if op == "click":
            sel = t.target["selector"]
            key = sel.lstrip("#.").split("[")[0]
            assert key in HTML, (t.name, sel)
        elif op == "call":
            fn = t.target["fn"]
            assert f"function {fn}(" in HTML or f"window.{fn} = " in HTML, (t.name, fn)
        elif op == "term":
            assert t.target["type"] in ("toolbar", "paste", "mode", "ctrl"), t.name
        else:
            raise AssertionError((t.name, op))


def test_app_targets_are_handled_by_cc_app():
    for t in cat.CATALOG:
        if t.target["kind"] != "app":
            continue
        assert f'"{t.target["command"]}":' in APP, (t.name, t.target["command"])


def test_schemas_generate_for_both_providers():
    a = cat.anthropic_tools()
    o = cat.openai_tools()
    assert len(a) == len(o) == len(cat.CATALOG)
    json.dumps(a)
    json.dumps(o)
    assert all(x["input_schema"]["type"] == "object" for x in a)
    assert all(x["function"]["parameters"]["type"] == "object" for x in o)


def test_minimum_coverage_per_group():
    counts = {}
    for t in cat.CATALOG:
        counts[t.group] = counts.get(t.group, 0) + 1
    floor = {"sessions": 20, "panes": 10, "models": 18, "usage": 16, "pomodoro": 7,
             "prefs": 18, "notifs": 6, "remote": 12, "snippets": 5, "fs": 4,
             "news": 4, "nav": 12, "term": 5, "app": 26, "chat": 5}   # nav 14→13: S2 quitó las filas de sesión y la actividad reciente; 13→12: Analytics quitó compare_set_window; chat 6→5: S4 retiró /operator
    for g, n in floor.items():
        assert counts.get(g, 0) >= n, (g, counts.get(g, 0))


def test_operator_actions_doc_lists_every_tool():
    doc = (ROOT / "docs" / "operator-actions.md").read_text()
    for t in cat.CATALOG:
        assert f"`{t.name}`" in doc, t.name


def test_open_panel_offers_no_panel_whose_ui_is_gone():
    # S2 quitó la actividad reciente (timeline) y el centro del tablero.
    spec = cat.by_name("open_panel")
    panels = spec.params["panel"]["enum"]
    assert "timeline" not in panels and "centro" not in panels
    assert "timeline" not in spec.description and "centro" not in spec.description
    assert panels == ["analytics", "sov", "switcher", "wizard"]
