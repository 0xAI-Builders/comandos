"""Reusable cc-dash server fixture for endpoint tests.

Usage in a test module::

    from dash_harness import dash          # noqa: F401  (pytest fixture)

    def test_x(dash):
        body = dash.get("/commands/catalog?session=demo&pane=%1")

Extra child environment (TMUX_TMPDIR, XDG_CONFIG_HOME, ...) comes from an
optional `dash_env` fixture that the test module itself defines::

    @pytest.fixture
    def dash_env(tmp_path):
        return {"TMUX_TMPDIR": str(tmp_path / "tmux")}

Every request is sent as if it came through a remote proxy (X-Forwarded-For),
so the token gate is really exercised: `token=False` must yield 401, while the
default sends the token read from $HOME/.claude/hooks/dash-token.
"""
import json
import os
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
_REMOTE_PEER = "100.64.0.9"
_TEST_TOKEN = "dash-harness-test-token"


def _free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Dash:
    def __init__(self, home, port):
        self.home = Path(home)
        self.port = port
        self.url = f"http://127.0.0.1:{port}"

    def _token(self):
        return (self.home / ".claude" / "hooks" / "dash-token").read_text().strip()

    def _open(self, method, path, body=None, token=True):
        headers = {"X-Forwarded-For": _REMOTE_PEER}
        if token:
            headers["X-Comandos-Token"] = self._token()
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        req = urllib.request.Request(self.url + path, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return r.status, r.read()
        except urllib.error.HTTPError as e:
            return e.code, e.read()

    def get(self, path):
        status, raw = self._open("GET", path)
        assert status == 200, f"GET {path} -> {status}: {raw[:300]!r}"
        return json.loads(raw or b"{}")

    def get_status(self, path, token=True):
        return self._open("GET", path, token=token)[0]

    def post(self, path, body):
        status, raw = self._open("POST", path, body)
        assert status == 200, f"POST {path} -> {status}: {raw[:300]!r}"
        return json.loads(raw or b"{}")

    def post_status(self, path, body, token=True):
        return self._open("POST", path, body, token=token)[0]


@pytest.fixture
def dash(tmp_path, request):
    try:  # optional per-module fixture; a test module defines `dash_env`
        dash_env = request.getfixturevalue("dash_env")
    except pytest.FixtureLookupError:
        dash_env = {}
    port = _free_port()
    env = os.environ.copy()
    env.pop("TMUX", None)  # a set $TMUX overrides TMUX_TMPDIR
    env["HOME"] = str(tmp_path)
    env.update({k: str(v) for k, v in dash_env.items()})
    hooks = tmp_path / ".claude" / "hooks"
    hooks.mkdir(parents=True, exist_ok=True)
    (hooks / "state").mkdir(exist_ok=True)
    (hooks / "app-tabs.json").write_text("{}")
    token = hooks / "dash-token"
    if not token.exists():
        fd = os.open(token, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w") as f:
            f.write(_TEST_TOKEN)
    proc = subprocess.Popen(
        [sys.executable, str(ROOT / "bin" / "cc-dash"), str(port), "--no-open"],
        env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    try:
        for _ in range(100):
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                    break
            except OSError:
                time.sleep(0.1)
        else:
            proc.kill()
            _out, err = proc.communicate(timeout=2)
            raise RuntimeError(f"cc-dash did not start: {err.decode(errors='ignore')[:500]}")
        yield Dash(tmp_path, port)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        for stream in (proc.stdout, proc.stderr):
            if stream:
                stream.close()
