"""One classification and delivery policy for every notice (N2, D5/D6).

Producers (hooks, Pomodoro, news, quotas) record N1 events; this module
decides, per event and from the current client presence, whether it is a
notice, how its float behaves, whether it sounds and on which device, and
whether it should be pushed. Nothing here keeps a parallel silence policy.

Helpers never COMMIT on the caller's behalf except the small durable writes
(`record_presence`, `mark_read`, `save_prefs`, `claim_sound`), each in its own
short transaction.
"""
import json
import sqlite3

import event_store

LOCAL_SPEAKER = "local-speaker"   # the hook's own machine when nobody is visible
PRESENCE_STALE_MS = 90_000        # a visible client must have reported recently
PUSH_AFTER_MS = 120_000           # D5: push when no client was visible for 2 min
ATTENDED_MS = 120_000             # a visible client counts as watched until 2 min after its last interaction
HISTORY_WINDOW = 500              # events scanned for pending requests

CATEGORIES = {
    "permission_requested": "attention",
    "input_requested": "attention",
    "turn_failed": "error",
    "turn_completed": "done",
    "turn_cancelled": "done",
    "focus_completed": "focus",
    "news_edition": "news",
    "announcement": "news",
    "usage_alert": "usage",
    "pane_closed": "info",
    "session_ended": "info",
}
NOT_NOTICES = {"prompt_accepted", "turn_started"}
NEEDS_HUMAN = {"permission_requested", "input_requested"}
PUSH_CATEGORIES = {"attention", "error", "focus", "done"}
MODES = {"visual", "sound"}


def default_prefs():
    # D5: sound only when you are needed; turn completed and news are visual.
    return {"modes": {"attention": "sound", "error": "sound", "focus": "sound",
                      "done": "visual", "news": "visual", "usage": "visual", "info": "visual"},
            "volume": 0.6, "muted": False, "floatMs": 6000, "burstMs": 10000}


def merge_prefs(current, update):
    prefs = json.loads(json.dumps(current or default_prefs()))
    if not isinstance(update, dict):
        raise ValueError("Preferencias inválidas")
    modes = update.get("modes")
    if modes is not None:
        if not isinstance(modes, dict):
            raise ValueError("Modos inválidos")
        for category, mode in modes.items():
            if category not in prefs["modes"] or mode not in MODES:
                raise ValueError("Modo de aviso inválido")
            prefs["modes"][category] = mode
    if "volume" in update:
        volume = update["volume"]
        if isinstance(volume, bool) or not isinstance(volume, (int, float)) or not 0 <= volume <= 1:
            raise ValueError("Volumen inválido")
        prefs["volume"] = float(volume)
    if "muted" in update:
        if not isinstance(update["muted"], bool):
            raise ValueError("Silencio inválido")
        prefs["muted"] = update["muted"]
    return prefs


def classify(event):
    kind = str(event.get("kind") or "")
    category = CATEGORIES.get(kind, "info")
    cue = {"attention": "permission" if kind == "permission_requested" else "attention",
           "error": "error", "focus": "complete", "done": "success", "news": "complete",
           "usage": "warning"}.get(category, "attention")
    return {"category": category, "needsHuman": kind in NEEDS_HUMAN, "cue": cue,
            "notice": kind not in NOT_NOTICES}


def _visible(c, now_ms):
    seen = c.get("lastSeenAt") or 0
    return bool(c.get("visible")) and c.get("connected", True) is not False and now_ms - int(seen) <= PRESENCE_STALE_MS


def sound_device(clients, now_ms):
    """D5: the visible client with the latest explicit interaction; D6: if it
    cannot play audio, the next visible client does. None when someone is
    visible but nobody can play; LOCAL_SPEAKER when nobody is visible."""
    visible = [c for c in clients or [] if _visible(c, now_ms)]
    if not visible:
        return LOCAL_SPEAKER
    visible.sort(key=lambda c: c.get("lastInteractionAt") or 0, reverse=True)
    for c in visible:
        if c.get("canPlayAudio"):
            return c.get("deviceId")
    return None


def _last_visible_ms(clients):
    """Last moment a person could have been looking at some client.

    A visible client only counts while someone interacted with it within
    ATTENDED_MS: a desktop window left open on an empty desk is not a reader."""
    last = 0
    for c in clients or []:
        if c.get("visible") and c.get("connected", True) is not False:
            seen = int(c.get("lastSeenAt") or 0)
        else:
            # Hidden clients count until the moment they were hidden.
            seen = int(c.get("hiddenSince") or c.get("lastSeenAt") or 0)
        attended = int(c.get("lastInteractionAt") or 0) + ATTENDED_MS
        last = max(last, min(seen, attended))
    return last


def _pushable(event, clients, now_ms):
    """Nobody has looked for PUSH_AFTER_MS, and the event arrived after the
    last moment someone could see it: what was already on a screen never
    reaches the phone later."""
    last = _last_visible_ms(clients)
    at = event.get("occurredAtMs") or event.get("receivedAtMs") or now_ms
    return now_ms - last >= PUSH_AFTER_MS and int(at) >= last


def route_event(event, clients, prefs, now_ms, focus_active=False):
    info = classify(event)
    prefs = prefs or default_prefs()
    category = info["category"]
    sound = (info["notice"] and prefs["modes"].get(category) == "sound" and not prefs.get("muted")
             and (not focus_active or event.get("kind") == "permission_requested"))
    return dict(info,
                float={"show": info["notice"] and category != "info", "ms": prefs.get("floatMs", 6000)},
                sound=bool(sound),
                soundDevice=sound_device(clients, now_ms) if sound else None,
                push=category in PUSH_CATEGORIES and _pushable(event, clients, now_ms))


def group_keys(events, prefs):
    """Same project and kind within burstMs of the previous one share a group."""
    burst = (prefs or default_prefs()).get("burstMs", 10000)
    last, groups = {}, {}
    for event in sorted(events, key=lambda e: (e.get("occurredAtMs") or 0, e.get("sequence") or 0)):
        key = (event.get("projectKey"), event.get("kind"))
        at = event.get("occurredAtMs") or 0
        prev = last.get(key)
        groups[event["eventId"]] = prev[1] if prev and at - prev[0] <= burst else event["eventId"]
        last[key] = (at, groups[event["eventId"]])
    return groups


def _pane_of(event):
    return event.get("paneKey") or (event.get("sessionKey"), event.get("paneId")) \
        if (event.get("paneKey") or event.get("paneId")) else event.get("conversationId")


def pending_requests(events):
    """Human requests not followed by any later event of the same pane."""
    pending = {}
    for event in sorted(events, key=lambda e: (e.get("sequence") or 0, e.get("occurredAtMs") or 0)):
        pane = _pane_of(event)
        if pane is None:
            continue
        if event.get("kind") in NEEDS_HUMAN:
            pending[pane] = event["eventId"]
        else:
            pending.pop(pane, None)
    return sorted(pending.values())


def notice(event, clients, prefs, now_ms, read, group, focus_active=False):
    route = route_event(event, clients, prefs, now_ms, focus_active)
    project = event.get("projectKey") if route["category"] != "news" else None
    return {"eventId": event["eventId"], "sequence": event.get("sequence"), "kind": event.get("kind"),
            "category": route["category"], "needsHuman": route["needsHuman"],
            "projectKey": project, "project": project,
            "sessionKey": event.get("sessionKey"), "paneKey": event.get("paneKey"), "paneId": event.get("paneId"),
            "title": event.get("title") or "", "excerpt": event.get("excerpt") or "",
            "occurredAtMs": event.get("occurredAtMs"), "read": event["eventId"] in read,
            "float": route["float"], "group": group,
            "editionId": (event.get("sourceEventId") or "")[len("news-edition:"):]
            if str(event.get("sourceEventId") or "").startswith("news-edition:") else None}


# ---- durable state -------------------------------------------------------------

def _tx(conn, fn):
    conn.execute("BEGIN IMMEDIATE")
    try:
        result = fn()
        conn.execute("COMMIT")
        return result
    except BaseException:
        conn.execute("ROLLBACK")
        raise


def record_presence(conn, device_id, visible, can_play_audio, interaction, now_ms, kind="web"):
    if not isinstance(device_id, str) or not device_id or len(device_id) > 200:
        raise ValueError("deviceId inválido")

    def write():
        row = conn.execute("SELECT visible, hidden_since_ms FROM client_presence WHERE device_id = ?",
                           (device_id,)).fetchone()
        hidden_since = None if visible else (row[1] if row and not row[0] and row[1] else now_ms)
        conn.execute(
            "INSERT INTO client_presence (device_id, kind, visible, can_play_audio, last_seen_at_ms, "
            "last_interaction_at_ms, hidden_since_ms) VALUES (?, ?, ?, ?, ?, ?, ?) "
            "ON CONFLICT(device_id) DO UPDATE SET kind = excluded.kind, visible = excluded.visible, "
            "can_play_audio = excluded.can_play_audio, last_seen_at_ms = excluded.last_seen_at_ms, "
            "last_interaction_at_ms = COALESCE(excluded.last_interaction_at_ms, last_interaction_at_ms), "
            "hidden_since_ms = excluded.hidden_since_ms",
            (device_id, str(kind or "web")[:16], int(bool(visible)), int(bool(can_play_audio)), now_ms,
             now_ms if interaction else None, hidden_since))
    _tx(conn, write)


def clients(conn, now_ms):
    rows = conn.execute("SELECT device_id, kind, visible, can_play_audio, last_seen_at_ms, "
                        "last_interaction_at_ms, hidden_since_ms FROM client_presence").fetchall()
    return [{"deviceId": r[0], "kind": r[1], "visible": bool(r[2]), "canPlayAudio": bool(r[3]),
             "lastSeenAt": r[4], "lastInteractionAt": r[5], "hiddenSince": r[6], "connected": True}
            for r in rows]


def mark_read(conn, event_ids, now_ms):
    ids = [str(e) for e in event_ids or [] if isinstance(e, str) and e][:1000]
    _tx(conn, lambda: conn.executemany("INSERT OR IGNORE INTO notice_reads (event_id, read_at_ms) VALUES (?, ?)",
                                       [(e, now_ms) for e in ids]))
    return ids


def load_prefs(conn):
    row = conn.execute("SELECT value FROM notice_prefs WHERE id = 1").fetchone()
    if not row:
        return default_prefs()
    try:
        return merge_prefs(default_prefs(), json.loads(row[0]))
    except (ValueError, TypeError):
        return default_prefs()


def save_prefs(conn, update):
    prefs = merge_prefs(load_prefs(conn), update)
    _tx(conn, lambda: conn.execute("INSERT OR REPLACE INTO notice_prefs (id, value) VALUES (1, ?)",
                                   (json.dumps(prefs),)))
    return prefs


def _recent(conn):
    latest = event_store.latest_sequence(conn)
    return event_store.list_events(conn, max(0, latest - HISTORY_WINDOW), HISTORY_WINDOW)


def _notice_events(conn):
    """Todos los eventos que son avisos, de la página más vieja a la más nueva."""
    after = 0
    while True:
        page = event_store.list_events(conn, after, event_store.MAX_PAGE)
        if not page:
            return
        for e in page:
            if classify(e)["notice"]:
                yield e
        after = page[-1]["sequence"]


def _read_ids(conn):
    return {r[0] for r in conn.execute("SELECT event_id FROM notice_reads")}


def badge_count(conn):
    """El número de la campana en todas las superficies: avisos sin leer más pedidos
    sin responder, sobre TODO el historial (antes solo los 500 eventos más viejos, y
    «Marcar leídos» tocaba los 500 más nuevos: el número nunca bajaba)."""
    read, pending = _read_ids(conn), set(pending_requests(_recent(conn)))
    return sum(1 for e in _notice_events(conn) if e["eventId"] not in read or e["eventId"] in pending)


def unread_notice_ids(conn, project=None):
    """Ids de todos los avisos sin leer (opcionalmente de un proyecto), para «Marcar leídos»."""
    read = _read_ids(conn)
    return [e["eventId"] for e in _notice_events(conn) if e["eventId"] not in read
            and (not project or (classify(e)["category"] != "news" and e.get("projectKey") == project))]


def list_notices(conn, after, limit, now_ms, device_id=None, focus_active=False):
    prefs = load_prefs(conn)
    present = clients(conn, now_ms)
    history = _recent(conn)
    groups = group_keys(history, prefs)
    page = event_store.list_events(conn, after, limit)
    read = {r[0] for r in conn.execute("SELECT event_id FROM notice_reads")}
    notices = [notice(e, present, prefs, now_ms, read, groups.get(e["eventId"], e["eventId"]), focus_active)
               for e in page if classify(e)["notice"]]
    return {"notices": notices, "nextAfter": page[-1]["sequence"] if page else after,
            "pending": pending_requests(history), "prefs": prefs, "focusActive": focus_active}


def focus_block_active(conn):
    """A running Pomodoro focus block, read from the shared state (D5)."""
    try:
        row = conn.execute("SELECT b.mode, b.status FROM pomodoro_state s JOIN pomodoro_blocks b "
                           "ON b.block_id = s.block_id WHERE s.id = 1").fetchone()
    except sqlite3.Error:
        return False
    return bool(row) and row[0] == "focus" and row[1] == "running"


def claim_sound(conn, event_id, device_id, now_ms, focus_active=None):
    """Play only on the chosen device, and only once per event across all of them."""
    event = event_store.get_event(conn, event_id)
    if event is None:
        return {"play": False, "reason": "Evento desconocido"}
    if focus_active is None:
        focus_active = focus_block_active(conn)
    route = route_event(event, clients(conn, now_ms), load_prefs(conn), now_ms, focus_active)
    if not route["sound"]:
        return {"play": False, "reason": "Solo visual"}
    if route["soundDevice"] != device_id:
        return {"play": False, "reason": "Suena en otro dispositivo"}
    try:
        claimed = _tx(conn, lambda: event_store.claim_delivery(conn, event_id, "sound", ""))
    except sqlite3.OperationalError:
        return {"play": False, "reason": "Ocupado"}
    if not claimed:
        return {"play": False, "reason": "Ya sonó"}
    return {"play": True, "cue": route["cue"]}


def push_policy(prefs, focus_active=False, read=frozenset()):
    """Adapter for web_push.send_due_push: push unread notices per route_event."""
    def policy(event, present, now_ms):
        return event.get("eventId") not in read and \
            route_event(event, present, prefs, now_ms, focus_active)["push"]
    return policy


def main(argv):
    """`notification_delivery.py claim-local <eventId> <deviceId>`: exit 0 to play here.

    Used by the hook on this machine: it plays its chime/voice only when this
    device (or the local speaker fallback) wins the single sound claim.
    """
    import time
    import app_state
    if len(argv) != 4 or argv[1] != "claim-local":
        return 2
    conn = app_state.connect()
    app_state.migrate(conn)
    now = int(time.time() * 1000)
    for device in (argv[3], LOCAL_SPEAKER):
        if claim_sound(conn, argv[2], device, now)["play"]:
            print(json.dumps({"play": True, "device": device}))
            return 0
    return 1


if __name__ == "__main__":
    import sys
    raise SystemExit(main(sys.argv))
