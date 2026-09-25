"""Pane controls use a private tmux server; never touch the user's panes."""
import importlib
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))


@pytest.fixture
def panes(tmp_path):
    command = ['tmux', '-S', str(tmp_path / 'tmux.sock'), '-f', '/dev/null']
    def tmux(*args):
        return subprocess.run(command + list(args), capture_output=True, text=True, timeout=4)
    assert tmux('new-session', '-d', '-s', 'fixture', '-x', '120', '-y', '40', 'sleep 900').returncode == 0
    assert tmux('split-window', '-h', '-t', '=fixture:', 'sleep 900').returncode == 0
    def identity(session, pane):
        fields = ('pid', 'session_id', 'session_name', 'pane_id', 'pane_pid')
        result = tmux('display-message', '-p', '-t', pane, '\t'.join('#{' + k + '}' for k in fields))
        parts = result.stdout.strip().split('\t')
        if result.returncode or len(parts) != len(fields):
            raise ValueError('El panel ya no existe')
        data = dict(zip(fields, parts))
        if data['session_name'] != session:
            raise ValueError('El panel no pertenece a esta sesión')
        return data
    snapshots = []
    def save(session, pane):
        snapshots.append((session, pane))
        return 'saved'
    try:
        module = importlib.import_module('terminal_panes')
        yield lambda data: module.execute(tmux, identity, save, data), tmux, snapshots
    finally:
        tmux('kill-server')


def test_close_targets_confirmed_pane_after_focus_changes(panes):
    execute, tmux, snapshots = panes
    listing = execute({'session': 'fixture'})
    target, survivor = listing['panes']
    tmux('select-pane', '-t', survivor['id'])
    result = execute({'session': 'fixture', 'action': 'close', 'pane': target['id'], 'identity': target['identity']})
    assert result['ok'] and result['closed'] == target['id']
    assert [p['id'] for p in result['panes']] == [survivor['id']]
    assert snapshots == [('fixture', target['id'])]
    assert tmux('has-session', '-t', '=fixture').returncode == 0


def test_last_pane_and_replayed_close_never_kill_session(panes):
    execute, tmux, snapshots = panes
    first, second = execute({'session': 'fixture'})['panes']
    execute({'session':'fixture', 'action':'close', 'pane':first['id'], 'identity':first['identity']})
    for target in [first, second]:
        with pytest.raises(ValueError):
            execute({'session':'fixture', 'action':'close', 'pane':target['id'], 'identity':target['identity']})
    assert len(snapshots) == 1
    assert tmux('has-session', '-t', '=fixture').returncode == 0


def test_stale_foreign_and_unconfirmed_targets_do_not_mutate(panes):
    execute, tmux, snapshots = panes
    before = execute({'session':'fixture'})['panes']
    for pane, identity in [(before[0]['id'], 'old'), ('%99999', before[0]['identity']), (before[0]['id'], '')]:
        with pytest.raises(ValueError):
            execute({'session':'fixture','action':'close','pane':pane,'identity':identity})
    assert execute({'session':'fixture'})['panes'] == before
    assert snapshots == []


def test_selection_changes_only_requested_pane_focus(panes):
    execute, tmux, snapshots = panes
    target = execute({'session':'fixture'})['panes'][0]
    result = execute({'session':'fixture','action':'select','pane':target['id'],'identity':target['identity']})
    assert next(p['id'] for p in result['panes'] if p['active']) == target['id']
    assert snapshots == []


def test_snapshot_failure_preserves_both_panes(panes):
    execute, tmux, snapshots = panes
    import terminal_panes
    target = execute({'session':'fixture'})['panes'][0]
    def fail(*args):
        raise OSError('disk unavailable')
    def identity(session, pane):
        fields = ('pid','session_id','session_name','pane_id','pane_pid')
        raw = tmux('display-message','-p','-t',pane,'\t'.join('#{'+f+'}' for f in fields)).stdout.strip()
        return dict(zip(fields,raw.split('\t')))
    with pytest.raises(OSError):
        terminal_panes.execute(tmux, identity, fail, {'session':'fixture','action':'close','pane':target['id'],'identity':target['identity']})
    assert len(execute({'session':'fixture'})['panes']) == 2


def test_server_guard_preserves_last_pane_when_other_client_closes_its_sibling(panes):
    execute, tmux, snapshots = panes
    import terminal_panes
    target, sibling = execute({'session':'fixture'})['panes']
    def identify(session, pane):
        fields = ('pid','session_id','session_name','pane_id','pane_pid')
        raw = tmux('display-message','-p','-t',pane,'\t'.join('#{'+f+'}' for f in fields)).stdout.strip()
        return dict(zip(fields,raw.split('\t')))
    def competing_client(*args):
        if args[0] == 'if-shell':
            assert tmux('kill-pane','-t',sibling['id']).returncode == 0
        return tmux(*args)
    with pytest.raises(ValueError, match='cambió'):
        terminal_panes.execute(competing_client, identify, lambda *args:'saved',
            {'session':'fixture','action':'close','pane':target['id'],'identity':target['identity']})
    assert tmux('has-session','-t','=fixture').returncode == 0
    assert [p['id'] for p in execute({'session':'fixture'})['panes']] == [target['id']]


def test_foreign_existing_pane_cannot_be_closed(panes):
    execute, tmux, snapshots = panes
    foreign = tmux('new-session','-d','-P','-F','#{pane_id}','-s','other','sleep 900').stdout.strip()
    own = execute({'session':'fixture'})['panes'][0]
    with pytest.raises(ValueError):
        execute({'session':'fixture','action':'close','pane':foreign,'identity':own['identity']})
    assert tmux('has-session','-t','=other').returncode == 0
    assert snapshots == []


@pytest.mark.parametrize('direction,flag', [('right', 'horizontal'), ('down', 'vertical')])
def test_split_opens_new_pane_beside_confirmed_pane(panes, direction, flag):
    execute, tmux, snapshots = panes
    before = execute({'session':'fixture'})['panes']
    target = before[0]
    result = execute({'session':'fixture','action':'split','direction':direction,
                      'pane':target['id'],'identity':target['identity']})
    assert len(result['panes']) == len(before) + 1
    new = [p for p in result['panes'] if p['id'] not in {b['id'] for b in before}]
    assert len(new) == 1 and result['opened'] == new[0]['id'] and new[0]['active']
    assert snapshots == []


def test_split_rejects_unknown_direction_and_stale_identity(panes):
    execute, tmux, snapshots = panes
    before = execute({'session':'fixture'})['panes']
    target = before[0]
    for direction, identity in [('diagonal', target['identity']), ('right', 'old')]:
        with pytest.raises(ValueError):
            execute({'session':'fixture','action':'split','direction':direction,
                     'pane':target['id'],'identity':identity})
    assert execute({'session':'fixture'})['panes'] == before


def test_listing_reports_each_pane_folder(panes, tmp_path):
    execute, tmux, snapshots = panes
    listing = execute({'session': 'fixture'})['panes']
    assert all(p['path'] for p in listing)
