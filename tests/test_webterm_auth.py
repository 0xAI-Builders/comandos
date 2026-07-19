import json
import os
import subprocess
import textwrap
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
ATTACH = ROOT / "bin" / "cc-webterm-attach"
TERM_HTML = (ROOT / "dash" / "term.html").read_text()
INDEX_HTML = (ROOT / "dash" / "index.html").read_text()
DASH_SOURCE = (ROOT / "bin" / "cc-dash").read_text()
WEBTERM_SOURCE = (ROOT / "bin" / "cc-webterm").read_text()


def extract_js_function(source, name):
    needle = f"function {name}("
    start = source.index(needle)
    brace = source.index("{", start)
    depth = 0
    for index in range(brace, len(source)):
        char = source[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[start:index + 1]
    raise AssertionError(f"Could not extract {name}")


def write_executable(path, body):
    path.write_text("#!/bin/sh\n" + body)
    path.chmod(0o755)


def run_attach(tmp_path, presented, session="demo"):
    home = tmp_path / "home"
    hooks = home / ".claude" / "hooks"
    hooks.mkdir(parents=True)
    (hooks / "dash-token").write_text("correct-token")
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    log = tmp_path / "tmux.log"
    write_executable(
        fake_bin / "tmux",
        'printf "%s\\n" "$*" >> "$TMUX_LOG"\n'
        'if [ "$1" = "has-session" ]; then exit 0; fi\n'
        'exit 0\n',
    )
    env = {
        **os.environ,
        "HOME": str(home),
        "PATH": f"{fake_bin}:{os.environ['PATH']}",
        "TMUX_LOG": str(log),
        "SHELL": "/bin/sh",
    }
    result = subprocess.run(
        [str(ATTACH), presented, session], env=env, text=True,
        capture_output=True,
    )
    return result, log.read_text() if log.exists() else ""


def test_attach_rejects_request_without_matching_dashboard_token(tmp_path):
    result, log = run_attach(tmp_path, "wrong-token")

    assert result.returncode != 0
    assert log == ""


def test_attach_uses_session_after_valid_dashboard_token(tmp_path):
    result, log = run_attach(tmp_path, "correct-token", "project-demo")

    assert result.returncode == 0
    assert "has-session -t =project-demo" in log
    assert "attach -t =project-demo" in log


def test_terminal_websocket_passes_token_before_session():
    helper = extract_js_function(TERM_HTML, "ttydWebSocketUrl")
    script = textwrap.dedent(
        f"""
        {helper}
        console.log(JSON.stringify(ttydWebSocketUrl(
          "ws://127.0.0.1:4779/ws", "dash secret", "project/demo"
        )));
        """
    )
    result = json.loads(
        subprocess.check_output(["node", "-e", script], text=True)
    )

    assert result == (
        "ws://127.0.0.1:4779/ws?arg=dash+secret&arg=project%2Fdemo"
    )


def test_dashboard_delivers_protected_token_to_terminal_frame():
    assert '"/webterm-token"' in DASH_SOURCE
    assert "webtermAccessToken" in INDEX_HTML
    assert "auth=${encodeURIComponent(accessToken)}" in INDEX_HTML
    assert "?arg=${encodeURIComponent(accessToken)}&arg=${encodeURIComponent(sess)}" in INDEX_HTML
    assert '"/webterm-token"' in (ROOT / "dash" / "sw.js").read_text()


def test_webterm_bootstraps_shared_dashboard_token():
    assert 'TOKEN_FILE="$HOOKS/dash-token"' in WEBTERM_SOURCE
    assert "ensure_dashboard_token" in WEBTERM_SOURCE
