"""Per-device focus: two real attach clients on a private tmux server.

Never touches the user's tmux: TMUX is unset, TMUX_TMPDIR and HOME point into
pytest's tmp dir, and the server is killed at the end.
"""
import fcntl
import importlib
import os
from pathlib import Path
import pty
import shutil
import signal
import struct
import subprocess
import sys
import termios
import time

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))


@pytest.fixture
def server(tmp_path):
    if not shutil.which("tmux"):
        pytest.skip("tmux required")
    home = tmp_path / "home"
    (home / ".claude/hooks").mkdir(parents=True)
    (home / ".claude/hooks/dash-token").write_text("tok\n")
    sockdir = tmp_path / "sock"
    sockdir.mkdir(mode=0o700)
    env = {k: v for k, v in os.environ.items() if k not in ("TMUX", "TMUX_PANE")}
    env.update(HOME=str(home), TMUX_TMPDIR=str(sockdir), TERM="xterm-256color", SHELL="/bin/sh")

    def tmux(*args):
        return subprocess.run(["tmux", "-f", "/dev/null", *args], capture_output=True, text=True, env=env, timeout=10)

    out = tmp_path / "out"
    out.mkdir()
    assert tmux("new-session", "-d", "-s", "fixture", "-x", "120", "-y", "40", f"stty -echo; cat > {out}/left").returncode == 0
    # The private socket must be the one we just created, never the user's.
    assert str(sockdir) in tmux("display-message", "-p", "#{socket_path}").stdout
    tmux("set-option", "-g", "mouse", "on")
    tmux("split-window", "-h", "-t", "=fixture:", f"stty -echo; cat > {out}/right")
    kids = []

    def attach():
        pid, fd = pty.fork()
        if pid == 0:
            os.execve(str(ROOT / "bin/cc-webterm-attach"), [str(ROOT / "bin/cc-webterm-attach"), "tok", "fixture"], env)
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        kids.append(pid)
        time.sleep(0.8)
        return fd
    try:
        yield tmux, attach, out
    finally:
        for pid in kids:
            try:
                os.kill(pid, signal.SIGTERM)
            except OSError:
                pass
        tmux("kill-server")


def read(path):
    try:
        return path.read_text().split()
    except FileNotFoundError:
        return []


def test_each_remote_client_types_into_the_pane_it_selected(server):
    tmux, attach, out = server
    phone, laptop = attach(), attach()
    assert all("active-pane" in line for line in tmux("list-clients", "-F", "#{client_flags}").stdout.split())
    panes = importlib.import_module("terminal_panes")
    run = lambda data: panes.execute(tmux, lambda s, p: {"pid": "1", "session_id": "$0", "pane_id": p, "pane_pid": "1"},
                                     lambda s, p: None, data)
    listing = run({"session": "fixture"})["panes"]
    left, right = sorted(listing, key=lambda p: p["index"])
    shared = tmux("display-message", "-p", "-t", "=fixture:", "#{pane_id}").stdout.strip()
    for fd, pane in ((phone, left), (laptop, right)):
        keys = run({"session": "fixture", "action": "select", "scope": "client", "pane": pane["id"], "identity": pane["identity"]})["clientKeys"]
        os.write(fd, keys.encode())
        time.sleep(0.4)
    os.write(phone, b"fromphone\r")
    os.write(laptop, b"fromlaptop\r")
    time.sleep(0.5)
    assert read(out / "left") == ["fromphone"]
    assert read(out / "right") == ["fromlaptop"]
    # Neither remote choice moved the shared focus the desktop follows.
    assert tmux("display-message", "-p", "-t", "=fixture:", "#{pane_id}").stdout.strip() == shared


def test_input_to_an_explicit_pane_does_not_depend_on_any_focus(server):
    tmux, attach, out = server
    attach()
    left = tmux("list-panes", "-t", "=fixture:", "-F", "#{pane_id}").stdout.split()[0]
    tmux("send-keys", "-t", left, "-l", "api")
    tmux("send-keys", "-t", left, "Enter")
    time.sleep(0.4)
    assert read(out / "left") == ["api"]


def test_saving_focus_keeps_the_device_drafts_and_anchors(tmp_path):
    import app_state
    import workspace_state as ws
    conn = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(conn)
    store = ws.WorkspaceStore(conn)
    store.save_client("phone", {"drafts": {"pane-1": {"text": "hola"}}, "readingAnchors": {"pane-1": {"line": 40}}})
    store.save_client("phone", {"activeTabId": "alpha"})
    state = store.client("phone")
    assert state["activeTabId"] == "alpha"
    assert state["drafts"] == {"pane-1": {"text": "hola"}} and state["readingAnchors"] == {"pane-1": {"line": 40}}
    store.save_client("phone", {"activeTabId": None})     # explicit null clears focus only
    assert store.client("phone")["activeTabId"] is None and store.client("phone")["drafts"]
    assert store.client("desk") is None
