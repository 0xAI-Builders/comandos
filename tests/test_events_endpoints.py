"""cc-dash N1 endpoints against a temporary database, hooks dir and token."""
import http.client
import http.server
import importlib.machinery
import importlib.util
import json
import sys
import threading
from pathlib import Path

import pytest


@pytest.fixture
def dash(tmp_path, monkeypatch):
    monkeypatch.setenv("COMANDOS_STATE_DB", str(tmp_path / "state.sqlite3"))
    bin_dir = str(Path("bin").resolve())
    if bin_dir not in sys.path:
        sys.path.insert(0, bin_dir)
    loader = importlib.machinery.SourceFileLoader(
        "cc_dash_events_under_test", str(Path("bin/cc-dash").resolve()))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    (tmp_path / "app-tabs.json").write_text(json.dumps({"sess": "Sess"}))
    monkeypatch.setattr(module, "TABS_FILE", str(tmp_path / "app-tabs.json"))
    monkeypatch.setattr(module, "HOOKS", str(tmp_path))
    monkeypatch.setattr(module, "LAYOUT_SNAPSHOT_FILE", str(tmp_path / "app-sessions-v2.json"))
    monkeypatch.setattr(module, "EVENTS", str(tmp_path / "events.jsonl"))
    monkeypatch.setattr(module, "TOKEN_FILE", str(tmp_path / "dash-token"))
    (tmp_path / "dash-token").write_text("local-test-token")
    (tmp_path / "events.jsonl").write_text(
        json.dumps({"project": "old", "status": "done", "detail": "viejo", "ts": 10}) + "\n")
    module._WORKSPACE_LOCAL.__dict__.clear()
    module._EVENTS_V2_LEGACY["done"] = False
    return module


@pytest.fixture
def server(dash):
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), dash.Handler)
    thread = threading.Thread(target=srv.serve_forever, daemon=True)
    thread.start()
    yield srv
    srv.shutdown()
    srv.server_close()
    thread.join(timeout=2)


def call(srv, method, path, body=None, headers=None):
    client = http.client.HTTPConnection(*srv.server_address, timeout=5)
    try:
        h = {"Content-Type": "application/json", **(headers or {})}
        client.request(method, path, body=None if body is None else json.dumps(body), headers=h)
        response = client.getresponse()
        raw = response.read()
        return response.status, (json.loads(raw) if raw else None)
    finally:
        client.close()


TOKEN = {"X-Comandos-Token": "local-test-token"}


def hook(kind="Stop", **kw):
    return {"hookEvent": kind, "agent": "codex", "project": "proj", "session": "sess",
            "pane": "%3", "conversationId": "thread-1", "turnId": "t1", "occurredAtMs": 1000, **kw}


def test_events_v2_is_behind_the_token_gate(dash):
    assert any("/events/v2".startswith(p) for p in dash.Handler.API_GET)


def test_list_imports_history_once_without_destination(server):
    status, body = call(server, "GET", "/events/v2")
    assert status == 200
    assert [(e["kind"], e["evidence"], e["destination"]) for e in body["events"]] == [
        ("turn_completed", "historical", "none")]
    assert body["nextAfter"] == body["latest"] == body["events"][0]["sequence"]
    status, again = call(server, "GET", "/events/v2?after=" + str(body["nextAfter"]))
    assert status == 200 and again["events"] == [] and again["nextAfter"] == body["nextAfter"]


def test_internal_post_records_and_pages(server):
    status, body = call(server, "POST", "/events/v2", hook("UserPromptSubmit", occurredAtMs=900), TOKEN)
    assert status == 200 and body["event"]["kind"] == "prompt_accepted"
    call(server, "POST", "/events/v2", hook(), TOKEN)
    status, dup = call(server, "POST", "/events/v2", hook(), TOKEN)
    assert status == 200 and dup["event"]["duplicate"] is True
    _, page = call(server, "GET", "/events/v2?after=1&limit=1&turns=1")
    assert [e["kind"] for e in page["events"]] == ["prompt_accepted"]  # 1 is the history
    turn = page["turns"]["tmux:sess:%3"]
    assert turn["state"] == "completed" and turn["turnId"] == "t1"


def test_internal_post_rejects_browsers_proxies_and_missing_token(server):
    assert call(server, "POST", "/events/v2", hook())[0] == 403
    assert call(server, "POST", "/events/v2", hook(), {"X-Comandos-Token": "wrong"})[0] == 403
    origin = {**TOKEN, "Origin": "http://%s:%d" % server.server_address,
              "Host": "%s:%d" % server.server_address}
    assert call(server, "POST", "/events/v2", hook(), origin)[0] == 403
    proxied = {**TOKEN, "X-Forwarded-For": "127.0.0.1"}
    assert call(server, "POST", "/events/v2", hook(), proxied)[0] in (401, 403)


def test_bad_pagination_and_bad_event(server):
    assert call(server, "GET", "/events/v2?after=-1")[0] == 400
    assert call(server, "GET", "/events/v2?limit=abc")[0] == 400
    status, _ = call(server, "POST", "/events/v2", {"kind": "nope", "source": "x"}, TOKEN)
    assert status == 400


def test_old_events_endpoint_still_serves_the_timeline(server):
    status, body = call(server, "GET", "/events")
    assert status == 200 and body[0]["project"] == "old"
