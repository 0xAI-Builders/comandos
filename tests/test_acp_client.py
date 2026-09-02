#!/usr/bin/env python3
"""lib/acp.py: cliente ACP de ComandOS.

Unitarios contra tests/fake_acp_agent.py (sin red). Los E2E reales contra
los agentes instalados (claude/codex/grok/opencode/agy con SUS suscripciones)
corren solo con COMANDOS_E2E=1 — son la prueba de que la matriz ACP funciona
de verdad y no en el papel.
"""
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import acp  # noqa: E402
import providers  # noqa: E402

FAKE = str(ROOT / "tests" / "fake_acp_agent.py")


def fake_spec(**extra):
    spec = {"command": [sys.executable, FAKE], "modelArgs": ["--model", "{model}"],
            "effortArgs": ["--effort", "{effort}"], "dangerArgs": ["--yolo"]}
    spec.update(extra)
    return spec


def test_build_command_places_arg_groups_where_the_template_says():
    spec = {"command": ["python3", "{effort_args}", "{model_args}", "{danger_args}", "stdio"],
            "modelArgs": ["-m", "{model}"], "effortArgs": ["--effort", "{effort}"], "dangerArgs": ["--always-approve"]}
    argv, _env = acp.build_command(spec, model="grok-4.6", effort="low", danger=True)
    assert argv[1:] == ["--effort", "low", "-m", "grok-4.6", "--always-approve", "stdio"]
    argv, _env = acp.build_command(spec, model="", effort="", danger=False)
    assert argv[1:] == ["stdio"], "sin valores no quedan placeholders ni flags huerfanos"


def test_build_command_appends_groups_without_placeholder_and_resolves_env():
    spec = {"command": ["python3"], "modelArgs": ["-c", 'model="{model}"'], "env": {"CLAUDE_CODE_EXECUTABLE": "which:python3"},
            "effortEnv": "MAX_THINKING_TOKENS", "effortEnvMap": {"low": "1024"},
            "modelEnv": "OPENCODE_CONFIG_CONTENT", "modelEnvTemplate": '{"model":"{model}"}'}
    argv, env = acp.build_command(spec, model="gpt-5.5", effort="low", extra_env={"CODEX_HOME": "/x"})
    assert argv[1:] == ["-c", 'model="gpt-5.5"']
    assert env["CLAUDE_CODE_EXECUTABLE"].endswith("python3")
    assert env["MAX_THINKING_TOKENS"] == "1024"
    assert json.loads(env["OPENCODE_CONFIG_CONTENT"]) == {"model": "gpt-5.5"}
    assert env["CODEX_HOME"] == "/x"


def test_build_command_rejects_missing_binary():
    with pytest.raises(acp.AcpError):
        acp.build_command({"command": ["definitely-not-a-binary-xyz"]})


def test_session_roundtrip_with_permission_tools_and_model_switch(tmp_path):
    log = tmp_path / "log.jsonl"
    events, decisions = [], []

    def permission(req):
        decisions.append(req["title"])
        return "allow-once"

    session = acp.open_session(fake_spec(), str(tmp_path), model="fake-1", effort="high", danger=True,
                               extra_env={"FAKE_ACP_LOG": str(log), "CODEX_HOME": "/acct"},
                               permission_handler=permission)
    try:
        session.initialize()
        assert session.supports_load()
        sid = session.new_session()
        assert sid == "sess-fake-1"
        assert [m["modelId"] for m in session.models] == ["fake-1", "fake-2"]
        assert session.current_model == "fake-1"
        assert session.prompt("hola", on_event=events.append) == "end_turn"
        session.set_model("fake-2")
        assert session.current_model == "fake-2"
        assert session.prompt("otra", on_event=events.append) == "end_turn"
        session.load_session("sess-fake-1")
    finally:
        session.close()
    texts = [e["text"] for e in events if e["type"] == "text"]
    assert texts == ["eco[fake-1|allow-once]: hola", "eco[fake-2|allow-once]: otra"]
    assert decisions == ["escribir archivo", "escribir archivo"]
    assert [e["type"] for e in events].count("tool") == 2
    assert [e["type"] for e in events].count("end") == 2
    first = json.loads(log.read_text().splitlines()[0])
    assert first["argv"] == ["--model", "fake-1", "--effort", "high", "--yolo"]
    assert first["env"]["CODEX_HOME"] == "/acct", "la cuenta viaja como env al agente"


def test_session_auto_allows_when_no_permission_handler(tmp_path):
    session = acp.open_session(fake_spec(), str(tmp_path))
    try:
        session.initialize(); session.new_session()
        got = []
        session.prompt("x", on_event=got.append)
    finally:
        session.close()
    assert any(e["type"] == "permission" and e["decision"] == "allow-once" for e in got)


def test_registry_declares_every_acp_agent_with_a_runnable_shape():
    registry = providers.load_registry(ROOT / "config/providers.json")
    specs = acp.agent_specs(registry)
    assert set(specs) == {"claude", "codex", "grok", "opencode", "agy"}
    for ident, spec in specs.items():
        assert spec["command"], ident
        assert spec.get("transport", "acp") in ("acp", "agy-stream")
        if spec.get("accountsProvider"):
            assert (registry["harnesses"][spec["accountsProvider"]]["capabilities"]).get("accounts"), ident


# ---------------------------------------------------------------- E2E reales
E2E = os.environ.get("COMANDOS_E2E") == "1"
PROMPT = "Reply with exactly the word PONG and nothing else."


def _run_cc_acp(args, cwd, timeout=240):
    env = dict(os.environ, PATH=os.pathsep.join([os.path.expanduser("~/.local/bin"), os.path.expanduser("~/.bun/bin"),
                                                 os.environ.get("PATH", "")]))
    return subprocess.run([sys.executable, str(ROOT / "bin" / "cc-acp"), *args, "--danger", "--once", PROMPT],
                          cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout)


@pytest.mark.skipif(not E2E, reason="COMANDOS_E2E=1 para correr contra los agentes reales con tus suscripciones")
@pytest.mark.parametrize("args", [
    ["--agent", "claude", "--model", "claude-fable-5-1", "--effort", "low"],
    ["--agent", "claude", "--account", "relotto"],
    ["--agent", "codex", "--model", "gpt-5.5", "--effort", "low"],
    ["--agent", "grok", "--model", "grok-4.6", "--effort", "low"],
    ["--agent", "opencode", "--model", "opencode/big-pickle"],
    ["--agent", "agy", "--model", "gemini-3.8-flash-low", "--effort", "low"],
], ids=["claude-main", "claude-relotto", "codex", "grok", "opencode", "agy"])
def test_e2e_every_acp_agent_answers_with_its_own_subscription(tmp_path, args):
    result = _run_cc_acp(args, str(tmp_path))
    assert result.returncode == 0, result.stdout + result.stderr
    assert "PONG" in result.stdout, result.stdout + result.stderr
    assert "end_turn" in result.stdout
