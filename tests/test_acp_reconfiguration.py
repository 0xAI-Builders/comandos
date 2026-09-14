"""ACP pane changes use the real pane class with a controlled session transport."""
import copy
import importlib.machinery
import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace

import pytest

from test_session_route_matrix import REGISTRY


@pytest.fixture
def client(tmp_path, monkeypatch):
    monkeypatch.setenv('HOME', str(tmp_path))
    loader = importlib.machinery.SourceFileLoader('acp_changes_test', str(Path('bin/cc-acp').resolve()))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec); loader.exec_module(module)
    registry = copy.deepcopy(REGISTRY)
    for provider in ('claude', 'codex', 'grok'):
        registry['harnesses'][provider].update(defaultHome=str(tmp_path / provider / 'main'), accountsRoot=str(tmp_path / provider))
    monkeypatch.setattr(module.provider_registry, 'load_registry', lambda: registry)
    monkeypatch.setattr(module.account_registry, 'list_accounts', lambda *args: [{'alias': a, 'selectable': True} for a in ('main', 'work')])
    monkeypatch.setattr(module.acp, 'agent_available', lambda spec: True)
    models = [m['id'] for m in registry['motors']['claude']['models']]
    events = []
    class Session:
        def __init__(self, label):
            self.label, self.session_id = label, 'original-session'
            self.current_model, self.current_mode = models[0], 'default'
            self.models = [{'modelId': m} for m in models]
            self.modes = [{'id': 'default'}, {'id': 'bypassPermissions'}]
            self.closed = False
            self.load_supported = True
            self.fail = ''
        def step(self, name):
            events.append((self.label, name))
            if self.fail == name:
                raise module.acp.AcpError('injected ' + name)
        def initialize(self): self.step('initialize')
        def supports_load(self): return self.load_supported
        def new_session(self):
            self.step('new'); self.session_id = 'new-session'
        def load_session(self, sid, **kwargs):
            self.step('load'); self.session_id = sid
        def set_model(self, model):
            self.step('model'); self.current_model = model
        def set_mode(self, mode):
            self.step('mode'); self.current_mode = mode
        def close(self):
            self.step('close'); self.closed = True
    origin, target = Session('origin'), Session('target')
    monkeypatch.setattr(module.acp, 'open_session', lambda *args, **kwargs: target)
    cwd = tmp_path / 'workspace'; cwd.mkdir()
    pane = module.Pane(SimpleNamespace(cwd=str(cwd), agent='claude', model=models[0], effort='high',
                                      account='main', danger=False, resume=''))
    pane.session = origin
    monkeypatch.setattr(pane, 'publish', lambda: events.append(('pane', 'publish')))
    return SimpleNamespace(module=module, pane=pane, origin=origin, target=target, events=events, tmp_path=tmp_path)


def test_explicit_resume_cannot_fall_back_to_new_conversation(client):
    client.pane.resume_id = 'exact-session'
    client.target.load_supported = False
    with pytest.raises(client.module.acp.AcpError, match='reanud'):
        client.pane.connect()
    assert ('target', 'new') not in client.events
    assert client.target.closed
    assert not client.origin.closed


@pytest.mark.parametrize('command,expected', [('/effort low', 'low'), ('/model claude-sonnet-5', 'claude-sonnet-5')])
def test_successful_configuration_changes_report_applied_values(client, command, expected):
    client.pane.command(command)
    if command.startswith('/model'):
        assert client.pane.model == client.origin.current_model == expected
        assert not client.origin.closed
    else:
        assert client.events.index(('target', 'load')) < client.events.index(('origin', 'close'))
        assert client.pane.effort == expected
        assert client.pane.session.session_id == 'original-session'


@pytest.mark.parametrize('failure', ['initialize', 'load', 'model'])
def test_reconnect_failure_retains_running_origin_and_old_config(client, failure):
    client.target.fail = failure
    if failure == 'model':
        client.target.current_model = 'claude-sonnet-5'
    before = (client.pane.agent, client.pane.model, client.pane.effort, client.pane.account)
    with pytest.raises(client.module.acp.AcpError):
        client.pane.command('/effort low')
    assert not client.origin.closed
    assert client.pane.session is client.origin
    assert (client.pane.agent, client.pane.model, client.pane.effort, client.pane.account) == before
    assert client.target.closed


@pytest.mark.parametrize('command', ['/account work', '/agent codex', '/agent grok', '/agent opencode', '/agent agy'])
def test_unsupported_transfer_is_rejected_before_closing_origin(client, command):
    with pytest.raises(client.module.acp.AcpError):
        client.pane.command(command)
    assert not client.origin.closed
    assert client.pane.agent == 'claude' and client.pane.account == 'main'
    assert not client.events


def test_failed_live_model_change_keeps_old_model(client):
    client.origin.fail = 'model'
    before = client.pane.model
    client.pane.command('/model claude-sonnet-5')
    assert client.pane.model == before and not client.origin.closed


def test_danger_off_restores_provider_mode(client):
    client.pane.command('/danger')
    assert client.origin.current_mode == 'bypassPermissions' and client.pane.danger
    client.pane.command('/danger')
    assert client.origin.current_mode == 'default' and not client.pane.danger


def test_failed_danger_mode_does_not_claim_enabled(client):
    client.origin.fail = 'mode'
    with pytest.raises(client.module.acp.AcpError):
        client.pane.command('/danger')
    assert not client.pane.danger and client.origin.current_mode == 'default'


def test_connect_never_writes_workspace_trust(client):
    client.pane.connect()
    assert not (client.tmp_path / '.claude.json').exists()
    assert not (client.tmp_path / 'claude/main/.claude.json').exists()


@pytest.mark.parametrize('command', ['/account main', '/agent claude', '/effort high'])
def test_noop_commands_keep_process(client, command):
    client.pane.command(command)
    assert not client.events and not client.origin.closed


def test_effort_config_option_changes_live_without_restart(client):
    client.origin.current_effort = 'high'
    client.origin.config_option = lambda category: {'id': 'reasoning'} if category == 'thought_level' else None
    def set_effort(effort):
        client.events.append(('origin', 'effort'))
        client.origin.current_effort = effort
    client.origin.set_effort = set_effort
    client.pane.command('/effort low')
    assert client.pane.effort == 'low'
    assert client.events == [('origin', 'effort'), ('pane', 'publish')]
    assert not client.origin.closed


def test_published_effort_is_unknown_until_protocol_acknowledges_it(client, monkeypatch):
    path = client.tmp_path / 'state.json'
    monkeypatch.setattr(client.module, 'STATE_FILE', str(path))
    monkeypatch.setattr(client.module, 'tmux_pane', lambda: '%99')
    monkeypatch.setattr(client.module, 'tmux', lambda *args: None)
    client.module.Pane.publish(client.pane)
    state = json.loads(path.read_text())['%99']
    assert state['requestedEffort'] == 'high' and state['observedEffort'] == ''
    assert state['effortSource'] == 'unconfirmed'
    client.origin.current_effort = 'low'
    client.module.Pane.publish(client.pane)
    state = json.loads(path.read_text())['%99']
    assert state['observedEffort'] == 'low' and state['effortSource'] == 'acp-config-options'


def test_new_acp_state_directory_is_created(client, monkeypatch):
    path = client.tmp_path / 'new-hooks/state.json'
    monkeypatch.setattr(client.module, 'STATE_FILE', str(path))
    monkeypatch.setattr(client.module, 'tmux_pane', lambda: '')
    client.module.Pane.publish(client.pane)
    assert path.is_file()


def test_explicit_safe_mode_disables_local_auto_approval(client):
    client.pane.command('/danger')
    client.pane.command('/mode default')
    assert client.origin.current_mode == 'default'
    assert client.pane.danger is False
