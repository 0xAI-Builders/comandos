"""cc-dash /news/editions and /news/edition against a temporary database.

Reading must never generate: the fetcher, the summarizer and the scheduler
are replaced with tripwires.
"""
import http.client
import http.server
import importlib.machinery
import importlib.util
import json
import sys
import threading
from datetime import date
from pathlib import Path

import pytest

sys.path.insert(0, str(Path("lib").resolve()))
import app_state  # noqa: E402
import news_editions  # noqa: E402


@pytest.fixture
def dash(tmp_path, monkeypatch):
    monkeypatch.setenv("COMANDOS_STATE_DB", str(tmp_path / "state.sqlite3"))
    bin_dir = str(Path("bin").resolve())
    if bin_dir not in sys.path:
        sys.path.insert(0, bin_dir)
    loader = importlib.machinery.SourceFileLoader("cc_dash_news_under_test", str(Path("bin/cc-dash").resolve()))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    monkeypatch.setattr(module, "HOOKS", str(tmp_path))
    module._NEWS_LOCAL.__dict__.clear()

    def tripwire(*a, **kw):
        raise AssertionError("reading an edition must not generate")
    monkeypatch.setattr(news_editions, "run_due", tripwire)
    monkeypatch.setattr(news_editions, "make_summarizer", tripwire)
    monkeypatch.setattr(news_editions, "make_fetcher", tripwire)
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


def call(srv, path, headers=None):
    client = http.client.HTTPConnection(*srv.server_address, timeout=5)
    try:
        client.request("GET", path, headers=headers or {})
        response = client.getresponse()
        raw = response.read()
        return response.status, (json.loads(raw) if raw else None)
    finally:
        client.close()


def seed(tmp_path):
    conn = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(conn)
    policy = news_editions.default_policy()
    news_editions.schedule_editions(conn, date(2026, 9, 28), policy)
    job = news_editions.claim_due_job(conn, news_editions.slot_times(date(2026, 9, 28), policy)[0][1] + 1000, policy)

    def fetch(policy, limit):
        return {"items": [{"url": "https://example.com/a", "title": "Nota A", "kind": "mcp", "source": "hn",
                           "text": "cuerpo", "fetchStatus": "ok", "discoveredAt": 1}], "failures": []}

    def summarize(req):
        return {"costUsd": 0.01, "model": "fake", "story": {
            "title": "MCP nuevo", "summary": "Resumen", "body": "<script>alert(1)</script> texto",
            "sourceIds": ["s1"]}}
    news_editions.build_edition(conn, job, fetch, summarize, policy, clock=lambda: job["claimedAt"])
    conn.close()
    return job["editionId"]


def test_editions_list_reports_unconfigured_and_existing_editions(dash, server, tmp_path):
    eid = seed(tmp_path)
    status, body = call(server, "/news/editions")
    assert status == 200
    assert body["configured"] is False and body["reason"] == "sin configurar"
    assert body["policy"]["slots"] == ["09:00", "15:00", "21:00"]
    assert body["policy"]["timezone"] == "America/Mexico_City"
    assert body["latest"] == eid
    listed = {e["id"]: e["status"] for e in body["editions"]}
    assert listed[eid] == "published"


def test_edition_detail_returns_stories_with_sources(server, tmp_path):
    eid = seed(tmp_path)
    status, body = call(server, "/news/edition?id=" + eid)
    assert status == 200
    story = body["stories"][0]
    assert story["title"] == "MCP nuevo"
    assert story["body"].startswith("<script>")       # stored as data; the reader escapes it
    assert story["sources"][0]["url"] == "https://example.com/a"
    assert call(server, "/news/edition?id=latest")[1]["edition"]["id"] == eid


def test_edition_detail_validates_the_id(server, tmp_path):
    seed(tmp_path)
    assert call(server, "/news/edition?id=../../etc")[0] == 400
    assert call(server, "/news/edition")[0] == 400
    assert call(server, "/news/edition?id=2026-01-01@09:00")[0] == 404


def test_news_edition_endpoints_require_token_remotely(server, tmp_path):
    seed(tmp_path)
    status, _ = call(server, "/news/editions", headers={"X-Forwarded-For": "100.64.0.9"})
    assert status == 401
    status, _ = call(server, "/news/edition?id=latest", headers={"X-Forwarded-For": "100.64.0.9"})
    assert status == 401
