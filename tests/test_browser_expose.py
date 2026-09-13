import json
import os
from pathlib import Path
import stat
import subprocess

import pytest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'bin/cc-browser-expose'


@pytest.fixture
def client(tmp_path):
    fake_bin = tmp_path / 'bin'
    fake_bin.mkdir()
    fake = fake_bin / 'ssh'
    fake.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
with open(os.environ['SSH_TEST_LOG'], 'a') as out:
    out.write(json.dumps(args) + '\\n')
socket = pathlib.Path(args[args.index('-S') + 1])
if '-O' in args:
    command = args[args.index('-O') + 1]
    if command == 'check':
        if os.environ.get('SSH_TEST_CHECK_FAIL'):
            sys.exit(255)
        sys.exit(0 if socket.exists() else 255)
    if command == 'exit':
        socket.unlink(missing_ok=True)
        sys.exit(0)
if os.environ.get('SSH_TEST_FAIL'):
    sys.exit(255)
socket.touch()
''')
    fake.chmod(0o755)
    state = tmp_path / 'state'
    log = tmp_path / 'ssh.log'
    env = {**os.environ, 'PATH': str(fake_bin) + os.pathsep + os.environ['PATH'],
           'CC_BROWSER_FORWARD_DIR': str(state), 'SSH_TEST_LOG': str(log)}

    def run(*args, extra_env=None):
        assert SCRIPT.exists(), 'forward command not implemented'
        return subprocess.run([str(SCRIPT), *args], env={**env, **(extra_env or {})}, capture_output=True, text=True)

    def calls():
        return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []

    return run, calls, state


def test_start_uses_owned_loopback_master_and_checks_it(client):
    run, calls, state = client
    result = run('start', '3000', '13000')
    assert result.returncode == 0, result.stderr
    start, check = calls()
    assert start == ['-M', '-S', str(state / '13000.sock'), '-fNT', '-o', 'ControlPersist=no',
                     '-o', 'BatchMode=yes', '-o', 'ExitOnForwardFailure=yes', '-o', 'ConnectTimeout=8',
                     '-R', '127.0.0.1:13000:127.0.0.1:3000', 'macmini']
    assert check[-3:] == ['-O', 'check', 'macmini']
    assert 'http://127.0.0.1:13000' in result.stdout
    assert stat.S_IMODE(state.stat().st_mode) == 0o700
    assert stat.S_IMODE((state / '13000.json').stat().st_mode) == 0o600


def test_repeat_start_same_mapping_is_idempotent_and_conflict_rejected(client):
    run, calls, _ = client
    assert run('start', '3000').returncode == 0
    assert run('start', '3000').returncode == 0
    assert sum('-M' in call for call in calls()) == 1
    before = len(calls())
    result = run('start', '4000', '3000')
    assert result.returncode != 0
    assert 'different local port' in result.stderr
    assert len(calls()) == before


@pytest.mark.parametrize('args', [('start', '0'), ('start', '65536'), ('start', 'abc'),
                                 ('start', '3000', '80'), ('stop', '1;touch /tmp/injected'),
                                 ('start', '$(id)'), ('start', '-R'), ('stop', '65536')])
def test_invalid_ports_do_not_start_ssh(client, args):
    run, calls, state = client
    result = run(*args)
    assert result.returncode != 0
    assert not calls()
    assert not state.exists()


def test_local_port_80_allowed(client):
    run, _, _ = client
    assert run('start', '80', '18080').returncode == 0


def test_status_empty_is_read_only_and_active_status_checks_master(client):
    run, calls, state = client
    result = run('status')
    assert result.returncode == 0
    assert 'No forwards' in result.stdout
    assert not state.exists()
    assert not calls()
    assert run('start', '3000').returncode == 0
    before = {p.name: p.stat().st_mtime_ns for p in state.iterdir()}
    result = run('status')
    assert 'running' in result.stdout
    assert '3000' in result.stdout
    assert before == {p.name: p.stat().st_mtime_ns for p in state.iterdir()}
    assert calls()[-1][-3:] == ['-O', 'check', 'macmini']


def test_stop_only_exits_owned_master_and_is_idempotent(client):
    run, calls, state = client
    assert run('stop', '3000').returncode == 0
    assert not calls()
    assert run('start', '3000').returncode == 0
    result = run('stop', '3000')
    assert result.returncode == 0, result.stderr
    assert calls()[-1][-3:] == ['-O', 'exit', 'macmini']
    assert calls()[-1][calls()[-1].index('-S') + 1] == str(state / '3000.sock')
    assert not (state / '3000.json').exists()
    before = len(calls())
    assert run('stop', '3000').returncode == 0
    assert len(calls()) == before


def test_start_failure_does_not_claim_success_or_keep_state(client):
    run, calls, state = client
    result = run('start', '3000', extra_env={'SSH_TEST_FAIL': '1'})
    assert result.returncode != 0
    assert 'http://' not in result.stdout
    assert not (state / '3000.json').exists()


def test_unknown_socket_and_symlink_state_are_refused(client, tmp_path):
    run, calls, state = client
    state.mkdir(mode=0o700)
    (state / '3000.sock').touch()
    result = run('start', '3000')
    assert result.returncode != 0
    assert not calls()
    (state / '3000.sock').unlink()
    (state / '.lock').unlink()
    state.rmdir()
    target = tmp_path / 'target'
    target.mkdir()
    state.symlink_to(target)
    assert run('start', '3000').returncode != 0
    assert not calls()


def test_failed_post_launch_check_cleans_up_owned_master(client):
    run, calls, state = client
    result = run('start', '3000', extra_env={'SSH_TEST_CHECK_FAIL': '1'})
    assert result.returncode != 0
    assert 'Started' not in result.stdout
    assert calls()[-1][-3:] == ['-O', 'exit', 'macmini']
    assert not (state / '3000.json').exists()
    assert not (state / '3000.sock').exists()
