#!/usr/bin/env python3
"""Migration and privacy tests for combination telemetry."""
import json
import importlib.util
import sqlite3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("cc_usage_telemetry", ROOT / "bin" / "cc_usage.py")
cc_usage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cc_usage)


LEGACY_TURNS = """create table usage_turns(
 id text primary key,provider text not null,agent text not null,tmux_session text not null,
 tmux_pane text not null,pane_pwd text not null,git_root text not null,model text not null default '',
 reasoning_effort text not null default '',turn_started_at integer not null,turn_finished_at integer not null,
 input_tokens integer not null default 0,output_tokens integer not null default 0,
 cache_read_tokens integer not null default 0,cache_write_tokens integer not null default 0,
 total_tokens integer not null default 0,cost_usd real not null default 0,source text not null,
 confidence text not null,raw text not null default '{}')"""


def test_populated_v1_migrates_without_row_loss(tmp_path):
    db = tmp_path / "usage.sqlite"
    with sqlite3.connect(db) as con:
        con.execute(LEGACY_TURNS)
        for ident, model, agent in (
            ("1", "claude-opus-5", "claude"),
            ("2", "gpt-5.6-sol", "claude"),
            ("3", "grok-4.6", "grok"),
        ):
            con.execute(
                "insert into usage_turns values(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                (ident, agent, agent, "s", "%1", "/x", "/x", model, "high", 1, 2,
                 1, 2, 0, 0, 3, 0, "legacy", "exact", "{}"),
            )
        con.execute("pragma user_version=1")

    cc_usage.init_db(str(db))

    with sqlite3.connect(db) as con:
        assert con.execute("pragma user_version").fetchone()[0] == cc_usage.USAGE_SCHEMA_VERSION
        assert con.execute("select count(*) from usage_turns").fetchone()[0] == 3
        routes = con.execute("select id,route_id from usage_turns order by id").fetchall()
        columns = {row[1] for row in con.execute("pragma table_info(usage_interactions)")}
    assert routes == [("1", "claude:claude"), ("2", "claude:codex"), ("3", "grok:grok")]
    assert {"prompt_id", "agent_session_id"}.issubset(columns)


def test_lifecycle_waiting_stays_open_and_terminal_correlates_prompt(tmp_path):
    db = str(tmp_path / "usage.sqlite")
    cc_usage.record_session_config(db, {
        "tmux_session": "s", "tmux_pane": "%1", "harness": "claude", "motor": "grok",
        "model": "grok-4.6", "effort": "xhigh", "route_id": "claude:grok", "effective_at": 1,
    })
    started = cc_usage.capture_lifecycle(db, {
        "status": "working", "tmux_session": "s", "tmux_pane": "%1",
        "prompt_id": "prompt-safe-id", "agent_session_id": "session-safe-id", "at_ms": 1000,
    })
    waiting = cc_usage.capture_lifecycle(db, {
        "status": "waiting", "tmux_session": "s", "tmux_pane": "%1",
        "prompt_id": "prompt-safe-id", "at_ms": 1500,
    })
    assert started["captured"] and waiting == {"captured": True, "pending": True}

    done = cc_usage.capture_lifecycle(db, {
        "status": "done", "tmux_session": "s", "tmux_pane": "%1",
        "prompt_id": "prompt-safe-id", "at_ms": 2500,
    })
    assert done["completion_status"] == "completed"
    with sqlite3.connect(db) as con:
        row = con.execute(
            "select prompt_id,agent_session_id,duration_ms,completion_status from usage_interactions"
        ).fetchone()
    assert row == ("prompt-safe-id", "session-safe-id", 1500, "completed")


def test_tool_events_store_name_and_timing_without_payloads(tmp_path):
    db = str(tmp_path / "usage.sqlite")
    cc_usage.capture_lifecycle(db, {
        "status": "working", "tmux_session": "s", "tmux_pane": "%1", "prompt_id": "p", "at_ms": 1000,
    })
    started = cc_usage.capture_tool_event(db, {
        "phase": "start", "tmux_session": "s", "tmux_pane": "%1",
        "tool_name": "Bash", "tool_use_id": "tool-1", "at_ms": 1200,
        "arguments": "must-not-be-stored", "result": "must-not-be-stored",
    })
    ended = cc_usage.capture_tool_event(db, {
        "phase": "success", "tmux_session": "s", "tmux_pane": "%1",
        "tool_name": "Bash", "tool_use_id": "tool-1", "at_ms": 1700,
    })
    assert started["captured"] and ended["captured"]
    with sqlite3.connect(db) as con:
        row = con.execute(
            "select tool_name,tool_family,duration_ms,status,error_class from usage_tool_calls"
        ).fetchone()
    assert row == ("Bash", "execution", 500, "success", "")


def test_grok_updates_import_exact_usage_without_content(tmp_path, monkeypatch):
    home = tmp_path / "grok"
    session = home / "sessions" / "2026" / "s-grok"
    session.mkdir(parents=True)
    (session / "summary.json").write_text(json.dumps({
        "info": {"id": "s-grok", "cwd": str(tmp_path)},
        "current_model_id": "grok-4.6", "reasoning_effort": "xhigh",
        "session_summary": "must not be stored", "generated_title": "also private",
    }))
    update = {
        "timestamp": 2_000_000,
        "params": {"sessionId": "s-grok", "_meta": {"eventId": "event-1"}, "update": {
            "sessionUpdate": "turn_completed", "prompt_id": "prompt-id", "stop_reason": "end_turn",
            "usage": {"inputTokens": 100, "outputTokens": 20, "totalTokens": 120,
                      "cachedReadTokens": 30, "cacheCreationTokens": 4,
                      "reasoningTokens": 7, "apiDurationMs": 1500,
                      "modelCalls": 1, "numTurns": 1, "modelUsage": {"grok-4.6-build": {}}},
        }},
    }
    (session / "updates.jsonl").write_text(json.dumps(update) + "\n")
    monkeypatch.setattr(cc_usage, "git_root_for_path", lambda path: path)
    db = str(tmp_path / "usage.sqlite")

    assert cc_usage.record_local_grok_updates(db, [home], now=2000) == 1
    with sqlite3.connect(db) as con:
        row = con.execute("""select model,reasoning_effort,input_tokens,output_tokens,
          cache_read_tokens,cache_write_tokens,reasoning_tokens,duration_ms,raw from usage_turns""").fetchone()
    assert row[:8] == ("grok-4.6-build", "xhigh", 100, 20, 30, 4, 7, 1500)
    assert "must not be stored" not in row[8]
    assert "also private" not in row[8]


def test_grok_updates_timestamps_in_seconds_pass_the_cutoff(tmp_path, monkeypatch):
    # Regresion: el CLI de Grok escribe `timestamp` en SEGUNDOS; el cutoff
    # esta en ms. Sin normalizar unidades, todos los eventos se descartaban.
    now = 1_788_000_000
    home = tmp_path / "grok"
    session = home / "sessions" / "2026" / "s-grok"
    session.mkdir(parents=True)
    (session / "summary.json").write_text(json.dumps({
        "info": {"id": "s-grok", "cwd": str(tmp_path)},
        "current_model_id": "grok-4.6", "reasoning_effort": "high",
    }))
    update = {
        "timestamp": now - 3600,  # hace 1 h, en segundos (formato real del CLI)
        "params": {"sessionId": "s-grok", "_meta": {"eventId": "event-s"}, "update": {
            "sessionUpdate": "turn_completed", "prompt_id": "p1", "stop_reason": "end_turn",
            "usage": {"inputTokens": 50, "outputTokens": 10, "totalTokens": 60,
                      "apiDurationMs": 900, "modelCalls": 1, "numTurns": 1,
                      "modelUsage": {"grok-4.6-build": {}}},
        }},
    }
    (session / "updates.jsonl").write_text(json.dumps(update) + "\n")
    monkeypatch.setattr(cc_usage, "git_root_for_path", lambda path: path)
    db = str(tmp_path / "usage.sqlite")

    assert cc_usage.record_local_grok_updates(db, [home], now=now, max_age_days=14) == 1
    with sqlite3.connect(db) as con:
        finished = con.execute(
            "select turn_finished_at from usage_turns where provider='grok'").fetchone()[0]
    assert finished == now - 3600  # almacenado en segundos, como el resto


def test_grok_measured_usage_aggregates_today_and_week(tmp_path):
    db = str(tmp_path / "usage.sqlite")
    cc_usage.init_db(db)
    now = 1_788_000_000
    rows = [(now - 600, 1000), (now - 3 * 86400, 5000)]  # hoy + hace 3 dias
    cc_usage.record_turns(db, [{
        "id": f"g{i}", "provider": "grok", "agent": "grok", "model": "grok-4.6-build",
        "turn_started_at": ts - 5, "turn_finished_at": ts,
        "input_tokens": tok, "output_tokens": 0, "total_tokens": tok,
        "source": "grok_updates", "confidence": "exact",
    } for i, (ts, tok) in enumerate(rows)])
    m = cc_usage.grok_measured_usage(db, now=now)
    assert m["tokens_7d"] == 6000 and m["turns_7d"] == 2
    assert m["tokens_today"] >= 1000 and m["turns_today"] >= 1
    assert cc_usage.grok_measured_usage(db, now=now + 30 * 86400) is None


def test_rating_task_type_and_recent_interaction_are_joined(tmp_path):
    db = str(tmp_path / "usage.sqlite")
    opened = cc_usage.capture_lifecycle(db, {
        "status": "working", "tmux_session": "s", "tmux_pane": "%1", "prompt_id": "p", "at_ms": 1000,
    })
    cc_usage.capture_lifecycle(db, {
        "status": "done", "tmux_session": "s", "tmux_pane": "%1", "prompt_id": "p", "at_ms": 2000,
    })
    interaction_id = opened["interaction_id"]
    cc_usage.set_interaction_feedback(db, interaction_id, "solved", 5, "local note")
    cc_usage.set_interaction_task(db, interaction_id, "debugging", "Bug fix")

    recent = cc_usage.recent_interactions(db, 1)
    assert recent[0]["id"] == interaction_id
    assert recent[0]["rating"] == 5
    assert recent[0]["outcome"] == "solved"
    assert recent[0]["task_type"] == "debugging"


def test_focus_blocks_are_durable_and_analytics_exclude_cancelled_minutes(tmp_path):
    db = str(tmp_path / "usage.sqlite")
    first = cc_usage.focus_block_start(db, {
        "mode": "focus", "project": "ComandOS", "tmux_session": "s", "tmux_pane": "%1",
        "planned_minutes": 25, "cycle_index": 1, "cycle_total": 4, "started_at_ms": 1_999_000_000,
    })
    cc_usage.focus_block_finish(db, first["id"], "completed", 2_000_000_000)
    second = cc_usage.focus_block_start(db, {
        "mode": "focus", "project": "ComandOS", "planned_minutes": 50, "started_at_ms": 1_999_500_000,
    })
    cc_usage.focus_block_finish(db, second["id"], "cancelled", 2_000_000_000)

    stats = cc_usage.focus_analytics(db, 7, now=2_000_000_000)
    assert stats["today"] == {"blocks": 1, "minutes": 25}
    assert stats["completionRate"] == 0.5
    assert stats["recent"][0]["status"] == "cancelled"


def test_recent_interactions_excludes_in_flight_work(tmp_path):
    db = str(tmp_path / "usage.sqlite")
    opened = cc_usage.capture_lifecycle(db, {
        "status": "working", "tmux_session": "s", "tmux_pane": "%1", "prompt_id": "p", "at_ms": 1000,
    })
    assert opened["captured"]
    assert cc_usage.recent_interactions(db) == []


def test_experiment_creation_and_pairs_share_one_task_id(tmp_path):
    db = str(tmp_path / "usage.sqlite")
    created = cc_usage.create_experiment(db, "Claude vs Grok", "debugging", [
        {"label":"Claude","harness":"claude","motor":"claude","model":"claude-opus-5","effort":"high","route_id":"claude:claude","harness_account":"main","motor_account":"main"},
        {"label":"Grok","harness":"claude","motor":"grok","model":"grok-4.6","effort":"high","route_id":"claude:grok","harness_account":"main","motor_account":"main"},
    ])
    pair = cc_usage.create_experiment_pair(db, created["id"], "same bug")
    assert len(pair["runs"]) == 2
    with sqlite3.connect(db) as con:
        task_ids = {row[0] for row in con.execute("select task_id from usage_experiment_runs")}
    assert task_ids == {pair["taskId"]}
    listed = cc_usage.list_experiments(db)
    assert listed[0]["status"] == "active"
    assert len(listed[0]["variants"]) == 2


def test_paired_experiment_requires_ten_complete_pairs_and_clear_interval(tmp_path, monkeypatch):
    db = str(tmp_path / "usage.sqlite")
    cc_usage.init_db(db)
    monkeypatch.setattr(cc_usage.time, "time", lambda: 2_000_000)
    with sqlite3.connect(db) as con:
        con.execute("""insert into usage_experiments
          (id,label,task_type,status,design,created_at,updated_at) values('exp','A/B','debugging','active','paired',1,1)""")
        for task_index in range(10):
            task_id = f"task-{task_index}"
            con.execute("insert into usage_tasks values(?,?,?,?,?,?,?,?)",
                        (task_id,"debugging","manual","exact","","paired",1,1))
            for variant in (0, 1):
                interaction_id = f"i-{task_index}-{variant}"
                con.execute("""insert into usage_interactions
                  (id,tmux_session,tmux_pane,task_id,config_id,started_at_ms,finished_at_ms,duration_ms,completion_status,source,confidence,created_at)
                  values(?,?,?,?,?,?,?,?,?,?,?,?)""",
                  (interaction_id,"s",f"%{variant}",task_id,"",1_999_000_000,1_999_001_000,1000,"completed","test","exact",1))
                outcome, rating = (("failed", 1) if variant == 0 else ("solved", 5))
                con.execute("insert into usage_ratings values(?,?,?,?,?)", (interaction_id,1,outcome,rating,""))
                con.execute("""insert into usage_experiment_runs
                  (id,experiment_id,interaction_id,variant_index,harness,motor,model,effort,route_id,status)
                  values(?,?,?,?,?,?,?,?,?,?)""",
                  (f"run-{task_index}-{variant}","exp",interaction_id,variant,"claude",("claude" if variant==0 else "grok"),
                   ("claude-opus-5" if variant==0 else "grok-4.6"),"high",("claude:claude" if variant==0 else "claude:grok"),"completed"))

    paired = cc_usage.experiment_analytics(db, 14)["pairedExperiments"][0]
    assert paired["completePairs"] == 10
    assert paired["eligible"] is True
    assert paired["winnerVariant"] == 1


def test_telemetry_schema_has_no_prompt_or_response_content_columns(tmp_path):
    db = str(tmp_path / "usage.sqlite")
    cc_usage.init_db(db)
    with sqlite3.connect(db) as con:
        columns = {
            table: {row[1] for row in con.execute(f"pragma table_info({table})")}
            for table in ("usage_interactions", "usage_tool_calls", "usage_ratings")
        }
    forbidden = {"prompt", "response", "arguments", "result", "tool_input", "tool_output", "token", "secret"}
    for names in columns.values():
        assert not (names & forbidden)


def test_grok_official_weekly_limit_read_from_cli_log(tmp_path):
    # El CLI de grok escribe su config de creditos (porcentaje, periodo
    # semanal, tier) en logs/unified.jsonl: eso es el limite OFICIAL.
    home = tmp_path / "grok"
    (home / "logs").mkdir(parents=True)
    lines = [
        {"ts": "2026-08-30T10:00:00.000Z", "msg": "billing: fetched credits config",
         "ctx": {"config": {"creditUsagePercent": 12.5, "currentPeriod": {
             "type": "USAGE_PERIOD_TYPE_WEEKLY",
             "start": "2026-08-29T20:54:01+00:00", "end": "2026-09-05T20:54:01+00:00"}},
             "subscriptionTier": "X Premium"}},
        {"ts": "2026-08-31T10:00:00.000Z", "msg": "billing: fetched credits config",
         "ctx": {"config": {"creditUsagePercent": 34.0, "currentPeriod": {
             "type": "USAGE_PERIOD_TYPE_WEEKLY",
             "start": "2026-08-29T20:54:01+00:00", "end": "2026-09-05T20:54:01+00:00"}},
             "subscriptionTier": "X Premium"}},
    ]
    (home / "logs" / "unified.jsonl").write_text(
        "\n".join(json.dumps(x) for x in lines) + "\n")
    now = 1_788_200_000  # antes del fin del periodo
    got = cc_usage.read_grok_credit_limits([home], now=now)
    assert got["percent"] == 34.0            # gana la entrada mas reciente
    assert got["tier"] == "X Premium"
    assert got["resets_at"] > now and "stale_period" not in got
    # Periodo vencido => marcado stale, nunca presentado como vigente
    stale = cc_usage.read_grok_credit_limits([home], now=got["resets_at"] + 10)
    assert stale["stale_period"] is True


def test_provider_comparison_prices_only_turns_with_breakdown(tmp_path):
    # Un turno sin desglose i/o/cache (codex solo reporta totales) NO puede
    # llevar precio: la cobertura debe decirlo en vez de estimar $0 "cubierto".
    db = str(tmp_path / "usage.sqlite")
    prices = tmp_path / "prices.json"
    prices.write_text(json.dumps({"patterns": [
        {"match": "opus", "in": 15, "out": 75, "cacheRead": 1.5, "cacheWrite": 18.75},
        {"match": "gpt", "in": 1.25, "out": 10},
        {"match": "fable", "in": None, "out": None},
    ]}))
    now = 1_788_000_000
    cc_usage.record_turns(db, [
        {"id": "a", "provider": "claude", "agent": "claude", "model": "claude-opus-5",
         "turn_started_at": now - 100, "turn_finished_at": now - 90,
         "input_tokens": 1_000_000, "output_tokens": 100_000,
         "total_tokens": 1_100_000, "source": "t", "confidence": "exact"},
        {"id": "b", "provider": "codex", "agent": "codex", "model": "gpt-5.6-sol",
         "turn_started_at": now - 80, "turn_finished_at": now - 70,
         "input_tokens": 0, "output_tokens": 0,
         "total_tokens": 5_000_000, "source": "t", "confidence": "exact"},
        {"id": "c", "provider": "claude", "agent": "claude", "model": "claude-fable-5",
         "turn_started_at": now - 60, "turn_finished_at": now - 50,
         "input_tokens": 2_000_000, "output_tokens": 0,
         "total_tokens": 2_000_000, "source": "t", "confidence": "exact"},
    ])
    d = cc_usage.provider_comparison(db, str(prices), now=now, days=14)
    est = {e["provider"]: e for e in d["est"]}
    assert est["claude"]["usd"] == round(15 + 7.5, 2)     # solo el turno opus
    assert est["claude"]["coverage"] == round(1_100_000 / 3_100_000 * 100, 1)
    assert est["codex"]["usd"] == 0.0 and est["codex"]["coverage"] == 0.0
    assert len(d["daily"]) == 14
    provs = {c["provider"] for c in d["composition"]}
    assert provs == {"claude", "codex"}


def test_provider_comparison_exposes_heatmap_daily_value_and_model_economics(tmp_path):
    db = str(tmp_path / "usage.sqlite")
    prices = tmp_path / "prices.json"
    prices.write_text(json.dumps({"patterns": [
        {"match": "opus", "in": 15, "out": 75, "cacheRead": 1.5, "cacheWrite": 18.75}]}))
    now = 1_788_000_000
    cc_usage.record_turns(db, [
        {"id": "h1", "provider": "claude", "agent": "claude", "model": "claude-opus-5",
         "turn_started_at": now - 3600, "turn_finished_at": now - 3590,
         "input_tokens": 1_000_000, "output_tokens": 200_000,
         "total_tokens": 1_200_000, "source": "t", "confidence": "exact",
         "duration_ms": 8000},
        {"id": "h2", "provider": "claude", "agent": "claude", "model": "claude-opus-5",
         "turn_started_at": now - 7200, "turn_finished_at": now - 7190,
         "input_tokens": 500_000, "output_tokens": 100_000,
         "total_tokens": 600_000, "source": "t", "confidence": "exact",
         "duration_ms": 4000},
    ])
    d = cc_usage.provider_comparison(db, str(prices), now=now, days=7)
    # Heatmap 7x24 con los tokens en alguna celda
    assert len(d["heat"]) == 7 and all(len(r) == 24 for r in d["heat"])
    assert sum(sum(r) for r in d["heat"]) == 1_800_000
    # Valor por dia: la suma coincide con el estimado total
    est_total = sum(sum(x["providers"].values()) for x in d["est_daily"])
    assert round(est_total, 2) == d["est"][0]["usd"]
    # Economia por modelo: tok/turno, p90 y $/turno presentes
    m = d["models"][0]
    assert m["tokens_per_turn"] == 900_000
    assert m["p90_ms"] == 8000 and m["usd_per_turn"] is not None
