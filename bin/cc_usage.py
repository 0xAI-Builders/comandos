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


def record_turn(db_path, event):
    init_db(db_path)
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
    fields = [
        "id", "provider", "agent", "tmux_session", "tmux_pane", "pane_pwd",
        "git_root", "model", "reasoning_effort", "turn_started_at",
        "turn_finished_at", "input_tokens", "output_tokens", "cache_read_tokens",
        "cache_write_tokens", "total_tokens", "cost_usd", "source",
        "confidence", "raw",
    ]
    values = {k: data.get(k, "") for k in fields}
    with connect(db_path) as con:
        con.execute(
            """
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
            """,
            values,
        )
    return {k: data.get(k) for k in fields}


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


def build_usage_state(db_path, live_panes=None, now=None):
    init_db(db_path)
    live_panes = live_panes or []
    ts = int(now if now is not None else time.time())
    panes = live_panes if live_panes else list_panes(db_path)
    with connect(db_path) as con:
        turns = _rows(con.execute("select * from usage_turns order by turn_finished_at desc"))
        provider_usage = _rows(con.execute("select * from provider_usage_buckets order by end_time desc"))
        provider_costs = _rows(con.execute("select * from provider_cost_buckets order by end_time desc"))
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
        "windows": {},
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


def model_switch_text(provider, preset):
    provider = (provider or "").lower()
    preset = (preset or "diario").lower()
    item = next((p for p in model_presets() if p["id"] == preset), None)
    if not item:
        item = next(p for p in model_presets() if p["id"] == "diario")
    if provider in ("claude", "anthropic"):
        return item["claude"]["command"]
    return item["codex"]["command"]


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


if __name__ == "__main__":
    raise SystemExit(main())


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
