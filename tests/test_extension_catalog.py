import importlib
import json
from pathlib import Path
import sys
import tomllib

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))


def module():
    return importlib.import_module('extension_catalog')


def test_sync_keeps_settings_permissions_and_is_idempotent(tmp_path):
    m = module()
    p = tmp_path / '.codex/config.toml'
    p.parent.mkdir()
    p.write_text('model="keep"\n# keep comment\n[mcp_servers.demo]\nurl="https://example.com"\n[mcp_servers.demo.tools.write]\napproval_mode="approve"\n')
    c = {'version': 1, 'servers': {'demo': {'command': '/bin/true', 'args': []}}}
    m.sync_configs(tmp_path, c, '/launcher')
    result = tomllib.loads(p.read_text())
    assert result['model'] == 'keep'
    assert '# keep comment' in p.read_text()
    assert result['mcp_servers']['demo']['tools']['write']['approval_mode'] == 'approve'
    assert result['mcp_servers']['demo']['command'] == '/launcher'
    before = p.read_bytes()
    assert not m.sync_configs(tmp_path, c, '/launcher')
    assert p.read_bytes() == before


def test_all_providers_and_future_account_receive_catalog(tmp_path):
    m = module()
    (tmp_path / '.claude-accounts/new').mkdir(parents=True)
    c = {'version': 1, 'servers': {'demo': {'command': 'echo', 'args': []}}}
    m.sync_configs(tmp_path, c, '/launcher')
    for target in m.targets(tmp_path):
        d = m.read_config(target['path'])
        assert 'demo' in d[target['key']]
    assert json.loads((tmp_path / '.claude-accounts/new/.claude.json').read_text())['mcpServers']['demo']['args'] == ['serve', 'demo']


def test_project_specific_endpoint_is_preserved(tmp_path):
    m = module()
    p = tmp_path / '.claude.json'
    d = {'mcpServers': {}, 'projects': {'/work': {'mcpServers': {'demo': {'type':'http','url':'https://different.example/mcp'}}}}}
    p.write_text(json.dumps(d))
    c = {'version': 1, 'servers': {'demo': {'url': 'https://example.com/mcp'}}}
    m.sync_configs(tmp_path, c, '/launcher')
    assert json.loads(p.read_text())['projects'] == d['projects']


def test_skill_sync_backs_up_conflicts_and_propagates_new_skill(tmp_path):
    m = module()
    for folder, body in [('.agents/skills/a','canonical'),('.claude/skills/a','other'),('.grok/skills/b','unique')]:
        p=tmp_path/folder;p.mkdir(parents=True);(p/'SKILL.md').write_text(body)
    m.sync_skills(tmp_path)
    assert (tmp_path/'.claude/skills/a').is_symlink()
    assert (tmp_path/'.claude/skills/a/SKILL.md').read_text() == 'canonical'
    assert (tmp_path/'.agents/skills/b/SKILL.md').read_text() == 'unique'
    assert (tmp_path/'.gemini/config/skills/b/SKILL.md').read_text() == 'unique'
    assert any(p.read_text()=='other' for p in (tmp_path/'.local/state/comandos/extensions/backups').rglob('SKILL.md'))
    assert not m.sync_skills(tmp_path)


def test_compare_write_rejects_concurrent_edit(tmp_path):
    m=module();p=tmp_path/'file';p.write_bytes(b'changed')
    with pytest.raises(m.CatalogError):m.replace_config(tmp_path,p,b'original',b'new')
    assert p.read_bytes()==b'changed'


def test_import_uses_remote_browser_and_preserves_disabled(tmp_path):
    m=module();p=tmp_path/'.codex/config.toml';p.parent.mkdir()
    p.write_text('[mcp_servers.chrome-bg]\ncommand="old-local-browser"\n[mcp_servers.off]\ncommand="echo"\nenabled=false\n')
    c=m.import_catalog(tmp_path)
    assert c['servers']['chrome-bg']['command']==str(tmp_path/'.local/bin/cc-browser-remote')
    assert c['servers']['off']['enabled'] is False


def test_native_edit_and_removal_propagate_without_overwriting_other_settings(tmp_path):
    m=module();c={'version':1,'servers':{'demo':{'command':'echo','args':[],'enabled':True}}}
    launcher='/launcher'
    m.sync_configs(tmp_path,c,launcher)
    m.save_snapshot(tmp_path,c,launcher)
    p=tmp_path/'.claude.json';d=json.loads(p.read_text())
    d['mcpServers']['demo']={'command':'new-command','args':['hello']};p.write_text(json.dumps(d))
    revised=m.reconcile(tmp_path,c,launcher)
    assert revised['servers']['demo']['command']=='new-command'
    m.sync_configs(tmp_path,revised,launcher);m.save_snapshot(tmp_path,revised,launcher)
    d=json.loads(p.read_text());del d['mcpServers']['demo'];p.write_text(json.dumps(d))
    revised=m.reconcile(tmp_path,revised,launcher)
    assert revised['servers']['demo']['enabled'] is False


def test_canonical_skill_pointing_into_provider_does_not_form_cycle(tmp_path):
    m=module();source=tmp_path/'.claude/skills/demo';source.mkdir(parents=True);(source/'SKILL.md').write_text('hello')
    root=tmp_path/'.agents/skills';root.mkdir(parents=True);(root/'demo').symlink_to(source)
    m.sync_skills(tmp_path)
    assert (root/'demo/SKILL.md').read_text()=='hello'
    assert (source/'SKILL.md').read_text()=='hello'


def test_import_prefers_existing_local_service_over_remote_duplicate(tmp_path):
    m=module();(tmp_path/'.claude.json').write_text(json.dumps({'mcpServers':{'gmail':{'type':'http','url':'https://example.com/mcp'}}}))
    p=tmp_path/'.codex/config.toml';p.parent.mkdir();p.write_text('[mcp_servers.gmail]\nurl="http://127.0.0.1:7000/mcp"\n')
    assert m.import_catalog(tmp_path)['servers']['gmail']['url']=='http://127.0.0.1:7000/mcp'


def test_new_account_definitions_are_imported_before_export(tmp_path):
    m=module();c={'version':1,'servers':{'one':{'command':'echo','args':[],'enabled':True}}}
    m.sync_configs(tmp_path,c,'/launcher');m.save_snapshot(tmp_path,c,'/launcher')
    p=tmp_path/'.claude-accounts/new/.claude.json';p.parent.mkdir(parents=True)
    p.write_text(json.dumps({'mcpServers':{'extra':{'command':'extra-command'}}}))
    assert m.reconcile(tmp_path,c,'/launcher')['servers']['extra']['command']=='extra-command'


def test_disabled_server_keeps_native_approval_policy_when_reenabled(tmp_path):
    m=module();p=tmp_path/'.codex/config.toml';p.parent.mkdir()
    p.write_text('[mcp_servers.demo]\ncommand="echo"\n[mcp_servers.demo.tools.write]\napproval_mode="approve"\n')
    c={'version':1,'servers':{'demo':{'command':'echo','args':[],'enabled':False}}}
    m.sync_configs(tmp_path,c,'/launcher')
    c['servers']['demo']['enabled']=True
    m.sync_configs(tmp_path,c,'/launcher')
    assert m.read_config(p)['mcp_servers']['demo']['tools']['write']['approval_mode']=='approve'


def test_provider_root_alias_cannot_corrupt_canonical_skill(tmp_path):
    m=module();p=tmp_path/'.agents/skills/demo';p.mkdir(parents=True);(p/'SKILL.md').write_text('hello')
    alias=tmp_path/'.claude/skills';alias.parent.mkdir();alias.symlink_to(p.parent)
    m.sync_skills(tmp_path)
    assert (p/'SKILL.md').read_text()=='hello'
    assert not p.is_symlink()


def test_relocated_skill_preserves_relative_resource_link(tmp_path):
    m=module();root=tmp_path/'.claude/skills';s=root/'demo';s.mkdir(parents=True)
    (s/'SKILL.md').write_text('hello');(root/'common').mkdir();(root/'common/data').write_text('resource')
    (s/'resources').symlink_to('../common')
    m.sync_skills(tmp_path)
    assert (tmp_path/'.agents/skills/demo/resources/data').read_text()=='resource'


def test_snapshot_refuses_intervening_native_edit(tmp_path):
    m=module();c={'version':1,'servers':{'demo':{'command':'echo','args':[]}}};observed={}
    m.sync_configs(tmp_path,c,'/launcher',observed=observed)
    p=tmp_path/'.claude.json';d=json.loads(p.read_text());d['mcpServers']['demo']['command']='new';p.write_text(json.dumps(d))
    with pytest.raises(m.CatalogError):m.save_snapshot(tmp_path,c,'/launcher',expected_targets=observed)
    assert json.loads(p.read_text())['mcpServers']['demo']['command']=='new'


def test_reinstalled_native_skill_updates_all_clients(tmp_path):
    m=module();s=tmp_path/'.agents/skills/demo';s.mkdir(parents=True);(s/'SKILL.md').write_text('old')
    m.sync_skills(tmp_path)
    source=tmp_path/'.grok/skills/demo';source.unlink();source.mkdir();(source/'SKILL.md').write_text('new')
    m.sync_skills(tmp_path)
    assert (s/'SKILL.md').read_text()=='new'
    assert (tmp_path/'.claude/skills/demo/SKILL.md').read_text()=='new'


def test_conflicting_skill_reinstalls_preserve_both_copies(tmp_path):
    m=module();s=tmp_path/'.agents/skills/demo';s.mkdir(parents=True);(s/'SKILL.md').write_text('old')
    m.sync_skills(tmp_path)
    for folder,text in [('.grok/skills','one'),('.claude/skills','two')]:
        p=tmp_path/folder/'demo';p.unlink();p.mkdir();(p/'SKILL.md').write_text(text)
    with pytest.raises(m.CatalogError):m.sync_skills(tmp_path)
    assert (tmp_path/'.grok/skills/demo/SKILL.md').read_text()=='one'
    assert (tmp_path/'.claude/skills/demo/SKILL.md').read_text()=='two'


def test_export_rejects_edit_after_reconciliation(tmp_path):
    m=module();c={'version':1,'servers':{'demo':{'command':'echo','args':[]}}};inputs={}
    m.sync_configs(tmp_path,c,'/launcher');m.save_snapshot(tmp_path,c,'/launcher')
    revised=m.reconcile(tmp_path,c,'/launcher',observed_inputs=inputs)
    p=tmp_path/'.claude.json';d=json.loads(p.read_text());d['mcpServers']['demo']['command']='new';p.write_text(json.dumps(d))
    with pytest.raises(m.CatalogError):m.sync_configs(tmp_path,revised,'/launcher',expected_inputs=inputs)
    assert json.loads(p.read_text())['mcpServers']['demo']['command']=='new'


def test_grok_native_disable_list_updates_shared_state(tmp_path):
    m=module();c={'version':1,'servers':{'demo':{'command':'echo','args':[],'enabled':True}}}
    m.sync_configs(tmp_path,c,'/launcher');m.save_snapshot(tmp_path,c,'/launcher')
    p=tmp_path/'.grok/config.toml';d=m.read_config(p);d['disabled_mcp_servers']=['demo'];p.write_text(m.tomlkit.dumps(d))
    assert m.reconcile(tmp_path,c,'/launcher')['servers']['demo']['enabled'] is False
