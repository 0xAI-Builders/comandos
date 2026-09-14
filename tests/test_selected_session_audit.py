"""The control card must never operate an unrelated session or stale pane."""
import json
import subprocess

import pytest
from test_remote_ui import HTML, extract_js_function
from test_live_pane_inventory import _mock_state_reader
from test_agent_launch import load_dash_module


def select(items, selected='', active=None, selected_ts=20):
    source = extract_js_function(HTML, 'pickSel')
    script = f"""
const S={{sel:{json.dumps(selected)},selTs:{selected_ts}}};
function rowKey(i){{return i.session+(i.pane?'|'+i.pane:'');}}
function sidebarActiveTab(){{return {json.dumps(active or {})};}}
{source}
console.log(JSON.stringify(pickSel({json.dumps(items)})));
"""
    return json.loads(subprocess.check_output(['node', '-e', script], text=True))


def card(session='local', pane='%0', **kwargs):
    return dict(session=session, pane=pane, alive=True, status='idle', **kwargs)


@pytest.mark.parametrize('selected', ['signara|%21', 'local|%missing', ''])
def test_active_local_never_uses_unrelated_waiting_card(selected):
    items = [card('signara', '%21'), card()]
    items[0]['status'] = 'waiting'
    assert select(items, selected, {'session': 'local', 'pane': '%0', 'ts': .01}) == items[1]


def test_missing_active_session_has_no_operational_fallback():
    assert select([card('signara', '%21')], 'signara|%21', {'session': 'local'}) is None


def test_missing_exact_active_pane_waits_for_fresh_inventory():
    assert select([card(), card('signara', '%21')], '', {'session': 'local', 'pane': '%gone'}) is None


def test_live_row_wins_over_history_regardless_of_order():
    live = card()
    history = {**live, 'alive': False, 'status': 'waiting', 'operable': False}
    assert select([history, live], 'local|%0', {'session': 'local'}) == live


def test_newer_split_click_within_active_session_is_preserved():
    items = [card(), card(pane='%1')]
    assert select(items, 'local|%1', {'session':'local', 'pane':'%0', 'ts':.01}) == items[1]


def test_local_shell_is_a_live_card_even_when_old_hook_belongs_to_signara(tmp_path, monkeypatch):
    dash = load_dash_module()
    (tmp_path / 'old.json').write_text(json.dumps({'session':'local','project':'Signara',
        'agent':'claude','status':'waiting','cwd':'/old','ts':1}))
    panes = [{'session':'local','pane':'%0','pane_pid':100,'command':'zsh','cwd':'/old','activity':50}]
    _mock_state_reader(dash, tmp_path, monkeypatch, panes, [], {})
    live = [i for i in dash.read_states() if i['alive']]
    assert len(live) == 1
    assert live[0]['session'] == 'local' and live[0]['pane'] == '%0'
    assert live[0]['agent'] is None
    assert 'local' in live[0]['project'].lower()


def test_draft_pins_process_and_conversation_but_never_invents_them():
    script = """
const assert=require('node:assert/strict'), C=require('./dash/session-config.js');
const observedConfig={identity:'server/pane/process',conversationId:'thread-1'};
const d=C.draft({agent:'codex',observedConfig});
assert.equal(d.expectedIdentity,observedConfig.identity);
assert.equal(d.expectedConversationId,observedConfig.conversationId);
assert.equal(C.draft({}).expectedIdentity,undefined);
"""
    subprocess.run(['node', '-e', script], check=True)


@pytest.mark.parametrize('active', [True,False])
def test_chat_missing_local_card_cannot_fall_back_to_desktop_or_clear_draft(active):
    source = extract_js_function(HTML, 'opSend')
    script = f"""
const assert=require('node:assert/strict'), S={{list:[],sel:'local|%missing'}}, OP={{ac:null}};
const notices=[];
function pickSel(){{return null;}}
function sidebarActiveTab(){{return {json.dumps({'session':'local','pane':'%missing'} if active else {})};}}
function toast(t){{notices.push(t);}}
function tf(es){{return es;}}
function $(){{throw new Error('must preserve draft and avoid sending');}}
{source}
(async()=>{{assert.equal(await opSend('estado'),false);assert.equal(OP.ac,null);assert.equal(notices.length,1);}})().catch(e=>{{console.error(e);process.exitCode=1;}});
"""
    subprocess.run(['node', '-e', script], check=True)


def test_remote_terminal_focus_updates_selected_pane_without_desktop_focus():
    source = extract_js_function(HTML, 'rememberRemotePaneFocus')
    script = f"""
const assert=require('node:assert/strict'), remotePaneFocus=new Map();
{source}
rememberRemotePaneFocus([{{session:'local',pane:'%0',alive:true,paneActive:true}}]);
const first=remotePaneFocus.get('local');
rememberRemotePaneFocus([{{session:'local',pane:'%0',alive:true,paneActive:true}}]);
assert.equal(remotePaneFocus.get('local'),first);
rememberRemotePaneFocus([{{session:'local',pane:'%1',alive:true,paneActive:true}}]);
assert.equal(remotePaneFocus.get('local').pane,'%1');
rememberRemotePaneFocus([]);assert.equal(remotePaneFocus.size,0);
"""
    subprocess.run(['node', '-e', script], check=True)


def test_tmux_active_pane_only_comes_from_active_window(monkeypatch):
    from types import SimpleNamespace
    dash=load_dash_module()
    monkeypatch.setattr(dash,'tmux',lambda *args:SimpleNamespace(returncode=0,
        stdout='local|%0|100|zsh|/tmp|1|1|0\nlocal|%1|200|zsh|/tmp|1|1|1\n'))
    assert [(p['pane'],p['paneActive']) for p in dash.tmux_pane_inventory()] == [('%0',False),('%1',True)]
