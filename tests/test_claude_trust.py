#!/usr/bin/env python3
"""Explicit legacy trust helper and safe automated launch boundaries.

Configuration operations must leave provider trust acceptance to the user.
The helper tests cover explicit calls only; launch tests never invoke it.
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import claude_trust  # noqa: E402


def _read(path):
    return json.loads(Path(path).read_text())


def test_ensure_cwd_trusted_writes_home_claude_json_not_only_config_dir(tmp_path):
    """Claude 2.1.263 loads ~/.claude.json. Marking only CLAUDE_CONFIG_DIR is a no-op."""
    home = tmp_path / "home"
    cfg = tmp_path / "cfg"
    cwd = tmp_path / "proj" / "ServerMacMini"
    home.mkdir()
    cfg.mkdir()
    cwd.mkdir(parents=True)
    (home / ".claude.json").write_text(json.dumps({
        "projects": {
            str(tmp_path / "proj"): {"hasTrustDialogAccepted": True, "keep": 1},
        }
    }))
    (cfg / ".claude.json").write_text(json.dumps({"projects": {}}))

    assert claude_trust.ensure_cwd_trusted(str(cwd), config_dir=str(cfg), home=str(home)) is True

    home_doc = _read(home / ".claude.json")
    assert home_doc["projects"][str(cwd.resolve())]["hasTrustDialogAccepted"] is True
    # parent entry stays intact
    assert home_doc["projects"][str(tmp_path / "proj")]["keep"] == 1
    cfg_doc = _read(cfg / ".claude.json")
    assert cfg_doc["projects"][str(cwd.resolve())]["hasTrustDialogAccepted"] is True


def test_ensure_cwd_trusted_is_idempotent_and_preserves_other_project_keys(tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    cwd = tmp_path / "repo"
    cwd.mkdir()
    (home / ".claude.json").write_text(json.dumps({
        "numStartups": 9,
        "projects": {
            str(cwd.resolve()): {
                "allowedTools": ["Bash"],
                "hasTrustDialogAccepted": False,
            }
        },
    }))
    claude_trust.ensure_cwd_trusted(str(cwd), home=str(home))
    claude_trust.ensure_cwd_trusted(str(cwd), home=str(home))
    doc = _read(home / ".claude.json")
    assert doc["numStartups"] == 9
    entry = doc["projects"][str(cwd.resolve())]
    assert entry["hasTrustDialogAccepted"] is True
    assert entry["allowedTools"] == ["Bash"]


def test_ensure_cwd_trusted_skips_home_directory_itself(tmp_path):
    """Claude treats home trust as session-only; persisting it is rejected."""
    home = tmp_path / "home"
    home.mkdir()
    (home / ".claude.json").write_text(json.dumps({"projects": {}}))
    assert claude_trust.ensure_cwd_trusted(str(home), home=str(home)) is False
    assert _read(home / ".claude.json")["projects"] == {}


def test_ensure_cwd_trusted_creates_missing_json(tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    cwd = tmp_path / "fresh"
    cwd.mkdir()
    assert claude_trust.ensure_cwd_trusted(str(cwd), home=str(home)) is True
    assert _read(home / ".claude.json")["projects"][str(cwd.resolve())]["hasTrustDialogAccepted"] is True


def test_ensure_cwd_trusted_rejects_relative_or_missing_cwd(tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    assert claude_trust.ensure_cwd_trusted("relative/path", home=str(home)) is False
    assert claude_trust.ensure_cwd_trusted(str(tmp_path / "nope"), home=str(home)) is False
    assert not (home / ".claude.json").exists()


def test_ensure_cwd_trusted_does_not_wipe_corrupt_claude_json(tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    cwd = tmp_path / "repo"
    cwd.mkdir()
    raw = "{not-json"
    (home / ".claude.json").write_text(raw)
    assert claude_trust.ensure_cwd_trusted(str(cwd), home=str(home)) is False
    assert (home / ".claude.json").read_text() == raw


def test_launch_commands_do_not_persist_workspace_trust(tmp_path, monkeypatch):
    from test_session_route_matrix import Boundary
    boundary = Boundary(tmp_path, monkeypatch)
    home_file = tmp_path / '.claude.json'
    home_file.write_text('{"projects": {}}')
    for account in ('main', 'work'):
        config = tmp_path / 'claude' / account / '.claude.json'
        config.write_text('{"projects": {}}')
        command = boundary.dash._configuration_command('claude', 'claude', 'claude-sonnet-5', 'high', account)
        assert '--model claude-sonnet-5' in command
        assert 'skip-permissions' not in command
        assert _read(config) == {'projects': {}}
    assert _read(home_file) == {'projects': {}}


def test_harness_switch_leaves_trust_dialog_unconfirmed_without_enter(tmp_path, monkeypatch):
    from test_session_route_matrix import Boundary
    boundary = Boundary(tmp_path, monkeypatch)
    boundary.configure_source()
    boundary.screen = 'Accessing workspace: /tmp\n> Yes, I trust this folder'
    def configure(data):
        data = {k: v for k, v in data.items() if k not in ('session', 'pane', 'requestId')}
        return 200, boundary.run(**data)[0]
    monkeypatch.setattr(boundary.dash, 'session_configure', configure)
    _, result = boundary.dash.harness_switch_apply({'sess': 'audit', 'pane': '%1', 'to': 'claude',
        'model': boundary.state['model'], 'effort': boundary.state['effort'], 'account': 'main'})
    assert result['ok'] is False
    assert not boundary.events


def test_account_snapshot_copies_history_without_accepting_destination_trust(tmp_path, monkeypatch):
    from test_session_route_matrix import Boundary
    boundary = Boundary(tmp_path, monkeypatch)
    boundary.configure_source()
    home_file = tmp_path / '.claude.json'
    home_file.write_text('{"projects": {}}')
    config = tmp_path / 'claude/work/.claude.json'
    config.write_text('{"projects": {}}')
    adapter = boundary.adapter(harnessAccount='work', motorAccount='work')
    snapshot = adapter.snapshot(adapter.prepare())
    assert snapshot['origin']['resume_id'] == 'original-session'
    assert boundary.transcript('claude', 'work', 'original-session').read_text() == 'original history\n'
    assert _read(config) == _read(home_file) == {'projects': {}}
    assert not boundary.events


def test_acp_connect_does_not_accept_workspace_trust(tmp_path, monkeypatch):
    import importlib.machinery
    import importlib.util
    from types import SimpleNamespace
    from test_session_route_matrix import Boundary
    boundary = Boundary(tmp_path, monkeypatch)
    loader = importlib.machinery.SourceFileLoader('acp_trust_test', str(ROOT / 'bin/cc-acp'))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    monkeypatch.setattr(module.provider_registry, 'load_registry', lambda: boundary.registry)
    monkeypatch.setattr(module.acp, 'agent_available', lambda spec: True)
    opened = []
    session = SimpleNamespace(initialize=lambda: None, new_session=lambda: opened.append('new'),
                              models=[{'modelId': 'claude-sonnet-5'}], modes=[], current_model='claude-sonnet-5', close=lambda: None)
    monkeypatch.setattr(module.acp, 'open_session', lambda *args, **kwargs: session)
    cwd = tmp_path / 'workspace'; cwd.mkdir()
    pane = module.Pane(SimpleNamespace(cwd=str(cwd), agent='claude', model='claude-sonnet-5',
                                      effort='high', account='work', danger=False, resume=''))
    monkeypatch.setattr(pane, 'publish', lambda: None)
    pane.connect()
    assert opened == ['new']
    assert not (tmp_path / '.claude.json').exists()
    assert not (tmp_path / 'claude/work/.claude.json').exists()
