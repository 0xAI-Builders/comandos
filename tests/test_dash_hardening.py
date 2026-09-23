#!/usr/bin/env python3
"""Robustez de cc-dash: validadores, escrituras atomicas y ruta de peticiones.
Todo corre contra tmp dirs y monkeypatch; nunca contra ~/.claude ni tmux real."""
import importlib.machinery
import importlib.util
import sys
from pathlib import Path

import pytest


@pytest.fixture(scope="module")
def dash():
    bin_dir = str(Path("bin").resolve())
    if bin_dir not in sys.path:
        sys.path.insert(0, bin_dir)
    loader = importlib.machinery.SourceFileLoader(
        "cc_dash_hardening_under_test", str(Path("bin/cc-dash").resolve()))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


@pytest.mark.parametrize("name,value", [
    ("SESSION_RE", "demo"),
    ("PANE_RE", "%12"),
    ("SNIPPET_ID_RE", "0123456789abcdef"),
    ("SSH_HOST_RE", "prod"),
    ("SSH_HOSTNAME_RE", "203.0.113.10"),
    ("SSH_USER_RE", "root"),
    ("SSH_PATH_RE", "~/.ssh/id_ed25519"),
])
def test_validators_reject_trailing_newline_even_with_match(dash, name, value):
    pattern = getattr(dash, name)
    assert pattern.match(value)
    # `$` acepta un "\n" final: con .match() eso colaba "demo\n" como sesion.
    assert not pattern.match(value + "\n")
    assert not pattern.fullmatch(value + "\n")


# ---- settings.json de Claude Code: nunca pisarlo si no parsea ----
SETTINGS = {"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "x"}]}]},
            "permissions": {"allow": ["Bash(ls)"]}, "model": "claude-x"}


def test_proxy_env_write_leaves_unparsable_settings_untouched(dash, tmp_path, monkeypatch):
    monkeypatch.setattr(dash, "PROXY_ENV_SINCE", str(tmp_path / "since.json"))
    path = tmp_path / "settings.json"
    path.write_text('{"hooks": {"Stop": [')  # Claude Code a mitad de escribir
    dash._proxy_env_write(str(path), True, 18765)
    assert path.read_text() == '{"hooks": {"Stop": ['


def test_proxy_env_write_keeps_hooks_and_permissions(dash, tmp_path, monkeypatch):
    import json
    monkeypatch.setattr(dash, "PROXY_ENV_SINCE", str(tmp_path / "since.json"))
    path = tmp_path / "settings.json"
    path.write_text(json.dumps(SETTINGS))
    path.chmod(0o640)
    dash._proxy_env_write(str(path), True, 18765)
    st = json.loads(path.read_text())
    assert st["hooks"] == SETTINGS["hooks"] and st["permissions"] == SETTINGS["permissions"]
    assert st["env"] == {"ANTHROPIC_BASE_URL": "http://127.0.0.1:18765"}
    assert path.stat().st_mode & 0o777 == 0o640
    assert sorted(p.name for p in tmp_path.iterdir() if p.name.endswith(".tmp")) == []


def test_proxy_toggle_and_motor_refuse_unparsable_main_settings(dash, tmp_path, monkeypatch):
    home = tmp_path / "home"
    (home / ".claude").mkdir(parents=True)
    monkeypatch.setenv("HOME", str(home))
    monkeypatch.setattr(dash, "PROXY_ENV_SINCE", str(tmp_path / "since.json"))
    monkeypatch.setattr(dash, "load_proxy_cfg", lambda: {"port": 18765, "codex": {"model": "gpt-x"}})
    monkeypatch.setattr(dash, "claude_accounts", lambda: [])
    monkeypatch.setattr(dash.subprocess, "run", lambda *a, **k: None)
    settings = home / ".claude" / "settings.json"
    settings.write_text("{not json")
    with pytest.raises(RuntimeError):
        dash.proxy_set_enabled(True)
    with pytest.raises(RuntimeError):
        dash.motor_set_global("codex", "gpt-x")
    assert settings.read_text() == "{not json"
    # Ausente si arranca de {}.
    settings.unlink()
    dash.proxy_set_enabled(True)
    import json
    assert json.loads(settings.read_text()) == {"env": {"ANTHROPIC_BASE_URL": "http://127.0.0.1:18765"}}


def test_update_json_object_serializes_concurrent_read_modify_write(dash, tmp_path):
    import json
    import threading
    import time as _time
    path = str(tmp_path / "settings.json")
    barrier = threading.Barrier(16)

    def bump(i):
        barrier.wait()

        def mutate(st):
            seen = dict(st)
            _time.sleep(0.002)  # ensancha la ventana de carrera
            st.clear()
            st.update(seen)
            st[f"k{i}"] = i
        dash.update_json_object(path, mutate)
    threads = [threading.Thread(target=bump, args=(i,)) for i in range(16)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert json.load(open(path)) == {f"k{i}": i for i in range(16)}
