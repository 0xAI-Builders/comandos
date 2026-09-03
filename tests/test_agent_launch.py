#!/usr/bin/env python3
"""Unit tests for cc-dash's agent_launch resolver (custom launch commands).

WHY: users with wrapper commands (claude-mpm, claude-personal) need the
dashboard's revive/new-session path to launch their command instead of
falling back to plain `claude`. agent_launch() is a pure function over
read_conf() + the AGENT_LAUNCH table, so it is unit-testable by monkeypatching
read_conf on the loaded module.
"""
import importlib.machinery
import importlib.util
import sys
from pathlib import Path


def load_dash_module():
    bin_dir = str(Path("bin").resolve())
    if bin_dir not in sys.path:
        sys.path.insert(0, bin_dir)
    loader = importlib.machinery.SourceFileLoader(
        "cc_dash_under_test", str(Path("bin/cc-dash").resolve()))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


def test_custom_launch_from_conf_with_normalized_key(monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(
        dash, "read_conf",
        lambda: {"AGENT_LAUNCH_CLAUDE_MPM": "claude-mpm run --continue"})
    assert dash.agent_launch("claude-mpm") == "claude-mpm run --continue"


def test_custom_launch_overrides_builtin(monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(
        dash, "read_conf", lambda: {"AGENT_LAUNCH_CLAUDE": "my-claude-wrapper"})
    assert dash.agent_launch("claude") == "my-claude-wrapper"


def test_builtin_default_when_no_conf_override(monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(dash, "read_conf", lambda: {})
    assert dash.agent_launch("codex") == dash.AGENT_LAUNCH["codex"]


def test_unknown_agent_falls_back_to_claude(monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(dash, "read_conf", lambda: {})
    assert dash.agent_launch("totally-unknown") == dash.AGENT_LAUNCH["claude"]


def test_dot_and_dash_both_normalize_to_underscore(monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(
        dash, "read_conf", lambda: {"AGENT_LAUNCH_MY_A_B": "custom-cmd"})
    assert dash.agent_launch("my-a.b") == "custom-cmd"


def test_read_conf_removes_only_one_matching_outer_quote_pair(tmp_path, monkeypatch):
    dash = load_dash_module()
    hooks = tmp_path / "hooks"
    hooks.mkdir()
    (hooks / "cc-notify.conf").write_text(
        'AGENT_LAUNCH_CLAUDE_MPM="claude-mpm --label \'daily account\'"\n'
    )
    monkeypatch.setattr(dash, "HOOKS", str(hooks))

    assert dash.read_conf()["AGENT_LAUNCH_CLAUDE_MPM"] == (
        "claude-mpm --label 'daily account'"
    )


if __name__ == "__main__":
    import pytest
    raise SystemExit(pytest.main([__file__, "-q"]))


def test_grok_builtin_launch_uses_official_continue(monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(dash, "read_conf", lambda: {})
    assert dash.agent_launch("grok") == "grok --continue 2>/dev/null || grok"
    assert dash.agent_process_aliases()["grok-linux-x86_64"] == "grok"


def test_generic_account_environment_is_applied_to_native_launches():
    source = Path("bin/cc-dash").read_text()
    assert "account_registry.account_environment(load_provider_registry(), agent, alias)" in source
    assert 'cmd = account_prefix + "codex"' in source
    assert 'cmd = account_prefix + "grok"' in source
    assert 'cmd = account_prefix + "claude"' in source
    assert 'alias, motor_alias, route["id"], "session-new"' in source


def test_acp_pane_launch_command_carries_motor_model_effort_account_and_danger():
    dash = load_dash_module()
    assert dash.agent_launch("acp") == "cc-acp"
    assert dash._acp_launch_cmd("codex", "gpt-5.5", "low", "main") == "cc-acp --agent codex --model gpt-5.5 --effort low"
    assert dash._acp_launch_cmd("claude", "claude-fable-5[1m]", "", "relotto", danger=True, resume="abc-1") == \
        "cc-acp --agent claude --model 'claude-fable-5[1m]' --account relotto --resume abc-1 --danger"
    # opencode/agy nativos tambien salen de _harness_launch_cmd (sin prefijo de cuenta)
    agy_cmd = dash._harness_launch_cmd("agy", "gemini-3.8-flash-low", "low", "main")
    assert "agy" in agy_cmd and "gemini-3.8-flash-low" in agy_cmd and "--effort low" in agy_cmd
    oc_cmd = dash._harness_launch_cmd("opencode", "opencode/big-pickle", "", "main")
    assert "opencode" in oc_cmd and "opencode/big-pickle" in oc_cmd


def test_harness_switch_to_opencode_or_agy_drops_foreign_model_and_uses_absolute_binary():
    """Al saltar de Claude → OpenCode/AGY el pane manda el modelo de Claude
    (claude-fable-5). OpenCode exige provider/model y aborta con
    'Invalid model format'. Además tmux no tiene ~/.opencode/bin en PATH."""
    dash = load_dash_module()
    oc = dash._harness_launch_cmd("opencode", "claude-fable-5", "high", "main")
    assert "claude-fable-5" not in oc
    assert "opencode/" in oc or oc.rstrip().endswith("opencode") or "/opencode" in oc
    assert oc.strip().startswith("/") or oc.split()[0].startswith("/")
    agy = dash._harness_launch_cmd("agy", "claude-fable-5", "high", "main")
    assert "claude-fable-5" not in agy
    assert "--effort high" not in agy  # effort de Claude no es del catalogo agy si el modelo se descarto
    assert agy.strip().startswith("/") or agy.split()[0].startswith("/")


def test_acp_is_a_first_class_detected_agent(monkeypatch):
    """Sin 'acp' en DEFAULT_AGENTS, agent_info_for_pane nunca ve cc-acp:
    /harness/switch a ACP queda en 'no arrancó' y no se puede salir de ACP.
    Un AGENTS= custom tampoco puede dejarlo fuera: el registro manda."""
    dash = load_dash_module()
    monkeypatch.setattr(dash, "read_conf", lambda: {"AGENTS": "claude"})
    assert "acp" in dash.DEFAULT_AGENTS.split()
    assert "acp" in dash.agent_set()
    aliases = dash.agent_process_aliases()
    assert aliases["cc-acp"] == "acp"
    assert aliases["acp"] == "acp"


def test_agy_and_opencode_are_first_class_detected_agents(monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(dash, "read_conf", lambda: {"AGENTS": "claude"})
    aliases = dash.agent_process_aliases()
    assert "agy" in dash.agent_set()
    assert "opencode" in dash.agent_set()
    assert aliases["agy"] == "agy"
    assert aliases["opencode"] == "opencode"


def test_harness_switch_exits_acp_with_slash_exit_and_records_motor():
    source = Path("bin/cc-dash").read_text()
    # /exit sale limpio de cc-acp (C-c cancela el turno, no cierra el pane)
    assert 'from_harness in ("claude", "acp")' in source or \
           'from_harness in {"claude", "acp"}' in source
    # al aterrizar en ACP el motor es el cerebro (claude/codex/…), no "acp"
    assert "harness-handoff" in source
    assert "dest_motor" in source
    assert 'record_runtime_config(sess, pane, to, dest_motor' in source or \
           'record_runtime_config(sess, pane, to, payload.get("motor")' in source
