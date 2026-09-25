import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))
sys.path.insert(0, str(ROOT / 'bin'))


def mod():
    import extension_launch
    return extension_launch


@pytest.fixture
def fx(tmp_path, monkeypatch):
    monkeypatch.setenv('HOME', str(tmp_path))
    homes = {h: tmp_path / ('.gemini' if h == 'agy' else '.' + h) for h in ('codex', 'claude', 'opencode', 'grok', 'agy')}
    registry = {'harnesses': {h: {'defaultHome': str(p), 'authFile':'auth.json', 'accountEnv': h.upper()+'_HOME', 'accountsRoot': str(tmp_path / (h+'-accounts')), 'capabilities': {'accounts': h in ('codex','claude','grok')}} for h,p in homes.items()}}
    for h,p in homes.items():
        p.mkdir()
        skill = p / ('config/skills' if h == 'agy' else 'skills') / 'design/SKILL.md'
        skill.parent.mkdir(parents=True)
        skill.write_text('---\nname: design\ndescription: fixture\n---\nTest')
        if h in ('codex','grok'):
            (p/'config.toml').write_text('[mcp_servers.docs]\ncommand="echo"\n[mcp_servers.off]\ncommand="echo"\nenabled=false\n')
        elif h == 'claude':
            (p/'.claude.json').write_text(json.dumps({'mcpServers': {'docs': {'command':'echo','env':{'TOKEN':'fixture-secret'}},'off':{'command':'echo','disabled':True}}}))
        elif h == 'opencode':
            (p/'opencode.json').write_text(json.dumps({'mcp':{'docs':{'type':'local','command':['echo']},'off':{'enabled':False}}}))
        else:
            (p/'config/mcp_config.json').write_text(json.dumps({'mcpServers':{'docs':{'command':'echo'},'off':{'command':'echo','disabled':True}}}))
            (p/'config/skills.json').write_text('{}')
    cwd = tmp_path/'project'; cwd.mkdir()
    return registry, homes, cwd, tmp_path/'runtime'


@pytest.mark.parametrize('harness', ['claude','codex','grok','opencode','agy'])
def test_inventory_and_launch_are_private_and_preserve_disabled_defaults(fx, harness):
    m=mod(); registry,homes,cwd,runtime=fx
    before={str(p):p.read_bytes() for home in homes.values() for p in home.rglob('*') if p.is_file()}
    inv=m.inventory(registry,harness,'main',str(cwd))
    assert 'fixture-secret' not in json.dumps(inv)
    assert next(r for r in inv['mcps'] if r['id']=='off')['enabled'] is False
    sk=next(r for r in inv['skills'] if r['name']=='design' and r['toggleable'])
    launch=m.prepare_launch(registry,harness,'main',str(cwd),{'mcps':{'docs':False},'skills':{sk['id']:False}},str(runtime),'op-1')
    assert launch['selection']['mcps']=={'docs':False,'off':False}
    assert 'fixture-secret' not in json.dumps(launch)
    path=Path(launch['manifest']); assert path.stat().st_mode & 0o777 == 0o600
    assert path.parent.stat().st_mode & 0o777 == 0o700
    assert all(Path(p).read_bytes()==data for p,data in before.items())
    data=json.loads(path.read_text())
    if harness=='codex':
        assert 'mcp_servers.docs.enabled=false' in data['args']
        assert any('SKILL.md' in arg for arg in data['args'])
    if harness=='opencode':
        assert json.loads(data['env']['OPENCODE_CONFIG_CONTENT'])['permission']['skill']['design']=='deny'
    if harness=='claude':
        assert '--strict-mcp-config' in data['args']
        settings=json.loads(Path(data['args'][data['args'].index('--settings')+1]).read_text())
        assert settings['skillOverrides']['design']=='off'


def test_invalid_selection_and_dotted_codex_names_fail_closed(fx):
    m=mod(); registry,homes,cwd,runtime=fx
    with (homes['codex']/'config.toml').open('a') as f: f.write('[mcp_servers."has.dot"]\ncommand="echo"\n')
    inv=m.inventory(registry,'codex','main',str(cwd))
    assert not next(r for r in inv['mcps'] if r['id']=='has.dot')['toggleable']
    for selection in ({'mcps':{'has.dot':False}}, {'mcps':{'absent':False}}, {'skills':{'x':'false'}}):
        with pytest.raises(ValueError):m.prepare_launch(registry,'codex','main',str(cwd),selection,str(runtime),'op')


def test_wrap_preserves_model_permission_and_overrides_prior_extension_flags(fx):
    m=mod();registry,homes,cwd,runtime=fx
    launch=m.prepare_launch(registry,'codex','main',str(cwd),{'mcps':{'docs':False}},str(runtime),'op')
    script=runtime/'capture.py'
    output=runtime/'captured.json'
    script.write_text('import json,sys,os;json.dump({"args":sys.argv[1:],"home":os.environ["HOME"]},open(sys.argv[1],"w"))')
    command=shlex.join([sys.executable,str(script),str(output),'--model','model-x','--dangerously-bypass-approvals-and-sandbox','-c','mcp_servers.docs.enabled=true','-c','unrelated=true'])
    subprocess.run(m.wrap_command(command,launch),shell=True,check=True)
    args=json.loads(output.read_text())['args']
    assert args.index('mcp_servers.docs.enabled=false') > args.index('mcp_servers.docs.enabled=true')
    assert '--model' in args and 'unrelated=true' in args
    assert json.loads(output.read_text())['home']==str(cwd.parent)


def test_namespace_proof_mounts_only_child_and_preserves_home(tmp_path):
    assert mod().namespace_preflight(str(tmp_path)) is True


def test_verification_requires_marker_manifest_and_effective_evidence(fx):
    m=mod();registry,homes,cwd,runtime=fx
    launch=m.prepare_launch(registry,'opencode','main',str(cwd),{'mcps':{'docs':False}},str(runtime),'live-op')
    cmd=shlex.join([sys.executable,'-c','import time;time.sleep(20)'])
    proc=subprocess.Popen(m.wrap_command(cmd,launch),shell=True,start_new_session=True)
    try:
        deadline=time.monotonic()+3
        pid=None
        while time.monotonic()<deadline:
            kids=Path(f'/proc/{proc.pid}/task/{proc.pid}/children').read_text().split()
            for kid in [str(proc.pid)]+kids:
                if m.verify_launch(int(kid),launch):pid=int(kid);break
            if pid:break
            time.sleep(.02)
        assert pid is not None
        assert m.launch_from_pid(pid)==launch
        assert not m.verify_launch(os.getpid(),launch)
        data=json.loads(Path(launch['manifest']).read_text());data['env']['OPENCODE_CONFIG_CONTENT']='{}'
        Path(launch['manifest']).write_text(json.dumps(data))
        assert not m.verify_launch(pid,launch)
    finally:
        import signal
        os.killpg(proc.pid,signal.SIGTERM);proc.wait()


def test_claude_enabled_value_matches_installed_schema_and_artifacts_verified(fx):
    m=mod();registry,homes,cwd,runtime=fx
    launch=m.prepare_launch(registry,'claude','main',str(cwd),{},str(runtime),'claude-on')
    data=json.loads(Path(launch['manifest']).read_text())
    settings=json.loads(Path(data['args'][1]).read_text())
    assert settings['skillOverrides']['design']=='on'
    assert data['artifacts']


@pytest.mark.parametrize('harness',['claude','codex','grok','opencode','agy'])
def test_shared_catalog_servers_get_private_proxy_declarations_and_disabled_stay_off(fx,harness):
    m=mod();registry,homes,cwd,runtime=fx
    path=cwd.parent/'.config/comandos/extensions/catalog.json';path.parent.mkdir(parents=True)
    path.write_text(json.dumps({'servers':{'shared-docs':{'command':'fixture','enabled':True},'global-off':{'command':'fixture','enabled':False}}}))
    inv=m.inventory(registry,harness,'main',str(cwd))
    shared=next(r for r in inv['mcps'] if r['id']=='shared-docs')
    assert shared['toggleable'] and shared['enabled']
    off=next(r for r in inv['mcps'] if r['id']=='global-off')
    assert not off['toggleable'] and off['enabled'] is False
    launch=m.prepare_launch(registry,harness,'main',str(cwd),{},str(runtime),'shared')
    data=json.loads(Path(launch['manifest']).read_text())
    # Never copy raw transports or secrets from catalog into command/API metadata.
    assert 'fixture' not in json.dumps(launch)
    if harness=='codex':assert any('mcp_servers.shared-docs={' in a for a in data['args'])
    elif harness=='opencode':assert json.loads(data['env']['OPENCODE_CONFIG_CONTENT'])['mcp']['shared-docs']['command'][-2:]==['serve','shared-docs']
    else:
        config=Path(data['args'][-1]) if harness=='claude' else Path(data['mounts'][0]['source'])
        assert 'shared-docs' in config.read_text()


def test_namespace_launch_verifies_mount_evidence_and_auth_path_is_original(fx):
    m=mod();registry,homes,cwd,runtime=fx
    launch=m.prepare_launch(registry,'grok','main',str(cwd),{'mcps':{'docs':False}},str(runtime),'ns-live')
    output=runtime/'seen.json'; script=runtime/'probe.py'
    script.write_text('import json,os,pathlib,sys,time; json.dump({"config":pathlib.Path(sys.argv[2]).read_text(),"home":os.environ["HOME"]},open(sys.argv[1],"w"));time.sleep(20)')
    command=shlex.join([sys.executable,str(script),str(output),str(homes['grok']/'config.toml')])
    proc=subprocess.Popen(m.wrap_command(command,launch),shell=True,start_new_session=True)
    try:
        deadline=time.monotonic()+3; valid=False
        while time.monotonic()<deadline:
            for pid in [proc.pid]+[int(p) for p in Path(f'/proc/{proc.pid}/task/{proc.pid}/children').read_text().split()]:
                valid=valid or m.verify_launch(pid,launch)
            if valid and output.exists():break
            time.sleep(.02)
        assert valid
        seen=json.loads(output.read_text());assert m._parse_toml(seen['config'])['disabled_mcp_servers']==['docs','off']
        assert seen['home']==str(cwd.parent)
        assert 'disabled_mcp_servers' not in (homes['grok']/'config.toml').read_text()
    finally:
        import signal
        os.killpg(proc.pid,signal.SIGTERM);proc.wait()


def test_env_unsets_and_assignments_survive_wrapper(fx):
    m=mod();registry,homes,cwd,runtime=fx
    launch=m.prepare_launch(registry,'opencode','main',str(cwd),{},str(runtime),'env')
    out=runtime/'env.json';script=runtime/'probe.py'
    script.write_text('import json,os,sys;json.dump(dict(os.environ),open(sys.argv[1],"w"))')
    command=shlex.join(['env','-u','CLAUDE_CONFIG_DIR','-u','CODEX_HOME','CODEX_HOME=/fixture/account', 'OPENCODE_CONFIG_CONTENT={"model":"retain"}',sys.executable,str(script),str(out)])
    subprocess.run(m.wrap_command(command,launch),shell=True,check=True,env={**os.environ,'CLAUDE_CONFIG_DIR':'wrong','COMANDOS_OPERATION_ID':'parent-op'})
    env=json.loads(out.read_text())
    assert 'CLAUDE_CONFIG_DIR' not in env and env['CODEX_HOME']=='/fixture/account'
    assert env['COMANDOS_OPERATION_ID']=='parent-op'
    assert json.loads(env['OPENCODE_CONFIG_CONTENT'])['model']=='retain'
    assert 'mcp' in json.loads(env['OPENCODE_CONFIG_CONTENT'])


def test_codex_retains_unlisted_cli_skill_overrides(fx):
    m=mod();registry,homes,cwd,runtime=fx
    launch=m.prepare_launch(registry,'codex','main',str(cwd),{},str(runtime),'prior-skills')
    output=runtime/'args.json';script=runtime/'probe.py'
    script.write_text('import json,sys;json.dump(sys.argv,open(sys.argv[1],"w"))')
    command=shlex.join([sys.executable,str(script),str(output),'-c','skills.config=[{name="unlisted-cli",enabled=false}]'])
    subprocess.run(m.wrap_command(command,launch),shell=True,check=True)
    args=json.loads(output.read_text())
    assert 'unlisted-cli' in next(a for a in reversed(args) if a.startswith('skills.config='))


def test_agy_without_skills_json_mounts_selected_directory_without_global_creation(fx):
    m=mod();registry,homes,cwd,runtime=fx
    path=homes['agy']/'config/skills.json';path.unlink()
    inv=m.inventory(registry,'agy','main',str(cwd)); skill=next(r for r in inv['skills'] if r['toggleable'])
    launch=m.prepare_launch(registry,'agy','main',str(cwd),{'skills':{skill['id']:False}},str(runtime),'agy-dir')
    data=json.loads(Path(launch['manifest']).read_text())
    mount=next(row for row in data['mounts'] if row.get('kind')=='directory')
    assert list(Path(mount['source']).iterdir())==[]
    assert not path.exists()
    assert (homes['agy']/'config/skills/design/SKILL.md').is_file()


def test_unknown_native_state_is_not_synthesized_in_selection():
    inv={'mcps':[{'id':'unknown','name':'unknown','enabled':None,'toggleable':False}], 'skills':[]}
    assert mod()._normalize(inv,{})=={'mcps':{},'skills':{}}
    with pytest.raises(ValueError):mod()._normalize(inv,{'mcps':{'unknown':False}})


def test_manifest_artifact_tampering_rejected_before_launch(fx):
    m=mod();registry,homes,cwd,runtime=fx
    launch=m.prepare_launch(registry,'claude','main',str(cwd),{},str(runtime),'tamper')
    data=json.loads(Path(launch['manifest']).read_text())
    artifact=Path(next(iter(data['artifacts'])))
    artifact.write_text('{}')
    with pytest.raises(ValueError):m.wrap_command('echo safe',launch)


def test_claude_merges_prior_settings_preserving_permissions(fx):
    m=mod();registry,homes,cwd,runtime=fx
    launch=m.prepare_launch(registry,'claude','main',str(cwd),{'mcps':{'docs':False}},str(runtime),'merge-settings')
    data=json.loads(Path(launch['manifest']).read_text())
    cmd,extra,env,settings=m._resolve_command(data,['claude','--model','opus','--settings',json.dumps({'permissions':{'deny':['Bash(rm:*)']},'skillOverrides':{'design':'off'}}),'--mcp-config','old.json'])
    assert settings['permissions']=={'deny':['Bash(rm:*)']}
    assert settings['skillOverrides']['design']=='on'
    assert 'old.json' not in cmd and '--model' in cmd


@pytest.mark.parametrize('harness',['claude','codex','grok','opencode','agy'])
def test_all_adapters_verify_real_fixture_processes(fx,harness):
    m=mod();registry,homes,cwd,runtime=fx
    launch=m.prepare_launch(registry,harness,'main',str(cwd),{'mcps':{'docs':False}},str(runtime),'fixture-live')
    command=shlex.join([sys.executable,'-c','import time;time.sleep(20)'])
    proc=subprocess.Popen(m.wrap_command(command,launch),shell=True,start_new_session=True)
    try:
        deadline=time.monotonic()+3;verified=False
        while time.monotonic()<deadline:
            children=Path(f'/proc/{proc.pid}/task/{proc.pid}/children').read_text().split()
            verified=any(m.verify_launch(int(pid),launch) for pid in [str(proc.pid)]+children)
            if verified:break
            time.sleep(.02)
        assert verified
    finally:
        import signal
        os.killpg(proc.pid,signal.SIGTERM);proc.wait()


def test_opencode_keeps_prior_blanket_permission_for_other_tools(fx):
    m=mod();registry,homes,cwd,runtime=fx
    launch=m.prepare_launch(registry,'opencode','main',str(cwd),{},str(runtime),'deny')
    data=json.loads(Path(launch['manifest']).read_text())
    _,_,env,_=m._resolve_command(data,['env','OPENCODE_CONFIG_CONTENT={"permission":"deny"}','opencode'])
    assert json.loads(env['OPENCODE_CONFIG_CONTENT'])['permission']['*']=='deny'


def test_globally_disabled_catalog_blocks_stale_enabled_native_declaration(fx):
    m=mod();registry,homes,cwd,runtime=fx
    path=cwd.parent/'.config/comandos/extensions/catalog.json';path.parent.mkdir(parents=True)
    path.write_text(json.dumps({'servers':{'docs':{'command':'fixture','enabled':False}}}))
    launch=m.prepare_launch(registry,'codex','main',str(cwd),{},str(runtime),'global-off')
    data=json.loads(Path(launch['manifest']).read_text())
    assert 'mcp_servers.docs.enabled=false' in data['args']


def test_opencode_command_overlay_wins_over_parent_unrelated_settings(fx,monkeypatch):
    m=mod();registry,homes,cwd,runtime=fx
    monkeypatch.setenv('OPENCODE_CONFIG_CONTENT','{"model":"parent","permission":{"bash":"allow"}}')
    launch=m.prepare_launch(registry,'opencode','main',str(cwd),{},str(runtime),'precedence')
    data=json.loads(Path(launch['manifest']).read_text())
    _,_,env,_=m._resolve_command(data,['env','OPENCODE_CONFIG_CONTENT={"model":"session","permission":{"bash":"deny"}}','opencode'])
    overlay=json.loads(env['OPENCODE_CONFIG_CONTENT'])
    assert overlay['model']=='session' and overlay['permission']['bash']=='deny'


def test_agy_project_mcp_is_filtered_in_private_project_mount(fx):
    m=mod();registry,homes,cwd,runtime=fx
    path=cwd/'.agents/mcp_config.json';path.parent.mkdir(parents=True)
    path.write_text(json.dumps({'mcpServers':{'project-docs':{'command':'fixture'}}}))
    launch=m.prepare_launch(registry,'agy','main',str(cwd),{'mcps':{'project-docs':False}},str(runtime),'project')
    data=json.loads(Path(launch['manifest']).read_text())
    mount=next(r for r in data['mounts'] if r['target']==str(path))
    assert 'project-docs' not in json.loads(Path(mount['source']).read_text())['mcpServers']
    assert 'project-docs' in path.read_text()


def test_default_python_runtime_can_prepare_all_adapters_without_optional_packages(tmp_path):
    script='''
import json,os,sys
from pathlib import Path
sys.path[:0]=[sys.argv[1]+'/lib',sys.argv[1]+'/bin']
import extension_launch as m
root=Path(os.environ['HOME']);cwd=root/'project';cwd.mkdir();runtime=root/'runtime'
registry={'harnesses':{}}
for h in ['claude','codex','grok','opencode','agy']:
 home=root/('.'+h);home.mkdir()
 registry['harnesses'][h]={'defaultHome':str(home),'capabilities':{'accounts':False}}
 if h in ('codex','grok'):(home/'config.toml').write_text('[mcp_servers.docs]\\ncommand="echo"\\n')
 if h=='agy':
  (home/'config').mkdir();(home/'config/mcp_config.json').write_text('{"mcpServers":{}}');(home/'config/skills.json').write_text('{}')
 launch=m.prepare_launch(registry,h,'main',str(cwd),{},str(runtime),'fixture-'+h)
 m.wrap_command('echo fixture',launch)
print('all-runtime-adapters-ok')
'''
    result=subprocess.run(['python3','-c',script,str(ROOT)],env={**os.environ,'HOME':str(tmp_path)},capture_output=True,text=True)
    assert result.returncode==0,result.stderr
    assert result.stdout.strip()=='all-runtime-adapters-ok'


def test_review_opencode_private_environment_receipt_and_second_apply(fx,monkeypatch):
    m=mod();registry,homes,cwd,runtime=fx
    inv=m.inventory(registry,'opencode','main',str(cwd))
    source={b'OPENCODE_CONFIG_CONTENT':b'{"model":"fixture/model","permission":{"edit":"deny"}}',b'OPENCODE_PERMISSION':b'{"bash":"deny"}',b'AUTH_TOKEN':b'never-copy'}
    ref=m.capture_opencode_environment(source,runtime,inv)
    assert 'never-copy' not in Path(ref['path']).read_text()
    monkeypatch.setenv('OPENCODE_PERMISSION','{"bash":"allow"}')
    command=shlex.join([sys.executable,'-c','import time;time.sleep(20)'])
    for index in range(2):
        launch=m.prepare_launch(registry,'opencode','main',str(cwd),{'mcps':{'docs':False},'skills':{}},runtime,'env-'+str(index))
        proc=subprocess.Popen(shlex.split(m.wrap_environment(m.wrap_command(command,launch),ref)))
        try:
            for _ in range(100):
                if m.verify_launch(proc.pid,launch):break
                time.sleep(.02)
            assert m.verify_launch(proc.pid,launch)
            env=m._process_env(proc.pid)
            assert env['OPENCODE_PERMISSION']=='{"bash":"deny"}'
            content=json.loads(env['OPENCODE_CONFIG_CONTENT'])
            assert content['model']=='fixture/model' and content['permission']['edit']=='deny'
            assert content['mcp']['docs']['enabled'] is False
            ref=m.capture_opencode_environment({k.encode():v.encode() for k,v in env.items()},runtime,inv,managed=True)
        finally:
            proc.terminate();proc.wait(timeout=5)


@pytest.mark.parametrize('source',[{b'OPENCODE_CONFIG':b'/other'},{b'OPENCODE_CONFIG_DIR':b'/other'},{b'OPENCODE_CONFIG_CONTENT':b'{"mcp":{"unknown":{"enabled":true}}}'},{b'OPENCODE_CONFIG_CONTENT':b'{"mcp":{"docs":{"command":["/private-server"]}}}'},{b'OPENCODE_CONFIG_CONTENT':b'{"plugin":["unknown"]}'},{b'OPENCODE_CONFIG_CONTENT':b'{"skills":{"paths":["/unknown"]}}'}])
def test_review_opencode_unsupported_scope_rejected(fx,source):
    m=mod();registry,homes,cwd,runtime=fx
    with pytest.raises(ValueError,match='compatible'):
        m.capture_opencode_environment(source,runtime,m.inventory(registry,'opencode','main',str(cwd)))
