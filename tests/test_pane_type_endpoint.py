"""POST /pane/type: literal text into a pane, never Enter (private tmux only)."""
import os
import subprocess
import time

import pytest

from dash_harness import dash  # noqa: F401  (pytest fixture)


@pytest.fixture
def tmux_dir(tmp_path):
    d = tmp_path / "tmux-priv"
    d.mkdir()
    return d


@pytest.fixture
def dash_env(tmux_dir):
    # The dash child and the test's tmux share ONE private TMUX_TMPDIR.
    return {"TMUX_TMPDIR": str(tmux_dir)}


class PrivateTmux:
    def __init__(self, tmux_dir):
        self.env = os.environ.copy()
        self.env.pop("TMUX", None)
        self.env["TMUX_TMPDIR"] = str(tmux_dir)

    def run(self, *args):
        return subprocess.run(["tmux", *args], env=self.env, capture_output=True, text=True, timeout=10)

    @property
    def pane(self):
        return self.run("display-message", "-p", "-t", "demo", "#{pane_id}").stdout.strip()

    def capture(self):
        return self.run("capture-pane", "-p", "-t", "demo").stdout


@pytest.fixture
def private_tmux(tmux_dir):
    t = PrivateTmux(tmux_dir)
    r = t.run("new-session", "-d", "-s", "demo", "-x", "120", "-y", "30",
              "env PS1='PROMPT> ' bash --norc --noprofile")
    assert r.returncode == 0, r.stderr
    try:
        for _ in range(50):
            if "PROMPT>" in t.capture():
                break
            time.sleep(0.1)
        else:
            raise RuntimeError("private shell never showed its prompt")
        yield t
    finally:
        t.run("kill-server")  # only this TMUX_TMPDIR's server


def test_type_route_writes_literal_text_without_enter(dash, private_tmux):
    r = dash.post("/pane/type", {"session": "demo", "pane": private_tmux.pane, "text": "/effort max", "requestId": "r1"})
    assert r["ok"] and r["typed"] == 11
    screen = private_tmux.capture()
    assert "/effort max" in screen and screen.rstrip().endswith("/effort max")   # sigue en el prompt, sin ejecutar


def test_type_route_is_idempotent_per_request_id(dash, private_tmux):
    dash.post("/pane/type", {"session": "demo", "pane": private_tmux.pane, "text": "abc", "requestId": "r2"})
    dash.post("/pane/type", {"session": "demo", "pane": private_tmux.pane, "text": "abc", "requestId": "r2"})
    assert private_tmux.capture().count("abc") == 1


def test_type_route_rejects_newline_and_unknown_pane(dash, private_tmux):
    assert dash.post_status("/pane/type", {"session": "demo", "pane": private_tmux.pane, "text": "ls\n"}) == 400
    assert dash.post_status("/pane/type", {"session": "demo", "pane": "%999", "text": "ls"}) == 404


def test_type_route_needs_a_token(dash, private_tmux):
    body = {"session": "demo", "pane": private_tmux.pane, "text": "abc", "requestId": "r3"}
    assert dash.post_status("/pane/type", body, token=False) == 401
    assert "abc" not in private_tmux.capture()


def test_second_type_on_the_same_pane_while_typing_gets_409_and_does_not_interleave(dash, private_tmux):
    import threading
    pane = private_tmux.pane
    long_text = "x" * 60 + "END"
    first = {}
    t = threading.Thread(target=lambda: first.update(dash.post("/pane/type", {
        "session": "demo", "pane": pane, "text": long_text, "requestId": "slow"})))
    t.start()
    time.sleep(0.25)   # 63 chars over ~1.2s budget: still typing
    assert dash.post_status("/pane/type", {"session": "demo", "pane": pane, "text": "ZZZ", "requestId": "fast"}) == 409
    t.join(timeout=10)
    assert first["ok"] and first["typed"] == 63
    screen = private_tmux.capture()
    assert long_text in screen.replace("\n", "") and "ZZZ" not in screen


def test_type_route_types_a_semicolon_literally(dash, private_tmux):
    r = dash.post("/pane/type", {"session": "demo", "pane": private_tmux.pane, "text": "a;b", "requestId": "semi"})
    assert r["ok"] and r["typed"] == 3
    assert private_tmux.capture().rstrip().endswith("a;b")


def test_type_route_requires_a_valid_pane(dash, private_tmux):
    assert dash.post_status("/pane/type", {"session": "demo", "text": "abc"}) == 400
    assert dash.post_status("/pane/type", {"session": "demo", "pane": "demo", "text": "abc"}) == 400
    assert "abc" not in private_tmux.capture()
