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


def _app_consumer(path, seen, stop):
    """Imita on_app_command de cc-app: lee app-command.json y lo borra."""
    import os, time
    while not stop.is_set():
        try:
            data = json.loads(path.read_text())
            os.remove(path)
            seen.append(data)
        except (FileNotFoundError, ValueError):
            pass
        time.sleep(0.005)


def test_app_target_writes_command_file(tmp_path):
    import threading
    seen, stop = [], threading.Event()
    t = threading.Thread(target=_app_consumer, args=(tmp_path / "app-command.json", seen, stop), daemon=True)
    t.start()
    try:
        out = _dispatcher(tmp_path, [], []).run("app_mosaic", {"state": "on"})
    finally:
        stop.set(); t.join()
    data = seen[0]
    assert out["ok"] and data["command"] == "mosaic" and data["args"] == {"state": "on"} and data["ts"] > 0


def test_app_sin_app_abierta_no_es_exito_ni_deja_el_comando(tmp_path):
    d = _dispatcher(tmp_path, [], [])
    d.app_wait = 0.05
    out = d.run("app_mosaic", {"state": "on"})
    assert out["ok"] is False and "app" in out["reply"].lower()
    assert not (tmp_path / "app-command.json").exists(), "un comando retirado no puede ejecutarse más tarde"


def test_app_no_pisa_un_comando_pendiente_sin_recoger(tmp_path):
    pendiente = tmp_path / "app-command.json"
    pendiente.write_text(json.dumps({"command": "split", "args": {}, "ts": 1}))
    d = _dispatcher(tmp_path, [], [])
    d.app_wait = 0.05
    out = d.run("app_mosaic", {"state": "on"})
    assert out["ok"] is False
    assert json.loads(pendiente.read_text())["command"] == "split"


def test_app_comando_abandonado_hace_rato_se_sustituye(tmp_path):
    import os
    pendiente = tmp_path / "app-command.json"
    pendiente.write_text(json.dumps({"command": "split", "args": {}, "ts": 1}))
    os.utime(pendiente, (1, 1))
    d = _dispatcher(tmp_path, [], [])
    d.app_wait = 0.05
    out = d.run("app_mosaic", {"state": "on"})
    assert "anterior" not in out["reply"]


def test_local_con_error_es_fallo_y_su_recibo_tambien(tmp_path):
    import operator_receipts
    d = od.Dispatcher(base_url="http://unused", token="", hooks_dir=str(tmp_path), receipt_root=tmp_path / "r",
                      local_handlers={"optimization_apply": lambda a: {"error": "s1: No hay sesión"},
                                      "rename": lambda a: {"ok": False, "reply": "s1: No hay sesión"}})
    out = d.run("optimization_apply", {"profile": "x", "sessions": ["s1"]})
    assert out["ok"] is False and "No hay sesión" in out["reply"] and out["reply"] != "Hecho."
    out2 = d.run("rename_tab", {"tab": "s1", "name": "n"})
    assert out2["ok"] is False
    estados = {r["id"]: r["status"] for r in operator_receipts.recent(tmp_path / "r")}
    assert estados[out["receiptId"]] == "failed" and estados[out2["receiptId"]] == "failed"


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
