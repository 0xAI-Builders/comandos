"""Uso por cuenta: Claude y Codex se importan con su alias, una vez por respuesta, con tramos medidos."""
import importlib.util
import json
import sqlite3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("cc_usage_accounts", ROOT / "bin" / "cc_usage.py")
cc_usage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cc_usage)

NOW = 1790841600  # 2026-10-01T06:00:00Z


def _write(path, rows):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(json.dumps(r) + "\n" for r in rows))


def _assistant(uuid, msg_id, req, ts="2026-10-01T05:00:00Z", out=5):
    return {"type": "assistant", "uuid": uuid, "requestId": req, "timestamp": ts, "cwd": "/repo", "sessionId": "s1",
            "message": {"id": msg_id, "model": "claude-test",
                        "usage": {"input_tokens": 10, "output_tokens": out, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}}}


def test_account_homes_lists_main_then_each_account(tmp_path):
    (tmp_path / "accts" / "relotto").mkdir(parents=True)
    (tmp_path / "accts" / "relotto.lock").mkdir()
    (tmp_path / "accts" / ".hidden").mkdir()
    homes = cc_usage.account_homes(str(tmp_path / "main"), str(tmp_path / "accts"))
    assert homes == [("main", str(tmp_path / "main")), ("relotto", str(tmp_path / "accts" / "relotto"))]


def test_claude_counts_each_response_once_and_keeps_the_account(tmp_path, monkeypatch):
    monkeypatch.setattr(cc_usage, "git_root_for_path", lambda p: p)
    db = tmp_path / "u.sqlite"
    # Una respuesta con dos bloques de contenido = dos líneas con el mismo usage.
    _write(tmp_path / "relotto" / "projects" / "p" / "a.jsonl", [
        _assistant("u1", "msg_1", "req_1"), _assistant("u2", "msg_1", "req_1"),
        _assistant("u3", "msg_2", "req_2", ts="2026-10-01T05:10:00Z", out=7),
        {"type": "system", "subtype": "turn_duration", "durationMs": 600000, "uuid": "t1",
         "timestamp": "2026-10-01T05:10:00Z", "cwd": "/repo", "sessionId": "s1"},
    ])
    cc_usage.record_local_claude_jsonl(db, tmp_path / "relotto" / "projects", now=NOW, max_age_days=30, account="relotto")
    con = sqlite3.connect(db)
    turns = con.execute("select harness_account, motor_account, total_tokens from usage_turns order by id").fetchall()
    spans = con.execute("select provider, account, git_root, finished_at - started_at from usage_spans").fetchall()
    assert turns == [("relotto", "relotto", 15), ("relotto", "relotto", 17)]
    assert spans == [("claude", "relotto", "/repo", 600.0)]


def test_unchanged_transcripts_are_not_read_again(tmp_path, monkeypatch):
    monkeypatch.setattr(cc_usage, "git_root_for_path", lambda p: p)
    db = tmp_path / "u.sqlite"
    _write(tmp_path / "projects" / "a.jsonl", [_assistant("u1", "msg_1", "req_1")])
    seen = {}
    assert cc_usage.record_local_claude_jsonl(db, tmp_path / "projects", now=NOW, max_age_days=30, seen=seen) == 1
    assert cc_usage.record_local_claude_jsonl(db, tmp_path / "projects", now=NOW, max_age_days=30, seen=seen) == 0


def test_v10_database_drops_rows_that_double_counted(tmp_path):
    db = tmp_path / "u.sqlite"
    cc_usage.record_turns(db, [
        {"id": "claude-jsonl-u1", "provider": "claude", "agent": "claude", "source": "claude_jsonl", "confidence": "local"},
        {"id": "codex-state-t", "provider": "codex", "agent": "codex", "source": "codex_state_db", "confidence": "shared"},
        {"id": "grok-1", "provider": "grok", "agent": "grok", "source": "grok_updates", "confidence": "exact"},
    ])
    con = sqlite3.connect(db)
    con.execute("pragma user_version=10")
    con.commit()
    con.close()
    cc_usage.init_db(db)
    con = sqlite3.connect(db)
    assert [r[0] for r in con.execute("select id from usage_turns")] == ["grok-1"]
    assert con.execute("pragma user_version").fetchone()[0] == cc_usage.USAGE_SCHEMA_VERSION
    tables = {r[0] for r in con.execute("select name from sqlite_master where type='table'")}
    assert {"usage_spans", "usage_quota_snapshots"} <= tables


def test_refresh_imports_every_claude_account_for_21_days(tmp_path, monkeypatch):
    import sys
    sys.path.insert(0, str(ROOT / "tests"))
    from test_usage_dash import load_dash_module
    dash = load_dash_module()
    (tmp_path / ".claude-accounts" / "relotto").mkdir(parents=True)
    monkeypatch.setenv("HOME", str(tmp_path))
    calls = []
    monkeypatch.setattr(dash, "usage_runtime_env", lambda: {})
    monkeypatch.setattr(dash, "ensure_observed_configs", lambda: None)
    monkeypatch.setattr(dash.cc_usage, "reconcile_orphan_interactions", lambda *a, **k: 0)
    monkeypatch.setattr(dash.cc_usage, "prune_old_turns", lambda *a, **k: calls.append(("prune", k["max_age_days"])) or 0)
    for name in ("record_local_grok_updates", "record_local_opencode_db", "record_local_codex_threads", "record_local_codex_rollouts"):
        if hasattr(dash.cc_usage, name):
            monkeypatch.setattr(dash.cc_usage, name, lambda *a, **k: 0)
    monkeypatch.setattr(dash.cc_usage, "record_local_claude_jsonl",
                        lambda db, root, **k: calls.append((k["account"], str(root), k["max_age_days"])) or 0)
    monkeypatch.setattr(dash.grok_state, "account_homes", lambda: [])
    dash._do_refresh_local_usage()
    assert calls == [("prune", 21),
                     ("main", str(tmp_path / ".claude" / "projects"), 21),
                     ("relotto", str(tmp_path / ".claude-accounts" / "relotto" / "projects"), 21)]
