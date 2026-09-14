"""Provider inventory must be scoped by verified live pane, never by a hint."""
import pytest
from test_agent_launch import load_dash_module


def setup(monkeypatch):
    dash = load_dash_module()
    reg = {'harnesses': {'codex': {'capabilities': {'accounts': True}},
                         'gemini': {'capabilities': {'accounts': False}}}}
    monkeypatch.setattr(dash, 'load_provider_registry', lambda: reg)
    monkeypatch.setattr(dash, '_pane_identity', lambda session, pane:
        {'pane_current_path':'/tmp'} if (session,pane)==('local','%0') else
        (_ for _ in ()).throw(ValueError('el panel no pertenece a esa sesión')))
    monkeypatch.setattr(dash, 'agent_info_for_pane', lambda pane: {'agent':'codex','pid':123})
    monkeypatch.setattr(dash, 'account_for_pid', lambda *args: {'account':'work'})
    monkeypatch.setattr(dash.account_registry, 'list_accounts', lambda *args: [])
    return dash


def test_real_pane_wins_over_stale_cwd_provider_and_account(monkeypatch):
    dash = setup(monkeypatch)
    calls = []
    def inventory(reg, harness, alias, cwd):
        calls.append((harness,alias,cwd))
        return {'skills':[{'name':'disabled','enabled':False}], 'mcps':[], 'capabilities':{}}
    monkeypatch.setattr(dash.session_profile_store, 'inventory', inventory)
    result = dash.session_brain_snapshot({'session':'local','pane':'%0','cwd':'/other',
                                          'harness':'claude','account':'main'})
    assert calls == [('codex','work','/tmp')]
    assert result['skills'][0]['enabled'] is False
    assert result['harness'] == 'codex' and result['account'] == 'work'


def test_session_pane_membership_mismatch_cannot_read_other_project(monkeypatch):
    dash = setup(monkeypatch)
    with pytest.raises(ValueError, match='no pertenece'):
        dash.session_brain_snapshot({'session':'signara','pane':'%0'})


def test_shell_never_inherits_claude_extensions(monkeypatch):
    dash = setup(monkeypatch)
    monkeypatch.setattr(dash, 'agent_info_for_pane', lambda pane: None)
    result = dash.session_brain_snapshot({'session':'local','pane':'%0','harness':'claude'})
    assert result['harness'] == 'shell'
    assert result['skills'] == result['mcps'] == []
    assert result['status'] == 'not_applicable'


def test_unknown_process_account_does_not_read_main(monkeypatch):
    dash = setup(monkeypatch)
    monkeypatch.setattr(dash, 'account_for_pid', lambda *args: {'account':'unknown'})
    monkeypatch.setattr(dash.session_profile_store, 'inventory', lambda *args: pytest.fail('must not guess'))
    result = dash.session_brain_snapshot({'session':'local','pane':'%0','account':'main'})
    assert result['status'] == 'unknown' and result['account'] == 'unknown'
    assert result['limitations']


def test_provider_without_accounts_still_has_inventory(monkeypatch):
    dash = setup(monkeypatch)
    monkeypatch.setattr(dash, 'agent_info_for_pane', lambda pane: {'agent':'gemini','pid':123})
    monkeypatch.setattr(dash.account_registry, 'list_accounts', lambda *args: pytest.fail('no account support'))
    monkeypatch.setattr(dash.session_profile_store, 'inventory', lambda reg,h,a,c:
        {'skills':[{'name':h,'enabled':True}], 'mcps':[], 'capabilities':{}})
    result = dash.session_brain_snapshot({'session':'local','pane':'%0'})
    assert result['harness'] == 'gemini' and result['skills'][0]['name'] == 'gemini'


def test_explicit_unsupported_harness_is_not_claude(monkeypatch):
    dash = setup(monkeypatch)
    with pytest.raises(ValueError, match='harness'):
        dash.session_brain_snapshot({'harness':'typo','cwd':'/tmp'})


def test_acp_uses_verified_underlying_provider_and_account(monkeypatch):
    dash = setup(monkeypatch)
    monkeypatch.setattr(dash, 'agent_info_for_pane', lambda pane: {'agent':'acp','pid':123})
    monkeypatch.setattr(dash, 'acp_state_for_pane', lambda pane: {'pid':123,'agent':'codex','account':'work'})
    monkeypatch.setattr(dash.session_profile_store, 'inventory', lambda reg,h,a,c:
        {'skills':[{'name':h+'-'+a,'enabled':True}], 'mcps':[], 'capabilities':{}})
    result = dash.session_brain_snapshot({'session':'local','pane':'%0'})
    assert result['harness'] == 'acp' and result['extensionProvider'] == 'codex'
    assert result['skills'][0]['name'] == 'codex-work'


def test_acp_stale_process_metadata_cannot_supply_extensions(monkeypatch):
    dash = setup(monkeypatch)
    monkeypatch.setattr(dash, 'agent_info_for_pane', lambda pane: {'agent':'acp','pid':123})
    monkeypatch.setattr(dash, 'acp_state_for_pane', lambda pane: {'pid':999,'agent':'codex','account':'work'})
    result = dash.session_brain_snapshot({'session':'local','pane':'%0'})
    assert result['status'] == 'unknown' and result['skills'] == []


@pytest.mark.parametrize('harness',['claude','codex','grok','opencode','gemini','agy'])
def test_api_inventory_preserves_each_provider_and_instruction_count(monkeypatch,harness):
    dash=setup(monkeypatch)
    monkeypatch.setattr(dash,'load_provider_registry',lambda:{'harnesses':{harness:{'capabilities':{'accounts':False}}}})
    monkeypatch.setattr(dash,'agent_info_for_pane',lambda pane:{'agent':harness,'pid':123})
    monkeypatch.setattr(dash.session_profile_store,'inventory',lambda *args:{'skills':[],'mcps':[],'status':'empty'})
    monkeypatch.setattr(dash,'_claude_md_lines',lambda cwd:('CLAUDE.md',17))
    result=dash.session_brain_snapshot({'session':'local','pane':'%0'})
    assert result['harness']==harness and result['status']=='empty'
    if harness=='claude':
        assert result['claudeMd']['lines']==17


def test_operation_status_is_bound_to_request_after_restart(monkeypatch):
    from types import SimpleNamespace
    dash = load_dash_module()
    old = {'id':'older','request':{'session':'local','pane':'%0'},'state':'confirmed',
           'result':{'ok':True,'observed':{'model':'previous'}},'updated':1}
    new = {**old, 'id':'newer','state':'waiting','result':None,'updated':2}
    monkeypatch.setattr(dash, 'session_operation_store', lambda: SimpleNamespace(
        recover_abandoned=lambda:None, get=lambda ident:old if ident=='older' else new,
        latest_for_target=lambda *args:new))
    assert dash.session_operation_status('local|%0','older')['model'] == 'previous'
    assert dash.session_operation_status('local|%0','newer')['stage'] == 'waiting'
    assert dash.session_operation_status('local|%0','newer')['ts'] == 2
    with pytest.raises(ValueError, match='no pertenece'):
        dash.session_operation_status('signara|%21','older')


def test_awaiting_status_does_not_report_success_until_refreshed_evidence(monkeypatch):
    from types import SimpleNamespace
    dash=load_dash_module()
    row={'id':'pending','request':{'session':'local','pane':'%0'},'state':'awaiting_confirmation',
         'result':{'ok':True,'pending':True,'confirmed':False,'recoveryAllowed':True},'updated':1}
    monkeypatch.setattr(dash,'session_operation_store',lambda:SimpleNamespace(
        recover_abandoned=lambda:None,get=lambda ident:row))
    monkeypatch.setattr(dash,'refresh_session_confirmation',lambda store,record:record)
    result=dash.session_operation_status('local|%0','pending')
    assert result['stage']=='awaiting_confirmation' and result['recoveryAllowed']
    assert 'ok' not in result and result['confirmed'] is False
    confirmed={**row,'state':'confirmed','result':{'ok':True,'observed':{'model':'verified'}}}
    monkeypatch.setattr(dash,'refresh_session_confirmation',lambda store,record:confirmed)
    result=dash.session_operation_status('local|%0','pending')
    assert result['ok'] and result['model']=='verified' and 'stage' not in result
