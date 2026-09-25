import json
from pathlib import Path
import shlex
from types import SimpleNamespace

import pytest
from test_agent_launch import load_dash_module


@pytest.fixture
def boundary(tmp_path,monkeypatch):
    monkeypatch.setenv('HOME',str(tmp_path))
    dash=load_dash_module()
    identity=dict(socket_path='fixture',pid='1',server_start='2',session_id='$1',session_name='test',pane_id='%1',pane_pid='99',pane_current_command='bash',pane_current_path=str(tmp_path))
    monkeypatch.setattr(dash,'_pane_identity',lambda s,p:dict(identity) if (s,p)==('test','%1') else (_ for _ in ()).throw(ValueError('target')))
    monkeypatch.setattr(dash,'agent_info_for_pane',lambda p:{})
    monkeypatch.setattr(dash,'HOOKS',str(tmp_path/'hooks'))
    store=dash.session_operations.OperationStore(tmp_path/'ops.sqlite')
    monkeypatch.setattr(dash,'session_operation_store',lambda:store)
    inv={'harness':'codex','mcps':[dict(id='docs',name='docs',enabled=True,toggleable=True,reason='')], 'skills':[dict(id='shared:design',name='design',enabled=True,toggleable=True,reason='')], 'limitations':[]}
    monkeypatch.setattr(dash.extension_launch,'inventory',lambda *args:inv)
    monkeypatch.setattr(dash,'harness_pane_busy',lambda *args:False)
    return dash,identity,store,inv


def payload(state):
    return dict(session='test',pane='%1',harness='codex',expectedIdentity=state['identity'],expectedConversationId=state['conversationId'],revision=state['revision'])


def test_draft_save_never_claims_loaded_and_is_cas_guarded(boundary):
    dash,identity,store,inv=boundary
    state=dash.pane_extensions_state({'session':'test','pane':'%1','harness':'codex'})
    assert state['loaded'] is None and state['desired']['mcps']=={'docs':True}
    data={**payload(state),'desired':{'mcps':{'docs':False},'skills':{'shared:design':True}}}
    code,saved=dash.pane_extensions_write('',data)
    assert code==200 and saved['revision']==1 and saved['loaded'] is None
    assert dash.pane_extensions_write('',data)[0]==409
    assert dash.pane_extensions_write('',{**data,'expectedIdentity':'wrong'})[0]==409


def test_apply_claims_existing_coordinator_before_rechecking_revision(boundary,monkeypatch):
    dash,identity,store,inv=boundary
    state=dash.pane_extensions_state({'session':'test','pane':'%1','harness':'codex'})
    order=[]
    real_claim=store.claim
    monkeypatch.setattr(store,'claim',lambda *a:(order.append('claim'),real_claim(*a))[1])
    real_revision=dash.pane_extension_state.ExtensionStore.require_revision
    monkeypatch.setattr(dash.pane_extension_state.ExtensionStore,'require_revision',lambda self,*a:(order.append('revision'),real_revision(self,*a))[1])
    monkeypatch.setattr(dash.threading,'Thread',lambda **kw:SimpleNamespace(start=lambda:None))
    code,result=dash.pane_extensions_write('/apply',{**payload(state),'requestId':'request-123','interrupt':False})
    assert code==202 and order.index('claim')<order.index('revision')
    assert store.get('request-123')['request']['extensionDraftKey']
    assert dash.pane_extensions_write('',{**payload(state),'desired':state['desired']})[0]==409


def test_templates_use_names_and_report_missing_without_applying(boundary):
    dash,identity,store,inv=boundary
    state=dash.pane_extensions_state({'session':'test','pane':'%1','harness':'codex'})
    code,result=dash.pane_extensions_write('/template',{**payload(state),'name':'Design'})
    assert code==200 and result['template']['selection']['skills']=={'design':True}
    inv['skills']=[]
    code,result=dash.pane_extensions_write('/template',{**payload(state),'templateId':result['template']['id']})
    assert code==200 and result['missing']['skills']==['design']
    assert result['loaded'] is None


def test_unknown_fields_and_wrong_pane_operation_are_rejected(boundary):
    dash,identity,store,inv=boundary
    state=dash.pane_extensions_state({'session':'test','pane':'%1','harness':'codex'})
    assert dash.pane_extensions_write('/apply',{**payload(state),'command':'rm x'})[0]==400
    store.claim('other-operation','other-identity',{'session':'other','pane':'%2'})
    code,result=dash.pane_extensions_write('/cancel',{**payload(state),'operationId':'other-operation'})
    assert code==409
    assert store.get('other-operation')['state']=='validating'


@pytest.mark.parametrize('harness,flag',[('claude','--resume'),('codex','resume'),('grok','--resume'),('opencode','--session'),('agy','--conversation')])
def test_configuration_command_resumes_exact_native_session(boundary,monkeypatch,harness,flag):
    dash,*_=boundary
    monkeypatch.setattr(dash.provider_registry,'which',lambda p:p)
    monkeypatch.setattr(dash,'_harness_bin',lambda h:h)
    monkeypatch.setattr(dash.account_registry,'account_environment',lambda *a:{})
    command=dash._configuration_command(harness,harness,'','','main','exact-session')
    args=shlex.split(command)
    assert args[args.index(flag)+1]=='exact-session'
    assert '--continue' not in args


def test_completed_apply_retry_is_idempotent_after_pane_disappears(boundary,monkeypatch):
    dash,identity,store,inv=boundary
    state=dash.pane_extensions_state({'session':'test','pane':'%1','harness':'codex'})
    monkeypatch.setattr(dash.threading,'Thread',lambda **kw:SimpleNamespace(start=lambda:None))
    data={**payload(state),'requestId':'repeat-1234','interrupt':False}
    assert dash.pane_extensions_write('/apply',data)[0]==202
    store.stage(data['requestId'],'confirmed',result={'ok':True})
    monkeypatch.setattr(dash,'_pane_identity',lambda *a:(_ for _ in ()).throw(ValueError('gone')))
    code,result=dash.pane_extensions_write('/apply',data)
    assert code==200 and result['state']=='confirmed'


def test_recovery_state_remains_readable_with_unknown_failed_process(boundary,monkeypatch):
    dash,identity,store,inv=boundary
    state=dash.pane_extensions_state({'session':'test','pane':'%1','harness':'codex'})
    target=dash._extension_target({'session':'test','pane':'%1','harness':'codex'})
    request={**payload(state),'requestId':'failed-1234','extensionsOnly':True,'extensionDraftKey':target['draft']['key']}
    store.claim(request['requestId'],state['identity'],request)
    store.stage(request['requestId'],'recovery_required',snapshot={'origin':{'identity':identity,'observed':{}},'destination':{'to':'codex','harnessAccount':'main'}},result={'recoveryRequired':True})
    identity['pane_current_command']='unidentified-startup'
    result=dash.pane_extensions_state({'session':'test','pane':'%1','harness':'codex'})
    assert result['operation']['recoveryAllowed'] and result['loaded'] is None
    assert result['desired']==state['desired']
    assert not result['applySupported']


def test_new_inventory_entry_stays_off_in_existing_draft(boundary):
    dash,identity,store,inv=boundary
    original=dash.pane_extensions_state({'session':'test','pane':'%1','harness':'codex'})
    inv['mcps'].append(dict(id='new-server',name='new-server',enabled=True,toggleable=True))
    state=dash.pane_extensions_state({'session':'test','pane':'%1','harness':'codex'})
    assert state['desired']['mcps']['new-server'] is False
    target=dash._extension_target({'session':'test','pane':'%1','harness':'codex'})
    assert target['draft']['desired']['mcps']['new-server'] is False
    assert state['revision']==original['revision']


def test_template_keeps_project_skill_separate_from_personal_name(boundary):
    dash,identity,store,inv=boundary
    inv['skills'].append(dict(id='skill:project',name='design',enabled=False,toggleable=True,scope='project'))
    state=dash.pane_extensions_state({'session':'test','pane':'%1','harness':'codex'})
    code,result=dash.pane_extensions_write('/template',{**payload(state),'name':'Scoped'})
    assert code==200
    assert result['template']['selection']['skills']=={'design':True,'project:design':False}
