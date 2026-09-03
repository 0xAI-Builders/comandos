"""Local usage accounting helpers for ComandOS.

This module is intentionally stdlib-only because it is imported by cc-dash,
which runs as a small user service without project packaging.
"""
import json
import re
import os
import sqlite3
import subprocess
import sys
import time
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from hashlib import sha256


DEFAULT_HOOKS_DIR = os.path.expanduser("~/.claude/hooks")
DB_FILENAME = "comandos-usage.sqlite"
USAGE_LIMIT_KEYS = {
    "COMANDOS_CODEX_DAILY_TOKEN_LIMIT",
    "COMANDOS_CODEX_WEEKLY_TOKEN_LIMIT",
    "COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT",
    "COMANDOS_CLAUDE_WEEKLY_TOKEN_LIMIT",
    "COMANDOS_DAILY_BUDGET_USD",
    "COMANDOS_ALERT_THRESHOLDS",
}


def parse_alert_thresholds(text, default=(70, 85, 95)):
    """Umbrales de alerta configurables: '70,85,95'. 'off' = apagadas
    (write_usage_settings borra claves vacias, asi que off necesita valor)."""
    if text is None or str(text).strip() == "":
        return tuple(default)
    if str(text).strip().lower() in ("off", "0", "none"):
        return ()
    vals = set()
    for part in str(text).replace(";", ",").split(","):
        v = _as_int(part.strip().rstrip("%"))
        if 1 <= v <= 100:
            vals.add(v)
    return tuple(sorted(vals))


def usage_db_path(hooks_dir=None):
    if hooks_dir is None and os.environ.get("COMANDOS_USAGE_DB"):
        return os.environ["COMANDOS_USAGE_DB"]
    return os.path.join(hooks_dir or DEFAULT_HOOKS_DIR, DB_FILENAME)


def connect(db_path):
    os.makedirs(os.path.dirname(db_path), exist_ok=True)
    con = sqlite3.connect(db_path, timeout=10)
    con.row_factory = sqlite3.Row
    con.execute("pragma busy_timeout=10000")
    con.execute("pragma foreign_keys=on")
    try:
        con.execute("pragma journal_mode=wal")
    except sqlite3.OperationalError:
        pass
    return con


def init_db(db_path):
    with connect(db_path) as con:
        con.executescript("""
        create table if not exists usage_panes (
          tmux_session text not null,
          tmux_pane text not null,
          pane_pwd text not null,
          git_root text not null,
          tab_label text not null default '',
          agent text not null,
          provider text not null,
          agent_pid integer not null default 0,
          model text not null default '',
          reasoning_effort text not null default '',
          started_at integer not null,
          last_seen_at integer not null,
          raw text not null default '{}',
          primary key (tmux_session, tmux_pane)
        );

        create table if not exists usage_turns (
          id text primary key,
          provider text not null,
          agent text not null,
          tmux_session text not null,
          tmux_pane text not null,
          pane_pwd text not null,
          git_root text not null,
          model text not null default '',
          reasoning_effort text not null default '',
          turn_started_at integer not null,
          turn_finished_at integer not null,
          input_tokens integer not null default 0,
          output_tokens integer not null default 0,
          cache_read_tokens integer not null default 0,
          cache_write_tokens integer not null default 0,
          total_tokens integer not null default 0,
          cost_usd real not null default 0,
          source text not null,
          confidence text not null,
          raw text not null default '{}'
        );

        create table if not exists provider_usage_buckets (
          id text primary key,
          provider text not null,
          start_time integer not null,
          end_time integer not null,
          input_tokens integer not null default 0,
          output_tokens integer not null default 0,
          cache_read_tokens integer not null default 0,
          cache_write_tokens integer not null default 0,
          total_tokens integer not null default 0,
          request_count integer not null default 0,
          project_id text not null default '',
          workspace_id text not null default '',
          user_id text not null default '',
          api_key_id text not null default '',
          model text not null default '',
          service_tier text not null default '',
          confidence text not null default 'exact',
          raw text not null default '{}'
        );

        create table if not exists provider_cost_buckets (
          id text primary key,
          provider text not null,
          start_time integer not null,
          end_time integer not null,
          cost_usd real not null default 0,
          currency text not null default 'usd',
          project_id text not null default '',
          workspace_id text not null default '',
          api_key_id text not null default '',
          line_item text not null default '',
          model text not null default '',
          confidence text not null default 'exact',
          raw text not null default '{}'
        );

        create table if not exists usage_reconciliation (
          id integer primary key autoincrement,
          provider_bucket_id text not null,
          usage_turn_id text not null,
          confidence text not null,
          created_at integer not null
        );

        create table if not exists usage_alerts (
          id text primary key,
          kind text not null,
          level text not null,
          message text not null,
          provider text not null default '',
          tmux_session text not null default '',
          tmux_pane text not null default '',
          created_at integer not null,
          last_seen_at integer not null,
          raw text not null default '{}'
        );

        create table if not exists model_presets (
          id text primary key,
          label text not null,
          description text not null,
          provider text not null default '',
          model text not null default '',
          reasoning_effort text not null default '',
          raw text not null default '{}'
        );

        create table if not exists usage_settings (
          key text primary key,
          value text not null
        );

        create table if not exists usage_alert_rules (
          id text primary key,
          scope text not null,
          target text not null,
          label text not null default '',
          threshold integer not null,
          created_at integer not null
        );
        """)
        _migrate_db(con)


USAGE_SCHEMA_VERSION = 8


def _table_columns(con, table):
    return {row[1] for row in con.execute(f"pragma table_info({table})")}


def _ensure_columns(con, table, columns):
    present = _table_columns(con, table)
    for name, ddl in columns.items():
        if name not in present:
            con.execute(f"alter table {table} add column {name} {ddl}")


def _migrate_db(con):
    """Idempotent additive migration from populated legacy databases."""
    current = int(con.execute("pragma user_version").fetchone()[0])
    if current >= USAGE_SCHEMA_VERSION:
        return
    with con:
        _ensure_columns(con, "usage_turns", {
            "harness": "text not null default ''",
            "motor": "text not null default ''",
            "route_id": "text not null default ''",
            "harness_account": "text not null default 'unknown'",
            "motor_account": "text not null default 'unknown'",
            "interaction_id": "text not null default ''",
            "experiment_run_id": "text not null default ''",
            "tool_profile": "text not null default ''",
            "duration_ms": "integer",
            "outcome": "text not null default 'unknown'",
            "reasoning_tokens": "integer",
        })
        tables = {row[0] for row in con.execute("select name from sqlite_master where type='table'")}
        # v4 adds correlation identifiers without retaining prompt contents.
        if "usage_interactions" in tables:
            _ensure_columns(con, "usage_interactions", {
                "prompt_id": "text not null default ''",
                "agent_session_id": "text not null default ''",
            })
        if "usage_experiments" in tables:
            _ensure_columns(con, "usage_experiments", {
                "project_id": "text not null default ''",
                "primary_metric": "text not null default 'outcome'",
                "min_pairs": "integer not null default 10",
            })
        if "usage_experiment_runs" in tables:
            _ensure_columns(con, "usage_experiment_runs", {
                "task_id": "text not null default ''",
                "project_id": "text not null default ''",
                "launch_order": "integer not null default 0",
            })
        con.executescript("""
        create table if not exists usage_session_configs (
          id text primary key, tmux_session text not null, tmux_pane text not null,
          effective_at integer not null, harness text not null, motor text not null,
          model text not null, effort text not null default '',
          harness_account text not null default 'unknown', motor_account text not null default 'unknown',
          route_id text not null, source text not null, confidence text not null
        );
        create table if not exists usage_tasks (
          id text primary key, task_type text not null default 'unclassified',
          type_source text not null default 'manual', type_confidence text not null default 'exact',
          label text not null default '', design text not null default 'observational',
          created_at integer not null, updated_at integer not null
        );
        create table if not exists usage_interactions (
          id text primary key, tmux_session text not null, tmux_pane text not null,
          task_id text not null default '', config_id text not null default '',
          prompt_id text not null default '', agent_session_id text not null default '',
          started_at_ms integer, first_output_at_ms integer, finished_at_ms integer,
          duration_ms integer, completion_status text not null default 'unknown',
          error_class text not null default '', source text not null, confidence text not null,
          created_at integer not null
        );
        create table if not exists usage_tool_calls (
          id text primary key, interaction_id text not null, sequence integer not null,
          tool_name text not null, tool_family text not null default '',
          started_at_ms integer, finished_at_ms integer, duration_ms integer,
          status text not null default 'unknown', error_class text not null default '',
          confidence text not null, foreign key(interaction_id) references usage_interactions(id) on delete cascade
        );
        create table if not exists usage_ratings (
          interaction_id text primary key, rated_at integer not null,
          outcome text not null default 'unknown', rating integer, note text not null default '',
          foreign key(interaction_id) references usage_interactions(id) on delete cascade
        );
        create table if not exists usage_experiments (
          id text primary key, label text not null, task_type text not null,
          status text not null, design text not null default 'paired',
          project_id text not null default '', primary_metric text not null default 'outcome',
          min_pairs integer not null default 10,
          created_at integer not null, updated_at integer not null
        );
        create table if not exists usage_experiment_variants (
          experiment_id text not null, variant_index integer not null,
          label text not null default '', harness text not null, motor text not null,
          model text not null, effort text not null default '', route_id text not null,
          harness_account text not null default 'unknown', motor_account text not null default 'unknown',
          primary key(experiment_id,variant_index),
          foreign key(experiment_id) references usage_experiments(id) on delete cascade
        );
        create table if not exists usage_experiment_runs (
          id text primary key, experiment_id text not null, interaction_id text not null default '',
          task_id text not null default '', project_id text not null default '',
          variant_index integer not null, harness text not null, motor text not null,
          model text not null, effort text not null default '', route_id text not null,
          harness_account text not null default 'unknown', motor_account text not null default 'unknown',
          tmux_session text not null default '', tmux_pane text not null default '',
          launch_order integer not null default 0,
          status text not null default 'planned', started_at integer, finished_at integer,
          foreign key(experiment_id) references usage_experiments(id) on delete cascade
        );
        create table if not exists usage_changes (
          id text primary key, created_at integer not null,
          origin text not null default 'manual', kind text not null default 'switch',
          tmux_session text not null default '', tmux_pane text not null default '',
          project text not null default '',
          before_model text not null default '', before_effort text not null default '',
          before_route text not null default '',
          after_model text not null default '', after_effort text not null default '',
          after_route text not null default '',
          status text not null default 'applied', note text not null default ''
        );
        create index if not exists idx_usage_changes_created on usage_changes(created_at);
        create table if not exists focus_blocks (
          id text primary key, mode text not null, project text not null default '',
          tmux_session text not null default '', tmux_pane text not null default '',
          planned_minutes integer not null, cycle_index integer not null default 1,
          cycle_total integer not null default 1, started_at_ms integer not null,
          ended_at_ms integer, status text not null default 'running',
          interruptions integer not null default 0, source text not null default 'comandos'
        );
        create table if not exists focus_settings (
          key text primary key, value text not null
        );
        create index if not exists idx_focus_blocks_started on focus_blocks(started_at_ms);
        create index if not exists idx_focus_blocks_project on focus_blocks(project, started_at_ms);
        create index if not exists idx_usage_turns_finished on usage_turns(turn_finished_at);
        create index if not exists idx_usage_turns_route_finished on usage_turns(route_id, turn_finished_at);
        create index if not exists idx_usage_turns_interaction on usage_turns(interaction_id);
        create index if not exists idx_session_configs_pane_time on usage_session_configs(tmux_session, tmux_pane, effective_at);
        create index if not exists idx_interactions_config_time on usage_interactions(config_id, finished_at_ms);
        create index if not exists idx_interactions_prompt on usage_interactions(tmux_session, tmux_pane, prompt_id);
        create index if not exists idx_tasks_type_time on usage_tasks(task_type, created_at);
        create index if not exists idx_experiment_variants_experiment on usage_experiment_variants(experiment_id, variant_index);
        create index if not exists idx_experiment_runs_experiment on usage_experiment_runs(experiment_id, variant_index);
        """)
        con.execute("""update usage_turns set harness=case when harness='' then agent else harness end""")
        con.execute("""update usage_turns set motor=case
          when motor!='' then motor
          when lower(model) like 'grok-%' then 'grok'
          when lower(model) like 'gpt-%' or lower(model) like 'codex%' then 'codex'
          when harness in ('codex','grok') then harness
          else 'claude' end""")
        con.execute("""update usage_turns set route_id=harness||':'||motor where route_id=''""")
        con.execute(f"pragma user_version={USAGE_SCHEMA_VERSION}")


def git_root_for_path(path, run=None):
    if not path:
        return ""
    runner = run or subprocess.run
    try:
        res = runner(
            ["git", "rev-parse", "--show-toplevel"],
            cwd=path,
            capture_output=True,
            text=True,
            timeout=3,
        )
    except TypeError:
        res = runner(["git", "rev-parse", "--show-toplevel"], cwd=path, timeout=3)
    except Exception:
        return path
    if getattr(res, "returncode", 1) == 0:
        root = (getattr(res, "stdout", "") or "").strip()
        if root:
            return root
    return path


def _clean_str(value, limit=500):
    return str(value or "")[:limit]


def _real_model(value, limit=120):
    """Nombre de modelo utilizable o "". Los transcripts de claude marcan los
    mensajes sintéticos (errores de API, avisos) con model "<synthetic>", y un
    proceso lanzado con flags rotos puede reportar un flag como modelo: ninguno
    de los dos debe llegar a chips, tabs ni atribución de uso."""
    name = str(value or "").strip()[:limit]
    if not name or name.startswith(("<", "-")):
        return ""
    return name


def _provider_for_agent(agent):
    if agent == "codex":
        return "codex"
    if agent == "claude":
        return "claude"
    return agent or "unknown"


def normalize_pane_identity(raw, labels=None, now=None):
    labels = labels or {}
    ts = int(now if now is not None else time.time())
    session = _clean_str(raw.get("session"), 80)
    pane = _clean_str(raw.get("pane"), 32)
    pane_pwd = _clean_str(raw.get("cwd") or raw.get("pane_pwd"), 1000)
    agent = _clean_str(raw.get("agent") or "claude", 32)
    provider = _clean_str(raw.get("provider") or _provider_for_agent(agent), 32)
    git_root = _clean_str(raw.get("git_root") or pane_pwd, 1000)
    return {
        "tmux_session": session,
        "tmux_pane": pane,
        "pane_pwd": pane_pwd,
        "git_root": git_root,
        "tab_label": _clean_str(labels.get(session) or raw.get("tab_label"), 120),
        "agent": agent,
        "provider": provider,
        "agent_pid": int(raw.get("pid") or raw.get("agent_pid") or 0),
        "model": _real_model(raw.get("model")),
        "reasoning_effort": _clean_str(raw.get("reasoning_effort"), 40),
        "started_at": int(raw.get("started_at") or ts),
        "last_seen_at": ts,
        "raw": json.dumps(raw, sort_keys=True),
    }


def record_pane(db_path, pane):
    init_db(db_path)
    fields = [
        "tmux_session", "tmux_pane", "pane_pwd", "git_root", "tab_label",
        "agent", "provider", "agent_pid", "model", "reasoning_effort",
        "started_at", "last_seen_at", "raw",
    ]
    values = {k: pane.get(k, "" if k != "agent_pid" else 0) for k in fields}
    with connect(db_path) as con:
        con.execute(
            """
            insert into usage_panes (
              tmux_session, tmux_pane, pane_pwd, git_root, tab_label,
              agent, provider, agent_pid, model, reasoning_effort,
              started_at, last_seen_at, raw
            ) values (
              :tmux_session, :tmux_pane, :pane_pwd, :git_root, :tab_label,
              :agent, :provider, :agent_pid, :model, :reasoning_effort,
              :started_at, :last_seen_at, :raw
            )
            on conflict(tmux_session, tmux_pane) do update set
              pane_pwd=excluded.pane_pwd,
              git_root=excluded.git_root,
              tab_label=excluded.tab_label,
              agent=excluded.agent,
              provider=excluded.provider,
              agent_pid=excluded.agent_pid,
              model=excluded.model,
              reasoning_effort=excluded.reasoning_effort,
              last_seen_at=excluded.last_seen_at,
              raw=excluded.raw
            """,
            values,
        )


def _rows(cur):
    return [dict(row) for row in cur.fetchall()]


def list_panes(db_path):
    init_db(db_path)
    with connect(db_path) as con:
        return _rows(con.execute(
            "select * from usage_panes order by git_root, tmux_session, tmux_pane"
        ))


def read_usage_settings(db_path):
    init_db(db_path)
    with connect(db_path) as con:
        rows = _rows(con.execute("select key, value from usage_settings"))
    return {r["key"]: r["value"] for r in rows if r["key"] in USAGE_LIMIT_KEYS}


def write_usage_settings(db_path, values):
    init_db(db_path)
    clean = {}
    for key, value in (values or {}).items():
        if key not in USAGE_LIMIT_KEYS:
            continue
        text = str(value or "").strip()
        if text:
            clean[key] = text
    with connect(db_path) as con:
        for key in USAGE_LIMIT_KEYS:
            if key in clean:
                con.execute(
                    "insert or replace into usage_settings(key, value) values(?, ?)",
                    (key, clean[key]),
                )
            elif key in (values or {}):
                con.execute("delete from usage_settings where key=?", (key,))
    return read_usage_settings(db_path)


TURN_FIELDS = [
    "id", "provider", "agent", "tmux_session", "tmux_pane", "pane_pwd",
    "git_root", "model", "reasoning_effort", "turn_started_at",
    "turn_finished_at", "input_tokens", "output_tokens", "cache_read_tokens",
    "cache_write_tokens", "total_tokens", "cost_usd", "source",
    "confidence", "raw", "harness", "motor", "route_id",
    "harness_account", "motor_account", "interaction_id", "experiment_run_id",
    "tool_profile", "duration_ms", "outcome", "reasoning_tokens",
]

TURN_INSERT_SQL = """
insert into usage_turns (id, provider, agent, tmux_session, tmux_pane, pane_pwd, git_root, model, reasoning_effort, turn_started_at, turn_finished_at, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, total_tokens, cost_usd, source, confidence, raw, harness, motor, route_id, harness_account, motor_account, interaction_id, experiment_run_id, tool_profile, duration_ms, outcome, reasoning_tokens)
values (:id, :provider, :agent, :tmux_session, :tmux_pane, :pane_pwd, :git_root, :model, :reasoning_effort, :turn_started_at, :turn_finished_at, :input_tokens, :output_tokens, :cache_read_tokens, :cache_write_tokens, :total_tokens, :cost_usd, :source, :confidence, :raw, :harness, :motor, :route_id, :harness_account, :motor_account, :interaction_id, :experiment_run_id, :tool_profile, :duration_ms, :outcome, :reasoning_tokens)
on conflict(id) do update set
  provider=excluded.provider,
  agent=excluded.agent,
  tmux_session=excluded.tmux_session,
  tmux_pane=excluded.tmux_pane,
  pane_pwd=excluded.pane_pwd,
  git_root=excluded.git_root,
  model=excluded.model,
  reasoning_effort=excluded.reasoning_effort,
  turn_started_at=excluded.turn_started_at,
  turn_finished_at=excluded.turn_finished_at,
  input_tokens=excluded.input_tokens,
  output_tokens=excluded.output_tokens,
  cache_read_tokens=excluded.cache_read_tokens,
  cache_write_tokens=excluded.cache_write_tokens,
  total_tokens=excluded.total_tokens,
  cost_usd=excluded.cost_usd,
  source=excluded.source,
  confidence=excluded.confidence,
  raw=excluded.raw,
  harness=excluded.harness,
  motor=excluded.motor,
  route_id=excluded.route_id,
  harness_account=excluded.harness_account,
  motor_account=excluded.motor_account,
  interaction_id=excluded.interaction_id,
  experiment_run_id=excluded.experiment_run_id,
  tool_profile=excluded.tool_profile,
  duration_ms=excluded.duration_ms,
  outcome=excluded.outcome,
  reasoning_tokens=excluded.reasoning_tokens
"""


def _turn_values(event):
    data = dict(event)
    data.setdefault("provider", _provider_for_agent(data.get("agent", "")))
    data.setdefault("agent", data.get("provider") or "")
    data.setdefault("tmux_session", "")
    data.setdefault("tmux_pane", "")
    data.setdefault("pane_pwd", "")
    data.setdefault("git_root", data.get("pane_pwd") or "")
    data.setdefault("model", "")
    data.setdefault("reasoning_effort", "")
    data.setdefault("turn_started_at", int(time.time()))
    data.setdefault("turn_finished_at", data["turn_started_at"])
    data.setdefault("input_tokens", 0)
    data.setdefault("output_tokens", 0)
    data.setdefault("cache_read_tokens", 0)
    data.setdefault("cache_write_tokens", 0)
    data.setdefault("total_tokens", _as_int(data.get("input_tokens")) + _as_int(data.get("output_tokens")))
    data.setdefault("cost_usd", 0.0)
    data.setdefault("source", "cli_turn")
    data.setdefault("confidence", "exact")
    # Raw metadata is opt-in from trusted importers. Never serialize arbitrary
    # event keys here because they may contain prompts, responses or tool payloads.
    data.setdefault("raw", "{}")
    data.setdefault("harness", data.get("agent") or "")
    model_lower = str(data.get("model") or "").lower()
    inferred_motor = ("grok" if model_lower.startswith("grok-") else
                      "codex" if model_lower.startswith(("gpt-", "codex")) else
                      data.get("harness") if data.get("harness") in ("codex", "grok") else "claude")
    data.setdefault("motor", inferred_motor)
    data.setdefault("route_id", f"{data.get('harness')}:{data.get('motor')}")
    data.setdefault("harness_account", "unknown")
    data.setdefault("motor_account", "unknown")
    data.setdefault("interaction_id", "")
    data.setdefault("experiment_run_id", "")
    data.setdefault("tool_profile", "")
    data.setdefault("duration_ms", max(0, (_as_int(data.get("turn_finished_at")) - _as_int(data.get("turn_started_at"))) * 1000))
    data.setdefault("outcome", "unknown")
    data.setdefault("reasoning_tokens", None)
    data["id"] = data.get("id") or _stable_id([
        data.get("provider"), data.get("tmux_session"), data.get("tmux_pane"),
        data.get("turn_started_at"), data.get("turn_finished_at"), data.get("model"),
        data.get("input_tokens"), data.get("output_tokens"), data.get("cost_usd"),
    ])
    return {k: data.get(k, "") for k in TURN_FIELDS}


def record_turn(db_path, event):
    init_db(db_path)
    values = _turn_values(event)
    with connect(db_path) as con:
        con.execute(TURN_INSERT_SQL, values)
    return dict(values)


def record_turns(db_path, events):
    init_db(db_path)
    count = 0
    with connect(db_path) as con:
        for event in events:
            con.execute(TURN_INSERT_SQL, _turn_values(event))
            count += 1
    return count


def _project_rollups(turns, panes):
    projects = {}
    for pane in panes:
        root = pane.get("git_root") or pane.get("pane_pwd") or ""
        confidence = pane.get("confidence") or "detected"
        item = projects.setdefault(root, {
            "git_root": root,
            "cost_usd": 0.0,
            "total_tokens": 0,
            "confidence": confidence,
            "panes": [],
        })
        pane_copy = dict(pane)
        pane_copy.setdefault("cost_usd", 0.0)
        pane_copy.setdefault("total_tokens", 0)
        pane_copy.setdefault("confidence", confidence)
        item["panes"].append(pane_copy)
    pane_index = {
        (p.get("tmux_session"), p.get("tmux_pane")): p
        for project in projects.values()
        for p in project["panes"]
    }
    for turn in turns:
        root = turn.get("git_root") or turn.get("pane_pwd") or ""
        item = projects.setdefault(root, {
            "git_root": root,
            "cost_usd": 0.0,
            "total_tokens": 0,
            "confidence": turn.get("confidence") or "exact",
            "panes": [],
        })
        key = (turn.get("tmux_session"), turn.get("tmux_pane"))
        pane = pane_index.get(key)
        if not pane:
            pane = {
                "tmux_session": turn.get("tmux_session", ""),
                "tmux_pane": turn.get("tmux_pane", ""),
                "pane_pwd": turn.get("pane_pwd", ""),
                "git_root": root,
                "agent": turn.get("agent", ""),
                "provider": turn.get("provider", ""),
                "model": turn.get("model", ""),
                "cost_usd": 0.0,
                "total_tokens": 0,
                "confidence": turn.get("confidence") or "exact",
            }
            item["panes"].append(pane)
            pane_index[key] = pane
        cost = _as_float(turn.get("cost_usd"))
        tokens = _as_int(turn.get("total_tokens"))
        pane["cost_usd"] = round(_as_float(pane.get("cost_usd")) + cost, 6)
        pane["total_tokens"] = _as_int(pane.get("total_tokens")) + tokens
        pane["confidence"] = turn.get("confidence") or pane.get("confidence") or "exact"
        item["cost_usd"] = round(item["cost_usd"] + cost, 6)
        item["total_tokens"] += tokens
        item["confidence"] = turn.get("confidence") or item.get("confidence") or "exact"
    return list(projects.values())


def _setting_float(settings, *keys):
    settings = settings or {}
    for key in keys:
        val = settings.get(key)
        if val not in (None, ""):
            return _as_float(val)
    return 0.0


def _setting_int(settings, *keys):
    settings = settings or {}
    for key in keys:
        val = settings.get(key)
        if val not in (None, ""):
            return _as_int(val)
    return 0


def _token_window(turns, provider, label, window, start_ts, limit):
    aliases = {
        "codex": {"codex", "openai"},
        "claude": {"claude", "anthropic"},
    }.get(provider, {provider})
    used = sum(
        _as_int(t.get("total_tokens"))
        for t in turns
        if (t.get("provider") or t.get("agent")) in aliases
        and _as_int(t.get("turn_finished_at")) >= start_ts
    )
    item = {
        "id": f"{provider}_{window}_tokens",
        "provider": provider,
        "label": label,
        "window": window,
        "metric": "tokens",
        "used": used,
        "limit": limit,
        "source": "local_usage",
    }
    if limit > 0:
        item.update({
            "status": "configured",
            "remaining": max(0, limit - used),
            "percent": min(100, round((used / limit) * 100, 1)),
        })
    else:
        item.update({
            "status": "missing_limit",
            "remaining": None,
            "percent": None,
        })
    return item


def _usage_windows(turns, settings, now):
    day_start = now - 24 * 3600
    week_start = now - 7 * 24 * 3600
    daily_budget = _setting_float(settings, "COMANDOS_DAILY_BUDGET_USD", "COMANDOS_USAGE_DAILY_BUDGET_USD")
    items = [
        _token_window(
            turns, "codex", "Codex diario", "daily", day_start,
            _setting_int(settings, "COMANDOS_CODEX_DAILY_TOKEN_LIMIT", "CODEX_DAILY_TOKEN_LIMIT"),
        ),
        _token_window(
            turns, "codex", "Codex semanal", "weekly", week_start,
            _setting_int(settings, "COMANDOS_CODEX_WEEKLY_TOKEN_LIMIT", "CODEX_WEEKLY_TOKEN_LIMIT"),
        ),
        _token_window(
            turns, "claude", "Claude diario", "daily", day_start,
            _setting_int(settings, "COMANDOS_CLAUDE_DAILY_TOKEN_LIMIT", "CLAUDE_DAILY_TOKEN_LIMIT"),
        ),
        _token_window(
            turns, "claude", "Claude semanal", "weekly", week_start,
            _setting_int(settings, "COMANDOS_CLAUDE_WEEKLY_TOKEN_LIMIT", "CLAUDE_WEEKLY_TOKEN_LIMIT"),
        ),
    ]
    return {
        "daily_budget_usd": daily_budget,
        "items": items,
    }


def _attach_pane_turn_usage(turns, panes, now):
    """Suma a cada pane vivo el uso local de las ultimas 24h de su proveedor
    en su carpeta, y hereda el modelo del turno mas reciente. Si dos panes
    vivos comparten proveedor+carpeta, el numero es del folder, no del pane:
    se marca 'compartido' y nunca se presenta como exacto por pane."""
    day_start = now - 24 * 3600
    groups = {}
    for pane in panes:
        provider = pane.get("provider") or pane.get("agent") or ""
        groups.setdefault((provider, pane.get("pane_pwd") or ""), []).append(pane)
    sums, latest_model, day_model_tokens = {}, {}, {}
    for turn in turns:  # vienen ordenados por turn_finished_at desc
        provider = turn.get("provider") or turn.get("agent") or ""
        key = (provider, turn.get("pane_pwd") or "")
        if key not in groups:
            continue
        # _real_model también aquí: la DB puede traer turnos viejos ya
        # ingeridos con "<synthetic>" o flags como modelo
        turn_model = _real_model(turn.get("model"))
        if turn_model and key not in latest_model:
            latest_model[key] = turn_model
        if _as_int(turn.get("turn_finished_at")) < day_start:
            continue
        if turn_model:
            mt = day_model_tokens.setdefault(key, {})
            mt[turn_model] = mt.get(turn_model, 0) + max(
                1, _as_int(turn.get("total_tokens")))
        item = sums.setdefault(key, {"tokens": 0, "cost": 0.0})
        item["tokens"] += _as_int(turn.get("total_tokens"))
        item["cost"] += _as_float(turn.get("cost_usd"))
    # Modelo DOMINANTE del dia (por tokens), no el del ultimo turno: la CLI
    # intercala turnos de modelos chicos (subagentes) y el chip bailaba.
    for key, mt in day_model_tokens.items():
        latest_model[key] = max(mt.items(), key=lambda kv: kv[1])[0]
    for key, group in groups.items():
        item = sums.get(key)
        model = latest_model.get(key, "")
        for pane in group:
            if model and not pane.get("model"):
                pane["model"] = model
            if not item:
                continue
            pane["total_tokens"] = item["tokens"]
            pane["cost_usd"] = round(item["cost"], 6)
            pane["usage_window"] = "24h"
            pane["confidence"] = "compartido" if len(group) > 1 else "local"


def build_usage_state(db_path, live_panes=None, now=None, settings=None, limits=None):
    init_db(db_path)
    live_panes = live_panes or []
    ts = int(now if now is not None else time.time())
    panes = live_panes if live_panes else list_panes(db_path)
    with connect(db_path) as con:
        turns = _rows(con.execute("select * from usage_turns order by turn_finished_at desc"))
        provider_usage = _rows(con.execute("select * from provider_usage_buckets order by end_time desc"))
        provider_costs = _rows(con.execute("select * from provider_cost_buckets order by end_time desc"))
    _attach_pane_turn_usage(turns, panes, ts)
    projects = _project_rollups(turns, panes)
    turn_cost = sum(_as_float(t.get("cost_usd")) for t in turns)
    provider_cost = sum(_as_float(c.get("cost_usd")) for c in provider_costs)
    turn_tokens = sum(_as_int(t.get("total_tokens")) for t in turns)
    provider_tokens = sum(_as_int(u.get("total_tokens")) for u in provider_usage)
    unattributed = []
    for row in provider_costs:
        unattributed.append({
            "provider": row.get("provider", ""),
            "start_time": row.get("start_time", 0),
            "end_time": row.get("end_time", 0),
            "cost_usd": round(_as_float(row.get("cost_usd")), 6),
            "currency": row.get("currency", "usd"),
            "project_id": row.get("project_id", ""),
            "workspace_id": row.get("workspace_id", ""),
            "api_key_id": row.get("api_key_id", ""),
            "line_item": row.get("line_item", ""),
            "model": row.get("model", ""),
            "confidence": "unattributed",
        })
    providers = {}
    for row in provider_costs:
        p = providers.setdefault(row.get("provider", ""), {
            "provider": row.get("provider", ""),
            "cost_usd": 0.0,
            "total_tokens": 0,
            "active_panes": 0,
        })
        p["cost_usd"] = round(p["cost_usd"] + _as_float(row.get("cost_usd")), 6)
    for row in provider_usage:
        p = providers.setdefault(row.get("provider", ""), {
            "provider": row.get("provider", ""),
            "cost_usd": 0.0,
            "total_tokens": 0,
            "active_panes": 0,
        })
        p["total_tokens"] += _as_int(row.get("total_tokens"))
    if not provider_usage:
        for turn in turns:
            provider = turn.get("provider") or turn.get("agent") or "unknown"
            p = providers.setdefault(provider, {
                "provider": provider,
                "cost_usd": 0.0,
                "total_tokens": 0,
                "active_panes": 0,
            })
            p["total_tokens"] += _as_int(turn.get("total_tokens"))
    for pane in panes:
        provider = pane.get("provider") or pane.get("agent") or "unknown"
        p = providers.setdefault(provider, {
            "provider": provider,
            "cost_usd": 0.0,
            "total_tokens": 0,
            "active_panes": 0,
        })
        p["active_panes"] = _as_int(p.get("active_panes")) + 1
    cost_total = provider_cost if provider_cost > 0 else turn_cost
    token_total = provider_tokens if provider_tokens > 0 else turn_tokens
    series = [
        {"ts": row.get("end_time", 0), "cost_usd": round(_as_float(row.get("cost_usd")), 6)}
        for row in provider_costs
    ]
    return {
        "generated_at": ts,
        "totals": {"cost_usd": round(cost_total, 6), "total_tokens": token_total},
        "providers": list(providers.values()),
        "projects": projects,
        "panes": panes,
        "unattributed": unattributed,
        "alerts": [],
        "limits": list(limits or []),
        "windows": _usage_windows(turns, settings or {}, ts),
        "series": series,
        "credential_health": {},
    }


RULE_SCOPES = ("session", "project", "provider", "limit")


def set_alert_rule(db_path, scope, target, label, threshold):
    """Regla de alerta por objetivo: sesion tmux, proyecto (git_root) o
    proveedor, con presupuesto de tokens por dia (24h moviles)."""
    if scope not in RULE_SCOPES or not target or _as_int(threshold) <= 0:
        return None
    init_db(db_path)
    rule_id = f"{scope}:{target}"
    with connect(db_path) as con:
        con.execute(
            "insert or replace into usage_alert_rules values (?,?,?,?,?,?)",
            (rule_id, scope, _clean_str(target, 300), _clean_str(label, 120),
             _as_int(threshold), int(time.time())))
    return rule_id


def delete_alert_rule(db_path, rule_id):
    init_db(db_path)
    with connect(db_path) as con:
        con.execute("delete from usage_alert_rules where id=?", (rule_id,))


def list_alert_rules(db_path):
    init_db(db_path)
    with connect(db_path) as con:
        return _rows(con.execute(
            "select * from usage_alert_rules order by created_at desc"))


def rule_current_values(db_path, rules, panes, limits=None, now=None):
    """Consumo actual por regla. Proyecto y proveedor van directo a los
    turnos (tokens 24h); sesion suma los grupos unicos (provider+carpeta)
    de sus panes vivos; limit compara contra el % exacto de esa ventana
    del plan (ej. codex sesion 5h)."""
    ts = int(now if now is not None else time.time())
    day_start = ts - 24 * 3600
    out = []
    with connect(db_path) as con:
        for rule in rules or []:
            scope, target = rule.get("scope"), rule.get("target")
            value = 0
            if scope == "limit":
                item = next((l for l in limits or [] if l.get("id") == target), None)
                value = _as_float(item.get("percent")) if item else 0.0
                out.append(dict(rule, value=value, unit="percent",
                                window_resets_at=_as_int(item.get("resets_at")) if item else 0,
                                percent=round(value / rule["threshold"] * 100, 1)
                                if _as_int(rule.get("threshold")) else 0))
                continue
            if scope == "project":
                value = _as_int(con.execute(
                    "select sum(total_tokens) from usage_turns "
                    "where git_root=? and turn_finished_at>=?",
                    (target, day_start)).fetchone()[0])
            elif scope == "provider":
                value = _as_int(con.execute(
                    "select sum(total_tokens) from usage_turns "
                    "where provider=? and turn_finished_at>=?",
                    (target, day_start)).fetchone()[0])
            elif scope == "session":
                seen = set()
                for p in panes or []:
                    if p.get("tmux_session") != target:
                        continue
                    key = (p.get("provider") or p.get("agent"), p.get("pane_pwd"))
                    if key in seen:
                        continue
                    seen.add(key)
                    value += _as_int(p.get("total_tokens"))
            out.append(dict(rule, value=value, unit="tokens",
                            percent=round(value / rule["threshold"] * 100, 1)
                            if _as_int(rule.get("threshold")) else 0))
    return out


def _fmt_tok(n):
    n = _as_int(n)
    if n >= 1_000_000_000:
        return f"{n / 1_000_000_000:.2f}B"
    if n >= 1_000_000:
        return f"{n / 1_000_000:.1f}M"
    if n >= 1000:
        return f"{n // 1000}k"
    return str(n)


def rule_alerts(rules_with_values, now=None):
    """Una alerta por regla: por dia calendario (presupuestos de tokens) o
    por ventana de reset (reglas de % sobre limites del plan)."""
    ts = int(now if now is not None else time.time())
    day = time.strftime("%Y-%m-%d", time.localtime(ts))
    alerts = []
    for rule in rules_with_values or []:
        if _as_float(rule.get("value")) < _as_float(rule.get("threshold")):
            continue
        label = rule.get("label") or rule.get("target")
        if rule.get("scope") == "limit":
            window = rule.get("window_resets_at") or day
            message = (f"{label}: {_as_float(rule.get('value')):.0f}% "
                       f"(umbral {_as_int(rule.get('threshold'))}%)")
            suffix = window
        else:
            message = (f"{label}: {_fmt_tok(rule.get('value'))} tok hoy "
                       f"(presupuesto {_fmt_tok(rule.get('threshold'))})")
            suffix = day
        alerts.append({
            "id": f"rule-{rule.get('id')}-{suffix}",
            "kind": "limit" if rule.get("scope") == "limit" else "budget",
            "level": "warning",
            "provider": rule.get("target") if rule.get("scope") == "provider" else "",
            "message": message,
            "created_at": ts,
        })
    return alerts


def record_alert_once(db_path, alert):
    """Inserta la alerta solo si su id no existe (cooldown natural: el id
    lleva el resets_at de la ventana). Devuelve True si es nueva."""
    init_db(db_path)
    with connect(db_path) as con:
        ts = int(alert.get("created_at") or time.time())
        try:
            con.execute(
                """insert into usage_alerts
                   (id, kind, level, message, provider, tmux_session, tmux_pane,
                    created_at, last_seen_at, raw)
                   values (?,?,?,?,?,?,?,?,?,?)""",
                (alert["id"], alert.get("kind", "limit"), alert.get("level", "warning"),
                 alert.get("message", ""), alert.get("provider", ""), "", "",
                 ts, ts, json.dumps(alert, sort_keys=True)))
            return True
        except sqlite3.IntegrityError:
            con.execute("update usage_alerts set last_seen_at=? where id=?",
                        (ts, alert["id"]))
            return False


def list_alerts(db_path, limit=12):
    init_db(db_path)
    with connect(db_path) as con:
        return _rows(con.execute(
            "select * from usage_alerts order by created_at desc limit ?", (limit,)))


def limit_threshold_alerts(limits, thresholds=(70, 85, 95), now=None):
    """Alertas por porcentaje de limite (la config que importa con suscripcion:
    no hay costos por request, hay ventanas con %). Una por ventana de reset."""
    ts = int(now if now is not None else time.time())
    alerts = []
    for item in limits or []:
        pct = _as_float(item.get("percent"))
        crossed = max((t for t in thresholds if pct >= t), default=None)
        if crossed is None:
            continue
        alerts.append({
            "id": f"{item.get('id')}-{crossed}-{item.get('resets_at')}",
            "kind": "limit",
            "level": "danger" if crossed >= 95 else "warning",
            "provider": _text(item.get("provider")),
            "message": f"{item.get('provider')} {item.get('label')}: "
                       f"{pct:.0f}% usado (umbral {crossed}%)",
            "percent": pct,
            "threshold": crossed,
            "resets_at": _as_int(item.get("resets_at")),
            "created_at": ts,
        })
    return alerts


def calculate_alerts(state, settings=None):
    settings = settings or {}
    alerts = []
    budget = _as_float((state.get("windows") or {}).get("daily_budget_usd"))
    cost = _as_float((state.get("totals") or {}).get("cost_usd"))
    thresholds = settings.get("cost_thresholds") or [0.7, 0.85, 0.95]
    if budget > 0:
        ratio = cost / budget
        for th in sorted(thresholds, reverse=True):
            if ratio >= th:
                level = "danger" if th >= 0.95 else "warning"
                alerts.append({
                    "kind": "budget",
                    "level": level,
                    "message": f"Usage is at {ratio:.0%} of daily budget",
                    "ratio": ratio,
                    "cost_usd": cost,
                    "budget_usd": budget,
                })
                break
    series = state.get("series") or []
    spike_usd = _as_float(settings.get("spike_usd"))
    if spike_usd > 0 and len(series) >= 2:
        delta = _as_float(series[-1].get("cost_usd")) - _as_float(series[0].get("cost_usd"))
        if delta >= spike_usd:
            alerts.append({
                "kind": "spike",
                "level": "warning",
                "message": f"Usage increased by ${delta:.2f}",
                "delta_usd": round(delta, 6),
            })
    return alerts


CLAUDE_OAUTH_CREDS = os.path.expanduser("~/.claude/.credentials.json")
CLAUDE_OAUTH_USAGE_URL = "https://api.anthropic.com/api/oauth/usage"
# Aliases documentados del /model de Claude Code; nada fuera de esta lista
# se inyecta a un pane.
CLAUDE_MODEL_ALIASES = ("haiku", "sonnet", "opus", "fable")


def parse_claude_oauth_limits(payload, now=None):
    """Normaliza la respuesta del endpoint OAuth de uso (la misma fuente que
    el /usage del CLI de Claude Code) a ventanas con porcentaje exacto."""
    ts = int(now if now is not None else time.time())
    rows = []

    def add(lid, kind, label, percent, resets_at, severity="normal",
            scope="", is_active=True, window=""):
        rows.append({
            "id": lid,
            "provider": "claude",
            "kind": kind,
            "label": label,
            "scope": scope,
            "percent": round(_as_float(percent), 1),
            "resets_at": _as_epoch(resets_at),
            "severity": _text(severity or "normal"),
            "is_active": bool(is_active),
            "window": window,
            "source": "oauth",
            "confidence": "exact",
            "captured_at": ts,
        })

    for item in _as_list(payload.get("limits")):
        if not isinstance(item, dict) or item.get("percent") is None:
            continue
        kind = _text(item.get("kind"))
        scope = item.get("scope") if isinstance(item.get("scope"), dict) else {}
        model = scope.get("model") if isinstance(scope.get("model"), dict) else {}
        scope_name = _text(model.get("display_name"))
        if kind == "session":
            lid, label, window = "claude_session", "Sesion 5h", "5h"
        elif kind == "weekly_all":
            lid, label, window = "claude_weekly", "Semana", "7d"
        elif kind == "weekly_scoped":
            slug = "".join(c for c in scope_name.lower() if c.isalnum()) or "modelo"
            lid, label, window = f"claude_weekly_{slug}", f"Semana {scope_name}".strip(), "7d"
        else:
            slug = "".join(c for c in kind.lower() if c.isalnum()) or "otro"
            lid, label, window = f"claude_{slug}", kind or "Limite", ""
        add(lid, kind, label, item.get("percent"), item.get("resets_at"),
            item.get("severity"), scope_name, item.get("is_active", True), window)

    if not rows:
        five = payload.get("five_hour") if isinstance(payload.get("five_hour"), dict) else {}
        seven = payload.get("seven_day") if isinstance(payload.get("seven_day"), dict) else {}
        if five.get("utilization") is not None:
            add("claude_session", "session", "Sesion 5h",
                five.get("utilization"), five.get("resets_at"), window="5h")
        if seven.get("utilization") is not None:
            add("claude_weekly", "weekly_all", "Semana",
                seven.get("utilization"), seven.get("resets_at"), window="7d")
    return rows


def fetch_claude_oauth_limits(creds_path=None, now=None, http=None):
    """Lee el token OAuth local de Claude Code y consulta el porcentaje de uso.
    El token nunca sale de este proceso: no se guarda ni se devuelve."""
    creds_path = creds_path or CLAUDE_OAUTH_CREDS
    ts = int(now if now is not None else time.time())
    health = {"provider": "claude", "source": "oauth",
              "configured": False, "status": "missing"}
    token = ""
    try:
        with open(creds_path) as f:
            token = _text((json.load(f).get("claudeAiOauth") or {}).get("accessToken"))
    except (OSError, ValueError):
        token = ""
    if not token:
        return [], health
    health["configured"] = True
    fetch = http or _http_json
    try:
        payload = fetch(CLAUDE_OAUTH_USAGE_URL, {
            "Authorization": f"Bearer {token}",
            "anthropic-beta": "oauth-2025-04-20",
            "Content-Type": "application/json",
        }, timeout=8)
        health.update({"status": "ok", "last_success_at": ts})
        return parse_claude_oauth_limits(payload, now=ts), health
    except Exception as e:
        health.update({"status": "error", "error": str(e)[:240]})
        return [], health


def parse_groq_ratelimit_headers(headers, now=None):
    """Groq no tiene OAuth de uso: el % sale de x-ratelimit-* de la última
    respuesta HTTP (remaining/limit). reset puede ser epoch o duración (7s)."""
    ts = int(now if now is not None else time.time())
    raw = {str(k).lower(): str(v) for k, v in (headers or {}).items()}

    def _reset_at(value):
        text = str(value or "").strip()
        if not text:
            return 0
        try:
            number = float(text)
            if number >= ts - 86400:
                return int(number)
            if 0 < number < 86400:
                return ts + int(number)
        except ValueError:
            pass
        total, num = 0.0, ""
        for ch in text.lower():
            if ch.isdigit() or ch == ".":
                num += ch
                continue
            if not num:
                continue
            amount = float(num)
            num = ""
            if ch == "h":
                total += amount * 3600
            elif ch == "m":
                total += amount * 60
            elif ch == "s":
                total += amount
        return ts + int(total) if total else 0

    rows = []
    windows = (
        ("requests", "groq_requests", "Requests", "rpm"),
        ("tokens", "groq_tokens", "Tokens", "tpm"),
    )
    for kind, lid, label, window in windows:
        limit = _as_float(raw.get(f"x-ratelimit-limit-{kind}"))
        remaining = raw.get(f"x-ratelimit-remaining-{kind}")
        if limit <= 0 or remaining in (None, ""):
            continue
        left = _as_float(remaining)
        used = max(0.0, limit - left)
        rows.append({
            "id": lid,
            "provider": "groq",
            "kind": "window",
            "label": label,
            "scope": "",
            "percent": round(used / limit * 100, 1),
            "resets_at": _reset_at(raw.get(f"x-ratelimit-reset-{kind}")),
            "severity": "normal",
            "is_active": True,
            "window": window,
            "source": "headers",
            "confidence": "exact",
            "captured_at": ts,
        })
    return rows


def _codex_window_label(minutes, default):
    if minutes == 300:
        return "Sesion 5h"
    if minutes == 10080:
        return "Semana"
    return default if not minutes else f"Ventana {minutes} min"


def _last_codex_rate_limit_snapshot(path, tail_bytes=262144):
    try:
        size = os.path.getsize(path)
        with open(path, "rb") as fh:
            fh.seek(max(0, size - tail_bytes))
            chunk = fh.read().decode("utf-8", "replace")
    except OSError:
        return None
    for line in reversed(chunk.splitlines()):
        if '"rate_limits"' not in line:
            continue
        try:
            data = json.loads(line)
        except ValueError:
            continue
        payload = data.get("payload") if isinstance(data, dict) else None
        limits = payload.get("rate_limits") if isinstance(payload, dict) else None
        if isinstance(limits, dict) and limits:
            return limits, _as_epoch(data.get("timestamp"))
    return None


def read_codex_rate_limits(sessions_root=None, now=None, max_files=16):
    """Ultimo snapshot de rate limits que Codex CLI escribio en sus rollouts.
    Es dato del proveedor (viene con cada turno), posiblemente desfasado:
    captured_at dice de cuando es."""
    sessions_root = os.fspath(sessions_root or os.path.expanduser("~/.codex/sessions"))
    if not os.path.isdir(sessions_root):
        return []
    files = []
    for root, _dirs, names in os.walk(sessions_root):
        for name in names:
            if not name.endswith(".jsonl"):
                continue
            path = os.path.join(root, name)
            try:
                files.append((os.path.getmtime(path), path))
            except OSError:
                continue
    files.sort(reverse=True)
    # Codex reporta rate_limits en primary/secondary, pero la POSICION no es
    # fija: primary puede ser la ventana de 5h O la semanal (segun turno), y a
    # veces secondary viene null. Clasificamos por window_minutes (300=5h,
    # 10080=semana) y nos quedamos con el valor MAS FRESCO de CADA ventana
    # entre los snapshots recientes — asi nunca se "pierde" la de 5h porque el
    # ultimo turno solo trajo la semanal.
    WINDOWS = {  # window_minutes -> (id, kind, window, label)
        300:   ("codex_session", "session",    "5h", "Sesion 5h"),
        10080: ("codex_weekly",  "weekly_all", "7d", "Semana"),
    }
    freshest = {}  # window_minutes -> (captured_at, item, plan_type)
    for _mtime, path in files[:max_files]:
        snapshot = _last_codex_rate_limit_snapshot(path)
        if not snapshot:
            continue
        limits, captured_at = snapshot
        for key in ("primary", "secondary"):
            item = limits.get(key) if isinstance(limits.get(key), dict) else None
            if not item or item.get("used_percent") is None:
                continue
            wm = _as_int(item.get("window_minutes"))
            if wm not in WINDOWS:
                continue
            if wm not in freshest or captured_at > freshest[wm][0]:
                freshest[wm] = (captured_at, item, _text(limits.get("plan_type")))
    ts = int(now if now is not None else time.time())
    rows = []
    for wm, (captured_at, item, plan_type) in freshest.items():
        lid, kind, window, label = WINDOWS[wm]
        # Si el dato es MAS viejo que la propia ventana, ya reseteo desde
        # entonces y el % es basura (una 5h de hace 50h no dice nada). Se
        # oculta hasta que Codex reporte de nuevo — con 1.5x de margen.
        if captured_at and ts - captured_at > wm * 60 * 1.5:
            continue
        rows.append({
            "id": lid,
            "provider": "codex",
            "kind": kind,
            "label": label,
            "scope": "",
            "percent": round(_as_float(item.get("used_percent")), 1),
            "resets_at": _as_epoch(item.get("resets_at")),
            "severity": "normal",
            "is_active": True,
            "window": window,
            "plan_type": plan_type,
            "source": "rollout",
            "confidence": "exact",
            "captured_at": captured_at,
        })
    rows.sort(key=lambda r: r["window"])  # 5h antes que 7d
    return rows


def _as_list(value):
    return value if isinstance(value, list) else []


def _usable_for_coding(obj):
    """Solo modelos que sirven para OpenCode agentico: entienden texto,
    responden texto, y hacen tool-calling. Deja fuera voz (whisper/orpheus),
    clasificadores (prompt-guard) y routers sin tools (compound) — que si se
    eligen dan APIError o 'se comporta raro'."""
    caps = obj.get("capabilities") or {}
    if not caps:
        return True  # opencode viejo sin capabilities: no filtrar de mas
    cin = caps.get("input") or {}
    cout = caps.get("output") or {}
    return bool(caps.get("toolcall")) and cin.get("text", True) and cout.get("text", True)


def parse_opencode_models(verbose_output, max_per_provider=40):
    """Parsea `opencode models --verbose` (linea id + bloque JSON por modelo).
    Filtra: solo activos, usables para codigo (tool-call + texto), sin aliases
    `~`, y si un provider trae demasiados (openrouter) se queda con los gratis."""
    models, decoder = [], json.JSONDecoder()
    text = verbose_output or ""
    i = 0
    while True:
        start = text.find("{", i)
        if start < 0:
            break
        try:
            obj, end = decoder.raw_decode(text[start:])
        except ValueError:
            i = start + 1
            continue
        i = start + end
        if not isinstance(obj, dict) or not obj.get("id") or not obj.get("providerID"):
            continue
        if obj.get("status") not in (None, "active"):
            continue
        if not _usable_for_coding(obj):
            continue
        mid = _text(obj["id"])
        if mid.startswith("~"):
            continue
        ctx = ((obj.get("limit") or {}).get("context")) or 0
        models.append({
            "provider": _text(obj["providerID"]),
            "id": f'{_text(obj["providerID"])}/{mid}',
            "name": _text(obj.get("name") or mid),
            "context": _as_int(ctx),
            "free": "free" in mid.lower() or "free" in _text(obj.get("name")).lower(),
        })
    by_provider = {}
    for m in models:
        by_provider.setdefault(m["provider"], []).append(m)
    result = []
    for provider, items in by_provider.items():
        if len(items) > max_per_provider:
            items = [m for m in items if m["free"]]
        if items:
            result.append({"provider": provider, "models": items})
    result.sort(key=lambda p: p["provider"])
    return result


def opencode_picker_query(model_name, provider):
    """Texto para el buscador fuzzy del picker de opencode: el picker matchea
    por NOMBRE mostrado (no por id), asi que va nombre limpio + provider."""
    clean = "".join(c if c.isalnum() else " " for c in f"{model_name} {provider}")
    return " ".join(clean.split()).lower()


def model_presets():
    return [
        {
            "id": "ahorro",
            "label": "Ahorro",
            "description": "Tareas simples, lectura y cambios mecanicos",
            "claude": {"model": "haiku", "command": "/model haiku"},
            "codex": {"model": "", "command": "/model"},
        },
        {
            "id": "diario",
            "label": "Diario",
            "description": "Programacion diaria balanceada",
            "claude": {"model": "sonnet", "command": "/model sonnet"},
            "codex": {"model": "", "command": "/model"},
        },
        {
            "id": "dificil",
            "label": "Dificil",
            "description": "Debug complejo, arquitectura y refactors grandes",
            "claude": {"model": "opus", "command": "/model opus"},
            "codex": {"model": "", "command": "/model"},
        },
        {
            "id": "maximo",
            "label": "Maximo",
            "description": "Problemas ambiguos, largos o de alto riesgo",
            "claude": {"model": "fable", "command": "/model fable"},
            "codex": {"model": "", "command": "/model"},
        },
    ]


def model_switch_text(provider, preset, model=None):
    provider = (provider or "").lower()
    preset = (preset or "diario").lower()
    model = (model or "").strip().lower()
    if provider not in ("claude", "anthropic"):
        return "/model"
    if model in CLAUDE_MODEL_ALIASES:
        return f"/model {model}"
    # id explicito (claude-fable-5[1m], gpt-5.6-sol[1m] via proxy): se pasa tal cual
    if model and re.match(r"^[a-z0-9][a-z0-9._\-\[\]/]{2,80}$", model):
        return f"/model {model}"
    item = next((p for p in model_presets() if p["id"] == preset), None)
    if not item:
        item = next(p for p in model_presets() if p["id"] == "diario")
    return item["claude"]["command"]


def record_local_codex_threads(db_path, state_db=None, now=None, max_age_days=14):
    state_db = state_db or os.path.expanduser("~/.codex/state_5.sqlite")
    if not os.path.exists(state_db):
        return 0
    ts = int(now if now is not None else time.time())
    cutoff = ts - int(max_age_days) * 24 * 3600
    try:
        con = sqlite3.connect(state_db)
        con.row_factory = sqlite3.Row
        rows = con.execute(
            """
            select id, created_at, updated_at, source, model_provider, cwd,
                   tokens_used, model, reasoning_effort
            from threads
            where tokens_used > 0 and updated_at >= ?
            """,
            (cutoff,),
        ).fetchall()
    except Exception:
        return 0
    finally:
        try:
            con.close()
        except Exception:
            pass
    events = []
    roots = {}
    for row in rows:
        cwd = row["cwd"] or ""
        if cwd not in roots:
            roots[cwd] = git_root_for_path(cwd)
        event = {
            "id": "codex-state-" + _text(row["id"]),
            "provider": "codex",
            "agent": "codex",
            "tmux_session": _text(row["id"]),
            "tmux_pane": "",
            "pane_pwd": cwd,
            "git_root": roots[cwd],
            "model": _text(row["model"]),
            "reasoning_effort": _text(row["reasoning_effort"]),
            # state_5 stores thread lifetime, not individual-turn latency. Keep
            # the cumulative token snapshot, but never mislabel lifetime as a turn.
            "turn_started_at": _as_int(row["updated_at"]),
            "turn_finished_at": _as_int(row["updated_at"]),
            "duration_ms": None,
            "total_tokens": _as_int(row["tokens_used"]),
            "source": "codex_state_db",
            "confidence": "shared",
            "raw": json.dumps({
                "thread_id": _text(row["id"]),
                "model_provider": _text(row["model_provider"]),
                "tokens_used": _as_int(row["tokens_used"]),
                "snapshot_at": _as_int(row["updated_at"]),
            }, sort_keys=True),
        }
        events.append(event)
    return record_turns(db_path, events)


def prune_old_turns(db_path, max_age_days=14, now=None):
    """Borra turnos fuera de la ventana de retencion. El import local es
    insert-or-replace (nunca borra), asi que sin esto la tabla crece sin fin
    y build_usage_state paga por procesar filas viejas que ni se muestran."""
    ts = int(now if now is not None else time.time())
    cutoff = ts - int(max_age_days) * 24 * 3600
    with connect(db_path) as con:
        cur = con.execute("delete from usage_turns where turn_finished_at < ?", (cutoff,))
        return cur.rowcount


def record_local_opencode_db(db_path, oc_db=None, now=None, max_age_days=14):
    """Importa turnos de OpenCode desde su SQLite local (mensajes assistant
    con tokens/costo/modelo). Lectura solo-lectura para no molestar al CLI."""
    oc_db = os.fspath(oc_db or os.path.expanduser("~/.local/share/opencode/opencode.db"))
    if not os.path.exists(oc_db):
        return 0
    ts = int(now if now is not None else time.time())
    cutoff_ms = (ts - int(max_age_days) * 24 * 3600) * 1000
    try:
        con = sqlite3.connect(f"file:{oc_db}?mode=ro", uri=True)
        rows = con.execute(
            """
            select m.id, m.session_id, m.time_created, m.data, s.directory
            from message m left join session s on s.id = m.session_id
            where m.time_created >= ?
            """,
            (cutoff_ms,),
        ).fetchall()
    except Exception:
        return 0
    finally:
        try:
            con.close()
        except Exception:
            pass
    events, roots = [], {}
    for mid, session_id, created_ms, data, directory in rows:
        try:
            msg = json.loads(data)
        except (TypeError, ValueError):
            continue
        if not isinstance(msg, dict) or msg.get("role") != "assistant":
            continue
        tokens = msg.get("tokens") if isinstance(msg.get("tokens"), dict) else {}
        cache = tokens.get("cache") if isinstance(tokens.get("cache"), dict) else {}
        input_tokens = _as_int(tokens.get("input"))
        output_tokens = _as_int(tokens.get("output")) + _as_int(tokens.get("reasoning"))
        cache_read = _as_int(cache.get("read"))
        cache_write = _as_int(cache.get("write"))
        cost = _as_float(msg.get("cost"))
        total = input_tokens + output_tokens + cache_read + cache_write
        if total <= 0 and cost <= 0:
            continue
        cwd = _text(directory)
        if cwd not in roots:
            roots[cwd] = git_root_for_path(cwd)
        provider_id = _text(msg.get("providerID"))
        model_id = _text(msg.get("modelID"))
        bucket = "groq" if provider_id == "groq" else "opencode"
        events.append({
            "id": "opencode-db-" + _text(mid),
            "provider": bucket,
            "agent": "opencode",
            "tmux_session": _text(session_id),
            "tmux_pane": "",
            "pane_pwd": cwd,
            "git_root": roots[cwd],
            "model": f"{provider_id}/{model_id}" if provider_id and model_id else model_id,
            "turn_started_at": _as_int(created_ms) // 1000,
            "turn_finished_at": _as_int(created_ms) // 1000,
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "cache_read_tokens": cache_read,
            "cache_write_tokens": cache_write,
            "total_tokens": total,
            "cost_usd": cost,
            "source": "opencode_db",
            "confidence": "local",
        })
    return record_turns(db_path, events)


def record_local_grok_updates(db_path, homes=None, now=None, max_age_days=14, max_files=200):
    """Import exact per-turn Grok usage without reading message content."""
    import glob
    init_db(db_path)
    homes = homes or [os.path.expanduser("~/.grok")]
    ts = int(now if now is not None else time.time())
    cutoff_ms = (ts - int(max_age_days) * 86400) * 1000
    files = []
    for home in homes:
        for path in glob.glob(os.path.join(os.fspath(home), "sessions", "**", "updates.jsonl"), recursive=True):
            try:
                if os.path.getmtime(path) * 1000 >= cutoff_ms:
                    files.append((os.path.getmtime(path), path))
            except OSError:
                pass
    files.sort(reverse=True)
    events = []
    with connect(db_path) as telemetry:
        for _mtime, path in files[:max_files]:
            summary_path = os.path.join(os.path.dirname(path), "summary.json")
            try:
                summary = json.load(open(summary_path, errors="replace"))
            except Exception:
                summary = {}
            info = summary.get("info") if isinstance(summary.get("info"), dict) else {}
            cwd = _text(info.get("cwd"))
            git_root = git_root_for_path(cwd)
            session_id = _text(info.get("id") or os.path.basename(os.path.dirname(path)))
            model = _text(summary.get("current_model_id"))
            effort = _text(summary.get("reasoning_effort"))
            try:
                lines = open(path, errors="replace")
            except OSError:
                continue
            with lines:
                for line_no, line in enumerate(lines, 1):
                    try:
                        row = json.loads(line)
                    except Exception:
                        continue
                    params = row.get("params") if isinstance(row, dict) else None
                    update = params.get("update") if isinstance(params, dict) else None
                    if not isinstance(update, dict) or update.get("sessionUpdate") != "turn_completed":
                        continue
                    usage = update.get("usage")
                    if not isinstance(usage, dict):
                        continue
                    finished_ms = _as_int(row.get("timestamp"))
                    if 0 < finished_ms < 100_000_000_000:   # el CLI escribe SEGUNDOS
                        finished_ms *= 1000
                    if finished_ms < cutoff_ms:
                        continue
                    prompt_id = _text(update.get("prompt_id"))
                    external_session = _text(params.get("sessionId")) or session_id
                    interaction = telemetry.execute("""select i.id,i.tmux_session,i.tmux_pane,c.harness_account
                      from usage_interactions i left join usage_session_configs c on c.id=i.config_id
                      where i.agent_session_id=? and (?='' or i.prompt_id=?)
                      order by i.started_at_ms desc limit 1""",
                      (external_session, prompt_id, prompt_id)).fetchone()
                    duration_ms = _as_int(usage.get("apiDurationMs"), 0) or None
                    finished = finished_ms // 1000
                    model_usage = usage.get("modelUsage") if isinstance(usage.get("modelUsage"), dict) else {}
                    actual_model = next(iter(model_usage), "") or model
                    input_tokens = _as_int(usage.get("inputTokens"))
                    output_tokens = _as_int(usage.get("outputTokens"))
                    cache_read = _as_int(usage.get("cachedReadTokens"))
                    cache_write = _as_int(usage.get("cacheCreationTokens"))
                    event_id = _text((params.get("_meta") or {}).get("eventId"))
                    events.append({
                        "id": "grok-update-" + (event_id or _stable_id([path, line_no, prompt_id])),
                        "provider": "grok", "agent": "grok", "harness": "grok", "motor": "grok",
                        "route_id": "grok:grok",
                        "tmux_session": interaction["tmux_session"] if interaction else external_session,
                        "tmux_pane": interaction["tmux_pane"] if interaction else "",
                        "pane_pwd": cwd, "git_root": git_root,
                        "model": actual_model, "reasoning_effort": effort,
                        "turn_started_at": finished - ((duration_ms or 0) // 1000),
                        "turn_finished_at": finished, "duration_ms": duration_ms,
                        "input_tokens": input_tokens, "output_tokens": output_tokens,
                        "cache_read_tokens": cache_read, "cache_write_tokens": cache_write,
                        "total_tokens": _as_int(usage.get("totalTokens")) or input_tokens + output_tokens,
                        "reasoning_tokens": _as_int(usage.get("reasoningTokens"), None),
                        "interaction_id": interaction["id"] if interaction else "",
                        "harness_account": (interaction["harness_account"] if interaction else "unknown") or "unknown",
                        "motor_account": "main", "source": "grok_updates", "confidence": "exact",
                        "raw": json.dumps({"session_id": external_session, "prompt_id": prompt_id,
                                           "stop_reason": _text(update.get("stop_reason")),
                                           "model_calls": _as_int(usage.get("modelCalls")),
                                           "num_turns": _as_int(usage.get("numTurns"))}, sort_keys=True),
                    })
    return record_turns(db_path, events)


def provider_comparison(db_path, prices_path=None, now=None, days=14):
    """Comparativa por proveedor: serie diaria, composicion de tokens, top
    modelos y valor API estimado (precios editables, cobertura reportada).
    Solo agregados; nunca contenido."""
    import datetime as _dtm
    ts = int(now if now is not None else time.time())
    days = max(1, min(30, int(days)))
    day0 = int(_dtm.datetime.fromtimestamp(ts)
               .replace(hour=0, minute=0, second=0, microsecond=0).timestamp())
    since = day0 - (days - 1) * 86400
    prices = []
    if prices_path:
        try:
            with open(prices_path) as fh:
                for p in (json.load(fh).get("patterns") or []):
                    try:
                        prices.append((re.compile(str(p.get("match") or ""), re.I), p))
                    except re.error:
                        pass
        except (OSError, ValueError):
            pass

    def price_for(model):
        for rx, p in prices:
            if rx.search(model or ""):
                return p
        return None

    init_db(db_path)
    out = {"days": days, "since": since, "daily": [], "composition": [],
           "models": [], "est": [], "est_daily": [], "heat": [],
           "priced_at": bool(prices)}
    with connect(db_path) as con:
        rows = con.execute(
            "SELECT t.provider, t.model, t.turn_finished_at, t.input_tokens,"
            " t.output_tokens, t.cache_read_tokens, t.cache_write_tokens,"
            " t.total_tokens, COALESCE(NULLIF(t.duration_ms, 0), i.duration_ms)"
            " FROM usage_turns t LEFT JOIN usage_interactions i"
            " ON i.id = t.interaction_id AND t.interaction_id != ''"
            " WHERE t.turn_finished_at >= ?", (since,)).fetchall()
    daily = {}
    comp = {}
    models = {}
    est = {}
    est_daily = {}
    heat = [[0] * 24 for _ in range(7)]   # [dia_semana][hora] tokens locales

    def turn_usd(model, i, o, cr, cw):
        p = price_for(model)
        # Sin desglose i/o/cache (p.ej. rollouts de codex solo traen total)
        # NO se puede poner precio: contarlo como cubierto seria mentir.
        if not p or p.get("in") is None or (i + o + cr + cw) <= 0:
            return None
        return (i * float(p.get("in") or 0) + o * float(p.get("out") or 0)
                + cr * float(p.get("cacheRead") or 0)
                + cw * float(p.get("cacheWrite") or p.get("in") or 0)) / 1e6

    for prov, model, fin, i, o, cr, cw, tot, dur in rows:
        i, o, cr, cw, tot = (int(i or 0), int(o or 0), int(cr or 0),
                             int(cw or 0), int(tot or 0))
        d = since + ((int(fin) - since) // 86400) * 86400
        daily.setdefault(d, {})[prov] = daily.get(d, {}).get(prov, 0) + tot
        lt = _dtm.datetime.fromtimestamp(int(fin))
        heat[lt.weekday()][lt.hour] += tot
        c = comp.setdefault(prov, {"input": 0, "output": 0, "cache_read": 0,
                                   "cache_write": 0, "total": 0, "turns": 0})
        c["input"] += i; c["output"] += o
        c["cache_read"] += cr; c["cache_write"] += cw
        c["total"] += tot; c["turns"] += 1
        m = models.setdefault((prov, model), {"provider": prov, "model": model,
                                              "tokens": 0, "turns": 0, "durs": [],
                                              "input": 0, "output": 0, "usd": 0.0,
                                              "priced_turns": 0})
        m["tokens"] += tot; m["turns"] += 1
        m["input"] += i; m["output"] += o
        if dur: m["durs"].append(int(dur))
        e = est.setdefault(prov, {"usd": 0.0, "priced_tokens": 0, "total_tokens": 0})
        e["total_tokens"] += tot
        usd = turn_usd(model, i, o, cr, cw)
        if usd is not None:
            e["usd"] += usd
            e["priced_tokens"] += tot
            m["usd"] += usd; m["priced_turns"] += 1
            est_daily.setdefault(d, {})[prov] = est_daily.get(d, {}).get(prov, 0.0) + usd
    for d in range(days):
        day = since + d * 86400
        out["daily"].append({"day": day, "providers": daily.get(day, {})})
        out["est_daily"].append({"day": day, "providers": {
            k: round(v, 2) for k, v in (est_daily.get(day) or {}).items()}})
    out["heat"] = heat
    for prov, c in sorted(comp.items(), key=lambda kv: -kv[1]["total"]):
        out["composition"].append({"provider": prov, **c})
    top = sorted(models.values(), key=lambda m: -m["tokens"])[:14]
    # Latencia medida por modelo: si los turnos no la traen, usar la de las
    # interacciones correlacionadas (misma fuente que las sugerencias por pane).
    lat = {}
    try:
        for c in experiment_analytics(db_path, days).get("configurations", []):
            mdl = c.get("model")
            if c.get("durationP50Ms") and (mdl not in lat or (c.get("attempts") or 0) > lat[mdl][2]):
                lat[mdl] = (c.get("durationP50Ms"), c.get("durationP90Ms"), c.get("attempts") or 0)
    except Exception:
        lat = {}
    for m in top:
        durs = sorted(m.pop("durs"))
        m["p50_ms"] = durs[len(durs) // 2] if durs else None
        m["p90_ms"] = durs[min(len(durs) - 1, int(len(durs) * 0.9))] if durs else None
        if m["p50_ms"] is None and m["model"] in lat:
            m["p50_ms"], m["p90_ms"] = lat[m["model"]][0], lat[m["model"]][1]
            m["latency_source"] = "interactions"
        m["tokens_per_turn"] = round(m["tokens"] / m["turns"]) if m["turns"] else 0
        m["usd"] = round(m["usd"], 2)
        m["usd_per_turn"] = round(m["usd"] / m["priced_turns"], 4) if m["priced_turns"] else None
        out["models"].append(m)
    for prov, e in est.items():
        cov = round(e["priced_tokens"] / e["total_tokens"] * 100, 1) if e["total_tokens"] else 0.0
        out["est"].append({"provider": prov, "usd": round(e["usd"], 2), "coverage": cov})
    out["est"].sort(key=lambda x: -x["usd"])
    return out


def read_grok_credit_limits(homes=None, now=None, tail_bytes=524288):
    """Limite semanal OFICIAL de Grok, leido del log local del CLI.

    grok escribe "billing: fetched credits config" (creditUsagePercent,
    currentPeriod, subscriptionTier) en logs/unified.jsonl cada vez que corre.
    Solo campos allowlisted; nada de tokens ni identidad.
    """
    ts = int(now if now is not None else time.time())
    homes = homes or [os.path.expanduser("~/.grok")]
    best = None
    for home in homes:
        path = os.path.join(os.fspath(home), "logs", "unified.jsonl")
        try:
            with open(path, "rb") as fh:
                fh.seek(0, 2)
                size = fh.tell()
                fh.seek(max(0, size - tail_bytes))
                chunk = fh.read().decode(errors="replace")
        except OSError:
            continue
        for line in chunk.splitlines():
            if '"billing: fetched credits config"' not in line and "fetched credits config" not in line:
                continue
            try:
                row = json.loads(line)
            except ValueError:
                continue
            ctx = row.get("ctx") or {}
            cfg = ctx.get("config") or {}
            period = cfg.get("currentPeriod") or {}
            try:
                captured = int(datetime.fromisoformat(
                    (row.get("ts") or "").replace("Z", "+00:00")).timestamp())
            except ValueError:
                captured = 0
            entry = {
                "percent": float(cfg.get("creditUsagePercent") or 0.0),
                "period_start": _iso_epoch(period.get("start")),
                "resets_at": _iso_epoch(period.get("end")),
                "tier": str(ctx.get("subscriptionTier") or "")[:40],
                "captured_at": captured,
            }
            if entry["resets_at"] and (best is None or captured > best["captured_at"]):
                best = entry
    if not best:
        return None
    # Ventana ya vencida => el % es de un periodo viejo; no es dato vigente
    if best["resets_at"] <= ts:
        best["stale_period"] = True
    return best


def _iso_epoch(value):
    try:
        return int(datetime.fromisoformat(str(value)).timestamp())
    except (ValueError, TypeError):
        return 0


def _measured_usage(db_path, provider, now=None):
    """Consumo medido de turnos locales para un provider sin API de limites."""
    import datetime
    ts = int(now if now is not None else time.time())
    day_start = int(datetime.datetime.fromtimestamp(ts)
                    .replace(hour=0, minute=0, second=0, microsecond=0).timestamp())
    init_db(db_path)
    with connect(db_path) as con:
        def agg(since):
            row = con.execute(
                "SELECT COUNT(*), COALESCE(SUM(total_tokens),0), MAX(turn_finished_at)"
                " FROM usage_turns WHERE provider=? AND turn_finished_at >= ?",
                (provider, since)).fetchone()
            return {"turns": int(row[0] or 0), "tokens": int(row[1] or 0),
                    "last_at": int(row[2] or 0)}
        today = agg(day_start)
        week = agg(ts - 7 * 86400)
        daily = []
        for d in range(13, -1, -1):
            start = day_start - d * 86400
            row = con.execute(
                "SELECT COALESCE(SUM(total_tokens),0) FROM usage_turns"
                " WHERE provider=? AND turn_finished_at >= ? AND turn_finished_at < ?",
                (provider, start, start + 86400)).fetchone()
            daily.append({"day": start, "tokens": int(row[0] or 0)})
    if not week["turns"]:
        return None
    return {"tokens_today": today["tokens"], "turns_today": today["turns"],
            "tokens_7d": week["tokens"], "turns_7d": week["turns"],
            "last_at": week["last_at"], "daily": daily}


def grok_measured_usage(db_path, now=None):
    """Consumo Grok medido de turnos locales; xAI no expone limites por API."""
    return _measured_usage(db_path, "grok", now)


def groq_measured_usage(db_path, now=None):
    """Consumo Groq medido (turnos OpenCode con providerID=groq)."""
    return _measured_usage(db_path, "groq", now)


def record_local_claude_jsonl(db_path, projects_root=None, now=None, max_age_days=14, max_files=400):
    projects_root = os.fspath(projects_root or os.path.expanduser("~/.claude/projects"))
    if not os.path.isdir(projects_root):
        return 0
    ts = int(now if now is not None else time.time())
    cutoff = ts - int(max_age_days) * 24 * 3600
    files = []
    for root, dirs, names in os.walk(projects_root):
        dirs[:] = [d for d in dirs if d not in {"tool-results", "memory"}]
        for name in names:
            if not name.endswith(".jsonl"):
                continue
            path = os.path.join(root, name)
            try:
                mtime = os.path.getmtime(path)
            except OSError:
                continue
            if mtime >= cutoff:
                files.append((mtime, path))
    files.sort(reverse=True)
    events = []
    roots = {}
    for _mtime, path in files[:max_files]:
        try:
            with open(path, errors="replace") as fh:
                for line_no, line in enumerate(fh, 1):
                    try:
                        data = json.loads(line)
                    except Exception:
                        continue
                    msg = data.get("message") if isinstance(data, dict) else None
                    usage = msg.get("usage") if isinstance(msg, dict) else None
                    if data.get("type") != "assistant" or not isinstance(usage, dict):
                        continue
                    finished = _as_epoch(data.get("timestamp")) or int(_mtime)
                    if finished < cutoff:
                        continue
                    input_tokens = _as_int(usage.get("input_tokens"))
                    output_tokens = _as_int(usage.get("output_tokens"))
                    cache_read = _as_int(usage.get("cache_read_input_tokens"))
                    cache_write = _as_int(usage.get("cache_creation_input_tokens"))
                    total = input_tokens + output_tokens + cache_read + cache_write
                    if total <= 0:
                        continue
                    cwd = _text(data.get("cwd"))
                    if cwd not in roots:
                        roots[cwd] = git_root_for_path(cwd)
                    stable = _text(data.get("uuid") or data.get("requestId") or f"{path}:{line_no}")
                    event = {
                        "id": "claude-jsonl-" + stable,
                        "provider": "claude",
                        "agent": "claude",
                        "tmux_session": _text(data.get("sessionId")),
                        "tmux_pane": "",
                        "pane_pwd": cwd,
                        "git_root": roots[cwd],
                        "model": _real_model(msg.get("model")),
                        "turn_started_at": finished,
                        "turn_finished_at": finished,
                        "input_tokens": input_tokens,
                        "output_tokens": output_tokens,
                        "cache_read_tokens": cache_read,
                        "cache_write_tokens": cache_write,
                        "total_tokens": total,
                        "source": "claude_jsonl",
                        "confidence": "local",
                        "raw": json.dumps({
                            "path": path,
                            "line": line_no,
                            "uuid": stable,
                            "usage": usage,
                        }, sort_keys=True),
                    }
                    events.append(event)
        except OSError:
            continue
    return record_turns(db_path, events)


def _parse_env_file(path):
    values = {}
    try:
        lines = open(path).read().splitlines()
    except OSError:
        return values
    for line in lines:
        s = line.strip()
        if not s or s.startswith("#"):
            continue
        if s.startswith("export "):
            s = s[7:].strip()
        if "=" not in s:
            continue
        key, val = s.split("=", 1)
        key = key.strip()
        val = val.strip()
        if not key or not re_env_key(key):
            continue
        if len(val) >= 2 and val[0] == val[-1] and val[0] in ("'", '"'):
            val = val[1:-1]
        values[key] = val
    return values


def re_env_key(key):
    return all(c.isalnum() or c == "_" for c in key)


def load_usage_config(paths=None, base=None):
    paths = paths or [
        os.path.expanduser("~/.claude/hooks/cc-notify.conf"),
        os.path.expanduser("~/.claude/hooks/usage.env"),
    ]
    data = {}
    for path in paths:
        data.update(_parse_env_file(path))
    data.update(base or {})
    return data


def capture_hook_payload(payload, db_path=None):
    db_path = db_path or usage_db_path()
    input_tokens = _as_int(payload.get("input_tokens"))
    output_tokens = _as_int(payload.get("output_tokens"))
    cache_read = _as_int(payload.get("cache_read_tokens"))
    cache_write = _as_int(payload.get("cache_write_tokens"))
    cost_usd = _as_float(payload.get("cost_usd"))
    total_tokens = _as_int(payload.get("total_tokens")) or input_tokens + output_tokens
    if not any((input_tokens, output_tokens, cache_read, cache_write, total_tokens, cost_usd)):
        return {"ok": True, "captured": False, "reason": "no_usage_numbers"}
    now_ts = int(time.time())
    event = {
        "provider": payload.get("provider") or _provider_for_agent(payload.get("agent", "")),
        "agent": payload.get("agent") or "",
        "tmux_session": payload.get("tmux_session") or payload.get("session") or "",
        "tmux_pane": payload.get("tmux_pane") or payload.get("pane") or "",
        "pane_pwd": payload.get("pane_pwd") or payload.get("cwd") or "",
        "git_root": payload.get("git_root") or payload.get("pane_pwd") or payload.get("cwd") or "",
        "model": payload.get("model") or "",
        "reasoning_effort": payload.get("reasoning_effort") or "",
        "turn_started_at": _as_int(payload.get("turn_started_at"), now_ts),
        "turn_finished_at": _as_int(payload.get("turn_finished_at"), now_ts),
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cache_read_tokens": cache_read,
        "cache_write_tokens": cache_write,
        "total_tokens": total_tokens,
        "cost_usd": cost_usd,
        "source": payload.get("source") or "hook",
        "confidence": payload.get("confidence") or "exact",
    }
    init_db(db_path)
    with connect(db_path) as con:
        interaction = con.execute("""select id,config_id from usage_interactions
          where tmux_session=? and tmux_pane=? and finished_at_ms is null
          order by started_at_ms desc limit 1""",
          (event["tmux_session"], event["tmux_pane"])).fetchone()
        if interaction:
            event["interaction_id"] = interaction["id"]
            config = con.execute("select * from usage_session_configs where id=?", (interaction["config_id"],)).fetchone()
            if config:
                event.update({key: config[key] for key in (
                    "harness", "motor", "route_id", "harness_account", "motor_account")})
    record_turn(db_path, event)
    return {"ok": True, "captured": True}


def focus_block_start(db_path, event):
    init_db(db_path)
    data = dict(event or {})
    mode = _text(data.get("mode") or "focus")
    if mode not in {"focus", "break"}:
        raise ValueError("invalid focus mode")
    minutes = max(1, min(180, _as_int(data.get("planned_minutes"), 25)))
    started = _as_int(data.get("started_at_ms") or int(time.time() * 1000))
    ident = _stable_id(["focus", started, data.get("tmux_session"), data.get("tmux_pane"), mode])
    with connect(db_path) as con:
        con.execute("""insert into focus_blocks
          (id,mode,project,tmux_session,tmux_pane,planned_minutes,cycle_index,cycle_total,started_at_ms,status,source)
          values(?,?,?,?,?,?,?,?,?,'running',?)""",
          (ident, mode, _text(data.get("project"))[:160], _text(data.get("tmux_session"))[:80],
           _text(data.get("tmux_pane"))[:32], minutes, max(1,_as_int(data.get("cycle_index"),1)),
           max(1,_as_int(data.get("cycle_total"),1)), started, _text(data.get("source")) or "comandos"))
    return {"id": ident, "mode": mode, "planned_minutes": minutes, "started_at_ms": started}


def focus_block_finish(db_path, block_id, status="completed", ended_at_ms=None):
    init_db(db_path)
    if status not in {"completed", "cancelled", "skipped"}:
        raise ValueError("invalid focus status")
    ended = _as_int(ended_at_ms or int(time.time() * 1000))
    with connect(db_path) as con:
        cur = con.execute("""update focus_blocks set ended_at_ms=?,status=?
          where id=? and status='running'""", (ended, status, _text(block_id)))
    return {"id": _text(block_id), "status": status, "updated": cur.rowcount > 0}


def focus_analytics(db_path, days=7, now=None):
    init_db(db_path)
    days = max(1, min(90, int(days)))
    now_ms = _as_int(now or int(time.time() * 1000))
    since = now_ms - days * 86400000
    with connect(db_path) as con:
        rows = _rows(con.execute("""select id,mode,project,tmux_session,tmux_pane,
          planned_minutes,cycle_index,cycle_total,started_at_ms,ended_at_ms,status,interruptions
          from focus_blocks where started_at_ms>=? order by started_at_ms desc""", (since,)))
    focus = [row for row in rows if row["mode"] == "focus"]
    completed = [row for row in focus if row["status"] == "completed"]
    today = datetime.fromtimestamp(now_ms / 1000).date()
    today_rows = [row for row in completed if datetime.fromtimestamp(row["started_at_ms"] / 1000).date() == today]
    by_day = []
    for offset in range(days - 1, -1, -1):
        day = today.fromordinal(today.toordinal() - offset)
        matching = [row for row in completed if datetime.fromtimestamp(row["started_at_ms"] / 1000).date() == day]
        by_day.append({"date": day.isoformat(), "blocks": len(matching),
                       "minutes": sum(row["planned_minutes"] for row in matching)})
    terminal = [row for row in focus if row["status"] != "running"]
    return {
        "days": days,
        "today": {"blocks": len(today_rows), "minutes": sum(row["planned_minutes"] for row in today_rows)},
        "completionRate": (len(completed) / len(terminal)) if terminal else None,
        "interruptions": sum(row["interruptions"] for row in focus),
        "byDay": by_day,
        "recent": rows[:30],
    }


def set_focus_settings(db_path, values):
    allowed = {"focusMinutes", "shortBreakMinutes", "longBreakMinutes", "cycles", "autoBreak", "dailyGoalMinutes"}
    init_db(db_path)
    with connect(db_path) as con:
        for key, value in (values or {}).items():
            if key in allowed:
                con.execute("insert into focus_settings(key,value) values(?,?) on conflict(key) do update set value=excluded.value",
                            (key, json.dumps(value)))
        rows = con.execute("select key,value from focus_settings").fetchall()
    output = {}
    for key, value in rows:
        try: output[key] = json.loads(value)
        except Exception: pass
    return output


def read_focus_settings(db_path):
    return set_focus_settings(db_path, {})


TASK_TYPES = {"implementation", "debugging", "testing", "review", "architecture", "research", "documentation", "operations", "other", "unclassified"}
OUTCOMES = {"solved", "partial", "failed", "unknown"}


def configuration_id(harness, motor, model, effort, account="unknown"):
    return _stable_id([harness, motor, model, effort, account])


def record_session_config(db_path, event):
    init_db(db_path)
    data = dict(event or {})
    session, pane = _text(data.get("tmux_session")), _text(data.get("tmux_pane"))
    harness, motor = _text(data.get("harness")), _text(data.get("motor"))
    model, effort = _text(data.get("model")), _text(data.get("effort"))
    route_id = _text(data.get("route_id")) or f"{harness}:{motor}"
    at = _as_int(data.get("effective_at") or time.time())
    ident = _stable_id([session, pane, at, route_id, model, effort])
    row = (ident, session, pane, at, harness, motor, model, effort,
           _text(data.get("harness_account")) or "unknown",
           _text(data.get("motor_account")) or "unknown", route_id,
           _text(data.get("source")) or "runtime", _text(data.get("confidence")) or "exact")
    with connect(db_path) as con:
        con.execute("""insert into usage_session_configs
          (id,tmux_session,tmux_pane,effective_at,harness,motor,model,effort,harness_account,motor_account,route_id,source,confidence)
          values(?,?,?,?,?,?,?,?,?,?,?,?,?) on conflict(id) do update set
          model=excluded.model,effort=excluded.effort,route_id=excluded.route_id""", row)
    return {"id": ident, "route_id": route_id}


def latest_session_config(db_path, session, pane=""):
    init_db(db_path)
    with connect(db_path) as con:
        row = con.execute("""select * from usage_session_configs
          where tmux_session=? and (tmux_pane=? or (?='' and tmux_pane=''))
          order by effective_at desc limit 1""", (session, pane, pane)).fetchone()
    return dict(row) if row else {}


def _latest_config(con, session, pane, at):
    row = con.execute("""select * from usage_session_configs where tmux_session=? and tmux_pane=? and effective_at<=?
                         order by effective_at desc limit 1""", (session, pane, at)).fetchone()
    return dict(row) if row else {}


def capture_lifecycle(db_path, event):
    """Record prompt/settle boundaries without storing prompt/response text."""
    init_db(db_path)
    data = dict(event or {})
    status = _text(data.get("status"))
    session, pane = _text(data.get("tmux_session")), _text(data.get("tmux_pane"))
    at_ms = _as_int(data.get("at_ms") or int(time.time() * 1000))
    prompt_id = _text(data.get("prompt_id"))
    agent_session_id = _text(data.get("agent_session_id"))
    if not session or not pane:
        return {"captured": False, "reason": "missing_identity"}
    with connect(db_path) as con:
        if status == "working":
            ident = _stable_id(["interaction", session, pane, prompt_id or at_ms])
            cfg = _latest_config(con, session, pane, at_ms // 1000)
            con.execute("""insert into usage_interactions
              (id,tmux_session,tmux_pane,task_id,config_id,prompt_id,agent_session_id,started_at_ms,completion_status,source,confidence,created_at)
              values(?,?,?,?,?,?,?,?,?,?,?,?) on conflict(id) do nothing""",
              (ident,session,pane,_text(data.get("task_id")),cfg.get("id", ""),prompt_id,agent_session_id,at_ms,"unknown",
               _text(data.get("source")) or "hook",_text(data.get("confidence")) or "exact",at_ms//1000))
            return {"captured": True, "interaction_id": ident}
        # Permission/attention notifications are an intermediate state, not a
        # completed response. Keeping the interaction open preserves latency.
        if status == "waiting":
            return {"captured": True, "pending": True}
        row = None
        if prompt_id:
            row = con.execute("""select id,started_at_ms from usage_interactions
                                 where tmux_session=? and tmux_pane=? and prompt_id=? and finished_at_ms is null
                                 order by started_at_ms desc limit 1""",(session,pane,prompt_id)).fetchone()
        if not row:
            row = con.execute("""select id,started_at_ms from usage_interactions where tmux_session=? and tmux_pane=? and finished_at_ms is null
                                 order by started_at_ms desc limit 1""",(session,pane)).fetchone()
        if not row:
            return {"captured": False, "reason": "no_open_interaction"}
        completion = {"done":"completed","error":"failed","idle":"cancelled","cancelled":"cancelled","end":"cancelled"}.get(status,"unknown")
        duration = max(0, at_ms - int(row["started_at_ms"] or at_ms))
        con.execute("""update usage_interactions set finished_at_ms=?,duration_ms=?,completion_status=?,error_class=? where id=?""",
                    (at_ms,duration,completion,_text(data.get("error_class")),row["id"]))
        return {"captured": True, "interaction_id": row["id"], "completion_status": completion}


def capture_tool_event(db_path, event):
    """Record tool timing/name only; arguments and results are never accepted."""
    init_db(db_path)
    data = dict(event or {})
    phase = _text(data.get("phase"))
    session, pane = _text(data.get("tmux_session")), _text(data.get("tmux_pane"))
    tool_name = _text(data.get("tool_name"))[:120]
    at_ms = _as_int(data.get("at_ms") or int(time.time() * 1000))
    if phase not in {"start", "success", "failed"} or not session or not pane or not tool_name:
        return {"captured": False, "reason": "invalid_tool_event"}
    with connect(db_path) as con:
        interaction = con.execute("""select id from usage_interactions
          where tmux_session=? and tmux_pane=? and finished_at_ms is null
          order by started_at_ms desc limit 1""", (session, pane)).fetchone()
        if not interaction:
            return {"captured": False, "reason": "no_open_interaction"}
        interaction_id = interaction["id"]
        external_id = _text(data.get("tool_use_id"))
        family = _tool_family(tool_name)
        if phase == "start":
            sequence = con.execute("select count(*) from usage_tool_calls where interaction_id=?", (interaction_id,)).fetchone()[0]
            ident = _stable_id(["tool", interaction_id, external_id or sequence, tool_name])
            con.execute("""insert into usage_tool_calls
              (id,interaction_id,sequence,tool_name,tool_family,started_at_ms,status,error_class,confidence)
              values(?,?,?,?,?,?,?,'',?) on conflict(id) do nothing""",
              (ident, interaction_id, sequence, tool_name, family, at_ms, "running", _text(data.get("confidence")) or "exact"))
        else:
            if external_id:
                ident = _stable_id(["tool", interaction_id, external_id, tool_name])
                row = con.execute("select started_at_ms from usage_tool_calls where id=?", (ident,)).fetchone()
            else:
                existing = con.execute("""select id,started_at_ms from usage_tool_calls
                  where interaction_id=? and tool_name=? and status='running'
                  order by sequence desc limit 1""", (interaction_id, tool_name)).fetchone()
                ident = existing["id"] if existing else _stable_id(["tool", interaction_id, at_ms, tool_name])
                row = existing
            if not row:
                sequence = con.execute("select count(*) from usage_tool_calls where interaction_id=?", (interaction_id,)).fetchone()[0]
                con.execute("""insert into usage_tool_calls
                  (id,interaction_id,sequence,tool_name,tool_family,started_at_ms,finished_at_ms,duration_ms,status,error_class,confidence)
                  values(?,?,?,?,?,?,?,?,?,?,?)""",
                  (ident, interaction_id, sequence, tool_name, family, None, at_ms, None,
                   phase, "tool_error" if phase == "failed" else "", _text(data.get("confidence")) or "exact"))
            else:
                started = row["started_at_ms"]
                duration = max(0, at_ms - started) if started is not None else None
                con.execute("""update usage_tool_calls set finished_at_ms=?,duration_ms=?,status=?,error_class=? where id=?""",
                            (at_ms, duration, phase, "tool_error" if phase == "failed" else "", ident))
        return {"captured": True, "interaction_id": interaction_id, "tool_call_id": ident}


def _tool_family(name):
    value = str(name or "").lower()
    if any(part in value for part in ("bash", "shell", "terminal", "computer")):
        return "execution"
    if any(part in value for part in ("read", "grep", "glob", "search", "fetch")):
        return "retrieval"
    if any(part in value for part in ("write", "edit", "notebook")):
        return "mutation"
    if any(part in value for part in ("agent", "task", "workflow")):
        return "orchestration"
    return "other"


def set_interaction_task(db_path, interaction_id, task_type, label=""):
    init_db(db_path)
    if task_type not in TASK_TYPES:
        raise ValueError("invalid task type")
    now = int(time.time())
    task_id = _stable_id(["task", interaction_id])
    with connect(db_path) as con:
        exists = con.execute("select 1 from usage_interactions where id=?", (interaction_id,)).fetchone()
        if not exists:
            raise ValueError("interaction not found")
        con.execute("""insert into usage_tasks
          (id,task_type,type_source,type_confidence,label,design,created_at,updated_at)
          values(?,?,'manual','exact',?,'observational',?,?)
          on conflict(id) do update set task_type=excluded.task_type,label=excluded.label,updated_at=excluded.updated_at""",
          (task_id, task_type, _text(label)[:80], now, now))
        con.execute("update usage_interactions set task_id=? where id=?", (task_id, interaction_id))
    return {"interaction_id": interaction_id, "task_id": task_id, "task_type": task_type}


def recent_interactions(db_path, limit=20):
    init_db(db_path)
    limit = max(1, min(100, int(limit)))
    with connect(db_path) as con:
        rows = con.execute("""select i.id,i.started_at_ms,i.finished_at_ms,i.duration_ms,
          i.completion_status,i.confidence,r.rating,r.outcome,t.task_type,
          c.harness,c.motor,c.model,c.effort,c.route_id,c.harness_account,c.motor_account,
          (select count(*) from usage_tool_calls x where x.interaction_id=i.id) as tool_calls,
          (select count(*) from usage_tool_calls x where x.interaction_id=i.id and x.status='failed') as tool_errors
          from usage_interactions i
          left join usage_ratings r on r.interaction_id=i.id
          left join usage_tasks t on t.id=i.task_id
          left join usage_session_configs c on c.id=i.config_id
          where i.finished_at_ms is not null
          order by i.started_at_ms desc limit ?""", (limit,)).fetchall()
    return [dict(row) for row in rows]


def set_interaction_feedback(db_path, interaction_id, outcome="unknown", rating=None, note=""):
    init_db(db_path)
    if outcome not in OUTCOMES:
        raise ValueError("invalid outcome")
    if rating is not None:
        rating = int(rating)
        if rating < 1 or rating > 5:
            raise ValueError("rating must be 1..5")
    note = _text(note)[:240]
    now = int(time.time())
    with connect(db_path) as con:
        exists = con.execute("select 1 from usage_interactions where id=?",(interaction_id,)).fetchone()
        if not exists:
            raise ValueError("interaction not found")
        con.execute("""insert into usage_ratings(interaction_id,rated_at,outcome,rating,note) values(?,?,?,?,?)
                       on conflict(interaction_id) do update set rated_at=excluded.rated_at,outcome=excluded.outcome,rating=excluded.rating,note=excluded.note""",
                    (interaction_id,now,outcome,rating,note))
    return {"interaction_id": interaction_id, "outcome": outcome, "rating": rating}


def percentile(values, q):
    values = sorted(float(value) for value in values if value is not None)
    if not values:
        return None
    if len(values) == 1:
        return values[0]
    position = max(0.0, min(1.0, float(q))) * (len(values) - 1)
    low = int(position)
    high = min(len(values) - 1, low + 1)
    fraction = position - low
    return values[low] + (values[high] - values[low]) * fraction


def wilson_interval(successes, total, z=1.96):
    if total <= 0:
        return (0.0, 1.0)
    p = successes / total
    den = 1 + z*z/total
    center = (p + z*z/(2*total))/den
    margin = z*((p*(1-p)/total + z*z/(4*total*total))**0.5)/den
    return (max(0.0,center-margin),min(1.0,center+margin))


def record_change(db_path, event):
    """Ledger local de cambios de configuración: qué había, qué quedó, por qué."""
    init_db(db_path)
    data = dict(event or {})
    now = _as_int(data.get("created_at") or time.time())
    ident = _stable_id(["change", now, data.get("tmux_session"), data.get("tmux_pane"),
                        data.get("after_model"), data.get("after_route")])
    with connect(db_path) as con:
        con.execute("""insert into usage_changes
          (id,created_at,origin,kind,tmux_session,tmux_pane,project,
           before_model,before_effort,before_route,after_model,after_effort,after_route,status,note)
          values(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) on conflict(id) do nothing""",
          (ident, now, _text(data.get("origin"))[:40] or "manual", _text(data.get("kind"))[:24] or "switch",
           _text(data.get("tmux_session"))[:80], _text(data.get("tmux_pane"))[:32],
           _text(data.get("project"))[:120],
           _text(data.get("before_model"))[:120], _text(data.get("before_effort"))[:16],
           _text(data.get("before_route"))[:80],
           _text(data.get("after_model"))[:120], _text(data.get("after_effort"))[:16],
           _text(data.get("after_route"))[:80],
           _text(data.get("status"))[:24] or "applied", _text(data.get("note"))[:200]))
    return {"id": ident}


def change_ledger(db_path, days=7):
    init_db(db_path)
    since = int(time.time()) - max(1, min(30, int(days))) * 86400
    with connect(db_path) as con:
        rows = _rows(con.execute("""select * from usage_changes where created_at>=?
          order by created_at desc limit 60""", (since,)))
    return rows


def token_guard_report(db_path, now=None):
    """Rolling local anomaly detector. It never pauses or signals a process."""
    init_db(db_path)
    now = _as_int(now or time.time())
    since_hour, since_ten = now - 3600, now - 600
    with connect(db_path) as con:
        rows = _rows(con.execute("""select git_root,model,total_tokens,turn_finished_at,raw
          from usage_turns where source='claude_jsonl' and turn_finished_at>=?
          and lower(model) like 'claude-%'""", (since_hour,)))
    projects = {}
    for row in rows:
        root = row.get("git_root") or "unknown"
        item = projects.setdefault(root, {"project": os.path.basename(root.rstrip("/")) or "unknown",
            "path": root if root != "unknown" else "",
            "calls10m":0,"callsHour":0,"tokensHour":0,"subagentFiles":set(),"models":{}})
        item["callsHour"] += 1
        item["tokensHour"] += _as_int(row.get("total_tokens"))
        if _as_int(row.get("turn_finished_at")) >= since_ten:
            item["calls10m"] += 1
        model = _text(row.get("model"))
        item["models"][model] = item["models"].get(model, 0) + 1
        try:
            path = json.loads(row.get("raw") or "{}").get("path", "")
            if "/subagents/" in path:
                item["subagentFiles"].add(os.path.basename(path))
        except Exception:
            pass
    output=[]
    for item in projects.values():
        models=item.pop("models")
        item["topModel"] = max(models, key=models.get) if models else ""
        item["subagentFiles"] = len(item["subagentFiles"])
        critical = item["calls10m"] >= 300 or item["tokensHour"] >= 100_000_000 or item["subagentFiles"] >= 20
        warning = item["calls10m"] >= 120 or item["tokensHour"] >= 40_000_000 or item["subagentFiles"] >= 8
        item["level"] = "critical" if critical else "warning" if warning else "normal"
        item["action"] = ("Revisar fan-out y encolar un modelo barato; no se congelará la sesión."
                          if critical else "Vigilar ritmo y contexto." if warning else "Dentro de límites locales.")
        output.append(item)
    output.sort(key=lambda row: ({"critical":0,"warning":1,"normal":2}[row["level"]],-row["tokensHour"]))
    return {"generatedAt":now,"policy":{"calls10mWarning":120,"calls10mCritical":300,
            "tokensHourWarning":40_000_000,"tokensHourCritical":100_000_000,
            "subagentsWarning":8,"subagentsCritical":20,"automaticFreeze":False},
            "projects":output,"critical":sum(row["level"]=="critical" for row in output),
            "warning":sum(row["level"]=="warning" for row in output)}


def create_experiment(db_path, label, task_type, variants, project_id="", min_pairs=10):
    init_db(db_path)
    if task_type not in TASK_TYPES:
        raise ValueError("invalid task type")
    if not isinstance(variants, list) or len(variants) != 2:
        raise ValueError("paired experiments require exactly two variants")
    now = int(time.time())
    ident = _stable_id(["experiment", now, label, project_id, *(v.get("route_id") for v in variants)])
    with connect(db_path) as con:
        con.execute("""insert into usage_experiments
          (id,label,task_type,status,design,project_id,primary_metric,min_pairs,created_at,updated_at)
          values(?,?,?,'draft','paired',?,'outcome',?,?,?)""",
          (ident, _text(label)[:120] or "A/B", task_type, _text(project_id)[:80],
           max(2,min(100,int(min_pairs))), now, now))
        for index, variant in enumerate(variants):
            con.execute("""insert into usage_experiment_variants
              (experiment_id,variant_index,label,harness,motor,model,effort,route_id,harness_account,motor_account)
              values(?,?,?,?,?,?,?,?,?,?)""",
              (ident,index,_text(variant.get("label"))[:80],_text(variant.get("harness"))[:32],
               _text(variant.get("motor"))[:32],_text(variant.get("model"))[:120],
               _text(variant.get("effort"))[:16],_text(variant.get("route_id"))[:80],
               _text(variant.get("harness_account"))[:64] or "unknown",
               _text(variant.get("motor_account"))[:64] or "unknown"))
    return {"id": ident, "status": "draft"}


def create_experiment_pair(db_path, experiment_id, label=""):
    init_db(db_path)
    now = int(time.time())
    task_id = _stable_id(["paired-task", experiment_id, now, label])
    with connect(db_path) as con:
        experiment = con.execute("select * from usage_experiments where id=?", (experiment_id,)).fetchone()
        variants = con.execute("select * from usage_experiment_variants where experiment_id=? order by variant_index", (experiment_id,)).fetchall()
        if not experiment or len(variants) != 2:
            raise ValueError("experiment not found or incomplete")
        con.execute("""insert into usage_tasks
          (id,task_type,type_source,type_confidence,label,design,created_at,updated_at)
          values(?,?,'experiment','exact',?,'paired',?,?)""",
          (task_id, experiment["task_type"], _text(label)[:80], now, now))
        runs = []
        for order, variant in enumerate(variants):
            run_id = _stable_id(["experiment-run", task_id, variant["variant_index"]])
            con.execute("""insert into usage_experiment_runs
              (id,experiment_id,task_id,project_id,variant_index,harness,motor,model,effort,route_id,
               harness_account,motor_account,launch_order,status)
              values(?,?,?,?,?,?,?,?,?,?,?,?,?,'planned')""",
              (run_id,experiment_id,task_id,experiment["project_id"],variant["variant_index"],
               variant["harness"],variant["motor"],variant["model"],variant["effort"],variant["route_id"],
               variant["harness_account"],variant["motor_account"],order))
            runs.append({"id": run_id, "variantIndex": variant["variant_index"], "status": "planned"})
        con.execute("update usage_experiments set status='active',updated_at=? where id=?", (now,experiment_id))
    return {"experimentId": experiment_id, "taskId": task_id, "runs": runs}


def list_experiments(db_path):
    init_db(db_path)
    with connect(db_path) as con:
        experiments = _rows(con.execute("""select id,label,task_type,status,design,project_id,primary_metric,min_pairs,created_at,updated_at
          from usage_experiments order by updated_at desc"""))
        for experiment in experiments:
            experiment["variants"] = _rows(con.execute("""select variant_index,label,harness,motor,model,effort,route_id,harness_account,motor_account
              from usage_experiment_variants where experiment_id=? order by variant_index""", (experiment["id"],)))
            experiment["runs"] = _rows(con.execute("""select id,task_id,variant_index,status,tmux_session,tmux_pane,started_at,finished_at
              from usage_experiment_runs where experiment_id=? order by id""", (experiment["id"],)))
    return experiments


def _paired_experiment_analytics(con):
    rows = con.execute("""select e.id as experiment_id,e.label,e.task_type,
      x.variant_index,x.harness,x.motor,x.model,x.effort,x.route_id,
      i.task_id,r.outcome,r.rating,i.duration_ms
      from usage_experiments e join usage_experiment_runs x on x.experiment_id=e.id
      join usage_interactions i on i.id=x.interaction_id
      left join usage_ratings r on r.interaction_id=i.id
      where x.status='completed' and i.finished_at_ms is not null and i.task_id!=''
      order by e.id,i.task_id,x.variant_index""").fetchall()
    experiments = {}
    for raw in rows:
        row = dict(raw)
        exp = experiments.setdefault(row["experiment_id"], {
            "id": row["experiment_id"], "label": row["label"], "taskType": row["task_type"], "tasks": {}, "variants": {},
        })
        exp["tasks"].setdefault(row["task_id"], {})[int(row["variant_index"])] = row
        exp["variants"].setdefault(int(row["variant_index"]), {
            "variantIndex": int(row["variant_index"]), "harness": row["harness"], "motor": row["motor"],
            "model": row["model"], "effort": row["effort"], "routeId": row["route_id"],
            "solved": 0, "failed": 0, "ratings": [], "durations": [],
        })
    output = []
    for exp in experiments.values():
        indexes = sorted(exp["variants"])
        complete = [task for task in exp["tasks"].values()
                    if indexes and all(i in task and task[i].get("outcome") in {"solved", "failed", "partial"} for i in indexes)]
        for task in complete:
            for index in indexes:
                row, variant = task[index], exp["variants"][index]
                if row.get("outcome") == "solved": variant["solved"] += 1
                elif row.get("outcome") == "failed": variant["failed"] += 1
                if row.get("rating") is not None: variant["ratings"].append(int(row["rating"]))
                if row.get("duration_ms") is not None: variant["durations"].append(int(row["duration_ms"]))
        variants = []
        for index in indexes:
            variant = exp["variants"][index]
            binary = variant["solved"] + variant["failed"]
            lo, hi = wilson_interval(variant["solved"], binary)
            durations = sorted(variant.pop("durations")); ratings = variant.pop("ratings")
            variant["successRate"] = variant["solved"] / binary if binary else None
            variant["successCI"] = [lo, hi]
            variant["ratingMean"] = sum(ratings) / len(ratings) if ratings else None
            variant["medianDurationMs"] = durations[len(durations)//2] if durations else None
            variants.append(variant)
        eligible = len(complete) >= 10 and len(variants) >= 2
        winner = None
        if eligible:
            ranked = sorted((v for v in variants if v["successRate"] is not None), key=lambda v: v["successCI"][0], reverse=True)
            if len(ranked) >= 2 and ranked[0]["successCI"][0] > max(v["successCI"][1] for v in ranked[1:]):
                winner = ranked[0]["variantIndex"]
        output.append({"id": exp["id"], "label": exp["label"], "taskType": exp["taskType"],
                       "completePairs": len(complete), "eligible": eligible, "winnerVariant": winner,
                       "variants": variants})
    return output


def experiment_analytics(db_path, days=14, task_type=""):
    init_db(db_path)
    days = max(1, min(30, int(days)))
    since_ms = int((time.time()-days*86400)*1000)
    where = "where i.finished_at_ms>=?"
    args = [since_ms]
    if task_type:
        if task_type not in TASK_TYPES:
            raise ValueError("invalid task type")
        where += " and t.task_type=?"; args.append(task_type)
    with connect(db_path) as con:
        rows = con.execute(f"""select i.*,r.outcome,r.rating,t.task_type,
          c.harness,c.motor,c.model,c.effort,c.route_id,c.harness_account,c.motor_account,
          (select sum(u.total_tokens) from usage_turns u where u.interaction_id=i.id) as interaction_tokens,
          (select sum(u.cache_read_tokens) from usage_turns u where u.interaction_id=i.id) as cache_read_tokens,
          (select sum(u.reasoning_tokens) from usage_turns u where u.interaction_id=i.id) as reasoning_tokens,
          (select count(*) from usage_tool_calls x where x.interaction_id=i.id) as tool_calls,
          (select count(*) from usage_tool_calls x where x.interaction_id=i.id and x.status='failed') as tool_errors
          from usage_interactions i
          left join usage_ratings r on r.interaction_id=i.id
          left join usage_tasks t on t.id=i.task_id
          left join usage_session_configs c on c.id=i.config_id
          {where}""",args).fetchall()
        paired = _paired_experiment_analytics(con)
    groups = {}
    for row in rows:
        d = dict(row); key = d.get("route_id") or "unknown"
        key += "|"+(d.get("model") or "")+"|"+(d.get("effort") or "")+"|"+(d.get("harness_account") or "unknown")+"|"+(d.get("motor_account") or "unknown")
        g = groups.setdefault(key,{"configKey":key,"harness":d.get("harness") or "unknown","motor":d.get("motor") or "unknown",
                                   "model":d.get("model") or "","effort":d.get("effort") or "",
                                   "harnessAccount":d.get("harness_account") or "unknown","motorAccount":d.get("motor_account") or "unknown","attempts":0,"labeled":0,"solved":0,"failed":0,"partial":0,"ratings":[],"durations":[],"tokens":[],"cacheRead":0,"reasoningTokens":0,"toolCalls":0,"toolErrors":0,"taskIds":set(),"days":set()})
        g["attempts"]+=1
        if d.get("task_id"): g["taskIds"].add(d["task_id"])
        if d.get("finished_at_ms"): g["days"].add(datetime.fromtimestamp(d["finished_at_ms"]/1000,timezone.utc).date().isoformat())
        outcome=d.get("outcome") or "unknown"
        if outcome in ("solved","failed","partial"):
            g["labeled"]+=1;g[outcome]+=1
        if d.get("rating") is not None:g["ratings"].append(int(d["rating"]))
        if d.get("duration_ms") is not None:g["durations"].append(int(d["duration_ms"]))
        if d.get("interaction_tokens") is not None:g["tokens"].append(int(d["interaction_tokens"]))
        g["cacheRead"] += _as_int(d.get("cache_read_tokens"))
        g["reasoningTokens"] += _as_int(d.get("reasoning_tokens"))
        g["toolCalls"] += _as_int(d.get("tool_calls"))
        g["toolErrors"] += _as_int(d.get("tool_errors"))
    out=[]
    for g in groups.values():
        judged=g["labeled"];lo,hi=wilson_interval(g["solved"],judged)
        g["successRate"]=(g["solved"]/judged if judged else None);g["partialRate"]=(g["partial"]/judged if judged else None);g["successCI"]=[lo,hi]
        g["ratingMean"]=(sum(g["ratings"])/len(g["ratings"]) if g["ratings"] else None);g["ratingN"]=len(g["ratings"])
        g["durationP50Ms"]=percentile(g["durations"],.5);g["durationP90Ms"]=percentile(g["durations"],.9);g["medianDurationMs"]=g["durationP50Ms"]
        g["tokensP50"]=percentile(g["tokens"],.5);g["tokensP90"]=percentile(g["tokens"],.9);g["medianTokens"]=g["tokensP50"]
        g["toolErrorRate"]=(g["toolErrors"]/g["toolCalls"] if g["toolCalls"] else None)
        g["distinctTasks"]=len(g.pop("taskIds"));g["activeDays"]=len(g.pop("days"));g.pop("ratings");g.pop("durations");g.pop("tokens")
        g["eligible"] = g["labeled"]>=12 and g["distinctTasks"]>=6 and g["activeDays"]>=3 and (hi-lo)<=0.50
        g["evidence"] = "eligible" if g["eligible"] else "insufficient"
        out.append(g)
    return {"days":days,"taskType":task_type or "all","configurations":sorted(out,key=lambda x:(not x["eligible"],-(x["successCI"][0] if x["eligible"] else 0),-x["attempts"])),
            "policy":{"observationalMinLabeled":12,"minDistinctTasks":6,"minActiveDays":3,"recommendationMinLabeled":20,"pairedMinComplete":10},
            "mode":"observational","pairedExperiments":paired,
            "disclaimer":"Direccional; solo experimentos pareados pueden declarar ganador."}


def main(argv=None):
    argv = argv or sys.argv[1:]
    if argv and argv[0] == "capture-hook":
        payload = json.load(sys.stdin)
        print(json.dumps(capture_hook_payload(payload)))
        return 0
    if argv and argv[0] == "lifecycle":
        payload = json.load(sys.stdin)
        print(json.dumps(capture_lifecycle(usage_db_path(), payload)))
        return 0
    if argv and argv[0] == "tool-event":
        payload = json.load(sys.stdin)
        print(json.dumps(capture_tool_event(usage_db_path(), payload)))
        return 0
    print("usage: cc_usage.py capture-hook|lifecycle|tool-event", file=sys.stderr)
    return 2


def _http_json(url, headers, timeout=8):
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req, timeout=timeout) as res:
        return json.loads(res.read().decode("utf-8"))


def _query(base, params):
    return base + "?" + urllib.parse.urlencode(params, doseq=True)


def fetch_openai_usage(env, now=None):
    key = (env or {}).get("OPENAI_ADMIN_KEY", "")
    health = {"provider": "openai", "configured": bool(key), "status": "missing"}
    if not key:
        return [], [], health
    ts = int(now if now is not None else time.time())
    start = ts - 24 * 3600
    headers = {"Authorization": f"Bearer {key}", "Content-Type": "application/json"}
    try:
        usage_payload = _http_json(_query(
            "https://api.openai.com/v1/organization/usage/completions",
            {
                "start_time": start,
                "end_time": ts,
                "bucket_width": "1h",
                "limit": 24,
                "group_by": ["project_id", "api_key_id", "model", "service_tier"],
            },
        ), headers)
        cost_payload = _http_json(_query(
            "https://api.openai.com/v1/organization/costs",
            {
                "start_time": start,
                "end_time": ts,
                "bucket_width": "1d",
                "limit": 7,
                "group_by": ["project_id", "api_key_id", "line_item"],
            },
        ), headers)
        health.update({"status": "ok", "last_success_at": ts})
        return parse_openai_usage_buckets(usage_payload), parse_openai_cost_buckets(cost_payload), health
    except Exception as e:
        health.update({"status": "error", "error": str(e)[:240]})
        return [], [], health


def fetch_anthropic_usage(env, now=None):
    key = (env or {}).get("ANTHROPIC_ADMIN_KEY", "")
    health = {"provider": "anthropic", "configured": bool(key), "status": "missing"}
    if not key:
        return [], [], health
    ts = int(now if now is not None else time.time())
    start = datetime.fromtimestamp(ts - 24 * 3600, timezone.utc).isoformat().replace("+00:00", "Z")
    end = datetime.fromtimestamp(ts, timezone.utc).isoformat().replace("+00:00", "Z")
    headers = {"x-api-key": key, "anthropic-version": "2023-06-01"}
    try:
        usage_payload = _http_json(_query(
            "https://api.anthropic.com/v1/organizations/usage_report/messages",
            {"starting_at": start, "ending_at": end, "bucket_width": "1h"},
        ), headers)
        cost_payload = _http_json(_query(
            "https://api.anthropic.com/v1/organizations/cost_report",
            {"starting_at": start, "ending_at": end, "bucket_width": "1d"},
        ), headers)
        health.update({"status": "ok", "last_success_at": ts})
        return parse_anthropic_usage_rows(usage_payload), parse_anthropic_cost_rows(cost_payload), health
    except Exception as e:
        health.update({"status": "error", "error": str(e)[:240]})
        return [], [], health


def _as_int(value, default=0):
    try:
        if value is None or value == "":
            return default
        return int(value)
    except (TypeError, ValueError):
        return default


def _as_float(value, default=0.0):
    try:
        if value is None or value == "":
            return default
        return float(value)
    except (TypeError, ValueError):
        return default


def _as_epoch(value):
    if isinstance(value, (int, float)):
        return int(value)
    if not value:
        return 0
    s = str(value)
    try:
        return int(float(s))
    except ValueError:
        pass
    try:
        if s.endswith("Z"):
            s = s[:-1] + "+00:00"
        return int(datetime.fromisoformat(s).timestamp())
    except ValueError:
        return 0


def _items(payload):
    if isinstance(payload, list):
        return payload
    for key in ("data", "results", "rows", "items"):
        val = payload.get(key) if isinstance(payload, dict) else None
        if isinstance(val, list):
            return val
    return []


def _text(value):
    return "" if value is None else str(value)


def parse_openai_usage_buckets(payload):
    rows = []
    for bucket in _items(payload):
        start = _as_epoch(bucket.get("start_time"))
        end = _as_epoch(bucket.get("end_time"))
        for result in bucket.get("results") or []:
            input_tokens = _as_int(result.get("input_tokens"))
            output_tokens = _as_int(result.get("output_tokens"))
            rows.append({
                "provider": "openai",
                "start_time": start,
                "end_time": end,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "cache_read_tokens": _as_int(result.get("input_cached_tokens")),
                "cache_write_tokens": 0,
                "total_tokens": input_tokens + output_tokens,
                "request_count": _as_int(result.get("num_model_requests")),
                "project_id": _text(result.get("project_id")),
                "workspace_id": "",
                "user_id": _text(result.get("user_id")),
                "api_key_id": _text(result.get("api_key_id")),
                "model": _text(result.get("model")),
                "service_tier": _text(result.get("service_tier")),
                "confidence": "exact",
            })
    return rows


def parse_openai_cost_buckets(payload):
    rows = []
    for bucket in _items(payload):
        start = _as_epoch(bucket.get("start_time"))
        end = _as_epoch(bucket.get("end_time"))
        for result in bucket.get("results") or []:
            amount = result.get("amount") or {}
            rows.append({
                "provider": "openai",
                "start_time": start,
                "end_time": end,
                "cost_usd": _as_float(amount.get("value")),
                "currency": _text(amount.get("currency") or "usd").lower(),
                "project_id": _text(result.get("project_id")),
                "workspace_id": "",
                "api_key_id": _text(result.get("api_key_id")),
                "line_item": _text(result.get("line_item")),
                "model": _text(result.get("model")),
                "confidence": "exact",
            })
    return rows


def parse_anthropic_usage_rows(payload):
    rows = []
    for item in _items(payload):
        input_tokens = _as_int(item.get("input_tokens") or item.get("input_token_count"))
        output_tokens = _as_int(item.get("output_tokens") or item.get("output_token_count"))
        rows.append({
            "provider": "anthropic",
            "start_time": _as_epoch(item.get("starting_at") or item.get("start_time")),
            "end_time": _as_epoch(item.get("ending_at") or item.get("end_time")),
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "cache_read_tokens": _as_int(
                item.get("cache_read_input_tokens") or item.get("cache_read_tokens")
            ),
            "cache_write_tokens": _as_int(
                item.get("cache_creation_input_tokens") or item.get("cache_write_tokens")
            ),
            "total_tokens": input_tokens + output_tokens,
            "request_count": _as_int(item.get("requests") or item.get("request_count")),
            "project_id": "",
            "workspace_id": _text(item.get("workspace_id")),
            "user_id": _text(item.get("user_id") or item.get("actor_id")),
            "api_key_id": _text(item.get("api_key_id")),
            "model": _text(item.get("model")),
            "service_tier": _text(item.get("service_tier")),
            "confidence": "exact",
        })
    return rows


def parse_anthropic_cost_rows(payload):
    rows = []
    for item in _items(payload):
        amount = item.get("amount") if isinstance(item.get("amount"), dict) else {}
        cost = item.get("cost_usd")
        if cost is None:
            cost = item.get("cost")
        if cost is None:
            cost = amount.get("value")
        rows.append({
            "provider": "anthropic",
            "start_time": _as_epoch(item.get("starting_at") or item.get("start_time")),
            "end_time": _as_epoch(item.get("ending_at") or item.get("end_time")),
            "cost_usd": _as_float(cost),
            "currency": _text(item.get("currency") or amount.get("currency") or "usd").lower(),
            "project_id": "",
            "workspace_id": _text(item.get("workspace_id")),
            "api_key_id": _text(item.get("api_key_id")),
            "line_item": _text(item.get("service") or item.get("line_item")),
            "model": _text(item.get("model")),
            "confidence": "exact",
        })
    return rows


def _stable_id(parts):
    joined = "\x1f".join(_text(p) for p in parts)
    return sha256(joined.encode()).hexdigest()[:32]


def record_provider_usage(db_path, provider, rows):
    init_db(db_path)
    fields = [
        "id", "provider", "start_time", "end_time", "input_tokens", "output_tokens",
        "cache_read_tokens", "cache_write_tokens", "total_tokens", "request_count",
        "project_id", "workspace_id", "user_id", "api_key_id", "model",
        "service_tier", "confidence", "raw",
    ]
    with connect(db_path) as con:
        for row in rows:
            data = dict(row)
            data["provider"] = provider or data.get("provider") or ""
            data.setdefault("raw", json.dumps(row, sort_keys=True))
            data["id"] = _stable_id([
                data.get("provider"), data.get("start_time"), data.get("end_time"),
                data.get("project_id"), data.get("workspace_id"), data.get("user_id"),
                data.get("api_key_id"), data.get("model"), data.get("service_tier"),
            ])
            values = {k: data.get(k, 0 if k.endswith("_tokens") or k in ("start_time", "end_time", "request_count") else "") for k in fields}
            con.execute(
                """
                insert or replace into provider_usage_buckets (
                  id, provider, start_time, end_time, input_tokens, output_tokens,
                  cache_read_tokens, cache_write_tokens, total_tokens, request_count,
                  project_id, workspace_id, user_id, api_key_id, model,
                  service_tier, confidence, raw
                ) values (
                  :id, :provider, :start_time, :end_time, :input_tokens, :output_tokens,
                  :cache_read_tokens, :cache_write_tokens, :total_tokens, :request_count,
                  :project_id, :workspace_id, :user_id, :api_key_id, :model,
                  :service_tier, :confidence, :raw
                )
                """,
                values,
            )


def record_provider_costs(db_path, provider, rows):
    init_db(db_path)
    fields = [
        "id", "provider", "start_time", "end_time", "cost_usd", "currency",
        "project_id", "workspace_id", "api_key_id", "line_item", "model",
        "confidence", "raw",
    ]
    with connect(db_path) as con:
        for row in rows:
            data = dict(row)
            data["provider"] = provider or data.get("provider") or ""
            data.setdefault("currency", "usd")
            data.setdefault("raw", json.dumps(row, sort_keys=True))
            data["id"] = _stable_id([
                data.get("provider"), data.get("start_time"), data.get("end_time"),
                data.get("project_id"), data.get("workspace_id"), data.get("api_key_id"),
                data.get("line_item"), data.get("model"),
            ])
            values = {k: data.get(k, 0 if k in ("start_time", "end_time", "cost_usd") else "") for k in fields}
            con.execute(
                """
                insert or replace into provider_cost_buckets (
                  id, provider, start_time, end_time, cost_usd, currency,
                  project_id, workspace_id, api_key_id, line_item, model,
                  confidence, raw
                ) values (
                  :id, :provider, :start_time, :end_time, :cost_usd, :currency,
                  :project_id, :workspace_id, :api_key_id, :line_item, :model,
                  :confidence, :raw
                )
                """,
                values,
            )


if __name__ == "__main__":
    raise SystemExit(main())
