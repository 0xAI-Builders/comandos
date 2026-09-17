import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
from test_agent_launch import load_dash_module

def test_inherit_trust_for_switch_only_between_claude_accounts(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    dash = load_dash_module()
    calls = []
    monkeypatch.setattr(dash.claude_trust, "inherit_cwd_trust",
                        lambda cwd, **kw: calls.append((cwd, kw)) or True)
    monkeypatch.setattr(dash, "_claude_config_dir",
                        lambda alias: None if alias == "main" else f"{tmp_path}/.claude-accounts/{alias}")
    assert dash.inherit_trust_for_switch("/repo", "main", "relotto", harness="claude") is True
    assert calls[0][0] == "/repo"
    assert calls[0][1]["dest_config_dir"].endswith("/relotto")
    assert calls[0][1]["source_config_dir"] is None
    assert dash.inherit_trust_for_switch("/repo", "main", "main", harness="claude") is False
    assert dash.inherit_trust_for_switch("/repo", "main", "work", harness="codex") is False
    assert len(calls) == 1

def test_inherit_trust_for_switch_does_not_invent_trust(tmp_path, monkeypatch):
    """Si la cuenta origen no tenía la carpeta aceptada, no se escribe nada."""
    monkeypatch.setenv("HOME", str(tmp_path))
    dash = load_dash_module()
    monkeypatch.setattr(dash, "_claude_config_dir",
                        lambda alias: None if alias == "main" else f"{tmp_path}/.claude-accounts/{alias}")
    repo = tmp_path / "repo"
    repo.mkdir()
    assert dash.inherit_trust_for_switch(str(repo), "main", "relotto", harness="claude") is False
    assert not (tmp_path / ".claude-accounts/relotto/.claude.json").exists()
