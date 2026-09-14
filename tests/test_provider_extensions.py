"""Account/project extension configuration, independent of running providers."""
import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'bin'))
import capabilities
import session_profiles


def registry(root):
    reg = json.loads((Path(__file__).resolve().parents[1] / 'config/providers.json').read_text())
    for name, spec in reg['harnesses'].items():
        if spec.get('capabilities', {}).get('accounts'):
            spec['defaultHome'] = str(root / ('.' + name))
            spec['accountsRoot'] = str(root / ('.' + name + '-accounts'))
    return reg


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data) if isinstance(data, dict) else data)
    return path


def skill(path, name='example', extra=''):
    return write(path / 'SKILL.md', f'---\nname: {name}\ndescription: Public skill description\n{extra}---\nInstructions\n')


@pytest.fixture
def context(tmp_path, monkeypatch):
    monkeypatch.setenv('HOME', str(tmp_path))
    monkeypatch.setenv('XDG_CONFIG_HOME', str(tmp_path / '.config'))
    for key in ('OPENCODE_CONFIG', 'OPENCODE_CONFIG_DIR', 'OPENCODE_CONFIG_CONTENT'):
        monkeypatch.delenv(key, raising=False)
    cwd = tmp_path / 'repo' / 'src'
    cwd.mkdir(parents=True)
    (cwd.parent / '.git').mkdir()
    return tmp_path, cwd, registry(tmp_path)


def test_codex_layer_overrides_are_case_sensitive_and_keep_provenance(context):
    root, cwd, reg = context
    write(root / '.codex/config.toml', '[mcp_servers.docs]\nenabled=true\n[mcp_servers.Docs]\nenabled=false\n')
    write(cwd.parent / '.codex/config.toml', '[mcp_servers.docs]\nenabled=false\n')
    rows = {x['name']: x for x in capabilities.session_capabilities(reg, 'codex', 'main', str(cwd))['mcps']}
    assert set(rows) == {'Docs', 'docs'}
    assert rows['docs']['enabled'] is False
    assert rows['docs']['source'] == 'codex-project'
    assert rows['docs']['sources'] == ['codex-user', 'codex-project']


def test_toml_inline_and_dotted_tables_and_malformed_files(context):
    root, cwd, reg = context
    write(root / '.codex/config.toml', 'mcp_servers = { inline = { enabled = false }, second = { command = "PRIVATE" } }\n')
    inv = session_profiles.inventory(reg, 'codex', 'main', str(cwd))
    assert {x['name']:x['enabled'] for x in inv['mcps']} == {'inline':False, 'second':True}
    write(root / '.codex/config.toml', '[mcp_servers.invalid]\nenabled=not-valid-toml\n')
    inv = session_profiles.inventory(reg, 'codex', 'main', str(cwd))
    assert inv['status'] == 'incomplete'
    assert not inv['mcps']
    assert 'PRIVATE' not in json.dumps(inv)


def test_claude_local_scope_and_account_isolation(context):
    root, cwd, reg = context
    write(root / '.claude-accounts/.claude.json', {'mcpServers':{'wrong-account':{}}})
    named = root / '.claude-accounts/work'
    named.mkdir()
    assert capabilities.session_capabilities(reg, 'claude', 'work', str(cwd))['mcps'] == []
    write(named / '.claude.json', {'mcpServers':{'docs':{'description':'User'}}, 'projects':{str(cwd):{'mcpServers':{'docs':{'description':'Local'},'private':{}}}}})
    write(cwd / '.mcp.json', {'mcpServers':{'docs':{'description':'Project'}}})
    rows = {x['name']:x for x in capabilities.session_capabilities(reg,'claude','work',str(cwd))['mcps']}
    assert set(rows) == {'docs','private'}
    assert rows['docs']['description'] == 'Local'
    assert rows['docs']['scope'] == 'local'


def test_claude_disabled_settings_do_not_disable_automatic_invocation(context):
    root, cwd, reg = context
    skill(root / '.claude/skills/manual', 'manual', 'disable-model-invocation: true\n')
    write(cwd / '.mcp.json', {'mcpServers':{'docs':{}}})
    write(root / '.claude/settings.json', {'disabledMcpjsonServers':['docs']})
    inv = session_profiles.inventory(reg,'claude','main',str(cwd))
    assert inv['mcps'][0]['enabled'] is False
    assert inv['skills'][0]['enabled'] is True
    assert inv['skills'][0]['automaticInvocation'] is False


def test_codex_skill_file_override_symlink_and_explicit_reenable(context):
    root, cwd, reg = context
    p = skill(root / '.codex/skills/design','design')
    alias = root / '.agents/skills/design'
    alias.parent.mkdir(parents=True)
    alias.symlink_to(p.parent)
    write(root / '.codex/config.toml', f'[[skills.config]]\npath={json.dumps(str(p))}\nenabled=false\n')
    inv = session_profiles.inventory(reg,'codex','main',str(cwd))
    assert len(inv['skills']) == 1
    row = inv['skills'][0]
    assert row['enabled'] is False and row['status'] == 'disabled'
    assert len(row['sources']) == 2
    args = session_profiles.launch_args({'harness':'codex','skills':{row['id']:True}},reg,str(cwd),'')
    assert args[1].count('enabled=') == 1
    assert 'enabled=true' in args[1]


def test_codex_plugins_are_namespaced_and_disabled_cache_is_not_active(context):
    root, cwd, reg = context
    plugin = root / '.codex/plugins/cache/team/docs/1.0'
    write(plugin / '.codex-plugin/plugin.json', {'name':'docs','skills':'./skills','mcpServers':{'docs':{'command':'PRIVATE'}}})
    skill(plugin / 'skills/query','query')
    write(root / '.codex/config.toml', '[plugins."docs@team"]\nenabled=false\n')
    inv = session_profiles.inventory(reg,'codex','main',str(cwd))
    assert inv['skills'] and inv['mcps']
    assert inv['skills'][0]['plugin'] == 'docs@team'
    assert inv['skills'][0]['enabled'] is False
    assert inv['mcps'][0]['enabled'] is False
    assert inv['mcps'][0]['effectiveNow'] is None
    assert not inv['mcps'][0]['toggleable']
    assert 'PRIVATE' not in json.dumps(inv)


def test_opencode_jsonc_project_mcp_and_shared_skill_detection(context):
    root,cwd,reg=context
    write(root / '.config/opencode/opencode.jsonc', '{//comment\n"mcp":{"docs":{"type":"remote","url":"https://PRIVATE","enabled":true,},},}')
    write(cwd.parent / 'opencode.json', {'mcp':{'docs':{'enabled':False}}})
    skill(cwd.parent / '.agents/skills/shared','shared')
    inv=session_profiles.inventory(reg,'opencode','main',str(cwd))
    assert inv['mcps'][0]['enabled'] is False
    assert inv['skills'][0]['name']=='shared'
    assert not inv['capabilities']['mcps']['supported']
    assert 'PRIVATE' not in json.dumps(inv)


def test_gemini_settings_skills_and_disabled_mcp(context):
    root,cwd,reg=context
    write(root/'.gemini/settings.json', {'mcpServers':{'docs':{},'other':{}},'mcp':{'excluded':['docs']},'skills':{'disabled':['query']}})
    skill(cwd/'.gemini/skills/query','query')
    inv=session_profiles.inventory(reg,'gemini','main',str(cwd))
    assert {r['name']:r['enabled'] for r in inv['mcps']} == {'docs':False,'other':True}
    assert inv['skills'][0]['enabled'] is False


def test_agy_uses_own_mcp_files_and_shell_has_no_extensions(context):
    root,cwd,reg=context
    write(root/'.gemini/config/mcp_config.json', {'mcpServers':{'global':{},'off':{'disabled':True}}})
    write(cwd/'.agents/mcp_config.json', {'mcpServers':{'local':{}}})
    inv=session_profiles.inventory(reg,'agy','main',str(cwd))
    assert {r['name']:r['enabled'] for r in inv['mcps']}=={'global':True,'local':True,'off':False}
    shell=session_profiles.inventory(reg,'shell','main',str(cwd))
    assert shell['status']=='unsupported' and shell['skills']==[] and shell['mcps']==[]
    acp=session_profiles.inventory(reg,'acp','main',str(cwd))
    assert acp['status']=='unknown' and acp['limitations']


def test_invalid_harness_or_account_never_falls_back(context):
    root,cwd,reg=context
    for h,a in [('unknown','main'),('codex','../../escape'),('opencode','work')]:
        with pytest.raises(ValueError):
            session_profiles.inventory(reg,h,a,str(cwd))


def test_claude_plugin_scope_and_disabled_entries(context):
    root,cwd,reg=context
    plugin=root/'plugin'
    write(plugin/'.claude-plugin/plugin.json',{'name':'docs','mcpServers':{'search':{'command':'PRIVATE'}}})
    skill(plugin/'skills/query','query')
    write(root/'.claude/plugins/installed_plugins.json',{'version':2,'plugins':{'docs@team':[{'scope':'user','installPath':str(plugin)}], 'other@team':[{'scope':'project','projectPath':str(root/'other'),'installPath':str(plugin)}]}})
    write(root/'.claude/settings.json',{'enabledPlugins':{'docs@team':True}})
    write(cwd/'.claude/settings.local.json',{'enabledPlugins':{'docs@team':False}})
    inv=session_profiles.inventory(reg,'claude','main',str(cwd))
    assert len(inv['mcps'])==1 and inv['mcps'][0]['enabled'] is False
    assert inv['skills'][0]['name']=='docs:query' and inv['skills'][0]['enabled'] is False


def test_grok_compat_off_and_disabled_skill_config(context):
    root,cwd,reg=context
    write(root/'.claude.json',{'mcpServers':{'foreign':{}}})
    write(root/'.grok/config.toml','[compat.claude]\nmcps=false\nskills=false\n[skills]\ndisabled=["native"]\n')
    skill(root/'.grok/skills/native','native')
    skill(root/'.claude/skills/foreign','foreign')
    inv=session_profiles.inventory(reg,'grok','main',str(cwd))
    assert not inv['mcps']
    assert [(r['name'],r['enabled']) for r in inv['skills']]==[('native',False)]


def test_codex_ignores_project_skill_selectors_and_directory_paths(context):
    root,cwd,reg=context
    p=skill(root/'.codex/skills/query','query')
    write(root/'.codex/config.toml',f'[[skills.config]]\npath={json.dumps(str(p.parent))}\nenabled=false\n')
    write(cwd/'.codex/config.toml','[[skills.config]]\nname="query"\nenabled=false\n')
    inv=session_profiles.inventory(reg,'codex','main',str(cwd))
    assert inv['skills'][0]['enabled'] is True
    assert any('skills.config' in text for text in inv['limitations'])


def test_codex_preserves_user_name_rule_when_project_array_is_present(context):
    root,cwd,reg=context
    p=skill(root/'.codex/skills/query','query')
    write(root/'.codex/config.toml','[[skills.config]]\nname="other"\nenabled=false\n')
    write(cwd/'.codex/config.toml','[[skills.config]]\nname="project-ignored"\nenabled=false\n')
    ident=session_profiles.inventory(reg,'codex','main',str(cwd))['skills'][0]['id']
    args=session_profiles.launch_args({'harness':'codex','skills':{ident:False}},reg,str(cwd),'')
    assert 'name="other",enabled=false' in args[1]
    assert 'project-ignored' not in args[1]


def test_codex_bundled_and_implicit_invocation_are_separate(context):
    root,cwd,reg=context
    system=skill(root/'.codex/skills/.system/base','base')
    manual=skill(root/'.codex/skills/manual','manual')
    write(manual.parent/'agents/openai.yaml','policy:\n  allow_implicit_invocation: false\n')
    write(root/'.codex/config.toml','[skills.bundled]\nenabled=false\n')
    rows={r['name']:r for r in session_profiles.inventory(reg,'codex','main',str(cwd))['skills']}
    assert rows['base']['enabled'] is False
    assert rows['manual']['enabled'] is True and rows['manual']['automaticInvocation'] is False


def test_gemini_extension_workspace_overrides_and_skill_precedence(context):
    root,cwd,reg=context
    ext=root/'.gemini/extensions/docs'
    write(ext/'gemini-extension.json',{'name':'docs','mcpServers':{'search':{}}})
    skill(ext/'skills/query','query')
    skill(cwd/'.agents/skills/query','query')
    write(root/'.gemini/extensions/extension-enablement.json',{'docs':{'overrides':['*','!'+str(cwd)+'/']}})
    inv=session_profiles.inventory(reg,'gemini','main',str(cwd))
    assert inv['mcps'][0]['enabled'] is False
    skills={r['source']:r for r in inv['skills']}
    assert skills['gemini-extension']['enabled'] is False
    assert skills['skills-directory']['enabled'] is True


def test_plugin_profile_selection_cannot_bypass_unsupported_row(context):
    root,cwd,reg=context
    plugin=root/'.codex/plugins/cache/team/docs/1.0'
    write(plugin/'.codex-plugin/plugin.json',{'name':'docs','mcpServers':{'search':{}}})
    write(root/'.codex/config.toml','[plugins."docs@team"]\nenabled=true\n')
    row=session_profiles.inventory(reg,'codex','main',str(cwd))['mcps'][0]
    with pytest.raises(ValueError,match='soportada'):
        session_profiles.launch_args({'harness':'codex','mcps':{row['id']:False}},reg,str(cwd),'')


def test_python310_toml_fallback_is_cached_and_never_retains_transport_secrets(context, monkeypatch):
    root,cwd,reg=context
    capabilities._TOML_CACHE.clear()
    monkeypatch.setattr(capabilities,'tomllib',None)
    calls=[]
    original=capabilities.subprocess.run
    def run(*a,**kw):
        calls.append(1)
        return original(*a,**kw)
    monkeypatch.setattr(capabilities.subprocess,'run',run)
    write(root/'.codex/config.toml','[mcp_servers.docs]\ncommand="PRIVATE_COMMAND"\nenv={TOKEN="PRIVATE_TOKEN"}\nenabled=false\n')
    first=session_profiles.inventory(reg,'codex','main',str(cwd))
    second=session_profiles.inventory(reg,'codex','main',str(cwd))
    assert first==second and len(calls)==1
    assert first['mcps'][0]['enabled'] is False
    assert 'PRIVATE' not in repr(capabilities._TOML_CACHE)
    capabilities._TOML_CACHE.clear()
    monkeypatch.setattr(capabilities.shutil,'which',lambda name:None)
    inv=session_profiles.inventory(reg,'codex','main',str(cwd))
    assert inv['status']=='incomplete' and not inv['capabilities']['mcps']['supported']


def test_multiple_project_declarations_and_symlink_config_deduplicate(context):
    root,cwd,reg=context
    write(root/'.codex/config.toml','[mcp_servers.docs]\nenabled=true\n')
    write(cwd.parent/'.codex/config.toml','[mcp_servers.docs]\nenabled=false\n')
    write(cwd/'.codex/config.toml','[mcp_servers.docs]\ndescription="Nearest definition"\n')
    inv=session_profiles.inventory(reg,'codex','main',str(cwd))
    row=inv['mcps'][0]
    assert row['enabled'] is False and len(row['declarations'])==3
    assert row['sourcePath']==str(cwd/'.codex/config.toml')


def test_grok_installed_plugin_registry_respects_disabled_name(context):
    root,cwd,reg=context
    plugin=root/'.grok/installed-plugins/repo/plugin'
    write(plugin/'.claude-plugin/plugin.json',{'name':'docs','mcpServers':{'search':{}}})
    write(root/'.grok/installed-plugins/registry.json',{'version':1,'repos':{'repo':{'path':str(plugin.parent),'plugins':{'docs':{'subdir':'plugin'}}}}})
    write(root/'.grok/config.toml','[plugins]\nenabled=["docs"]\ndisabled=["docs"]\n')
    inv=session_profiles.inventory(reg,'grok','main',str(cwd))
    assert inv['mcps'][0]['enabled'] is False and inv['mcps'][0]['source']=='grok-installed-plugin'


def test_malformed_json_shape_and_unknown_plugin_versions_are_explicit(context):
    root,cwd,reg=context
    write(root/'.gemini/settings.json',{'mcpServers':['invalid']})
    inv=session_profiles.inventory(reg,'gemini','main',str(cwd))
    assert inv['status']=='incomplete' and inv['errors']
    plugin=root/'.codex/plugins/cache/team/docs/1.0'
    write(plugin/'.codex-plugin/plugin.json',{'name':'docs','mcpServers':{'docs':{}}})
    inv=session_profiles.inventory(reg,'codex','main',str(cwd))
    assert inv['mcps'][0]['enabled'] is None
    assert inv['mcps'][0]['status']=='installed'
    assert inv['mcps'][0]['runtimeEnabled'] is None


def test_cyclic_skill_links_do_not_hang_or_duplicate(context):
    root,cwd,reg=context
    p=skill(root/'.agents/skills/query','query')
    (p.parent/'cycle').symlink_to(p.parent)
    alias=root/'.codex/skills/alias'
    alias.parent.mkdir(parents=True)
    alias.symlink_to(p.parent)
    inv=session_profiles.inventory(reg,'codex','main',str(cwd))
    assert len(inv['skills'])==1


def test_profile_reenable_one_of_duplicate_named_skills_preserves_name_rule(context):
    root,cwd,reg=context
    first=skill(root/'.codex/skills/query','query')
    second=skill(cwd/'.agents/skills/query','query')
    write(root/'.codex/config.toml','[[skills.config]]\nname="query"\nenabled=false\n')
    rows=session_profiles.inventory(reg,'codex','main',str(cwd))['skills']
    assert len(rows)==2 and all(r['enabled'] is False for r in rows)
    row=next(r for r in rows if r['path']==str(first))
    args=session_profiles.launch_args({'harness':'codex','skills':{row['id']:True}},reg,str(cwd),'')
    assert 'name="query",enabled=false' in args[1]
    assert str(first) in args[1] and str(second) not in args[1]


def test_invalid_enabled_type_is_not_reported_as_enabled(context):
    root,cwd,reg=context
    write(root/'.codex/config.toml','[mcp_servers.docs]\nenabled="false"\n')
    inv=session_profiles.inventory(reg,'codex','main',str(cwd))
    assert inv['status']=='incomplete'
    assert not inv['capabilities']['mcps']['supported']


def test_codex_ambiguous_skill_selector_is_not_applied(context):
    root,cwd,reg=context
    path=skill(root/'.codex/skills/query','query')
    write(root/'.codex/config.toml',f'[[skills.config]]\npath={json.dumps(str(path))}\nname="query"\nenabled=false\n')
    assert session_profiles.inventory(reg,'codex','main',str(cwd))['skills'][0]['enabled'] is True
