"""Closing a whole group: fixed identities, revalidation, honest partial results."""
from pathlib import Path
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import app_state  # noqa: E402
import workspace_state as ws  # noqa: E402


def leaf(t):
    return {"type": "tab", "tabId": t}


def split(a, b):
    return {"type": "split", "axis": "x", "ratio": 0.5, "first": a, "second": b}


def document():
    return {"schema": 1,
            "groups": [{"id": "g1", "tree": split(leaf("alpha"), split(leaf("beta"), leaf("local")))},
                       {"id": "g2", "tree": leaf("gamma")}],
            "tabs": {t: {"session": t, "label": t.title(), "paneKeys": []} for t in ("alpha", "beta", "local", "gamma")}}


@pytest.fixture
def store(tmp_path):
    conn = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(conn)
    s = ws.WorkspaceStore(conn)
    s.set_phase("ready")
    s.commit(0, document(), "seed")
    return s


LIVE = {"alpha": "$1", "beta": "$2", "local": "$0", "gamma": "$3"}


def identity(session):
    return LIVE.get(session)


def preview(store):
    return ws.close_group_preview(store.current()["document"], "g1", identity)


def test_preview_lists_every_member_with_its_live_identity(store):
    p = preview(store)
    assert [m["tabId"] for m in p["members"]] == ["alpha", "beta", "local"]
    assert [m["sessionId"] for m in p["members"]] == ["$1", "$2", "$0"]
    assert [m["kept"] for m in p["members"]] == [False, False, True]


def test_closes_only_the_confirmed_members_and_keeps_local(store):
    closed = []
    p = preview(store)
    result = ws.close_group(store, "g1", 1, p["members"], "r1", identity, lambda s: closed.append(s))
    assert closed == ["alpha", "beta"]
    assert result == {"ok": True, "closed": ["alpha", "beta"], "remaining": [], "kept": ["local"], "error": None}


def test_stale_revision_cancels_without_closing(store):
    closed = []
    p = preview(store)
    store.commit(1, document(), "other-device")
    with pytest.raises(ws.Conflict):
        ws.close_group(store, "g1", 1, p["members"], "r2", identity, closed.append)
    assert closed == []


def test_member_changed_during_confirmation_cancels_everything(store):
    closed = []
    p = preview(store)
    LIVE["beta"] = "$9"          # session recreated under the same name
    try:
        with pytest.raises(ValueError):
            ws.close_group(store, "g1", 1, p["members"], "r3", identity, closed.append)
    finally:
        LIVE["beta"] = "$2"
    assert closed == []


def test_member_set_mismatch_cancels(store):
    p = preview(store)
    with pytest.raises(ValueError):
        ws.close_group(store, "g1", 1, p["members"][:1], "r4", identity, lambda s: None)


def test_failure_after_a_close_reports_what_really_happened(store):
    closed = []

    def close(session):
        if session == "beta":
            return "tmux falló"
        closed.append(session)
    result = ws.close_group(store, "g1", 1, preview(store)["members"], "r5", identity, close)
    assert closed == ["alpha"]
    assert result["ok"] is False and result["closed"] == ["alpha"] and result["remaining"] == ["beta"]
    assert result["error"] == "tmux falló"


def test_repeated_request_returns_the_first_result_without_closing_again(store):
    calls = []
    p = preview(store)
    first = ws.close_group(store, "g1", 1, p["members"], "r6", identity, calls.append)
    again = ws.close_group(store, "g1", 99, p["members"], "r6", identity, calls.append)
    assert calls == ["alpha", "beta"] and again == first


def test_unknown_group_is_rejected(store):
    with pytest.raises(ValueError):
        ws.close_group_preview(store.current()["document"], "nope", identity)
