"""cc-dash /workspace endpoints against a temporary database and tab registry."""
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
        "cc_dash_workspace_under_test", str(Path("bin/cc-dash").resolve()))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    tabs = tmp_path / "app-tabs.json"
    tabs.write_text(json.dumps({"alpha": "Alpha", "term-1": "Uno"}))
    monkeypatch.setattr(module, "TABS_FILE", str(tabs))
    monkeypatch.setattr(module, "HOOKS", str(tmp_path))
    monkeypatch.setattr(module, "TABS_META_FILE", str(tmp_path / "meta.json"), raising=False)
    monkeypatch.setattr(module, "LAYOUT_SNAPSHOT_FILE", str(tmp_path / "app-sessions-v2.json"))
    module._WORKSPACE_LOCAL.__dict__.clear()
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


def call(srv, method, path, body=None):
    client = http.client.HTTPConnection(*srv.server_address, timeout=5)
    try:
        client.request(method, path, body=None if body is None else json.dumps(body),
                       headers={"Content-Type": "application/json"})
        response = client.getresponse()
        raw = response.read()
        return response.status, (json.loads(raw) if raw else None)
    finally:
        client.close()


def test_get_builds_workspace_from_the_tab_registry(server):
    status, body = call(server, "GET", "/workspace")
    assert status == 200 and body["ready"] is True
    assert [g["tree"]["tabId"] for g in body["groups"]] == ["local", "alpha", "term-1"]
    assert body["tabs"]["alpha"]["label"] == "Alpha"
    assert body["revision"] == 1
    assert call(server, "GET", "/workspace")[1]["revision"] == 1  # no churn


def test_import_leaves_the_legacy_registry_untouched(dash, server):
    before = Path(dash.TABS_FILE).read_bytes()
    call(server, "GET", "/workspace")
    assert Path(dash.TABS_FILE).read_bytes() == before


def test_post_groups_tabs_and_rejects_stale_revision(server):
    _, current = call(server, "GET", "/workspace")
    document = {k: current[k] for k in ("schema", "groups", "tabs")}
    document["groups"] = [current["groups"][0], {"id": "g-work", "tree": {
        "type": "split", "axis": "x", "ratio": 0.4,
        "first": {"type": "tab", "tabId": "alpha"}, "second": {"type": "tab", "tabId": "term-1"}}}]
    status, saved = call(server, "POST", "/workspace",
                         {"requestId": "r-1", "expectedRevision": current["revision"], "document": document})
    assert status == 200 and saved["revision"] == 2
    status, conflict = call(server, "POST", "/workspace",
                            {"requestId": "r-2", "expectedRevision": 1, "document": document})
    assert status == 409 and conflict["current"]["revision"] == 2
    assert call(server, "GET", "/workspace")[1]["groups"][1]["id"] == "g-work"


def test_post_cannot_add_or_drop_tabs_through_the_layout(server):
    _, current = call(server, "GET", "/workspace")
    document = {"schema": 1, "groups": current["groups"][:2],
                "tabs": {k: current["tabs"][k] for k in ("local", "alpha")}}
    status, body = call(server, "POST", "/workspace",
                        {"requestId": "drop", "expectedRevision": current["revision"], "document": document})
    assert status == 400 and body["error"]


def test_closing_a_tab_keeps_the_rest_of_the_arrangement(dash, server, monkeypatch):
    _, current = call(server, "GET", "/workspace")
    document = {k: current[k] for k in ("schema", "groups", "tabs")}
    document["groups"] = [current["groups"][0], {"id": "g-work", "tree": {
        "type": "split", "axis": "y", "ratio": 0.7,
        "first": {"type": "tab", "tabId": "alpha"}, "second": {"type": "tab", "tabId": "term-1"}}}]
    call(server, "POST", "/workspace", {"requestId": "g", "expectedRevision": 1, "document": document})
    monkeypatch.setattr(dash, "tmux", lambda *a, **k: dash.subprocess.CompletedProcess(a, 1, "", ""))
    monkeypatch.setattr(dash, "remember_tab", lambda *a, **k: None)
    monkeypatch.setattr(dash, "state_agent", lambda s: "")
    assert dash.close_app_tab("term-1") is None
    _, after = call(server, "GET", "/workspace")
    assert after["groups"][1] == {"id": "g-work", "tree": {"type": "tab", "tabId": "alpha"}}
    assert "term-1" not in after["tabs"]


def test_client_focus_is_per_device(server):
    call(server, "GET", "/workspace")
    assert call(server, "POST", "/workspace/client",
                {"deviceId": "phone", "activeTabId": "alpha", "activePaneKey": None})[0] == 200
    assert call(server, "POST", "/workspace/client",
                {"deviceId": "desk", "activeTabId": "term-1"})[0] == 200
    assert call(server, "GET", "/workspace/client?deviceId=phone")[1]["activeTabId"] == "alpha"
    assert call(server, "GET", "/workspace/client?deviceId=desk")[1]["activeTabId"] == "term-1"
    assert call(server, "GET", "/workspace")[1]["revision"] == 1
    assert call(server, "POST", "/workspace/client", {"deviceId": ""})[0] == 400


def test_workspace_requires_the_security_gate(dash):
    assert any("/workspace".startswith(p) for p in dash.Handler.API_GET)
