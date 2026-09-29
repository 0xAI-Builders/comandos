"""cc-dash owns one Pomodoro authority: GET snapshot, POST commands, scheduler."""

import importlib.machinery
import importlib.util
import json
import os
from pathlib import Path
import sys
import threading
import time

import pytest


ROOT = Path(__file__).resolve().parents[1]
MIN = 60_000
T0 = 2_000_000_000_000


class Clock:
    def __init__(self):
        self.now = T0

    def __call__(self):
        return self.now


@pytest.fixture
def dash(tmp_path, monkeypatch):
    home = tmp_path / "home"
    (home / ".claude" / "hooks").mkdir(parents=True)
    monkeypatch.setenv("HOME", str(home))
    monkeypatch.setenv("COMANDOS_STATE_DB", str(tmp_path / "state.sqlite3"))
    monkeypatch.delenv("COMANDOS_DASH_DIR", raising=False)
    for path in (ROOT / "bin", ROOT / "lib"):
        if str(path) not in sys.path:
            sys.path.insert(0, str(path))
    loader = importlib.machinery.SourceFileLoader(
        f"cc_dash_pomodoro_{os.getpid()}_{id(tmp_path)}", str(ROOT / "bin" / "cc-dash"))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    clock = Clock()
    module.POMODORO_CLOCK = clock
    module.test_clock = clock
    events = []
    module._pomodoro_emit = lambda conn, event: events.append(event)
    module.test_events = events
    return module


def get(dash, path="/pomodoro"):
    handler = object.__new__(dash.Handler)
    handler.path = path
    handler._json = lambda code, body: (code, body)
    return handler._do_GET()


def test_get_is_the_server_snapshot_and_is_token_gated(dash):
    assert any("/pomodoro".startswith(p) for p in dash.Handler.API_GET)
    code, body = get(dash)
    assert code == 200
    assert body["block"] is None and body["revision"] == 0
    assert body["serverNowMs"] == T0
    assert body["queue"] == [] and isinstance(body["settings"], dict)


def test_post_command_contract(dash):
    code, started = dash.pomodoro_post({"requestId": "a", "expectedRevision": 0, "action": "start",
                                        "mode": "focus", "targetMs": 25 * MIN, "project": "ComandOS",
                                        "sessionKey": "s", "paneKey": "%1"})
    assert code == 200 and started["block"]["status"] == "running"
    dash.test_clock.now += MIN
    code, extended = dash.pomodoro_post({"requestId": "b", "expectedRevision": started["revision"],
                                         "action": "extend", "deltaMs": 5 * MIN})
    assert code == 200 and extended["block"]["targetMs"] == 30 * MIN
    _, snap = get(dash)
    assert snap["block"]["deadlineMs"] == T0 + 30 * MIN
    code, stale = dash.pomodoro_post({"requestId": "c", "expectedRevision": 0, "action": "cancel"})
    assert code == 409 and stale["code"] == "stale_revision"
    assert stale["state"]["block"]["targetMs"] == 30 * MIN
    code, bad = dash.pomodoro_post({"requestId": "d", "expectedRevision": None, "action": "explode"})
    assert code == 400 and bad["ok"] is False


def test_legacy_operator_body_still_starts_and_stops(dash):
    code, body = dash.pomodoro_post({"mins": 15, "mode": "focus", "project": "Lola", "session": "s1"})
    assert code == 200 and body["block"]["targetMs"] == 15 * MIN and body["block"]["project"] == "Lola"
    code, body = dash.pomodoro_post({"stop": True, "status": "cancelled"})
    assert code == 200 and body["block"]["status"] == "cancelled"


def test_scheduler_settles_without_any_client(dash):
    _, started = dash.pomodoro_post({"requestId": "a", "expectedRevision": 0, "action": "start",
                                     "mode": "focus", "targetMs": MIN})
    stop = threading.Event()
    thread = threading.Thread(target=dash.pomodoro_scheduler_loop, args=(stop,), daemon=True)
    thread.start()
    try:
        dash.test_clock.now = started["block"]["deadlineMs"] + 5
        dash._POMODORO_WAKE.set()
        for _ in range(100):
            if get(dash)[1]["block"]["status"] == "completed":
                break
            time.sleep(0.02)
    finally:
        stop.set()
        dash._POMODORO_WAKE.set()
        thread.join(timeout=2)
    assert get(dash)[1]["block"]["status"] == "completed"
    assert [e["kind"] for e in dash.test_events] == ["focus_completed"]


def test_legacy_focus_file_is_adopted_and_retired(dash):
    until = (T0 + 10 * MIN) / 1000
    Path(dash.FOCUS_FILE).write_text(json.dumps({
        "blockId": "old-1", "mode": "focus", "project": "MRP", "session": "s", "pane": "%2",
        "until": until, "startedAt": until - 25 * 60, "mins": 25}))
    assert dash.pomodoro_adopt_legacy() is True
    assert not Path(dash.FOCUS_FILE).exists()
    assert list(Path(dash.FOCUS_FILE).parent.glob("focus.json.migrated-*"))
    block = get(dash)[1]["block"]
    assert block["blockId"] == "old-1" and block["deadlineMs"] == T0 + 10 * MIN


def test_missing_event_store_is_a_logged_noop(tmp_path, monkeypatch, capsys):
    import sqlite3
    monkeypatch.setitem(sys.modules, "event_store", None)
    loader = importlib.machinery.SourceFileLoader("cc_dash_emit_probe", str(ROOT / "bin" / "cc-dash"))
    monkeypatch.setenv("HOME", str(tmp_path))
    (tmp_path / ".claude" / "hooks").mkdir(parents=True)
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    conn = sqlite3.connect(":memory:")
    module._pomodoro_emit(conn, {"kind": "focus_completed"})
    module._pomodoro_emit(conn, {"kind": "focus_completed"})
    assert capsys.readouterr().err.count("event_store no disponible") == 1


def test_style_is_one_global_setting_and_never_touches_the_block(dash):
    _, started = dash.pomodoro_post({"requestId": "a", "expectedRevision": 0, "action": "start",
                                     "mode": "focus", "targetMs": 25 * MIN})
    assert get(dash)[1]["settings"].get("style") in (None, "alchemy")
    code, body = dash.pomodoro_post({"settings": {"style": "crystals"}})
    assert code == 200 and body["settings"]["style"] == "crystals"
    assert get(dash)[1]["settings"]["style"] == "crystals"
    code, body = dash.pomodoro_post({"settings": {"style": "vaporwave"}})
    assert code == 400
    snap = get(dash)[1]
    assert snap["revision"] == started["revision"] and snap["block"] == started["block"]


def test_report_endpoint_filters_and_rejects_bad_ranges(dash):
    _, started = dash.pomodoro_post({"requestId": "a", "expectedRevision": 0, "action": "start",
                                     "mode": "focus", "targetMs": 25 * MIN, "project": "ComandOS"})
    dash.test_clock.now += 10 * MIN
    dash.pomodoro_post({"requestId": "b", "expectedRevision": None, "action": "cancel"})
    code, report = get(dash, "/pomodoro/report")
    assert code == 200 and report["measured"]["cancelled"]["activeMs"] == 10 * MIN
    assert len(report["byDay"]) == 7 and report["range"]["timezone"] == "America/Mexico_City"
    code, other = get(dash, "/pomodoro/report?project=Lola")
    assert code == 200 and other["measured"]["activeMs"] == 0
    assert get(dash, "/pomodoro/report?from=10&to=5")[0] == 400
    assert get(dash, "/pomodoro/report?from=x")[0] == 400


def test_scheduler_maps_legacy_usage_history_once(dash):
    import cc_usage
    block = cc_usage.focus_block_start(dash.USAGE_DB, {"mode": "focus", "planned_minutes": 25,
                                                       "started_at_ms": T0 - 3600_000})
    cc_usage.focus_block_finish(dash.USAGE_DB, block["id"], "completed", T0 - 2100_000)
    stop = threading.Event()
    stop.set()   # run only the start-up work
    dash.pomodoro_scheduler_loop(stop)
    dash.pomodoro_scheduler_loop(stop)
    code, report = get(dash, "/pomodoro/report")
    assert report["legacy"]["completedBlocks"] == 1 and report["measured"]["activeMs"] == 0


def test_report_accepts_local_dates(dash):
    code, report = get(dash, "/pomodoro/report?fromDate=2033-05-01&toDate=2033-05-03")
    assert code == 200 and [d["date"] for d in report["byDay"]] == ["2033-05-01", "2033-05-02", "2033-05-03"]
    assert get(dash, "/pomodoro/report?fromDate=mayo")[0] == 400
