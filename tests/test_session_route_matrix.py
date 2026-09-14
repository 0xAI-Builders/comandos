"""Real coordinator/adapter/registry with isolated account files and stub process IO.

The process boundary is simulated; these tests never authenticate or infer.
Private tmux coverage lives in test_session_tmux.py.
"""
import copy
import json
from pathlib import Path
import shlex
import sys
from types import SimpleNamespace

import pytest

from test_agent_launch import load_dash_module
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
from session_operations import OperationStore, run_operation

REGISTRY = json.loads(Path('config/providers.json').read_text())
CELLS = [(h, m) for h in REGISTRY['matrixHarnesses'] for m in REGISTRY['motors']]
RECOVERABLE = {'claude:claude', 'claude:codex', 'claude:grok', 'codex:codex', 'grok:grok'}


class Boundary:
    def __init__(self, tmp_path, monkeypatch):
        monkeypatch.setenv('HOME', str(tmp_path))
        self.dash = dash = load_dash_module()
        self.observe = dash.observe_pane
        self.exit_current = dash._pane_exit_current
        self.registry = registry = copy.deepcopy(REGISTRY)
        self.root = tmp_path
        self.events, self.busy = [], []
        self.state = {}
        self.pid = 100
        self.marker = ''
        self.fail_launch = False
        self.screen = '> ready'
        self.identity = dict(socket_path='private-fixture', pid='1', server_start='2', session_id='$1',
                             session_name='audit', pane_id='%1', pane_pid='99',
                             pane_current_command='bash', pane_current_path=str(tmp_path))
        self.store = OperationStore(tmp_path / 'ops.sqlite3')
        for provider, spec in registry['harnesses'].items():
            if not spec['capabilities'].get('accounts'):
                continue
            spec.update(defaultHome=str(tmp_path / provider / 'main'), accountsRoot=str(tmp_path / provider))
            auth = {'claude': {'claudeAiOauth': {'accessToken': 'test-only'}},
                    'codex': {'tokens': {'access_token': 'test-only'}},
                    'grok': {'test': {'refresh_token': 'test-only'}}}[provider]
            for alias in ('main', 'work'):
                home = tmp_path / provider / alias
                home.mkdir(parents=True)
                (home / spec['authFile']).write_text(json.dumps(auth))
        facts = {'harnesses': {h: {'available': True, 'authenticated': True} for h in registry['harnesses']},
                 'motors': {m: {'authenticated': True} for m in registry['motors']}, 'gateway': {'alive': True}}
        monkeypatch.setattr(dash, 'load_provider_registry', lambda: registry)
        monkeypatch.setattr(dash, 'capability_matrix', lambda: dash.provider_registry.evaluate_capability_matrix(registry, facts))
        monkeypatch.setattr(dash, 'session_operation_store', lambda: self.store)
        monkeypatch.setattr(dash, '_harness_bin', lambda h: '/stub/' + h)
        monkeypatch.setattr(dash.provider_registry, 'which', lambda binary: binary)
        monkeypatch.setattr(dash, 'proxy_alive', lambda: True)
        monkeypatch.setattr(dash, 'load_proxy_cfg', lambda: {'port': 18765})
        monkeypatch.setattr(dash, '_pane_identity', lambda *args: dict(self.identity))
        monkeypatch.setattr(dash, 'agent_info_for_pane', lambda pane: self.info())
        monkeypatch.setattr(dash, '_process_start', lambda pid: str(pid) + '-start')
        monkeypatch.setattr(dash, '_read_environ', lambda pid: {b'COMANDOS_OPERATION_ID': self.marker.encode()})
        monkeypatch.setattr(dash, 'observe_pane', lambda *args: dict(self.state))
        monkeypatch.setattr(dash, 'harness_pane_busy', lambda *args: self.busy.pop(0) if self.busy else False)
        monkeypatch.setattr(dash, 'capture_handoff', lambda *args, **kwargs: 'fixture handoff')
        monkeypatch.setattr(dash.pane_snapshot, 'PaneInspector', lambda: None)
        monkeypatch.setattr(dash.tmux_snapshot, 'capture_session', lambda *args: {'windows': [{'panes': [self.origin()]}]})
        monkeypatch.setattr(dash, '_restore_shell_tty', lambda pane: self.events.append('tty'))
        monkeypatch.setattr(dash, '_pane_exit_current', self.exit)
        monkeypatch.setattr(dash, '_send_shell_line', self.send)
        monkeypatch.setattr(dash, 'tmux', lambda *args: SimpleNamespace(returncode=0, stdout=self.screen, stderr=''))
        monkeypatch.setattr(dash, 'time', SimpleNamespace(sleep=lambda seconds: None, time=lambda: 1))

    def configure_source(self, harness='claude', motor=None, account='main'):
        motor = motor or harness
        model = self.registry['motors'].get(motor, {}).get('models', [{}])[0]
        self.state = dict(harness=harness, motor=motor, model=model.get('id', ''),
                          effort=model.get('defaultEffort', ''), harnessAccount=account if harness != 'acp' else 'main',
                          motorAccount=account, conversationId='original-session', confirmed=True,
                          source='conversation', identity=self.dash._identity_key(self.identity), pid=self.pid)
        if harness == 'shell':
            self.state = {}
        self.identity['pane_current_command'] = harness if harness != 'shell' else 'bash'
        if harness in ('claude', 'codex', 'grok'):
            self.transcript(harness, account, 'original-session').write_text('original history\n')

    def transcript(self, harness, account, sid):
        suffix = {'claude': f'projects/repo/{sid}.jsonl', 'codex': f'sessions/rollout-{sid}.jsonl',
                  'grok': f'sessions/repo/{sid}/summary.json'}[harness]
        path = self.root / harness / account / suffix
        path.parent.mkdir(parents=True, exist_ok=True)
        return path

    def info(self):
        return {'pid': self.pid, 'agent': self.state['harness']} if self.state else {}

    def origin(self):
        harness = self.state.get('harness', 'shell')
        result = dict(id='%1', agent=harness, cwd=str(self.root), resume_id=self.state.get('conversationId', ''), flags=[])
        if harness in ('claude', 'codex', 'grok'):
            result[{'claude': 'claude_config_dir', 'codex': 'codex_home', 'grok': 'grok_home'}[harness]] = str(self.root / harness / self.state['harnessAccount'])
        if harness == 'acp':
            result['acp'] = {'sessionId': self.state['conversationId']}
        return result

    def exit(self, pane, pid, harness, expected_start=None):
        self.events.append('exit')
        self.state = {}
        self.identity['pane_current_command'] = 'bash'
        return True

    def send(self, pane, command):
        self.events.append(command)
        if self.fail_launch:
            self.fail_launch = False
            return
        args = shlex.split(command)
        binary = next(a for a in args if a.startswith('/stub/'))
        harness = binary.rsplit('/', 1)[1]
        def arg(name, default=''):
            return args[args.index(name) + 1] if name in args else default
        motor = arg('--agent', harness)
        model = arg('-m', arg('--model'))
        if harness == 'claude':
            motor = self.dash.provider_registry.engine_for_model(self.registry, model)
        effort = arg('--effort', arg('-c').split('=')[-1].strip('"'))
        account = arg('--account', 'main')
        if harness in ('claude', 'codex', 'grok'):
            key = self.registry['harnesses'][harness]['accountEnv'] + '='
            account = next(a for a in args if a.startswith(key)).split('/')[-1]
        self.marker = next((a.split('=', 1)[1] for a in args if a.startswith('COMANDOS_OPERATION_ID=')), '')
        self.pid += 1
        self.state = dict(harness=harness, motor=motor, model=model, effort=effort,
                          harnessAccount=account if harness != 'acp' else 'main',
                          motorAccount=account if harness in ('acp', motor) else 'main',
                          conversationId=arg('resume', arg('--resume', 'new-session')), confirmed=True,
                          source='conversation', identity=self.dash._identity_key(self.identity), pid=self.pid)
        self.identity['pane_current_command'] = harness

    def adapter(self, **data):
        request = dict(session='audit', pane='%1', requestId='matrix-request', **data)
        adapter = self.dash.SessionConfiguration(request, dict(self.identity))
        adapter.store = self.store
        self.store.claim(request['requestId'], self.dash._identity_key(self.identity), request)
        return adapter

    def run(self, **data):
        adapter = self.adapter(**data)
        return run_operation(self.store, 'matrix-request', adapter), adapter


@pytest.fixture
def boundary(tmp_path, monkeypatch):
    return Boundary(tmp_path, monkeypatch)


@pytest.mark.parametrize('harness,motor', CELLS, ids=[f'{h}:{m}' for h, m in CELLS])
@pytest.mark.parametrize('dimension', ['model', 'effort', 'account', 'combined'])
def test_every_registered_matrix_cell(boundary, harness, motor, dimension):
    boundary.configure_source()
    spec = REGISTRY['motors'][motor]['models'][-1 if dimension in ('model', 'combined') else 0]
    account = 'work' if dimension in ('account', 'combined') else 'main'
    harness_account = account if harness in ('claude', 'codex', 'grok') else 'main'
    motor_account = account if harness == motor or harness == 'acp' else 'main'
    result, adapter = boundary.run(toHarness=harness, motor=motor, model=spec['id'],
        effort=(spec['efforts'] or [''])[0], harnessAccount=harness_account, motorAccount=motor_account)
    supported = f'{harness}:{motor}' in RECOVERABLE
    assert result['ok'] is supported, result
    if supported:
        observed = result['observed']
        assert (observed['harness'], observed['motor'], observed['model'], observed['effort']) == (
            harness, motor, spec['id'], (spec['efforts'] or [''])[0])
        assert (observed['harnessAccount'], observed['motorAccount']) == (harness_account, motor_account)
    else:
        assert 'exit' not in boundary.events


@pytest.mark.parametrize('harness', ['claude', 'codex', 'grok', 'acp', 'opencode', 'agy', 'gemini', 'shell'])
def test_every_source_harness_has_explicit_recovery_policy(boundary, harness):
    boundary.configure_source(harness, 'claude' if harness == 'acp' else harness)
    result, _ = boundary.run(toHarness='codex', motor='codex', model='gpt-5.5', effort='low')
    assert result['ok'] is (harness in ('claude', 'codex', 'grok', 'acp', 'shell')), result
    if not result['ok']:
        assert 'exit' not in boundary.events


@pytest.mark.parametrize('change', [dict(routeId='codex:codex', toHarness='claude', motor='claude'),
                                    dict(profileId='some-profile'), dict(expectedIdentity='stale'),
                                    dict(expectedConversationId='another-conversation')])
def test_stale_or_unsupported_draft_fails_before_exit(boundary, change):
    boundary.configure_source()
    result, _ = boundary.run(effort='low', **change)
    assert result['ok'] is False
    assert 'exit' not in boundary.events


@pytest.mark.parametrize('field,value', [('motor', 'wrong'), ('confirmed', False), ('effort', 'wrong'),
                                        ('model', 'wrong'), ('harnessAccount', 'wrong'), ('motorAccount', 'wrong')])
def test_verification_requires_full_observed_configuration(boundary, field, value):
    boundary.configure_source()
    adapter = boundary.adapter()
    expected = dict(boundary.state, to='claude', unchanged=True)
    boundary.state[field] = value
    assert adapter._verify(expected, 'original-session') is None


@pytest.mark.parametrize('field,value', [('model', 'claude-sonnet-5'), ('effort', 'low'), ('conversationId', 'other')])
def test_waiting_draft_rejects_changed_source_configuration(boundary, field, value):
    boundary.configure_source()
    adapter = boundary.adapter(effort='medium')
    adapter.prepare()
    boundary.state[field] = value
    with pytest.raises(ValueError, match='cambi'):
        adapter.snapshot(adapter.plan)
    assert 'exit' not in boundary.events


@pytest.mark.parametrize('harness', ['claude', 'codex', 'grok'])
def test_account_switch_preserves_exact_transcript_and_config(boundary, harness):
    boundary.configure_source(harness)
    result, adapter = boundary.run(harnessAccount='work', motorAccount='work', effort='low')
    assert result['ok'], result
    assert result['observed']['conversationId'] == 'original-session'
    assert boundary.transcript(harness, 'work', 'original-session').read_text() == 'original history\n'


def test_launch_failure_rolls_back_observed_original_configuration(boundary):
    boundary.configure_source()
    before = dict(boundary.state)
    boundary.fail_launch = True
    result, _ = boundary.run(effort='low')
    assert result['rolledBack'] is True, result
    for field in ('harness', 'motor', 'model', 'effort', 'harnessAccount', 'motorAccount', 'conversationId'):
        assert result['observed'][field] == before[field]


def test_same_configuration_and_wait_idle_use_real_adapter(boundary):
    boundary.configure_source()
    result, _ = boundary.run()
    assert result['unchanged'] is True
    assert not boundary.events


def test_wait_idle_rechecks_process_and_then_applies(boundary):
    boundary.configure_source()
    boundary.busy = [True, True, False]
    result, _ = boundary.run(effort='low')
    assert result['ok'], result
    assert not boundary.busy and 'exit' in boundary.events


def test_unconfirmed_owned_cli_waits_durably_and_later_confirms(boundary):
    boundary.configure_source()
    boundary.screen = 'initializing, no prompt yet'
    result, adapter = boundary.run(effort='low')
    assert result['pending'] and not result['confirmed'] and result['recoveryAllowed']
    row = boundary.store.get('matrix-request')
    assert row['state'] == 'awaiting_confirmation' and row['snapshot']['origin']['resume_id'] == 'original-session'
    with pytest.raises(Exception, match='operación pendiente'):
        boundary.store.claim('other-request', row['pane_key'], {})
    boundary.store.recover_abandoned()
    assert boundary.store.get('matrix-request')['state'] == 'awaiting_confirmation'
    events = list(boundary.events)
    boundary.screen = '> ready'
    row = boundary.dash.refresh_session_confirmation(boundary.store, row)
    assert row['state'] == 'confirmed' and row['result']['observed']['effort'] == 'low'
    assert boundary.events == events


def test_pending_poll_leaves_unchanged_row_and_records_confirmation_once(boundary, monkeypatch):
    boundary.configure_source()
    boundary.screen = 'initializing, no prompt yet'
    result, _ = boundary.run(effort='low')
    assert result['pending']
    row = boundary.store.get('matrix-request')
    updated = row['updated']
    recorded = []
    monkeypatch.setattr(boundary.dash, 'record_runtime_config', lambda *args: recorded.append(args))
    for _ in range(3):
        row = boundary.dash.refresh_session_confirmation(boundary.store, row)
        assert row['updated'] == updated
    assert not recorded
    boundary.screen = '> ready'
    stale_row = row
    row = boundary.dash.refresh_session_confirmation(boundary.store, row)
    assert row['state'] == 'confirmed'
    boundary.dash.refresh_session_confirmation(boundary.store, stale_row)
    boundary.dash.refresh_session_confirmation(boundary.store, row)
    assert len(recorded) == 1 and recorded[0][4:6] == (boundary.state['model'], 'low')


def test_pending_startup_without_conversation_can_be_explicitly_recovered(boundary, monkeypatch):
    boundary.configure_source()
    original_send = boundary.send
    first = True
    def launch_without_metadata(pane, command):
        nonlocal first
        original_send(pane, command)
        if first:
            boundary.state.update(conversationId='', confirmed=False)
            first = False
    monkeypatch.setattr(boundary.dash, '_send_shell_line', launch_without_metadata)
    result, adapter = boundary.run(toHarness='codex', motor='codex', model='gpt-5.5', effort='high')
    assert result['pending'] and result['handoffRequired'] and result['continuity'] == 'new-conversation'
    assert Path(result['handoffPath']).is_file()
    monkeypatch.setattr(boundary.dash, 'motor_stage', lambda *args, **kwargs: None)
    monkeypatch.setattr(boundary.dash, 'motor_result_set', lambda *args, **kwargs: None)
    monkeypatch.setattr(boundary.dash, '_stop_owned_startup', lambda pid, started: boundary.exit('%1', pid, 'codex', started))
    monkeypatch.setattr(boundary.dash, 'threading', SimpleNamespace(Thread=lambda **kwargs: SimpleNamespace(start=kwargs['target'])))
    status, _ = boundary.dash.session_recover({'operationId': 'matrix-request'})
    assert status == 202
    row = boundary.store.get('matrix-request')
    assert row['state'] == 'rolled_back' and row['result']['observed']['conversationId'] == 'original-session'


def test_wait_idle_timeout_keeps_source(boundary):
    boundary.configure_source()
    boundary.busy = [True] * 1350
    result, _ = boundary.run(effort='low')
    assert result['ok'] is False and '45 minutos' in result['error']
    assert not boundary.events


def test_unobserved_source_effort_cannot_be_guessed_for_rollback(boundary):
    boundary.configure_source()
    boundary.state['effort'] = ''
    result, _ = boundary.run(effort='low')
    assert result['ok'] is False and 'exit' not in boundary.events


def test_reused_process_id_never_receives_exit_keys(boundary, monkeypatch):
    events = []
    monkeypatch.setattr(boundary.dash, 'tmux', lambda *args: events.append(args))
    assert boundary.exit_current('%1', 123, 'claude', 'old-start') is False
    assert not events


def test_process_appearing_after_source_exit_never_receives_launch_text(boundary, monkeypatch):
    boundary.configure_source()
    def user_started_process(*args):
        boundary.events.append('exit')
        boundary.pid += 1
        boundary.state = dict(boundary.state, harness='codex')
        boundary.marker = 'unrelated-user-process'
        boundary.identity['pane_current_command'] = 'codex'
        return True
    monkeypatch.setattr(boundary.dash, '_pane_exit_current', user_started_process)
    result, _ = boundary.run(effort='low')
    assert result['recoveryRequired'] is True
    assert boundary.events == ['exit']


def test_account_copy_rejects_symlink_parent_without_writing_outside_account(boundary):
    boundary.configure_source()
    outside = boundary.root / 'outside'; outside.mkdir()
    projects = boundary.root / 'claude/work/projects'; projects.mkdir()
    (projects / 'repo').symlink_to(outside, target_is_directory=True)
    result, _ = boundary.run(harnessAccount='work', motorAccount='work', effort='low')
    assert result['ok'] is False
    assert not list(outside.iterdir())
    assert 'exit' not in boundary.events


def test_interrupted_account_copy_preserves_existing_history(boundary, monkeypatch):
    boundary.configure_source()
    source = boundary.transcript('claude', 'main', 'original-session')
    target = boundary.transcript('claude', 'work', 'original-session')
    source.write_text('old\nnew\n'); target.write_text('old\n')
    def interrupted(src, dst):
        Path(dst).write_text('partial')
        raise OSError('copy interrupted')
    monkeypatch.setattr(boundary.dash.shutil, 'copy2', interrupted)
    result, _ = boundary.run(harnessAccount='work', motorAccount='work')
    assert result['ok'] is False and target.read_text() == 'old\n'
    assert 'exit' not in boundary.events
    assert not list(target.parent.glob('.comandos-copy-*'))


MODELS_AND_EFFORTS = [(route, model['id'], effort) for route in sorted(RECOVERABLE)
                     for model in REGISTRY['motors'][route.split(':')[1]]['models']
                     for effort in (model['efforts'] or [''])]


@pytest.mark.parametrize('route,model,effort', MODELS_AND_EFFORTS,
                         ids=[f'{r}/{m}/{e}' for r, m, e in MODELS_AND_EFFORTS])
def test_every_catalog_model_and_effort_on_recoverable_routes(boundary, route, model, effort):
    boundary.configure_source()
    harness, motor = route.split(':')
    result, _ = boundary.run(toHarness=harness, motor=motor, model=model, effort=effort)
    assert result['ok'], result
    assert result['observed']['model'] == model and result['observed']['effort'] == effort


@pytest.mark.parametrize('harness,motor', CELLS)
@pytest.mark.parametrize('invalid', ['model', 'effort', 'account'])
def test_every_route_rejects_invalid_draft_before_exit(boundary, harness, motor, invalid):
    boundary.configure_source()
    model = REGISTRY['motors'][motor]['models'][0]
    data = dict(toHarness=harness, motor=motor, model=model['id'], effort=model['defaultEffort'])
    data[{'model': 'model', 'effort': 'effort', 'account': 'harnessAccount'}[invalid]] = 'not-a-valid-selection'
    result, _ = boundary.run(**data)
    assert result['ok'] is False, result
    assert 'exit' not in boundary.events


def test_acp_existing_session_needs_observed_effort_options_before_restart(boundary):
    boundary.configure_source('acp', 'claude')
    boundary.state['effortSource'] = 'acp-config-options'
    result, _ = boundary.run(effort='low')
    assert result['ok'], result
    assert result['observed']['conversationId'] == 'original-session'


def test_configure_endpoint_binds_retries_and_concurrent_requests(boundary, monkeypatch):
    boundary.configure_source()
    workers = []
    monkeypatch.setattr(boundary.dash, 'threading', SimpleNamespace(Thread=lambda **kwargs: SimpleNamespace(start=lambda: workers.append(kwargs))))
    monkeypatch.setattr(boundary.dash, 'motor_stage', lambda *args, **kwargs: None)
    data = dict(session='audit', pane='%1', requestId='endpoint-request', effort='low')
    first = boundary.dash.session_configure(data)
    replay = boundary.dash.session_configure(data)
    concurrent = boundary.dash.session_configure(dict(data, requestId='other-request'))
    changed = boundary.dash.session_configure(dict(data, effort='high'))
    assert first[0] == replay[0] == 202
    assert first[1]['operationId'] == replay[1]['operationId'] == data['requestId']
    assert concurrent[0] == changed[0] == 409
    assert len(workers) == 1 and not boundary.events


@pytest.mark.parametrize('matching_pid', [True, False])
def test_acp_observation_uses_matching_protocol_state_not_launch_request(boundary, monkeypatch, matching_pid):
    boundary.configure_source('acp', 'claude')
    monkeypatch.setattr(boundary.dash, '_proc_cmdline', lambda pid: ['cc-acp', '--model', 'requested-model', '--effort', 'max'])
    monkeypatch.setattr(boundary.dash, 'acp_state_for_pane', lambda pane: {
        'pid': boundary.pid if matching_pid else boundary.pid + 1, 'sessionId': 'original-session',
        'agent': 'claude', 'account': 'main', 'model': 'requested-model', 'effort': 'max',
        'observedModel': 'claude-sonnet-5', 'observedEffort': '', 'effortSource': 'unconfirmed'})
    observed = boundary.observe('audit', '%1', boundary.info(), lambda pane: {
        'agent': 'acp', 'acp': {'sessionId': 'original-session'}})
    assert observed['effort'] == ''
    assert observed['model'] == ('claude-sonnet-5' if matching_pid else '')
    assert observed['confirmed'] is matching_pid


def test_return_to_saved_harness_restores_exact_id_and_permissions(boundary):
    boundary.configure_source('codex')
    prior = dict(agent='claude', resume_id='saved-claude', claude_config_dir=str(boundary.root / 'claude/main'),
                 flags=['--permission-mode', 'plan'], observed={'conversationId': 'saved-claude', 'harnessAccount': 'main'})
    boundary.transcript('claude', 'main', 'saved-claude').write_text('saved history')
    boundary.store.claim('prior-operation', boundary.dash._identity_key(boundary.identity), {})
    boundary.store.stage('prior-operation', 'confirmed', snapshot={'origin': prior})
    result, adapter = boundary.run(toHarness='claude', motor='claude', model='claude-sonnet-5', effort='low')
    assert result['ok'], result
    assert result['observed']['conversationId'] == 'saved-claude'
    assert '--permission-mode plan' in adapter.plan['command']


@pytest.mark.parametrize('dialog', ['Accessing workspace: /tmp\n> Yes, I trust this folder',
                                    '> Do you trust this workspace?', '> sign in required'])
def test_trust_or_login_dialog_never_confirms_or_receives_input(boundary, dialog):
    boundary.configure_source()
    boundary.screen = dialog
    result, _ = boundary.run()
    assert result['ok'] is False
    assert not boundary.events
