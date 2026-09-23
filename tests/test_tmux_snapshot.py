"""Exercise snapshot round trips against private tmux servers, never user sessions."""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import shlex
import subprocess
import uuid

import pytest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("tmux_snapshot", ROOT / "lib/tmux_snapshot.py")
tmux_snapshot = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(tmux_snapshot)


@pytest.fixture
def tmux(tmp_path, monkeypatch):
    if not shutil.which("tmux"):
        pytest.skip("tmux is required for real layout integration tests")
    socket_name = "comandos-snapshot-test-" + uuid.uuid4().hex
    # Resume through a plain shell, without loading the user's shell hooks.
    monkeypatch.setenv("SHELL", "/bin/sh")
    env = dict(os.environ)
    env.pop("TMUX", None)

    def run(*args):
        return subprocess.run(
            ["tmux", "-L", socket_name, "-f", "/dev/null", *args],
            capture_output=True, text=True, env=env, timeout=10,
        )

    # Keep the server alive when the session under test is removed, so restored
    # pane IDs differ and layout restoration must actually remap their identities.
    result = run("new-session", "-d", "-s", "test-keeper", "-c", str(tmp_path), "exec sleep 999")
    assert result.returncode == 0, result.stderr
    try:
        checked(run, "set-option", "-g", "default-shell", "/bin/sh")
        checked(run, "set-option", "-g", "default-command", "exec sleep 999")
        yield run
    finally:
        run("kill-server")


def checked(tmux, *args):
    result = tmux(*args)
    assert result.returncode == 0, f"tmux {args!r}: {result.stderr}"
    return result.stdout.strip()


def pane_geometry(tmux, session):
    rows = checked(
        tmux, "list-panes", "-s", "-t", session, "-F",
        "#{pane_id}\t#{window_index}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}",
    )
    return {parts[0]: tuple(map(int, parts[1:])) for row in rows.splitlines() if (parts := row.split("\t"))}


def session_state(tmux, session):
    return checked(
        tmux, "list-panes", "-s", "-t", session, "-F",
        "#{window_id}\t#{window_index}\t#{window_name}\t#{window_active}\t"
        "#{window_zoomed_flag}\t#{window_layout}\t#{pane_id}\t#{pane_active}\t"
        "#{pane_pid}\t#{pane_current_path}\t#{pane_start_command}",
    )


def make_split_session(tmux, tmp_path):
    session = "layout-roundtrip"
    directories = [tmp_path / name for name in ("project one", "project two", "claude account", "codex account", "other window")]
    for directory in directories:
        directory.mkdir()
    first = checked(
        tmux, "new-session", "-d", "-s", session, "-n", "Build & review",
        "-x", "180", "-y", "60", "-c", str(directories[0]), "-P", "-F", "#{pane_id}",
    )
    right = checked(tmux, "split-window", "-h", "-p", "37", "-t", first, "-c", str(directories[1]), "-P", "-F", "#{pane_id}")
    bottom_right = checked(tmux, "split-window", "-v", "-p", "29", "-t", right, "-c", str(directories[2]), "-P", "-F", "#{pane_id}")
    bottom_left = checked(tmux, "split-window", "-v", "-p", "43", "-t", first, "-c", str(directories[3]), "-P", "-F", "#{pane_id}")
    # Pane creation order now differs from visual order. Metadata must follow
    # pane identity rather than relying on enumeration or pane index order.
    checked(tmux, "swap-pane", "-d", "-s", first, "-t", bottom_right)
    checked(tmux, "select-pane", "-t", bottom_left)
    second = checked(
        tmux, "new-window", "-d", "-t", session + ":4", "-n", "Logs with spaces",
        "-c", str(directories[4]), "-P", "-F", "#{pane_id}",
    )
    checked(tmux, "set-window-option", "-t", session + ":0", "automatic-rename", "off")
    checked(tmux, "set-window-option", "-t", session + ":4", "automatic-rename", "off")
    checked(tmux, "resize-window", "-t", session + ":0", "-x", "180", "-y", "60")
    checked(tmux, "resize-window", "-t", session + ":4", "-x", "132", "-y", "44")
    checked(tmux, "select-window", "-t", session + ":4")
    panes = [first, right, bottom_right, bottom_left, second]
    metadata = {
        pane: {"agent": "codex" if index % 2 else "claude", "conversation": f"conversation-{index}", "account": f"account-{index}"}
        for index, pane in enumerate(panes)
    }
    return session, metadata, dict(zip(panes, map(str, directories)))


@pytest.mark.parametrize("zoomed", [False, True])
def test_roundtrip_preserves_geometry_and_conversations_after_panes_are_swapped(tmux, tmp_path, zoomed):
    session, metadata, directories = make_split_session(tmux, tmp_path)
    original_geometry = pane_geometry(tmux, session)
    if zoomed:
        checked(tmux, "resize-pane", "-Z", "-t", session + ":0")

    described = {}

    def describe(pane):
        described[pane["id"]] = dict(pane)
        return metadata[pane["id"]]

    snapshot = tmux_snapshot.capture_session(tmux, session, describe)
    windows = snapshot["windows"]
    assert [window["index"] for window in windows] == [0, 4]
    assert [window["name"] for window in windows] == ["Build & review", "Logs with spaces"]
    assert [(window["width"], window["height"]) for window in windows] == [(180, 60), (132, 44)]
    assert [bool(window["active"]) for window in windows] == [False, True]
    assert [bool(window["zoomed"]) for window in windows] == [zoomed, False]
    assert all(window["id"].startswith("@") and window["layout"] for window in windows)
    assert set(described) == set(metadata)
    saved_panes = {pane["id"]: pane for window in windows for pane in window["panes"]}
    for pane_id, expected in metadata.items():
        assert described[pane_id]["cwd"] == directories[pane_id]
        assert "index" in described[pane_id] and "active" in described[pane_id]
        assert {key: saved_panes[pane_id][key] for key in expected} == expected

    checked(tmux, "kill-session", "-t", session)
    resumed = {}

    def resume(pane):
        resumed[pane["id"]] = dict(pane)
        return "exec sleep " + str(900 + int(pane["conversation"].split("-")[-1]))

    restored_ids = tmux_snapshot.restore_session(tmux, session, snapshot, resume)
    assert set(restored_ids) == set(metadata)
    assert len(set(restored_ids.values())) == len(metadata)
    assert set(restored_ids).isdisjoint(restored_ids.values())
    assert set(resumed) == set(metadata)
    restored = tmux_snapshot.capture_session(tmux, session, lambda pane: {})
    for before, after in zip(windows, restored["windows"], strict=True):
        for key in ("index", "name", "width", "height", "active", "zoomed"):
            assert after[key] == before[key], key
        old_active = next(pane["id"] for pane in before["panes"] if pane["active"])
        new_active = next(pane["id"] for pane in after["panes"] if pane["active"])
        assert new_active == restored_ids[old_active]
    for pane_id, new_id in restored_ids.items():
        assert checked(tmux, "display-message", "-p", "-t", new_id, "#{pane_current_path}") == directories[pane_id]
        assert metadata[pane_id].items() <= resumed[pane_id].items()
        duration = 900 + int(metadata[pane_id]["conversation"].split("-")[-1])
        start_command = checked(tmux, "display-message", "-p", "-t", new_id, "#{pane_start_command}")
        argv = shlex.split(start_command)
        assert argv[:2] == ["/bin/sh", "-ilc"]
        assert argv[2].split(";", 1)[0] == f"exec sleep {duration}"

    if zoomed:
        checked(tmux, "resize-pane", "-Z", "-t", session + ":0")
    new_geometry = pane_geometry(tmux, session)
    assert {old: new_geometry[new] for old, new in restored_ids.items()} == original_geometry


def test_restore_leaves_existing_live_session_and_processes_untouched(tmux, tmp_path):
    session, metadata, _ = make_split_session(tmux, tmp_path)
    snapshot = tmux_snapshot.capture_session(tmux, session, lambda pane: metadata[pane["id"]])
    # The live session diverges after the snapshot; neither its layout nor its
    # processes should be replaced with the older state during app startup.
    checked(tmux, "split-window", "-h", "-t", session + ":4", "-c", str(tmp_path))
    before = session_state(tmux, session)

    def unexpected_resume(pane):
        pytest.fail("existing sessions must not run resume commands")

    assert tmux_snapshot.restore_session(tmux, session, snapshot, unexpected_resume) == {}
    assert session_state(tmux, session) == before


def test_restore_shell_pane_when_no_resume_command_is_available(tmux, tmp_path):
    pane = checked(tmux, "new-session", "-d", "-s", "plain-shell", "-c", str(tmp_path), "-P", "-F", "#{pane_id}")
    snapshot = tmux_snapshot.capture_session(tmux, "plain-shell", lambda pane: {"agent": "shell"})
    checked(tmux, "kill-session", "-t", "plain-shell")
    restored_ids = tmux_snapshot.restore_session(tmux, "plain-shell", snapshot, lambda pane: None)
    assert set(restored_ids) == {pane}
    assert checked(tmux, "display-message", "-p", "-t", restored_ids[pane], "#{pane_current_path}") == str(tmp_path)
    assert checked(tmux, "display-message", "-p", "-t", restored_ids[pane], "#{pane_dead}") == "0"


def test_snapshot_recovers_previous_valid_generation_when_current_file_is_corrupt(tmux, tmp_path):
    session, metadata, _ = make_split_session(tmux, tmp_path)
    first = tmux_snapshot.capture_session(tmux, session, lambda pane: metadata[pane["id"]])
    path = tmp_path / "state" / "layouts.json"
    tmux_snapshot.write_snapshot(path, {session: first})
    assert tmux_snapshot.read_snapshot(path)["sessions"] == {session: first}

    checked(tmux, "resize-window", "-t", session + ":0", "-x", "160", "-y", "55")
    second = tmux_snapshot.capture_session(tmux, session, lambda pane: metadata[pane["id"]])
    assert second != first
    tmux_snapshot.write_snapshot(path, {session: second})
    assert tmux_snapshot.read_snapshot(path)["sessions"] == {session: second}

    path.write_text('{"version": 2, "sessions":')
    assert tmux_snapshot.read_snapshot(path)["sessions"] == {session: first}
    # A successful write after corruption must not replace the good backup
    # with the corrupt generation.
    tmux_snapshot.write_snapshot(path, {session: second})
    path.write_text("broken again")
    assert tmux_snapshot.read_snapshot(path)["sessions"] == {session: first}


def test_capture_rejects_mid_capture_split_and_keeps_complete_saved_layout(tmux, tmp_path):
    session, metadata, _ = make_split_session(tmux, tmp_path)
    path = tmp_path / "layouts.json"
    complete = tmux_snapshot.capture_session(tmux, session, lambda pane: metadata[pane["id"]])
    tmux_snapshot.write_snapshot(path, {session: complete})
    original_bytes = path.read_bytes()
    changed = False

    def describe_while_splitting(pane):
        nonlocal changed
        if not changed:
            changed = True
            checked(tmux, "split-window", "-d", "-h", "-t", pane["id"], "-c", str(tmp_path))
        return metadata[pane["id"]]

    with pytest.raises(RuntimeError, match="[Ll]ayout|[Rr]esiz|[Cc]hanged"):
        sessions = tmux_snapshot.read_snapshot(path)["sessions"]
        sessions[session] = tmux_snapshot.capture_session(tmux, session, describe_while_splitting)
        tmux_snapshot.write_snapshot(path, sessions)
    assert path.read_bytes() == original_bytes
    assert tmux_snapshot.read_snapshot(path)["sessions"] == {session: complete}


def test_failed_write_preserves_current_generation_without_leaving_partial_files(tmp_path):
    path = tmp_path / "layouts.json"
    tmux_snapshot.write_snapshot(path, {})
    original_bytes = path.read_bytes()
    with pytest.raises(TypeError):
        tmux_snapshot.write_snapshot(path, {"unserializable": object()})
    assert path.read_bytes() == original_bytes
    assert sorted(p.name for p in tmp_path.iterdir()) == ["layouts.json"]


def test_snapshot_rejects_structurally_incomplete_json_and_uses_complete_backup(tmux, tmp_path):
    session, metadata, _ = make_split_session(tmux, tmp_path)
    snapshot = tmux_snapshot.capture_session(tmux, session, lambda pane: metadata[pane["id"]])
    path = tmp_path / "layouts.json"
    tmux_snapshot.write_snapshot(path, {session: snapshot})
    tmux_snapshot.write_snapshot(path, {session: snapshot})
    incomplete = json.loads(path.read_text())
    # The document still parses and claims version 2, but a pane required by
    # the native layout is missing. It cannot be used for a faithful restore.
    incomplete["sessions"][session]["windows"][0]["panes"].pop()
    path.write_text(json.dumps(incomplete))
    assert tmux_snapshot.read_snapshot(path)["sessions"] == {session: snapshot}


def test_restore_failure_preserves_a_workload_that_already_started(tmux, tmp_path):
    session, metadata, _ = make_split_session(tmux, tmp_path)
    snapshot = tmux_snapshot.capture_session(tmux, session, lambda pane: metadata[pane["id"]])
    checked(tmux, "kill-session", "-t", session)
    calls = 0

    def resume(pane):
        nonlocal calls
        calls += 1
        if calls == 1:
            return "exec sleep 901"
        # A real resumed job may have the same process name as the temporary
        # placeholders. Starting it must still protect it from rollback.
        assert "exec sleep 901" in checked(
            tmux, "display-message", "-p", "-t", session + ":0.0", "#{pane_start_command}",
        )
        raise LookupError("the next conversation is unavailable")

    with pytest.raises(LookupError, match="next conversation"):
        tmux_snapshot.restore_session(tmux, session, snapshot, resume)
    assert tmux("has-session", "-t", session).returncode == 0
    assert "exec sleep 901" in checked(
        tmux, "display-message", "-p", "-t", session + ":0.0", "#{pane_start_command}",
    )


def _session(*panes):
    return {'windows': [{'panes': [dict(p) for p in panes]}]}


def test_agent_without_id_keeps_previous_id_only_for_the_same_pane_process():
    previous = _session(
        {'id': '%1', 'pid': 10, 'command': 'codex', 'agent': 'codex', 'resume_id': 'old-codex', 'flags': ['-m', 'x']},
        {'id': '%2', 'pid': 20, 'command': 'grok', 'agent': 'grok', 'resume_id': 'old-grok'},
        {'id': '%3', 'pid': 30, 'command': 'claude', 'agent': 'claude', 'resume_id': 'old-claude'},
    )
    captured = _session(
        {'id': '%1', 'pid': 10, 'command': 'codex', 'agent': 'codex'},            # mismo proceso: hereda
        {'id': '%2', 'pid': 99, 'command': 'grok', 'agent': 'grok'},              # pane respawneado: sin id
        {'id': '%3', 'pid': 30, 'command': 'codex', 'agent': 'codex'},            # otro agente: sin id
        {'id': '%4', 'pid': 40, 'command': 'claude', 'agent': 'claude', 'resume_id': 'fresh'},
    )
    panes = tmux_snapshot.carry_resume_ids(captured, previous)['windows'][0]['panes']
    assert panes[0]['resume_id'] == 'old-codex' and panes[0]['flags'] == ['-m', 'x']
    assert 'resume_id' not in panes[1]
    assert 'resume_id' not in panes[2]
    assert panes[3]['resume_id'] == 'fresh'
    assert tmux_snapshot.carry_resume_ids(_session({'id': '%1', 'pid': 1, 'command': 'grok', 'agent': 'grok'}), None)
