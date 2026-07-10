"""Local usage accounting helpers for ComandOS.

This module is intentionally stdlib-only because it is imported by cc-dash,
which runs as a small user service without project packaging.
"""
import json
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
}


def usage_db_path(hooks_dir=None):
    if hooks_dir is None and os.environ.get("COMANDOS_USAGE_DB"):
        return os.environ["COMANDOS_USAGE_DB"]
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
    "confidence", "raw",
]

TURN_INSERT_SQL = """
insert or replace into usage_turns (
  id, provider, agent, tmux_session, tmux_pane, pane_pwd,
  git_root, model, reasoning_effort, turn_started_at,
  turn_finished_at, input_tokens, output_tokens, cache_read_tokens,
  cache_write_tokens, total_tokens, cost_usd, source,
  confidence, raw
) values (
  :id, :provider, :agent, :tmux_session, :tmux_pane, :pane_pwd,
  :git_root, :model, :reasoning_effort, :turn_started_at,
  :turn_finished_at, :input_tokens, :output_tokens, :cache_read_tokens,
  :cache_write_tokens, :total_tokens, :cost_usd, :source,
  :confidence, :raw
)
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
    data.setdefault("raw", json.dumps(event, sort_keys=True))
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
    sums, latest_model = {}, {}
    for turn in turns:  # vienen ordenados por turn_finished_at desc
        provider = turn.get("provider") or turn.get("agent") or ""
        key = (provider, turn.get("pane_pwd") or "")
        if key not in groups:
            continue
        if turn.get("model") and key not in latest_model:
            latest_model[key] = turn["model"]
        if _as_int(turn.get("turn_finished_at")) < day_start:
            continue
        item = sums.setdefault(key, {"tokens": 0, "cost": 0.0})
        item["tokens"] += _as_int(turn.get("total_tokens"))
        item["cost"] += _as_float(turn.get("cost_usd"))
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


def read_codex_rate_limits(sessions_root=None, now=None, max_files=8):
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
    # Cada sesion de Codex escribe su propio snapshot y pueden convivir varios
    # con edades distintas: gana el timestamp mas reciente, no el mtime.
    best, best_at = None, -1
    for _mtime, path in files[:max_files]:
        snapshot = _last_codex_rate_limit_snapshot(path)
        if not snapshot:
            continue
        limits, captured_at = snapshot
        if captured_at > best_at:
            best, best_at = limits, captured_at
    if not best:
        return []
    rows = []
    for key, lid, kind, window in (
        ("primary", "codex_session", "session", "5h"),
        ("secondary", "codex_weekly", "weekly_all", "7d"),
    ):
        item = best.get(key) if isinstance(best.get(key), dict) else {}
        if item.get("used_percent") is None:
            continue
        minutes = _as_int(item.get("window_minutes"))
        rows.append({
            "id": lid,
            "provider": "codex",
            "kind": kind,
            "label": _codex_window_label(minutes, "Sesion 5h" if key == "primary" else "Semana"),
            "scope": "",
            "percent": round(_as_float(item.get("used_percent")), 1),
            "resets_at": _as_epoch(item.get("resets_at")),
            "severity": "normal",
            "is_active": True,
            "window": window,
            "plan_type": _text(best.get("plan_type")),
            "source": "rollout",
            "confidence": "exact",
            "captured_at": best_at,
        })
    return rows


def _as_list(value):
    return value if isinstance(value, list) else []


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
            select id, created_at, updated_at, source, model_provider, cwd, title,
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
            "turn_started_at": _as_int(row["created_at"]),
            "turn_finished_at": _as_int(row["updated_at"]),
            "total_tokens": _as_int(row["tokens_used"]),
            "source": "codex_state_db",
            "confidence": "local",
            "raw": json.dumps(dict(row), sort_keys=True),
        }
        events.append(event)
    return record_turns(db_path, events)


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
        events.append({
            "id": "opencode-db-" + _text(mid),
            "provider": "opencode",
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
                        "model": _text(msg.get("model")),
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
    record_turn(db_path, event)
    return {"ok": True, "captured": True}


def main(argv=None):
    argv = argv or sys.argv[1:]
    if argv and argv[0] == "capture-hook":
        payload = json.load(sys.stdin)
        print(json.dumps(capture_hook_payload(payload)))
        return 0
    print("usage: cc_usage.py capture-hook", file=sys.stderr)
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
