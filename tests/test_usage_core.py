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


def test_parse_openai_usage_buckets_preserves_grouping_dimensions():
    payload = {
        "data": [{
            "start_time": 100,
            "end_time": 160,
            "results": [{
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_cached_tokens": 250,
                "num_model_requests": 3,
                "project_id": "proj_1",
                "user_id": "user_1",
                "api_key_id": "key_1",
                "model": "gpt-test",
                "service_tier": "default",
            }]
        }],
        "has_more": False,
    }

    rows = cc_usage.parse_openai_usage_buckets(payload)

    assert rows == [{
        "provider": "openai",
        "start_time": 100,
        "end_time": 160,
        "input_tokens": 1000,
        "output_tokens": 500,
        "cache_read_tokens": 250,
        "cache_write_tokens": 0,
        "total_tokens": 1500,
        "request_count": 3,
        "project_id": "proj_1",
        "workspace_id": "",
        "user_id": "user_1",
        "api_key_id": "key_1",
        "model": "gpt-test",
        "service_tier": "default",
        "confidence": "exact",
    }]


def test_parse_openai_cost_buckets_preserves_amount_currency():
    payload = {"data": [{"start_time": 100, "end_time": 200, "results": [{
        "amount": {"value": 1.25, "currency": "usd"},
        "line_item": "Completions",
        "project_id": "proj_1",
        "api_key_id": "key_1",
    }]}]}

    rows = cc_usage.parse_openai_cost_buckets(payload)

    assert rows[0]["cost_usd"] == 1.25
    assert rows[0]["currency"] == "usd"
    assert rows[0]["project_id"] == "proj_1"
    assert rows[0]["api_key_id"] == "key_1"
    assert rows[0]["confidence"] == "exact"


def test_parse_anthropic_rows_accepts_current_and_generic_shapes():
    payload = {"data": [{
        "starting_at": "2026-07-09T00:00:00Z",
        "ending_at": "2026-07-10T00:00:00Z",
        "workspace_id": "wrk_1",
        "model": "claude-sonnet",
        "input_tokens": 10,
        "output_tokens": 5,
        "cache_read_input_tokens": 3,
        "cache_creation_input_tokens": 2,
    }]}

    rows = cc_usage.parse_anthropic_usage_rows(payload)

    assert rows[0]["provider"] == "anthropic"
    assert rows[0]["workspace_id"] == "wrk_1"
    assert rows[0]["model"] == "claude-sonnet"
    assert rows[0]["input_tokens"] == 10
    assert rows[0]["output_tokens"] == 5
    assert rows[0]["cache_read_tokens"] == 3
    assert rows[0]["cache_write_tokens"] == 2
    assert rows[0]["confidence"] == "exact"


def test_record_provider_usage_and_costs_round_trip():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        cc_usage.record_provider_usage(db, "openai", [{
            "provider": "openai",
            "start_time": 100,
            "end_time": 160,
            "input_tokens": 10,
            "output_tokens": 5,
            "cache_read_tokens": 2,
            "cache_write_tokens": 0,
            "total_tokens": 15,
            "request_count": 1,
            "project_id": "proj_1",
            "workspace_id": "",
            "user_id": "user_1",
            "api_key_id": "key_1",
            "model": "gpt-test",
            "service_tier": "default",
            "confidence": "exact",
        }])
        cc_usage.record_provider_costs(db, "openai", [{
            "provider": "openai",
            "start_time": 100,
            "end_time": 160,
            "cost_usd": 0.25,
            "currency": "usd",
            "project_id": "proj_1",
            "workspace_id": "",
            "api_key_id": "key_1",
            "line_item": "Completions",
            "model": "gpt-test",
            "confidence": "exact",
        }])
        con = sqlite3.connect(db)
        usage_count = con.execute("select count(*) from provider_usage_buckets").fetchone()[0]
        cost_count = con.execute("select count(*) from provider_cost_buckets").fetchone()[0]

    assert usage_count == 1
    assert cost_count == 1


if __name__ == "__main__":
    test_usage_db_path_lives_under_hooks_dir()
    test_init_db_creates_required_tables()
    test_git_root_for_path_uses_git_when_available_and_falls_back()
    test_normalize_pane_identity_requires_pane_pwd_not_session_pwd()
    test_record_and_list_panes_round_trip()
    test_parse_openai_usage_buckets_preserves_grouping_dimensions()
    test_parse_openai_cost_buckets_preserves_amount_currency()
    test_parse_anthropic_rows_accepts_current_and_generic_shapes()
    test_record_provider_usage_and_costs_round_trip()
