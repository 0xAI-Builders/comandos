"""Datos de la vista Analytics (Cuentas · Comparar · Pomodoro) para una ventana de 8 días.

Puro: recibe filas ya leídas y devuelve el modelo que pinta dash/analytics-render.js,
con la misma forma que tests/fixtures/analytics/week-*.json. Todo es POR CUENTA
(proveedor + alias); nunca se suman cuentas ni porcentajes de cuota entre cuentas.
"""
import os
import re
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo

TZ = "America/Mexico_City"
WINDOW_DAYS = 8
GAP_S = 15 * 60
WASTE_CYCLES = 4
PROVIDERS = ("claude", "codex", "grok", "agy")
CLI = {"claude": "Claude", "codex": "Codex", "grok": "Grok", "agy": "Antigravity"}
COLORS = {("claude", "main"): "#8B7CFF", ("claude", "relotto"): "#FF9A5C",
          ("codex", "main"): "#4CC2FF", ("grok", "main"): "#C5E35A", ("agy", "main"): "#5BD6A0"}
EXTRA_COLORS = ("#2fd3c0", "#FF6B5B", "#FFAE1A", "#FF9AD5", "#9AA6BF")
WD = ("lun", "mar", "mié", "jue", "vie", "sáb", "dom")
MON = ("ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic")


def account_of(account):
    """Alias limpio; las filas viejas sin cuenta cuentan como la principal."""
    alias = (account or "").strip()
    return "main" if alias in ("", "unknown") else alias


def account_id(provider, account):
    return f"{provider}:{account_of(account)}"


def project_of(path):
    path = (path or "").rstrip("/")
    return os.path.basename(path) if path else "Sin carpeta"


def pomodoro_project(name):
    """'ComandOS ⎇ ⫽30' -> 'ComandOS': quita rama y pane que añade el Pomodoro."""
    return re.split(r"\s+[⎇⫽]", name or "")[0].strip() or "Sin proyecto"


def fmt_left(seconds):
    s = max(0, int(seconds))
    d, h, m = s // 86400, s % 86400 // 3600, s % 3600 // 60
    if d:
        return f"{d}d {h}h"
    if h:
        return f"{h}h {m}m"
    return f"{m} min"


def fmt_reset(ts, tz):
    t = datetime.fromtimestamp(ts, tz)
    return f"{WD[t.weekday()]} {t.day} {MON[t.month - 1]}, {t:%H:%M}"


def window_dates(now, offset, tz):
    """Los 8 días de la ventana, del más nuevo al más viejo. offset -1 = los 8 anteriores."""
    end = datetime.fromtimestamp(now, tz).date() + timedelta(days=WINDOW_DAYS * offset)
    return [end - timedelta(days=i) for i in range(WINDOW_DAYS)]


def day_rows(dates):
    out = []
    for i, d in enumerate(dates):
        label = f"{d.day} {MON[d.month - 1]}" if i == 0 or d.day == 1 else str(d.day)
        out.append([d.isoformat(), WD[d.weekday()], label])
    return out


def week_label(dates):
    a, b = dates[-1], dates[0]
    if a.month == b.month:
        return f"{a.day} – {b.day} {MON[b.month - 1]}"
    return f"{a.day} {MON[a.month - 1]} – {b.day} {MON[b.month - 1]}"


def _intervals(turns, spans):
    """(cuenta, proyecto) -> [(inicio, fin, tokens)] de turnos y tramos medidos."""
    out = {}
    for t in turns:
        if t["provider"] not in PROVIDERS:
            continue
        key = (account_id(t["provider"], t.get("account")), project_of(t.get("git_root") or t.get("pane_pwd")))
        end = float(t["finished"])
        start = min(end, float(t.get("started") or end))
        out.setdefault(key, []).append((start, end, int(t.get("tokens") or 0)))
    for s in spans:
        if s["provider"] not in PROVIDERS:
            continue
        key = (account_id(s["provider"], s.get("account")), project_of(s.get("git_root")))
        end = float(s["finished"])
        out.setdefault(key, []).append((min(end, float(s["started"])), end, 0))
    return out


def sessions(turns, spans, tz):
    """Sesiones del calendario: tramos de la misma cuenta y proyecto separados por ≤15 min,
    partidos a medianoche local. Horas locales en st/en; tokens en millones."""
    out = []
    for (acc, proj), items in _intervals(turns, spans).items():
        items.sort()
        merged = []
        for start, end, tok in items:
            if merged and start - merged[-1][1] <= GAP_S:
                merged[-1][1] = max(merged[-1][1], end)
                merged[-1][2].append((end, tok))
            else:
                merged.append([start, end, [(end, tok)]])
        for start, end, toks in merged:
            cur = datetime.fromtimestamp(start, tz)
            stop = datetime.fromtimestamp(end, tz)
            while True:
                midnight = datetime.combine(cur.date() + timedelta(days=1), datetime.min.time(), tz)
                piece_end = min(stop, midnight)
                a, b = cur.timestamp(), piece_end.timestamp()
                tok = sum(t for at, t in toks if a <= at <= b and (at < b or piece_end == stop))
                st = cur.hour + cur.minute / 60 + cur.second / 3600
                en = st + (b - a) / 3600
                out.append({"d": cur.date().isoformat(), "acc": acc, "proj": proj,
                            "st": st, "en": en, "tok": tok / 1e6})
                if piece_end >= stop:
                    break
                cur = piece_end
    out.sort(key=lambda s: (s["d"], s["st"], s["acc"], s["proj"]))
    return out


def _limit_slots(limits):
    """Por cuenta: la fila semanal, la de modelo con más uso y la de 5 h."""
    slots = {}
    for row in limits:
        if row.get("provider") not in PROVIDERS or row.get("percent") is None:
            continue
        acc = account_id(row["provider"], row.get("account"))
        slot = slots.setdefault(acc, {"provider": row["provider"], "alias": account_of(row.get("account"))})
        if row.get("window") == "5h":
            slot["h5"] = row
        elif row.get("window") == "7d" and row.get("scope"):
            if "model" not in slot or row["percent"] > slot["model"]["percent"]:
                slot["model"] = row
        elif row.get("window") == "7d":
            slot["week"] = row
    return slots


def _cycles(snapshots, acc):
    rows = [s for s in snapshots if account_id(s["provider"], s["account"]) == acc
            and s["window"] == "7d" and not s.get("scope")]
    return sorted(rows, key=lambda s: -s["resets_at"])


def waste(snapshots, accounts, now):
    """Cuota que quedó sin usar en las últimas semanas cerradas, de la más nueva a la más vieja."""
    out = []
    for a in accounts:
        closed = [s for s in _cycles(snapshots, a["id"]) if s["resets_at"] <= now][:WASTE_CYCLES]
        out.append({"id": a["id"], "cyc": [round(100 - s["percent"]) for s in closed]})
    return out


def _reset(row, tz):
    """Fecha del reset; None si la fila no trae uno real (Grok con cuota declarada trae 0)."""
    return fmt_reset(row["resets_at"], tz) if row and (row.get("resets_at") or 0) > 0 else None


def _left(row, now):
    return fmt_left(row["resets_at"] - now) if row and (row.get("resets_at") or 0) > 0 else None


def _used_at(snapshots, acc, window_end, now):
    """Uso final del ciclo semanal vigente al cerrar la ventana; None si aún no se sabe.
    Solo cuenta un ciclo que reinicia a lo más una semana (más el redondeo a la hora) después
    del cierre: si falta su foto, la de un ciclo posterior no es la de esta semana."""
    for s in reversed(_cycles(snapshots, acc)):
        if s["resets_at"] > window_end:
            if s["resets_at"] - window_end > 7 * 86400 + 3600:
                return None
            return round(s["percent"]) if s["resets_at"] <= now else None
    return None


def build_accounts(limits, sess, today, window_end, snapshots, now, tz, past):
    slots = _limit_slots(limits)
    order = sorted(set(slots) | {s["acc"] for s in sess},
                   key=lambda a: (PROVIDERS.index(a.split(":")[0]), a.split(":")[1] != "main", a))
    out, extra = [], 0
    for acc in order:
        provider, alias = acc.split(":", 1)
        slot = slots.get(acc, {})
        color = COLORS.get((provider, alias))
        if color is None:
            color, extra = EXTRA_COLORS[extra % len(EXTRA_COLORS)], extra + 1
        week, model, h5 = slot.get("week"), slot.get("model"), slot.get("h5")
        mine = [s for s in sess if s["acc"] == acc]
        today_s = [s for s in mine if s["d"] == today]
        item = {
            "id": acc, "provider": provider, "cli": CLI[provider], "alias": alias, "color": color,
            "week": round(week["percent"]) if week else None,
            "model": ({"n": model["scope"], "v": round(model["percent"]),
                       "reset": _reset(model, tz), "left": _left(model, now)}
                      if model else None),
            "h5": round(h5["percent"]) if h5 else None,
            "reset": _reset(week, tz),
            "left": _left(week, now),
            "h5Reset": _reset(h5, tz),
            "h5Left": _left(h5, now),
            "hoy": {"h": sum(s["en"] - s["st"] for s in today_s), "tok": round(sum(s["tok"] for s in today_s)),
                    "ses": len(today_s)},
            "sem": {"h": sum(s["en"] - s["st"] for s in mine), "tok": round(sum(s["tok"] for s in mine) / 1000, 1),
                    "ses": len(mine)},
        }
        item["weekUsed"] = _used_at(snapshots, acc, window_end, now) if past else item["week"]
        out.append(item)
    return out


def sidebar_accounts(accounts, limits, turns, now):
    """Cuotas y consumo medido de la columna; mantiene el modelo de Analytics intacto."""
    out = [dict(a) for a in accounts]
    by_id = {a["id"]: a for a in out}
    groups = {}
    for row in limits:
        item = by_id.get(account_id(row.get("provider"), row.get("account")))
        plan = row.get("plan_type") or row.get("plan")
        if item is not None and plan:
            item["plan"] = plan
    for turn in turns:
        if not now - 7 * 86400 <= float(turn.get("finished") or 0) <= now:
            continue
        provider = "opencode" if turn.get("agent") == "opencode" else turn.get("provider")
        acc = account_id(provider, turn.get("account"))
        if acc not in by_id:
            if provider != "opencode":
                continue
            item = {"id": acc, "provider": provider, "cli": "OpenCode",
                    "alias": account_of(turn.get("account")), "color": "#2fd3c0",
                    "week": None, "model": None, "h5": None}
            by_id[acc] = item
            out.append(item)
        group = groups.setdefault(acc, {"sessions": set(), "tokens": 0, "cost": 0.0, "models": set()})
        if turn.get("session"):
            group["sessions"].add(turn["session"])
        group["tokens"] += max(0, int(turn.get("tokens") or 0))
        group["cost"] += max(0.0, float(turn.get("cost") or 0))
        if turn.get("model"):
            group["models"].add(turn["model"])
    for acc, group in groups.items():
        by_id[acc]["measured"] = {"sessions": len(group["sessions"]), "tokens": group["tokens"],
                                  "costUsd": round(group["cost"], 6), "models": sorted(group["models"])}
    return out


def pomodoros(records, days, tz):
    keep = set(days)
    out = []
    for r in records:
        if r.get("mode") != "focus" or r.get("status") not in ("completed", "cancelled", "skipped"):
            continue
        start = datetime.fromtimestamp(r["startedAtMs"] / 1000, tz)
        if start.date().isoformat() not in keep:
            continue
        st = start.hour + start.minute / 60 + start.second / 3600
        ended = r.get("endedAtMs") or r["startedAtMs"] + (r.get("activeMs") or 0)
        # Un bloque que cruza medianoche se dibuja hasta las 24:00 de su día.
        out.append({"d": start.date().isoformat(), "st": st, "en": min(24, st + (ended - r["startedAtMs"]) / 3_600_000),
                    "plan": round((r.get("targetMs") or r.get("plannedMs") or 0) / 60000),
                    "act": round((r.get("activeMs") or 0) / 60000), "pause": 0,
                    "status": "completed" if r["status"] == "completed" else "cancelled",
                    "proj": pomodoro_project(r.get("project"))})
    out.sort(key=lambda f: (f["d"], f["st"]))
    return out


def build_week(*, now, offset, limits, turns, spans, snapshots, records, tz_name=TZ):
    tz = ZoneInfo(tz_name)
    dates = window_dates(now, offset, tz)
    prev = window_dates(now, offset - 1, tz)
    days = day_rows(dates)
    iso = {d[0] for d in days}
    all_sess = sessions(turns, spans, tz)
    sess = [s for s in all_sess if s["d"] in iso]
    prev_iso = {d.isoformat() for d in prev}
    last = {}
    for s in all_sess:
        if s["d"] in prev_iso:
            last[s["proj"]] = last.get(s["proj"], 0) + s["en"] - s["st"]
    window_end = datetime.combine(dates[0] + timedelta(days=1), datetime.min.time(), tz).timestamp()
    today = dates[0].isoformat() if offset == 0 else None
    accounts = build_accounts(limits, sess, today, window_end, snapshots, now, tz, offset < 0)
    local_now = datetime.fromtimestamp(now, tz)
    return {
        "week": {"offset": offset, "label": week_label(dates), "start": dates[-1].isoformat(), "end": dates[0].isoformat(),
                 "today": today, "now": (local_now.hour + local_now.minute / 60) if offset == 0 else None,
                 "measuredAt": f"{local_now:%H:%M}"},
        "days": days,
        "accounts": accounts,
        "sessions": sess,
        "lastWeek": {p: h for p, h in sorted(last.items())},
        "waste": waste(snapshots, accounts, now),
        "pomodoros": pomodoros(records, iso, tz),
    }
