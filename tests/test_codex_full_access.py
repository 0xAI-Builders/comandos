"""Batch restart checks use simulated panes and never contact the user's tmux."""
import importlib.util
import json
from pathlib import Path
import shlex
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'lib'))


def module():
    path = ROOT / 'lib/codex_full_access.py'
    assert path.is_file(), 'missing user-operated Codex restart batch'
    spec = importlib.util.spec_from_file_location('codex_full_access', path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def test_command_keeps_exact_conversation_account_model_and_effort():
    m = module()
    plan = {'sid': 'exact-id', 'home': '/tmp/account work', 'cwd': '/tmp/project work',
            'binary': '/tmp/codex', 'flags': ['--model', 'gpt-model', '-c', 'model_reasoning_effort="ultra"',
                '--sandbox=workspace-write', '-a', 'on-request', '-c', 'sandbox_mode="read-only"',
                '--approve-for-me', '--no-alt-screen', '--dangerously-bypass-approvals-and-sandbox']}
    argv = shlex.split(m.launch_command(plan))
    assert 'CODEX_HOME=/tmp/account work' in argv
    assert argv[argv.index('resume') + 1:argv.index('resume') + 4] == [
        '--no-daemon', '--dangerously-bypass-approvals-and-sandbox', 'exact-id']
    assert argv[-1] == 'continua'
    assert '--no-daemon' in argv
    assert argv.count('--dangerously-bypass-approvals-and-sandbox') == 1
    assert 'on-request' not in argv and '--sandbox=workspace-write' not in argv
    assert 'sandbox_mode="read-only"' not in argv and '--approve-for-me' not in argv
    assert 'gpt-model' in argv and 'model_reasoning_effort="ultra"' in argv
    assert '--no-alt-screen' in argv


def test_remote_session_is_not_silently_moved_to_local_machine():
    m = module()
    with pytest.raises(ValueError, match='remot'):
        m.launch_command({'sid': 'exact', 'home': '/tmp/home', 'cwd': '/tmp',
                          'binary': 'codex', 'flags': ['--remote', 'unix://server']})


def test_effective_permissions_require_filesystem_network_and_never_approval():
    m = module()
    context = {'approval_policy': 'never', 'sandbox_policy': {'type': 'danger-full-access'}}
    assert m.full_access(context)
    assert not m.full_access({**context, 'sandbox_policy': {'type': 'workspace-write'}})
    assert not m.full_access({**context, 'approval_policy': 'on-request'})
    assert not m.full_access({**context, 'permission_profile': {
        'type': 'managed', 'file_system': {'type': 'restricted'}, 'network': 'restricted'}})
    assert m.full_access({**context, 'permission_profile': {'type': 'disabled'}})


def test_existing_full_access_turn_does_not_confirm_new_launch(tmp_path):
    m = module()
    path = tmp_path / 'rollout.jsonl'
    ctx = {'approval_policy': 'never', 'sandbox_policy': {'type': 'danger-full-access'}}
    path.write_text(json.dumps({'type': 'turn_context', 'payload': ctx}) + '\n')
    offset = path.stat().st_size
    assert m.read_new_context(path, offset) is None
    with path.open('a') as f:
        f.write(json.dumps({'type': 'turn_context', 'payload': {**ctx, 'model': 'new'}}) + '\n')
    assert m.read_new_context(path, offset)['model'] == 'new'


def test_batch_continues_after_failure_and_reports_every_target():
    m = module()
    plans = [{'pane': '%1'}, {'pane': '%2'}, {'pane': '%3'}]
    calls = []
    def restart(plan):
        calls.append(plan['pane'])
        if plan['pane'] == '%2':
            raise RuntimeError('failed startup')
        return {'status': 'confirmed'}
    results = m.run_batch(plans, restart)
    assert calls == ['%1', '%2', '%3']
    assert [r['status'] for r in results] == ['confirmed', 'failed', 'confirmed']
    assert 'failed startup' in results[1]['error']


def test_apply_preflight_failure_never_stops_any_source(monkeypatch):
    m = module()
    class Runtime:
        def inventory(self):
            return [{'session': 'one', 'pane': '%1', 'sid': 'exact', 'home': '/tmp'}], ['%2: missing exact conversation']
        def restart(self, plan):
            pytest.fail('No session may be stopped when batch preflight fails')
    monkeypatch.setattr(m, 'LocalRuntime', Runtime)
    monkeypatch.setattr(m.os, 'kill', lambda *_: pytest.fail('No process may be signalled'))
    monkeypatch.setattr(m, '__name__', 'codex_full_access')
    monkeypatch.setattr('sys.argv', ['cc-codex-full-access', '--apply'])
    assert m.main() == 1


def test_apply_installs_permanent_rule_even_when_no_old_yolo_panes_exist(monkeypatch):
    import codex_yolo_install
    m = module(); installs = []
    class Runtime:
        def inventory(self):
            return [], []
    monkeypatch.setattr(m, 'LocalRuntime', Runtime)
    def install():
        installs.append(True)
        return {'wrapper': '/tmp/user/bin/codex', 'original': '/tmp/original'}
    monkeypatch.setattr(codex_yolo_install, 'install', install)
    monkeypatch.setattr('sys.argv', ['cc-codex-full-access', '--apply'])
    assert m.main() == 0 and installs == [True]


def test_install_only_does_not_contact_tmux(monkeypatch):
    import codex_yolo_install
    m = module()
    monkeypatch.setattr(m, 'LocalRuntime', lambda: pytest.fail('install-only must not contact tmux'))
    monkeypatch.setattr(codex_yolo_install, 'install', lambda: {'wrapper': '/tmp/codex', 'original': '/tmp/native'})
    monkeypatch.setattr('sys.argv', ['cc-codex-full-access', '--install-only'])
    assert m.main() == 0


def test_restart_checks_identity_before_termination_and_sends_continua_once(monkeypatch, tmp_path):
    m = module()
    transcript = tmp_path / 'rollout.jsonl'; transcript.write_text('')
    calls = []
    class Runtime(m.LocalRuntime):
        def __init__(self):
            self.stopped = False
        def tmux(self, *args):
            calls.append(args)
            if args == ('send-keys', '-t', '%1', 'C-c'):
                self.stopped = True
            if args[0] == 'display-message':
                return {'#{session_name}': 'session', '#{pane_pid}': '1', '#{pane_current_command}': 'zsh' if self.stopped else 'codex',
                        '#{pane_tty}': '/dev/pts/99'}[args[-1]]
            return ''
        def process(self, pid):
            if pid == 10 and self.stopped:
                raise FileNotFoundError()
            return {'start': 'shell' if pid == 1 else 'agent', 'state': 'S'}
    runtime = Runtime()
    monkeypatch.setattr(m.subprocess, 'run', lambda *a, **k: None)
    monkeypatch.setattr(m, 'release_writer', lambda plan, checkpoint=None: plan['transcript'], raising=False)
    monkeypatch.setattr(m, 'read_new_context', lambda *a: {'approval_policy': 'never', 'sandbox_policy': {'type': 'danger-full-access'}})
    plan = {'session': 'session', 'pane': '%1', 'panePid': 1, 'paneStart': 'shell', 'pid': 10, 'start': 'agent',
            'sid': 'exact', 'transcript': str(transcript), 'command': 'codex resume exact --no-daemon --dangerously-bypass-approvals-and-sandbox continua'}
    assert runtime.restart(plan)['status'] == 'confirmed'
    assert ('send-keys', '-t', '%1', 'C-c') in calls
    assert len([c for c in calls if c[0] == 'send-keys' and '-l' in c]) == 1
    assert next(c for c in calls if c[0] == 'send-keys' and '-l' in c)[-1].endswith(' continua')
    calls.clear(); runtime.stopped = False
    with pytest.raises(RuntimeError, match='agente'):
        runtime.restart({**plan, 'start': 'changed'})
    assert not any(c[0] == 'send-keys' for c in calls)


class RestartRuntime:
    """Model a TUI that needs rapid interrupts or refuses them entirely."""
    def __init__(self, m, monkeypatch, tmp_path, close_on_second=True):
        self.now = 0.0
        self.calls = []
        self.interrupts = []
        self.stopped = False
        self.reused = False
        self.close_on_second = close_on_second
        self.transcript = tmp_path / 'rollout.jsonl'
        self.transcript.write_text('')
        self.context = {'approval_policy': 'never', 'sandbox_policy': {'type': 'danger-full-access'}}
        self.m = m
        owner = self
        class Runtime(m.LocalRuntime):
            def __init__(self):
                pass
            def tmux(self, *args):
                owner.calls.append(args)
                if args[0] == 'display-message':
                    return {'#{session_name}': 'session', '#{pane_pid}': '1',
                            '#{pane_current_command}': 'zsh' if owner.stopped else 'codex',
                            '#{pane_tty}': '/dev/pts/99'}[args[-1]]
                if args[-1] == 'C-c':
                    owner.interrupts.append(owner.now)
                    if owner.close_on_second and len(owner.interrupts) == 2:
                        if owner.interrupts[-1] - owner.interrupts[-2] < .5:
                            owner.stopped = True
                    # A closing source appends its final, old turn context.
                    if owner.stopped:
                        with owner.transcript.open('a') as f:
                            f.write(json.dumps({'type': 'turn_context', 'payload': owner.context}) + '\n')
                return ''
            def process(self, pid):
                if pid == 10 and owner.stopped:
                    raise FileNotFoundError()
                return {'start': 'shell' if pid == 1 else 'replacement' if owner.reused else 'agent', 'state': 'S'}
        self.runtime = Runtime()
        self.plan = {'session': 'session', 'pane': '%1', 'panePid': 1, 'paneStart': 'shell',
                     'pid': 10, 'start': 'agent', 'sid': 'exact', 'transcript': str(self.transcript),
                     'command': 'codex resume exact --no-daemon --dangerously-bypass-approvals-and-sandbox continua'}
        def sleep(seconds):
            self.now += seconds
        monkeypatch.setattr(m.time, 'monotonic', lambda: self.now)
        monkeypatch.setattr(m.time, 'sleep', sleep)
        monkeypatch.setattr(m.subprocess, 'run', lambda *a, **k: None)
        monkeypatch.setattr(m, 'release_writer', lambda plan, checkpoint=None: plan['transcript'], raising=False)


def test_restart_uses_fast_double_interrupt_and_does_not_verify_source_turn(monkeypatch, tmp_path):
    m = module()
    fake = RestartRuntime(m, monkeypatch, tmp_path)
    result = fake.runtime.restart(fake.plan)
    assert fake.stopped
    assert fake.interrupts[1] - fake.interrupts[0] < .5
    assert result['status'] == 'unverified'
    commands = [c[-1] for c in fake.calls if c[0] == 'send-keys' and '-l' in c]
    assert commands.count(fake.plan['command']) == 1


def test_stuck_tui_uses_only_validated_pid_then_relaunches(monkeypatch, tmp_path):
    m = module()
    fake = RestartRuntime(m, monkeypatch, tmp_path, close_on_second=False)
    signals = []
    monkeypatch.setattr(m.os, 'pidfd_open', lambda pid: signals.append(('open', pid)) or 99)
    monkeypatch.setattr(m.os, 'close', lambda fd: signals.append(('close', fd)))
    def terminate(fd, sig, *args):
        signals.append(('signal', fd, sig))
        fake.stopped = True
    monkeypatch.setattr(m.signal, 'pidfd_send_signal', terminate)
    monkeypatch.setattr(m, 'read_new_context', lambda *args: fake.context)
    assert fake.runtime.restart(fake.plan)['status'] == 'confirmed'
    assert ('open', 10) in signals
    assert ('signal', 99, m.signal.SIGTERM) in signals
    assert signals[-1] == ('close', 99)
    assert not any(c[-1] == 'continua' for c in fake.calls)


def test_pid_reuse_before_termination_never_signals_replacement(monkeypatch, tmp_path):
    m = module()
    fake = RestartRuntime(m, monkeypatch, tmp_path, close_on_second=False)
    def open_pid(pid):
        fake.reused = True
        return 99
    monkeypatch.setattr(m.os, 'pidfd_open', open_pid)
    monkeypatch.setattr(m.os, 'close', lambda *_: None)
    monkeypatch.setattr(m.signal, 'pidfd_send_signal', lambda *_: pytest.fail('Replacement process cannot be killed'))
    with pytest.raises(RuntimeError, match='shell|agente'):
        fake.runtime.restart(fake.plan)
    assert not any(c[-1] == fake.plan['command'] for c in fake.calls)


def test_restart_releases_writer_after_close_and_before_continua(monkeypatch, tmp_path):
    m = module()
    fake = RestartRuntime(m, monkeypatch, tmp_path)
    released = []
    def release(plan, checkpoint=None):
        assert fake.stopped
        assert not any(c[-1] == plan['command'] for c in fake.calls)
        released.append(plan['pane'])
        return plan['transcript']
    monkeypatch.setattr(m, 'release_writer', release, raising=False)
    monkeypatch.setattr(m, 'read_new_context', lambda *_: fake.context)
    assert fake.runtime.restart(fake.plan)['status'] == 'confirmed'
    assert released == ['%1']


def test_retry_only_restarts_pending_pane_and_reports_entire_original_batch(monkeypatch, tmp_path):
    import codex_yolo_install
    m = module()
    prior = tmp_path / 'prior.json'
    plans = [{'session': 'session', 'pane': f'%{i}', 'sid': f'id-{i}', 'home': '/tmp/account',
              'binary': '/tmp/codex', 'panePid': i, 'paneStart': f'shell-{i}'} for i in range(1, 4)]
    results = [{'pane': '%1', 'status': 'unverified'}, {'pane': '%2', 'status': 'confirmed'},
               {'pane': '%3', 'status': 'confirmed'}]
    prior.write_text(json.dumps({'plans': plans, 'results': results}))
    calls = []
    class Runtime:
        def inventory(self, panes=None):
            assert panes == {'%1'}
            return [dict(plans[0], pid=20, start='current-agent')], []
        def restart(self, plan):
            calls.append(plan['pane'])
            return {'status': 'confirmed', 'conversationId': plan['sid'], 'prompt': 'continua'}
    monkeypatch.setattr(m, 'LocalRuntime', Runtime)
    monkeypatch.setattr(codex_yolo_install, 'install', lambda: {'wrapper': '/tmp/installed'})
    monkeypatch.setattr(m.subprocess, 'run', lambda *a, **k: type('Result', (), {'stdout': '--no-daemon'})())
    monkeypatch.setenv('XDG_STATE_HOME', str(tmp_path / 'state'))
    monkeypatch.setattr('sys.argv', ['cc-codex-full-access', '--apply', '--retry-report', str(prior)])
    assert m.main() == 0
    assert calls == ['%1']
    report = next((tmp_path / 'state/comandos/codex-full-access').glob('*.json'))
    data = json.loads(report.read_text())
    assert len(data['plans']) == 3
    assert [r['status'] for r in data['results']] == ['confirmed', 'confirmed', 'confirmed']
    assert data['previousReport'] == str(prior)


def test_retry_refuses_to_close_a_different_conversation():
    m = module()
    old = {'session': 'session', 'pane': '%1', 'sid': 'expected', 'home': '/tmp/account',
           'panePid': 1, 'paneStart': 'shell'}
    fresh = {**old, 'sid': 'different'}
    with pytest.raises(ValueError, match='cambió'):
        m.select_retry([old], [fresh], object())


def test_retry_carries_pending_history_restoration_to_the_new_process():
    m = module()
    old = {'session': 'session', 'pane': '%1', 'sid': 'expected', 'home': '/tmp/account',
           'panePid': 1, 'paneStart': 'shell', 'releaseRecovery': {'pending': True, 'threadIds': ['child', 'expected']}}
    fresh = {key: value for key, value in old.items() if key != 'releaseRecovery'}
    assert m.select_retry([old], [fresh], object())[0]['releaseRecovery'] == old['releaseRecovery']


def test_retry_recovers_missing_agent_only_from_unchanged_shell(monkeypatch):
    m = module()
    old = {'session': 'session', 'pane': '%1', 'sid': 'expected', 'home': '/tmp/account',
           'cwd': '/tmp/project', 'binary': '/tmp/codex', 'flags': [], 'panePid': 1, 'paneStart': 'shell'}
    class Runtime:
        def check_pane(self, plan):
            assert plan['paneStart'] == 'shell'
        def tmux(self, *args):
            return 'zsh'
    result = m.select_retry([old], [], Runtime())
    assert result[0]['pid'] is None
    assert 'CODEX_HOME=/tmp/account' in result[0]['command']
    assert 'expected' in result[0]['command']


def test_retry_from_latest_finished_report_does_not_contact_tmux(monkeypatch, tmp_path):
    m = module()
    directory = tmp_path / 'comandos/codex-full-access'; directory.mkdir(parents=True)
    (directory / '1.json').write_text(json.dumps({'plans': [{'pane': '%1'}],
        'results': [{'pane': '%1', 'status': 'confirmed'}]}))
    monkeypatch.setenv('XDG_STATE_HOME', str(tmp_path))
    monkeypatch.setattr('sys.argv', ['cc-codex-full-access', '--apply', '--retry-failed'])
    monkeypatch.setattr(m, 'LocalRuntime', lambda: pytest.fail('No pending target to inspect'))
    assert m.main() == 0


def test_report_write_failure_keeps_the_previous_recovery_record(monkeypatch, tmp_path):
    m = module()
    path = tmp_path / 'report.json'
    path.write_text('{"plans":[],"results":[]}')
    def fail_replace(*_):
        raise OSError('simulated interrupted report replacement')
    monkeypatch.setattr(m.os, 'replace', fail_replace)
    with pytest.raises(OSError, match='report replacement'):
        m.write_report(path, '{"plans":[{"recovery":"saved"}],"results":[]}')
    assert json.loads(path.read_text()) == {'plans': [], 'results': []}
    assert list(tmp_path.iterdir()) == [path]


def test_close_only_never_inspects_tmux_installs_or_resumes(monkeypatch):
    import codex_yolo_install
    m = module()
    calls = []
    monkeypatch.setattr(m, 'close_session', lambda sid: calls.append(sid) or 0, raising=False)
    monkeypatch.setattr(m, 'LocalRuntime', lambda: pytest.fail('Close-only does not operate terminals'))
    monkeypatch.setattr(codex_yolo_install, 'install', lambda: pytest.fail('Close-only does not install'))
    sid = '01a0fdda-ad8d-7122-b425-6815444c19b7'
    monkeypatch.setattr('sys.argv', ['cc-codex-full-access', '--close-session', sid])
    assert m.main() == 0
    assert calls == [sid]


CLOSE_SID = '01a0fdda-ad8d-7122-b425-6815444c19b7'


def close_fixture(m, monkeypatch, tmp_path):
    home = tmp_path / 'account'
    transcript = home / 'sessions/2026/10/03' / f'rollout-{CLOSE_SID}.jsonl'
    transcript.parent.mkdir(parents=True)
    transcript.write_text(json.dumps({'type': 'session_meta', 'payload': {
        'id': CLOSE_SID, 'cwd': str(tmp_path / 'project')}}) + '\n')
    monkeypatch.setenv('CODEX_HOME', str(home))
    monkeypatch.setattr(m, 'LocalRuntime', lambda: pytest.fail('Close never inspects panes'))
    monkeypatch.setattr(m, 'writer_locked', lambda *_: True, raising=False)
    monkeypatch.setattr(m, 'codex_binary', lambda: '/tmp/fake-codex', raising=False)
    return home, transcript


def test_close_releases_validated_exact_account_and_saves_recovery(monkeypatch, tmp_path):
    m = module()
    home, transcript = close_fixture(m, monkeypatch, tmp_path)
    calls = []
    def release(plan, checkpoint):
        calls.append(dict(plan))
        assert plan['sid'] == CLOSE_SID and plan['home'] == str(home)
        assert plan['transcript'] == str(transcript)
        assert plan['binary'] == '/tmp/fake-codex'
        plan['releaseRecovery'] = {'pending': True, 'threadIds': [CLOSE_SID]}
        checkpoint()
        saved = json.loads((home / 'comandos-close-recovery' / f'{CLOSE_SID}.json').read_text())
        assert saved['releaseRecovery']['pending'] is True
        plan['releaseRecovery']['pending'] = False
        checkpoint()
        return str(transcript)
    monkeypatch.setattr(m, 'release_writer', release)
    assert m.close_session(CLOSE_SID) == 0
    assert len(calls) == 1
    assert not json.loads((home / 'comandos-close-recovery' / f'{CLOSE_SID}.json').read_text())['releaseRecovery']['pending']


@pytest.mark.parametrize('problem', ['missing', 'duplicate', 'mismatch', 'outside-account'])
def test_close_refuses_ambiguous_or_mismatched_transcript(monkeypatch, tmp_path, problem):
    m = module()
    home, transcript = close_fixture(m, monkeypatch, tmp_path)
    if problem == 'missing':
        transcript.unlink()
    elif problem == 'duplicate':
        second = transcript.parent / f'rollout-other-{CLOSE_SID}.jsonl'
        second.write_bytes(transcript.read_bytes())
    elif problem == 'mismatch':
        transcript.write_text(json.dumps({'type': 'session_meta', 'payload': {'id': 'different'}}))
    else:
        outside = tmp_path / 'foreign.jsonl'
        transcript.rename(outside)
        transcript.symlink_to(outside)
    monkeypatch.setattr(m, 'release_writer', lambda *_: pytest.fail('Invalid target cannot be released'))
    with pytest.raises((ValueError, RuntimeError)):
        m.close_session(CLOSE_SID)


def test_close_without_writer_lock_does_not_connect_resolve_binary_or_write_recovery(monkeypatch, tmp_path):
    m = module()
    home, _ = close_fixture(m, monkeypatch, tmp_path)
    monkeypatch.setattr(m, 'writer_locked', lambda *_: False)
    monkeypatch.setattr(m, 'codex_binary', lambda: pytest.fail('No binary needed for already closed writer'))
    monkeypatch.setattr(m, 'release_writer', lambda *_: pytest.fail('No control needed for already closed writer'))
    assert m.close_session(CLOSE_SID) == 0
    assert not (home / 'comandos-close-recovery').exists()


def test_close_invalid_id_does_not_read_account_or_connect(monkeypatch):
    m = module()
    monkeypatch.setattr(m, 'release_writer', lambda *_: pytest.fail('Invalid ID'))
    with pytest.raises(ValueError, match='ID'):
        m.close_session('../last')


def test_close_preserves_pending_recovery_and_retries_from_archived_transcript(monkeypatch, tmp_path):
    m = module()
    home, transcript = close_fixture(m, monkeypatch, tmp_path)
    def interrupted(plan, checkpoint):
        plan['releaseRecovery'] = {'pending': True, 'threadIds': [CLOSE_SID]}
        checkpoint()
        archived = home / 'archived_sessions' / transcript.name
        archived.parent.mkdir()
        transcript.rename(archived)
        raise RuntimeError('restore interrupted')
    monkeypatch.setattr(m, 'release_writer', interrupted)
    with pytest.raises(RuntimeError, match='restore interrupted'):
        m.close_session(CLOSE_SID)
    saved = home / 'comandos-close-recovery' / f'{CLOSE_SID}.json'
    assert json.loads(saved.read_text())['releaseRecovery']['pending']
    monkeypatch.setattr(m, 'writer_locked', lambda *_: False)
    def restore(plan, checkpoint):
        assert plan['releaseRecovery']['pending']
        assert Path(plan['transcript']).parent == home / 'archived_sessions'
        Path(plan['transcript']).rename(transcript)
        plan['transcript'] = str(transcript)
        plan['releaseRecovery']['pending'] = False
        checkpoint()
        return str(transcript)
    monkeypatch.setattr(m, 'release_writer', restore)
    assert m.close_session(CLOSE_SID) == 0
    assert not json.loads(saved.read_text())['releaseRecovery']['pending']


def test_close_refuses_foreign_recovery_record(monkeypatch, tmp_path):
    m = module()
    home, transcript = close_fixture(m, monkeypatch, tmp_path)
    directory = home / 'comandos-close-recovery'; directory.mkdir()
    (directory / f'{CLOSE_SID}.json').write_text(json.dumps({'sid': CLOSE_SID,
        'home': str(tmp_path / 'different-account'), 'transcript': str(transcript),
        'releaseRecovery': {'pending': True, 'threadIds': [CLOSE_SID]}}))
    monkeypatch.setattr(m, 'release_writer', lambda *_: pytest.fail('Foreign recovery cannot be applied'))
    with pytest.raises(ValueError, match='cuenta'):
        m.close_session(CLOSE_SID)


@pytest.mark.parametrize('option', ['--apply', '--install-only', '--retry-failed', '--retry-report'])
def test_close_only_rejects_restart_or_install_modes(monkeypatch, option):
    m = module()
    monkeypatch.setattr(m, 'close_session', lambda *_: pytest.fail('Conflicting options'), raising=False)
    monkeypatch.setattr('sys.argv', ['cc-codex-full-access', '--close-session', CLOSE_SID, option,
        *(['/tmp/report.json'] if option == '--retry-report' else [])])
    with pytest.raises(SystemExit) as exc:
        m.main()
    assert exc.value.code == 2


def test_close_missing_native_binary_never_releases_writer(monkeypatch, tmp_path):
    m = module()
    close_fixture(m, monkeypatch, tmp_path)
    monkeypatch.setattr(m, 'codex_binary', lambda: (_ for _ in ()).throw(RuntimeError('Codex no disponible')))
    monkeypatch.setattr(m, 'release_writer', lambda *_: pytest.fail('Missing binary'))
    with pytest.raises(RuntimeError, match='no disponible'):
        m.close_session(CLOSE_SID)


def test_close_only_propagates_failure_without_installation(monkeypatch):
    m = module()
    monkeypatch.setattr(m, 'close_session', lambda *_: (_ for _ in ()).throw(RuntimeError('control unavailable')), raising=False)
    monkeypatch.setattr(m, 'LocalRuntime', lambda: pytest.fail('Close failure never restarts'))
    monkeypatch.setattr('sys.argv', ['cc-codex-full-access', '--close-session', CLOSE_SID])
    assert m.main() == 1


@pytest.mark.parametrize('escaped', ['sessions', 'comandos-close-recovery'])
def test_close_refuses_account_directory_symlinks(monkeypatch, tmp_path, escaped):
    m = module()
    home, _ = close_fixture(m, monkeypatch, tmp_path)
    outside = tmp_path / f'foreign-{escaped}'
    directory = home / escaped
    if directory.exists():
        directory.rename(outside)
    else:
        outside.mkdir()
    directory.symlink_to(outside, target_is_directory=True)
    monkeypatch.setattr(m, 'release_writer', lambda *_: pytest.fail('Escaped account'))
    with pytest.raises(ValueError, match='cuenta'):
        m.close_session(CLOSE_SID)


def test_close_native_binary_resolution_follows_yolo_wrapper(monkeypatch, tmp_path):
    m = module()
    native = tmp_path / 'original-codex'; native.write_bytes(b'\x7fELF')
    wrapper = tmp_path / 'codex'
    wrapper.write_text('# COMANDOS_CODEX_ORIGINAL=' + json.dumps(str(native)) + '\n')
    monkeypatch.setattr(m.shutil, 'which', lambda _: str(wrapper))
    assert m.codex_binary() == str(native)
    monkeypatch.setattr(m.shutil, 'which', lambda _: None)
    with pytest.raises(RuntimeError, match='ejecutable original'):
        m.codex_binary()


def test_close_uses_real_release_helper_and_existing_control_only(monkeypatch, tmp_path):
    import codex_thread_release as release
    from test_codex_thread_release import Daemon, CHILD
    m = module()
    home, transcript = close_fixture(m, monkeypatch, tmp_path)
    plan = {'sid': CLOSE_SID, 'home': str(home), 'cwd': str(tmp_path), 'transcript': str(transcript)}
    daemon = Daemon(plan)
    monkeypatch.setattr(m, 'writer_locked', lambda *_: daemon.loaded)
    monkeypatch.setattr(release, 'writer_locked', lambda *_: daemon.loaded)
    monkeypatch.setattr(release, 'ControlClient', lambda _: daemon)
    assert m.close_session(CLOSE_SID) == 0
    assert daemon.closed and daemon.restored == [CHILD, CLOSE_SID]
    assert not daemon.archived
    assert [method for method, _ in daemon.calls].count('thread/archive') == 1
    saved = json.loads((home / 'comandos-close-recovery' / f'{CLOSE_SID}.json').read_text())
    assert saved['releaseRecovery']['pending'] is False


def test_close_missing_control_socket_fails_without_starting_any_process(monkeypatch, tmp_path):
    import codex_thread_release as release
    m = module()
    close_fixture(m, monkeypatch, tmp_path)
    monkeypatch.setattr(release, 'writer_locked', lambda *_: True)
    monkeypatch.setattr(m.subprocess, 'Popen', lambda *_a, **_k: pytest.fail('No daemon may be started'))
    with pytest.raises(RuntimeError, match='no hay control'):
        m.close_session(CLOSE_SID)


@pytest.mark.parametrize('record', [[], {'sid': CLOSE_SID, 'releaseRecovery': []}])
def test_close_refuses_malformed_recovery_before_control(monkeypatch, tmp_path, record):
    m = module()
    home, _ = close_fixture(m, monkeypatch, tmp_path)
    directory = home / 'comandos-close-recovery'; directory.mkdir()
    (directory / f'{CLOSE_SID}.json').write_text(json.dumps(record))
    monkeypatch.setattr(m, 'release_writer', lambda *_: pytest.fail('Malformed recovery'))
    with pytest.raises(ValueError):
        m.close_session(CLOSE_SID)


@pytest.mark.parametrize('option', [None, '--apply', '--install-only', '--retry-failed', '--retry-report'])
def test_empty_close_id_stays_close_only_before_installer_or_runtime(monkeypatch, option):
    import builtins
    m = module()
    original_import = builtins.__import__
    def guarded_import(name, *args, **kwargs):
        if name == 'codex_yolo_install':
            pytest.fail('Close dispatch must precede installer import')
        return original_import(name, *args, **kwargs)
    monkeypatch.setattr(builtins, '__import__', guarded_import)
    monkeypatch.setattr(m, 'LocalRuntime', lambda: pytest.fail('Empty close ID cannot inspect panes'))
    monkeypatch.setattr(m, 'codex_binary', lambda: pytest.fail('Empty close ID cannot resolve a binary'))
    monkeypatch.setattr('sys.argv', ['cc-codex-full-access', '--close-session', '',
        *([option] if option else []), *(['/tmp/report.json'] if option == '--retry-report' else [])])
    if option:
        with pytest.raises(SystemExit) as exc:
            m.main()
        assert exc.value.code == 2
    else:
        assert m.main() == 1


@pytest.mark.parametrize('recovery', [
    {'pending': True}, {'pending': 'true', 'threadIds': [CLOSE_SID]},
    {'pending': 1, 'threadIds': [CLOSE_SID]}, {'threadIds': [CLOSE_SID]},
    {'pending': True, 'threadIds': CLOSE_SID}, {'pending': True, 'threadIds': [None, CLOSE_SID]},
    {'pending': True, 'threadIds': ['-' * 36, CLOSE_SID]}, {'pending': True, 'threadIds': []},
    {'pending': True, 'threadIds': ['01a0fe68-c183-7c52-a56b-56697faa54a2']},
    {'pending': True, 'threadIds': [CLOSE_SID, CLOSE_SID]},
    {'pending': True, 'threadIds': [CLOSE_SID], 'previouslyArchived': 'invalid'},
    {'pending': True, 'threadIds': [CLOSE_SID], 'previouslyArchived': [None]},
    {'pending': True, 'threadIds': [CLOSE_SID], 'commands': 'invalid'},
    {'pending': True, 'threadIds': [CLOSE_SID], 'commands': [None]},
])
def test_close_nested_recovery_schema_fails_before_binary_control_and_writes(monkeypatch, tmp_path, recovery):
    m = module()
    home, transcript = close_fixture(m, monkeypatch, tmp_path)
    directory = home / 'comandos-close-recovery'; directory.mkdir()
    report = directory / f'{CLOSE_SID}.json'
    report.write_text(json.dumps({'sid': CLOSE_SID, 'home': str(home),
        'transcript': str(transcript), 'releaseRecovery': recovery}))
    previous = report.read_bytes()
    monkeypatch.setattr(m, 'codex_binary', lambda: pytest.fail('Invalid recovery cannot resolve a binary'))
    monkeypatch.setattr(m, 'release_writer', lambda *_: pytest.fail('Invalid recovery cannot open control'))
    with pytest.raises(ValueError, match='recuperación'):
        m.close_session(CLOSE_SID)
    assert report.read_bytes() == previous
    assert not (directory / f'{CLOSE_SID}.lock').exists()


def test_close_corrupt_saved_tree_cannot_restore_unrelated_same_account_thread(monkeypatch, tmp_path):
    import codex_thread_release as release
    from test_codex_thread_release import Daemon
    m = module()
    home, transcript = close_fixture(m, monkeypatch, tmp_path)
    unrelated = '01a0fed9-f17e-7311-a029-9eaa33772b64'
    directory = home / 'comandos-close-recovery'; directory.mkdir()
    (directory / f'{CLOSE_SID}.json').write_text(json.dumps({'sid': CLOSE_SID, 'home': str(home),
        'transcript': str(transcript), 'releaseRecovery': {'pending': True, 'threadIds': [unrelated, CLOSE_SID]}}))
    daemon = Daemon({'sid': CLOSE_SID, 'home': str(home), 'cwd': str(tmp_path), 'transcript': str(transcript)})
    daemon.loaded = False
    daemon.archived = {unrelated}
    original = daemon.call
    def call(method, params):
        if method == 'thread/list':
            return {'data': [], 'nextCursor': None}
        return original(method, params)
    daemon.call = call
    monkeypatch.setattr(release, 'ControlClient', lambda _: daemon)
    monkeypatch.setattr(release, 'writer_locked', lambda *_: False)
    with pytest.raises((ValueError, RuntimeError)):
        m.close_session(CLOSE_SID)
    assert daemon.closed and not daemon.restored
    assert not any(method == 'thread/unarchive' for method, _ in daemon.calls)


def test_close_pending_archived_rollout_recovers_exact_tree_and_durable_history(monkeypatch, tmp_path):
    import codex_thread_release as release
    from test_codex_thread_release import Daemon, CHILD
    m = module()
    home, transcript = close_fixture(m, monkeypatch, tmp_path)
    history = transcript.read_bytes()
    archived = home / 'archived_sessions' / transcript.name
    archived.parent.mkdir()
    transcript.rename(archived)
    directory = home / 'comandos-close-recovery'; directory.mkdir()
    report = directory / f'{CLOSE_SID}.json'
    report.write_text(json.dumps({'sid': CLOSE_SID, 'home': str(home), 'transcript': str(transcript),
        'releaseRecovery': {'pending': True, 'threadIds': [CHILD, CLOSE_SID], 'previouslyArchived': []}}))
    daemon = Daemon({'sid': CLOSE_SID, 'home': str(home), 'cwd': str(tmp_path), 'transcript': str(transcript)})
    daemon.loaded = False
    daemon.archived = {CHILD, CLOSE_SID}
    original = daemon.call
    def call(method, params):
        result = original(method, params)
        if method == 'thread/unarchive' and params['threadId'] == CLOSE_SID:
            archived.rename(transcript)
        return result
    daemon.call = call
    monkeypatch.setattr(m, 'writer_locked', lambda *_: False)
    monkeypatch.setattr(release, 'writer_locked', lambda *_: False)
    monkeypatch.setattr(release, 'ControlClient', lambda _: daemon)
    assert m.close_session(CLOSE_SID) == 0
    assert daemon.closed and daemon.restored == [CHILD, CLOSE_SID] and not daemon.archived
    assert transcript.read_bytes() == history and not archived.exists()
    saved = json.loads(report.read_text())
    assert saved['sid'] == CLOSE_SID and saved['transcript'] == str(transcript)
    assert saved['releaseRecovery']['threadIds'] == [CHILD, CLOSE_SID]
    assert saved['releaseRecovery']['pending'] is False
    assert not any(method == 'thread/archive' for method, _ in daemon.calls)
