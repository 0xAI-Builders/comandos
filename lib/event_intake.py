#!/usr/bin/env python3
"""Hook intake for N1 events: normalize what harness hooks really supply.

Hooks call ``python3 lib/event_intake.py record`` with one JSON object on
stdin (built by hooks/cc-notify.sh). The intake keeps session, pane,
conversation, turn and request identities that the old timeline dropped,
resolves the logical ``paneKey`` from the W1 workspace bindings by process
identity, and writes the event in its own short transaction. It works while
cc-dash is down and never raises into the hook.

Identity supplied per harness (see tests/fixtures/hook_payloads.json):
  claude  session_id (conversation), notification_type; no turn id -> local
  codex   session_id (conversation), turn_id, call/approval id on permission
  grok    sessionId (conversation), promptId (turn)
"""
import hashlib
import json
import os
import re
import sqlite3
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import app_state  # noqa: E402
import event_store  # noqa: E402
import work_marks  # noqa: E402
from tmux_snapshot import process_start_time  # noqa: E402

HOOK_KINDS = {
    "UserPromptSubmit": "prompt_accepted",
    "Stop": "turn_completed",
    "Notification": "input_requested",
    "PermissionRequest": "permission_requested",
    "GrokError": "turn_failed",
    "StopFailure": "turn_failed",
    "GrokCancelled": "turn_cancelled",
    "StopCancelled": "turn_cancelled",
    "SessionEnd": "session_ended",
}
PERMISSION_NOTIFICATIONS = frozenset({"permission_prompt"})
LEGACY_KINDS = {"done": "turn_completed", "waiting": "input_requested", "working": "prompt_accepted",
                "error": "turn_failed", "cancelled": "turn_cancelled"}
LEGACY_MARKER = "events.legacy_import"
_SESSION = re.compile(r"[A-Za-z0-9._-]{1,80}")
_PANE = re.compile(r"%[0-9]{1,9}")
_PID = re.compile(r"[0-9]{1,10}")
_AGENT = re.compile(r"[a-z0-9_-]{1,32}")


def _match(pattern, value):
    return value if isinstance(value, str) and pattern.fullmatch(value) else None


def _ident(value):
    if not isinstance(value, str) or not value or len(value) > event_store.MAX_ID:
        return None
    return None if any(ord(c) < 32 for c in value) else value


def _process_key(pid):
    pid = _match(_PID, pid)
    if pid is None:
        return None
    start = process_start_time(pid)
    return f"{pid}-{start}" if start is not None else pid


def normalize_hook(payload):
    """Hook payload -> N1 event dict, or None when the hook is not a lifecycle event."""
    hook_event = payload.get("hookEvent")
    kind = HOOK_KINDS.get(hook_event)
    if kind is None:
        return None
    if kind == "input_requested" and payload.get("notificationType") in PERMISSION_NOTIFICATIONS:
        kind = "permission_requested"
    agent = _match(_AGENT, payload.get("agent")) or "unknown"
    session = _match(_SESSION, payload.get("session"))
    pane = _match(_PANE, payload.get("pane"))
    turn = _ident(payload.get("turnId")) or _ident(payload.get("promptId"))
    request = _ident(payload.get("requestId"))
    if turn or (request and kind == "permission_requested"):
        correlation = "source"
    elif session and pane:
        correlation = "local"
    else:
        correlation = "unknown"
    project = payload.get("project")
    at = payload.get("occurredAtMs")
    return {
        "source": f"hook:{agent}", "harness": agent,
        "projectKey": _ident(project) if isinstance(project, str) else None,
        "sessionKey": session, "paneId": pane, "paneKey": None,
        "processKey": _process_key(payload.get("panePid")),
        "conversationId": _ident(payload.get("conversationId")),
        "turnId": turn, "requestId": request,
        "sourceEventId": _ident(payload.get("sourceEventId")),
        "kind": kind, "evidence": "confirmed", "correlation": correlation,
        "occurredAtMs": at if isinstance(at, int) and not isinstance(at, bool) and at >= 0 else None,
        "title": payload.get("title") if isinstance(payload.get("title"), str) else "",
        "excerpt": payload.get("excerpt") if isinstance(payload.get("excerpt"), str) else "",
    }


def resolve_pane_key(connection, event):
    """paneKey whose W1 binding is this tmux pane *and* this process."""
    if not (event.get("sessionKey") and event.get("paneId")):
        return None
    row = connection.execute("SELECT document FROM workspace_current WHERE id = 1").fetchone()
    try:
        bindings = (json.loads(row[0]).get("bindings") or {}) if row else {}
    except (ValueError, AttributeError):
        return None
    pid, _, start = (event.get("processKey") or "").partition("-")
    found = []
    for key, binding in bindings.items():
        if not isinstance(binding, dict) or binding.get("session") != event["sessionKey"] \
                or binding.get("paneId") != event["paneId"]:
            continue
        if pid and binding.get("pid") is not None and str(binding["pid"]) != pid:
            continue  # %N reused by another process
        if start and binding.get("startTime") is not None and str(binding["startTime"]) != start:
            continue
        found.append(key)
    return found[0] if len(found) == 1 else None


def record(connection, payload):
    """Normalize, resolve and persist one reception; returns the stored event."""
    event = normalize_hook(payload) if "hookEvent" in payload else dict(payload)
    if event is None:
        return None
    own = not connection.in_transaction
    if own:
        connection.execute("BEGIN IMMEDIATE")
    try:
        if not event.get("paneKey"):
            event["paneKey"] = resolve_pane_key(connection, event)
        stored = event_store.append_event(connection, event)
        after_append(connection, stored)
        if own:
            connection.execute("COMMIT")
    except BaseException:
        if own and connection.in_transaction:
            connection.execute("ROLLBACK")
        raise
    return stored


def after_append(connection, event):
    """Consumers that must change atomically with the event (E1 marks)."""
    if not event.get("duplicate"):
        work_marks.apply_turn_event(connection, event)


def import_legacy(connection, path):
    """Import the old timeline once as events without an exact destination.

    Legacy rows only carry project/status/detail/ts: they never target a pane
    or session and never drive turn state (evidence 'historical')."""
    own = not connection.in_transaction
    if own:
        connection.execute("BEGIN IMMEDIATE")
    try:
        done = connection.execute("SELECT value FROM workspace_meta WHERE key = ?",
                                  (LEGACY_MARKER,)).fetchone()
        count = 0
        if not done:
            # Hooks may have recorded N1 events before this import runs; their
            # timeline lines are the same receptions, not history.
            first = connection.execute("SELECT MIN(occurred_at_ms) FROM events "
                                       "WHERE evidence != 'historical'").fetchone()[0]
            cutoff = first // 1000 if first is not None else None
            seen = {}
            try:
                with open(path, encoding="utf-8", errors="replace") as fh:
                    lines = fh.read().splitlines()
            except FileNotFoundError:
                lines = []
            for line in lines:
                try:
                    row = json.loads(line)
                except ValueError:
                    continue
                kind = LEGACY_KINDS.get(row.get("status")) if isinstance(row, dict) else None
                ts = row.get("ts") if kind else None
                if not kind or not isinstance(ts, (int, float)) or isinstance(ts, bool) or ts < 0:
                    continue
                if cutoff is not None and ts >= cutoff:
                    continue
                digest = hashlib.sha256(line.encode()).hexdigest()[:32]
                seen[digest] = seen.get(digest, 0) + 1
                project = row.get("project") if isinstance(row.get("project"), str) else ""
                event_store.append_event(connection, {
                    "source": "legacy-timeline", "sourceEventId": f"legacy:{digest}:{seen[digest]}",
                    "projectKey": _ident(project), "kind": kind, "evidence": "historical",
                    "correlation": "unknown", "occurredAtMs": int(ts * 1000),
                    "title": project, "excerpt": row.get("detail") if isinstance(row.get("detail"), str) else "",
                })
                count += 1
            connection.execute("INSERT INTO workspace_meta VALUES (?, ?)",
                               (LEGACY_MARKER, json.dumps({"at": time.time(), "count": count})))
        if own:
            connection.execute("COMMIT")
    except BaseException:
        if own and connection.in_transaction:
            connection.execute("ROLLBACK")
        raise
    return count


def open_state(path=None, busy_ms=1500):
    """Connect and migrate; a concurrent migrator is retried, not fatal."""
    for attempt in range(3):
        conn = app_state.connect(path)
        conn.execute(f"PRAGMA busy_timeout = {int(busy_ms)}")
        try:
            app_state.migrate(conn)
            return conn
        except sqlite3.Error:
            conn.close()
            if attempt == 2:
                raise
            time.sleep(0.05 * (attempt + 1))


def main(argv):
    if argv[1:] != ["record"]:
        print("uso: event_intake.py record < evento.json", file=sys.stderr)
        return 2
    try:
        payload = json.loads(sys.stdin.read() or "{}")
        if not isinstance(payload, dict):
            return 1
        conn = open_state()
        try:
            record(conn, payload)
        finally:
            conn.close()
    except (ValueError, sqlite3.Error, OSError) as exc:
        print(f"event_intake: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
