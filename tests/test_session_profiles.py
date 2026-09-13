import importlib.util
import json
import shlex
import sqlite3
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))
sys.path.insert(0, str(ROOT / 'bin'))
import cc_usage


def module():
    import session_profiles
    return session_profiles


def registry(tmp_path):
    return {'harnesses': {name: {'defaultHome': str(tmp_path / ('.' + name)),
        'accountsRoot': str(tmp_path / (name + '-accounts')), 'authFile': 'auth.json',
        'accountEnv': name.upper() + '_HOME', 'capabilities': {'accounts': True}}
        for name in ('codex', 'claude', 'grok')}}


def test_profiles_persist_roundtrip_and_reject_unknown_fields(tmp_path):
    mod = module()
    db = str(tmp_path / 'usage.sqlite')
    p = mod.save_profile(db, {'name': 'Diseño + DigitalOcean', 'harness': 'codex',
        'motor': 'codex', 'skills': {}, 'mcps': {}, 'model': 'gpt-6-astra', 'effort': 'high'})
    assert mod.get_profile(db, p['id']) == p
    assert mod.list_profiles(db) == [p]
    with pytest.raises(ValueError):
        mod.save_profile(db, {'name': 'invalid', 'token': 'secret'})
    with pytest.raises(ValueError):
        mod.get_profile(db, '../../private')
    mod.delete_profile(db, p['id'])
    assert mod.list_profiles(db) == []


def test_codex_profile_overrides_are_process_local_and_preserve_existing(tmp_path, monkeypatch):
    mod = module()
    monkeypatch.setenv('HOME', str(tmp_path))
    home = tmp_path / '.codex'
    skill = home / 'skills' / 'design' / 'SKILL.md'
    skill.parent.mkdir(parents=True)
    skill.write_text('---\nname: design\ndescription: Design\n---\nText')
    config = home / 'config.toml'
    config.write_text('[[skills.config]]\nname="unlisted"\nenabled=false\n[mcp_servers.docs]\ncommand="echo"\n')
    before = config.read_bytes()
    inv = mod.inventory(registry(tmp_path), 'codex', 'main', str(tmp_path))
    row = next(x for x in inv['skills'] if x['name'] == 'design')
    profile = {'harness': 'codex', 'harnessAccount': 'main', 'skills': {row['id']: False}, 'mcps': {'docs': False}}
    args = mod.launch_args(profile, registry(tmp_path), str(tmp_path), str(tmp_path / 'runtime'))
    assert args[0] == '-c'
    assert any('skills.config=' in x and 'unlisted' in x and str(skill) in x for x in args)
    assert 'mcp_servers."docs".enabled=false' in args
    assert config.read_bytes() == before
    assert row['effectiveNow'] is None
    assert row['toggleable'] is True


def test_unknown_skill_and_unsupported_harness_fail_before_launch(tmp_path, monkeypatch):
    mod = module()
    monkeypatch.setenv('HOME', str(tmp_path))
    with pytest.raises(ValueError, match='skill'):
        mod.launch_args({'harness': 'codex', 'skills': {'../../secret': False}}, registry(tmp_path), str(tmp_path), str(tmp_path / 'runtime'))
    with pytest.raises(ValueError, match='soport'):
        mod.launch_args({'harness': 'claude', 'skills': {'skill': False}}, registry(tmp_path), str(tmp_path), str(tmp_path / 'runtime'))


def test_claude_mcp_selection_writes_only_private_launch_config(tmp_path, monkeypatch):
    mod = module()
    monkeypatch.setenv('HOME', str(tmp_path))
    project = tmp_path / 'project'
    project.mkdir()
    cfg = project / '.mcp.json'
    cfg.write_text(json.dumps({'mcpServers': {'docs': {'command': 'echo', 'env': {'TOKEN': 'private'}}, 'other': {'command': 'echo'}}}))
    before = cfg.read_bytes()
    args = mod.launch_args({'harness': 'claude', 'mcps': {'docs': False}}, registry(tmp_path), str(project), str(tmp_path / 'runtime'))
    assert '--strict-mcp-config' in args
    launch = Path(args[args.index('--mcp-config') + 1])
    assert json.loads(launch.read_text()) == {'mcpServers': {'other': {'command': 'echo'}}}
    assert launch.stat().st_mode & 0o777 == 0o600
    assert cfg.read_bytes() == before
    inv = mod.inventory(registry(tmp_path), 'claude', 'main', str(project))
    assert 'private' not in json.dumps(inv)


def test_extension_usage_scopes_counts_and_null_unmeasured_cost(tmp_path):
    mod = module()
    db = str(tmp_path / 'usage.sqlite')
    for pane in ('%1', '%2'):
        cc_usage.capture_lifecycle(db, {'status':'working','tmux_session':'s','tmux_pane':pane,'at_ms':1000})
        for phase, at in [('start',1100),('success',1400)]:
            cc_usage.capture_tool_event(db, {'phase':phase,'tmux_session':'s','tmux_pane':pane,
                'tool_name':'mcp__docs__search','tool_use_id':'mcp1','at_ms':at})
            cc_usage.capture_tool_event(db, {'phase':phase,'tmux_session':'s','tmux_pane':pane,
                'tool_name':'Skill','skill_name':'superpowers:brainstorming','tool_use_id':'sk1','at_ms':at,
                'arguments':'secret payload'})
    result = mod.extension_usage(db, 's', '%1', days=7, now=2)
    assert result['scope'] == {'session':'s','pane':'%1'}
    mcp = next(x for x in result['extensions'] if x['kind'] == 'mcp')
    skill = next(x for x in result['extensions'] if x['kind'] == 'skill')
    assert mcp['name'] == 'docs' and mcp['count'] == 1 and mcp['durationMs'] == 300
    assert skill['name'] == 'superpowers:brainstorming' and skill['count'] == 1
    assert skill['costUsd'] is None and skill['tokens'] is None
    assert 'secret payload' not in Path(db).read_bytes().decode('utf-8', errors='ignore')
    assert mod.extension_usage(db, 's', '%3', now=2)['extensions'] == []


def test_skill_name_validation_does_not_store_arbitrary_payload(tmp_path):
    module()
    db = str(tmp_path / 'usage.sqlite')
    cc_usage.capture_lifecycle(db, {'status':'working','tmux_session':'s','tmux_pane':'%1','at_ms':1000})
    cc_usage.capture_tool_event(db, {'phase':'start','tmux_session':'s','tmux_pane':'%1',
        'tool_name':'Skill','skill_name':'not a skill\nsecret','at_ms':1100})
    result = module().extension_usage(db, 's', '%1', now=2)
    assert result['unattributedSkillCalls'] == 1
    assert 'secret' not in json.dumps(result)


def test_python310_can_override_mcps_and_skills_without_replacing_existing_arrays(tmp_path, monkeypatch):
    mod = module()
    monkeypatch.setattr(mod, 'tomllib', None)
    monkeypatch.setenv('HOME', str(tmp_path))
    home = tmp_path / '.codex'
    skill = home / 'skills' / 'design' / 'SKILL.md'
    skill.parent.mkdir(parents=True)
    skill.write_text('---\nname: design\n---\n')
    config = home / 'config.toml'
    config.write_text('[mcp_servers.docs]\ncommand="echo"\n')
    inv = mod.inventory(registry(tmp_path), 'codex', 'main', str(tmp_path))
    ident = inv['skills'][0]['id']
    assert inv['skills'][0]['toggleable']
    args = mod.launch_args({'harness':'codex', 'skills':{ident:False}}, registry(tmp_path), str(tmp_path), '')
    assert 'skills.config=' in args[1]
    config.write_text(config.read_text() + '\n[[skills.config]]\nname="other"\nenabled=false\n')
    inv = mod.inventory(registry(tmp_path), 'codex', 'main', str(tmp_path))
    assert not inv['skills'][0]['toggleable']
    with pytest.raises(ValueError, match='TOML'):
        mod.launch_args({'harness':'codex','skills':{ident:False}}, registry(tmp_path), str(tmp_path), '')
    assert mod.launch_args({'harness':'codex','mcps':{'docs':False}}, registry(tmp_path), str(tmp_path), '')


def test_model_context_suffix_and_empty_route_are_accepted(tmp_path):
    mod = module()
    p = mod.save_profile(str(tmp_path / 'db'), {'name':'Long context','model':'gpt-5.6-sol[1m]'})
    assert mod.launch_draft(p)['model'] == 'gpt-5.6-sol[1m]'
    assert 'routeId' not in mod.launch_draft(p)


def dash_module():
    import importlib.machinery
    loader = importlib.machinery.SourceFileLoader('profile_dash_test', str(ROOT / 'bin/cc-dash'))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    dash = importlib.util.module_from_spec(spec)
    loader.exec_module(dash)
    return dash


def post(dash, path, data):
    import io
    from types import SimpleNamespace
    body = json.dumps(data).encode()
    handler = SimpleNamespace(path=path, headers={'Content-Length': str(len(body))},
        rfile=io.BytesIO(body), _security_gate=lambda: None, _json=lambda status, payload: (status, payload))
    return dash.Handler.do_POST(handler)


def test_profile_endpoints_save_prepare_and_refuse_shared_toggles(tmp_path, monkeypatch):
    dash = dash_module()
    monkeypatch.setattr(dash, 'USAGE_DB', str(tmp_path / 'usage.sqlite'))
    monkeypatch.setattr(dash, 'resolve_route_selection', lambda *a: ({}, {}))
    status, saved = post(dash, '/session-profiles', {'name':'Daily','harness':'codex','model':'gpt-6-astra'})
    assert status == 200
    status, prepared = post(dash, '/session-profile-apply', {'profileId': saved['profile']['id']})
    assert status == 200 and prepared['effectiveNow'] is False
    assert prepared['launchDraft']['model'] == 'gpt-6-astra'
    for endpoint in ('/skill-toggle','/mcp-toggle'):
        status, body = post(dash, endpoint, {'path':'/home/me/.claude/skills/sample/SKILL.md','on':False})
        assert status == 409 and body['code'] == 'profile_required'
    assert '/session-profiles' in dash.Handler.API_GET
    assert '/extension-usage' in dash.Handler.API_GET


def test_session_new_launches_saved_profile_flags_without_mutating_configuration(tmp_path, monkeypatch):
    from types import SimpleNamespace
    dash = dash_module()
    monkeypatch.setenv('HOME', str(tmp_path))
    monkeypatch.setattr(dash, 'USAGE_DB', str(tmp_path / 'usage.sqlite'))
    monkeypatch.setattr(dash, 'HOOKS', str(tmp_path / 'hooks'))
    reg = registry(tmp_path)
    home = tmp_path / '.codex'
    home.mkdir()
    config = home / 'config.toml'
    config.write_text('[mcp_servers.docs]\ncommand="echo"\n')
    monkeypatch.setattr(dash, 'load_provider_registry', lambda: reg)
    selected = {'model':'gpt-6-astra','effort':'high','harnessAccount':'main','motorAccount':'main'}
    seen = []
    def resolve(data, context):
        assert data['model'] == 'gpt-6-astra' and data['profileId']
        return {'harness':'codex','motor':'codex','id':'codex:codex'}, selected
    monkeypatch.setattr(dash, 'resolve_route_selection', resolve)
    monkeypatch.setattr(dash, 'read_conf', lambda: {'AUTO_WORKTREE':'0'})
    monkeypatch.setattr(dash, 'load_agent_roles', lambda: {})
    monkeypatch.setattr(dash, 'register_app_tab', lambda *a, **k: None)
    monkeypatch.setattr(dash, 'record_runtime_config', lambda *a, **k: None)
    def tmux(*args):
        seen.append(args)
        return SimpleNamespace(returncode=1 if args[0]=='has-session' else 0,
            stdout='%55\n' if args[0]=='display-message' else '', stderr='')
    monkeypatch.setattr(dash, 'tmux', tmux)
    class ImmediateThread:
        def __init__(self, target, **kwargs): self.target=target
        def start(self): self.target()
    monkeypatch.setattr(dash.threading, 'Thread', ImmediateThread)
    monkeypatch.setattr(dash.time, 'sleep', lambda *a: None)
    profile = module().save_profile(dash.USAGE_DB, {'name':'Docs off','harness':'codex',
        'model':'gpt-6-astra','effort':'high','mcps':{'docs':False}})
    status, result = post(dash, '/session-new', {'profileId':profile['id'],'cwd':str(tmp_path)})
    assert status == 200 and result['profileId'] == profile['id']
    assert result['profileEffectiveNow'] is None
    command = next(args[3] for args in seen if args[0]=='send-keys')
    assert 'COMANDOS_SESSION_PROFILE=' in command
    assert 'mcp_servers."docs".enabled=false' in shlex.split(command)
    assert config.read_text() == '[mcp_servers.docs]\ncommand="echo"\n'


def test_skill_schema_migrates_existing_usage_database(tmp_path):
    db = str(tmp_path / 'usage.sqlite')
    cc_usage.init_db(db)
    with sqlite3.connect(db) as con:
        con.execute('alter table usage_tool_calls drop column skill_name')
        con.execute('pragma user_version=8')
    cc_usage.init_db(db)
    with sqlite3.connect(db) as con:
        assert 'skill_name' in {r[1] for r in con.execute('pragma table_info(usage_tool_calls)')}


def test_claude_inventory_marks_unsafe_isolation_unsupported(tmp_path, monkeypatch):
    mod = module()
    monkeypatch.setenv('HOME', str(tmp_path))
    home = tmp_path / '.claude'
    home.mkdir()
    (home / 'settings.json').write_text(json.dumps({'enabledPlugins':{'docs@example':True}}))
    (tmp_path / '.mcp.json').write_text(json.dumps({'mcpServers':{'docs':{'command':'echo'}}}))
    inv = mod.inventory(registry(tmp_path), 'claude', 'main', str(tmp_path))
    assert inv['capabilities']['mcps']['supported'] is False
    assert not inv['mcps'][0]['toggleable']


def test_profile_named_account_alias_does_not_accept_path_syntax(tmp_path):
    with pytest.raises(ValueError, match='cuenta'):
        module().save_profile(str(tmp_path/'db'), {'name':'Invalid account','harnessAccount':'account/escape'})


def test_global_extension_usage_aggregates_observed_calls_with_bounded_time_window(tmp_path):
    mod = module()
    db = str(tmp_path/'usage.sqlite')
    for session, at in [('s1', 1000000000), ('s2', 1000000000), ('old', 1000)]:
        cc_usage.capture_lifecycle(db, {'status':'working','tmux_session':session,'tmux_pane':'%1','at_ms':at})
        cc_usage.capture_tool_event(db, {'phase':'success','tmux_session':session,'tmux_pane':'%1',
            'tool_name':'mcp__docs__search','at_ms':at+100})
    result = mod.extension_usage(db, '', days=7, now=1000001)
    assert result['extensions'][0]['count'] == 2
    assert result['scope'] == {'session':'','pane':''}
    assert result['extensions'][0]['durationMs'] is None
    with pytest.raises(ValueError):
        mod.extension_usage(db, '', pane='%1')
