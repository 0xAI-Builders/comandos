"""Login requests launch isolated subscription logins without touching live panes."""
import json
import shlex
from types import SimpleNamespace

import pytest

from test_agent_launch import load_dash_module
from test_provider_accounts import registry


@pytest.fixture
def login(tmp_path, monkeypatch):
    dash = load_dash_module()
    reg = registry(tmp_path)
    calls = []
    monkeypatch.setattr(dash, 'load_provider_registry', lambda: reg)
    monkeypatch.setattr(dash, 'register_app_tab', lambda *a, **k: None)
    monkeypatch.setattr(dash.time, 'sleep', lambda _: None)
    class ImmediateThread:
        def __init__(self, target, **kwargs): self.target = target
        def start(self): self.target()
    monkeypatch.setattr(dash.threading, 'Thread', ImmediateThread)
    def tmux(*args):
        calls.append(args)
        return SimpleNamespace(returncode=1 if args[0] == 'has-session' else 0, stderr='')
    monkeypatch.setattr(dash, 'tmux', tmux)
    return dash, reg, calls


@pytest.mark.parametrize('provider', ['claude', 'codex'])
def test_add_without_alias_assigns_a_name_and_uses_isolated_subscription_login(login, tmp_path, provider):
    dash, reg, calls = login
    code, result = dash.account_add_request({'provider': provider, 'cwd': str(tmp_path)})
    assert code == 200 and result['alias'] == 'cuenta-2'
    assert result['session']
    command = next(c[-2] for c in calls if c[0] == 'send-keys')
    argv = shlex.split(command)
    assert 'env' == argv[0]
    assert f"{reg['harnesses'][provider]['accountEnv']}={tmp_path / (provider + '-accounts') / 'cuenta-2'}" in argv
    if provider == 'claude':
        assert argv[-4:] == ['claude', 'auth', 'login', '--claudeai']
    else:
        assert argv[-1] == 'login'
        assert '--device-auth' not in argv
        config = tmp_path / 'codex-accounts' / result['alias'] / 'config.toml'
        assert 'cli_auth_credentials_store = "file"' in config.read_text()


def test_invalid_and_authenticated_aliases_never_launch_login(login, tmp_path):
    dash, reg, calls = login
    home = tmp_path / 'codex-accounts' / 'work'
    home.mkdir(parents=True)
    (home / 'auth.json').write_text(json.dumps({'tokens': {'refresh_token': 'private'}}))
    for alias in ('../escape', 'work'):
        code, result = dash.account_add_request({'provider': 'codex', 'alias': alias})
        assert code in (400, 409)
        assert result['error']
    assert not calls
    assert json.loads((home / 'auth.json').read_text())['tokens']['refresh_token'] == 'private'


def test_automatic_alias_skips_existing_accounts(login, tmp_path):
    dash, reg, calls = login
    (tmp_path / 'claude-accounts' / 'cuenta-2').mkdir(parents=True)
    code, result = dash.account_add_request({'provider': 'claude'})
    assert code == 200 and result['alias'] == 'cuenta-3'


def test_codex_remote_login_can_use_a_device_code(login):
    dash, reg, calls = login
    code, result = dash.account_add_request({'provider': 'codex', 'deviceAuth': True})
    assert code == 200
    assert shlex.split(next(c[-2] for c in calls if c[0] == 'send-keys'))[-2:] == ['login', '--device-auth']


def test_main_without_login_can_sign_in_without_inheriting_a_named_home(login):
    dash, reg, calls = login
    code, result = dash.account_add_request({'provider': 'claude', 'alias': 'main'})
    assert code == 200 and result['alias'] == 'main'
    argv = shlex.split(next(c[-2] for c in calls if c[0] == 'send-keys'))
    assert 'CLAUDE_CONFIG_DIR' in argv
    assert not any(a.startswith('CLAUDE_CONFIG_DIR=') for a in argv)
