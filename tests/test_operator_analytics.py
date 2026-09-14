"""Operator requests exercised through the shipped handlers and tool dispatcher."""
import ast
import json
import sys
from pathlib import Path
from types import SimpleNamespace

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))
import operator_catalog
import operator_chat
import operator_dispatch
import operator_stream
import operator_tools


def load_functions(*names):
    tree = ast.parse((ROOT / 'bin/cc-dash').read_text())
    defs = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name in names]
    assert {node.name for node in defs} == set(names)
    ns = dict(json=json, operator_chat=operator_chat, operator_catalog=operator_catalog,
              operator_tools=operator_tools, operator_stream=operator_stream)
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


def test_nonstream_handler_uses_request_context_not_desktop(tmp_path):
    ns = load_functions('operator_handle_chat')
    ns.update(operator_store_root=lambda: tmp_path, operator_tabs_payload=lambda: [],
              operator_active_session=lambda: 'neighbor',
              _operator_request_context=lambda data: (data['session'], data['pane']))
    targets = []
    ns['operator_build_dispatcher'] = lambda *args: targets.append(args) or object()
    ns['operator_agent_turn'] = lambda *args, **kwargs: {'reply': kwargs['active'], 'actions': []}
    out = ns['operator_handle_chat']({'text': 'estado', 'session': 'local', 'pane': '%1'})
    assert out['reply'] == 'local'
    assert targets == [('local', '%1')]


def test_provider_error_cannot_finish_as_success_or_execute_queued_tools():
    ns = load_functions('operator_agent_stream', '_llm_poison', '_run_tool_batch')
    ns['operator_provider_stream'] = lambda *args, **kwargs: ('anthropic', iter([
        {'t': 'tool_call', 'id': '1', 'name': 'model_switch', 'input': {}},
        {'t': 'error', 'text': 'provider overloaded'}, {'t': 'done', 'stop': None}]))
    calls = []
    events = list(ns['operator_agent_stream']('status', model='haiku', tabs=[], memory='', active='local',
        convo={}, dispatcher=SimpleNamespace(run=lambda *args: calls.append(args))))
    assert calls == []
    assert events[-1]['t'] == 'final' and events[-1]['ok'] is False
    assert events[-1]['reply'] != 'Listo.'


def test_malformed_tool_json_does_not_become_empty_default_target_command():
    events = [dict(type='content_block_start', index=0, content_block={'type':'tool_use', 'id':'x','name':'model_switch'}),
              dict(type='content_block_delta', index=0, delta={'type':'input_json_delta','partial_json':'{"model":'}),
              dict(type='content_block_stop', index=0), dict(type='message_delta',delta={'stop_reason':'tool_use'})]
    result = list(operator_stream.stream_anthropic(iter(events)))
    assert not any(event['t'] == 'tool_call' for event in result)
    assert any(event['t'] == 'error' for event in result)


def test_prompt_includes_exact_context_and_evidence_requirements():
    text = operator_tools.agent_system_prompt([], '', 'local', context={'session':'local','pane':'%1','cwd':'/tmp/local'})
    assert '%1' in text and '/tmp/local' in text
    assert 'session_status' in text and 'session_usage' in text and 'extension_usage' in text
    assert 'estim' in text.lower() and 'recomend' in text.lower()


def test_analysis_request_cannot_apply_a_recommendation(tmp_path):
    calls = []
    d = dispatcher(tmp_path, lambda *args: {}, readonly=True)
    d._post = lambda *args: calls.append(args) or {'ok': True}
    out = d.run('model_switch', {'model':'cheap'})
    assert not out['ok'] and calls == []


@pytest.mark.parametrize('text', [
    '¿Qué recomiendas para cambiar de modelo?', 'No cambies nada, analiza el consumo',
    '¿Qué skills y MCPs usa esta sesión?', '¿Cuántos tokens llevo hoy?',
    '¿Cómo van mis sesiones?', 'How much did this session cost?',
])
def test_analysis_questions_are_readonly(text):
    assert operator_tools.analysis_request(text)


@pytest.mark.parametrize('text', ['Aplica el perfil ahorro', 'Cambia el modelo a haiku',
    'Abre estadísticas', 'Envíale a Local: analiza el consumo', 'Guarda mi cuota'])
def test_explicit_actions_remain_actionable(text):
    assert not operator_tools.analysis_request(text)


@pytest.mark.parametrize('stream', [False, True])
def test_invalid_selected_pane_is_rejected_before_inference_or_persistence(tmp_path, stream):
    handler = 'operator_handle_chat_stream' if stream else 'operator_handle_chat'
    ns = load_functions(handler, '_operator_request_context')
    calls = []
    def identity(session, pane):
        calls.append((session, pane))
        raise ValueError('panel de otra sesión')
    ns.update(_pane_identity=identity, operator_active_session=lambda: 'neighbor',
              operator_store_root=lambda: pytest.fail('invalid context must not create chat state'))
    args = [{'text':'estado', 'session':'local', 'pane':'%9'}]
    if stream:
        args.append(lambda chunk: pytest.fail('invalid context must not stream'))
    with pytest.raises(ValueError, match='otra sesión'):
        ns[handler](*args)
    assert calls == [('local', '%9')]


def test_analytics_conversation_uses_actual_dispatch_and_keeps_evidence(tmp_path):
    ns = load_functions('operator_agent_stream', '_llm_poison', '_run_tool_batch')
    calls, prompts = [], []
    def get(path, query):
        calls.append((path, query))
        if path == '/state':
            return [dict(session='local', pane='%1', alive=True, status='waiting')]
        return {'scope':query, 'extensions':[], 'tokens':None, 'costUsd':None,
                'coverage':'Solo eventos capturados; ausencia no demuestra ausencia de uso.'}
    d = dispatcher(tmp_path, get)
    rounds = iter([
        [{'t':'tool_call','id':'1','name':'session_status','input':{}},
         {'t':'tool_call','id':'2','name':'extension_usage','input':{}}, {'t':'done','stop':'tool_use'}],
        [{'t':'delta','text':'Local está esperando. No hay atribución de tokens a skills.'}, {'t':'done','stop':'end_turn'}],
    ])
    def provider(model, prompt, conv, **kwargs):
        prompts.append((prompt, json.loads(json.dumps(conv))))
        return 'anthropic', iter(next(rounds))
    ns['operator_provider_stream'] = provider
    result = list(ns['operator_agent_stream']('Estado y uso de skills', model='haiku', tabs=[], memory='', active='local', convo={}, dispatcher=d))
    assert sorted(calls) == sorted([('/state', {}), ('/extension-usage', {'session':'local','pane':'%1'})])
    evidence = prompts[1][1][-1]['content']
    assert json.loads(evidence[0]['content'])['sessions'][0]['status'] == 'waiting'
    assert json.loads(evidence[1]['content'])['tokens'] is None
    assert d.readonly and result[-1]['reply'].startswith('Local')


@pytest.mark.parametrize('family', ['anthropic', 'openai'])
def test_incomplete_provider_response_emits_error(family):
    if family == 'anthropic':
        events = [{'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':'partial'}}]
        read = operator_stream.stream_anthropic
    else:
        events = [{'choices':[{'delta':{'tool_calls':[{'index':0,'id':'x','function':{'name':'model_switch','arguments':'{}'}}]}}]}]
        read = operator_stream.stream_openai
    result = list(read(iter(events)))
    assert any(event['t'] == 'error' for event in result)
    assert not any(event['t'] == 'tool_call' for event in result)


def test_payload_converts_tool_history_when_provider_family_changes():
    ns = load_functions('operator_build_payload')
    ns.update(OPERATOR_CREDS=SimpleNamespace(claude=lambda:'test', grok=lambda:'test'),
              load_proxy_cfg=lambda:{'port':1})
    history = [{'role':'assistant','content':[{'type':'tool_use','id':'one','name':'session_status','input':{}}]},
               {'role':'user','content':[{'type':'tool_result','tool_use_id':'one','content':'{"sessions":[]}'}]}]
    _, grok, _, _ = ns['operator_build_payload']('grok-4.5', 'SYS', history)
    assert grok['messages'][1]['tool_calls'][0]['id'] == 'one'
    assert grok['messages'][2] == {'role':'tool','tool_call_id':'one','content':'{"sessions":[]}'}
    _, claude, _, _ = ns['operator_build_payload']('haiku', 'SYS', grok['messages'][1:])
    assert claude['messages'] == history


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


@pytest.mark.parametrize('endpoint', ['/operator/chat', '/operator/chat/stream'])
def test_http_invalid_context_returns_400_before_sse_headers(endpoint):
    import io
    tree = ast.parse((ROOT / 'bin/cc-dash').read_text())
    method = next(node for node in ast.walk(tree) if isinstance(node, ast.FunctionDef) and node.name == 'do_POST')
    ns = {'json':json}
    def reject(data):
        raise ValueError('panel no pertenece a local')
    ns['_operator_request_context'] = reject
    ns['operator_handle_chat'] = reject
    exec(compile(ast.Module(body=[method], type_ignores=[]), 'cc-dash', 'exec'), ns)
    body = json.dumps({'text':'estado', 'session':'local', 'pane':'%9'}).encode()
    request = SimpleNamespace(path=endpoint, headers={'Content-Length':str(len(body))}, rfile=io.BytesIO(body),
        _security_gate=lambda:None, _json=lambda status, payload, **kwargs:(status,payload),
        send_response=lambda *args:pytest.fail('must validate before SSE success headers'))
    status, payload = ns['do_POST'](request)
    assert status == 400 and 'local' in payload['error']


@pytest.mark.parametrize('family', ['anthropic', 'openai'])
def test_token_limit_is_reported_as_incomplete(family):
    if family == 'anthropic':
        result = list(operator_stream.stream_anthropic(iter([{'type':'message_delta','delta':{'stop_reason':'max_tokens'}}])))
    else:
        result = list(operator_stream.stream_openai(iter([{'choices':[{'delta':{},'finish_reason':'length'}]}])))
    assert any(event['t'] == 'error' for event in result)


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


def test_provider_message_conversion_does_not_mutate_previous_history():
    history = [{'role':'user','content':[{'type':'tool_result','tool_use_id':'one','content':'1'}]},
               {'role':'tool','tool_call_id':'two','content':'2'}]
    original = json.loads(json.dumps(history))
    operator_tools.provider_messages(history, 'anthropic')
    assert history == original


@pytest.mark.parametrize('query', ['local', 'Local', 'actual'])
def test_selected_local_cannot_resolve_to_another_tab(query):
    ns = load_functions('_op_tab')
    session, error = ns['_op_tab']({'tab':query}, [{'session':'local-neighbor','label':'Local neighbor'}], 'local')
    assert session == 'local' and error is None


def test_operator_tab_inventory_includes_local_once():
    ns = load_functions('operator_tabs_payload')
    ns.update(HIDDEN_SESSIONS={'local','hub'}, tab_labels=lambda:{'local':'Local','hub':'Hub','signara':'Signara'})
    assert ns['operator_tabs_payload']() == [{'session':'local','label':'Local'}, {'session':'signara','label':'Signara'}]
