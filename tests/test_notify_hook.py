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
if [ "$1" = "display-message" ] && [[ "$*" == *pane_pid* ]]; then
  printf '%s\n' "${FAKE_PANE_PID:-}"
  exit 0
fi
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


# ---- Bug 8: Grok, un Stop tardio del prompt A no pisa el working de B ------

def grok(event, prompt, **extra):
    return {"hookEventName": event, "sessionId": "gs1", "promptId": prompt,
            "cwd": "/tmp/gproj", **extra}


def test_grok_late_stop_does_not_overwrite_newer_prompt(env):
    (env.hooks / "cc-notify.conf").write_text(
        "SOUND_ENABLED=0\nSPEAK_ATTENTION=0\nSPEAK_DONE=0\nDESKTOP_NOTIFY=0\nTELEGRAM_ENABLED=0\n")
    env.run(grok("UserPromptSubmit", "b"))
    state = env.state / "gproj.json"
    assert json.loads(state.read_text())["status"] == "working"
    env.run(grok("Stop", "a", lastAssistantMessage="viejo"))
    assert json.loads(state.read_text())["status"] == "working", "el Stop tardio de A piso a B"
    env.run(grok("Stop", "b", lastAssistantMessage="nuevo"))
    after = json.loads(state.read_text())
    assert after["status"] == "done"
    env.run(grok("SessionEnd", "b"))
    assert not state.exists()
    assert not list(env.state.iterdir()), "SessionEnd debe limpiar tambien el evento vigente"


# ---- Bug 9: at_ms del lifecycle con resolucion de milisegundos -------------

def lifecycle_rows(env, wait_for, timeout=10):
    deadline = time.monotonic() + timeout
    while True:
        rows = []
        if env.usage_log.exists():
            rows = [json.loads(l.split(" ", 1)[1]) for l in env.usage_log.read_text().splitlines()
                    if l.startswith("lifecycle ")]
        if len(rows) >= wait_for or time.monotonic() > deadline:
            return rows
        time.sleep(0.05)


def test_lifecycle_at_ms_has_millisecond_resolution(env):
    env.usage_script()
    before = int(time.time() * 1000)
    for _ in range(3):
        env.run({"hook_event_name": "UserPromptSubmit", "cwd": "/tmp/proj"},
                pane="%3", session="sess")
    rows = lifecycle_rows(env, 3)
    assert len(rows) == 3
    stamps = [r["at_ms"] for r in rows]
    assert all(before - 1000 <= s <= int(time.time() * 1000) + 1000 for s in stamps), stamps
    assert any(s % 1000 for s in stamps), f"at_ms sigue truncado a segundos: {stamps}"
    assert rows[0]["tmux_pane"] == "%3" and rows[0]["tmux_session"] == "sess"


# ---- Bugs 3 y 4: hook rapido y token fuera del argv ------------------------

def telegram_calls(calls):
    return [c for c in calls if "api.telegram.org" in (c["stdin"] + " ".join(c["argv"]))]


def test_hook_returns_fast_while_notifications_run_detached(env):
    env.telegram()
    (env.hooks / "cc-notify.conf").write_text(
        "SOUND_ENABLED=1\nSPEAK_ATTENTION=1\nSPEAK_DONE=1\nDESKTOP_NOTIFY=1\n"
        "TELEGRAM_ENABLED=1\nCC_LANG=es\n")
    # voz lenta que hereda stdout: el harness esperaria su EOF
    _exe(env.stubs / "spd-say", "#!/usr/bin/env bash\necho hablando; sleep 3\n")
    proc, elapsed = env.run(
        {"hook_event_name": "Stop", "cwd": "/tmp/proj"},
        pane="%3", session="sess", extra={"CURL_SLEEP": "3"})
    assert proc.returncode == 0
    assert elapsed < 2.0, f"el hook bloqueo {elapsed:.1f}s (capture_output espera EOF)"
    assert proc.stdout == ""
    calls = env.curl_calls(wait_for=2, timeout=15)
    assert any("127.0.0.1:4778/notify" in " ".join(c["argv"]) for c in calls), calls
    assert telegram_calls(calls), calls


def test_telegram_token_never_appears_in_argv(env):
    env.telegram()
    env.run({"hook_event_name": "Notification", "cwd": "/tmp/proj", "message": "permiso"},
            pane="%3", session="sess")
    env.run({"hook_event_name": "Stop", "cwd": "/tmp/proj"}, pane="%3", session="sess")
    calls = env.curl_calls(wait_for=4)
    tg = telegram_calls(calls)
    assert len(tg) == 2, calls
    for call in calls:
        assert FAKE_TOKEN not in " ".join(call["argv"]), call["argv"]
    assert all(FAKE_TOKEN in c["stdin"] and "sendMessage" in c["stdin"] for c in tg)


def test_plain_bot_token_also_stays_out_of_argv(env):
    env.telegram(dedicated=False)
    env.run({"hook_event_name": "Stop", "cwd": "/tmp/proj"})
    calls = env.curl_calls(wait_for=2)
    assert telegram_calls(calls)
    assert all(FAKE_TOKEN not in " ".join(c["argv"]) for c in calls)


# ---- Bugs 1 y 5: botones de Telegram con pane y callback_data seguro ------

def reply_markup(call):
    for arg in call["argv"]:
        if arg.startswith("reply_markup="):
            return json.loads(arg[len("reply_markup="):])
    return None


def callbacks(markup):
    return [b["callback_data"] for row in markup["inline_keyboard"] for b in row]


def waiting(env, cwd="/tmp/proj", **kw):
    env.run({"hook_event_name": "Notification", "cwd": cwd, "message": "permiso"}, **kw)
    tg = telegram_calls(env.curl_calls(wait_for=2))
    assert len(tg) == 1, tg
    return tg[0]


def test_telegram_buttons_carry_the_pane(env):
    env.telegram()
    markup = reply_markup(waiting(env, pane="%12", session="sess"))
    assert callbacks(markup) == ["k|sess|1|12", "k|sess|2|12", "k|sess|3|12",
                                 "k|sess|Enter|12", "k|sess|Escape|12"]


def test_notifyd_payload_carries_the_pane(env):
    env.telegram()
    env.run({"hook_event_name": "Stop", "cwd": "/tmp/proj"}, pane="%12", session="sess")
    calls = env.curl_calls(wait_for=2)
    notifyd = [c for c in calls if "127.0.0.1:4778/notify" in " ".join(c["argv"])]
    assert notifyd
    argv = notifyd[0]["argv"]
    payload = json.loads(argv[argv.index("-d") + 1])
    assert payload["session"] == "sess" and payload["pane"] == "%12"


def test_long_session_falls_back_to_short_local_token(env):
    env.telegram()
    sess = "s" * 80
    markup = reply_markup(waiting(env, pane="%1234567", session=sess))
    datas = callbacks(markup)
    assert all(len(d.encode()) <= 64 for d in datas), datas
    assert all(d.startswith("t|") for d in datas), datas
    token = datas[0].split("|")[1]
    assert {d.split("|")[1] for d in datas} == {token}
    target = json.loads((env.hooks / "tg-targets" / f"{token}.json").read_text())
    assert target == {"session": sess, "pane": "%1234567"}


def test_unsafe_project_name_does_not_break_waiting_notification(env):
    env.telegram()
    call = waiting(env, cwd='/tmp/pro"j x')
    markup = reply_markup(call)   # json.loads falla si el nombre rompe el JSON
    datas = callbacks(markup)
    assert len(datas) == 5 and all(len(d.encode()) <= 64 for d in datas)


def test_sent_message_id_maps_to_real_session_and_pane(env):
    env.telegram()
    env.run({"hook_event_name": "Stop", "cwd": "/tmp/proj"}, pane="%12", session="real-sess",
            extra={"CURL_OUT": '{"ok":true,"result":{"message_id":777}}'})
    target = env.hooks / "tg-targets" / "msg-777.json"
    deadline = time.monotonic() + 10
    while not target.exists() and time.monotonic() < deadline:
        time.sleep(0.05)
    data = json.loads(target.read_text())
    assert data["session"] == "real-sess" and data["pane"] == "%12"


# ---- N1: el hook conserva la identidad que antes se perdia -----------------

import sqlite3  # noqa: E402
import sys  # noqa: E402

sys.path.insert(0, str(ROOT / "lib"))
import event_store  # noqa: E402


def v2_events(env):
    db = env.home / ".local" / "state" / "comandos" / "app-state.sqlite3"
    if not db.exists():
        return []
    conn = sqlite3.connect(str(db))
    try:
        return event_store.list_events(conn, limit=500)
    finally:
        conn.close()


def quiet(env):
    (env.hooks / "cc-notify.conf").write_text(
        "SOUND_ENABLED=0\nSPEAK_ATTENTION=0\nSPEAK_DONE=0\nDESKTOP_NOTIFY=0\nCC_LANG=es\n")


def test_claude_hook_records_identity_and_keeps_old_timeline(env):
    quiet(env)
    env.run({"hook_event_name": "UserPromptSubmit", "cwd": "/tmp/proj", "session_id": "conv-1"},
            pane="%12", session="sess", extra={"FAKE_PANE_PID": str(os.getpid())})
    env.run({"hook_event_name": "Notification", "cwd": "/tmp/proj", "session_id": "conv-1",
             "message": "Claude needs your permission to use Bash",
             "notification_type": "permission_prompt"}, pane="%12", session="sess")
    env.run({"hook_event_name": "Notification", "cwd": "/tmp/proj", "session_id": "conv-1",
             "message": "Claude is waiting for your input", "notification_type": "idle_prompt"},
            pane="%12", session="sess")
    evs = v2_events(env)
    assert [e["kind"] for e in evs] == ["prompt_accepted", "permission_requested", "input_requested"]
    first = evs[0]
    assert (first["sessionKey"], first["paneId"], first["conversationId"]) == ("sess", "%12", "conv-1")
    assert first["harness"] == "claude" and first["correlation"] == "local"
    assert first["evidence"] == "confirmed" and first["processKey"].startswith(f"{os.getpid()}-")
    assert first["projectKey"] == "proj"
    legacy = [json.loads(l) for l in (env.hooks / "events.jsonl").read_text().splitlines()]
    assert [r["status"] for r in legacy] == ["working", "waiting", "waiting"]


def test_grok_prompt_id_is_the_turn_and_cancellation_is_recorded(env):
    quiet(env)
    env.run(grok("UserPromptSubmit", "p1"), pane="%4", session="gs")
    env.run(grok("StopCancelled", "p1"), pane="%4", session="gs")
    evs = v2_events(env)
    assert [(e["kind"], e["turnId"], e["correlation"]) for e in evs] == [
        ("prompt_accepted", "p1", "source"), ("turn_cancelled", "p1", "source")]
    assert evs[0]["conversationId"] == "gs1"


def codex_env(env):
    link = env.hooks / "cc-notify.sh"
    link.symlink_to(NOTIFY)
    quiet(env)


def run_codex(env, payload, pane="%5", session="csess"):
    e = dict(env.env, TMUX_PANE=pane, FAKE_TMUX_SESSION=session)
    return subprocess.run(["bash", str(ROOT / "adapters" / "codex-hooks.sh")], input=json.dumps(payload),
                          capture_output=True, text=True, env=e, timeout=20)


def test_codex_hooks_keep_session_turn_and_request(env):
    codex_env(env)
    run_codex(env, {"hook_event_name": "UserPromptSubmit", "cwd": "/tmp/cproj",
                    "session_id": "thread-9", "turn_id": "turn-1"})
    run_codex(env, {"hook_event_name": "PermissionRequest", "cwd": "/tmp/cproj",
                    "session_id": "thread-9", "turn_id": "turn-1", "tool_name": "Bash",
                    "call_id": "call-42", "command": "ls"})
    run_codex(env, {"hook_event_name": "Stop", "cwd": "/tmp/cproj", "session_id": "thread-9",
                    "turn_id": "turn-1", "last_assistant_message": "listo"})
    evs = v2_events(env)
    assert [(e["kind"], e["turnId"], e["requestId"]) for e in evs] == [
        ("prompt_accepted", "turn-1", None), ("permission_requested", "turn-1", "call-42"),
        ("turn_completed", "turn-1", None)]
    assert all(e["conversationId"] == "thread-9" and e["harness"] == "codex"
               and e["sessionKey"] == "csess" and e["paneId"] == "%5" for e in evs)
    assert evs[-1]["excerpt"].startswith("listo")


def test_codex_hook_and_notify_fallback_are_one_turn_event(env):
    codex_env(env)
    run_codex(env, {"hook_event_name": "Stop", "cwd": "/tmp/cproj", "session_id": "thread-9",
                    "turn_id": "turn-2", "last_assistant_message": "fin"})
    # the fallback fires after the dedupe window of the old state file
    state = next(env.state.glob("cproj*.json"))
    data = json.loads(state.read_text()); data["ts"] = 0; state.write_text(json.dumps(data))
    e = dict(env.env, TMUX_PANE="%5", FAKE_TMUX_SESSION="csess")
    subprocess.run(["bash", str(ROOT / "adapters" / "codex-notify.sh"), json.dumps(
        {"type": "agent-turn-complete", "cwd": "/tmp/cproj", "thread-id": "thread-9",
         "turn-id": "turn-2", "last-assistant-message": "fin"})], env=e, timeout=20, capture_output=True)
    evs = v2_events(env)
    assert len(evs) == 1 and evs[0]["turnId"] == "turn-2"
    db = env.home / ".local" / "state" / "comandos" / "app-state.sqlite3"
    conn = sqlite3.connect(str(db))
    assert conn.execute("SELECT COUNT(*) FROM event_receipts").fetchone()[0] == 2
    conn.close()
