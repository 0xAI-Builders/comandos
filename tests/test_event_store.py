"""Event identity contract: persisted before delivery, idempotent by source
identity, paginated by a local monotonic sequence."""

import json
from pathlib import Path
import sqlite3
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import app_state  # noqa: E402
import event_store  # noqa: E402
import event_intake  # noqa: E402


@pytest.fixture
def conn(tmp_path):
    c = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(c)
    yield c
    c.close()


def base(**kw):
    out = {"source": "codex-hook", "harness": "codex", "projectKey": "project-a",
           "sessionKey": "session-a", "paneKey": "pane-a", "processKey": "123-456",
           "conversationId": "conversation-a", "turnId": "turn-a", "kind": "turn_completed",
           "evidence": "confirmed", "correlation": "source", "occurredAtMs": 1000,
           "title": "Terminó el turno", "excerpt": "Respuesta disponible"}
    out.update(kw)
    return out


def receipts(conn):
    return conn.execute("SELECT COUNT(*) FROM event_receipts").fetchone()[0]


def test_migration_creates_event_tables(conn):
    assert app_state.schema_version(conn) >= 2
    tables = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    assert {"events", "event_receipts", "deliveries"} <= tables


def test_append_assigns_identity_and_monotonic_sequence(conn):
    a = event_store.append_event(conn, base(turnId="t1"))
    b = event_store.append_event(conn, base(turnId="t2"))
    assert a["eventId"].startswith("event-") and a["eventId"] != b["eventId"]
    assert b["sequence"] > a["sequence"]
    assert a["receivedAtMs"] >= 1000 and a["occurredAtMs"] == 1000
    assert a["duplicate"] is False


def test_source_event_id_is_idempotent_and_keeps_every_receipt(conn):
    a = event_store.append_event(conn, base(sourceEventId="src-1", turnId=None, correlation="unknown"))
    b = event_store.append_event(conn, base(sourceEventId="src-1", turnId=None, correlation="unknown",
                                            excerpt="otra copia"))
    assert (b["eventId"], b["sequence"]) == (a["eventId"], a["sequence"])
    assert b["duplicate"] is True and b["excerpt"] == "Respuesta disponible"
    assert len(event_store.list_events(conn)) == 1
    assert receipts(conn) == 2


def test_confirmed_cycle_key_deduplicates_two_channels_of_the_same_turn(conn):
    a = event_store.append_event(conn, base(source="codex-hook"))
    b = event_store.append_event(conn, base(source="codex-notify"))
    assert a["eventId"] == b["eventId"] and b["duplicate"]
    # same turn, another lifecycle step is a different event
    c = event_store.append_event(conn, base(kind="prompt_accepted"))
    assert c["eventId"] != a["eventId"]


def test_ambiguous_receptions_are_never_merged(conn):
    """Two local completions with equal text/time stay two receptions."""
    kw = dict(turnId=None, correlation="local", evidence="confirmed")
    a = event_store.append_event(conn, base(**kw))
    b = event_store.append_event(conn, base(**kw))
    assert a["eventId"] != b["eventId"]
    # same text, two distinct turns: two events
    c = event_store.append_event(conn, base(turnId="x1"))
    d = event_store.append_event(conn, base(turnId="x2"))
    assert c["eventId"] != d["eventId"]


def test_append_never_commits_on_its_own(conn):
    conn.execute("BEGIN IMMEDIATE")
    event_store.append_event(conn, base())
    assert conn.in_transaction
    conn.execute("ROLLBACK")
    assert event_store.list_events(conn) == []


def test_append_joins_a_caller_transaction(conn):
    conn.execute("BEGIN IMMEDIATE")
    ev = event_store.append_event(conn, base(kind="focus_completed", source="pomodoro",
                                             sourceEventId="focus:cycle-1", turnId=None,
                                             correlation="source"))
    conn.execute("COMMIT")
    assert [e["eventId"] for e in event_store.list_events(conn)] == [ev["eventId"]]


def test_list_events_pages_after_sequence(conn):
    seqs = [event_store.append_event(conn, base(turnId=f"t{i}"))["sequence"] for i in range(5)]
    page = event_store.list_events(conn, after_sequence=seqs[1], limit=2)
    assert [e["sequence"] for e in page] == seqs[2:4]
    assert event_store.list_events(conn, after_sequence=seqs[-1]) == []
    assert len(event_store.list_events(conn, limit=10_000)) == 5


def test_validation(conn):
    with pytest.raises(ValueError):
        event_store.append_event(conn, base(kind="made_up"))
    with pytest.raises(ValueError):
        event_store.append_event(conn, base(evidence="sure"))
    with pytest.raises(ValueError):
        event_store.append_event(conn, base(source=""))
    long = event_store.append_event(conn, base(excerpt="x" * 5000, title="y" * 900))
    assert len(long["excerpt"]) <= event_store.MAX_EXCERPT and len(long["title"]) <= event_store.MAX_TITLE


def test_destination_is_reported(conn):
    assert event_store.append_event(conn, base())["destination"] == "pane"
    assert event_store.append_event(conn, base(paneKey=None, turnId="s"))["destination"] == "session"
    assert event_store.append_event(conn, base(paneKey=None, sessionKey=None, turnId="n"))["destination"] == "none"


def test_delivery_claim_is_unique_per_event_and_channel(conn):
    ev = event_store.append_event(conn, base())
    assert event_store.claim_delivery(conn, ev["eventId"], "audio") is True
    assert event_store.claim_delivery(conn, ev["eventId"], "audio") is False
    assert event_store.claim_delivery(conn, ev["eventId"], "push", "android-1") is True
    with pytest.raises(sqlite3.IntegrityError):
        event_store.claim_delivery(conn, "event-missing", "audio")


# ---- hook intake ----------------------------------------------------------

def hook(**kw):
    out = {"hookEvent": "Stop", "agent": "claude", "cwd": "/tmp/proj", "project": "proj",
           "session": "sess", "pane": "%3", "occurredAtMs": 5000}
    out.update(kw)
    return out


def test_intake_maps_hook_events_without_inventing_permissions():
    n = event_intake.normalize_hook
    assert n(hook(hookEvent="UserPromptSubmit"))["kind"] == "prompt_accepted"
    assert n(hook(hookEvent="Stop"))["kind"] == "turn_completed"
    assert n(hook(hookEvent="Notification"))["kind"] == "input_requested"
    assert n(hook(hookEvent="Notification", notificationType="idle_prompt"))["kind"] == "input_requested"
    assert n(hook(hookEvent="Notification", notificationType="permission_prompt"))["kind"] == "permission_requested"
    assert n(hook(hookEvent="PermissionRequest", agent="codex", requestId="call-9"))["kind"] == "permission_requested"
    assert n(hook(hookEvent="GrokError", agent="grok"))["kind"] == "turn_failed"
    assert n(hook(hookEvent="GrokCancelled", agent="grok"))["kind"] == "turn_cancelled"
    assert n(hook(hookEvent="SessionEnd"))["kind"] == "session_ended"
    assert n(hook(hookEvent="GrokIdle")) is None


def test_intake_keeps_provider_identity():
    e = event_intake.normalize_hook(hook(agent="codex", conversationId="thread-1", turnId="turn-7",
                                         hookEvent="Stop"))
    assert (e["harness"], e["conversationId"], e["turnId"]) == ("codex", "thread-1", "turn-7")
    assert e["correlation"] == "source" and e["evidence"] == "confirmed"
    assert e["sessionKey"] == "sess" and e["paneId"] == "%3" and e["projectKey"] == "proj"
    grok = event_intake.normalize_hook(hook(agent="grok", promptId="p-1", hookEvent="Stop"))
    assert grok["turnId"] == "p-1" and grok["correlation"] == "source"
    claude = event_intake.normalize_hook(hook(conversationId="cs-1"))
    assert claude["turnId"] is None and claude["correlation"] == "local"
    anon = event_intake.normalize_hook(hook(session="", pane=""))
    assert anon["correlation"] == "unknown" and anon["sessionKey"] is None


def test_intake_rejects_bad_identifiers():
    e = event_intake.normalize_hook(hook(pane="%3; rm -rf /", session="a b", panePid="12x"))
    assert e["paneId"] is None and e["sessionKey"] is None and e["processKey"] is None


def workspace(conn, bindings):
    doc = {"schema": 1, "groups": [{"id": "g", "tree": {"type": "tab", "tabId": "sess"}}],
           "tabs": {"sess": {"session": "sess", "paneKeys": list(bindings)}}, "bindings": bindings}
    conn.execute("INSERT INTO workspace_current VALUES (1, 1, ?, 0)", (json.dumps(doc),))


def test_record_resolves_pane_key_by_process_and_rejects_reused_pane_id(conn, monkeypatch):
    workspace(conn, {"pk-live": {"paneId": "%3", "pid": 123, "startTime": 456, "session": "sess"}})
    monkeypatch.setattr(event_intake, "process_start_time", lambda pid: 456 if int(pid) == 123 else 999)
    ok = event_intake.record(conn, hook(panePid="123"))
    assert ok["paneKey"] == "pk-live" and ok["processKey"] == "123-456"
    reused = event_intake.record(conn, hook(panePid="777"))
    assert reused["paneKey"] is None and reused["sessionKey"] == "sess"
    assert not conn.in_transaction


def test_legacy_timeline_imports_once_without_destination(conn, tmp_path):
    legacy = tmp_path / "events.jsonl"
    rows = [{"project": "proj", "status": "done", "detail": "ok", "ts": 100},
            {"project": "proj", "status": "waiting", "detail": "¿permiso?", "ts": 101},
            {"project": "proj", "status": "done", "detail": "ok", "ts": 100}]
    legacy.write_text("".join(json.dumps(r) + "\n" for r in rows) + "not json\n")
    assert event_intake.import_legacy(conn, legacy) == 3
    evs = event_store.list_events(conn)
    assert [e["kind"] for e in evs] == ["turn_completed", "input_requested", "turn_completed"]
    assert all(e["evidence"] == "historical" and e["destination"] == "none" for e in evs)
    assert all(e["paneKey"] is None and e["sessionKey"] is None for e in evs)
    assert evs[0]["occurredAtMs"] == 100_000 and evs[0]["projectKey"] == "proj"
    legacy.write_text(legacy.read_text() + json.dumps({"project": "p2", "status": "done", "ts": 5}) + "\n")
    assert event_intake.import_legacy(conn, legacy) == 0
    assert len(event_store.list_events(conn)) == 3


def test_fixture_payloads_keep_the_declared_identities():
    fixtures = json.loads((ROOT / "tests" / "fixtures" / "hook_payloads.json").read_text())
    for harness, item in fixtures.items():
        if harness.startswith("_"):
            continue
        event = event_intake.normalize_hook(item["intake"])
        for key, value in item["expected"].items():
            assert event[key] == value, (harness, key, event[key])
        assert event["conversationId"] == item["intake"]["conversationId"]


def test_legacy_import_skips_lines_already_recorded_as_n1_events(conn, tmp_path):
    live = event_store.append_event(conn, base(occurredAtMs=50_500))
    legacy = tmp_path / "events.jsonl"
    legacy.write_text("".join(json.dumps(r) + "\n" for r in [
        {"project": "proj", "status": "done", "ts": 49},
        {"project": "proj", "status": "done", "ts": 50},
        {"project": "proj", "status": "done", "ts": 51}]))
    assert event_intake.import_legacy(conn, legacy) == 1
    assert [e["occurredAtMs"] for e in event_store.list_events(conn)] == [50_500, 49_000]
    assert live["evidence"] == "confirmed"
