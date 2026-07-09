"""Local usage accounting helpers for ComandOS.

This module is intentionally stdlib-only because it is imported by cc-dash,
which runs as a small user service without project packaging.
"""
import json
import os
import sqlite3
import subprocess
import time


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
