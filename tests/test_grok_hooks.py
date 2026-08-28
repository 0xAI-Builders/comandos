#!/usr/bin/env python3
import importlib.util
import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
ADAPTER_PATH = ROOT / "adapters" / "grok-hooks.py"
SPEC = importlib.util.spec_from_file_location("grok_hooks", ADAPTER_PATH)
assert SPEC and SPEC.loader
GROK_HOOKS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GROK_HOOKS)


def payload(event, **values):
    return {"hookEventName": event, **values}


def test_user_prompt_submit_normalizes_metadata_without_prompt_body():
    actual = GROK_HOOKS.normalize(
        payload(
            "UserPromptSubmit",
            sessionId="session-7",
            promptId="prompt-9",
            model="grok-4.6",
            effort="high",
            pane="%4",
            prompt="do not expose this token=prompt-secret",
        )
    )

    assert actual == {
        "event": "UserPromptSubmit",
        "status": "working",
        "detail": "",
        "sessionId": "session-7",
        "promptId": "prompt-9",
        "model": "grok-4.6",
        "effort": "high",
        "pane": "%4",
    }


@pytest.mark.parametrize(
    ("event", "values", "status", "detail"),
    [
        ("Stop", {"lastAssistantMessage": "Finished cleanly"}, "done", "Finished cleanly"),
        (
            "StopFailure",
            {"error": "server_error", "errorDetails": {"message": "upstream failed"}},
            "error",
            "upstream failed",
        ),
        ("StopCancelled", {"reason": "interrupted"}, "idle", "interrupted"),
        ("SessionEnd", {"reason": "shutdown"}, "end", "shutdown"),
    ],
)
def test_terminal_events_have_stable_status_and_detail(event, values, status, detail):
    assert GROK_HOOKS.normalize(payload(event, **values)) == {
        "event": event,
        "status": status,
        "detail": detail,
    }


@pytest.mark.parametrize(("notification_type", "status"), [("permission_prompt", "waiting"), ("idle_prompt", "idle")])
def test_supported_notifications_have_correct_state(notification_type, status):
    assert GROK_HOOKS.normalize(
        payload(
            "Notification",
            notificationType=notification_type,
            message="User attention required",
            sessionId="s1",
            promptId="p1",
        )
    ) == {
        "event": "Notification",
        "status": status,
        "detail": "User attention required",
        "sessionId": "s1",
        "promptId": "p1",
    }


def test_notification_type_is_used_as_deterministic_fallback_detail():
    assert GROK_HOOKS.normalize(
        payload("Notification", notificationType="idle_prompt")
    ) == {
        "event": "Notification",
        "status": "idle",
        "detail": "idle_prompt",
    }


def test_parent_lifecycle_event_with_subagent_type_is_ignored():
    assert GROK_HOOKS.normalize(
        payload("Stop", subagentType="explore", lastAssistantMessage="child done")
    ) is None


@pytest.mark.parametrize(
    "bad_payload",
    [
        None,
        [],
        "json",
        {},
        {"hookEventName": 12},
        payload("Unknown"),
        payload("SubagentStart", subagentType="explore"),
        payload("SubagentStop", subagentType="explore"),
        payload("Notification", notificationType="other"),
        {"hook_event_name": "Stop"},
    ],
)
def test_malformed_unsupported_and_non_camel_case_payloads_are_ignored(bad_payload):
    assert GROK_HOOKS.normalize(bad_payload) is None


def test_only_scalar_metadata_is_emitted():
    actual = GROK_HOOKS.normalize(
        payload(
            "Stop",
            lastAssistantMessage="ok",
            sessionId={"unexpected": "object"},
            promptId=42,
            model=["grok-4.6"],
            effort=True,
            pane=None,
        )
    )

    assert actual == {
        "event": "Stop",
        "status": "done",
        "detail": "ok",
        "promptId": "42",
        "effort": "true",
    }


def test_token_and_auth_fields_are_never_echoed_and_embedded_secrets_are_redacted():
    secret = "secret-that-must-not-escape"
    actual = GROK_HOOKS.normalize(
        payload(
            "StopFailure",
            errorDetails={"authToken": secret, "message": f"authorization: Bearer {secret}"},
            token=secret,
            accessToken=secret,
            refreshToken=secret,
            auth={"apiKey": secret},
            apiKey=secret,
        )
    )
    encoded = json.dumps(actual)

    assert secret not in encoded
    assert set(actual) == {"event", "status", "detail"}
    assert "[REDACTED]" in actual["detail"]


def test_late_terminal_event_for_previous_prompt_is_rejected():
    current = GROK_HOOKS.normalize(
        payload("UserPromptSubmit", sessionId="s1", promptId="prompt-b")
    )
    late_stop = GROK_HOOKS.normalize(
        payload("Stop", sessionId="s1", promptId="prompt-a")
    )

    assert not GROK_HOOKS.should_accept_event(current, late_stop)


def test_same_prompt_moves_forward_but_not_backward():
    working = GROK_HOOKS.normalize(
        payload("UserPromptSubmit", sessionId="s1", promptId="prompt-a")
    )
    waiting = GROK_HOOKS.normalize(
        payload(
            "Notification",
            notificationType="permission_prompt",
            sessionId="s1",
            promptId="prompt-a",
        )
    )
    done = GROK_HOOKS.normalize(payload("Stop", sessionId="s1", promptId="prompt-a"))

    assert GROK_HOOKS.should_accept_event(working, waiting)
    assert GROK_HOOKS.should_accept_event(waiting, done)
    assert not GROK_HOOKS.should_accept_event(done, waiting)
    assert GROK_HOOKS.event_order_key(done) == ("s1", "prompt-a", 30)


def test_new_prompt_and_other_session_are_accepted():
    current = GROK_HOOKS.normalize(payload("Stop", sessionId="s1", promptId="prompt-a"))
    new_prompt = GROK_HOOKS.normalize(
        payload("UserPromptSubmit", sessionId="s1", promptId="prompt-b")
    )
    other_session = GROK_HOOKS.normalize(
        payload("Stop", sessionId="s2", promptId="prompt-z")
    )

    assert GROK_HOOKS.should_accept_event(current, new_prompt)
    assert GROK_HOOKS.should_accept_event(current, other_session)


def run_adapter(stdin):
    return subprocess.run(
        [sys.executable, str(ADAPTER_PATH)],
        input=stdin,
        text=True,
        capture_output=True,
        check=False,
    )


def test_cli_reads_stdin_and_prints_compact_deterministic_json():
    result = run_adapter(
        json.dumps(
            payload(
                "Stop",
                lastAssistantMessage="listo",
                sessionId="s1",
                promptId="p1",
                model="grok-4.6",
                effort="xhigh",
                pane="%8",
            )
        )
    )

    assert result.returncode == 0
    assert result.stderr == ""
    assert result.stdout == (
        '{"event":"Stop","status":"done","detail":"listo",'
        '"sessionId":"s1","promptId":"p1","model":"grok-4.6",'
        '"effort":"xhigh","pane":"%8"}\n'
    )


@pytest.mark.parametrize("stdin", ["", "not-json", "[]", '{"hookEventName":"Unknown"}'])
def test_cli_fails_open_silently(stdin):
    result = run_adapter(stdin)

    assert result.returncode == 0
    assert result.stdout == ""
    assert result.stderr == ""

@pytest.mark.parametrize(
    ("wire", "canonical"),
    [
        ("user_prompt_submit", "UserPromptSubmit"),
        ("stop", "Stop"),
        ("stop_failure", "StopFailure"),
        ("stop_cancelled", "StopCancelled"),
        ("notification", "Notification"),
        ("session_end", "SessionEnd"),
    ],
)
def test_official_cli_snake_case_event_values_are_accepted(wire, canonical):
    values = {"notificationType": "permission_prompt"} if canonical == "Notification" else {}
    actual = GROK_HOOKS.normalize(payload(wire, **values))
    assert actual and actual["event"] == canonical
