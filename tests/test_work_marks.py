"""E1 work marks: independent per scope, favorite independent of the mark,
only a confirmed accepted prompt reopens Resuelto (D1)."""

import json
from pathlib import Path
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import app_state  # noqa: E402
import event_intake  # noqa: E402
import event_store  # noqa: E402
import work_marks as wm  # noqa: E402
from work_marks import mark_after_event  # noqa: E402


@pytest.fixture
def db(tmp_path):
    return tmp_path / "state.sqlite3"


@pytest.fixture
def conn(db):
    c = app_state.connect(db)
    app_state.migrate(c)
    yield c
    c.close()


def marks(conn):
    return {(r["scope"], r["key"]): (r["mark"], r["favorite"]) for r in wm.list_marks(conn)}


def test_accepted_prompt_only_reopens_resolved():
    event = {'kind': 'prompt_accepted', 'evidence': 'confirmed'}
    assert mark_after_event('resolved', event) == 'none'
    assert mark_after_event('frozen', event) == 'frozen'
    assert mark_after_event('awaiting_reply', event) == 'awaiting_reply'
    assert mark_after_event('resolved', {**event, 'evidence': 'inferred'}) == 'resolved'


def test_finishing_a_turn_never_assigns_a_mark():
    for kind in ("turn_completed", "turn_cancelled", "turn_failed", "permission_requested",
                 "input_requested", "turn_started"):
        for mark in wm.MARKS:
            assert mark_after_event(mark, {"kind": kind, "evidence": "confirmed"}) == mark


def test_migration_v3_creates_the_mark_tables(conn):
    assert app_state.schema_version(conn) >= 3
    tables = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    assert {"work_marks", "work_mark_applied"} <= tables


def test_new_pane_starts_without_mark(conn):
    row = wm.get_mark(conn, "pane", "pane-new")
    assert (row["mark"], row["favorite"], row["revision"]) == ("none", False, 0)


def test_session_mark_does_not_change_its_panes(conn):
    wm.set_mark(conn, "pane", "pk1", "frozen", 0)
    wm.set_mark(conn, "session", "sess-a", "resolved", 0)
    assert marks(conn) == {("pane", "pk1"): ("frozen", False), ("session", "sess-a"): ("resolved", False)}


def test_pane_mark_does_not_change_session_or_siblings(conn):
    """Two panes of the same project and two sessions stay independent."""
    wm.set_mark(conn, "session", "sess-a", "awaiting_reply", 0)
    wm.set_mark(conn, "pane", "pk1", "resolved", 0)
    assert wm.get_mark(conn, "pane", "pk2")["mark"] == "none"
    assert wm.get_mark(conn, "session", "sess-a")["mark"] == "awaiting_reply"
    assert wm.get_mark(conn, "session", "sess-b")["mark"] == "none"


def test_favorite_is_independent_of_the_mark(conn):
    wm.set_mark(conn, "pane", "pk1", "frozen", 0)
    row = wm.set_mark(conn, "pane", "pk1", {"favorite": True}, 1)
    assert (row["mark"], row["favorite"], row["revision"]) == ("frozen", True, 2)
    row = wm.set_mark(conn, "pane", "pk1", "none", 2)
    assert (row["mark"], row["favorite"]) == ("none", True)


def test_stale_revision_conflicts_with_current(conn):
    wm.set_mark(conn, "session", "s", "frozen", 0)
    with pytest.raises(wm.Conflict) as exc:
        wm.set_mark(conn, "session", "s", "resolved", 0)
    assert exc.value.current["mark"] == "frozen" and exc.value.current["revision"] == 1


def test_invalid_input(conn):
    with pytest.raises(ValueError):
        wm.set_mark(conn, "window", "k", "none", 0)
    with pytest.raises(ValueError):
        wm.set_mark(conn, "pane", "k", "done", 0)
    with pytest.raises(ValueError):
        wm.set_mark(conn, "pane", "", "none", 0)
    with pytest.raises(ValueError):
        wm.set_mark(conn, "pane", "k", {"favorite": "yes"}, 0)
    with pytest.raises(ValueError):
        wm.set_mark(conn, "pane", "k", "none", True)


def test_marks_survive_reload(conn, db):
    wm.set_mark(conn, "pane", "pk1", "awaiting_reply", 0)
    other = app_state.connect(db)
    try:
        assert wm.get_mark(other, "pane", "pk1")["mark"] == "awaiting_reply"
    finally:
        other.close()


def prompt(event_id="e1", **kw):
    return {"eventId": event_id, "kind": "prompt_accepted", "evidence": "confirmed",
            "paneKey": "pk1", "sessionKey": "sess-a", **kw}


def test_accepted_prompt_reopens_resolved_in_both_scopes_only_once(conn):
    wm.set_mark(conn, "pane", "pk1", "resolved", 0)
    wm.set_mark(conn, "session", "sess-a", "resolved", 0)
    changed = wm.apply_turn_event(conn, prompt())
    assert {(r["scope"], r["mark"]) for r in changed} == {("pane", "none"), ("session", "none")}
    # the user resolves again; replaying the same event must not reopen it
    wm.set_mark(conn, "pane", "pk1", "resolved", 2)
    assert wm.apply_turn_event(conn, prompt()) == []
    assert wm.get_mark(conn, "pane", "pk1")["mark"] == "resolved"


def test_blocking_marks_are_kept_on_new_activity(conn):
    wm.set_mark(conn, "pane", "pk1", "frozen", 0)
    wm.set_mark(conn, "session", "sess-a", "awaiting_reply", 0)
    assert wm.apply_turn_event(conn, prompt()) == []
    assert marks(conn) == {("pane", "pk1"): ("frozen", False), ("session", "sess-a"): ("awaiting_reply", False)}


def test_unconfirmed_or_other_events_do_not_touch_marks(conn):
    wm.set_mark(conn, "pane", "pk1", "resolved", 0)
    assert wm.apply_turn_event(conn, prompt("e2", evidence="inferred")) == []
    assert wm.apply_turn_event(conn, prompt("e3", kind="turn_completed")) == []
    assert wm.apply_turn_event(conn, prompt("e4", paneKey=None, sessionKey=None)) == []
    assert wm.get_mark(conn, "pane", "pk1")["mark"] == "resolved"


def test_apply_joins_the_caller_transaction(conn):
    wm.set_mark(conn, "pane", "pk1", "resolved", 0)
    conn.execute("BEGIN IMMEDIATE")
    wm.apply_turn_event(conn, prompt())
    assert conn.in_transaction
    conn.execute("ROLLBACK")
    assert wm.get_mark(conn, "pane", "pk1")["mark"] == "resolved"


def test_hook_intake_applies_marks_atomically_with_the_event(conn):
    doc = {"schema": 1, "groups": [{"id": "g", "tree": {"type": "tab", "tabId": "sess-a"}}],
           "tabs": {"sess-a": {"session": "sess-a", "paneKeys": ["pk1"]}},
           "bindings": {"pk1": {"paneId": "%3", "session": "sess-a"}}}
    conn.execute("INSERT INTO workspace_current VALUES (1, 1, ?, 0)", (json.dumps(doc),))
    wm.set_mark(conn, "pane", "pk1", "resolved", 0)
    stored = event_intake.record(conn, {"hookEvent": "UserPromptSubmit", "agent": "claude",
                                        "session": "sess-a", "pane": "%3", "occurredAtMs": 1})
    assert stored["paneKey"] == "pk1"
    assert wm.get_mark(conn, "pane", "pk1")["mark"] == "none"
    assert len(event_store.list_events(conn)) == 1


# ---- animation phases baked into standalone SVGs (desktop parity with the web loops) ----

def test_animated_icons_change_with_the_phase_and_static_ones_do_not():
    for name in wm.ANIMATED:
        frames = {wm.icon_svg(name, phase=i / 8) for i in range(8)}
        assert len(frames) >= 4, name          # resolved holds its check drawn for 40 % of the loop
        assert wm.icon_svg(name, phase=0) != wm.icon_svg(name, phase=0.2), name
        assert wm.CYCLE_S[name] > 0
    assert wm.icon_svg("none", phase=0.5) == wm.icon_svg("none")
    assert wm.icon_svg("resolved", phase=0.0) == wm.icon_svg("resolved", phase=1.0), "phase wraps"
    assert "wm-spin" not in wm.icon_svg("working", phase=0.25) and "rotate(90" in wm.icon_svg("working", phase=0.25)


def test_frame_index_quantizes_a_clock_into_the_icon_cycle():
    n = wm.frame_count("working")
    assert n >= 8
    assert wm.frame_index("working", 0.0) == 0
    assert wm.frame_index("working", wm.CYCLE_S["working"]) == 0, "one full cycle wraps"
    assert wm.frame_index("working", wm.CYCLE_S["working"] / 2) == n // 2
    assert wm.frame_count("frozen") > wm.frame_count("working"), "a slow loop needs more steps to stay smooth"
    assert wm.frame_count("none") == 1


def test_ai_status_uses_sprite_sets_of_one_character_per_state():
    """Grill 30-sep: lo que pone la IA va en su propio canal como sprites pixel
    del mismo personaje (ComandOS bot por default, burbujas elegibles); quieta no se anima."""
    import work_marks as wm
    assert wm.ai_status("working") == "work"
    assert wm.ai_status("awaiting_permission") == wm.ai_status("awaiting_input") == wm.ai_status("waiting") == "need"
    assert wm.ai_status("completed") == wm.ai_status("done") == "done"
    assert wm.ai_status("failed") == wm.ai_status("error") == "error"
    assert wm.ai_status(None) == wm.ai_status("ended") == wm.ai_status("cancelled") == "idle"
    assert wm.DEFAULT_AI_SET == "comandos" and set(wm.AI_SETS) == {"comandos", "kit"}
    assert wm.ai_set("nope") == "comandos" and wm.ai_set("kit") == "kit"
    for set_name, meta in wm.AI_SETS.items():
        for state in wm.AI_STATES:
            sp = wm.ai_sprite(state, set_name)
            png = ROOT / "dash" / sp["file"].lstrip("/")
            assert png.is_file(), f"{set_name}/{state}: strip shipped in dash/icons"
            assert sp["px"] == meta["px"] and sp["frames"] == meta["frames"][state]
            assert sp["tab"] % sp["px"] == 0 or sp["tab"] == sp["px"] or sp["px"] % sp["tab"] == 0, "hard pixels: integer scale"
        assert wm.ai_sprite("idle", set_name)["cycle"] == 0, "idle is still"
        assert wm.ai_sprite("work", set_name)["cycle"] > 0, "working moves"
    html = wm.ai_icon_html("need")
    assert 'class="ai-icon ai-sprite ai-need"' in html and "/icons/semaforos/comandos/need.png" in html and "--ai-frames:2" in html
    assert sorted({wm.ai_frame_index("work", t) for t in (0, 0.1, 0.2, 0.3)}) == [0, 1], "the strip steps through its frames"
    assert wm.ai_frame_index("idle", 0.3) == 0
    assert wm.STICKERS == {"frozen": "Aparcado", "awaiting_reply": "Esperando", "resolved": "Hecho"}


def test_ai_sprite_sets_are_identical_on_web_and_desktop():
    import re
    import work_marks as wm
    js = (ROOT / "dash" / "work-marks.js").read_text()
    for set_name, meta in wm.AI_SETS.items():
        block = re.search(set_name + r": \{(.*?)credit", js, re.S)
        assert block, f"{set_name}: dash/work-marks.js must carry the same set"
        assert f"px: {meta['px']}, tab: {meta['tab']}" in block.group(1)
        for state, n in meta["frames"].items():
            assert f"{state}: {n}" in block.group(1), f"{set_name}.{state} frames"
        for state, fps in meta["fps"].items():
            assert f"{state}: {fps}" in block.group(1), f"{set_name}.{state} fps"
    assert "const DEFAULT_AI_SET = 'comandos'" in js


