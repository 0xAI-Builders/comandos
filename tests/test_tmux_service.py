"""Execute the unit startup against private tmux servers only."""
import configparser
import os
from pathlib import Path
import shlex
import subprocess

import pytest


UNIT = Path("systemd/tmux.service").read_text()


@pytest.fixture
def private_tmux(tmp_path):
    socket = str(tmp_path / "tmux.sock")
    env = dict(os.environ, HOME=str(tmp_path), SHELL="/bin/sh")
    env.pop("TMUX", None)

    def run(args, check=True):
        result = subprocess.run(["/usr/bin/tmux", "-S", socket, "-f", "/dev/null", *args],
                                env=env, text=True, capture_output=True, timeout=5)
        if check:
            assert result.returncode == 0, result.stderr
        return result

    def start():
        # systemd passes a literal semicolon only when it is escaped in ExecStart.
        for line in UNIT.splitlines():
            if line.startswith("ExecStart="):
                argv = shlex.split(line.partition("=")[2])
                assert argv[0] == "/usr/bin/tmux"
                run(argv[1:])

    try:
        yield run, start
    finally:
        run(["kill-server"], check=False)


def test_unit_cold_start_keeps_empty_server_alive(private_tmux):
    run, start = private_tmux
    start()
    assert run(["show-options", "-sv", "exit-empty"]).stdout.strip() == "off"
    assert run(["show-options", "-sv", "exit-unattached"]).stdout.strip() == "off"
    assert run(["list-sessions"], check=False).returncode == 0


def test_unit_start_preserves_running_server_sessions_and_splits(private_tmux):
    run, start = private_tmux
    run(["new-session", "-d", "-s", "fixture", "/bin/sh"])
    run(["split-window", "-h", "-d", "-t", "fixture", "/bin/sh"])
    before = run(["list-panes", "-a", "-F", "#{pid} #{session_id} #{window_id} #{pane_id} #{pane_pid} #{window_layout}"]).stdout
    start()
    start()
    assert run(["list-panes", "-a", "-F", "#{pid} #{session_id} #{window_id} #{pane_id} #{pane_pid} #{window_layout}"]).stdout == before


def test_unit_stop_does_not_kill_server_or_session_children():
    unit = configparser.ConfigParser(strict=False)
    unit.read_string(UNIT)
    assert unit["Service"]["Type"] == "oneshot"
    assert unit["Service"]["RemainAfterExit"] == "yes"
    assert unit["Service"].get("KillMode") == "process"
    assert "ExecStop" not in unit["Service"]
