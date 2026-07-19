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
