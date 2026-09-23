#!/usr/bin/env python3
"""Pruebas de comportamiento de hooks/cc-notify.sh.

Todo corre en un HOME temporal con stubs de curl/tmux en el PATH: nunca se
toca ~/.claude, el servidor tmux por defecto, cc-notifyd ni Telegram real.
"""
import json
import os
import stat
import subprocess
import threading
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
NOTIFY = ROOT / "hooks" / "cc-notify.sh"
FAKE_TOKEN = "123456:FAKE-TOKEN-never-real"

CURL_STUB = r"""#!/usr/bin/env python3
import json, os, sys, time
stdin = ""
if "-K" in sys.argv or "--config" in sys.argv:
    stdin = sys.stdin.read()
with open(os.environ["CURL_LOG"], "a") as f:
    f.write(json.dumps({"argv": sys.argv[1:], "stdin": stdin}) + "\n")
time.sleep(float(os.environ.get("CURL_SLEEP", "0") or 0))
sys.stdout.write(os.environ.get("CURL_OUT", ""))
"""

TMUX_STUB = r"""#!/usr/bin/env bash
# tmux falso: jamas habla con un servidor real
if [ "$1" = "display-message" ]; then
  printf '%s\n' "${FAKE_TMUX_SESSION:-}"
  exit 0
fi
exit 1
"""

USAGE_STUB = r"""#!/usr/bin/env python3
import os, sys
with open(os.environ["USAGE_LOG"], "a") as f:
    f.write(sys.argv[1] + " " + sys.stdin.read().strip() + "\n")
"""


def _exe(path, text):
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


class Env:
    def __init__(self, tmp_path):
        self.home = tmp_path / "home"
        self.hooks = self.home / ".claude" / "hooks"
        self.state = self.hooks / "state"
        self.hooks.mkdir(parents=True)
        self.stubs = tmp_path / "stubs"
        self.stubs.mkdir()
        _exe(self.stubs / "curl", CURL_STUB)
        _exe(self.stubs / "tmux", TMUX_STUB)
        self.curl_log = tmp_path / "curl.jsonl"
        self.usage_log = tmp_path / "usage.log"
        (self.hooks / "cc-notify.conf").write_text(
            "SOUND_ENABLED=0\nSPEAK_ATTENTION=0\nSPEAK_DONE=0\nDESKTOP_NOTIFY=1\n"
            "TELEGRAM_ENABLED=1\nCC_LANG=es\n")
        self.env = {
            "HOME": str(self.home),
            "PATH": f"{self.stubs}:/usr/bin:/bin",
            "CURL_LOG": str(self.curl_log),
            "USAGE_LOG": str(self.usage_log),
            "LANG": "C.UTF-8",
        }

    def telegram(self, dedicated=True):
        key = "CC_TELEGRAM_BOT_TOKEN" if dedicated else "TELEGRAM_BOT_TOKEN"
        (self.hooks / "telegram.env").write_text(f"{key}={FAKE_TOKEN}\nTELEGRAM_CHAT_ID=42\n")

    def usage_script(self):
        bindir = self.home / ".local" / "bin"
        bindir.mkdir(parents=True, exist_ok=True)
        _exe(bindir / "cc_usage.py", USAGE_STUB)

    def run(self, payload=None, args=(), pane=None, session=None, extra=None, timeout=20):
        env = dict(self.env)
        if pane:
            env["TMUX_PANE"] = pane
            env["FAKE_TMUX_SESSION"] = session or ""
        env.update(extra or {})
        started = time.monotonic()
        proc = subprocess.run(
            ["bash", str(NOTIFY), *args],
            input=json.dumps(payload) if payload is not None else "",
            capture_output=True, text=True, env=env, timeout=timeout)
        return proc, time.monotonic() - started

    def curl_calls(self, wait_for=0, timeout=10):
        deadline = time.monotonic() + timeout
        while True:
            calls = []
            if self.curl_log.exists():
                calls = [json.loads(l) for l in self.curl_log.read_text().splitlines() if l]
            if len(calls) >= wait_for or time.monotonic() > deadline:
                return calls
            time.sleep(0.05)


@pytest.fixture
def env(tmp_path):
    return Env(tmp_path)


def state_files(env):
    return sorted(p.name for p in env.state.glob("*.json"))


# ---- Bug 6: escrituras atomicas de estado y timeline ----------------------

def test_state_write_is_atomic_for_concurrent_readers(env):
    stop = threading.Event()
    bad = []
    target = env.state / "proj.json"

    def reader():
        while not stop.is_set():
            try:
                raw = target.read_text()
            except FileNotFoundError:
                continue
            try:
                json.loads(raw)
            except ValueError:
                bad.append(raw)

    t = threading.Thread(target=reader, daemon=True)
    t.start()
    try:
        for _ in range(25):
            env.run({"hook_event_name": "UserPromptSubmit", "cwd": "/tmp/proj"})
    finally:
        stop.set()
        t.join(timeout=5)
    assert not bad, f"un lector vio estado truncado: {bad[:3]!r}"
    assert state_files(env) == ["proj.json"]
    assert not [p for p in env.state.iterdir() if p.name != "proj.json"], "quedo basura temporal"


def test_events_trim_keeps_concurrent_appends(env):
    events = env.hooks / "events.jsonl"
    events.write_text("".join(
        json.dumps({"project": f"old{i}", "status": "done", "detail": "", "ts": 1}) + "\n"
        for i in range(2000)))
    procs = []
    for i in range(30):
        e = dict(env.env)
        procs.append(subprocess.Popen(
            ["bash", str(NOTIFY)], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, env=e, text=True))
    for i, p in enumerate(procs):
        p.stdin.write(json.dumps({"hook_event_name": "UserPromptSubmit", "cwd": f"/tmp/new{i}"}))
        p.stdin.close()
    for p in procs:
        p.wait(timeout=30)
    rows = [json.loads(l) for l in events.read_text().splitlines() if l]
    projects = {r["project"] for r in rows}
    missing = [f"new{i}" for i in range(30) if f"new{i}" not in projects]
    assert not missing, f"se perdieron eventos al recortar: {missing}"
    assert len(rows) <= 2000
    leftovers = [p.name for p in env.hooks.glob("events.jsonl.*") if p.name != "events.jsonl.lock"]
    assert not leftovers, f"quedo un temporal del recorte: {leftovers}"
