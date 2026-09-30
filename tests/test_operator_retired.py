"""S4: the CommandOS operator chat backend is retired.

Every /operator* route answers 410, the chat modules are gone, the
conversations already on disk stay exactly as they were, and the desktop
action bridge (POST /app/command -> app-command.json -> cc-app) survives.
"""
from pathlib import Path

from dash_harness import dash  # noqa: F401  (pytest fixture)

ROOT = Path(__file__).resolve().parents[1]
RETIRED = {"error": "El chat de CommandOS se retiró; usa la barra de comandos", "code": "retired"}


def test_operator_routes_answer_410(dash):
    for path in ("/operator?id=x", "/operator/action-results"):
        assert dash.get_status(path) == 410
    for path in ("/operator/chat", "/operator/chat/stream", "/operator/new", "/operator/model", "/operator/action-result"):
        assert dash.post_status(path, {}) == 410


def test_retired_body_is_the_agreed_message(dash):
    import json
    status, raw = dash._open("POST", "/operator/chat", {"text": "hola"})
    assert status == 410 and json.loads(raw) == RETIRED
    status, raw = dash._open("GET", "/operator")
    assert status == 410 and json.loads(raw) == RETIRED


def test_post_to_a_retired_route_still_requires_the_token(dash):
    assert dash.post_status("/operator/chat", {"text": "hola"}, token=False) == 401


def test_conversations_on_disk_are_not_touched(dash):
    store = dash.home / ".claude" / "hooks" / "operator"
    store.mkdir(parents=True, exist_ok=True)
    convo = store / "conversations.json"
    memory = store / "memory.md"
    convo.write_bytes(b'{"current":"c1","conversations":[{"id":"c1","messages":[]}]}')
    memory.write_bytes(b"recuerdo\n")
    before = {p.name: (p.read_bytes(), p.stat().st_mtime_ns) for p in store.iterdir()}
    for path in ("/operator/new", "/operator/chat", "/operator/model", "/operator/action-result"):
        dash.post_status(path, {"text": "hola", "model": "haiku"})
    dash.get_status("/operator?id=c1")
    after = {p.name: (p.read_bytes(), p.stat().st_mtime_ns) for p in store.iterdir()}
    assert after == before


def test_app_command_bridge_survives(dash):
    assert dash.post_status("/app/command", {}) != 410
    # A real catalog command reaches the desktop bridge; with no app to pick up
    # app-command.json it is a 409, never a 410.
    status, raw = dash._open("POST", "/app/command", {"command": "help"})
    assert status == 409 and b"app" in raw


def test_no_chat_module_is_loaded():
    src = (ROOT / "bin" / "cc-dash").read_text()
    assert "operator_chat" not in src and "operator_stream" not in src and "operator_handle_chat" not in src
    assert "operator_tools" not in src
    for gone in ("operator_chat.py", "operator_stream.py", "operator_tools.py"):
        assert not (ROOT / "lib" / gone).exists(), gone
