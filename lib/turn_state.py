"""Observable turn state per pane, reconciled from N1 events.

``reduce_turn`` is pure: an event that does not belong to the current
process/turn, or that would move the pane back to an older turn, returns
``current`` unchanged (the same object). Historical or inferred events never
drive state; only hooks that fired (``evidence == 'confirmed'``) do.
"""

STARTS = {"prompt_accepted": "working", "turn_started": "working"}
REQUESTS = {"permission_requested": "awaiting_permission", "input_requested": "awaiting_input"}
TERMINALS = {"turn_completed": "completed", "turn_cancelled": "cancelled", "turn_failed": "failed"}
ENDINGS = {"pane_closed": "closed", "session_ended": "ended"}
FINISHED = frozenset(TERMINALS.values()) | frozenset(ENDINGS.values())


def _at(event):
    value = event.get("occurredAtMs")
    return value if isinstance(value, (int, float)) and not isinstance(value, bool) else None


def _not_older(event, current):
    at, started = _at(event), current.get("startedAtMs")
    return at is None or started is None or at >= started


def _same_turn(event, current):
    if not current.get("turnId"):
        return True
    turn = event.get("turnId")
    if turn and not str(current["turnId"]).startswith("local:"):
        return turn == current["turnId"]
    # Without provider ids the only honest ordering is time of occurrence.
    return _not_older(event, current)


def _base(event, current):
    out = dict(current or {})
    out.update(lastEventId=event.get("eventId"), updatedAtMs=_at(event),
               evidence=event.get("evidence"))
    if event.get("processKey"):
        out["processKey"] = event["processKey"]
    if event.get("conversationId"):
        out["conversationId"] = event["conversationId"]
    return out


def reduce_turn(current, event):
    kind = (event or {}).get("kind")
    if kind not in STARTS and kind not in REQUESTS and kind not in TERMINALS and kind not in ENDINGS:
        return current
    if event.get("evidence") != "confirmed":
        return current
    cur = current or {}
    process = event.get("processKey")
    foreign = bool(process and cur.get("processKey") and process != cur["processKey"])

    if kind in ENDINGS:
        if foreign:
            return current
        out = _base(event, cur)
        out["state"] = ENDINGS[kind]
        return out

    if kind in STARTS:
        turn = event.get("turnId")
        if not foreign and turn and turn == cur.get("turnId"):
            return current  # repeated or late start of the known turn
        if cur and not _not_older(event, cur):
            return current  # an older prompt (or a dead process') arriving late
        out = _base(event, {} if foreign else cur)
        out.update(state=STARTS[kind], turnId=turn or f"local:{event.get('eventId')}",
                   correlation=event.get("correlation") if turn else "local",
                   startedAtMs=_at(event), requestId=None)
        out.pop("finishedAtMs", None)
        return out

    if foreign or not _same_turn(event, cur):
        return current
    if cur.get("state") in FINISHED:
        return current  # a second completion, or an idle notice after it
    out = _base(event, cur)
    if not cur.get("turnId"):
        out.update(turnId=event.get("turnId") or f"local:{event.get('eventId')}",
                   correlation=event.get("correlation") if event.get("turnId") else "local",
                   startedAtMs=None)
    if kind in REQUESTS:
        out.update(state=REQUESTS[kind], requestId=event.get("requestId"))
    else:
        out.update(state=TERMINALS[kind], finishedAtMs=_at(event))
    return out


def target_of(event):
    """Exact destination of a lifecycle event; None when it has none."""
    if event.get("paneKey"):
        return "pane:" + event["paneKey"]
    if event.get("sessionKey") and event.get("paneId"):
        return f"tmux:{event['sessionKey']}:{event['paneId']}"
    return None


def turns_from_events(events):
    """Fold events (ascending sequence) into ``{target: state}``."""
    turns = {}
    for event in events:
        target = target_of(event)
        if target is None:
            continue
        before = turns.get(target)
        after = reduce_turn(before, event)
        if after is not before and after is not None:
            turns[target] = dict(after, target=target, sessionKey=event.get("sessionKey"),
                                 paneKey=event.get("paneKey"), paneId=event.get("paneId"),
                                 projectKey=event.get("projectKey"), harness=event.get("harness"))
    return turns
