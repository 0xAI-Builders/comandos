"""Read-only recommendations from durable, confirmed operations. No live tmux."""
import json
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
from session_operations import OperationStore
from test_agent_launch import load_dash_module

IDENTITY = dict(socket_path='/tmp/tmux-private', pid='12', server_start='100',
                session_id='$1', session_name='project', pane_id='%2', pane_pid='123',
                pane_current_path='/private/project')
KEY = '|'.join(IDENTITY[k] for k in ('socket_path', 'pid', 'server_start', 'session_id', 'pane_id', 'pane_pid'))


def observed(model='new-model', **extra):
    return dict(harness='codex', motor='codex', model=model, effort='high',
                harnessAccount='main', motorAccount='main', confirmed=True,
                conversationId='private-conversation', transcriptPath='/private/transcript', **extra)


def record(store, ident='op1', model='new-model', at=100, state='confirmed', cwd='/private/project',
           request=None, result=None, identity=None, origin_observed=None):
    identity = identity or IDENTITY
    key = '|'.join(identity[k] for k in ('socket_path', 'pid', 'server_start', 'session_id', 'pane_id', 'pane_pid'))
    store.claim(ident, key, dict(session='project', pane='%2', **(request or {})))
    store.stage(ident, state, snapshot={'origin': {'cwd': cwd, 'identity': identity,
                'observed': observed('old-model') if origin_observed is None else origin_observed}},
                result=result if result is not None else {'ok': True, 'observed': observed(model)})
    with store.connect() as db:
        db.execute('UPDATE session_operations SET updated=? WHERE id=?', (at, ident))


def history(store, key=KEY):
    return OperationStore.config_history(store.path, '/private/project', key)


def test_groups_project_usage_and_returns_only_config_metadata(tmp_path):
    store = OperationStore(tmp_path / 'operations.db')
    record(store, 'a', at=10)
    record(store, 'b', at=20)
    record(store, 'c', model='other-model', at=30)
    record(store, 'foreign', cwd='/another/project', at=40)
    result = history(store)
    assert [(i['config']['model'], i['count'], i['lastUsed']) for i in result['items']] == [
        ('new-model', 2, 20), ('other-model', 1, 30)]
    assert result['previous']['config']['model'] == 'old-model'
    assert result['previous']['lastUsed'] == 30
    assert result['scope'] == 'project' and result['provenance'] == 'confirmed-operations'
    assert set(result['items'][0]['config']) == {'toHarness', 'motor', 'model', 'effort', 'harnessAccount', 'motorAccount'}
    assert 'private' not in json.dumps(result)


@pytest.mark.parametrize('change', [
    {'state': 'failed'}, {'state': 'rolled_back'}, {'state': 'awaiting_confirmation'},
    {'request': {'extensionsOnly': True}},
    {'result': {'ok': True, 'unchanged': True, 'observed': observed()}},
    {'result': {'ok': False, 'observed': observed()}},
    {'result': {'ok': True, 'observed': dict(observed(), confirmed=False)}},
    {'result': {'ok': True, 'observed': dict(observed(), motorAccount='unknown')}},
    {'result': {'ok': True, 'observed': dict(observed(), model={'secret': 'value'})}},
    {'result': {'ok': True, 'observed': ['malformed']}},
])
def test_excludes_non_configuration_or_unconfirmed_operations(tmp_path, change):
    store = OperationStore(tmp_path / 'operations.db')
    record(store, **change)
    assert history(store) == dict(items=[], previous=None, scope='project', provenance='confirmed-operations')


def test_previous_never_crosses_pane_or_server_generation(tmp_path):
    store = OperationStore(tmp_path / 'operations.db')
    record(store)
    assert history(store, KEY + '-reused')['previous'] is None
    with store.connect() as db:
        snap = store.get('op1')['snapshot']
        snap['origin']['identity']['server_start'] = 'other-server'
        db.execute('UPDATE session_operations SET snapshot=?', (json.dumps(snap),))
    assert history(store)['previous'] is None


def test_latest_invalid_origin_does_not_offer_older_previous(tmp_path):
    store = OperationStore(tmp_path / 'operations.db')
    record(store, 'a')
    record(store, 'b', at=200, origin_observed={})
    assert history(store)['previous'] is None


def test_orders_ties_and_limits_groups_to_twenty(tmp_path):
    store = OperationStore(tmp_path / 'operations.db')
    for n in reversed(range(25)):
        record(store, str(n), model=f'model-{n:02}', at=50)
    assert [i['config']['model'] for i in history(store)['items']] == [f'model-{n:02}' for n in range(20)]


def test_missing_database_is_empty_and_not_created(tmp_path):
    path = tmp_path / 'missing.db'
    assert OperationStore.config_history(path, '/private/project', KEY)['items'] == []
    assert not path.exists()


def handler(dash, path):
    h = object.__new__(dash.Handler)
    h.path = path
    h._json = lambda status, body: (status, body)
    return h


def test_endpoint_uses_live_cwd_and_is_read_only(tmp_path, monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(dash, 'HOOKS', str(tmp_path))
    store = OperationStore(tmp_path / 'session-operations.sqlite3')
    record(store)
    before = Path(store.path).read_bytes()
    calls = []
    monkeypatch.setattr(dash, '_pane_identity', lambda s, p: (calls.append((s, p)), IDENTITY)[1])
    monkeypatch.setattr(dash, 'session_operation_store', lambda: pytest.fail('GET must not recover abandoned operations'))
    code, body = handler(dash, '/session-config-history?session=project&pane=%252&cwd=/another/project')._do_GET()
    assert code == 200 and body['items'][0]['config']['model'] == 'new-model'
    assert calls == [('project', '%2')]
    assert Path(store.path).read_bytes() == before


@pytest.mark.parametrize('query', ['', '?session=project', '?session=project&pane=2',
                                  '?session=project&pane=%252&pane=%253'])
def test_endpoint_rejects_missing_invalid_or_ambiguous_targets(monkeypatch, query):
    dash = load_dash_module()
    monkeypatch.setattr(dash, '_pane_identity', lambda *a: pytest.fail('invalid query reached tmux'))
    assert handler(dash, '/session-config-history' + query)._do_GET()[0] == 400


def test_endpoint_does_not_disclose_identity_error_and_requires_auth(monkeypatch):
    dash = load_dash_module()
    def gone(*args):
        raise ValueError('/private/secret')
    monkeypatch.setattr(dash, '_pane_identity', gone)
    code, body = handler(dash, '/session-config-history?session=project&pane=%252')._do_GET()
    assert code == 400 and '/private' not in json.dumps(body)
    h = handler(dash, '/session-config-history?session=project&pane=%252')
    h._header_values = lambda name: ['localhost:7777']
    h._security_gate = lambda: (401, {'error': 'unauthorized'})
    h._do_GET = lambda: pytest.fail('unauthenticated handler reached history')
    assert h.do_GET()[0] == 401


@pytest.mark.parametrize('column,raw', [('snapshot', '{broken'), ('result', '{broken'),
                                        ('snapshot', '[]'), ('result', 'null')])
def test_malformed_record_does_not_hide_valid_history(tmp_path, column, raw):
    store = OperationStore(tmp_path / 'operations.db')
    record(store, 'valid')
    record(store, 'broken', at=200)
    with store.connect() as db:
        db.execute(f'UPDATE session_operations SET {column}=? WHERE id=?', (raw, 'broken'))
    assert history(store)['items'][0]['count'] == 1


@pytest.mark.parametrize('field,value', [('harnessAccount', '/secret/auth.json'),
                                        ('model', 'model\nsecret'), ('effort', 'high token=secret')])
def test_identifier_sanitization_rejects_paths_and_unbounded_text(tmp_path, field, value):
    store = OperationStore(tmp_path / 'operations.db')
    record(store, result={'ok': True, 'observed': dict(observed(), **{field: value})})
    assert history(store)['items'] == []


def test_models_without_effort_remain_reusable(tmp_path):
    store = OperationStore(tmp_path / 'operations.db')
    obs = observed()
    obs.pop('effort')
    record(store, result={'ok': True, 'observed': obs})
    assert history(store)['items'][0]['config']['effort'] == ''
