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


def test_completion_is_written_to_the_event_log(dash):
    import app_state
    loader = importlib.machinery.SourceFileLoader("cc_dash_emit_probe", str(ROOT / "bin" / "cc-dash"))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    fresh = importlib.util.module_from_spec(spec)
    loader.exec_module(fresh)           # the fixture stubs _pomodoro_emit; this one is real
    fresh._play_local_sound = lambda path: None      # never sound on the test machine
    conn = app_state.connect()
    app_state.migrate(conn)
    conn.execute("BEGIN IMMEDIATE")
    fresh._pomodoro_emit(conn, {"eventId": "pomodoro:b1:completed", "source": "pomodoro", "kind": "focus_completed",
                               "evidence": "confirmed", "correlation": "source", "occurredAtMs": 1, "receivedAtMs": 1,
                               "paneKey": "pane-a"})
    conn.execute("COMMIT")
    assert [e["kind"] for e in dash.event_store.list_events(conn)] == ["focus_completed"]


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


def test_snapshot_carries_ledger_progress_after_a_real_completion(dash):
    _, started = dash.pomodoro_post({"requestId": "a", "expectedRevision": 0, "action": "start",
                                     "mode": "focus", "targetMs": 25 * MIN})
    code, snap = get(dash)
    assert snap["progress"]["xp"] == 0 and snap["progress"]["policyVersion"] == "v1"
    dash.test_clock.now = started["block"]["deadlineMs"]
    dash.pomodoro_store().settle_due()
    dash.pomodoro_store().settle_due()
    progress = get(dash)[1]["progress"]
    assert progress["xp"] == 250 and progress["level"] == 1 and progress["todayMinutes"] == 25
    assert progress["dailyGoalMinutes"] == 100


def test_payload_says_where_the_end_will_sound(dash):
    """Grill 30-sep: «nunca escuché la notificación» — the panel shows which
    device will ring when the focus block ends."""
    code, body = get(dash)
    assert code == 200
    sound = body["sound"]
    assert set(sound) >= {"enabled", "device", "desktopDevice"}
    assert sound["desktopDevice"] == dash.DESKTOP_DEVICE


def test_focus_end_on_this_machine_is_the_game_jingle_at_the_notice_volume(dash, monkeypatch):
    played = []
    monkeypatch.setattr(dash, "_play_local_sound", lambda path, volume=None: played.append((path, volume)) or True)
    import notification_delivery
    monkeypatch.setattr(notification_delivery, "claim_sound", lambda conn, e, d, now, *a: {"play": d == dash.DESKTOP_DEVICE, "cue": "complete"})
    monkeypatch.setattr(notification_delivery, "load_prefs", lambda conn: {"volume": 0.8})
    assert dash._desktop_notice_sound("pomodoro:b1:completed") is True
    path, volume = played[0]
    assert path.endswith("pomodoro-complete.wav") and Path(path).is_file()
    assert volume == 0.8
    played.clear()
    dash._desktop_notice_sound("event-other")
    assert played[0][0] == dash.DESKTOP_NOTICE_SOUND
