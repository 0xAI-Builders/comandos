"""Local usage accounting helpers for ComandOS.

This module is intentionally stdlib-only because it is imported by cc-dash,
which runs as a small user service without project packaging.
"""
import json
import os
import sqlite3
import subprocess
import time
from datetime import datetime, timezone
from hashlib import sha256


DEFAULT_HOOKS_DIR = os.path.expanduser("~/.claude/hooks")
DB_FILENAME = "comandos-usage.sqlite"


def usage_db_path(hooks_dir=None):
    return os.path.join(hooks_dir or DEFAULT_HOOKS_DIR, DB_FILENAME)


def connect(db_path):
    os.makedirs(os.path.dirname(db_path), exist_ok=True)
    con = sqlite3.connect(db_path)
    con.row_factory = sqlite3.Row
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
        """)


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
        "model": _clean_str(raw.get("model"), 120),
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
