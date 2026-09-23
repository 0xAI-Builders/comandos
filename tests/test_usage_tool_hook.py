#!/usr/bin/env python3
"""hooks/cc-usage-tool.sh corre en CADA Pre/PostToolUse: debe ser barato
(una sola jq) y mantener exactamente el mismo evento. tmux y cc_usage.py
estan simulados; nunca toca el servidor tmux real ni la DB real."""
import json
import stat
import subprocess
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
HOOK = ROOT / "hooks" / "cc-usage-tool.sh"


def _exe(path, text):
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


@pytest.fixture
def run(tmp_path):
    home = tmp_path / "home"
    (home / ".local" / "bin").mkdir(parents=True)
    stubs = tmp_path / "stubs"
    stubs.mkdir()
    log = tmp_path / "events.log"
    jq_log = tmp_path / "jq.log"
    _exe(home / ".local" / "bin" / "cc_usage.py",
         "#!/usr/bin/env python3\nimport sys\n"
         f"open({str(log)!r}, 'a').write(sys.argv[1] + ' ' + sys.stdin.read().strip() + '\\n')\n")
    _exe(stubs / "tmux", "#!/usr/bin/env bash\n"
         "[ \"$1\" = display-message ] && { echo \"${FAKE_TMUX_SESSION:-sess}\"; exit 0; }\nexit 1\n")
    _exe(stubs / "jq", f"#!/usr/bin/env bash\necho x >> {jq_log}\nexec /usr/bin/jq \"$@\"\n")

    def _run(payload, pane="%4", session="sess"):
        for f in (log, jq_log):
            f.unlink(missing_ok=True)
        env = {"HOME": str(home), "PATH": f"{stubs}:/usr/bin:/bin", "FAKE_TMUX_SESSION": session}
        if pane:
            env["TMUX_PANE"] = pane
        proc = subprocess.run(["bash", str(HOOK)], input=json.dumps(payload), text=True,
                              capture_output=True, env=env, timeout=10)
        assert proc.returncode == 0, proc.stderr
        deadline = time.monotonic() + 5
        while payload.get("_expect", True) and not log.exists() and time.monotonic() < deadline:
            time.sleep(0.02)
        jq_calls = len(jq_log.read_text().splitlines()) if jq_log.exists() else 0
        rows = []
        if log.exists():
            rows = [json.loads(l.split(" ", 1)[1]) for l in log.read_text().splitlines()]
        return rows, jq_calls

    return _run


def test_pre_tool_use_emits_start_with_single_jq(run):
    rows, jq_calls = run({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_use_id": "t1"})
    assert len(rows) == 1
    ev = rows[0]
    assert ev["phase"] == "start" and ev["tool_name"] == "Bash" and ev["tool_use_id"] == "t1"
    assert ev["tmux_session"] == "sess" and ev["tmux_pane"] == "%4"
    assert ev["skill_name"] == "" and ev["confidence"] == "exact"
    assert isinstance(ev["at_ms"], int) and ev["at_ms"] > 1_600_000_000_000
    assert jq_calls <= 1, f"{jq_calls} jq por tool call"


@pytest.mark.parametrize("response,phase", [
    ({"is_error": True}, "failed"),
    ({"error": "boom"}, "failed"),
    ({"stdout": "ok"}, "success"),
    ("texto plano", "success"),
])
def test_post_tool_use_phase(run, response, phase):
    rows, jq_calls = run({"hook_event_name": "PostToolUse", "tool_name": "Read",
                          "tool_response": response})
    assert rows[0]["phase"] == phase
    assert jq_calls <= 1


@pytest.mark.parametrize("skill,expected", [("superpowers:tdd", "superpowers:tdd"),
                                            ("bad skill;rm", ""), ("ok\nrm -rf", ""),
                                            ({"x": 1}, "")])
def test_skill_name_is_validated(run, skill, expected):
    rows, _ = run({"hook_event_name": "PreToolUse", "tool_name": "Skill",
                   "tool_input": {"skill": skill}})
    assert rows[0]["skill_name"] == expected


def test_skill_name_only_for_skill_tool(run):
    rows, _ = run({"hook_event_name": "PreToolUse", "tool_name": "Bash",
                   "tool_input": {"skill": "x"}})
    assert rows[0]["skill_name"] == ""


@pytest.mark.parametrize("payload,pane,session", [
    ({"hook_event_name": "Stop", "tool_name": "Bash"}, "%4", "sess"),
    ({"hook_event_name": "PreToolUse", "tool_name": ""}, "%4", "sess"),
    ({"hook_event_name": "PreToolUse", "tool_name": "Bash"}, None, "sess"),
    ({"hook_event_name": "PreToolUse", "tool_name": "Bash"}, "%4", "bad sess"),
])
def test_ignored_events_emit_nothing(run, payload, pane, session):
    rows, _ = run(dict(payload, _expect=False), pane=pane, session=session)
    assert rows == []
