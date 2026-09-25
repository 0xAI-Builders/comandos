"""Provenance is bound to installation paths; token counts never mean usage."""
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
import extension_metadata as metadata


def test_installed_origin_does_not_leak_to_same_named_project_skill(tmp_path):
    root = tmp_path / '.agents/skills/grill-me'
    root.mkdir(parents=True)
    skill = root / 'SKILL.md'
    skill.write_text('hello world')
    (tmp_path / '.agents/.skill-lock.json').write_text(json.dumps({'skills': {
        'grill-me': {'source': 'mattpocock/skills', 'sourceType': 'github'}}}))
    origin = metadata.skill_origin({'path': str(skill), 'name': 'grill-me'}, tmp_path)
    assert origin['label'] == 'mattpocock/skills'
    project = tmp_path / 'project/grill-me/SKILL.md'
    project.parent.mkdir(parents=True)
    project.write_text('hello world')
    assert metadata.skill_origin({'path': str(project), 'name': 'grill-me'}, tmp_path)['kind'] == 'unknown'
    alias = tmp_path / 'alias'
    alias.symlink_to(root, target_is_directory=True)
    assert metadata.skill_origin({'path': str(alias/'SKILL.md'), 'name': 'renamed'}, tmp_path) == origin


def test_plugin_identity_has_priority_over_skill_name(tmp_path):
    origin = metadata.skill_origin({'name': 'grill-me', 'plugin': 'official@market'}, tmp_path)
    assert origin == {'id': 'plugin:official@market', 'label': 'official@market', 'kind': 'plugin'}


def test_full_file_counts_and_unreadable_remains_unknown(tmp_path, monkeypatch):
    skill = tmp_path/'SKILL.md'
    skill.write_text('hello world')
    monkeypatch.setattr(metadata, 'token_counts', lambda texts: [2 for text in texts])
    rows = [{'id':'ok','path':str(skill)}, {'id':'missing','path':str(tmp_path/'missing')}]
    result = metadata.skill_metadata(rows, tmp_path)
    assert result['ok']['size']['tokens'] == 2
    assert result['ok']['size']['basis'] == 'skill-file'
    assert result['ok']['size']['tokenizer'] == 'cl100k_base'
    assert result['missing']['size']['tokens'] is None
    assert 'hello world' not in json.dumps(result)


def test_schema_measurement_is_order_independent_and_pins_configuration(tmp_path, monkeypatch):
    monkeypatch.setattr(metadata, 'token_counts', lambda texts: [len(text) for text in texts])
    tools = [{'name':'b','inputSchema':{'type':'object'}}, {'name':'a','description':'docs','inputSchema':{}}]
    spec = {'url':'https://example.test/mcp','enabled':True}
    metadata.record_mcp_size(tmp_path, 'docs', spec, tools)
    first = metadata.mcp_size(tmp_path, 'docs', spec)
    assert first['tokens'] > 0 and first['basis'] == 'tool-definitions'
    metadata.record_mcp_size(tmp_path, 'docs', spec, list(reversed(tools)))
    assert metadata.mcp_size(tmp_path, 'docs', spec)['tokens'] == first['tokens']
    assert metadata.mcp_size(tmp_path, 'docs', {**spec,'url':'https://other.test'})['tokens'] is None
    stored = next((tmp_path/'.local/state/comandos/extensions/sizes').glob('*.json')).read_text()
    assert 'https://example' not in stored and 'inputSchema' not in stored


def test_paginated_tool_measurement_never_treats_a_partial_page_as_complete():
    capture = metadata.ToolListCapture()
    assert capture.add(None, 'p2', [{'name':'a'}]) is None
    assert capture.add('wrong', None, [{'name':'b'}]) is None
    assert capture.add('p2', None, [{'name':'b'}]) is None
    assert capture.add(None, 'p2', [{'name':'a'}]) is None
    assert capture.add('p2', None, [{'name':'b'}]) == [{'name':'a'},{'name':'b'}]
    assert capture.add(None, None, []) == []


def test_malformed_provenance_and_measurement_cache_remain_unknown(tmp_path):
    root = tmp_path/'.agents/skills/example'
    root.mkdir(parents=True)
    path = root/'SKILL.md'
    path.write_text('example')
    (tmp_path/'.agents/.skill-lock.json').write_text(json.dumps({'skills':{'example':[]}}))
    assert metadata.skill_origin({'path':str(path)},tmp_path)['kind'] == 'unknown'
    spec = {'url':'https://example.test'}
    cache = metadata._size_path(tmp_path,'test')
    cache.parent.mkdir(parents=True)
    for bad in [{'measuredAt':'yesterday'}, {'measuredAt':1}, {'measuredAt':None}]:
        cache.write_text(json.dumps({'configuration':metadata._digest(spec),'tokenizer':'cl100k_base','tokens':123,**bad}))
        assert metadata.mcp_size(tmp_path,'test',spec)['tokens'] is None
