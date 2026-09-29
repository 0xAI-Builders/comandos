"""Personal focus progression: a pure, versioned rule set plus a reward ledger.

``progress(blocks, policy, now_ms)`` is deterministic and independent of any
Analytics filter. Rewards are written once per ``(blockId, policyVersion)``
inside the Pomodoro settlement transaction, so a retried completion never
duplicates points. A policy only rewards blocks that end after its activation:
there are no retroactive points on earlier history (decision D2).
"""
from datetime import datetime, timedelta
import json
from zoneinfo import ZoneInfo

MINUTE_MS = 60_000

# D2 ("La del mockup"): 10 XP per active focus minute, a level every 1,000 XP,
# 100 minutes daily goal. Cancelled blocks count only their active time;
# breaks never count. Streaks use Mexico City days.
POLICY_V1 = {
    "policyVersion": "v1",
    "xpPerMinute": 10,
    "xpPerLevel": 1000,
    "dailyGoalMinutes": 100,
    "countCancelledActive": True,
    "timezone": "America/Mexico_City",
    "achievements": [
        {"id": "first", "kind": "completedBlocks", "threshold": 1, "asset": "first", "title": "Primer bloque"},
        {"id": "hundred", "kind": "focusMinutes", "threshold": 100, "asset": "hundred", "title": "100 minutos"},
        {"id": "streak3", "kind": "maxStreakDays", "threshold": 3, "asset": "streak", "title": "3 días seguidos"},
    ],
}


def _eligible(block, policy, activated_at_ms=None):
    if block.get("mode") != "focus" or block.get("provenance", "measured") != "measured":
        return False
    if block.get("activeMs") is None or block.get("endedAtMs") is None:
        return False
    if activated_at_ms is not None and block["endedAtMs"] < activated_at_ms:
        return False
    if block.get("status") == "completed":
        return True
    return block.get("status") == "cancelled" and bool(policy.get("countCancelledActive"))


def reward_for(block, policy):
    minutes = max(0, int(block["activeMs"]) // MINUTE_MS)
    return {"minutes": minutes, "xp": minutes * int(policy["xpPerMinute"])}


def level_for(xp, policy):
    return 1 + int(xp) // int(policy["xpPerLevel"])


def _day(ms, zone):
    return datetime.fromtimestamp(ms / 1000, zone).date()


def _streaks(days, today):
    current = 0
    cursor = today if today in days else today - timedelta(days=1)
    while cursor in days:
        current += 1
        cursor -= timedelta(days=1)
    best = run = 0
    previous = None
    for day in sorted(days):
        run = run + 1 if previous is not None and day == previous + timedelta(days=1) else 1
        best = max(best, run)
        previous = day
    return current, best


def progress(blocks, policy, now_ms, activated_at_ms=None):
    zone = ZoneInfo(policy.get("timezone") or "America/Mexico_City")
    seen = {}
    for block in blocks or ():
        if _eligible(block, policy, activated_at_ms) and block["blockId"] not in seen:
            seen[block["blockId"]] = block
    rewarded = list(seen.values())
    xp = minutes = today_minutes = 0
    today = _day(now_ms, zone)
    completed_days = set()
    completed = 0
    for block in rewarded:
        r = reward_for(block, policy)
        xp += r["xp"]
        minutes += r["minutes"]
        start_day = _day(block.get("startedAtMs") or block["endedAtMs"], zone)
        if start_day == today:
            today_minutes += r["minutes"]
        if block["status"] == "completed":
            completed += 1
            completed_days.add(start_day)
    streak, best = _streaks(completed_days, today)
    per_level = int(policy["xpPerLevel"])
    values = {"completedBlocks": completed, "focusMinutes": minutes, "maxStreakDays": best, "streakDays": streak}
    return {
        "policyVersion": policy["policyVersion"],
        "xpPerMinute": int(policy["xpPerMinute"]),
        "xpPerLevel": per_level,
        "xp": xp,
        "level": level_for(xp, policy),
        "levelPct": round(100.0 * (xp % per_level) / per_level, 2),
        "xpToNextLevel": per_level - xp % per_level,
        "focusMinutes": minutes,
        "completedBlocks": completed,
        "todayMinutes": today_minutes,
        "dailyGoalMinutes": int(policy["dailyGoalMinutes"]),
        "streakDays": streak,
        "maxStreakDays": best,
        "achievements": [dict(a, unlocked=values.get(a["kind"], 0) >= a["threshold"])
                         for a in policy.get("achievements", ())],
    }


# ---- ledger (connection from app_state; the caller owns the transaction) ----

def ensure_policy(conn, policy, now_ms):
    """Record the policy's activation once; returns its activation time."""
    conn.execute("INSERT INTO focus_policies (policy_version, document, activated_at_ms) VALUES (?, ?, ?) "
                 "ON CONFLICT(policy_version) DO NOTHING",
                 (policy["policyVersion"], json.dumps(policy, sort_keys=True), int(now_ms)))
    return conn.execute("SELECT activated_at_ms FROM focus_policies WHERE policy_version = ?",
                        (policy["policyVersion"],)).fetchone()[0]


def _activation(conn, policy):
    row = conn.execute("SELECT activated_at_ms FROM focus_policies WHERE policy_version = ?",
                       (policy["policyVersion"],)).fetchone()
    return row[0] if row else None


def award(conn, record, policy, now_ms):
    """Write the reward for one terminal record; None if ineligible or already paid."""
    activated = _activation(conn, policy)
    if activated is None or not _eligible(record, policy, activated):
        return None
    reward = reward_for(record, policy)
    version = policy["policyVersion"]
    before = conn.execute("SELECT COALESCE(SUM(xp), 0) FROM focus_rewards WHERE policy_version = ?",
                          (version,)).fetchone()[0]
    level_before, level_after = level_for(before, policy), level_for(before + reward["xp"], policy)
    reached = level_after if level_after > level_before else None
    cur = conn.execute(
        "INSERT INTO focus_rewards (block_id, policy_version, xp, minutes, status, started_at_ms, ended_at_ms, "
        "level_reached, awarded_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) "
        "ON CONFLICT(block_id, policy_version) DO NOTHING",
        (record["blockId"], version, reward["xp"], reward["minutes"], record["status"],
         int(record.get("startedAtMs") or record["endedAtMs"]), int(record["endedAtMs"]), reached, int(now_ms)))
    if cur.rowcount != 1:
        return None
    return {"blockId": record["blockId"], "policyVersion": version, "xp": reward["xp"],
            "minutes": reward["minutes"], "levelReached": reached}


def ledger_progress(conn, policy, now_ms):
    """Progress from the paid ledger of this policy, plus the latest level-up."""
    rows = conn.execute("SELECT block_id, minutes, status, started_at_ms, ended_at_ms FROM focus_rewards "
                        "WHERE policy_version = ?", (policy["policyVersion"],)).fetchall()
    blocks = [{"blockId": b, "mode": "focus", "status": st, "activeMs": m * MINUTE_MS, "startedAtMs": s,
               "endedAtMs": e, "provenance": "measured"} for b, m, st, s, e in rows]
    out = progress(blocks, policy, now_ms)
    last = conn.execute("SELECT block_id, level_reached, awarded_at_ms FROM focus_rewards "
                        "WHERE policy_version = ? AND level_reached IS NOT NULL "
                        "ORDER BY awarded_at_ms DESC, level_reached DESC LIMIT 1",
                        (policy["policyVersion"],)).fetchone()
    out["lastLevelUp"] = {"blockId": last[0], "level": last[1], "awardedAtMs": last[2]} if last else None
    out["activatedAtMs"] = _activation(conn, policy)
    return out
