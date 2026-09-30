"""The news summarizer drives Claude, Codex, Grok, agy and OpenCode as helpers.
With COMANDOS_SILENT_AGENT=1 their hooks must not create notices, states or
phantom sessions."""
import os
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


def run(cmd, home, stdin, args=()):
    env = {**os.environ, "HOME": str(home), "COMANDOS_SILENT_AGENT": "1", "XDG_STATE_HOME": str(home / "state")}
    return subprocess.run([str(ROOT / cmd), *args], input=stdin, capture_output=True, text=True, env=env, timeout=20)


@pytest.mark.parametrize("cmd,args,stdin", [
    ("hooks/cc-notify.sh", (), '{"hook_event_name":"Stop","cwd":"/tmp/x","session_id":"s"}'),
    ("hooks/cc-notify.sh", ("--agent", "grok", "--event", "done", "--cwd", "/tmp/x"), "{}"),
    ("adapters/agy-hooks.sh", ("done",), '{"workspacePaths":["/tmp/x"],"conversationId":"c"}'),
    ("adapters/codex-hooks.sh", (), '{"hook_event_name":"Stop","cwd":"/tmp/x"}'),
])
def test_silent_helper_agents_leave_no_trace(tmp_path, cmd, args, stdin):
    (tmp_path / ".claude" / "hooks").mkdir(parents=True)
    before = sorted(p.relative_to(tmp_path) for p in tmp_path.rglob("*"))
    out = run(cmd, tmp_path, stdin, args)
    assert out.returncode == 0
    after = sorted(p.relative_to(tmp_path) for p in tmp_path.rglob("*"))
    assert after == before
    if cmd.endswith("agy-hooks.sh"):
        assert out.stdout.strip() == "{}"
