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


def test_terminal_reconnect_controller_retries_with_bounded_backoff():
    helper = extract_js_function(TERM_HTML, "createWebSocketReconnectController")
    script = textwrap.dedent(
        f"""
        {helper}
        const queued = [];
        let connects = 0;
        const setTimer = (fn, delay) => {{
          const timer = {{fn, delay, cancelled:false}};
          queued.push(timer);
          return timer;
        }};
        const clearTimer = timer => {{ timer.cancelled = true; }};
        const ctl = createWebSocketReconnectController(
          () => {{ connects += 1; }}, setTimer, clearTimer);

        const delays = [];
        for (let i = 0; i < 10; i += 1) {{
          delays.push(ctl.schedule());
          if (ctl.schedule() !== false) throw new Error('duplicate timer');
          queued.at(-1).fn();
        }}
        ctl.connected();
        const resetDelay = ctl.schedule();
        ctl.dispose();
        const afterDispose = ctl.schedule();
        console.log(JSON.stringify({{delays, connects, resetDelay, afterDispose}}));
        """
    )
    result = json.loads(subprocess.check_output(["node", "-e", script], text=True))

    assert result["connects"] == 10
    assert result["delays"][0] == 250
    assert max(result["delays"]) == 4000
    assert result["resetDelay"] == 250
    assert result["afterDispose"] is False


def test_terminal_socket_close_schedules_reconnect_without_disposing_resize():
    assert "reconnectController.schedule()" in TERM_HTML
    assert "ws.addEventListener('close', disposeResizeCoordinator)" not in TERM_HTML
    assert "term.reset()" in TERM_HTML
