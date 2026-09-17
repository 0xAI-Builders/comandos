"""Reparto de cuota: motor puro (sin I/O).

Cuotas → ritmo y proyección; sesiones + cuotas → propuesta determinista;
propuesta + un cambio tentativo → impacto en vivo.
"""
from __future__ import annotations

import copy
import hashlib
import json
import re

WINDOW_SECONDS = {"5h": 5 * 3600, "7d": 7 * 24 * 3600}
LAYER_EFFORT = {3: "high", 2: "medium", 1: "low"}
LAYER_TIER = {3: "high", 2: "mid", 1: "low"}
WEIGHT = {"low": 0.5, "medium": 1.0, "high": 1.6, "xhigh": 2.2, "max": 3.0}
_HIGH_EFFORTS = {"high", "xhigh", "max"}


def fmt_duration(seconds: float) -> str:
    seconds = max(0.0, float(seconds))
    if seconds < 3600:
        return f"{int(round(seconds / 60))} min"
    if seconds < 24 * 3600:
        return f"{int(round(seconds / 3600))} h"
    days, rest = divmod(int(round(seconds / 3600)), 24)
    return f"{days}d {rest}h"


def enrich_limits(limits: list[dict], now: float) -> list[dict]:
    out = []
    for raw in limits:
        row = dict(raw)
        win = WINDOW_SECONDS.get(str(row.get("window") or ""))
        resets_at = float(row.get("resets_at") or 0)
        pct = row.get("percent")
        row["windowSeconds"] = win
        if not win or resets_at <= now or pct is None:
            row.update(pace=None, burn=None, runsOutIn=None, reachesReset=None, verdict="")
            out.append(row)
            continue
        remaining = resets_at - now
        elapsed = max(1.0, win - remaining)
        pace = min(100.0, elapsed / win * 100.0)
        pct = float(pct)
        burn = pct / max(1.0, pace)
        rate = pct / elapsed  # % por segundo
        runs_out = (100.0 - pct) / rate if rate > 0 else None
        reaches = runs_out is None or runs_out >= remaining
        row.update(pace=pace, burn=burn,
                   runsOutIn=None if reaches else runs_out,
                   reachesReset=reaches,
                   verdict="llega al reset" if reaches else f"se acaba en {fmt_duration(runs_out)}")
        out.append(row)
    return out


def layer_of(session: dict) -> int:
    effort = str(session.get("effort") or "").lower()
    base = 3 if effort in _HIGH_EFFORTS else 1 if effort == "low" else 2
    if str(session.get("status") or "") in ("idle", "done"):
        base = max(1, base - 1)
    return base


def _tier_of(model_id: str, tiers: dict) -> str:
    for pat in tiers.get("patterns") or []:
        if re.search(pat.get("match", ""), model_id, re.I):
            return pat.get("tier", "unknown")
    return "unknown"


def model_for_layer(motor: str, layer: int, registry: dict, tiers: dict) -> str:
    models = [m["id"] for m in ((registry.get("motors") or {}).get(motor) or {}).get("models") or []]
    if not models:
        return ""
    for tier in (LAYER_TIER[layer], "mid"):
        for mid in models:
            if _tier_of(mid, tiers) == tier:
                return mid
    return models[0]


def pool_key(motor: str, account: str) -> str:
    return f"{motor}:{account or 'main'}"


def governing_limit(limits: list[dict], motor: str, account: str) -> dict | None:
    rows = [r for r in limits if r.get("provider") == motor and (r.get("account") or "main") == (account or "main")]
    for r in rows:
        if r.get("window") == "7d" and r.get("kind") in ("weekly_all", "window", None) and r.get("kind") != "weekly_scoped":
            return r
    for r in rows:
        if r.get("window") == "7d":
            return r
    return rows[0] if rows else None
