"""Workspace persistence contract: revisions, idempotency, restore by identity."""

import importlib.util
import json
from pathlib import Path
import sqlite3
import sys

import pytest


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import app_state  # noqa: E402
import workspace_state as ws  # noqa: E402


def raw_leaves(node):
    if node.get("type") == "tab":
        return [node.get("tabId")]
    return raw_leaves(node["first"]) + raw_leaves(node["second"])


def doc(*groups, tabs=None, bindings=None):
    tabs = tabs if tabs is not None else {}
    for group in groups:
        for tab in raw_leaves(group["tree"]):
            tabs.setdefault(tab, {"session": tab, "paneKeys": ["pk-" + tab]})
    out = {"schema": 1, "groups": list(groups), "tabs": tabs}
    if bindings is not None:
        out["bindings"] = bindings
    return out


def leaf(tab):
    return {"type": "tab", "tabId": tab}


def split(first, second, axis="x", ratio=0.5):
    return {"type": "split", "axis": axis, "ratio": ratio, "first": first, "second": second}


def group(gid, tree):
    return {"id": gid, "tree": tree}


@pytest.fixture
def store(tmp_path):
    conn = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(conn)
    s = ws.WorkspaceStore(conn)
    s.set_phase("ready")
    yield s
    conn.close()


def test_connect_uses_manual_transactions_and_foreign_keys(tmp_path):
    conn = app_state.connect(tmp_path / "state.sqlite3")
    assert conn.isolation_level is None
    assert conn.execute("PRAGMA foreign_keys").fetchone()[0] == 1
    assert conn.execute("PRAGMA busy_timeout").fetchone()[0] > 0


def test_default_path_honours_xdg_state_home(tmp_path, monkeypatch):
    monkeypatch.setenv("XDG_STATE_HOME", str(tmp_path))
    assert app_state.default_path() == tmp_path / "comandos" / "app-state.sqlite3"


def test_migrate_is_idempotent_and_backs_up_existing_database(tmp_path):
    path = tmp_path / "state.sqlite3"
    conn = app_state.connect(path)
    conn.execute("CREATE TABLE user_data (x)")
    conn.execute("INSERT INTO user_data VALUES (1)")
    app_state.migrate(conn)
    version = app_state.schema_version(conn)
    app_state.migrate(conn)
    assert app_state.schema_version(conn) == version
    backups = list(tmp_path.glob("state.sqlite3.pre-migration-*"))
    assert len(backups) == 1
    copy = sqlite3.connect(backups[0])
    assert copy.execute("SELECT x FROM user_data").fetchall() == [(1,)]


def test_failed_migration_rolls_back(tmp_path, monkeypatch):
    conn = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(conn)
    before = app_state.schema_version(conn)
    monkeypatch.setattr(app_state, "MIGRATIONS", app_state.MIGRATIONS + [
        (before + 1, "broken", "CREATE TABLE good_part (x); THIS IS NOT SQL;")])
    with pytest.raises(sqlite3.Error):
        app_state.migrate(conn)
    assert app_state.schema_version(conn) == before
    assert not conn.execute("SELECT 1 FROM sqlite_master WHERE name='good_part'").fetchone()


def test_migrations_merged_late_are_applied_below_the_current_version(tmp_path, monkeypatch):
    # Branches assign versions ahead of time; a database may reach v7 before
    # v2..v6 land. Those must still run instead of being skipped.
    conn = app_state.connect(tmp_path / "state.sqlite3")
    monkeypatch.setattr(app_state, "MIGRATIONS", [m for m in app_state.MIGRATIONS if m[0] in (1, 7)])
    app_state.migrate(conn)
    monkeypatch.setattr(app_state, "MIGRATIONS", app_state.MIGRATIONS + [(3, "late", "CREATE TABLE late_table (x)")])
    app_state.migrate(conn)
    assert conn.execute("SELECT 1 FROM sqlite_master WHERE name='late_table'").fetchone()
    assert {r[0] for r in conn.execute("SELECT version FROM schema_migrations")} >= {1, 3, 7}


def test_first_start_has_no_document(store):
    assert store.current() is None


def test_commit_and_reopen(tmp_path):
    path = tmp_path / "state.sqlite3"
    conn = app_state.connect(path)
    app_state.migrate(conn)
    s = ws.WorkspaceStore(conn)
    s.set_phase("ready")
    d = doc(group("g1", split(leaf("a"), leaf("b"), ratio=0.6)))
    result = s.commit(0, d, "req-1")
    assert result["revision"] == 1
    conn.close()
    conn = app_state.connect(path)
    app_state.migrate(conn)
    again = ws.WorkspaceStore(conn).current()
    assert again["revision"] == 1
    assert again["document"]["groups"][0]["tree"]["ratio"] == 0.6


def test_stale_revision_conflicts_without_overwriting(store):
    store.commit(0, doc(group("g1", leaf("a"))), "r1")
    store.commit(1, doc(group("g1", leaf("a")), group("g2", leaf("b"))), "r2")
    with pytest.raises(ws.Conflict) as err:
        store.commit(1, doc(group("g9", leaf("z"))), "r3")
    assert err.value.current["revision"] == 2
    assert store.current()["document"]["groups"][1]["id"] == "g2"


def test_repeated_request_returns_same_result(store):
    d = doc(group("g1", leaf("a")))
    first = store.commit(0, d, "same")
    second = store.commit(0, d, "same")
    assert first == second
    assert store.current()["revision"] == 1


def test_reused_request_id_with_other_content_conflicts(store):
    store.commit(0, doc(group("g1", leaf("a"))), "same")
    with pytest.raises(ws.Conflict):
        store.commit(1, doc(group("g1", leaf("b"))), "same")


@pytest.mark.parametrize("bad", [
    doc(group("g1", split(leaf("a"), leaf("a")))),
    {"schema": 1, "groups": [group("g1", leaf("a"))], "tabs": {"a": {"session": "a", "paneKeys": []}, "orphan": {"session": "o", "paneKeys": []}}},
    doc(group("g1", split(leaf("a"), leaf("b"), ratio=1))),
    doc(group("g1", split(leaf("a"), leaf("b"), ratio=True))),
    doc(group("g1", split(leaf("a"), leaf("b"), axis="z"))),
    {"schema": 2, "groups": [], "tabs": {}},
    doc(group("g1", leaf("a")), group("g1", leaf("b"))),
    doc(group("g1", {"type": "tab", "tabId": ""})),
])
def test_invalid_documents_are_rejected(store, bad):
    with pytest.raises(ValueError):
        store.commit(0, bad, "bad")
    assert store.current() is None


def test_depth_limit(store):
    tree = leaf("t0")
    for i in range(1, 70):
        tree = split(tree, leaf(f"t{i}"))
    with pytest.raises(ValueError):
        store.commit(0, doc(group("g", tree)), "deep")


def test_pane_key_shared_by_two_tabs_is_rejected(store):
    d = doc(group("g1", split(leaf("a"), leaf("b"))),
            tabs={"a": {"session": "a", "paneKeys": ["p"]}, "b": {"session": "b", "paneKeys": ["p"]}})
    with pytest.raises(ValueError):
        store.commit(0, d, "dup")


def test_truncated_current_falls_back_to_previous(store):
    store.commit(0, doc(group("g1", leaf("a"))), "r1")
    store.commit(1, doc(group("g1", leaf("a")), group("g2", leaf("b"))), "r2")
    store.conn.execute("UPDATE workspace_current SET document = substr(document, 1, 20)")
    recovered = store.current()
    assert recovered["revision"] == 1
    assert recovered["recovered"] is True


def test_failure_inside_commit_keeps_previous_state(store, monkeypatch):
    store.commit(0, doc(group("g1", leaf("a"))), "r1")
    real = store._write_current

    def explode(*args):
        real(*args)
        raise OSError("disk full")
    monkeypatch.setattr(store, "_write_current", explode)
    with pytest.raises(OSError):
        store.commit(1, doc(group("g2", leaf("b"))), "r2")
    assert store.current()["document"]["groups"][0]["id"] == "g1"
    assert store.current()["revision"] == 1


def test_automatic_save_requires_ready_phase(tmp_path):
    conn = app_state.connect(tmp_path / "s.sqlite3")
    app_state.migrate(conn)
    s = ws.WorkspaceStore(conn)
    assert s.phase == "restoring"
    with pytest.raises(ws.NotReady):
        s.commit(0, doc(group("g1", leaf("a"))), "auto", reason="auto")
    s.set_phase("failed")
    with pytest.raises(ws.NotReady):
        s.commit(0, doc(group("g1", leaf("a"))), "auto", reason="auto")
    assert s.commit(0, doc(group("g1", leaf("a"))), "user", reason="user")["revision"] == 1


def test_empty_inventory_never_replaces_last_state_automatically(store):
    store.commit(0, doc(group("g1", leaf("a"))), "r1")
    with pytest.raises(ws.EmptyInventory):
        store.commit(1, doc(), "auto-empty", reason="auto")
    assert store.current()["document"]["groups"]


def test_human_close_can_persist_empty_workspace(store):
    store.commit(0, doc(group("g1", leaf("a"))), "r1")
    result = store.commit(1, doc(), "closed-last", reason="user")
    assert result["document"]["groups"] == []
    assert store.current()["document"]["tabs"] == {}


def test_client_state_is_separate_from_layout(store):
    store.commit(0, doc(group("g1", split(leaf("a"), leaf("b")))), "r1")
    store.save_client("phone", {"activeTabId": "b", "activePaneKey": "pk-b", "drafts": {"pk-b": "hola"}, "readingAnchors": {}})
    store.save_client("desk", {"activeTabId": "a", "activePaneKey": "pk-a", "drafts": {}, "readingAnchors": {}})
    assert store.client("phone")["activeTabId"] == "b"
    assert store.client("desk")["activeTabId"] == "a"
    assert store.current()["revision"] == 1
    with pytest.raises(ValueError):
        store.save_client("", {})
    with pytest.raises(ValueError):
        store.save_client("x", {"drafts": {"k": "x" * 300_000}})


# ---- restore by identity -------------------------------------------------

def binding(**kw):
    base = {"server": "srv", "generation": "gen1", "session": "alpha", "paneId": "%3",
            "pid": 100, "startTime": 555, "conversation": {"agent": "claude", "id": "conv-1"}}
    base.update(kw)
    return base


def test_restore_attaches_live_identity_without_resuming():
    d = doc(group("g1", leaf("a")), tabs={"a": {"session": "alpha", "paneKeys": ["pk"]}},
            bindings={"pk": binding()})
    calls = []
    out = ws.restore_workspace(d, inspect=lambda b: {"pid": 100, "startTime": 555},
                               resume_exact=lambda b: calls.append(b))
    assert out == {"attached": ["pk"], "resumed": {}, "unavailable": []}
    assert calls == []


def test_reused_pane_id_does_not_inherit_identity():
    d = doc(group("g1", leaf("a")), tabs={"a": {"session": "alpha", "paneKeys": ["pk"]}},
            bindings={"pk": binding()})
    out = ws.restore_workspace(d, inspect=lambda b: {"pid": 999, "startTime": 1},
                               resume_exact=lambda b: {**b, "paneId": "%8", "pid": 7})
    assert out["attached"] == []
    assert out["resumed"]["pk"]["paneId"] == "%8"


def test_missing_conversation_is_marked_unavailable_not_replaced():
    d = doc(group("g1", leaf("a")), tabs={"a": {"session": "alpha", "paneKeys": ["pk", "pk2"]}},
            bindings={"pk": binding(conversation=None), "pk2": binding(paneId="%4")})
    resumed = []
    out = ws.restore_workspace(d, inspect=lambda b: None,
                               resume_exact=lambda b: resumed.append(b["paneId"]) or None)
    assert out["unavailable"] == ["pk", "pk2"]
    # Only the pane with an exact conversation id is ever resumed.
    assert resumed == ["%4"]


def test_resume_failure_keeps_place():
    d = doc(group("g1", leaf("a")), tabs={"a": {"session": "alpha", "paneKeys": ["pk"]}},
            bindings={"pk": binding()})

    def boom(b):
        raise RuntimeError("resume failed")
    out = ws.restore_workspace(d, inspect=lambda b: None, resume_exact=boom)
    assert out["unavailable"] == ["pk"]


# ---- reconcile + legacy migration -----------------------------------------

def test_reconcile_keeps_arrangement_adds_and_removes_tabs():
    d = doc(group("g1", split(leaf("a"), leaf("b"), ratio=0.3)), group("g2", leaf("c")))
    out = ws.reconcile(d, [("a", "A"), ("c", "C"), ("d", "D")])
    assert [ws.tab_ids(g["tree"]) for g in out["groups"]] == [["a"], ["c"], ["d"]]
    assert out["tabs"]["d"]["session"] == "d"
    out2 = ws.reconcile(d, [("a", "A"), ("b", "B"), ("c", "C")])
    assert out2["groups"][0]["tree"]["ratio"] == 0.3


def test_reconcile_is_stable_for_an_already_fitting_document():
    d = doc(group("g1", split(leaf("a"), leaf("b"))))
    assert ws.reconcile(d, [("a", None), ("b", None)]) == d


def test_reconcile_publishes_pane_keys_and_process_identity():
    d = doc(group("g1", leaf("a")), tabs={"a": {"session": "a", "paneKeys": ["old"]}})
    snap = {"windows": [{"panes": [
        {"id": "%1", "pid": 5, "start": 50, "key": "k1", "agent": "codex", "resume_id": "c1"},
        {"id": "%2", "pid": 6, "start": 60, "key": "k2"}]}]}
    out = ws.reconcile(d, [("a", None)], {"a": ws.pane_bindings(snap)})
    assert out["tabs"]["a"]["paneKeys"] == ["k1", "k2"]
    assert out["bindings"]["k1"] == {"paneId": "%1", "pid": 5, "startTime": 50, "session": "a",
                                     "conversation": {"agent": "codex", "id": "c1"}}
    assert out["bindings"]["k2"]["conversation"] is None
    # A missing capture keeps the last known panes rather than emptying the tab.
    assert ws.reconcile(out, [("a", None)], {"a": []})["tabs"]["a"]["paneKeys"] == ["k1", "k2"]
