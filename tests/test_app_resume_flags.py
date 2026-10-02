"""Resume flags without importing GTK or reading real provider sessions."""
import ast
import builtins
import glob
import io
import os
from pathlib import Path
import re
import shlex
from types import SimpleNamespace

import pytest


SOURCE = Path("bin/cc-app").read_text()
SID = "11111111-1111-1111-1111-111111111111"


def helpers():
    names = {"_sane_flags", "_proc_flags", "_codex_info_for_session",
             "resume_command", "exact_resume_command"}
    constants = {"_VALUE_FLAGS", "_RESUME_SKIP_FLAGS", "_ROLLOUT_RE"}
    tree = ast.parse(SOURCE)
    nodes = [node for node in tree.body
             if isinstance(node, ast.FunctionDef) and node.name in names
             or isinstance(node, ast.Assign)
             and any(isinstance(target, ast.Name) and target.id in constants
                     for target in node.targets)]
    namespace = dict(os=os, glob=glob, re=re, shlex=shlex,
                     _which_cli=lambda cli: "/fixture/" + cli)
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "bin/cc-app", "exec"), namespace)
    return namespace


@pytest.mark.parametrize("flags", [
    ["-m", "gpt-6.1-sol", "-c", 'model_reasoning_effort="high"',
     "--sandbox", "read-only", "--ask-for-approval", "untrusted",
     "--profile", "work"],
    ["--model", "gpt-6.1-sol", "--config", 'model_reasoning_effort="high"',
     "-c", "features.foo=true", "-s", "workspace-write", "-a", "on-request",
     "-p", "work", "--cd", "/project with spaces", "--add-dir", "/extra"],
    ["--model=gpt-6.1-sol", '-cmodel_reasoning_effort="high"',
     "--sandbox=read-only", "--ask-for-approval=untrusted", "--profile=work"],
])
def test_codex_retains_original_config_and_permission_flag_values(flags):
    assert helpers()["_sane_flags"](["resume", SID, *flags], agent="codex") == flags


def test_codex_orphan_value_flags_are_dropped_without_inserting_permissions():
    sane = helpers()["_sane_flags"]
    assert sane(["-c", "--sandbox", "--ask-for-approval", "--profile"], agent="codex") == []
    assert sane(["--last", "--resume", SID, "-r", SID, "-c", "x=1"], agent="codex") == ["-c", "x=1"]
    assert sane([], agent="codex") == []


def test_default_and_claude_keep_existing_continue_semantics():
    sane = helpers()["_sane_flags"]
    raw = ["-c", "--resume", SID, "--model", "sonnet", "--effort", "high"]
    expected = ["--model", "sonnet", "--effort", "high"]
    assert sane(raw) == expected
    assert sane(raw, agent="claude") == expected


def test_proc_flags_uses_the_requested_harness(monkeypatch):
    argv = b"/fixture/codex\0resume\0" + SID.encode() + b'\0-c\0model_reasoning_effort="high"\0--sandbox\0read-only\0'
    monkeypatch.setattr(builtins, "open", lambda *args, **kwargs: io.BytesIO(argv))
    assert helpers()["_proc_flags"](123, agent="codex") == [
        "-c", 'model_reasoning_effort="high"', "--sandbox", "read-only"]


def test_codex_snapshot_capture_selects_codex_flag_semantics():
    namespace = helpers()
    observed = []
    namespace.update(
        tmuxc=lambda *args: SimpleNamespace(returncode=0, stdout="123"),
        os=SimpleNamespace(listdir=lambda path: ["4"], readlink=lambda path: f"/fixture/rollout-test-{SID}.jsonl"),
        _proc_flags=lambda pid, agent="claude": observed.append((pid, agent)) or [])
    assert namespace["_codex_info_for_session"]("fixture", {})["id"] == SID
    assert observed == [(123, "codex")]


def test_exact_codex_resume_quotes_values_and_never_uses_last(tmp_path):
    namespace = helpers()
    sessions = tmp_path / "sessions/2026/10/02"
    sessions.mkdir(parents=True)
    rollout = sessions / f"rollout-test-{SID}.jsonl"
    rollout.touch()
    flags = ["-m", "gpt-6.1-sol", "-c", 'model_reasoning_effort="high"',
             "--sandbox", "read-only", "--ask-for-approval", "untrusted", "--profile", "my work"]
    pane = dict(agent="codex", codex_home=str(tmp_path), resume_id=SID,
                flags=["resume", "old-id", "--last", *flags])
    command = namespace["exact_resume_command"](pane)
    assert shlex.split(command)[1:] == ["codex", "resume", SID, *flags]
    rollout.unlink()
    assert namespace["exact_resume_command"](pane) is None
    pane.pop("resume_id")
    assert namespace["exact_resume_command"](pane) is None
