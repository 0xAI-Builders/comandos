"""Pomodoro analytics come from records by blockId, never from the visual clock."""

from datetime import datetime
from pathlib import Path
import sys
from zoneinfo import ZoneInfo

import pytest


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import app_state  # noqa: E402
import pomodoro  # noqa: E402

MIN = 60_000
MX = ZoneInfo("America/Mexico_City")


def local_ms(y, mo, d, h=0, mi=0):
    return int(datetime(y, mo, d, h, mi, tzinfo=MX).timestamp() * 1000)


class Clock:
    def __init__(self, now):
        self.now = now

    def __call__(self):
        return self.now


@pytest.fixture
def env(tmp_path):
    conn = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(conn)
    clock = Clock(local_ms(2026, 9, 28, 9))
    return pomodoro.PomodoroStore(conn, clock), clock


def run_block(store, clock, rid, target_min, project="ComandOS", mode="focus", pause_min=0,
              extend_min=0, cancel_after=None):
    rev = store.snapshot()["revision"]
    block = store.command({"requestId": rid, "expectedRevision": rev, "action": "start", "mode": mode,
                           "targetMs": target_min * MIN, "project": project})["block"]
    if pause_min:
        clock.now += 5 * MIN
        store.command({"requestId": rid + "-p", "expectedRevision": None, "action": "pause"})
        clock.now += pause_min * MIN
        store.command({"requestId": rid + "-r", "expectedRevision": None, "action": "resume"})
    if extend_min:
        store.command({"requestId": rid + "-e", "expectedRevision": None, "action": "extend",
                       "deltaMs": extend_min * MIN})
    if cancel_after is not None:
        clock.now += cancel_after * MIN
        store.command({"requestId": rid + "-c", "expectedRevision": None, "action": "cancel"})
    else:
        clock.now = store.snapshot()["block"]["deadlineMs"]
        store.settle_due()
    clock.now += MIN
    return block["blockId"]


def test_report_separates_completed_cancelled_active_and_planned(env):
    store, clock = env
    run_block(store, clock, "a", 25, pause_min=40)           # pause never counts
    run_block(store, clock, "b", 25, extend_min=5)           # extension counts once measured
    run_block(store, clock, "c", 50, cancel_after=12)        # cancelled keeps its active time
    run_block(store, clock, "d", 5, mode="break")            # breaks are not work
    report = pomodoro.focus_report(store.conn, local_ms(2026, 9, 28), local_ms(2026, 9, 29))
    m = report["measured"]
    assert m["completed"] == {"blocks": 2, "activeMs": 55 * MIN, "plannedMs": 55 * MIN}
    assert m["cancelled"] == {"blocks": 1, "activeMs": 12 * MIN, "plannedMs": 50 * MIN}
    assert m["activeMs"] == 67 * MIN and m["plannedMs"] == 105 * MIN
    assert m["completionRate"] == pytest.approx(2 / 3)
    assert report["breaks"] == {"blocks": 1, "activeMs": 5 * MIN}
    day = [d for d in report["byDay"] if d["date"] == "2026-09-28"][0]
    assert day["activeMs"] == 67 * MIN and day["completed"] == 2 and day["cancelled"] == 1
    assert [h["mode"] for h in report["history"]].count("break") == 1


def test_local_midnight_uses_mexico_city_days(env):
    store, clock = env
    clock.now = local_ms(2026, 9, 28, 23, 50)                # 05:50 UTC next day
    run_block(store, clock, "late", 25)
    report = pomodoro.focus_report(store.conn, local_ms(2026, 9, 28), local_ms(2026, 9, 30))
    days = {d["date"]: d for d in report["byDay"]}
    assert days["2026-09-28"]["completed"] == 1
    assert days["2026-09-29"]["completed"] == 0
    assert report["range"]["timezone"] == "America/Mexico_City"


def test_project_filter_and_project_list(env):
    store, clock = env
    run_block(store, clock, "a", 25, project="ComandOS")
    run_block(store, clock, "b", 10, project="Lola")
    lo, hi = local_ms(2026, 9, 28), local_ms(2026, 9, 29)
    only = pomodoro.focus_report(store.conn, lo, hi, project="Lola")
    assert only["measured"]["activeMs"] == 10 * MIN
    assert {h["project"] for h in only["history"]} == {"Lola"}
    assert only["projects"] == ["ComandOS", "Lola"]
    by = {p["project"]: p for p in pomodoro.focus_report(store.conn, lo, hi)["byProject"]}
    assert by["ComandOS"]["activeMs"] == 25 * MIN and by["Lola"]["activeMs"] == 10 * MIN


def test_date_filter_excludes_other_days(env):
    store, clock = env
    run_block(store, clock, "a", 25)
    report = pomodoro.focus_report(store.conn, local_ms(2026, 9, 29), local_ms(2026, 9, 30))
    assert report["measured"]["activeMs"] == 0 and report["history"] == []


def test_running_block_is_not_counted_until_recorded(env):
    store, clock = env
    store.command({"requestId": "live", "expectedRevision": 0, "action": "start", "mode": "focus",
                   "targetMs": 25 * MIN})
    clock.now += 20 * MIN
    report = pomodoro.focus_report(store.conn, local_ms(2026, 9, 28), local_ms(2026, 9, 29))
    assert report["measured"]["activeMs"] == 0


def test_legacy_history_keeps_its_provenance(env):
    store, clock = env
    rows = [
        {"id": "old-1", "mode": "focus", "project": "MRP", "tmux_session": "s", "tmux_pane": "%1",
         "planned_minutes": 25, "started_at_ms": local_ms(2026, 9, 28, 7), "ended_at_ms": local_ms(2026, 9, 28, 7, 25),
         "status": "completed"},
        {"id": "old-2", "mode": "focus", "project": "MRP", "tmux_session": "s", "tmux_pane": "%1",
         "planned_minutes": 50, "started_at_ms": local_ms(2026, 9, 28, 8), "ended_at_ms": None, "status": "running"},
        {"id": "old-3", "mode": "break", "project": "", "tmux_session": "", "tmux_pane": "",
         "planned_minutes": 5, "started_at_ms": local_ms(2026, 9, 28, 8), "ended_at_ms": None, "status": "skipped"},
    ]
    assert store.import_legacy_history(rows) == 3
    assert store.import_legacy_history(rows) == 0            # idempotent
    report = pomodoro.focus_report(store.conn, local_ms(2026, 9, 28), local_ms(2026, 9, 29))
    assert report["measured"]["activeMs"] == 0, "planned minutes are never presented as measured"
    assert report["legacy"]["blocks"] == 2 and report["legacy"]["completedBlocks"] == 1
    assert report["legacy"]["plannedMs"] == 25 * MIN, "only completed legacy blocks add planned minutes"
    old = [h for h in report["history"] if h["blockId"] == "old-2"][0]
    assert old["provenance"] == "legacy-planned" and old["status"] == "unknown" and old["activeMs"] is None


def test_legacy_import_never_duplicates_an_adopted_block(env):
    store, clock = env
    until = (clock.now + 10 * MIN) / 1000
    store.import_legacy_focus({"blockId": "old-live", "until": until, "startedAt": until - 1500, "mins": 25})
    rows = [{"id": "old-live", "mode": "focus", "project": "", "tmux_session": "", "tmux_pane": "",
             "planned_minutes": 25, "started_at_ms": int(until * 1000) - 25 * MIN, "ended_at_ms": None,
             "status": "running"}]
    assert store.import_legacy_history(rows) == 0
    clock.now = int(until * 1000)
    assert store.settle_due() == ["old-live"]
    report = pomodoro.focus_report(store.conn, local_ms(2026, 9, 28), local_ms(2026, 9, 29))
    assert report["measured"]["completed"]["blocks"] == 1
