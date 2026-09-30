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



def test_an_unreadable_registry_never_collapses_the_arrangement(dash, server):
    _, current = call(server, "GET", "/workspace")
    document = {k: current[k] for k in ("schema", "groups", "tabs")}
    document["groups"] = [current["groups"][0], {"id": "g-work", "tree": {
        "type": "split", "axis": "x", "ratio": 0.4,
        "first": {"type": "tab", "tabId": "alpha"}, "second": {"type": "tab", "tabId": "term-1"}}}]
    call(server, "POST", "/workspace", {"requestId": "arr", "expectedRevision": 1, "document": document})
    Path(dash.TABS_FILE).write_text('{"alpha": "Alp')          # corrupt, not "no tabs"
    for _ in range(2):
        status, body = call(server, "GET", "/workspace")
        assert status == 200 and body["revision"] == 2
        assert body["groups"][1]["id"] == "g-work" and set(body["tabs"]) == {"local", "alpha", "term-1"}
    Path(dash.TABS_FILE).write_text(json.dumps({"alpha": "Alpha", "term-1": "Uno"}))
    assert call(server, "GET", "/workspace")[1]["groups"][1]["id"] == "g-work"


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


def test_group_close_previews_then_closes_only_confirmed_members(dash, server, monkeypatch):
    _, current = call(server, "GET", "/workspace")
    document = {k: current[k] for k in ("schema", "groups", "tabs")}
    document["groups"] = [current["groups"][0], {"id": "g-work", "tree": {
        "type": "split", "axis": "x", "ratio": 0.5,
        "first": {"type": "tab", "tabId": "alpha"}, "second": {"type": "tab", "tabId": "term-1"}}}]
    call(server, "POST", "/workspace", {"requestId": "g", "expectedRevision": 1, "document": document})
    ids = {"alpha": "$4", "term-1": "$5"}
    monkeypatch.setattr(dash, "workspace_session_identity", lambda s: ids.get(s))
    closed = []
    monkeypatch.setattr(dash, "close_app_tab", lambda s: closed.append(s))
    status, preview = call(server, "GET", "/workspace/close-group?groupId=g-work")
    assert status == 200 and [m["label"] for m in preview["members"]] == ["Alpha", "Uno"]
    body = {"requestId": "close-1", "groupId": "g-work", "expectedRevision": preview["revision"],
            "members": preview["members"]}
    ids["term-1"] = "$6"                 # recreated between preview and confirm
    status, refused = call(server, "POST", "/workspace/close-group", body)
    assert status == 400 and closed == []
    ids["term-1"] = "$5"
    status, result = call(server, "POST", "/workspace/close-group", dict(body, requestId="close-2"))
    assert status == 200 and result["closed"] == ["alpha", "term-1"] and closed == ["alpha", "term-1"]
    status, stale = call(server, "POST", "/workspace/close-group", dict(body, requestId="close-3", expectedRevision=1))
    assert status == 409


def test_operator_split_close_uses_the_guarded_pane_route(dash, monkeypatch):
    calls = []
    fake = type(sys)("terminal_panes")

    def execute(tmux, identify, save, data):
        calls.append(dict(data))
        if data.get("action", "list") == "list":
            return {"ok": True, "panes": [{"id": "%4", "identity": "v4"}, {"id": "%5", "identity": "v5"}]}
        return {"ok": True}
    fake.execute = execute
    monkeypatch.setitem(sys.modules, "terminal_panes", fake)
    monkeypatch.setattr(dash, "operator_pane", lambda s, p=None: ("alpha", "=alpha", "=alpha:"))
    monkeypatch.setattr(dash, "tmux", lambda *a, **k: dash.subprocess.CompletedProcess(a, 0, "%5\n", ""))
    assert dash.operator_close_split("alpha") is None
    assert calls[-1] == {"session": "alpha", "action": "close", "pane": "%5", "identity": "v5"}

    def refuse(tmux, identify, save, data):
        if data.get("action", "list") == "list":
            return {"ok": True, "panes": [{"id": "%5", "identity": "v5"}]}
        raise ValueError("El último panel permanece abierto")
    fake.execute = refuse
    assert dash.operator_close_split("alpha") == "El último panel permanece abierto"


def test_sort_once_reorders_the_shared_arrangement_and_can_be_undone(dash, server, monkeypatch):
    """Grill 30-sep: «Ordenar una vez» vive en el servidor para que escritorio y
    remoto vean el mismo orden; devuelve el orden anterior para Deshacer."""
    monkeypatch.setattr(dash, "read_prefs", lambda: {"favorites": ["term-1"]})
    monkeypatch.setattr(dash, "read_states_cached", lambda ttl=1.2: [])
    code, body = call(server, "GET", "/workspace")
    before = [g["id"] for g in body["groups"]]
    code, body = call(server, "POST", "/workspace/sort", {"by": "fav"})
    assert code == 200
    after = [g["id"] for g in body["groups"]]
    fav_group = next(g["id"] for g in body["groups"] if g["tree"].get("tabId") == "term-1")
    assert after[0] == fav_group or after[1] == fav_group, "the favourite goes first (after local)"
    assert body["previous"] == before
    code, body = call(server, "POST", "/workspace/sort", {"restore": before})
    assert code == 200 and [g["id"] for g in body["groups"]] == before
    code, body = call(server, "POST", "/workspace/sort", {"by": "nope"})
    assert code == 400
