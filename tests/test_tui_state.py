"""Native TUI state regressions. Fixtures contain no prompts or credentials."""
from types import SimpleNamespace
from pathlib import Path
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))
from test_agent_launch import load_dash_module


def test_claude_native_model_confirmation_without_custom_statusline(monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(dash, 'tmux', lambda *args: SimpleNamespace(returncode=0, stdout='❯ /model gpt-6-astra\n  ⎿  Set model to gpt-6-astra\n❯\n'))
    assert dash.claude_pane_model('%1') == 'gpt-6-astra'


def test_codex_open_model_menu_is_not_confirmation(monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(dash, 'tmux', lambda *args: SimpleNamespace(returncode=0, stdout='Select Model and Effort\n› 1. gpt-6-astra high\n  2. gpt-5.6-luna low\nPress enter to continue\n'))
    assert dash.codex_pane_model('%1') == ('', '')


def test_codex_assistant_prose_is_not_status(monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(dash, 'tmux', lambda *args: SimpleNamespace(returncode=0, stdout='Use gpt-6-astra high for this task.\n›\n'))
    assert dash.codex_pane_model('%1') == ('', '')


def test_new_conversation_evidence_beats_unchanged_footer():
    from tui_state import StateTracker
    tracker = StateTracker()
    launch = {'model': 'gpt-5.6-luna', 'effort': 'low'}
    old = {'model': 'gpt-5.6-luna', 'effort': 'low', 'revision': 'turn-1'}
    footer = {'model': 'gpt-5.6-luna', 'effort': 'low', 'kind': 'status'}
    tracker.observe('root', launch, old, footer, now=10)
    new = {'model': 'gpt-6-astra', 'effort': 'high', 'revision': 'turn-2'}
    assert tracker.observe('root', launch, new, footer, now=11)['model'] == 'gpt-6-astra'
    assert tracker.observe('root', launch, new, footer, now=12)['model'] == 'gpt-6-astra'
    selected = {'model': 'gpt-5.6-terra', 'effort': 'medium', 'kind': 'confirmation'}
    assert tracker.observe('root', launch, new, selected, now=13)['model'] == 'gpt-5.6-terra'
    assert tracker.observe('root', launch, new, {}, now=14)['model'] == 'gpt-5.6-terra'
    assert tracker.observe('new-conversation', {}, {}, {}, now=15)['model'] == ''


def test_transcript_cache_ignores_nested_agents_and_does_not_reread_idle(tmp_path, monkeypatch):
    import builtins, json
    from tui_state import TranscriptCache
    path = tmp_path / 'root.jsonl'
    rows = [dict(type='assistant', sessionId='root', timestamp='2026-09-11T10:00:00Z', effort='high', message={'model': 'claude-fable-5'}),
            dict(type='assistant', sessionId='child', isSidechain=True, message={'model': 'claude-haiku-4-5'})]
    path.write_text(''.join(json.dumps(r)+'\n' for r in rows))
    cache = TranscriptCache()
    value = cache.read('claude', 'root', path)
    assert value['model'] == 'claude-fable-5'
    assert value['effort'] == 'high'
    real_open = builtins.open
    def no_open(*args, **kwargs):
        raise AssertionError('idle transcript must not be opened')
    monkeypatch.setattr(builtins, 'open', no_open)
    assert cache.read('claude', 'root', path) == value
    monkeypatch.setattr(builtins, 'open', real_open)
    with path.open('a') as handle:
        handle.write(json.dumps(dict(type='assistant', sessionId='root', effort='low', message={'model': 'gpt-5.6-luna'}))+'\n')
    assert cache.read('claude', 'root', path)['model'] == 'gpt-5.6-luna'


def test_model_switch_does_not_inherit_old_effort():
    from tui_state import StateTracker
    tracker = StateTracker()
    tracker.observe('root', {}, {'model': 'claude-fable-5', 'effort': 'high'}, {}, now=10)
    got = tracker.observe('root', {}, {'model': 'claude-fable-5', 'effort': 'high'}, {'model': 'gpt-5.6-luna', 'kind': 'confirmation'}, now=11)
    assert got['model'] == 'gpt-5.6-luna'
    assert got['effort'] == ''


def test_observe_pane_uses_native_changes_and_ignores_stale_footer(tmp_path, monkeypatch):
    import json
    dash = load_dash_module()
    identity = dict(socket_path='sock', pid='1', server_start='2', session_id='$1', pane_id='%1', pane_pid='10', pane_current_command='claude')
    monkeypatch.setattr(dash, '_pane_identity', lambda *a: identity)
    monkeypatch.setattr(dash, '_process_start', lambda *a: 'born')
    monkeypatch.setattr(dash, 'account_for_pid', lambda *a: {'account': 'main'})
    monkeypatch.setattr(dash, '_proc_cmdline', lambda *a: ['claude'])
    path = tmp_path / 'projects' / 'project' / 'root.jsonl'
    path.parent.mkdir(parents=True)
    path.write_text(json.dumps(dict(type='assistant', sessionId='root', effort='high', message={'model': 'claude-fable-5'}))+'\n')
    screen = ['  ⎿  Set model to gpt-5.6-luna\n❯\n']
    monkeypatch.setattr(dash, 'tmux', lambda *a: SimpleNamespace(returncode=0, stdout=screen[0]))
    inspector = lambda *a: {'resume_id': 'root', 'claude_config_dir': str(tmp_path)}
    got = dash.observe_pane('test', '%1', {'agent': 'claude', 'pid': 123}, inspector)
    assert got['model'] == 'gpt-5.6-luna'
    assert got['effort'] == ''
    assert got['motor'] == 'codex'
    with path.open('a') as handle:
        handle.write(json.dumps(dict(type='assistant', sessionId='root', effort='medium', message={'model': 'claude-sonnet-5'}))+'\n')
    got = dash.observe_pane('test', '%1', {'agent': 'claude', 'pid': 123}, inspector)
    assert got['model'] == 'claude-sonnet-5'
    assert got['effort'] == 'medium'


def test_claude_effort_confirm_and_auto_reset():
    from tui_state import screen_state
    assert screen_state('claude', '❯ /effort low\n ⎿ Set effort level to low\n❯')['effort'] == 'low'
    assert screen_state('claude', ' ⎿ Effort level set to auto\n❯')['effort'] == 'auto'
    assert not screen_state('claude', '❯ /effort high\nChange effort level?\nYes, switch to high\n')
    assert not screen_state('claude', ' ⎿ Effort set to auto for this session, but CLAUDE_CODE_EFFORT_LEVEL=high still controls this session')


def test_older_claude_confirmation_before_assistant_response_is_ignored():
    from tui_state import screen_state
    text = '❯ /model gpt-5.6-luna\n ⎿ Set model to gpt-5.6-luna\n❯ question\n● Response from the newer model.\n❯\n'
    assert screen_state('claude', text) == {}


def test_claude_persisted_native_change_survives_dashboard_restart(tmp_path):
    import json
    from tui_state import TranscriptCache
    path = tmp_path / 'root.jsonl'
    rows = [dict(type='assistant', sessionId='root', effort='high', message={'model': 'claude-fable-5'}),
            dict(type='user', sessionId='root', message={'content': '<local-command-stdout>Set model to gpt-5.6-luna</local-command-stdout>'}),
            dict(type='user', sessionId='root', message={'content': '<local-command-stdout>Set effort level to low</local-command-stdout>'})]
    path.write_text(''.join(json.dumps(r)+'\n' for r in rows))
    got = TranscriptCache().read('claude', 'root', path)
    assert got['model'] == 'gpt-5.6-luna'
    assert got['effort'] == 'low'


def test_process_launch_arguments_are_not_live_confirmation(monkeypatch):
    dash = load_dash_module()
    identity = dict(pane_pid='10', pane_current_command='agy')
    monkeypatch.setattr(dash, '_pane_identity', lambda *a: identity)
    monkeypatch.setattr(dash, '_process_start', lambda *a: 'birth')
    monkeypatch.setattr(dash, '_proc_cmdline', lambda *a: ['agy', '--model', 'gemini-3.6-flash-high'])
    monkeypatch.setattr(dash, 'account_for_pid', lambda *a: {'account': 'main'})
    got = dash.observe_pane('test', '%1', {'agent': 'agy', 'pid': 123}, lambda *a: {})
    assert got['model'] == 'gemini-3.6-flash-high'
    assert got['source'] == 'process'
    assert got['confirmed'] is False
    assert got['limitations']


def test_native_login_and_logout_refresh_account_without_process_restart(tmp_path, monkeypatch):
    import json
    dash = load_dash_module()
    monkeypatch.setattr(dash, '_process_start', lambda *a: 'birth')
    monkeypatch.setattr(dash, '_read_environ', lambda *a: {b'GROK_HOME': str(tmp_path).encode()})
    auth = tmp_path / 'auth.json'
    auth.write_text(json.dumps({'profile': {'email': 'before@example.test'}}))
    assert dash.account_for_pid(999, 'grok')['accountEmail'] == 'before@example.test'
    auth.write_text(json.dumps({'profile': {'email': 'after@example.test'}}))
    assert dash.account_for_pid(999, 'grok')['accountEmail'] == 'after@example.test'
    auth.unlink()
    assert dash.account_for_pid(999, 'grok')['accountEmail'] == ''


def test_repeated_confirmation_after_disappearance_is_a_new_change():
    from tui_state import StateTracker
    tracker = StateTracker()
    pane_a = {'model': 'gpt-5.6-luna', 'kind': 'confirmation'}
    tracker.observe('root', {}, {}, pane_a, now=1)
    tracker.observe('root', {}, {'model': 'gpt-6-astra', 'revision': 'next'}, {}, now=2)
    got = tracker.observe('root', {}, {'model': 'gpt-6-astra', 'revision': 'next'}, pane_a, now=3)
    assert got['model'] == 'gpt-5.6-luna'


def test_cold_cache_finds_model_before_large_tool_output(tmp_path):
    import json
    from tui_state import TranscriptCache
    path = tmp_path / 'root.jsonl'
    path.write_text(json.dumps(dict(type='assistant', sessionId='root', effort='xhigh', message={'model': 'claude-opus-5'}))+'\n'+json.dumps(dict(type='user',sessionId='root',message={'content':'x'*400000}))+'\n')
    assert TranscriptCache().read('claude','root',path)['model'] == 'claude-opus-5'


def test_opencode_native_prompt_footer_is_live_but_picker_is_not():
    from tui_state import screen_state
    screen = ' ┃\n ┃ Ask anything...\n ┃\n ┃ Build · Claude Sonnet 4.6 Cloudflare AI Gateway · high\n ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀\n tab agents ctrl+p commands\n'
    got = screen_state('opencode', screen)
    assert got['model'] == 'claude-sonnet-4-6'
    assert got['effort'] == 'high'
    assert not screen_state('opencode', 'Select model\n› Claude Sonnet 4.6 Cloudflare AI Gateway\n')


def test_grok_metadata_idle_avoids_glob_and_json_reads(tmp_path, monkeypatch):
    import json, builtins, glob
    from tui_state import GrokMetadataCache
    summary = tmp_path / 'sessions' / '2026' / 'root' / 'summary.json'
    summary.parent.mkdir(parents=True)
    (tmp_path/'active_sessions.json').write_text(json.dumps([{'pid': 22, 'session_id': 'root'}, {'pid': 23, 'session_id': 'child'}]))
    summary.write_text(json.dumps({'current_model_id': 'grok-4.6', 'reasoning_effort': 'high'}))
    cache = GrokMetadataCache()
    got = cache.read(22, str(tmp_path))
    assert got['model'] == 'grok-4.6'
    def forbidden(*a, **kw):
        raise AssertionError('idle metadata must be stat-only')
    with monkeypatch.context() as patch:
        patch.setattr(builtins, 'open', forbidden)
        patch.setattr(glob, 'glob', forbidden)
        assert cache.read(22, str(tmp_path)) == got
    summary.write_text(json.dumps({'current_model_id': 'grok-4.6', 'reasoning_effort': 'low'}))
    assert cache.read(22, str(tmp_path))['effort'] == 'low'
    (tmp_path/'active_sessions.json').write_text('[]')
    assert cache.read(22, str(tmp_path)) == {}


def test_claude_native_display_label_and_combined_effort_confirmation():
    from tui_state import screen_state
    got = screen_state('claude', ' ⎿ Set model to `Opus 5 (1M context)` with xhigh effort\n❯')
    assert got['model'] == 'claude-opus-5[1m]'
    assert got['effort'] == 'xhigh'


def test_claude_custom_status_cannot_override_transcript_on_cold_start():
    from tui_state import StateTracker, screen_state
    visible=screen_state('claude','repo · claude · claude-sonnet-5\n')
    got=StateTracker().observe('root',{}, {'model':'gpt-6-astra','revision':'current'},visible,now=10)
    assert got['model']=='gpt-6-astra'


def test_unchanged_status_reappearing_after_picker_is_not_a_new_model_change():
    from tui_state import StateTracker
    tracker=StateTracker()
    footer={'model':'gpt-5.6-luna','effort':'low','kind':'status'}
    tracker.observe('root',{}, {'model':'gpt-5.6-luna','revision':'old'},footer,now=1)
    current={'model':'gpt-6-astra','effort':'high','revision':'new'}
    tracker.observe('root',{},current,{},now=2)
    assert tracker.observe('root',{},current,footer,now=3)['model']=='gpt-6-astra'


def test_transcript_preserves_effort_on_same_model_when_new_turn_omits_it(tmp_path):
    import json
    from tui_state import TranscriptCache
    rows=[{'type':'assistant','sessionId':'root','effort':'high','message':{'model':'claude-fable-5'}},
          {'type':'assistant','sessionId':'root','message':{'model':'claude-fable-5'}}]
    path=tmp_path/'root.jsonl'
    path.write_text(''.join(json.dumps(r)+'\n' for r in rows))
    assert TranscriptCache().read('claude','root',path)['effort']=='high'


def test_malformed_transcript_message_does_not_break_observation(tmp_path):
    import json
    from tui_state import TranscriptCache
    path=tmp_path/'root.jsonl'
    path.write_text(json.dumps({'type':'user','sessionId':'root','message':['malformed']})+'\n')
    assert TranscriptCache().read('claude','root',path)=={}


def test_native_provider_motor_is_not_inferred_from_its_model_family(monkeypatch):
    dash=load_dash_module()
    identity={'pane_pid':'10','pane_current_command':'opencode'}
    monkeypatch.setattr(dash,'_pane_identity',lambda *a:identity)
    monkeypatch.setattr(dash,'_process_start',lambda *a:'birth')
    monkeypatch.setattr(dash,'_proc_cmdline',lambda *a:['opencode'])
    monkeypatch.setattr(dash,'account_for_pid',lambda *a:{'account':'main'})
    monkeypatch.setattr(dash,'pane_visible_config',lambda *a:{'model':'gpt-5.6-luna','kind':'status'})
    got=dash.observe_pane('test','%1',{'agent':'opencode','pid':123},lambda *a:{})
    assert got['motor']=='opencode' and got['confirmed'] is True


def test_multiline_native_command_output_is_read_without_retaining_text(tmp_path):
    import json
    from tui_state import TranscriptCache
    path=tmp_path/'root.jsonl'
    path.write_text(json.dumps({'type':'user','sessionId':'root','message':{'content':'<local-command-stdout>\nSet model to gpt-5.6-luna\nSet effort level to high\n</local-command-stdout>'}})+'\n')
    cache=TranscriptCache()
    got=cache.read('claude','root',path)
    assert got['model']=='gpt-5.6-luna' and got['effort']=='high'
    assert 'local-command-stdout' not in repr(cache.entries)


def test_malformed_model_and_effort_values_are_not_runtime_configuration(tmp_path):
    import json
    from tui_state import TranscriptCache
    path=tmp_path/'root.jsonl'
    path.write_text(json.dumps({'type':'assistant','sessionId':'root','message':{'model':['private']},'effort':{'private':'text'}})+'\n')
    assert not TranscriptCache().read('claude','root',path).get('model')


def test_unknown_provider_does_not_invent_a_model_from_prose():
    from tui_state import screen_state
    for harness in ('grok','gemini','agy','shell','acp'):
        assert screen_state(harness,'Use gpt-6-astra high for this task.\n')=={}


def test_custom_status_script_is_not_live_confirmation(monkeypatch):
    dash=load_dash_module()
    monkeypatch.setattr(dash,'_pane_identity',lambda *a:{'pane_pid':'10','pane_current_command':'claude'})
    monkeypatch.setattr(dash,'_process_start',lambda *a:'birth')
    monkeypatch.setattr(dash,'_proc_cmdline',lambda *a:['claude'])
    monkeypatch.setattr(dash,'account_for_pid',lambda *a:{'account':'main'})
    monkeypatch.setattr(dash,'pane_visible_config',lambda *a:{'model':'claude-fable-5','kind':'custom-status'})
    got=dash.observe_pane('test','%1',{'agent':'claude','pid':123},lambda *a:{})
    assert got['source']=='status-script' and got['confirmed'] is False


def test_assistant_tool_output_cannot_masquerade_as_native_model_confirmation():
    from tui_state import screen_state
    text='● Example output from a script\n ⎿ Set model to gpt-5.6-luna\n❯\n'
    assert screen_state('claude',text)=={}
