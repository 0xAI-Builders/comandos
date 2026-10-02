"""Uso por cuenta: Claude y Codex se importan con su alias, una vez por respuesta, con tramos medidos."""
import importlib.util
import json
import sqlite3
import time
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
    for name in ("record_local_grok_updates", "record_local_opencode_db"):
        if hasattr(dash.cc_usage, name):
            monkeypatch.setattr(dash.cc_usage, name, lambda *a, **k: 0)
    codex_kwargs = {}
    monkeypatch.setattr(dash.cc_usage, "record_local_codex_rollouts", lambda *a, **k: codex_kwargs.update(k) or 0)
    monkeypatch.setattr(dash.cc_usage, "record_local_claude_jsonl",
                        lambda db, root, **k: calls.append((k["account"], str(root), k["max_age_days"])) or 0)
    monkeypatch.setattr(dash.grok_state, "account_homes", lambda: [])
    dash._do_refresh_local_usage()
    assert calls == [("prune", 21),
                     ("main", str(tmp_path / ".claude" / "projects"), 21),
                     ("relotto", str(tmp_path / ".claude-accounts" / "relotto" / "projects"), 21)]
    # Codex: misma ventana de 21 d, sin tope de archivos y con su propio `seen`.
    assert codex_kwargs["max_age_days"] == 21
    assert isinstance(codex_kwargs["seen"], dict)
    assert codex_kwargs["max_files"] is None


def test_usage_state_stays_on_its_14_day_window(tmp_path):
    db = tmp_path / "u.sqlite"
    day = 86400
    cc_usage.record_turns(db, [
        {"id": "old", "provider": "claude", "agent": "claude", "source": "claude_jsonl", "confidence": "local",
         "turn_started_at": NOW - 20 * day, "turn_finished_at": NOW - 20 * day, "total_tokens": 1000, "git_root": "/repo"},
        {"id": "recent", "provider": "claude", "agent": "claude", "source": "claude_jsonl", "confidence": "local",
         "turn_started_at": NOW - day, "turn_finished_at": NOW - day, "total_tokens": 40, "git_root": "/repo"},
    ])
    state = cc_usage.build_usage_state(db, [], now=NOW)
    assert cc_usage.USAGE_STATE_DAYS == 14
    assert state["totals"]["total_tokens"] == 40
    con = sqlite3.connect(db)
    assert con.execute("select count(*) from usage_turns").fetchone()[0] == 2  # la DB conserva los 21 d


def test_claude_import_accepts_no_file_cap(tmp_path, monkeypatch):
    monkeypatch.setattr(cc_usage, "git_root_for_path", lambda p: p)
    db = tmp_path / "u.sqlite"
    for i in range(3):
        _write(tmp_path / "projects" / f"f{i}.jsonl", [_assistant(f"u{i}", f"msg_{i}", f"req_{i}")])
    assert cc_usage.record_local_claude_jsonl(db, tmp_path / "projects", now=NOW, max_age_days=30, max_files=None) == 3
    assert cc_usage.record_local_claude_jsonl(db, tmp_path / "projects", now=NOW, max_age_days=30, max_files=1) == 1


def _rollout(path, rows):
    _write(path, [{"timestamp": ts, "type": kind, "payload": payload} for ts, kind, payload in rows])


def test_codex_reads_each_response_once_per_account_even_across_forks(tmp_path, monkeypatch):
    monkeypatch.setattr(cc_usage, "git_root_for_path", lambda p: p)
    db = tmp_path / "u.sqlite"
    usage = {"input_tokens": 1000, "cached_input_tokens": 800, "cache_write_input_tokens": 0,
             "output_tokens": 50, "reasoning_output_tokens": 10, "total_tokens": 1050}
    parent = [
        ("2026-10-01T05:00:00Z", "session_meta", {"id": "th1", "cwd": "/repo"}),
        ("2026-10-01T05:00:00Z", "turn_context", {"turn_id": "tu1", "cwd": "/repo/app", "model": "gpt-5.5", "effort": "high"}),
        ("2026-10-01T05:01:00Z", "token_usage_record", {"thread_id": "th1", "turn_id": "tu1", "response_id": "resp_1", "usage": usage}),
        ("2026-10-01T05:02:00Z", "event_msg", {"type": "task_complete", "turn_id": "tu1", "started_at": 1790830800, "completed_at": 1790830920}),
    ]
    home = tmp_path / "codex-main"
    _rollout(home / "sessions" / "2026" / "10" / "01" / "rollout-a.jsonl", parent)
    # Un rollout bifurcado repite el historial del padre con los mismos ids.
    _rollout(home / "sessions" / "2026" / "10" / "01" / "rollout-b.jsonl", [("2026-10-01T05:10:00Z", "session_meta", {"id": "th2", "cwd": "/repo"})] + parent[1:])
    _rollout(tmp_path / "codex-work" / "sessions" / "rollout-c.jsonl", [
        ("2026-10-01T05:20:00Z", "token_usage_record", {"thread_id": "th3", "turn_id": "tu3", "response_id": "resp_3", "usage": usage}),
    ])
    cc_usage.record_local_codex_rollouts(db, [("main", str(home)), ("work", str(tmp_path / "codex-work"))], now=NOW, max_age_days=30)
    con = sqlite3.connect(db)
    turns = con.execute("select id, harness_account, git_root, model, input_tokens, cache_read_tokens, total_tokens, reasoning_tokens "
                        "from usage_turns order by id").fetchall()
    assert turns == [("codex-resp-resp_1", "main", "/repo/app", "gpt-5.5", 200, 800, 1050, 10),
                     ("codex-resp-resp_3", "work", "", "", 200, 800, 1050, 10)]
    assert con.execute("select id, account, git_root, finished_at - started_at from usage_spans").fetchall() == [
        ("codex-turn-tu1", "main", "/repo/app", 120.0)]


def test_quota_snapshots_keep_the_last_reading_of_each_cycle(tmp_path):
    db = tmp_path / "u.sqlite"
    row = {"id": "relotto:claude_weekly", "provider": "claude", "account": "relotto", "window": "7d", "scope": ""}
    cc_usage.record_quota_snapshots(db, [{**row, "percent": 40.0, "resets_at": 1791014400, "captured_at": 100}])
    # Mismo ciclo: el proveedor movió el reset 12 s; la lectura más nueva gana.
    cc_usage.record_quota_snapshots(db, [{**row, "percent": 91.0, "resets_at": 1791014412, "captured_at": 200}])
    cc_usage.record_quota_snapshots(db, [{**row, "percent": 50.0, "resets_at": 1791014400, "captured_at": 150}])
    cc_usage.record_quota_snapshots(db, [{**row, "id": "x", "window": "", "percent": 5.0, "resets_at": 1}, {**row, "percent": None, "resets_at": 9}])
    assert cc_usage.quota_snapshots(db) == [
        {"provider": "claude", "account": "relotto", "window": "7d", "scope": "", "resets_at": 1791014400, "percent": 91.0}]


def _turn(id_, account, source="claude_jsonl", provider="claude"):
    return {"id": id_, "provider": provider, "agent": provider, "tmux_session": "", "tmux_pane": "", "pane_pwd": "/x/P",
            "git_root": "/x/P", "turn_started_at": int(time.time()) - 60, "turn_finished_at": int(time.time()),
            "total_tokens": 10, "harness_account": account, "motor_account": account, "source": source, "confidence": "local"}


def test_legacy_rows_of_a_db_already_at_v11_are_dropped(tmp_path):
    # La base real ya estaba en v11 cuando el cc-dash viejo seguía importando: la migración no corre otra vez.
    db = tmp_path / "u.sqlite"
    cc_usage.record_turns(db, [_turn("uuid-old", "unknown"), _turn("codex-thread-1", "unknown", "codex_state_db", "codex"),
                               _turn("claude-jsonl-m:r", "main"), _turn("codex-resp-1", "main", "codex_rollout", "codex")])
    cc_usage.prune_old_turns(db, max_age_days=21)
    with cc_usage.connect(db) as con:
        assert {r[0] for r in con.execute("select id from usage_turns")} == {"claude-jsonl-m:r", "codex-resp-1"}


def test_a_conversation_copied_to_another_account_keeps_its_past_where_it_was(tmp_path):
    # «Cuenta» copia el transcript a la otra cuenta: las respuestas viejas aparecen en las dos carpetas.
    db = tmp_path / "u.sqlite"
    cc_usage.record_turns(db, [_turn("claude-jsonl-a:1", "main")])
    cc_usage.record_turns(db, [_turn("claude-jsonl-a:1", "relotto")])
    cc_usage.record_turns(db, [_turn("hook-1", "unknown", "hook")])
    cc_usage.record_turns(db, [_turn("hook-1", "relotto", "hook")])
    span = {"id": "claude-turn-u", "provider": "claude", "account": "main", "session_id": "s", "git_root": "/x/P",
            "started_at": 1.0, "finished_at": 2.0, "source": "claude_jsonl"}
    cc_usage.record_spans(db, [span])
    cc_usage.record_spans(db, [{**span, "account": "relotto", "finished_at": 3.0}])
    with cc_usage.connect(db) as con:
        acc = dict(con.execute("select id, harness_account from usage_turns"))
        assert acc == {"claude-jsonl-a:1": "main", "hook-1": "relotto"}
        assert [tuple(r) for r in con.execute("select account, finished_at from usage_spans")] == [("main", 3.0)]
