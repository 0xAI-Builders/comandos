#!/usr/bin/env python3
"""Safe readers for Grok Build local metadata (never returns tokens)."""
from __future__ import annotations

import glob
import json
import os
from pathlib import Path
from typing import Any


def _home(path: str | None = None) -> Path:
    return Path(os.path.expanduser(path or os.environ.get("GROK_HOME") or "~/.grok")).resolve()


def account_homes() -> list[tuple[str, Path]]:
    out = [("main", _home("~/.grok"))]
    root = _home("~/.grok-accounts")
    if root.is_dir():
        out.extend((p.name, p.resolve()) for p in sorted(root.iterdir()) if p.is_dir() and not p.name.startswith((".", "-")))
    return out


def auth_identity(home: str | os.PathLike[str]) -> dict[str, Any]:
    path = Path(home) / "auth.json"
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        rec = next((v for v in data.values() if isinstance(v, dict)), {})
    except Exception:
        rec = {}
    return {
        "authenticated": bool(rec.get("refresh_token") or rec.get("key")),
        "name": rec.get("first_name") or rec.get("email") or rec.get("user_id") or rec.get("principal_id") or "",
        "email": rec.get("email") or "",
        "retentionOptOut": rec.get("coding_data_retention_opt_out") is True,
    }


def accounts_public() -> list[dict[str, Any]]:
    out = []
    for alias, home in account_homes():
        ident = auth_identity(home)
        out.append({"alias": alias, "home": str(home), **ident})
    return out


def models(home: str | os.PathLike[str] | None = None) -> list[dict[str, Any]]:
    path = _home(str(home) if home else None) / "models_cache.json"
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        raw = data.get("models") or {}
    except Exception:
        raw = {}
    out = []
    for ident, row in raw.items():
        info = (row or {}).get("info") or {}
        efforts = []
        for effort in info.get("reasoning_efforts") or []:
            if isinstance(effort, dict) and effort.get("id"):
                efforts.append(str(effort["id"]))
        out.append({
            "id": str(info.get("id") or ident),
            "name": str(info.get("name") or ident),
            "contextWindow": int(info.get("context_window") or 0),
            "efforts": efforts,
            "defaultEffort": str(info.get("reasoning_effort") or ""),
            "hidden": bool(info.get("hidden")),
        })
    return sorted((m for m in out if not m["hidden"]), key=lambda m: m["id"], reverse=True)


def active_sessions(home: str | os.PathLike[str] | None = None) -> list[dict[str, Any]]:
    path = _home(str(home) if home else None) / "active_sessions.json"
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return []
    return [x for x in data if isinstance(x, dict) and x.get("session_id")]


def summary(home: str | os.PathLike[str], session_id: str) -> dict[str, Any]:
    root = Path(home) / "sessions"
    hits = glob.glob(str(root / "**" / str(session_id) / "summary.json"), recursive=True)
    if not hits:
        return {}
    try:
        return json.loads(Path(hits[0]).read_text(encoding="utf-8"))
    except Exception:
        return {}


def session_for_pid(pid: int, home: str | os.PathLike[str]) -> dict[str, Any]:
    for row in active_sessions(home):
        if int(row.get("pid") or 0) == int(pid):
            data = summary(home, str(row["session_id"]))
            return {"sessionId": row["session_id"], "home": str(Path(home)), **_summary_public(data)}
    return {}


def _summary_public(data: dict[str, Any]) -> dict[str, Any]:
    info = data.get("info") or {}
    return {
        "cwd": str(info.get("cwd") or ""),
        "model": str(data.get("current_model_id") or ""),
        "effort": str(data.get("reasoning_effort") or ""),
        "title": str(data.get("generated_title") or data.get("session_summary") or ""),
        "contextWindow": 0,
        "lastActiveAt": str(data.get("last_active_at") or ""),
    }
