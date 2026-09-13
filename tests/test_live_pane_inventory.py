"""Card membership comes from live panes, never from legacy hook files."""
import json
from types import SimpleNamespace

from test_agent_launch import load_dash_module


def test_inventory_emits_all_mixed_splits_and_shells_once(tmp_path, monkeypatch):
    dash = load_dash_module()
    panes = [
        {'session': 'mixed', 'pane': '%1', 'pane_pid': 100, 'command': 'node', 'cwd': '/same', 'activity': 50},
        {'session': 'mixed', 'pane': '%2', 'pane_pid': 200, 'command': 'claude', 'cwd': '/same', 'activity': 50},
        {'session': 'mixed', 'pane': '%3', 'pane_pid': 300, 'command': 'grok', 'cwd': '/same', 'activity': 50},
        {'session': 'mixed', 'pane': '%4', 'pane_pid': 400, 'command': 'grok', 'cwd': '/same', 'activity': 50},
        {'session': 'other', 'pane': '%5', 'pane_pid': 500, 'command': 'node', 'cwd': '/same', 'activity': 40},
        {'session': 'terminal', 'pane': '%6', 'pane_pid': 600, 'command': 'zsh', 'cwd': '/shell', 'activity': 30},
        {'session': 'ssh-host', 'pane': '%7', 'pane_pid': 700, 'command': 'ssh', 'cwd': '/shell', 'activity': 20},
    ]
    procs = [(101, '/same', 'codex'), (201, '/same', 'claude'), (301, '/same', 'grok'),
             (401, '/same', 'grok'), (501, '/same', 'codex'),
             (601, '/shell', 'claude')]  # a background agent must not replace the shell
    for name, record in {
        'exact': {'session': 'mixed', 'pane': '%3', 'agent': 'grok', 'status': 'waiting', 'detail': 'exact question', 'ts': 10, 'cwd': '/stale-cwd'},
        'duplicate': {'session': 'mixed', 'pane': '%3', 'agent': 'grok', 'status': 'working', 'ts': 5},
        'stale': {'session': 'mixed', 'pane': '%99', 'agent': 'claude', 'status': 'working', 'ts': 99},
        'legacy': {'session': 'mixed', 'agent': 'grok', 'status': 'waiting', 'detail': 'ambiguous question', 'ts': 100},
        'nested': {'session': 'other', 'pane': '%5', 'agent': 'claude', 'status': 'waiting', 'detail': 'delegated question', 'ts': 100},
    }.items():
        (tmp_path / (name + '.json')).write_text(json.dumps(record))
    monkeypatch.setattr(dash, 'STATE', str(tmp_path))
    monkeypatch.setattr(dash, 'tmux_pane_inventory', lambda: panes)
    monkeypatch.setattr(dash, 'agent_procs', lambda: procs)
    monkeypatch.setattr(dash, 'parent_pid', lambda pid: pid - 1 if pid % 100 == 1 else 0)
    monkeypatch.setattr(dash, '_proc_cmdline', lambda pid: ['native-cli'])
    monkeypatch.setattr(dash.pane_snapshot, 'PaneInspector', lambda: object())
    monkeypatch.setattr(dash, 'session_labels', lambda: {'mixed': 'Project'})
    monkeypatch.setattr(dash, 'pane_status_hint', lambda *args: '')
    monkeypatch.setattr(dash, 'claude_pane_busy', lambda pane: '')
    monkeypatch.setattr(dash, 'account_for_pid', lambda *args: {'account': 'main'})
    monkeypatch.setattr(dash, 'grok_metadata_for_pid', lambda pid: {})
    monkeypatch.setattr(dash, 'reconcile_card_config', lambda item, *args: item)
    monkeypatch.setattr(dash, 'ssh_state', lambda session: 'mux')
    monkeypatch.setattr(dash, '_suggestion_context', lambda: ({}, set(), {}))
    monkeypatch.setattr(dash, '_annotate_suggestion', lambda *args: None)
    monkeypatch.setattr(dash, 'write_app_tab_models', lambda items: None)
    items = dash.read_states()
    live_items = [item for item in items if item['alive']]
    assert len(live_items) == len(panes)
    cards = {item['pane']: item for item in live_items}
    assert set(cards) == {'%1', '%2', '%3', '%4', '%5', '%6', '%7'}
    assert [cards[p]['agent'] for p in ('%1', '%2', '%3', '%4', '%5')] == ['codex', 'claude', 'grok', 'grok', 'codex']
    assert cards['%3']['detail'] == 'exact question'
    assert cards['%4']['detail'] == ''  # ambiguous hook not copied across splits
    assert cards['%5']['detail'] == ''  # delegated Claude hook cannot annotate Codex
    assert cards['%6']['agent'] is None and cards['%7']['sshConnected'] is True
    assert all(item.get('pane') for item in items if item['alive'])


def test_codex_wrapper_selects_native_child_not_nested_agent(monkeypatch):
    dash = load_dash_module()
    pane = {'session': 'mixed', 'pane': '%1', 'pane_pid': 100, 'command': 'node', 'cwd': '/repo'}
    parents = {110: 100, 120: 110, 130: 120, 140: 120}
    argv = {110: ['node', '/bin/codex'], 120: ['/native/codex'], 130: ['claude'], 140: ['codex']}
    monkeypatch.setattr(dash, 'parent_pid', lambda pid: parents.get(pid, 0))
    monkeypatch.setattr(dash, '_proc_cmdline', lambda pid: argv.get(pid, []))
    by_session, by_cwd = dash.agent_pane_maps([(110, '/repo', 'codex'), (120, '/repo', 'codex'),
                                            (130, '/repo', 'claude'), (140, '/repo', 'codex')], [pane])
    assert by_session['mixed']['pid'] == 120
    assert len(by_cwd['/repo']) == 1


def test_exact_hook_in_existing_session_cannot_create_missing_alive_pane(tmp_path, monkeypatch):
    dash = load_dash_module()
    (tmp_path / 'stale.json').write_text(json.dumps({'session': 'terminal', 'pane': '%old', 'agent': 'claude', 'status': 'working'}))
    monkeypatch.setattr(dash, 'STATE', str(tmp_path))
    monkeypatch.setattr(dash, 'tmux_pane_inventory', lambda: [{'session': 'terminal', 'pane': '%1', 'pane_pid': 100, 'command': 'zsh', 'cwd': '/repo', 'activity': 1}])
    monkeypatch.setattr(dash, 'agent_procs', lambda: [])
    monkeypatch.setattr(dash.pane_snapshot, 'PaneInspector', lambda: object())
    monkeypatch.setattr(dash, 'session_labels', lambda: {})
    monkeypatch.setattr(dash, 'pane_status_hint', lambda *args: '')
    monkeypatch.setattr(dash, '_suggestion_context', lambda: ({}, set(), {}))
    monkeypatch.setattr(dash, '_annotate_suggestion', lambda *args: None)
    monkeypatch.setattr(dash, 'write_app_tab_models', lambda items: None)
    items = dash.read_states()
    assert len(items) == 1 and items[0]['pane'] == '%1' and items[0]['agent'] is None


def test_tmux_inventory_contains_plain_shell_and_ssh_without_agent_records(monkeypatch):
    dash = load_dash_module()
    monkeypatch.setattr(dash, 'tmux', lambda *args: SimpleNamespace(returncode=0,
        stdout='term|%7|100|zsh|/repo|20\nssh-host|%8|200|ssh|/home/user|30\n'))
    assert [(row['pane'], row['command']) for row in dash.tmux_pane_inventory()] == [('%7', 'zsh'), ('%8', 'ssh')]


def _mock_state_reader(dash, tmp_path, monkeypatch, panes, procs, parents):
    monkeypatch.setattr(dash, 'STATE', str(tmp_path))
    monkeypatch.setattr(dash, 'tmux_pane_inventory', lambda: panes)
    monkeypatch.setattr(dash, 'agent_procs', lambda: procs)
    monkeypatch.setattr(dash, 'parent_pid', lambda pid: parents.get(pid, 0))
    monkeypatch.setattr(dash, '_proc_cmdline', lambda pid: ['native-cli'])
    monkeypatch.setattr(dash.pane_snapshot, 'PaneInspector', lambda: object())
    monkeypatch.setattr(dash, 'session_labels', lambda: {})
    monkeypatch.setattr(dash, 'pane_status_hint', lambda *args: '')
    monkeypatch.setattr(dash, 'claude_pane_busy', lambda pane: '')
    monkeypatch.setattr(dash, 'account_for_pid', lambda *args: {'account': 'main'})
    monkeypatch.setattr(dash, 'reconcile_card_config', lambda item, *args: item)
    monkeypatch.setattr(dash, '_suggestion_context', lambda: ({}, set(), {}))
    monkeypatch.setattr(dash, '_annotate_suggestion', lambda *args: None)
    monkeypatch.setattr(dash, 'write_app_tab_models', lambda items: None)


def test_tmux_wrappers_and_delegates_cannot_keep_old_waiting_hook_external(tmp_path, monkeypatch):
    dash = load_dash_module()
    (tmp_path / 'old.json').write_text(json.dumps({'session': 'old-session', 'pane': '%99', 'agent': 'claude',
        'cwd': '/repo', 'status': 'waiting', 'detail': 'historical question', 'ts': 1}))
    panes = [{'session': 'current', 'pane': '%1', 'pane_pid': 100, 'command': 'node', 'cwd': '/repo', 'activity': 50}]
    procs = [(110, '/repo', 'codex'), (120, '/repo', 'codex'), (130, '/repo', 'claude')]
    _mock_state_reader(dash, tmp_path, monkeypatch, panes, procs, {110:100, 120:110, 130:120})
    items = dash.read_states()
    old = next(item for item in items if item['session'] == 'old-session')
    assert old['alive'] is False and old['external'] is False
    assert old['status'] == 'dead' and old['detail'] == 'historical question'
    assert old['operable'] is False


def test_waiting_hook_survives_as_history_when_pane_becomes_shell(tmp_path, monkeypatch):
    dash = load_dash_module()
    (tmp_path / 'waiting.json').write_text(json.dumps({'session': 'current', 'pane': '%1', 'agent': 'claude',
        'cwd': '/repo', 'status': 'waiting', 'detail': 'keep this unanswered question', 'ts': dash.time.time()}))
    panes = [{'session': 'current', 'pane': '%1', 'pane_pid': 100, 'command': 'zsh', 'cwd': '/repo', 'activity': 50}]
    _mock_state_reader(dash, tmp_path, monkeypatch, panes, [], {})
    items = dash.read_states()
    live = [item for item in items if item['alive']]
    history = [item for item in items if not item['alive']]
    assert len(live) == 1 and live[0]['pane'] == '%1' and live[0]['agent'] is None
    assert len(history) == 1 and history[0]['detail'] == 'keep this unanswered question'
    assert history[0]['status'] == 'waiting' and history[0]['operable'] is False
    assert history[0]['previousPane'] == '%1' and not history[0].get('pane')


def test_unmapped_external_agent_keeps_its_waiting_hook_external(tmp_path, monkeypatch):
    dash = load_dash_module()
    (tmp_path / 'external.json').write_text(json.dumps({'session': 'external', 'agent': 'claude',
        'cwd': '/external', 'status': 'waiting', 'detail': 'external question', 'ts': 1}))
    _mock_state_reader(dash, tmp_path, monkeypatch, [], [(900, '/external', 'claude')], {900:899, 899:1})
    items = dash.read_states()
    assert len(items) == 1 and items[0]['external'] is True and items[0]['status'] == 'waiting'
    assert items[0]['alive'] is False


def test_external_codex_child_claude_does_not_resurrect_claude_hook(tmp_path, monkeypatch):
    dash = load_dash_module()
    (tmp_path / 'old.json').write_text(json.dumps({'session': 'old-claude', 'agent': 'claude',
        'cwd': '/external', 'status': 'waiting', 'detail': 'old question', 'ts': 1}))
    procs = [(900, '/external', 'codex'), (910, '/external', 'claude')]
    _mock_state_reader(dash, tmp_path, monkeypatch, [], procs, {910:900, 900:899, 899:1})
    items = dash.read_states()
    assert len(items) == 1 and items[0]['external'] is False and items[0]['status'] == 'dead'
