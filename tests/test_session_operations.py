"""Configuration recovery tests use fake adapters; no user's tmux is contacted."""
import json
from pathlib import Path
import sys
from concurrent.futures import ThreadPoolExecutor

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
from session_operations import OperationConflict, OperationStore, run_operation
from test_agent_launch import load_dash_module


class Adapter:
    def __init__(self, store, failure=None):
        self.store, self.failure, self.calls = store, failure, []

    def step(self, name):
        self.calls.append(name)
        if self.failure == name:
            raise RuntimeError('simulated ' + name)

    def prepare(self):
        self.step('prepare')
        return {'command': 'codex resume exact-conversation-id'}

    def wait_idle(self):
        self.step('wait')

    def check_identity(self):
        self.step('identity')

    def snapshot(self, plan):
        self.step('snapshot')
        return {'origin': {'resume_id': 'exact-original', 'resume_command': 'claude --resume exact-original'},
                'layout': {'windows': ['untouched split']}}

    def apply(self, plan, snapshot):
        row = self.store.get('request-1234')
        assert row['state'] == 'applying'
        assert row['snapshot'] == snapshot
        self.step('apply')

    def verify(self, plan, snapshot):
        self.step('verify')
        return {'harness': 'codex', 'model': 'gpt-6-astra', 'effort': 'ultra'}

    def rollback(self, snapshot):
        assert snapshot['origin']['resume_id'] == 'exact-original'
        self.step('rollback')


def operation(tmp_path, failure=None):
    store = OperationStore(tmp_path / 'operations.db')
    store.claim('request-1234', 'server-start|%2', {'session': 'test', 'pane': '%2'})
    adapter = Adapter(store, failure)
    return store, adapter


def test_snapshot_is_durable_before_apply_and_survives_confirmation(tmp_path):
    store, adapter = operation(tmp_path)
    result = run_operation(store, 'request-1234', adapter)
    assert result['ok'] is True
    reopened = OperationStore(store.path)
    assert reopened.get('request-1234')['snapshot']['origin']['resume_id'] == 'exact-original'
    assert reopened.get('request-1234')['state'] == 'confirmed'


@pytest.mark.parametrize('failure', ['prepare', 'wait', 'snapshot', 'identity'])
def test_preflight_failure_never_closes_source(tmp_path, failure):
    store, adapter = operation(tmp_path, failure)
    assert run_operation(store, 'request-1234', adapter)['ok'] is False
    assert 'apply' not in adapter.calls and 'rollback' not in adapter.calls
    assert store.get('request-1234')['state'] == 'failed'


@pytest.mark.parametrize('failure', ['apply', 'verify'])
def test_failure_after_exit_restores_exact_source(tmp_path, failure):
    store, adapter = operation(tmp_path, failure)
    result = run_operation(store, 'request-1234', adapter)
    assert result['ok'] is False and result['rolledBack'] is True
    assert store.get('request-1234')['state'] == 'rolled_back'
    assert adapter.calls[-1] == 'rollback'


def test_failed_recovery_keeps_lock_and_snapshot(tmp_path):
    store, adapter = operation(tmp_path, 'apply')
    def fail(snapshot):
        raise RuntimeError('origin also failed')
    adapter.rollback = fail
    result = run_operation(store, 'request-1234', adapter)
    assert result['recoveryRequired'] is True
    assert store.get('request-1234')['snapshot']
    with pytest.raises(OperationConflict):
        store.claim('another-request', 'server-start|%2', {})
    assert store.claim_recovery('request-1234') is True
    assert store.claim_recovery('request-1234') is False


def test_same_request_is_idempotent_and_changed_draft_is_rejected(tmp_path):
    store, adapter = operation(tmp_path)
    assert not store.claim('request-1234', 'different-server', {'session': 'test', 'pane': '%2'})
    with pytest.raises(OperationConflict):
        store.claim('request-1234', 'server-start|%2', {'model': 'another'})
    run_operation(store, 'request-1234', adapter)
    assert not store.claim('request-1234', 'server-start|%2', {'session': 'test', 'pane': '%2'})


def test_concurrent_pane_requests_only_one_claim_succeeds(tmp_path):
    store = OperationStore(tmp_path / 'operations.db')
    def claim(index):
        try:
            return store.claim(f'request-{index}', 'generation|%1', {'index': index})
        except OperationConflict:
            return False
    with ThreadPoolExecutor(max_workers=8) as pool:
        assert sum(pool.map(claim, range(8))) == 1
    assert store.claim('other-pane', 'generation|%2', {})
    assert store.claim('other-server', 'new-generation|%1', {})


def test_dashboard_crash_does_not_replay_destructive_step(tmp_path, monkeypatch):
    import session_operations
    store, adapter = operation(tmp_path)
    store.stage('request-1234', 'applying', snapshot=adapter.snapshot({}))
    def dead(pid, signal):
        raise ProcessLookupError()
    monkeypatch.setattr(session_operations.os, 'kill', dead)
    OperationStore(store.path).recover_abandoned()
    row = store.get('request-1234')
    assert row['state'] == 'recovery_required'
    assert row['snapshot']['origin']['resume_id'] == 'exact-original'
    assert 'apply' not in adapter.calls


def test_status_transport_error_after_commit_never_rolls_back(tmp_path):
    store, adapter = operation(tmp_path)
    def broken_status(stage, result):
        raise BrokenPipeError()
    result = run_operation(store, 'request-1234', adapter, broken_status)
    assert result['ok'] is True
    assert 'rollback' not in adapter.calls
    assert store.get('request-1234')['state'] == 'confirmed'


@pytest.mark.parametrize('effort', ['low', 'xhigh', 'max', 'ultra'])
def test_codex_status_accepts_each_supported_effort(monkeypatch, effort):
    from types import SimpleNamespace
    dash = load_dash_module()
    monkeypatch.setattr(dash, 'tmux', lambda *args: SimpleNamespace(returncode=0, stdout=f'gpt-6-astra {effort} · account'))
    assert dash.codex_pane_model('%9') == ('gpt-6-astra', effort)


def test_card_does_not_inherit_session_or_another_process_configuration(monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(dash.cc_usage, 'latest_session_config', lambda *args: {'harness': 'claude', 'model': 'old-model', 'effort': 'high'})
    def observe(sess, pane, info):
        return {'harness': 'codex', 'pid': 9, 'conversationId': 'root-9', 'model': 'gpt-6-astra', 'effort': 'ultra',
                'motor': 'codex', 'harnessAccount': 'personal', 'motorAccount': 'personal', 'source': 'conversation', 'confirmed': True}
    monkeypatch.setattr(dash, 'observe_pane', observe)
    item = {'session': 'mixed', 'pane': '%9', 'alive': True, 'model': 'claude-old', 'effort': 'low', 'account': 'work'}
    result = dash.reconcile_card_config(item, {'pid': 9, 'agent': 'codex'})
    assert (result['model'], result['effort'], result['account']) == ('gpt-6-astra', 'ultra', 'personal')
    assert result['agentSessionId'] == 'root-9'
    monkeypatch.setattr(dash, 'observe_pane', lambda *args: {'confirmed': False, 'source': 'unconfirmed'})
    unknown = dash.reconcile_card_config({'session': 'mixed', 'pane': '%10', 'alive': True})
    assert unknown['model'] == '' and unknown['effort'] == ''
    assert unknown['lastConfirmedConfig']['model'] == 'old-model'


def test_complete_codex_launch_applies_account_model_effort_and_exact_resume_once(monkeypatch):
    import shlex
    dash = load_dash_module()
    monkeypatch.setattr(dash, '_harness_bin', lambda name: '/mock/' + name)
    monkeypatch.setattr(dash.provider_registry, 'which', lambda name: name)
    monkeypatch.setattr(dash, 'load_provider_registry', lambda: {'harnesses': {'codex': {'capabilities': {'accounts': True}}}})
    monkeypatch.setattr(dash.account_registry, 'account_environment', lambda registry, provider, account: {'CODEX_HOME': '/accounts/' + account})
    command = dash._configuration_command('codex', 'codex', 'gpt-6-astra', 'ultra', 'work', 'exact-id',
        ['--model', 'old-model', '--effort', 'low', '--dangerously-bypass-approvals-and-sandbox'])
    args = shlex.split(command)
    assert args.count('resume') == 1 and 'exact-id' in args
    assert args.count('-m') == 1 and args.count('-c') == 1
    assert 'CODEX_HOME=/accounts/work' in args
    assert 'old-model' not in args and '--last' not in args
    assert '--dangerously-bypass-approvals-and-sandbox' in args


def test_retry_after_pane_disappears_returns_durable_result_without_tmux(tmp_path, monkeypatch):
    dash = load_dash_module()
    store = OperationStore(tmp_path / 'ops.db')
    data = {'session': 'test', 'pane': '%1', 'requestId': 'request-1234'}
    store.claim('request-1234', 'old-server|%1', data)
    store.stage('request-1234', 'confirmed', result={'ok': True})
    monkeypatch.setattr(dash, 'session_operation_store', lambda: store)
    def forbidden(*args):
        pytest.fail('retry must not inspect or operate live tmux')
    monkeypatch.setattr(dash, '_pane_identity', forbidden)
    status, result = dash.session_configure(data)
    assert status == 200 and result['ok'] is True
    assert result['pending'] is False


def test_cancel_before_apply_prevents_terminal_input_even_after_snapshot(tmp_path):
    store, adapter = operation(tmp_path)
    original_check = adapter.check_identity
    def cancel_at_second_check():
        original_check()
        if adapter.calls.count('identity') == 2:
            assert store.cancel_waiting('request-1234')
    adapter.check_identity = cancel_at_second_check
    result = run_operation(store, 'request-1234', adapter)
    assert result['ok'] is False
    assert 'apply' not in adapter.calls


def test_same_configuration_verifies_without_restart_or_snapshot(tmp_path):
    store, adapter = operation(tmp_path)
    adapter.prepare = lambda: {'unchanged': True}
    result = run_operation(store, 'request-1234', adapter)
    assert result['unchanged'] is True
    assert 'apply' not in adapter.calls and 'snapshot' not in adapter.calls


def test_account_copy_refuses_divergent_subagent_before_changing_root(tmp_path, monkeypatch):
    dash = load_dash_module()
    source = tmp_path / 'source'; target = tmp_path / 'target'
    sid = 'exact-session'
    src_project = source / 'projects' / 'same-project'; src_project.mkdir(parents=True)
    dst_project = target / 'projects' / 'same-project'; dst_project.mkdir(parents=True)
    (src_project / (sid + '.jsonl')).write_text('original\nnew source event\n')
    (dst_project / (sid + '.jsonl')).write_text('original\n')
    (src_project / sid / 'subagents').mkdir(parents=True)
    (dst_project / sid / 'subagents').mkdir(parents=True)
    (src_project / sid / 'subagents' / 'agent-a.jsonl').write_text('source subagent\n')
    other = dst_project / sid / 'subagents' / 'agent-a.jsonl'
    other.write_text('destination unique work\n')
    monkeypatch.setattr(dash, 'load_provider_registry', lambda: {})
    monkeypatch.setattr(dash.account_registry, 'account_environment', lambda *args: {'CLAUDE_CONFIG_DIR': str(target)})
    adapter = object.__new__(dash.SessionConfiguration)
    adapter.frm = 'claude'
    with pytest.raises(ValueError, match='cambios propios'):
        adapter._copy_conversation({'agent': 'claude', 'resume_id': sid, 'claude_config_dir': str(source)}, 'work')
    assert other.read_text() == 'destination unique work\n'
    assert (dst_project / (sid + '.jsonl')).read_text() == 'original\n'


def test_recovery_never_closes_later_user_process_of_same_cli(monkeypatch):
    dash = load_dash_module()
    adapter = object.__new__(dash.SessionConfiguration)
    adapter.sess, adapter.pane, adapter.frm = 'test', '%1', 'claude'
    adapter.data = {'requestId': 'request-1234'}
    adapter.original = {'pid': 10}
    adapter.identity = {'pane_current_command': 'codex'}
    adapter.plan = {'to': 'codex'}
    monkeypatch.setattr(dash, '_pane_identity', lambda *args: adapter.identity)
    monkeypatch.setattr(dash, 'agent_info_for_pane', lambda pane: {'pid': 999, 'agent': 'codex'})
    monkeypatch.setattr(dash, '_read_environ', lambda pid: {})
    monkeypatch.setattr(dash, '_pane_exit_current', lambda *args: pytest.fail('must not close user process'))
    snapshot = {'origin': {'observed': {'conversationId': 'original'}, 'agent_start': 'start'}}
    with pytest.raises(RuntimeError, match='otro proceso'):
        adapter.rollback(snapshot)


def test_snapshot_lookup_restores_same_pane_generation_only(tmp_path):
    store, adapter = operation(tmp_path)
    snapshot = adapter.snapshot({})
    snapshot['origin'].update(agent='claude', observed={'conversationId': 'exact-original'})
    store.stage('request-1234', 'confirmed', snapshot=snapshot, result={'ok': True})
    assert store.saved_origin('server-start|%2', 'claude')['resume_id'] == 'exact-original'
    assert store.saved_origin('another-start|%2', 'claude') is None


def test_missing_exact_transcript_rejected_instead_of_latest_fallback(tmp_path):
    dash = load_dash_module()
    root = tmp_path / 'codex'; (root / 'sessions').mkdir(parents=True)
    (root / 'sessions' / 'rollout-other-id.jsonl').write_text('other conversation')
    with pytest.raises(ValueError, match='historial'):
        dash._snapshot_transcript({'agent': 'codex', 'resume_id': 'requested-id', 'codex_home': str(root)})


def test_claude_transcript_lookup_uses_pinned_account(tmp_path):
    dash = load_dash_module()
    cfg = tmp_path / 'work'; (cfg / 'projects' / 'project').mkdir(parents=True)
    path = cfg / 'projects' / 'project' / 'exact-id.jsonl'
    path.write_text('actual work account conversation')
    assert dash._snapshot_transcript({'agent': 'claude', 'resume_id': 'exact-id', 'claude_config_dir': str(cfg)}) == str(path)


def test_crash_before_destination_pin_cannot_close_new_user_conversation(monkeypatch):
    dash = load_dash_module()
    adapter = object.__new__(dash.SessionConfiguration)
    adapter.sess, adapter.pane, adapter.frm = 'test', '%1', 'claude'
    adapter.data = {'requestId': 'request-1234'}
    adapter.original = {'pid': 10}
    adapter.identity = {'pane_current_command': 'codex'}
    adapter.plan = {'to': 'codex'}
    monkeypatch.setattr(dash, '_pane_identity', lambda *args: adapter.identity)
    monkeypatch.setattr(dash, 'agent_info_for_pane', lambda pane: {'pid': 999, 'agent': 'codex'})
    monkeypatch.setattr(dash, '_read_environ', lambda pid: {b'COMANDOS_OPERATION_ID': b'request-1234'})
    monkeypatch.setattr(dash, '_pane_exit_current', lambda *args: pytest.fail('no pinned target conversation'))
    snapshot = {'origin': {'observed': {'conversationId': 'original'}, 'agent_start': 'start'}}
    with pytest.raises(RuntimeError, match='no se guardó la conversación'):
        adapter.rollback(snapshot)


def test_explicit_incompatible_account_is_not_silently_replaced(monkeypatch):
    dash = load_dash_module()
    adapter = object.__new__(dash.SessionConfiguration)
    adapter.frm, adapter.sess, adapter.pane = 'claude', 'test', '%1'
    adapter.original = {'pid': 42}
    adapter.identity = {}
    adapter.data = {'toHarness': 'acp', 'motor': 'claude', 'harnessAccount': 'work', 'motorAccount': 'work'}
    monkeypatch.setattr(dash, 'observe_pane', lambda *args: {'motor': 'claude', 'conversationId': 'original'})
    monkeypatch.setattr(dash, 'load_provider_registry', lambda: {})
    monkeypatch.setattr(dash.provider_registry, 'route_for', lambda *args: {})
    monkeypatch.setattr(dash.provider_registry, 'engine_for_model', lambda *args: '')
    monkeypatch.setattr(dash, 'resolve_route_selection', lambda *args: ({'id': 'acp:claude'},
        {'model': 'model', 'effort': 'high', 'harnessAccount': 'main', 'motorAccount': 'work'}))
    monkeypatch.setattr(dash, '_configuration_command', lambda *args, **kwargs: pytest.fail('invalid account must stop before launch construction'))
    with pytest.raises(ValueError, match='harnessAccount'):
        adapter.prepare()


def test_operation_poll_and_saved_origin_use_indexes(tmp_path):
    store = OperationStore(tmp_path / 'ops.db')
    with store.connect() as db:
        target_plan = db.execute("EXPLAIN QUERY PLAN SELECT id FROM session_operations "
            "WHERE json_extract(request,'$.session')=? AND json_extract(request,'$.pane')=? ORDER BY updated DESC LIMIT 1",
            ('test', '%1')).fetchall()
        origin_plan = db.execute('EXPLAIN QUERY PLAN SELECT snapshot FROM session_operations WHERE pane_key=? '
            'AND snapshot IS NOT NULL ORDER BY updated DESC', ('server|%1',)).fetchall()
    assert any('session_operations_target_updated' in row['detail'] for row in target_plan)
    assert any('session_operations_pane_updated' in row['detail'] for row in origin_plan)
