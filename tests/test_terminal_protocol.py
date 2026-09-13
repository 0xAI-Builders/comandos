"""Real tmux/PTY input regressions, isolated from the user's server and panes."""
import os
import pty
import select
import subprocess
import time
from pathlib import Path

import pytest

from test_remote_ui import run_terminal_toolbar_lifecycle


def test_remote_backspace_sends_a_delete_byte_to_the_terminal():
    result = run_terminal_toolbar_lifecycle('''
sendToolbarKey("backspace");
console.log(JSON.stringify({sent, active}));
''')
    assert result['sent'] == ['\x7f']
    assert result['active'] == 'terminal'
    html = Path('dash/term.html').read_text()
    toolbar = html.split('id="term-toolbar"', 1)[1].split('</div>', 1)[0]
    assert 'data-key="backspace"' in toolbar
    assert toolbar.index('data-key="backspace"') < toolbar.index('data-action="history"')


@pytest.fixture
def isolated_terminal(tmp_path):
    socket = str(tmp_path / 'tmux.sock')
    command = ['tmux', '-S', socket]
    log, ready = tmp_path / 'input', tmp_path / 'ready'
    program = tmp_path / 'capture.py'
    program.write_text('import tty,os,pathlib\ntty.setraw(0)\nf=open(' + repr(str(log)) +
        ',"ab",buffering=0)\npathlib.Path(' + repr(str(ready)) + ').touch()\n'
        'while True: f.write(os.read(0,4096))\n')

    def tmux(*args):
        return subprocess.run(command + list(args), capture_output=True, text=True, timeout=3, check=True)

    child = None
    try:
        tmux('-f', '/dev/null', 'new-session', '-d', '-s', 'fixture', f'python3 {program}')
        until = time.monotonic() + 2
        while not ready.exists() and time.monotonic() < until:
            time.sleep(.01)
        assert ready.exists()
        tmux('source-file', str(Path('config/terminal-replies.conf').resolve()))
        child, fd = pty.fork()
        if child == 0:
            env = dict(os.environ, TERM='xterm-256color')
            env.pop('TMUX', None)
            os.execvpe('tmux', command + ['attach', '-t', 'fixture'], env)
        output = b''
        until = time.monotonic() + 2
        while b'\x1b[>c' not in output and time.monotonic() < until:
            if select.select([fd], [], [], .05)[0]:
                output += os.read(fd, 65536)
        assert b'\x1b[>c' in output
        yield fd, log, tmux
    finally:
        subprocess.run(command + ['kill-server'], capture_output=True, timeout=3)
        if child:
            os.waitpid(child, 0)
            os.close(fd)


@pytest.mark.parametrize('reply', [b'\x1b[>65;6800;1c', b'\x1b[>0;276;0c'])
@pytest.mark.parametrize('late', [False, True])
def test_device_replies_never_become_prompt_input(isolated_terminal, reply, late):
    fd, log, tmux = isolated_terminal
    if late:
        # tmux 3.2a stops recognizing DA after its one-second start timer.
        time.sleep(1.2)
    os.write(fd, reply)
    time.sleep(.06)
    for _ in range(5):
        tmux('refresh-client')
        os.write(fd, reply)
        time.sleep(.02)
    assert log.read_bytes() == b'', 'A device reply leaked into the prompt'
    # Ordinary text, including the same numeric suffix, and actual editing
    # keys remain untouched. Only complete protocol replies are consumed.
    user_input = b'65;6800;1c text\x7f\x1b[D\r'
    os.write(fd, user_input)
    until = time.monotonic() + 1
    while len(log.read_bytes()) < len(user_input) and time.monotonic() < until:
        time.sleep(.01)
    assert log.read_bytes() == user_input


def test_guard_reload_preserves_custom_user_keys(isolated_terminal):
    _, _, tmux = isolated_terminal
    tmux('set-option', '-s', 'user-keys[990]', '\x1b[custom~')
    tmux('bind-key', '-T', 'root', 'User990', 'send-keys', '-l', 'custom')
    tmux('source-file', str(Path('config/terminal-replies.conf').resolve()))
    assert 'custom' in tmux('list-keys', '-T', 'root', 'User990').stdout
    assert 'User991' in tmux('list-keys', '-T', 'root', 'User991').stdout
