#!/usr/bin/env python3
import os
import sqlite3
import tempfile
from pathlib import Path

import importlib.util


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("cc_usage", ROOT / "bin" / "cc_usage.py")
cc_usage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cc_usage)


def test_usage_db_path_lives_under_hooks_dir():
    assert cc_usage.usage_db_path("/tmp/hooks") == "/tmp/hooks/comandos-usage.sqlite"


def test_init_db_creates_required_tables():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        con = sqlite3.connect(db)
        tables = {r[0] for r in con.execute("select name from sqlite_master where type='table'")}
        assert {
            "usage_panes",
            "usage_turns",
            "provider_usage_buckets",
            "provider_cost_buckets",
            "usage_reconciliation",
            "usage_alerts",
            "model_presets",
            "usage_settings",
        }.issubset(tables)


def test_git_root_for_path_uses_git_when_available_and_falls_back():
    calls = []

    def fake_run(args, cwd=None, timeout=3):
        calls.append((tuple(args), cwd))

        class R:
            returncode = 0
            stdout = "/repo\n"
            stderr = ""

        return R()

    assert cc_usage.git_root_for_path("/repo/app", run=fake_run) == "/repo"
    assert calls == [(("git", "rev-parse", "--show-toplevel"), "/repo/app")]

    def failing_run(args, cwd=None, timeout=3):
        class R:
            returncode = 1
            stdout = ""
            stderr = "not a repo"

        return R()

    assert cc_usage.git_root_for_path("/repo/app", run=failing_run) == "/repo/app"


def test_normalize_pane_identity_requires_pane_pwd_not_session_pwd():
    pane = cc_usage.normalize_pane_identity({
        "session": "term-123",
        "pane": "%18",
        "cwd": "/repo/frontend",
        "agent": "claude",
        "pid": 9001,
        "model": "sonnet",
    }, labels={"term-123": "Frontend"}, now=123)

    assert pane["tmux_session"] == "term-123"
    assert pane["tmux_pane"] == "%18"
    assert pane["pane_pwd"] == "/repo/frontend"
    assert pane["agent"] == "claude"
    assert pane["tab_label"] == "Frontend"
    assert pane["last_seen_at"] == 123


def test_record_and_list_panes_round_trip():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        pane = cc_usage.normalize_pane_identity({
            "session": "term-123",
            "pane": "%18",
            "cwd": "/repo/frontend",
            "agent": "codex",
            "pid": 111,
        }, now=123)
        cc_usage.record_pane(db, pane)
        rows = cc_usage.list_panes(db)

    assert len(rows) == 1
    assert rows[0]["tmux_session"] == "term-123"
    assert rows[0]["tmux_pane"] == "%18"
    assert rows[0]["pane_pwd"] == "/repo/frontend"
    assert rows[0]["agent"] == "codex"


if __name__ == "__main__":
    test_usage_db_path_lives_under_hooks_dir()
    test_init_db_creates_required_tables()
    test_git_root_for_path_uses_git_when_available_and_falls_back()
    test_normalize_pane_identity_requires_pane_pwd_not_session_pwd()
    test_record_and_list_panes_round_trip()
