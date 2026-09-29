"""Persisted CommandOS events (N1): stored before any channel delivers them.

Every event gets a local ``eventId`` (one reception) and a monotonic
``sequence`` for pagination. A local UUID never proves the provider's turn
identity: deduplication only uses ``sourceEventId`` or a confirmed cycle key
(harness + conversation + turn/request + kind). Ambiguous receptions stay
separate events.

``append_event`` never opens, commits or rolls back a transaction: the caller
owns BEGIN/COMMIT so it can group the event with its own writes (Pomodoro
records ``focus_completed`` inside its cycle transaction).
"""
import sqlite3
import time
import uuid

KINDS = frozenset({
    "prompt_accepted", "turn_started", "input_requested", "permission_requested",
    "turn_completed", "turn_cancelled", "turn_failed", "pane_closed",
    "session_ended", "focus_completed", "news_edition",
    # N2: producers that are not agent turns (never a waiting request).
    "announcement", "usage_alert",
})
EVIDENCE = frozenset({"confirmed", "inferred", "historical", "unknown"})
CORRELATION = frozenset({"source", "local", "unknown"})
MAX_TITLE = 200
MAX_EXCERPT = 500
MAX_ID = 200
MAX_PAGE = 500

_COLUMNS = (
    ("sequence", "sequence"), ("eventId", "event_id"), ("sourceEventId", "source_event_id"),
    ("source", "source"), ("harness", "harness"), ("projectKey", "project_key"),
    ("sessionKey", "session_key"), ("paneKey", "pane_key"), ("paneId", "pane_id"),
    ("processKey", "process_key"), ("conversationId", "conversation_id"),
    ("turnId", "turn_id"), ("requestId", "request_id"), ("kind", "kind"),
    ("evidence", "evidence"), ("correlation", "correlation"),
    ("occurredAtMs", "occurred_at_ms"), ("receivedAtMs", "received_at_ms"),
    ("title", "title"), ("excerpt", "excerpt"),
)
_SELECT = "SELECT " + ", ".join(col for _, col in _COLUMNS) + " FROM events"
_IDENTS = ("sourceEventId", "harness", "projectKey", "sessionKey", "paneKey", "paneId",
           "processKey", "conversationId", "turnId", "requestId")


def now_ms():
    return int(time.time() * 1000)


def _opt_ident(event, name):
    value = event.get(name)
    if value is None or value == "":
        return None
    if not isinstance(value, str) or len(value) > MAX_ID or any(ord(c) < 32 for c in value):
        raise ValueError(f"{name} inválido")
    return value


def _ms(value, name):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or value < 0:
        raise ValueError(f"{name} inválido")
    return int(value)


def _text(value, limit):
    return value[:limit] if isinstance(value, str) else ""


def dedupe_key(event):
    """Identity used for idempotency, or None when the event is ambiguous."""
    if event.get("sourceEventId"):
        return f"src:{event['sourceEventId']}"
    step = event.get("turnId") or event.get("requestId")
    if event.get("correlation") == "source" and event.get("evidence") == "confirmed" and step:
        return "cycle:" + "|".join((event.get("harness") or "", event.get("conversationId") or "",
                                    event.get("turnId") or "", event.get("requestId") or "",
                                    event["kind"]))
    return None


def destination(event):
    if event.get("paneKey"):
        return "pane"
    if event.get("sessionKey"):
        return "session"
    return "none"


def _row(row):
    event = {name: row[i] for i, (name, _) in enumerate(_COLUMNS)}
    event["destination"] = destination(event)
    return event


def normalize(event):
    """Validate and complete an event dict (camelCase keys)."""
    if not isinstance(event, dict):
        raise ValueError("Evento inválido")
    kind = event.get("kind")
    if kind not in KINDS:
        raise ValueError("kind desconocido")
    source = event.get("source")
    if not isinstance(source, str) or not source or len(source) > MAX_ID:
        raise ValueError("source inválido")
    evidence = event.get("evidence") or "unknown"
    if evidence not in EVIDENCE:
        raise ValueError("evidence inválida")
    correlation = event.get("correlation") or "unknown"
    if correlation not in CORRELATION:
        raise ValueError("correlation inválida")
    out = {name: _opt_ident(event, name) for name in _IDENTS}
    received = _ms(event.get("receivedAtMs", now_ms()), "receivedAtMs")
    occurred = event.get("occurredAtMs")
    out.update(kind=kind, source=source, evidence=evidence, correlation=correlation,
               receivedAtMs=received,
               occurredAtMs=received if occurred is None else _ms(occurred, "occurredAtMs"),
               title=_text(event.get("title"), MAX_TITLE),
               excerpt=_text(event.get("excerpt"), MAX_EXCERPT),
               eventId=_opt_ident(event, "eventId") or "event-" + uuid.uuid4().hex)
    return out


def get_event(connection, event_id):
    row = connection.execute(_SELECT + " WHERE event_id = ?", (event_id,)).fetchone()
    return _row(row) if row else None


def append_event(connection, event):
    """Persist ``event`` (or find its twin) and record this reception.

    Returns the stored event with ``eventId``, ``sequence``, ``destination``
    and ``duplicate`` (True when an earlier event had the same identity).
    """
    ev = normalize(event)
    key = dedupe_key(ev)
    existing = None
    if key is not None:
        row = connection.execute(_SELECT + " WHERE dedupe_key = ?", (key,)).fetchone()
        existing = _row(row) if row else None
    if existing is None:
        cols = [col for name, col in _COLUMNS if name != "sequence"]
        values = [ev[name] for name, _ in _COLUMNS if name != "sequence"]
        connection.execute(
            f"INSERT INTO events ({', '.join(cols)}, dedupe_key) VALUES ({', '.join('?' * len(cols))}, ?)",
            (*values, key))
        stored = get_event(connection, ev["eventId"])
    else:
        stored = existing
    connection.execute("INSERT INTO event_receipts VALUES (?, ?, ?, ?, ?)",
                       ("receipt-" + uuid.uuid4().hex, stored["eventId"], ev["source"],
                        ev["receivedAtMs"], int(existing is not None)))
    stored["duplicate"] = existing is not None
    return stored


def list_events(connection, after_sequence=0, limit=100):
    """Events with ``sequence > after_sequence`` in ascending order."""
    try:
        after = max(0, int(after_sequence))
        size = max(1, min(int(limit), MAX_PAGE))
    except (TypeError, ValueError):
        raise ValueError("Paginación inválida")
    rows = connection.execute(_SELECT + " WHERE sequence > ? ORDER BY sequence LIMIT ?",
                              (after, size)).fetchall()
    return [_row(r) for r in rows]


def latest_sequence(connection):
    return connection.execute("SELECT COALESCE(MAX(sequence), 0) FROM events").fetchone()[0]


def claim_delivery(connection, event_id, channel, device_id=""):
    """Reserve ``(event, channel, device)`` once; False if already claimed."""
    if not isinstance(channel, str) or not channel or len(channel) > 64:
        raise ValueError("channel inválido")
    if not isinstance(device_id, str) or len(device_id) > MAX_ID:
        raise ValueError("deviceId inválido")
    if get_event(connection, event_id) is None:
        raise sqlite3.IntegrityError("evento inexistente")
    stamp = now_ms()
    cur = connection.execute(
        "INSERT OR IGNORE INTO deliveries (event_id, channel, device_id, state, created_at_ms, updated_at_ms) "
        "VALUES (?, ?, ?, 'claimed', ?, ?)", (event_id, channel, device_id, stamp, stamp))
    return cur.rowcount == 1
