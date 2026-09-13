#!/usr/bin/env python3
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import operator_catalog as cat  # noqa: E402
import operator_dispatch as od  # noqa: E402


def test_substitute_replaces_and_drops_missing():
    body = {"session": "$session", "pane": "$pane", "literal": True, "nested": {"a": "$a"}}
    assert od.substitute(body, {"session": "s1", "a": 3}) == {"session": "s1", "literal": True, "nested": {"a": 3}}


def _dispatcher(tmp_path, posts, gets, local=None):
    def post(path, body):
        posts.append((path, body)); return {"ok": True, "echo": body}
    def get(path, query):
        gets.append((path, query)); return [{"session": "a", "label": "A"}]
    return od.Dispatcher(base_url="http://127.0.0.1:1", token="t", hooks_dir=str(tmp_path),
                         local_handlers=local or {}, http_post=post, http_get=get)


def test_api_post_target_builds_body_from_args(tmp_path):
    posts, gets = [], []
    out = _dispatcher(tmp_path, posts, gets).run("paste_text", {"session": "s1", "text": "hola"})
    assert out["ok"] and posts == [("/paste", {"session": "s1", "text": "hola"})]


def test_api_get_target_builds_query(tmp_path):
    posts, gets = [], []
    out = _dispatcher(tmp_path, posts, gets).run("tmux_mouse_get", {"session": "s1"})
    assert gets == [("/tmux-mouse", {"session": "s1"})] and out["data"]


def test_destructive_requires_confirm(tmp_path):
    posts, gets = [], []
    d = _dispatcher(tmp_path, posts, gets)
    out = d.run("ssh_delete", {"host": "x"})
    assert out["ok"] is False and "confirm" in out["reply"] and posts == []
    assert d.run("ssh_delete", {"host": "x", "confirm": True})["ok"] and posts == [("/ssh-del", {"host": "x"})]


def test_ui_targets_return_browser_actions(tmp_path):
    d = _dispatcher(tmp_path, [], [])
    assert d.run("open_pomodoro", {})["actions"] == [{"type": "ui", "op": "click", "selector": "#btn-pomo"}]
    assert d.run("show_view", {"view": "term:x"})["actions"] == [{"type": "ui", "op": "call", "fn": "showView", "args": ["term:x"]}]
    assert d.run("term_key", {"key": "enter", "session": "x"})["actions"] == [{"type": "ui", "op": "term", "term": {"type": "toolbar", "key": "enter", "session": "x"}}]


def test_app_target_writes_command_file(tmp_path):
    out = _dispatcher(tmp_path, [], []).run("app_mosaic", {"state": "on"})
    data = json.loads((tmp_path / "app-command.json").read_text())
    assert out["ok"] and data["command"] == "mosaic" and data["args"] == {"state": "on"} and data["ts"] > 0


def test_local_target_calls_handler(tmp_path):
    seen = []
    d = _dispatcher(tmp_path, [], [], local={"focus": lambda a: (seen.append(a) or {"ok": True, "reply": "Foco en " + a["tab"]})})
    assert d.run("focus_tab", {"tab": "Signara"})["reply"] == "Foco en Signara" and seen == [{"tab": "Signara"}]


def test_unknown_tool_and_missing_required(tmp_path):
    d = _dispatcher(tmp_path, [], [])
    assert d.run("nope", {})["ok"] is False
    out = d.run("paste_text", {"session": "s"})
    assert out["ok"] is False and "text" in out["reply"]


def test_every_local_intent_in_catalog_is_declared():
    intents = {t.target["intent"] for t in cat.CATALOG if t.target["kind"] == "local"}
    assert intents <= set(od.LOCAL_INTENTS), intents - set(od.LOCAL_INTENTS)


def test_api_failure_without_error_is_not_success(tmp_path):
    d = od.Dispatcher(base_url="http://unused", token="", hooks_dir=str(tmp_path),
                      local_handlers={}, http_post=lambda *args: {"ok": False})
    result = d.run("model_switch", {"session": "test"})
    assert result["ok"] is False
    assert result["reply"] != "Hecho."


def test_queued_action_is_not_reported_as_finished(tmp_path):
    d = od.Dispatcher(base_url="http://unused", token="", hooks_dir=str(tmp_path),
                      local_handlers={}, http_post=lambda *args: {"ok": True, "operationKey": "test|%1"})
    assert d.run("model_switch", {"session": "test"})["pending"] is True
    result = d.run("open_pomodoro", {})
    assert result["pending"] is True
    assert "falta confirmar" in result["reply"]
