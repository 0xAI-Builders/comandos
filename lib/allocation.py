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
