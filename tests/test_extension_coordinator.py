import json
import shlex
from pathlib import Path

import pytest
from test_session_route_matrix import Boundary
from session_operations import run_operation


@pytest.fixture
def rig(tmp_path,monkeypatch):
    b=Boundary(tmp_path,monkeypatch);d=b.dash
    monkeypatch.setattr(d,'HOOKS',str(tmp_path/'hooks'))
    inv={'mcps':[dict(id='docs',name='docs',enabled=True,toggleable=True)],'skills':[]}
    monkeypatch.setattr(d.extension_launch,'inventory',lambda *a:inv)
    monkeypatch.setattr(d.pane_snapshot,'PaneInspector',lambda:lambda pane:b.origin())
    transcript=tmp_path/'transcript';transcript.write_text('fixture')
    monkeypatch.setattr(d,'_snapshot_transcript',lambda origin:str(transcript) if origin.get('agent')!='shell' else '')
    monkeypatch.setattr(d.extension_launch,'prepare_launch',lambda *a:{'selection':a[4],'harness':a[1],'operationId':a[6],'manifest':'fixture-private'})
    monkeypatch.setattr(d.extension_launch,'wrap_command',lambda cmd,launch:cmd)
    monkeypatch.setattr(d.extension_launch,'launch_from_pid',lambda pid:None)
    monkeypatch.setattr(d.extension_launch,'verify_launch',lambda pid,launch:True)
    old_send=b.send
    def send(pane,command):
        old_send(pane,command)
        if b.state:
            args=shlex.split(command)
            for flag in ('--session','--conversation'):
                if flag in args:b.state['conversationId']=args[args.index(flag)+1]
    monkeypatch.setattr(d,'_send_shell_line',send)
    return b


def adapter(b,harness='codex',interrupt=False):
    b.configure_source(harness)
    b.state.update(model='',effort='',confirmed=False,source='unconfirmed')
    d=b.dash
    state=d.pane_extensions_state({'session':'audit','pane':'%1'})
    desired={'mcps':{'docs':False},'skills':{}}
    target=d._extension_target({'session':'audit','pane':'%1'})
    row=d._extension_store().save(target['draft']['key'],0,desired)
    request=dict(session='audit',pane='%1',harness=harness,requestId='extensions-request',extensionsOnly=True,
        extensionDraftKey=row['key'],revision=row['revision'],expectedIdentity=state['identity'],expectedConversationId=state['conversationId'],interrupt=interrupt)
    a=d.PaneExtensionConfiguration(request,dict(b.identity));a.store=b.store
    b.store.claim(request['requestId'],d._identity_key(b.identity),request)
    return a


@pytest.mark.parametrize('harness',['claude','codex','grok','opencode','agy'])
def test_exact_resume_without_inventing_unknown_model(rig,harness):
    b=rig;a=adapter(b,harness)
    result=run_operation(b.store,a.data['requestId'],a)
    assert result['ok'],result
    row=b.store.get(a.data['requestId'])
    assert row['state']=='confirmed'
    assert row['snapshot']['destination']['extensionLaunch']['selection']['mcps']=={'docs':False}
    assert row['snapshot']['origin']['resume_id']=='original-session'
    commands=[x for x in b.events if 'original-session' in x]
    assert commands and all('--model' not in command and ' -m ' not in command for command in commands)
    assert 'layout' in row['snapshot']


def test_namespace_preflight_failure_does_not_close_original(rig,monkeypatch):
    b=rig;a=adapter(b,'grok')
    monkeypatch.setattr(b.dash.extension_launch,'prepare_launch',lambda *a:(_ for _ in ()).throw(ValueError('namespace unavailable')))
    result=run_operation(b.store,a.data['requestId'],a)
    assert not result['ok'] and b.events==[]
    assert b.state['conversationId']=='original-session'


def test_changed_conversation_during_wait_cannot_close_target(rig,monkeypatch):
    b=rig;a=adapter(b)
    monkeypatch.setattr(a,'wait_idle',lambda:b.state.update(conversationId='other-session'))
    result=run_operation(b.store,a.data['requestId'],a)
    assert not result['ok'] and 'exit' not in b.events


def test_failed_destination_recovers_exact_origin_without_loaded_claim(rig,monkeypatch):
    b=rig;a=adapter(b)
    original_pid=b.pid
    monkeypatch.setattr(b.dash.extension_launch,'verify_launch',lambda pid,launch:False)
    monkeypatch.setattr(a,'pending_confirmation',lambda *a:None)
    result=run_operation(b.store,a.data['requestId'],a)
    assert result.get('rolledBack'),result
    assert b.state['conversationId']=='original-session'
    assert b.store.get(a.data['requestId'])['state']=='rolled_back'


def test_model_switch_pins_and_restores_original_extension_bundle(rig,monkeypatch):
    b=rig;d=b.dash;b.configure_source('codex')
    original={'selection':{'mcps':{'docs':False},'skills':{}},'harness':'codex','manifest':'old-private','operationId':'old-op'}
    monkeypatch.setattr(d.extension_launch,'launch_from_pid',lambda pid:original)
    wraps=[]
    monkeypatch.setattr(d.extension_launch,'wrap_command',lambda cmd,bundle:(wraps.append(bundle['operationId']),cmd)[1])
    a=b.adapter(toHarness='codex',motor='codex',model='gpt-6-astra',effort='low')
    plan=a.prepare()
    assert plan['extensionLaunch']['selection']==original['selection']
    snapshot=a.snapshot(plan)
    assert snapshot['origin']['extensionLaunch']==original
    assert 'old-op' in wraps and a.data['requestId'] in wraps


def test_unchanged_selection_verifies_running_bundle_without_restarting(rig,monkeypatch):
    b=rig;a=adapter(b)
    original={'selection':{'mcps':{'docs':False},'skills':{}},'harness':'codex','manifest':'old-private','operationId':'old-op'}
    monkeypatch.setattr(b.dash.extension_launch,'launch_from_pid',lambda pid:original)
    monkeypatch.setattr(b.dash.extension_launch,'verify_launch',lambda pid,launch:launch==original)
    result=run_operation(b.store,a.data['requestId'],a)
    assert result.get('unchanged') and result['ok'],result
    assert b.events==[]


def test_shell_launch_without_native_session_id_remains_recoverable(rig,monkeypatch):
    b=rig;d=b.dash;b.configure_source('shell')
    state=d.pane_extensions_state({'session':'audit','pane':'%1','harness':'codex'})
    target=d._extension_target({'session':'audit','pane':'%1','harness':'codex'})
    request=dict(session='audit',pane='%1',harness='codex',requestId='shell-extension',extensionsOnly=True,
        extensionDraftKey=target['draft']['key'],revision=0,expectedIdentity=state['identity'],expectedConversationId='')
    a=d.PaneExtensionConfiguration(request,dict(b.identity));a.store=b.store
    b.store.claim(request['requestId'],state['identity'],request)
    send=d._send_shell_line
    monkeypatch.setattr(d,'_send_shell_line',lambda p,c:(send(p,c),b.state.update(conversationId='')))
    result=run_operation(b.store,request['requestId'],a)
    assert result.get('pending') and result.get('recoveryAllowed'),result
    assert b.store.get(request['requestId'])['state']=='awaiting_confirmation'


def test_foreign_draft_key_cannot_reconfigure_this_pane(rig):
    b=rig;a=adapter(b)
    foreign=b.dash._extension_store().state('other-pane','original-session','codex',{'mcps':{'docs':True},'skills':{}})
    a.data.update(extensionDraftKey=foreign['key'],revision=foreign['revision'])
    result=run_operation(b.store,a.data['requestId'],a)
    assert not result['ok'] and b.events==[]


def test_review_queued_model_change_is_rejected_before_exit(rig):
    b=rig;a=adapter(b)
    b.state.update(model='gpt-5.5',effort='high',confirmed=True)
    plan=a.prepare()
    b.state.update(model='gpt-6-astra',effort='low')
    with pytest.raises(ValueError,match='configuración'):
        a.snapshot(plan)
    assert b.events==[]


def test_review_preserved_selection_defaults_new_destination_items_off(rig,monkeypatch):
    b=rig;d=b.dash;b.configure_source('codex')
    prior={'selection':{'mcps':{'docs':False},'skills':{}},'harness':'codex','manifest':'old-private','operationId':'old-op'}
    monkeypatch.setattr(d.extension_launch,'launch_from_pid',lambda pid:prior)
    inv={'mcps':[dict(id='docs',name='docs',enabled=True,toggleable=True),dict(id='new',name='new',enabled=True,toggleable=True)],'skills':[]}
    monkeypatch.setattr(d.extension_launch,'inventory',lambda *a:inv)
    a=b.adapter(toHarness='codex',motor='codex',model='gpt-6-astra',effort='low')
    assert a.prepare()['extensionLaunch']['selection']['mcps']=={'docs':False,'new':False}


def test_review_opencode_live_environment_survives_destination_and_rollback(rig,monkeypatch,tmp_path):
    import os, subprocess, sys
    b=rig;d=b.dash;a=adapter(b,'opencode')
    content=json.dumps({'model':'fixture/model','permission':{'edit':'deny'}})
    source=subprocess.Popen([sys.executable,'-c','import time; time.sleep(30)'],env=dict(os.environ,OPENCODE_CONFIG_CONTENT=content,OPENCODE_PERMISSION='{"bash":"deny"}'))
    try:
        monkeypatch.setattr(d,'_read_environ',lambda pid:dict(v.split(b'=',1) for v in Path(f'/proc/{source.pid}/environ').read_bytes().split(b'\0') if b'=' in v))
        plan=a.prepare();snapshot=a.snapshot(plan)
        assert content not in json.dumps(snapshot)
        for index,command in enumerate((plan['command'],snapshot['origin']['resume_command'])):
            words=shlex.split(command)
            assert '--environment-file' in words
            path=Path(words[words.index('--environment-file')+1])
            assert path.stat().st_mode & 0o777==0o600
            out=tmp_path/f'env-{index}.json'
            script='import json,os,sys; open(sys.argv[1],"w").write(json.dumps({k:v for k,v in os.environ.items() if k.startswith("OPENCODE_")}))'
            run=subprocess.run([*words[:words.index('--')+1],sys.executable,'-c',script,str(out)],env=dict(os.environ,OPENCODE_PERMISSION='{"bash":"allow"}',OPENCODE_CONFIG_DIR='/wrong'),capture_output=True)
            assert run.returncode==0
            env=json.loads(out.read_text())
            assert env['OPENCODE_CONFIG_CONTENT']==content
            assert env['OPENCODE_PERMISSION']=='{"bash":"deny"}'
            assert 'OPENCODE_CONFIG_DIR' not in env
        assert b.events==[]
    finally:
        source.terminate();source.wait(timeout=5)
