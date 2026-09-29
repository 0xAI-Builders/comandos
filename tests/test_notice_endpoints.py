"""N2 endpoints over HTTP with a temporary state database."""
import http.client
import http.server
import importlib.machinery
import importlib.util
import json
from pathlib import Path
import sys
import threading

import pytest

ROOT = Path(__file__).resolve().parents[1]


@pytest.fixture
def server(tmp_path, monkeypatch):
    monkeypatch.setenv("COMANDOS_STATE_DB", str(tmp_path / "state.sqlite3"))
    monkeypatch.setenv("HOME", str(tmp_path))
    (tmp_path / ".claude/hooks").mkdir(parents=True)
    for path in (ROOT / "bin", ROOT / "lib"):
        if str(path) not in sys.path:
            sys.path.insert(0, str(path))
    loader = importlib.machinery.SourceFileLoader("cc_dash_notices_under_test", str(ROOT / "bin/cc-dash"))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    dash = importlib.util.module_from_spec(spec)
    loader.exec_module(dash)
    dash._NOTICES_LOCAL.__dict__.clear()
    monkeypatch.setattr(dash, "notices_focus_active", lambda: False)
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), dash.Handler)
    thread = threading.Thread(target=srv.serve_forever, daemon=True)
    thread.start()
    yield srv, dash
    srv.shutdown()
    srv.server_close()


def call(srv, method, path, body=None):
    c = http.client.HTTPConnection(*srv.server_address, timeout=5)
    try:
        c.request(method, path, body=None if body is None else json.dumps(body),
                  headers={"Content-Type": "application/json"})
        r = c.getresponse()
        return r.status, json.loads(r.read() or b"null")
    finally:
        c.close()


def record(dash, kind, eid, pane="%1"):
    conn = dash.notices_conn()
    now = dash.notices_now()
    conn.execute("BEGIN IMMEDIATE")
    dash.event_store.append_event(conn, {"eventId": eid, "source": "test", "kind": kind, "evidence": "confirmed",
                                         "correlation": "local", "occurredAtMs": now, "receivedAtMs": now,
                                         "projectKey": "ComandOS", "sessionKey": "alpha", "paneId": pane})
    conn.execute("COMMIT")


def test_strip_reads_notices_and_marking_read_keeps_requests_pending(server):
    srv, dash = server
    record(dash, "turn_started", "s1")
    record(dash, "permission_requested", "p1")
    status, page = call(srv, "GET", "/notices?after=0&deviceId=phone")
    assert status == 200 and [n["eventId"] for n in page["notices"]] == ["p1"]
    assert page["pending"] == ["p1"] and page["prefs"]["modes"]["done"] == "visual"
    assert call(srv, "POST", "/notices/read", {"all": True})[1]["read"] == ["p1"]
    again = call(srv, "GET", "/notices?after=0")[1]
    assert again["notices"][0]["read"] is True and again["pending"] == ["p1"]


def test_only_the_last_interacted_visible_device_plays_once(server):
    srv, dash = server
    record(dash, "permission_requested", "p1")
    call(srv, "POST", "/presence", {"deviceId": "desk", "visible": True, "canPlayAudio": True, "interaction": True})
    call(srv, "POST", "/presence", {"deviceId": "phone", "visible": True, "canPlayAudio": True, "interaction": True})
    assert call(srv, "POST", "/notices/sound", {"eventId": "p1", "deviceId": "desk"})[1]["play"] is False
    assert call(srv, "POST", "/notices/sound", {"eventId": "p1", "deviceId": "phone"})[1] == {"play": True, "cue": "permission"}
    assert call(srv, "POST", "/notices/sound", {"eventId": "p1", "deviceId": "phone"})[1]["play"] is False


def test_prefs_round_trip_and_validation(server):
    srv, _ = server
    assert call(srv, "POST", "/notices/prefs", {"modes": {"done": "sound"}, "volume": 0.25})[0] == 200
    assert call(srv, "GET", "/notices/prefs")[1]["modes"]["done"] == "sound"
    assert call(srv, "POST", "/notices/prefs", {"volume": 9})[0] == 400


def test_notice_reads_are_token_gated(server):
    _, dash = server
    assert any("/notices".startswith(p) for p in dash.Handler.API_GET)


def test_model_news_and_usage_producers_go_through_the_event_log(server, monkeypatch):
    srv, dash = server
    posted = []
    monkeypatch.setattr(dash.urllib.request, "urlopen",
                        lambda req, timeout=None: posted.append(json.loads(req.data.decode())))
    dash._model_watch_notify({"codex": ["gpt-9"]}, {"codex": "1.0"})
    dash.usage_alert_send("Límite al 90%")
    kinds = [n["kind"] for n in call(srv, "GET", "/notices?after=0")[1]["notices"]]
    assert kinds == ["announcement", "usage_alert"]
    assert posted and all(p["kind"] != "waiting" for p in posted)


def test_a_published_edition_becomes_one_news_notice_with_its_id(server):
    srv, dash = server
    edition = {"id": "2026-09-29@15:00", "slot": "15:00", "status": "published", "storyCount": 7}
    dash._news_edition_notice(edition)
    dash._news_edition_notice(edition)          # same edition twice: one notice
    notices = call(srv, "GET", "/notices?after=0")[1]["notices"]
    assert [(n["kind"], n["editionId"], n["project"]) for n in notices] == [("news_edition", "2026-09-29@15:00", None)]


def test_focus_end_plays_on_the_desktop_speaker_only_when_it_wins(server, monkeypatch):
    srv, dash = server
    played = []
    monkeypatch.setattr(dash, "_play_local_sound", lambda path: played.append(path))
    record(dash, "focus_completed", "pomodoro:b1:completed")
    # Nobody visible: this machine is the fallback speaker.
    assert dash._desktop_notice_sound("pomodoro:b1:completed") is True and len(played) == 1
    assert dash._desktop_notice_sound("pomodoro:b1:completed") is False      # claimed once
    record(dash, "focus_completed", "pomodoro:b2:completed")
    call(srv, "POST", "/presence", {"deviceId": "phone", "visible": True, "canPlayAudio": True, "interaction": True})
    assert dash._desktop_notice_sound("pomodoro:b2:completed") is False and len(played) == 1
    assert dash.DESKTOP_DEVICE.startswith("desktop-")
