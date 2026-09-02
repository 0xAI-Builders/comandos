import json
import os
import shutil
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HOOK = ROOT / "hooks" / "cc-status.sh"


def run_hook(
    tmp_path,
    count,
    *,
    first_project="needs-you",
    include_bad=True,
    include_non_file=False,
):
    home = tmp_path / "home"
    state = home / ".claude" / "hooks" / "state"
    wrappers = tmp_path / "bin"
    state.mkdir(parents=True)
    wrappers.mkdir()
    now = 2_000_000_000
    rows = [
        {"project": first_project, "status": "waiting", "ts": now},
        {"project": "finished", "status": "done", "ts": now - 10},
        {"project": "edge", "status": "waiting", "ts": now - 28800},
        {"project": "old", "status": "done", "ts": now - 28801},
    ]
    for i in range(count):
        value = rows[i] if i < len(rows) else {
            "project": f"idle-{i}", "status": "working", "ts": now
        }
        (state / f"{i:03}.json").write_text(json.dumps(value, indent=2))
    if include_bad:
        (state / "bad.json").write_text("{not-json")
    if include_non_file:
        (state / "vanished.json").mkdir()
    counter = tmp_path / "jq-count"
    real_jq = shutil.which("jq")
    assert real_jq is not None
    (wrappers / "date").write_text("#!/bin/sh\nprintf '%s\\n' 2000000000\n")
    (wrappers / "jq").write_text(
        "#!/bin/sh\nprintf x >> \"$JQ_COUNTER\"\nexec \"$REAL_JQ\" \"$@\"\n"
    )
    for path in wrappers.iterdir():
        path.chmod(0o755)
    env = {
        **os.environ,
        "HOME": str(home),
        "PATH": f"{wrappers}:{os.environ['PATH']}",
        "JQ_COUNTER": str(counter),
        "REAL_JQ": real_jq,
    }
    result = subprocess.run(
        [str(HOOK)], env=env, text=True, capture_output=True, check=True
    )
    return (
        result.stdout,
        len(counter.read_text()) if counter.exists() else 0,
        result.stderr,
    )


def test_status_hook_preserves_output_with_one_jq(tmp_path):
    output, calls, _stderr = run_hook(tmp_path, 4)
    # cc-status.sh ahora emite la paleta "aurora" con glifos nerd-font
    # (\uf0f3 atencion / \uf00c listo) y separador · entre proyecto y extra
    assert output == (
        "#[fg=#D08770]\uf0f3 needs-you · edge#[default]  "
        "#[fg=#A3BE8C]\uf00c finished#[default]"
    )
    assert calls == 1


def test_status_hook_process_count_does_not_scale(tmp_path):
    _output, calls, _stderr = run_hook(tmp_path, 40)
    assert calls == 1


def test_status_hook_preserves_newline_in_project(tmp_path):
    output, calls, _stderr = run_hook(tmp_path, 1, first_project="needs\nyou")
    # el salto de linea del proyecto se colapsa a " · " (una sola linea de barra)
    assert output == "#[fg=#D08770]\uf0f3 needs · you#[default]"
    assert calls == 1


def test_status_hook_empty_state_uses_no_jq(tmp_path):
    output, calls, _stderr = run_hook(tmp_path, 0, include_bad=False)
    assert output == ""
    assert calls == 0


def test_status_hook_skips_non_files_without_stderr_noise(tmp_path):
    output, calls, stderr = run_hook(
        tmp_path, 1, include_bad=False, include_non_file=True
    )
    assert "needs-you" in output
    assert calls == 1
    assert stderr == ""
