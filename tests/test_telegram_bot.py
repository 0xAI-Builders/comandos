#!/usr/bin/env python3
"""bin/cc-telegram: las teclas y respuestas llegan al PANE correcto.

tmux y la Bot API estan simulados: nunca toca un servidor tmux real ni
Telegram."""
import importlib.machinery
import importlib.util
import json
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


@pytest.fixture
def bot(monkeypatch, tmp_path):
    loader = importlib.machinery.SourceFileLoader(
        "cc_telegram_under_test", str(ROOT / "bin" / "cc-telegram"))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    calls = []
    alive_panes = {"%12": "sess", "%5": "other"}
    sessions = {"sess", "other"}

    def fake_tmux(*args):
        calls.append(list(args))
        if args[0] == "has-session":
            ok = args[2].lstrip("=") in sessions
            return subprocess.CompletedProcess(args, 0 if ok else 1, "", "")
        if args[0] == "display-message":
            target = args[args.index("-t") + 1]
            # tmux 3.2a: rc=0 aunque el pane no exista; stdout vacio
            out = f"{target}\n" if target in alive_panes else "\n"
            return subprocess.CompletedProcess(args, 0, out, "")
        return subprocess.CompletedProcess(args, 0, "", "")

    sent = []
    monkeypatch.setattr(module, "tmux", fake_tmux)
    monkeypatch.setattr(module, "api", lambda token, method, params=None, timeout=60:
                        sent.append((method, params)) or {"ok": True})
    monkeypatch.setattr(module, "TARGETS", str(tmp_path / "tg-targets"))
    (tmp_path / "tg-targets").mkdir()
    module._calls, module._sent, module._tmp = calls, sent, tmp_path
    return module


def cb(bot, data):
    update = {"callback_query": {"id": "q1", "data": data, "from": {"id": 7},
                                 "message": {"chat": {"id": 42}}}}
    bot.handle_update("T", "42", "", "7", update)
    return [c for c in bot._calls if c[0] == "send-keys"]


def test_new_callback_targets_the_exact_pane(bot):
    assert cb(bot, "k|sess|1|12") == [["send-keys", "-t", "%12", "1"]]


def test_legacy_callback_without_pane_still_targets_session(bot):
    assert cb(bot, "k|sess|Enter") == [["send-keys", "-t", "=sess:", "Enter"]]


def test_token_callback_resolves_session_and_pane(bot):
    (bot._tmp / "tg-targets" / "ab12cd34ef.json").write_text(
        json.dumps({"session": "other", "pane": "%5"}))
    assert cb(bot, "t|ab12cd34ef|Escape") == [["send-keys", "-t", "%5", "Escape"]]


@pytest.mark.parametrize("data", ["k|sess|1|99", "k|sess|1|12x", "k|sess|1|%12",
                                  "t|../../etc|1", "t|ffffffffff|1", "k|sess|rm|12"])
def test_dead_or_invalid_targets_never_fall_back_to_active_pane(bot, data):
    assert cb(bot, data) == []
    note = bot._sent[-1][1]["text"]
    assert "enviado" not in note.lower()


def test_reply_to_notification_routes_to_recorded_session_and_pane(bot):
    (bot._tmp / "tg-targets" / "msg-777.json").write_text(
        json.dumps({"session": "sess", "pane": "%12", "project": "proj"}))
    update = {"message": {"message_id": 900, "text": "hazlo", "from": {"id": 7},
                          "chat": {"id": 42},
                          "reply_to_message": {"message_id": 777,
                                               "text": "✅ [proyecto-distinto] Claude termino"}}}
    bot.handle_update("T", "42", "", "7", update)
    keys = [c for c in bot._calls if c[0] == "send-keys"]
    assert keys == [["send-keys", "-t", "%12", "-l", "--", "hazlo"],
                    ["send-keys", "-t", "%12", "Enter"]]


def test_reply_without_recorded_target_keeps_project_fallback(bot):
    update = {"message": {"message_id": 900, "text": "hola", "from": {"id": 7},
                          "chat": {"id": 42},
                          "reply_to_message": {"message_id": 1, "text": "✅ [sess] Claude termino"}}}
    bot.handle_update("T", "42", "", "7", update)
    keys = [c for c in bot._calls if c[0] == "send-keys"]
    assert keys[0] == ["send-keys", "-t", "=sess:", "-l", "--", "hola"]


NOTIFYD_CHECK = r"""
import importlib.machinery, importlib.util, json, subprocess, sys
loader = importlib.machinery.SourceFileLoader("n", sys.argv[1])
spec = importlib.util.spec_from_loader("n", loader)
n = importlib.util.module_from_spec(spec)
try:
    loader.exec_module(n)
except Exception as exc:
    print(json.dumps({"skip": repr(exc)})); sys.exit(0)
posted, tmuxed = [], []
n.dash = lambda path, payload: posted.append([path, payload])
n.send_key("sess", "1", "%12")
def down(path, payload):
    raise OSError("cc-dash caido")
n.dash = down
n.tmux = lambda *a: tmuxed.append(list(a)) or subprocess.CompletedProcess(
    a, 0, "%12\n" if a[0] == "display-message" else "", "")
n.send_key("sess", "Enter", "%12")
n.send_key("sess", "Enter", "bad")
print(json.dumps({"posted": posted, "tmuxed": tmuxed}))
"""


def test_notifyd_send_key_uses_pane():
    # cc-notifyd importa Gtk (gi): corre con el python3 del sistema, que es
    # el que tiene gi; si no esta disponible se omite.
    proc = subprocess.run(["python3", "-c", NOTIFYD_CHECK, str(ROOT / "bin" / "cc-notifyd")],
                          capture_output=True, text=True, timeout=60,
                          env={"PATH": "/usr/bin:/bin", "HOME": "/nonexistent"})
    assert proc.returncode == 0, proc.stderr
    out = json.loads(proc.stdout.strip().splitlines()[-1])
    if "skip" in out:
        pytest.skip(f"gi no disponible: {out['skip']}")
    assert out["posted"] == [["/key", {"session": "sess", "key": "1", "pane": "%12"}]]
    sends = [c for c in out["tmuxed"] if c[0] == "send-keys"]
    assert sends == [["send-keys", "-t", "%12", "Enter"]]


# ---- cc-dash POST /key: pane opcional validado, sin caer al pane activo ----

@pytest.fixture(scope="module")
def dash_mod():
    loader = importlib.machinery.SourceFileLoader(
        "cc_dash_key_under_test", str(ROOT / "bin" / "cc-dash"))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


def post_key(dash, monkeypatch, data, alive=("%12",)):
    import io
    from types import SimpleNamespace
    calls = []

    def fake_tmux(*args, **_kw):
        calls.append(list(args))
        out = ""
        if args[0] == "display-message":
            target = args[args.index("-t") + 1]
            out = f"{target}\n" if target in alive else "\n"
        return subprocess.CompletedProcess(args, 0, out, "")

    monkeypatch.setattr(dash, "tmux", fake_tmux)
    monkeypatch.setattr(dash, "resolve_project_session", lambda sess: None)
    body = json.dumps(data).encode()
    handler = SimpleNamespace(
        path="/key", headers={"Content-Length": str(len(body))}, rfile=io.BytesIO(body),
        _security_gate=lambda: None,
        _json=lambda status, payload, **_kw: (status, payload))
    status, payload = dash.Handler.do_POST(handler)
    return status, [c for c in calls if c[0] == "send-keys"]


def test_dash_key_with_live_pane_targets_it(dash_mod, monkeypatch):
    status, keys = post_key(dash_mod, monkeypatch, {"session": "sess", "key": "1", "pane": "%12"})
    assert status == 200 and keys == [["send-keys", "-t", "%12", "1"]]


def test_dash_key_without_pane_keeps_session_target(dash_mod, monkeypatch):
    status, keys = post_key(dash_mod, monkeypatch, {"session": "sess", "key": "Enter"})
    assert status == 200 and keys == [["send-keys", "-t", "=sess:", "Enter"]]


@pytest.mark.parametrize("pane,code", [("%99", 404), ("bad", 400), ("%1;rm", 400)])
def test_dash_key_with_dead_or_invalid_pane_never_hits_active_pane(dash_mod, monkeypatch, pane, code):
    status, keys = post_key(dash_mod, monkeypatch, {"session": "sess", "key": "1", "pane": pane})
    assert status == code and keys == []
