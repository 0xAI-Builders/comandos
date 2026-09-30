"""Operator catalog requests exercised through the shipped tool dispatcher.

The chat handlers that drove it were retired in S4; their tests left with them."""
import ast
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))
import operator_catalog
import operator_dispatch


def load_functions(*names):
    tree = ast.parse((ROOT / 'bin/cc-dash').read_text())
    defs = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name in names]
    assert {node.name for node in defs} == set(names)
    ns = dict(json=json, operator_catalog=operator_catalog)
    exec(compile(ast.Module(body=defs, type_ignores=[]), 'cc-dash', 'exec'), ns)
    return ns


def dispatcher(tmp_path, get, **options):
    return operator_dispatch.Dispatcher(base_url='http://unused', token='', hooks_dir=str(tmp_path),
        local_handlers={}, http_get=get, default_session='local', default_pane='%1', **options)


def test_session_status_filters_exact_live_pane_before_summarizing(tmp_path):
    rows = [dict(session='neighbor', pane='%9', alive=True, detail='x' * 3000),
            dict(session='local', pane='%1', alive=False, model='old'),
            dict(session='local', pane='%1', alive=True, model='gpt-fixture', status='waiting'),
            dict(session='local', pane='%2', alive=True, model='sibling')]
    out = dispatcher(tmp_path, lambda path, query: rows).run('session_status', {})
    assert out['ok']
    assert out['data']['sessions'] == [rows[2]]
    assert 'gpt-fixture' in out['reply'] and 'neighbor' not in out['reply']


def test_extension_usage_explicit_all_scope_does_not_reapply_defaults(tmp_path):
    calls = []
    d = dispatcher(tmp_path, lambda path, query: calls.append((path, query)) or {'groups': []})
    d.run('extension_usage', {})
    d.run('extension_usage', {'scope': 'all', 'days': 7})
    assert calls == [('/extension-usage', {'session': 'local', 'pane': '%1'}),
                     ('/extension-usage', {'days': 7})]


def test_explicit_other_tab_never_inherits_selected_pane(tmp_path):
    seen = []
    d = dispatcher(tmp_path, lambda *args: {})
    d.local_handlers['send'] = lambda args: seen.append(args) or {'ok': True}
    d.run('send_text', {'tab': 'Signara', 'text': 'hello'})
    assert seen == [{'tab': 'Signara', 'text': 'hello'}]


def test_session_brain_resolves_identity_instead_of_mixing_cwd_and_pane(tmp_path):
    calls = []
    def get(path, query):
        calls.append((path, query))
        if path == '/state':
            return [dict(session='local', pane='%1', alive=True, cwd='/tmp/local', agent='codex', account='work')]
        return {'mcps': []}
    d = dispatcher(tmp_path, get)
    out = d.run('session_brain', {})
    assert out['ok']
    assert calls[-1] == ('/session-brain', {'session': 'local', 'pane': '%1', 'cwd': '/tmp/local', 'harness': 'codex', 'account': 'work'})
    calls.clear()
    out = d.run('session_brain', {'cwd': '/tmp/other'})
    assert not out['ok'] and not any(path == '/session-brain' for path, _ in calls)


def test_missing_usage_is_unavailable_not_zero_or_neighbor_usage(tmp_path):
    d = dispatcher(tmp_path, lambda *args: {'panes': [dict(session='neighbor', pane='%9', tokens=999)],
                                          'providers': [{'tokens': 999}], 'ts': 123})
    out = d.run('session_usage', {})
    assert out['ok']
    assert out['data']['panes'] == []
    assert out['data']['attribution'] == 'unavailable'
    assert '999' not in out['reply']


def test_large_analytics_result_is_valid_json_with_explicit_truncation(tmp_path):
    out = dispatcher(tmp_path, lambda *args: {'configurations': [dict(model=f'model-{i}', notes='x'*3000) for i in range(200)]}).run('usage_analytics', {})
    result = json.loads(out['reply'])
    assert result['truncated'] is True
    assert len(out['reply']) <= 16000


def test_analysis_request_cannot_apply_a_recommendation(tmp_path):
    calls = []
    d = dispatcher(tmp_path, lambda *args: {}, readonly=True)
    d._post = lambda *args: calls.append(args) or {'ok': True}
    out = d.run('model_switch', {'model':'cheap'})
    assert not out['ok'] and calls == []


def test_pending_operation_exposes_reference_to_the_model(tmp_path):
    d = dispatcher(tmp_path, lambda *args: {})
    d._post = lambda *args: {'ok':True, 'operationKey':'local|%1', 'operationId':'op123'}
    result = d.run('model_switch', {'model':'haiku'})
    evidence = json.loads(result['reply'])
    assert evidence['pending'] is True
    assert evidence['data']['operationKey'] == 'local|%1'
    assert evidence['data']['operationId'] == 'op123'


def test_session_usage_retains_actual_tmux_attribution_fields(tmp_path):
    row = {'tmux_session':'local', 'tmux_pane':'%1', 'total_tokens':42,
           'confidence':'compartido', 'usage_window':'24h', 'pane_pwd':'/tmp/shared'}
    out = dispatcher(tmp_path, lambda *args: {'generated_at':123, 'panes':[row]}).run('session_usage', {})
    assert out['data']['panes'] == [row]
    assert out['data']['generated_at'] == 123
    assert 'compartido' in out['reply']


def test_explicit_session_scope_includes_sibling_panes(tmp_path):
    rows = [dict(session='local',pane='%1',alive=True), dict(session='local',pane='%2',alive=True)]
    d = dispatcher(tmp_path, lambda *args: rows)
    out = d.run('session_status', {'scope':'session'})
    assert out['data']['sessions'] == rows
    assert out['data']['scope']['pane'] is None


def test_profile_inventory_uses_selected_provider_and_account(tmp_path):
    calls = []
    def get(path, query):
        calls.append((path,query))
        if path == '/state':
            return [dict(session='local',pane='%1',alive=True,cwd='/tmp/local',agent='claude',harnessAccount='work')]
        return {'profiles':[], 'inventory':{}}
    out = dispatcher(tmp_path, get).run('list_session_profiles', {})
    assert out['ok']
    assert calls[-1] == ('/session-profiles', {'cwd':'/tmp/local','harness':'claude','account':'work'})


def test_operator_tab_inventory_includes_local_once():
    ns = load_functions('operator_tabs_payload')
    ns.update(HIDDEN_SESSIONS={'local','hub'}, tab_labels=lambda:{'local':'Local','hub':'Hub','signara':'Signara'})
    assert ns['operator_tabs_payload']() == [{'session':'local','label':'Local'}, {'session':'signara','label':'Signara'}]
