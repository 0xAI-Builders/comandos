#!/usr/bin/env python3
"""Normalize Grok Build hook payloads without forwarding credentials."""

from __future__ import annotations

import json
import re
import sys
from collections.abc import Mapping
from typing import Any

SUPPORTED_EVENTS = frozenset(
    {
        "UserPromptSubmit",
        "Stop",
        "StopFailure",
        "StopCancelled",
        "Notification",
        "SessionEnd",
    }
)

_EVENT_ALIASES = {
    "user_prompt_submit": "UserPromptSubmit",
    "stop": "Stop",
    "stop_failure": "StopFailure",
    "stop_cancelled": "StopCancelled",
    "notification": "Notification",
    "session_end": "SessionEnd",
}

_EVENT_STATUS = {
    "UserPromptSubmit": "working",
    "Stop": "done",
    "StopFailure": "error",
    "StopCancelled": "cancelled",
    "Notification": "waiting",
    "SessionEnd": "end",
}

_NOTIFICATION_TYPES = frozenset({"permission_prompt", "idle_prompt"})
_PHASE = {
    "UserPromptSubmit": 10,
    "Notification": 20,
    "Stop": 30,
    "StopFailure": 30,
    "StopCancelled": 30,
    "SessionEnd": 40,
}

# Detail is intentionally selected from an allowlist. These substitutions are a
# second line of defence for secrets embedded in otherwise legitimate messages.
_SENSITIVE_ASSIGNMENT = re.compile(
    r"(?i)\b(authorization|auth|access[_-]?token|refresh[_-]?token|token|"
    r"api[_-]?key|password|passwd|client[_-]?secret)\b"
    r"(\s*[:=]\s*)([^\s,;]+)"
)
_BEARER = re.compile(r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]+")


def _text(value: Any) -> str:
    """Return a stable, redacted scalar representation or an empty string."""

    if value is None or isinstance(value, (Mapping, list, tuple, set)):
        return ""
    if isinstance(value, bool):
        text = "true" if value else "false"
    elif isinstance(value, (str, int, float)):
        text = str(value)
    else:
        return ""
    text = _BEARER.sub("Bearer [REDACTED]", text)
    return _SENSITIVE_ASSIGNMENT.sub(r"\1\2[REDACTED]", text)


def _nested_detail(value: Any) -> str:
    """Extract only known descriptive fields from a structured error."""

    if not isinstance(value, Mapping):
        return _text(value)
    for key in ("message", "detail", "reason", "description", "error", "code"):
        detail = _text(value.get(key))
        if detail:
            return detail
    return ""


def _first_detail(payload: Mapping[str, Any], *keys: str) -> str:
    for key in keys:
        detail = _nested_detail(payload.get(key))
        if detail:
            return detail
    return ""


def normalize(payload: Any) -> dict[str, str] | None:
    """Return the deterministic ComandOS representation of one Grok hook.

    Unknown events, unsupported notification kinds, subagent lifecycle events,
    and malformed payloads fail open by returning ``None``. Output is built from
    an explicit allowlist, so token/auth fields from the input cannot be echoed.
    """

    if not isinstance(payload, Mapping):
        return None

    event = payload.get("hookEventName")
    if isinstance(event, str):
        event = _EVENT_ALIASES.get(event, event)
    if not isinstance(event, str) or event not in SUPPORTED_EVENTS:
        return None
    if payload.get("subagentType"):
        return None

    if event == "Notification":
        notification_type = payload.get("notificationType") or payload.get("notification_type")
        if notification_type not in _NOTIFICATION_TYPES:
            return None
        detail = _first_detail(payload, "message", "detail", "reason", "title")
        if not detail:
            detail = str(notification_type)
        status = "idle" if notification_type == "idle_prompt" else "waiting"
    elif event == "UserPromptSubmit":
        # Prompts may themselves contain credentials and are not needed for state.
        detail = ""
    elif event == "Stop":
        detail = _first_detail(payload, "lastAssistantMessage", "reason", "message")
    elif event == "StopFailure":
        detail = _first_detail(payload, "errorDetails", "error", "reason", "message")
    elif event == "StopCancelled":
        detail = _first_detail(payload, "reason", "message", "lastAssistantMessage")
    else:  # SessionEnd
        detail = _first_detail(payload, "reason", "message")

    # Insertion order is part of the adapter contract and makes CLI JSON stable.
    normalized = {
        "event": event,
        "status": status if event == "Notification" else _EVENT_STATUS[event],
        "detail": detail,
    }
    if event == "StopCancelled":
        normalized["status"] = "idle"
    for key in ("sessionId", "promptId", "model", "effort", "pane"):
        value = _text(payload.get(key))
        if value:
            normalized[key] = value
    return normalized


normalize_event = normalize


def event_order_key(event: Mapping[str, Any] | None) -> tuple[str, str, int]:
    """Return a deterministic grouping/phase key for normalized events."""

    if not isinstance(event, Mapping):
        return ("", "", -1)
    session_id = _text(event.get("sessionId"))
    prompt_id = _text(event.get("promptId"))
    phase = _PHASE.get(event.get("event"), -1)
    return (session_id, prompt_id, phase)


def should_accept_event(
    current: Mapping[str, Any] | None,
    candidate: Mapping[str, Any] | None,
) -> bool:
    """Decide whether ``candidate`` may replace ``current``.

    A new prompt becomes authoritative immediately. Once it is current, a late
    notification or terminal event for a different (or unidentifiable) prompt is
    rejected. Events for one prompt may only move forward through its phases.
    This keeps a delayed Stop for prompt A from overwriting prompt B's working
    state, without clocks, mutable globals, or assumptions about prompt ID shape.
    """

    if not isinstance(candidate, Mapping) or candidate.get("event") not in _PHASE:
        return False
    if not isinstance(current, Mapping) or current.get("event") not in _PHASE:
        return True

    current_session = _text(current.get("sessionId"))
    candidate_session = _text(candidate.get("sessionId"))
    if current_session and candidate_session and current_session != candidate_session:
        return True

    candidate_event = candidate["event"]
    current_event = current["event"]
    if candidate_event == "SessionEnd":
        return True
    if current_event == "SessionEnd":
        return False

    current_prompt = _text(current.get("promptId"))
    candidate_prompt = _text(candidate.get("promptId"))
    if current_prompt and candidate_prompt and current_prompt != candidate_prompt:
        return candidate_event == "UserPromptSubmit"

    if current_prompt and not candidate_prompt and current_event == "UserPromptSubmit":
        return candidate_event == "UserPromptSubmit"

    if current_prompt == candidate_prompt:
        return _PHASE[candidate_event] >= _PHASE[current_event]

    # With no correlatable IDs, a submit is the only reliable new-turn marker.
    return candidate_event == "UserPromptSubmit"


REJECTED_EXIT = 3


def accept_and_record(path: str, candidate: Mapping[str, Any] | None) -> bool:
    """Apply ``should_accept_event`` against the event persisted at ``path``.

    The file holds only the correlation fields of the last accepted event
    (never details). It is updated under an exclusive lock so two concurrent
    hooks of the same pane cannot both win. I/O problems fail open.
    """

    try:
        import fcntl
        import os

        fd = os.open(path, os.O_RDWR | os.O_CREAT, 0o600)
    except (ImportError, OSError):
        return True
    with os.fdopen(fd, "r+", encoding="utf-8") as handle:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX)
            raw = handle.read()
        except (OSError, UnicodeError):
            return True
        try:
            current = json.loads(raw) if raw.strip() else None
        except ValueError:
            current = None
        if not should_accept_event(current, candidate):
            return False
        record = {
            key: candidate[key]
            for key in ("event", "sessionId", "promptId")
            if isinstance(candidate, Mapping) and candidate.get(key)
        }
        try:
            handle.seek(0)
            handle.truncate()
            json.dump(record, handle, separators=(",", ":"))
        except OSError:
            pass
    return True


def main(argv: list[str] | None = None) -> int:
    """Read one JSON object from stdin and write normalized JSON when supported.

    ``--accept PATH``: stdin is an already normalized event; exit 0 when it
    may replace the pane's current event (and record it), ``REJECTED_EXIT``
    when it is a late/out-of-order event that must be dropped.
    """

    args = sys.argv[1:] if argv is None else argv
    if len(args) == 2 and args[0] == "--accept":
        try:
            candidate = json.load(sys.stdin)
        except (json.JSONDecodeError, OSError, UnicodeError, ValueError):
            return 0
        if not isinstance(candidate, Mapping):
            return 0
        return 0 if accept_and_record(args[1], candidate) else REJECTED_EXIT

    try:
        payload = json.load(sys.stdin)
    except (json.JSONDecodeError, OSError, UnicodeError, ValueError):
        return 0

    normalized = normalize(payload)
    if normalized is not None:
        json.dump(normalized, sys.stdout, ensure_ascii=False, separators=(",", ":"))
        sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
