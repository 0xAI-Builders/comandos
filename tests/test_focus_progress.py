"""Versioned personal focus progression: pure rules plus a reward ledger."""

from datetime import datetime
from pathlib import Path
import sqlite3
import sys
from zoneinfo import ZoneInfo

import pytest


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import app_state  # noqa: E402
import focus_progress as fp  # noqa: E402
import pomodoro  # noqa: E402

MIN = 60_000
MX = ZoneInfo("America/Mexico_City")
TRIAL = {"policyVersion": "trial", "xpPerMinute": 1, "xpPerLevel": 50, "dailyGoalMinutes": 30,
         "countCancelledActive": False, "timezone": "America/Mexico_City", "achievements": []}


def ms(y, mo, d, h=12, mi=0):
    return int(datetime(y, mo, d, h, mi, tzinfo=MX).timestamp() * 1000)


def blk(bid, day, minutes, status="completed", mode="focus", h=12):
    start = ms(2026, 9, day, h)
    return {"blockId": bid, "mode": mode, "status": status, "activeMs": minutes * MIN,
            "startedAtMs": start, "endedAtMs": start + minutes * MIN, "provenance": "measured"}


class Clock:
    def __init__(self, now):
        self.now = now

    def __call__(self):
        return self.now


class Emit:
    def __init__(self, fail=0):
        self.fail = fail
        self.items = []

    def __call__(self, conn, event):
        if self.fail:
            self.fail -= 1
            raise RuntimeError("crash before commit")
        self.items.append(event)


def test_policy_v1_is_the_decided_one():
    p = fp.POLICY_V1
    assert (p["policyVersion"], p["xpPerMinute"], p["xpPerLevel"], p["dailyGoalMinutes"]) == ("v1", 10, 1000, 100)
    assert p["countCancelledActive"] is True and p["timezone"] == "America/Mexico_City"
    assert {a["id"] for a in p["achievements"]} == {"first", "hundred", "streak3"}


def test_progress_rules():
    now = ms(2026, 9, 28, 20)
    blocks = [blk("a", 28, 25), blk("b", 28, 12, status="cancelled"), blk("c", 28, 5, mode="break"),
              blk("a", 28, 25)]  # duplicate completion of the same block
    p = fp.progress(blocks, fp.POLICY_V1, now)
    assert p["xp"] == 370 and p["focusMinutes"] == 37
    assert p["level"] == 1 and p["xpToNextLevel"] == 630 and p["levelPct"] == pytest.approx(37.0)
    assert p["todayMinutes"] == 37 and p["dailyGoalMinutes"] == 100
    assert p["completedBlocks"] == 1


def test_levels_and_policy_parameters():
    blocks = [blk(f"b{i}", 28, 50) for i in range(2)] + [blk("c", 28, 1)]
    assert fp.progress(blocks, fp.POLICY_V1, ms(2026, 9, 28, 23))["level"] == 2   # 1010 XP
    trial = fp.progress(blocks, TRIAL, ms(2026, 9, 28, 23))
    assert trial["xp"] == 101 and trial["level"] == 3
    only_cancelled = fp.progress([blk("x", 28, 30, status="cancelled")], TRIAL, ms(2026, 9, 28, 23))
    assert only_cancelled["xp"] == 0


def test_streaks_use_mexico_city_days_and_keep_achievements():
    late = blk("late", 25, 25, h=23)          # 23:00 local = next day in UTC
    blocks = [late, blk("d26", 26, 25), blk("d27", 27, 25)]
    p = fp.progress(blocks, fp.POLICY_V1, ms(2026, 9, 28, 9))
    assert p["streakDays"] == 3, "today without a block does not break the streak yet"
    assert p["maxStreakDays"] == 3
    later = fp.progress(blocks, fp.POLICY_V1, ms(2026, 10, 3, 9))
    assert later["streakDays"] == 0 and later["maxStreakDays"] == 3
    unlocked = {a["id"]: a["unlocked"] for a in later["achievements"]}
    assert unlocked == {"first": True, "hundred": False, "streak3": True}


def test_activation_excludes_history_and_filters_do_not_matter():
    before = blk("old", 20, 25)
    after = blk("new", 28, 25)
    p = fp.progress([before, after], fp.POLICY_V1, ms(2026, 9, 28, 20), activated_at_ms=ms(2026, 9, 27))
    assert p["xp"] == 250 and p["completedBlocks"] == 1
    legacy = dict(after, blockId="legacy", provenance="legacy-planned", activeMs=None)
    assert fp.progress([legacy], fp.POLICY_V1, ms(2026, 9, 28, 20))["xp"] == 0


@pytest.fixture
def store(tmp_path):
    conn = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(conn)
    clock = Clock(ms(2026, 9, 28, 9))
    fp.ensure_policy(conn, fp.POLICY_V1, clock.now)
    emit = Emit()
    s = pomodoro.PomodoroStore(conn, clock, emit=emit,
                               rewards=lambda c, record, now: fp.award(c, record, fp.POLICY_V1, now))
    s.test_clock, s.test_emit = clock, emit
    return s


def start(store, rid, minutes):
    return store.command({"requestId": rid, "expectedRevision": None, "action": "start", "mode": "focus",
                          "targetMs": minutes * MIN, "project": "ComandOS"})["block"]


def ledger(conn):
    return conn.execute("SELECT block_id, policy_version, xp, level_reached FROM focus_rewards "
                        "ORDER BY awarded_at_ms, block_id").fetchall()


def test_migration_five_and_single_activation(store):
    versions = {r[0] for r in store.conn.execute("SELECT version FROM schema_migrations")}
    assert {4, 5} <= versions
    first = fp.ensure_policy(store.conn, fp.POLICY_V1, store.test_clock.now + 999)
    assert first == ms(2026, 9, 28, 9), "re-activation keeps the original time"


def test_completion_awards_once_even_after_a_crash_and_retry(store):
    store.test_emit.fail = 1
    block = start(store, "a", 25)
    store.test_clock.now = block["deadlineMs"]
    with pytest.raises(RuntimeError):
        store.settle_due()
    assert ledger(store.conn) == [], "reward rolls back with the failed completion"
    store.settle_due()
    store.settle_due()
    assert ledger(store.conn) == [(block["blockId"], "v1", 250, None)]
    assert fp.award(store.conn, {"blockId": block["blockId"], "mode": "focus", "status": "completed",
                                 "activeMs": 25 * MIN, "startedAtMs": block["startedAtMs"],
                                 "endedAtMs": block["deadlineMs"], "provenance": "measured"},
                    fp.POLICY_V1, store.test_clock.now) is None


def test_cancelled_counts_only_its_active_time(store):
    start(store, "a", 50)
    store.test_clock.now += 12 * MIN
    store.command({"requestId": "c", "expectedRevision": None, "action": "cancel"})
    assert [row[2] for row in ledger(store.conn)] == [120]


def test_breaks_never_award(store):
    store.command({"requestId": "b", "expectedRevision": None, "action": "start", "mode": "break",
                   "targetMs": 5 * MIN})
    store.test_clock.now += 6 * MIN
    store.settle_due()
    assert ledger(store.conn) == []


def test_single_level_up_notice(store):
    for i in range(5):
        block = start(store, f"s{i}", 25)
        store.test_clock.now = block["deadlineMs"] + MIN
        store.settle_due()
    rows = ledger(store.conn)
    assert sum(r[2] for r in rows) == 1250
    assert [r[3] for r in rows] == [None, None, None, 2, None], "level 2 announced exactly once"
    prog = fp.ledger_progress(store.conn, fp.POLICY_V1, store.test_clock.now)
    assert prog["xp"] == 1250 and prog["level"] == 2
    assert prog["lastLevelUp"]["level"] == 2 and prog["lastLevelUp"]["blockId"] == rows[3][0]


def test_no_retroactive_points_for_blocks_before_activation(tmp_path):
    conn = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(conn)
    clock = Clock(ms(2026, 9, 1))
    s = pomodoro.PomodoroStore(conn, clock)
    block = s.command({"requestId": "a", "expectedRevision": 0, "action": "start", "mode": "focus",
                       "targetMs": 25 * MIN})["block"]
    clock.now = block["deadlineMs"]
    s.settle_due()
    s.import_legacy_history([{"id": "legacy", "mode": "focus", "planned_minutes": 25,
                              "started_at_ms": ms(2026, 8, 30), "status": "completed"}])
    fp.ensure_policy(conn, fp.POLICY_V1, ms(2026, 9, 28))
    assert fp.ledger_progress(conn, fp.POLICY_V1, ms(2026, 9, 28, 20))["xp"] == 0
    assert ledger(conn) == []


def test_policy_versions_have_separate_ledgers(store):
    fp.ensure_policy(store.conn, TRIAL, store.test_clock.now)
    block = start(store, "a", 25)
    store.test_clock.now = block["deadlineMs"]
    store.settle_due()
    record = {"blockId": block["blockId"], "mode": "focus", "status": "completed", "activeMs": 25 * MIN,
              "startedAtMs": block["startedAtMs"], "endedAtMs": block["deadlineMs"], "provenance": "measured"}
    store.conn.execute("BEGIN IMMEDIATE")
    trial = fp.award(store.conn, record, TRIAL, store.test_clock.now)
    store.conn.execute("COMMIT")
    assert trial["xp"] == 25
    assert sorted((r[1], r[2]) for r in ledger(store.conn)) == [("trial", 25), ("v1", 250)]


def test_ledger_progress_matches_pure_progress_and_ignores_filters(store):
    block = start(store, "a", 25)
    store.test_clock.now = block["deadlineMs"]
    store.settle_due()
    before = fp.ledger_progress(store.conn, fp.POLICY_V1, store.test_clock.now)
    pomodoro.focus_report(store.conn, 0, store.test_clock.now + 1, project="Otro")
    after = fp.ledger_progress(store.conn, fp.POLICY_V1, store.test_clock.now)
    assert before == after and after["xp"] == 250
