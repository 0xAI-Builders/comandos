"""Ctrl+C debe alcanzar el árbol de Grok/Codex, no solo el grupo en primer plano."""
import os
import signal
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))

import agent_stop


def proc(pid, ppid, pgid, comm, start=1):
    return {"pid": pid, "ppid": ppid, "pgid": pgid, "comm": comm, "start": start}


def test_ctrl_c_targets_grok_and_setsid_children_not_the_shell():
    processes = [
        proc(10, 1, 10, "zsh"),
        proc(20, 10, 20, "grok"),
        proc(21, 20, 21, "node"),   # MCP con sesión propia
        proc(22, 21, 21, "sh"),
        proc(30, 10, 30, "claude"),  # otro CLI en el mismo shell no se toca
    ]
    records = agent_stop.agent_stop_records(10, processes)
    assert [item["pid"] for item in records] == [20, 21, 22]


def test_codex_binary_prefix_counts_as_codex():
    processes = [
        proc(1, 0, 1, "bash"),
        proc(4, 1, 4, "codex-x86_64-lin"),
        proc(5, 4, 9, "python3"),
    ]
    records = agent_stop.agent_stop_records(1, processes)
    assert [item["pid"] for item in records] == [4, 5]


def test_shell_without_agent_is_left_alone():
    processes = [proc(10, 1, 10, "zsh"), proc(11, 10, 11, "vim")]
    assert agent_stop.agent_stop_records(10, processes) == []


def test_interrupt_sends_sigint_and_kill_survives_only_same_starttime(monkeypatch, tmp_path):
    sent = []
    monkeypatch.setattr(agent_stop.os, "kill", lambda pid, sig: sent.append((pid, sig)))
    processes = [proc(10, 1, 10, "zsh", 3), proc(20, 10, 20, "grok", 8), proc(21, 20, 21, "node", 9)]
    records = agent_stop.interrupt_agent_tree(10, processes)
    assert sent == [(20, signal.SIGINT), (21, signal.SIGINT)]

    procdir = tmp_path / "proc"
    # still_same reads /proc; point it at a fixture via the open() path by
    # replacing still_same's file reads through a fake /proc layout is heavier.
    # Check the filter directly: a reused pid with another starttime is skipped.
    monkeypatch.setattr(agent_stop, "still_same", lambda recs: [rec for rec in recs if rec["start"] == 8])
    sent.clear()
    killed = agent_stop.kill_survivors(records)
    assert killed == [20]
    assert sent == [(20, signal.SIGKILL)]


def test_live_process_table_sees_this_python():
    processes = agent_stop.read_processes()
    me = next(item for item in processes if item["pid"] == os.getpid())
    assert me["ppid"] == os.getppid()
    assert me["comm"]
    assert me["start"] > 0


def test_desktop_ctrl_c_hooks_the_agent_stop():
    src = Path(__file__).resolve().parents[1].joinpath("bin", "cc-app").read_text()
    handler = src.split("def on_key(w, e):", 1)[1].split("\ndef ", 1)[0]
    helper = src.split("def _stop_agent_on_ctrl_c(term):", 1)[1].split("\ndef on_key", 1)[0]
    assert "_stop_agent_on_ctrl_c" in handler
    assert "interrupt_agent_tree" in helper
    assert "kill_survivors" in helper
