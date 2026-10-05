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
