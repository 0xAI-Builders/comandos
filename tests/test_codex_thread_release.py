"""Exercise the targeted daemon release against simulated protocol responses."""
import importlib.util
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
SID = '01a0fdda-ad8d-7122-b425-6815444c19b7'
CHILD = '01a0fe68-c183-7c52-a56b-56697faa54a2'
LATE_CHILD = '01a0d7bf-64ce-78c0-946a-b7dcd13788e1'


def module():
    spec = importlib.util.spec_from_file_location('codex_thread_release', ROOT / 'lib/codex_thread_release.py')
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


class Daemon:
    def __init__(self, plan, archive_error=False, unknown_owner=False, child_skipped=False, archive_rejected=False, late_child=False):
        self.plan = plan
        self.calls = []
        self.loaded = not unknown_owner
        self.archive_error = archive_error
        self.child_skipped = child_skipped
        self.archive_rejected = archive_rejected
        self.late_child = late_child
        self.archived = set()
        self.restored = []
        self.closed = False

    def call(self, method, params):
        self.calls.append((method, params))
        if method == 'thread/loaded/list':
            return {'data': [SID, 'unrelated'] if self.loaded else ['unrelated'], 'nextCursor': None}
        if method == 'thread/read':
            ident = params['threadId']
            name = Path(self.plan['transcript']).name if ident == SID else ident + '.jsonl'
            path = str(Path(self.plan['home']) / 'archived_sessions' / name) if ident in self.archived else self.plan['transcript']
            return {'thread': {'id': ident, 'path': path, 'cwd': self.plan['cwd']}}
        if method == 'thread/list':
            ids = self.archived - {SID} if params.get('archived') else {CHILD} - self.archived
            return {'data': [{'id': ident, 'parentThreadId': SID} for ident in sorted(ids)], 'nextCursor': None}
        if method == 'thread/archive':
            assert params == {'threadId': SID}
            if self.archive_rejected:
                raise RuntimeError('archive rejected')
            self.loaded = False
            self.archived = {SID} if self.child_skipped else {SID, CHILD}
            if self.late_child:
                self.archived.add(LATE_CHILD)
            if self.archive_error:
                raise RuntimeError('archive reply lost')
            return {}
        if method == 'thread/unarchive':
            assert params['threadId'] in self.archived, 'Already-active threads must not be unarchived'
            self.archived.remove(params['threadId'])
            self.restored.append(params['threadId'])
            return {'thread': {'id': params['threadId'], 'path': self.plan['transcript']}}
        raise AssertionError(method)

    def close(self):
        self.closed = True


def fixture(m, monkeypatch, tmp_path, **kwargs):
    plan = {'sid': SID, 'home': str(tmp_path), 'cwd': str(tmp_path / 'project'),
            'binary': '/tmp/fake-codex', 'transcript': str(tmp_path / 'sessions/rollout.jsonl')}
    daemon = Daemon(plan, **kwargs)
    # Hold until the exact daemon thread is unloaded; no real user's socket.
    monkeypatch.setattr(m, 'writer_locked', lambda *_: daemon.loaded)
    monkeypatch.setattr(m, 'ControlClient', lambda *_: daemon)
    return plan, daemon


def test_releases_exact_thread_preserves_id_and_restores_descendants(monkeypatch, tmp_path):
    m = module()
    plan, daemon = fixture(m, monkeypatch, tmp_path)
    checkpoints = []
    result = m.release_writer(plan, lambda: checkpoints.append(dict(plan['releaseRecovery'])))
    assert result == plan['transcript']
    assert daemon.restored == [CHILD, SID]
    assert daemon.closed
    assert checkpoints[0]['pending'] is True
    assert checkpoints[0]['threadIds'] == [CHILD, SID]
    assert checkpoints[-1]['pending'] is False
    assert not any(method in ('thread/start', 'thread/fork', 'thread/delete', 'turn/start') for method, _ in daemon.calls)
    assert all(params.get('threadId') in (None, SID, CHILD) for _, params in daemon.calls)


def test_ambiguous_archive_failure_still_restores_every_target(monkeypatch, tmp_path):
    m = module()
    plan, daemon = fixture(m, monkeypatch, tmp_path, archive_error=True)
    with pytest.raises(RuntimeError, match='archive reply lost'):
        m.release_writer(plan)
    assert daemon.restored == [CHILD, SID]
    assert daemon.closed


def test_unknown_lock_owner_does_not_touch_any_thread(monkeypatch, tmp_path):
    m = module()
    plan, daemon = fixture(m, monkeypatch, tmp_path, unknown_owner=True)
    monkeypatch.setattr(m, 'writer_locked', lambda *_: True)
    with pytest.raises(RuntimeError, match='otro proceso'):
        m.release_writer(plan)
    assert not daemon.restored
    assert not any(method == 'thread/archive' for method, _ in daemon.calls)


def test_no_lock_does_not_connect_to_or_start_a_daemon(monkeypatch, tmp_path):
    m = module()
    plan, _ = fixture(m, monkeypatch, tmp_path)
    monkeypatch.setattr(m, 'writer_locked', lambda *_: False)
    monkeypatch.setattr(m, 'ControlClient', lambda *_: pytest.fail('No connection needed'))
    assert m.release_writer(plan) == plan['transcript']


def test_mismatched_transcript_never_archives_a_thread(monkeypatch, tmp_path):
    m = module()
    plan, daemon = fixture(m, monkeypatch, tmp_path)
    plan['transcript'] = str(tmp_path / 'wrong.jsonl')
    original = daemon.call
    def call(method, params):
        if method == 'thread/read':
            return {'thread': {'id': SID, 'path': str(tmp_path / 'actual.jsonl'), 'cwd': plan['cwd']}}
        return original(method, params)
    daemon.call = call
    with pytest.raises(RuntimeError, match='transcript'):
        m.release_writer(plan)
    assert not any(method == 'thread/archive' for method, _ in daemon.calls)


def test_partial_cascade_leaves_unarchived_child_untouched(monkeypatch, tmp_path):
    m = module()
    plan, daemon = fixture(m, monkeypatch, tmp_path, child_skipped=True)
    assert m.release_writer(plan) == plan['transcript']
    assert daemon.restored == [SID]


def test_rejected_archive_does_not_try_to_unarchive_active_conversations(monkeypatch, tmp_path):
    m = module()
    plan, daemon = fixture(m, monkeypatch, tmp_path, archive_rejected=True)
    with pytest.raises(RuntimeError, match='archive rejected'):
        m.release_writer(plan)
    assert not daemon.restored
    assert plan['releaseRecovery']['pending'] is True


def test_retry_restores_saved_children_even_when_writer_lock_is_already_free(monkeypatch, tmp_path):
    m = module()
    plan, daemon = fixture(m, monkeypatch, tmp_path)
    daemon.archived = {SID, CHILD}
    daemon.loaded = False
    plan['releaseRecovery'] = {'pending': True, 'threadIds': [CHILD, SID]}
    assert m.release_writer(plan) == plan['transcript']
    assert daemon.restored == [CHILD, SID]
    assert plan['releaseRecovery']['pending'] is False
    assert not any(method == 'thread/archive' for method, _ in daemon.calls)


def test_failed_restore_keeps_recovery_pending_and_restores_other_threads(monkeypatch, tmp_path):
    m = module()
    plan, daemon = fixture(m, monkeypatch, tmp_path)
    original = daemon.call
    def call(method, params):
        if method == 'thread/unarchive' and params['threadId'] == CHILD:
            raise RuntimeError('temporary restoration failure')
        return original(method, params)
    daemon.call = call
    with pytest.raises(RuntimeError, match='recuperación está guardada'):
        m.release_writer(plan)
    assert daemon.restored == [SID]
    assert plan['releaseRecovery']['pending'] is True


def test_descendant_created_during_archive_is_also_restored(monkeypatch, tmp_path):
    m = module()
    plan, daemon = fixture(m, monkeypatch, tmp_path, late_child=True)
    assert m.release_writer(plan) == plan['transcript']
    assert set(daemon.restored) == {SID, CHILD, LATE_CHILD}
    assert daemon.restored[-1] == SID


def test_crash_recovery_finds_late_descendants_without_restoring_previously_archived_history(monkeypatch, tmp_path):
    m = module()
    plan, daemon = fixture(m, monkeypatch, tmp_path)
    old_archived = '01a0fed9-f17e-7311-a029-9eaa33772b64'
    daemon.archived = {SID, CHILD, LATE_CHILD, old_archived}
    daemon.loaded = False
    plan['releaseRecovery'] = {'pending': True, 'threadIds': [CHILD, SID], 'previouslyArchived': [old_archived]}
    m.release_writer(plan)
    assert set(daemon.restored) == {SID, CHILD, LATE_CHILD}
    assert daemon.archived == {old_archived}


@pytest.mark.parametrize('bad_root', ['wrong-path', 'wrong-id', 'outside-account'])
def test_pending_restore_proves_exact_server_root_before_any_unarchive(monkeypatch, tmp_path, bad_root):
    m = module()
    plan, daemon = fixture(m, monkeypatch, tmp_path)
    daemon.loaded = False
    daemon.archived = {CHILD, SID}
    plan['releaseRecovery'] = {'pending': True, 'threadIds': [CHILD, SID]}
    original = daemon.call
    def call(method, params):
        if method == 'thread/read' and params['threadId'] == SID:
            path = tmp_path / ('archived_sessions/other.jsonl' if bad_root == 'wrong-path' else '../foreign.jsonl')
            return {'thread': {'id': CHILD if bad_root == 'wrong-id' else SID, 'path': str(path)}}
        return original(method, params)
    daemon.call = call
    with pytest.raises(RuntimeError, match='transcript'):
        m.release_writer(plan)
    assert daemon.closed and not daemon.restored
    assert plan['releaseRecovery']['pending']
    assert not any(method == 'thread/unarchive' for method, _ in daemon.calls)


def test_restore_preflights_all_target_paths_before_first_unarchive(monkeypatch, tmp_path):
    m = module()
    plan, daemon = fixture(m, monkeypatch, tmp_path)
    daemon.loaded = False
    daemon.archived = {CHILD, LATE_CHILD}
    plan['releaseRecovery'] = {'pending': True, 'threadIds': [CHILD, LATE_CHILD, SID]}
    original = daemon.call
    def call(method, params):
        if method == 'thread/list':
            return {'data': [{'id': ident, 'parentThreadId': SID} for ident in (CHILD, LATE_CHILD)], 'nextCursor': None}
        if method == 'thread/read' and params['threadId'] == LATE_CHILD:
            return {'thread': {'id': LATE_CHILD, 'path': str(tmp_path / '../foreign.jsonl')}}
        return original(method, params)
    daemon.call = call
    with pytest.raises(RuntimeError, match='transcript'):
        m.release_writer(plan)
    assert daemon.closed and not daemon.restored
    assert not any(method == 'thread/unarchive' for method, _ in daemon.calls)
