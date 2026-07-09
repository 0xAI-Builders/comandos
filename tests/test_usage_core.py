#!/usr/bin/env python3
import json
import os
import sqlite3
import subprocess
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


def test_live_panes_are_detected_not_unattributed():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        pane = cc_usage.normalize_pane_identity({
            "session": "term-123",
            "pane": "%18",
            "cwd": "/repo/frontend",
            "agent": "codex",
            "provider": "codex",
            "pid": 111,
        }, now=123)
        state = cc_usage.build_usage_state(db, [pane], now=130)

    assert state["unattributed"] == []
    assert state["projects"][0]["confidence"] == "detected"
    assert state["projects"][0]["panes"][0]["confidence"] == "detected"
    assert state["providers"][0]["provider"] == "codex"
    assert state["providers"][0]["active_panes"] == 1


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


def test_record_turn_rolls_up_by_project_session_and_pane():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        event = {
            "provider": "claude",
            "agent": "claude",
            "tmux_session": "term-1",
            "tmux_pane": "%1",
            "pane_pwd": "/repo/frontend",
            "git_root": "/repo",
            "model": "sonnet",
            "turn_started_at": 100,
            "turn_finished_at": 120,
            "input_tokens": 10,
            "output_tokens": 5,
            "cost_usd": 0.01,
            "source": "cli_turn",
            "confidence": "exact",
        }
        cc_usage.record_turn(db, event)
        state = cc_usage.build_usage_state(db, [], now=130)

    assert state["totals"]["cost_usd"] == 0.01
    assert state["projects"][0]["git_root"] == "/repo"
    assert state["projects"][0]["panes"][0]["tmux_pane"] == "%1"
    assert state["projects"][0]["panes"][0]["confidence"] == "exact"


def test_aggregate_provider_bucket_is_unattributed_without_matching_pane():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        cc_usage.record_provider_costs(db, "openai", [{
            "provider": "openai",
            "start_time": 100,
            "end_time": 200,
            "cost_usd": 0.25,
            "currency": "usd",
            "project_id": "",
            "api_key_id": "",
            "line_item": "Completions",
            "confidence": "exact",
        }])
        state = cc_usage.build_usage_state(db, [], now=210)

    assert state["totals"]["cost_usd"] == 0.25
    assert state["unattributed"][0]["cost_usd"] == 0.25
    assert state["unattributed"][0]["confidence"] == "unattributed"


def test_build_usage_state_includes_token_windows_from_settings():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        cc_usage.record_turn(db, {
            "id": "codex-1",
            "provider": "codex",
            "agent": "codex",
            "tmux_session": "term-1",
            "tmux_pane": "%1",
            "pane_pwd": "/repo",
            "git_root": "/repo",
            "model": "gpt-test",
            "turn_started_at": 200,
            "turn_finished_at": 250,
            "total_tokens": 70,
            "source": "codex_state_db",
            "confidence": "local",
        })
        state = cc_usage.build_usage_state(db, [], now=300, settings={
            "COMANDOS_CODEX_DAILY_TOKEN_LIMIT": "100",
            "COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT": "50",
        })

    windows = {w["id"]: w for w in state["windows"]["items"]}
    assert windows["codex_daily_tokens"]["used"] == 70
    assert windows["codex_daily_tokens"]["limit"] == 100
    assert windows["codex_daily_tokens"]["remaining"] == 30
    assert windows["codex_daily_tokens"]["percent"] == 70
    assert windows["claude_daily_tokens"]["status"] == "missing_limit" or windows["claude_daily_tokens"]["used"] == 0


def test_record_local_codex_threads_imports_sqlite_tokens():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        state_db = os.path.join(d, "state.sqlite")
        con = sqlite3.connect(state_db)
        con.execute("""
            create table threads (
              id text primary key, created_at integer, updated_at integer,
              source text, model_provider text, cwd text, title text,
              tokens_used integer, model text, reasoning_effort text
            )
        """)
        con.execute(
            "insert into threads values (?,?,?,?,?,?,?,?,?,?)",
            ("thread-1", 100, 200, "cli", "openai", "/repo", "Work", 1234, "gpt-test", "medium"),
        )
        con.commit()
        con.close()

        count = cc_usage.record_local_codex_threads(db, state_db, now=300)
        state = cc_usage.build_usage_state(db, [], now=300)

    assert count == 1
    assert state["totals"]["total_tokens"] == 1234
    assert state["projects"][0]["git_root"] == "/repo"
    assert state["projects"][0]["panes"][0]["provider"] == "codex"
    assert state["projects"][0]["panes"][0]["confidence"] == "local"


def test_record_local_claude_jsonl_imports_message_usage():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        root = Path(d) / "projects"
        root.mkdir()
        item = {
            "type": "assistant",
            "uuid": "msg-1",
            "timestamp": "2026-07-09T20:00:00Z",
            "cwd": "/repo",
            "sessionId": "session-1",
            "message": {
                "model": "claude-test",
                "usage": {
                    "input_tokens": 10,
                    "output_tokens": 5,
                    "cache_read_input_tokens": 7,
                    "cache_creation_input_tokens": 3,
                },
            },
        }
        (root / "session.jsonl").write_text(json.dumps(item) + "\n")

        count = cc_usage.record_local_claude_jsonl(db, root, now=1783632000, max_age_days=30)
        state = cc_usage.build_usage_state(db, [], now=1783632000)

    assert count == 1
    assert state["totals"]["total_tokens"] == 25
    assert state["projects"][0]["panes"][0]["provider"] == "claude"
    assert state["projects"][0]["panes"][0]["confidence"] == "local"


def test_usage_settings_round_trip_filters_to_limit_keys():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.write_usage_settings(db, {
            "COMANDOS_CODEX_DAILY_TOKEN_LIMIT": "100",
            "OPENAI_ADMIN_KEY": "secret",
            "bad": "ignored",
        })
        settings = cc_usage.read_usage_settings(db)

    assert settings == {"COMANDOS_CODEX_DAILY_TOKEN_LIMIT": "100"}


def test_cc_usage_capture_hook_cli_runs_after_helpers_are_defined():
    payload = {
        "agent": "claude",
        "input_tokens": 1,
        "output_tokens": 2,
        "turn_started_at": 100,
        "turn_finished_at": 101,
    }
    env = os.environ.copy()
    with tempfile.TemporaryDirectory() as d:
        env["COMANDOS_USAGE_DB"] = os.path.join(d, "usage.sqlite")
        result = subprocess.run(
            ["python3", str(ROOT / "bin" / "cc_usage.py"), "capture-hook"],
            input=json.dumps(payload),
            capture_output=True,
            text=True,
            env=env,
            timeout=5,
        )

    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)["captured"] is True


def test_alerts_fire_on_cost_threshold_and_spike():
    state = {
        "totals": {"cost_usd": 9.5},
        "windows": {"daily_budget_usd": 10.0},
        "series": [{"ts": 100, "cost_usd": 1.0}, {"ts": 200, "cost_usd": 9.5}],
    }

    alerts = cc_usage.calculate_alerts(
        state,
        {"cost_thresholds": [0.7, 0.85, 0.95], "spike_usd": 5.0},
    )

    assert any(a["kind"] == "budget" and a["level"] == "danger" for a in alerts)
    assert any(a["kind"] == "spike" for a in alerts)


def test_model_presets_include_savings_daily_hard_maximum():
    names = {p["id"] for p in cc_usage.model_presets()}
    assert {"ahorro", "diario", "dificil", "maximo"}.issubset(names)


def test_model_switch_text_opens_provider_model_picker():
    assert cc_usage.model_switch_text("claude", "diario") == "/model sonnet"
    assert cc_usage.model_switch_text("codex", "diario") == "/model"


def test_capture_hook_payload_noops_without_usage_numbers():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        result = cc_usage.capture_hook_payload({
            "agent": "claude",
            "tmux_session": "term-1",
            "tmux_pane": "%1",
            "pane_pwd": "/repo",
            "git_root": "/repo",
        }, db)
        con = sqlite3.connect(db)
        count = con.execute("select count(*) from usage_turns").fetchone()[0]

    assert result["captured"] is False
    assert count == 0


def test_capture_hook_payload_records_when_usage_numbers_exist():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        result = cc_usage.capture_hook_payload({
            "agent": "claude",
            "provider": "claude",
            "tmux_session": "term-1",
            "tmux_pane": "%1",
            "pane_pwd": "/repo",
            "git_root": "/repo",
            "model": "sonnet",
            "input_tokens": 10,
            "output_tokens": 5,
            "cost_usd": 0.01,
            "turn_started_at": 100,
            "turn_finished_at": 110,
        }, db)
        state = cc_usage.build_usage_state(db, [], now=120)

    assert result["captured"] is True
    assert state["totals"]["total_tokens"] == 15
    assert state["totals"]["cost_usd"] == 0.01


CLAUDE_OAUTH_PAYLOAD = {
    "five_hour": {"utilization": 5.0, "resets_at": "2026-07-10T01:00:00+00:00"},
    "seven_day": {"utilization": 1.0, "resets_at": "2026-07-11T08:00:00+00:00"},
    "limits": [
        {"kind": "session", "group": "session", "percent": 5, "severity": "normal",
         "resets_at": "2026-07-10T01:00:00+00:00", "scope": None, "is_active": True},
        {"kind": "weekly_all", "group": "weekly", "percent": 1, "severity": "normal",
         "resets_at": "2026-07-11T08:00:00+00:00", "scope": None, "is_active": False},
        {"kind": "weekly_scoped", "group": "weekly", "percent": 0, "severity": "normal",
         "resets_at": "2026-07-11T08:00:00+00:00",
         "scope": {"model": {"id": None, "display_name": "Fable"}, "surface": None},
         "is_active": False},
    ],
}


def _epoch(iso):
    from datetime import datetime
    return int(datetime.fromisoformat(iso).timestamp())


def test_parse_claude_oauth_limits_normalizes_percent_and_reset():
    rows = cc_usage.parse_claude_oauth_limits(CLAUDE_OAUTH_PAYLOAD, now=100)
    by_id = {r["id"]: r for r in rows}

    assert by_id["claude_session"]["percent"] == 5.0
    assert by_id["claude_session"]["resets_at"] == _epoch("2026-07-10T01:00:00+00:00")
    assert by_id["claude_session"]["confidence"] == "exact"
    assert by_id["claude_session"]["provider"] == "claude"
    assert by_id["claude_session"]["captured_at"] == 100
    assert by_id["claude_weekly"]["percent"] == 1.0
    assert by_id["claude_weekly_fable"]["scope"] == "Fable"
    assert by_id["claude_weekly_fable"]["percent"] == 0.0


def test_fetch_claude_oauth_limits_uses_local_token_and_hides_it():
    with tempfile.TemporaryDirectory() as d:
        creds = os.path.join(d, "credentials.json")
        with open(creds, "w") as f:
            json.dump({"claudeAiOauth": {"accessToken": "tok-123"}}, f)
        captured = {}

        def fake_http(url, headers, timeout=8):
            captured["url"] = url
            captured["auth"] = headers.get("Authorization")
            return CLAUDE_OAUTH_PAYLOAD

        rows, health = cc_usage.fetch_claude_oauth_limits(
            creds_path=creds, now=100, http=fake_http)

    assert "oauth/usage" in captured["url"]
    assert captured["auth"] == "Bearer tok-123"
    assert health["status"] == "ok"
    assert health["provider"] == "claude"
    assert rows and "tok-123" not in json.dumps(rows)
    assert "tok-123" not in json.dumps(health)


def test_fetch_claude_oauth_limits_reports_missing_credentials():
    rows, health = cc_usage.fetch_claude_oauth_limits(
        creds_path="/nonexistent/creds.json", now=100,
        http=lambda *a, **k: (_ for _ in ()).throw(AssertionError("no llamar")))

    assert rows == []
    assert health["status"] == "missing"


def test_read_codex_rate_limits_reads_latest_rollout_snapshot():
    with tempfile.TemporaryDirectory() as d:
        day = Path(d) / "2026" / "07" / "09"
        day.mkdir(parents=True)
        lines = [
            json.dumps({"timestamp": "2026-07-09T10:00:00.000Z", "type": "session_meta",
                        "payload": {"type": "session_meta"}}),
            json.dumps({"timestamp": "2026-07-09T11:00:00.000Z", "type": "event_msg",
                        "payload": {"type": "token_count", "info": {},
                                    "rate_limits": {
                                        "limit_id": "codex",
                                        "primary": {"used_percent": 42.5, "window_minutes": 300,
                                                    "resets_at": 5000},
                                        "secondary": {"used_percent": 7.0, "window_minutes": 10080,
                                                      "resets_at": 9000},
                                        "plan_type": "pro"}}}),
        ]
        (day / "rollout-2026-07-09T11-00-00-abc.jsonl").write_text("\n".join(lines) + "\n")

        rows = cc_usage.read_codex_rate_limits(sessions_root=d, now=6000)

    by_id = {r["id"]: r for r in rows}
    assert by_id["codex_session"]["percent"] == 42.5
    assert by_id["codex_session"]["resets_at"] == 5000
    assert by_id["codex_session"]["confidence"] == "exact"
    assert by_id["codex_session"]["provider"] == "codex"
    assert by_id["codex_weekly"]["percent"] == 7.0
    assert by_id["codex_weekly"]["resets_at"] == 9000


def test_read_codex_rate_limits_empty_without_sessions():
    with tempfile.TemporaryDirectory() as d:
        assert cc_usage.read_codex_rate_limits(sessions_root=d, now=100) == []
    assert cc_usage.read_codex_rate_limits(sessions_root="/nonexistent", now=100) == []


def test_build_usage_state_exposes_provider_limits():
    with tempfile.TemporaryDirectory() as d:
        db = os.path.join(d, "usage.sqlite")
        cc_usage.init_db(db)
        state = cc_usage.build_usage_state(db, [], now=100, limits=[
            {"id": "claude_session", "provider": "claude", "percent": 5.0},
        ])

    assert state["limits"][0]["id"] == "claude_session"
    assert state["limits"][0]["percent"] == 5.0


def test_model_switch_text_accepts_direct_model():
    assert cc_usage.model_switch_text("claude", "", model="opus") == "/model opus"
    assert cc_usage.model_switch_text("claude", "", model="fable") == "/model fable"
    # Modelos fuera de la lista blanca no se inyectan al pane
    assert cc_usage.model_switch_text("claude", "", model="rm -rf /") == "/model sonnet"
    assert cc_usage.model_switch_text("codex", "", model="gpt-x") == "/model"


if __name__ == "__main__":
    test_usage_db_path_lives_under_hooks_dir()
    test_init_db_creates_required_tables()
    test_git_root_for_path_uses_git_when_available_and_falls_back()
    test_normalize_pane_identity_requires_pane_pwd_not_session_pwd()
    test_record_and_list_panes_round_trip()
    test_live_panes_are_detected_not_unattributed()
    test_parse_openai_usage_buckets_preserves_grouping_dimensions()
    test_parse_openai_cost_buckets_preserves_amount_currency()
    test_parse_anthropic_rows_accepts_current_and_generic_shapes()
    test_record_provider_usage_and_costs_round_trip()
    test_record_turn_rolls_up_by_project_session_and_pane()
    test_aggregate_provider_bucket_is_unattributed_without_matching_pane()
    test_build_usage_state_includes_token_windows_from_settings()
    test_record_local_codex_threads_imports_sqlite_tokens()
    test_record_local_claude_jsonl_imports_message_usage()
    test_usage_settings_round_trip_filters_to_limit_keys()
    test_cc_usage_capture_hook_cli_runs_after_helpers_are_defined()
    test_alerts_fire_on_cost_threshold_and_spike()
    test_model_presets_include_savings_daily_hard_maximum()
    test_model_switch_text_opens_provider_model_picker()
    test_capture_hook_payload_noops_without_usage_numbers()
    test_capture_hook_payload_records_when_usage_numbers_exist()
    test_parse_claude_oauth_limits_normalizes_percent_and_reset()
    test_fetch_claude_oauth_limits_uses_local_token_and_hides_it()
    test_fetch_claude_oauth_limits_reports_missing_credentials()
    test_read_codex_rate_limits_reads_latest_rollout_snapshot()
    test_read_codex_rate_limits_empty_without_sessions()
    test_build_usage_state_exposes_provider_limits()
    test_model_switch_text_accepts_direct_model()
