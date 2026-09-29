"""Turn lifecycle reconciliation: late, duplicate or foreign events never
move a pane backwards to an older turn."""

from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
from turn_state import reduce_turn, turns_from_events  # noqa: E402


def ev(kind, turn=None, process="p", at=None, correlation=None, evidence="confirmed", **extra):
    out = {"kind": kind, "turnId": turn, "processKey": process, "evidence": evidence,
           "correlation": correlation or ("source" if turn else "local"),
           "eventId": extra.pop("eventId", f"e-{kind}-{turn}-{at}")}
    if at is not None:
        out["occurredAtMs"] = at
    out.update(extra)
    return out


def test_completion_cannot_finish_a_newer_turn():
    current = {'turnId': 'new', 'state': 'working', 'processKey': 'p'}
    old = {'kind': 'turn_completed', 'turnId': 'old', 'processKey': 'p',
           'evidence': 'confirmed', 'correlation': 'source'}
    assert reduce_turn(current, old) == current


def test_full_cycle_with_source_ids():
    s = reduce_turn(None, ev("prompt_accepted", "t1", at=10))
    assert s["state"] == "working" and s["turnId"] == "t1"
    s = reduce_turn(s, ev("permission_requested", "t1", at=20, requestId="r1"))
    assert s["state"] == "awaiting_permission" and s["requestId"] == "r1"
    s = reduce_turn(s, ev("turn_completed", "t1", at=30))
    assert s["state"] == "completed" and s["evidence"] == "confirmed"


def test_duplicate_completion_is_a_noop():
    s = reduce_turn(reduce_turn(None, ev("prompt_accepted", "t1", at=10)), ev("turn_completed", "t1", at=30))
    assert reduce_turn(s, ev("turn_completed", "t1", at=31)) == s
    assert reduce_turn(s, ev("turn_failed", "t1", at=32)) == s


def test_late_start_of_a_finished_turn_does_not_reopen_it():
    s = reduce_turn(None, ev("turn_completed", "t1", at=30))
    assert reduce_turn(s, ev("prompt_accepted", "t1", at=10)) == s


def test_two_quick_turns_out_of_order():
    s = reduce_turn(None, ev("prompt_accepted", "a", at=10))
    s = reduce_turn(s, ev("prompt_accepted", "b", at=12))
    # a's completion arrives after b started
    s2 = reduce_turn(s, ev("turn_completed", "a", at=11))
    assert s2 == s and s2["state"] == "working" and s2["turnId"] == "b"
    s3 = reduce_turn(s2, ev("turn_completed", "b", at=20))
    assert s3["state"] == "completed"
    # an older start arriving late cannot replace the newer turn
    assert reduce_turn(s3, ev("prompt_accepted", "a", at=10)) == s3


def test_cancellation_and_failure():
    s = reduce_turn(None, ev("prompt_accepted", "t", at=1))
    assert reduce_turn(s, ev("turn_cancelled", "t", at=2))["state"] == "cancelled"
    assert reduce_turn(s, ev("turn_failed", "t", at=2))["state"] == "failed"


def test_waiting_is_not_permission_unless_declared():
    s = reduce_turn(None, ev("prompt_accepted", "t", at=1))
    assert reduce_turn(s, ev("input_requested", "t", at=2))["state"] == "awaiting_input"


def test_restart_new_process_adopts_new_turn_and_ignores_old_process():
    s = reduce_turn(None, ev("prompt_accepted", "t1", process="old", at=1))
    s = reduce_turn(s, ev("prompt_accepted", "t2", process="new", at=5))
    assert s["processKey"] == "new" and s["turnId"] == "t2"
    # the dead process' late completion does not finish the new turn
    assert reduce_turn(s, ev("turn_completed", "t1", process="old", at=6)) == s


def test_reused_pane_id_with_other_process_is_ignored():
    """%N reused: same tmux id, different process; its stray events are foreign."""
    s = reduce_turn(None, ev("prompt_accepted", "x", process="pid1-100", at=1))
    stray = ev("turn_completed", "x", process="pid9-900", at=2)
    assert reduce_turn(s, stray) == s


def test_process_death_closes_only_its_own_process():
    s = reduce_turn(None, ev("prompt_accepted", "t", process="p", at=1))
    assert reduce_turn(s, ev("pane_closed", process="other", at=2)) == s
    closed = reduce_turn(s, ev("pane_closed", process="p", at=2))
    assert closed["state"] == "closed"
    assert reduce_turn(s, ev("session_ended", process="p", at=2))["state"] == "ended"


def test_local_correlation_orders_by_time():
    s = reduce_turn(None, ev("prompt_accepted", at=100))
    assert s["turnId"].startswith("local:") and s["correlation"] == "local"
    # a completion that happened before this prompt belongs to an older turn
    assert reduce_turn(s, ev("turn_completed", at=90)) == s
    done = reduce_turn(s, ev("turn_completed", at=110))
    assert done["state"] == "completed" and done["correlation"] == "local"


def test_idle_input_request_after_completion_keeps_completion():
    s = reduce_turn(reduce_turn(None, ev("prompt_accepted", at=1)), ev("turn_completed", at=2))
    assert reduce_turn(s, ev("input_requested", at=60)) == s


def test_historical_and_inferred_events_do_not_move_state():
    s = reduce_turn(None, ev("prompt_accepted", "t", at=1))
    assert reduce_turn(s, ev("turn_completed", "t", at=2, evidence="historical")) == s
    assert reduce_turn(s, ev("turn_completed", "t", at=2, evidence="inferred")) == s


def test_unknown_kinds_are_ignored():
    s = reduce_turn(None, ev("prompt_accepted", "t", at=1))
    assert reduce_turn(s, {"kind": "focus_completed", "evidence": "confirmed"}) == s
    assert reduce_turn(s, {"kind": "news_edition"}) == s


def test_turns_are_kept_per_pane_even_in_the_same_project():
    events = [
        {**ev("prompt_accepted", "a", at=1), "paneKey": "pk1", "projectKey": "proj", "sequence": 1},
        {**ev("prompt_accepted", "b", at=2), "paneKey": "pk2", "projectKey": "proj", "sequence": 2},
        {**ev("turn_completed", "a", at=3), "paneKey": "pk1", "projectKey": "proj", "sequence": 3},
        {**ev("turn_completed", "old", at=0, evidence="historical"), "projectKey": "proj", "sequence": 4},
    ]
    turns = turns_from_events(events)
    assert turns["pane:pk1"]["state"] == "completed"
    assert turns["pane:pk2"]["state"] == "working"
    assert not any(k.startswith("project:") for k in turns), "history must not target a pane by project"
