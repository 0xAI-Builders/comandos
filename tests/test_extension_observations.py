"""Only exact-conversation tool records count as observed extension use."""
import json
import sqlite3
import sys
import pytest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))

INV = {'mcps': [{'id': 'docs', 'name': 'docs'}, {'id': 'other', 'name': 'other'}],
       'skills': [{'id': 'shared:tdd', 'name': 'tdd', 'path': '/skills/tdd/SKILL.md'}]}


def usage(harness, sid, path, **kwargs):
    import extension_observations
    return extension_observations.conversation_usage(harness, sid, path, INV, **kwargs)


def write(path, records):
    path.write_text('\n'.join(json.dumps(x) for x in records) + '\n')
    return path


def test_claude_deduplicates_tool_ids_and_does_not_count_mentions_or_other_sessions(tmp_path):
    call = {'type': 'assistant', 'sessionId': 'exact', 'message': {'content': [
        {'type': 'tool_use', 'id': 'one', 'name': 'mcp__docs__search', 'input': {}},
        {'type': 'tool_use', 'id': 'two', 'name': 'Skill', 'input': {'skill': 'tdd'}}]}}
    p = write(tmp_path/'session.jsonl', [{'type': 'user', 'sessionId':'exact'}, call, call,
        {'type': 'assistant', 'sessionId': 'different', 'message': {'content': [
            {'type': 'tool_use', 'id': 'other', 'name': 'mcp__other__search'}]}},
        {'type': 'user', 'sessionId': 'exact', 'message': {'content': 'mcp__other__search SKILL.md'}}])
    result = usage('claude', 'exact', p)
    assert result['counts'] == {'mcps': {'docs': 1, 'other': 0}, 'skills': {'shared:tdd': 1}}
    assert result['complete'] and result['turns'] == 2
    assert 'input' not in json.dumps(result)


def test_missing_or_partial_sources_never_claim_zero_usage(tmp_path):
    assert usage('codex', 'exact', tmp_path/'missing')['counts']['mcps']['docs'] is None
    p = write(tmp_path/'large', [{'type':'user'}, {'type':'padding','data':'x'*2000}])
    result = usage('claude', 'exact', p, max_bytes=200)
    assert not result['complete']
    assert result['counts']['skills']['shared:tdd'] is None


def test_codex_wrapper_code_does_not_prove_a_tool_was_called(tmp_path):
    p = write(tmp_path/'rollout', [
        {'type':'session_meta','payload':{'id':'exact','source':'cli'}},
        {'type':'response_item','payload':{'type':'function_call','name':'mcp__docs__search','call_id':'a','arguments':'{}'}},
        {'type':'response_item','payload':{'type':'function_call','name':'exec','call_id':'b','arguments':'tools.mcp__other__search({})'}},
    ])
    result = usage('codex', 'exact', p)
    assert result['counts']['mcps']['docs'] == 1
    assert result['counts']['mcps']['other'] is None
    assert not result['complete']


def test_opencode_sqlite_filters_conversation_and_skill_calls(tmp_path):
    p = tmp_path/'opencode.db'
    with sqlite3.connect(p) as db:
        db.execute('create table part (id text, session_id text, data text)')
        for ident, sid, tool, inp in [('a','exact','docs_search',{}),('b','elsewhere','other_search',{}),('c','exact','skill',{'name':'tdd'})]:
            db.execute('insert into part values (?,?,?)',(ident,sid,json.dumps({'type':'tool','tool':tool,'callID':ident,'state':{'status':'completed','input':inp}})))
    result = usage('opencode','exact',p)
    assert result['counts']['mcps']['docs'] == 1
    assert result['counts']['mcps']['other'] == 0
    assert result['counts']['skills']['shared:tdd'] == 1


def test_agy_records_explicit_mcp_and_skill_file_reads(tmp_path):
    p = write(tmp_path/'transcript', [{'step_index':1,'type':'TOOL_CALL','tool_calls':[
        {'name':'call_mcp_tool','args':{'ServerName':'docs','ToolName':'search'}},
        {'name':'view_file','args':{'AbsolutePath':'/skills/tdd/SKILL.md'}}]}])
    result=usage('agy','exact',p)
    assert result['counts']['mcps']['docs']==1
    assert result['counts']['skills']['shared:tdd']==1
    assert result['counts']['mcps']['other'] is None


def test_cache_invalidates_when_session_file_changes(tmp_path):
    p=write(tmp_path/'session',[{'type':'user','sessionId':'exact'}])
    assert usage('claude','exact',p)['counts']['mcps']['docs']==0
    with p.open('a') as f:
        f.write(json.dumps({'type':'assistant','sessionId':'exact','message':{'content':[
            {'type':'tool_use','id':'new','name':'mcp__docs__search','input':{}}]}})+'\n')
    assert usage('claude','exact',p)['counts']['mcps']['docs']==1


def test_sqlite_budget_marks_unobserved_tools_unknown(tmp_path):
    p=tmp_path/'opencode.db'
    with sqlite3.connect(p) as db:
        db.execute('create table part (id text, session_id text, data text)')
        db.execute('insert into part values (?,?,?)',('a','exact',json.dumps({'type':'text','text':'x'*2000})))
    result=usage('opencode','exact',p,max_bytes=100)
    assert not result['complete']
    assert result['counts']['mcps']['docs'] is None


def test_grok_completed_mcp_call_counts_but_partial_coverage_stays_unknown(tmp_path):
    p=write(tmp_path/'events',[{'type':'mcp_tool_call_completed','data':{'server_name':'docs','tool_name':'search','tool_call_id':'call-1'}}])
    result=usage('grok','exact',p)
    assert result['counts']['mcps']['docs']==1
    assert result['counts']['mcps']['other'] is None


@pytest.mark.parametrize('block', [
    {'type':'tool_use'},
    {'type':'tool_use','name':'Skill','input':{}},
    {'type':'tool_use','name':'Skill','input':{'skill':[]}},
    {'type':'tool_use','name':'mcp__'},
])
def test_incomplete_claude_tool_records_keep_positive_counts_without_false_zeros(tmp_path, block):
    p=write(tmp_path/'session', [{'type':'user'}, {'type':'assistant','message':{'content':[
        {'type':'tool_use','id':'observed','name':'mcp__docs__search'}, block]}}])
    result=usage('claude','exact',p)
    assert result['counts']['mcps']['docs']==1
    assert result['counts']['mcps']['other'] is None
    assert not result['complete']


@pytest.mark.parametrize('part', [
    {'type':'tool'},
    {'type':'tool','tool':'skill','state':{'status':'completed','input':{}}},
    {'type':'tool','tool':'docs_search','state':{'status':'pending','input':{}}},
    {'type':'tool','tool':'docs_search','state':{'input':{}}},
])
def test_incomplete_opencode_records_do_not_prove_invocation_or_zero(tmp_path, part):
    p=tmp_path/'opencode.db'
    with sqlite3.connect(p) as db:
        db.execute('create table part (id text, session_id text, data text)')
        db.execute('insert into part values (?,?,?)', ('a','exact',json.dumps(part)))
    result=usage('opencode','exact',p)
    assert not result['complete']
    assert result['counts']['mcps']['docs'] is None
