"""Pomodoro authority contract: one server-owned block, revisions, settlement."""

from pathlib import Path
import sqlite3
import sys
import threading

import pytest


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import app_state  # noqa: E402
import pomodoro  # noqa: E402

MIN = 60_000
T0 = 2_000_000_000_000


class Clock:
    def __init__(self, now=T0):
        self.now = now

    def __call__(self):
        return self.now


class Events:
    def __init__(self, fail=0):
        self.items = []
        self.fail = fail

    def __call__(self, conn, event):
        assert conn.in_transaction, "emit must run inside the settlement transaction"
        if self.fail:
            self.fail -= 1
            raise RuntimeError("simulated crash before commit")
        self.items.append(event)


def open_store(path, clock, events=None, **kw):
    conn = app_state.connect(path)
    app_state.migrate(conn)
    return pomodoro.PomodoroStore(conn, clock, emit=events, **kw)


@pytest.fixture
def env(tmp_path):
    clock = Clock()
    events = Events()
    store = open_store(tmp_path / "state.sqlite3", clock, events)
    return store, clock, events, tmp_path / "state.sqlite3"


def start(store, rid="r1", rev=None, target=25 * MIN, mode="focus", **extra):
    rev = store.snapshot()["revision"] if rev is None else rev
    req = {"requestId": rid, "expectedRevision": rev, "action": "start", "mode": mode,
           "targetMs": target, "project": "ComandOS", "sessionKey": "session-a", "paneKey": "pane-a"}
    req.update(extra)
    return store.command(req)


def act(store, action, rid, **extra):
    req = {"requestId": rid, "expectedRevision": store.snapshot()["revision"], "action": action}
    req.update(extra)
    return store.command(req)


def records(path):
    conn = sqlite3.connect(str(path))
    try:
        return conn.execute("SELECT block_id, status, active_ms, target_ms, ended_at_ms FROM pomodoro_records").fetchall()
    finally:
        conn.close()


def test_paused_time_does_not_count():
    block = {"status": "paused", "targetMs": 1500000, "activeMs": 120000, "resumedAtMs": None}
    assert pomodoro.elapsed_ms(block, 9000000) == 120000
    assert pomodoro.remaining_ms(block, 9000000) == 1380000


def test_elapsed_is_independent_of_ticks():
    block = {"status": "running", "targetMs": 25 * MIN, "activeMs": 2 * MIN, "resumedAtMs": 1000}
    assert pomodoro.elapsed_ms(block, 1000 + 3 * MIN) == 5 * MIN
    assert pomodoro.elapsed_ms(block, 10 ** 15) == 25 * MIN
    assert pomodoro.remaining_ms(block, 10 ** 15) == 0


def test_migration_is_version_four_and_starts_empty(env):
    store, clock, _, path = env
    conn = store.conn
    assert 4 in {row[0] for row in conn.execute("SELECT version FROM schema_migrations")}
    snap = store.snapshot()
    assert snap["block"] is None and snap["revision"] == 0 and snap["serverNowMs"] == clock.now


def test_start_returns_confirmed_contract(env):
    store, clock, _, _ = env
    result = start(store)
    block = result["block"]
    assert result["revision"] == 1 and result["serverNowMs"] == T0
    assert block["status"] == "running" and block["mode"] == "focus"
    assert block["targetMs"] == 25 * MIN and block["activeMs"] == 0
    assert block["resumedAtMs"] == T0 and block["deadlineMs"] == T0 + 25 * MIN
    assert (block["project"], block["sessionKey"], block["paneKey"]) == ("ComandOS", "session-a", "pane-a")
    assert block["blockId"]
    assert store.snapshot()["block"] == block


def test_get_does_not_mutate(env):
    store, clock, events, path = env
    start(store)
    clock.now += 30 * MIN
    before = store.snapshot()
    assert before["block"]["status"] == "running"
    assert store.snapshot()["revision"] == before["revision"]
    assert records(path) == [] and events.items == []


def test_pause_resume_extend_and_cancel(env):
    store, clock, _, path = env
    start(store)
    clock.now += 2 * MIN
    paused = act(store, "pause", "p1")["block"]
    assert paused["status"] == "paused" and paused["activeMs"] == 2 * MIN
    assert paused["deadlineMs"] is None and paused["resumedAtMs"] is None
    clock.now += 60 * MIN  # a long pause never counts
    resumed = act(store, "resume", "p2")["block"]
    assert resumed["deadlineMs"] == clock.now + 23 * MIN
    clock.now += MIN
    extended = act(store, "extend", "p3", deltaMs=5 * MIN)["block"]
    assert extended["targetMs"] == 30 * MIN
    assert extended["deadlineMs"] == clock.now + 27 * MIN
    clock.now += MIN
    cancelled = act(store, "cancel", "p4")["block"]
    assert cancelled["status"] == "cancelled" and cancelled["activeMs"] == 4 * MIN
    assert records(path) == [(cancelled["blockId"], "cancelled", 4 * MIN, 30 * MIN, clock.now)]


def test_extend_while_paused_and_bounds(env):
    store, clock, _, _ = env
    start(store)
    clock.now += 3 * MIN
    act(store, "pause", "p1")
    block = act(store, "extend", "p2", deltaMs=10 * MIN)["block"]
    assert block["targetMs"] == 35 * MIN and block["deadlineMs"] is None
    with pytest.raises(pomodoro.Invalid):
        act(store, "extend", "p3", deltaMs=-33 * MIN)  # would end before the active time
    with pytest.raises(pomodoro.Invalid):
        act(store, "extend", "p4", deltaMs=200 * MIN)  # beyond the product limit
    shrunk = act(store, "extend", "p5", deltaMs=-30 * MIN)["block"]
    assert shrunk["targetMs"] == 5 * MIN


def test_target_limits_match_product(env):
    store, _, _, _ = env
    for bad in (0, 30_000, 181 * MIN, "25", None):
        with pytest.raises(pomodoro.Invalid):
            start(store, rid=f"bad-{bad}", target=bad)
    assert start(store, rid="ok", target=180 * MIN)["block"]["targetMs"] == 180 * MIN


def test_invalid_mode_and_action(env):
    store, _, _, _ = env
    with pytest.raises(pomodoro.Invalid):
        start(store, mode="nap")
    with pytest.raises(pomodoro.Invalid):
        act(store, "finish", "x")
    with pytest.raises(pomodoro.Invalid):
        store.command({"expectedRevision": 0, "action": "start", "targetMs": MIN})


def test_second_start_is_an_explicit_conflict(env):
    store, _, _, _ = env
    first = start(store)
    with pytest.raises(pomodoro.Conflict) as exc:
        start(store, rid="r2", rev=first["revision"])
    assert exc.value.code == "active_block"
    assert exc.value.snapshot["block"]["blockId"] == first["block"]["blockId"]
    assert store.snapshot()["block"]["status"] == "running"


def test_request_id_replay_is_idempotent(env):
    store, _, _, _ = env
    first = start(store)
    again = start(store, rev=0)  # retried with the original revision
    assert again["replayed"] is True
    assert again["block"]["blockId"] == first["block"]["blockId"]
    assert again["revision"] == first["revision"]
    with pytest.raises(pomodoro.Conflict) as exc:
        start(store, rev=0, target=50 * MIN)  # same id, different body
    assert exc.value.code == "request_reused"


def test_stale_revision_is_rejected(env):
    store, clock, _, _ = env
    start(store)
    clock.now += MIN
    with pytest.raises(pomodoro.Conflict) as exc:
        store.command({"requestId": "late", "expectedRevision": 0, "action": "pause"})
    assert exc.value.code == "stale_revision"
    assert exc.value.snapshot["revision"] == 1


def test_scheduler_settles_once_with_record_and_event(env):
    store, clock, events, path = env
    block = start(store)["block"]
    clock.now = block["deadlineMs"] + 7_000
    assert store.settle_due() == [block["blockId"]]
    assert store.settle_due() == []
    snap = store.snapshot()["block"]
    assert snap["status"] == "completed" and snap["activeMs"] == 25 * MIN
    assert snap["endedAtMs"] == block["deadlineMs"]
    assert records(path) == [(block["blockId"], "completed", 25 * MIN, 25 * MIN, block["deadlineMs"])]
    assert len(events.items) == 1
    event = events.items[0]
    assert event["kind"] == "focus_completed" and event["source"] == "pomodoro"
    assert event["evidence"] == "confirmed" and event["correlation"] == "source"
    assert event["blockId"] == block["blockId"]
    assert (event["projectKey"], event["sessionKey"], event["paneKey"]) == ("ComandOS", "session-a", "pane-a")
    assert event["occurredAtMs"] == block["deadlineMs"] and event["receivedAtMs"] == clock.now
    assert event["eventId"] and event["title"] and "excerpt" in event


def test_breaks_complete_without_focus_event(env):
    store, clock, events, path = env
    block = start(store, mode="break", target=5 * MIN)["block"]
    clock.now += 6 * MIN
    assert store.settle_due() == [block["blockId"]]
    assert events.items == []
    assert records(path)[0][1] == "completed"


def test_command_settles_due_block_first(env):
    store, clock, events, _ = env
    block = start(store)["block"]
    clock.now = block["deadlineMs"] + 1
    with pytest.raises(pomodoro.Conflict) as exc:
        store.command({"requestId": "late-pause", "expectedRevision": None, "action": "pause"})
    assert exc.value.code == "not_active"
    assert exc.value.snapshot["block"]["status"] == "completed"
    assert len(events.items) == 1
    # After completion a new block can start without cancelling anything.
    nxt = start(store, rid="r2")
    assert nxt["block"]["blockId"] != block["blockId"]


def test_crash_before_commit_rolls_back_then_settles_once(tmp_path):
    clock = Clock()
    events = Events(fail=1)
    path = tmp_path / "state.sqlite3"
    store = open_store(path, clock, events)
    block = start(store)["block"]
    clock.now = block["deadlineMs"] + 1
    with pytest.raises(RuntimeError):
        store.settle_due()
    assert store.snapshot()["block"]["status"] == "running"
    assert records(path) == [] and events.items == []
    assert store.settle_due() == [block["blockId"]]
    assert len(records(path)) == 1 and len(events.items) == 1


def test_backend_restart_settles_overdue_block(tmp_path):
    clock = Clock()
    path = tmp_path / "state.sqlite3"
    first = open_store(path, clock, Events())
    block = start(first)["block"]
    first.conn.close()
    clock.now = block["deadlineMs"] + 3 * 3600_000  # backend was down for hours
    events = Events()
    second = open_store(path, clock, events)
    assert second.snapshot()["block"]["status"] == "running"
    assert second.settle_due() == [block["blockId"]]
    assert second.snapshot()["block"]["endedAtMs"] == block["deadlineMs"]
    assert len(events.items) == 1 and len(records(path)) == 1


def test_two_connections_settle_one_record(tmp_path):
    clock = Clock()
    path = tmp_path / "state.sqlite3"
    events = Events()
    a = open_store(path, clock, events)
    b = open_store(path, clock, events)
    block = start(a)["block"]
    clock.now = block["deadlineMs"]
    done = a.settle_due() + b.settle_due()
    assert done == [block["blockId"]]
    assert len(events.items) == 1 and len(records(path)) == 1


def test_simultaneous_starts_from_two_clients(tmp_path):
    clock = Clock()
    path = tmp_path / "state.sqlite3"
    open_store(path, clock).conn.close()
    results, errors = [], []
    barrier = threading.Barrier(4)

    def client(rid):
        store = open_store(path, clock)
        barrier.wait()
        try:
            results.append(start(store, rid=rid, rev=0))
        except pomodoro.Conflict as exc:
            errors.append(exc.code)
        finally:
            store.conn.close()

    threads = [threading.Thread(target=client, args=(f"c{i}",)) for i in range(4)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert len(results) == 1
    assert set(errors) <= {"active_block", "stale_revision"} and len(errors) == 3


def test_simultaneous_retries_of_same_request(tmp_path):
    clock = Clock()
    path = tmp_path / "state.sqlite3"
    open_store(path, clock).conn.close()
    out = []
    barrier = threading.Barrier(3)

    def client():
        store = open_store(path, clock)
        barrier.wait()
        out.append(start(store, rid="same", rev=0))
        store.conn.close()

    threads = [threading.Thread(target=client) for _ in range(3)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert len({r["block"]["blockId"] for r in out}) == 1
    assert sum(1 for r in out if not r["replayed"]) == 1


def test_attribution_is_fixed_at_start(env):
    store, clock, _, _ = env
    start(store)
    extended = act(store, "extend", "e1", deltaMs=MIN, project="Other", sessionKey="s2", paneKey="p2")["block"]
    assert (extended["project"], extended["sessionKey"], extended["paneKey"]) == ("ComandOS", "session-a", "pane-a")


def test_next_deadline_for_scheduler(env):
    store, clock, _, _ = env
    assert store.next_deadline_ms() is None
    block = start(store)["block"]
    assert store.next_deadline_ms() == block["deadlineMs"]
    act(store, "pause", "p")
    assert store.next_deadline_ms() is None


def test_legacy_focus_file_is_imported_once(env):
    store, clock, _, _ = env
    legacy = {"blockId": "legacy-1", "mode": "focus", "project": "Lola", "session": "s", "pane": "%3",
              "until": (T0 + 10 * MIN) / 1000, "startedAt": (T0 - 15 * MIN) / 1000, "mins": 25}
    assert store.import_legacy_focus(legacy) is True
    block = store.snapshot()["block"]
    assert block["blockId"] == "legacy-1" and block["status"] == "running"
    assert block["deadlineMs"] == T0 + 10 * MIN and block["targetMs"] == 25 * MIN
    assert block["project"] == "Lola" and block["source"] == "legacy-focus-file"
    assert store.import_legacy_focus(legacy) is False


def test_expired_or_invalid_legacy_focus_is_not_imported(env):
    store, clock, _, _ = env
    assert store.import_legacy_focus({"until": (T0 - 1000) / 1000, "startedAt": 1, "mins": 25}) is False
    assert store.import_legacy_focus({"until": "soon"}) is False
    assert store.import_legacy_focus(None) is False
    assert store.snapshot()["block"] is None


def test_revision_before_automatic_completion_stays_valid(env):
    store, clock, _, _ = env
    first = start(store)
    clock.now = first["block"]["deadlineMs"] + 1
    store.settle_due()
    # The client saw revision 1 (running); only the scheduler changed it since.
    nxt = start(store, rid="after-completion", rev=first["revision"])
    assert nxt["block"]["status"] == "running"
    # A human change in between invalidates the old basis again.
    act(store, "pause", "pz")
    with pytest.raises(pomodoro.Conflict) as exc:
        store.command({"requestId": "old", "expectedRevision": nxt["revision"], "action": "cancel"})
    assert exc.value.code == "stale_revision"
