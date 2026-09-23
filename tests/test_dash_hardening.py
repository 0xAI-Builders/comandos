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


# ---- ~/.ssh/config: editar es UNA reescritura atomica ----
@pytest.fixture
def ssh_home(tmp_path, monkeypatch):
    home = tmp_path / "home"
    (home / ".ssh").mkdir(parents=True)
    monkeypatch.setenv("HOME", str(home))
    config = home / ".ssh" / "config"
    config.write_text("Host prod\n    HostName 203.0.113.10\n    User root\n\n"
                      "Host other\n    HostName 203.0.113.11\n")
    config.chmod(0o600)
    return config


def test_ssh_update_with_invalid_new_entry_keeps_original(dash, ssh_home):
    before = ssh_home.read_text()
    err = dash.ssh_update("prod", {"host": "prod", "hostname": "bad host"})
    assert err
    assert ssh_home.read_text() == before


def test_ssh_update_to_existing_alias_keeps_original(dash, ssh_home):
    before = ssh_home.read_text()
    assert dash.ssh_update("prod", {"host": "other", "hostname": "203.0.113.12"})
    assert ssh_home.read_text() == before


def test_ssh_update_rewrites_entry_in_place_and_keeps_mode(dash, ssh_home):
    assert dash.ssh_update("prod", {"host": "prod2", "hostname": "203.0.113.20",
                                    "port": "2222"}) is None
    hosts = {h["host"]: h for h in dash.parse_ssh_config()}
    assert set(hosts) == {"other", "prod2"}
    assert hosts["prod2"] == {"host": "prod2", "hostname": "203.0.113.20", "port": "2222"}
    assert ssh_home.stat().st_mode & 0o777 == 0o600


def test_ssh_add_and_remove_are_atomic_and_private(dash, ssh_home):
    ssh_home.unlink()
    assert dash.ssh_add({"host": "new", "hostname": "203.0.113.30"}) is None
    assert ssh_home.stat().st_mode & 0o777 == 0o600
    assert dash.ssh_add({"host": "new", "hostname": "203.0.113.31"})
    assert dash.ssh_remove("new") is None
    assert dash.parse_ssh_config() == []
    assert [p.name for p in ssh_home.parent.iterdir() if p.name.endswith(".tmp")] == []


# ---- archivos propios: leer-modificar-escribir bajo candado ----
def test_concurrent_app_tab_writes_do_not_drop_entries(dash, tmp_path, monkeypatch):
    import json
    import threading
    tabs = tmp_path / "app-tabs.json"
    monkeypatch.setattr(dash, "TABS_FILE", str(tabs))
    real_load = dash.load_json_file

    def slow_load(path, default):
        data = real_load(path, default)
        __import__("time").sleep(0.002)
        return data
    monkeypatch.setattr(dash, "load_json_file", slow_load)
    barrier = threading.Barrier(12)

    def add(i):
        barrier.wait()
        dash.write_app_tab(f"s{i}", f"S{i}")
    threads = [threading.Thread(target=add, args=(i,)) for i in range(12)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert json.loads(tabs.read_text()) == {f"s{i}": f"S{i}" for i in range(12)}


def test_write_conf_key_is_atomic_and_preserves_other_keys(dash, tmp_path, monkeypatch):
    conf = tmp_path / "cc-notify.conf"
    conf.write_text("# comentario\nVOLUME=40\nCC_LANG=es\n")
    monkeypatch.setattr(dash, "CONF_PATH", str(conf))
    dash.write_conf_key("VOLUME", "70")
    assert conf.read_text() == "# comentario\nVOLUME=70\nCC_LANG=es\n"
    assert [p.name for p in tmp_path.iterdir() if p.name.endswith(".tmp")] == []


def test_ui_log_rotation_skips_corrupt_lines_instead_of_aborting(dash, tmp_path, monkeypatch):
    import json
    import time as _time
    log = tmp_path / "ui-events.jsonl"
    now = _time.time()
    good = json.dumps({"ts": now, "k": "old"}) + "\n"
    log.write_text(good * 60000 + "{corrupt\n")
    assert log.stat().st_size > 2_000_000
    monkeypatch.setattr(dash, "UI_LOG", str(log))
    dash.ui_log_append([{"ts": now, "k": "click"}])
    lines = log.read_text().splitlines()
    assert len(lines) <= 20000
    assert json.loads(lines[-1])["k"] == "click"


# ---- ruta de peticiones: nada inesperado tumba la conexion sin respuesta ----
@pytest.fixture
def server(dash):
    import http.server
    import threading
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), dash.Handler)
    thread = threading.Thread(target=srv.serve_forever, daemon=True)
    thread.start()
    yield srv
    srv.shutdown()
    srv.server_close()
    thread.join(timeout=2)


def _request(srv, method, path, body=None):
    import http.client
    import json
    client = http.client.HTTPConnection(*srv.server_address, timeout=5)
    try:
        client.request(method, path, body=body,
                       headers={"Content-Type": "application/json"})
        response = client.getresponse()
        raw = response.read()
        return response.status, (json.loads(raw) if raw else None)
    finally:
        client.close()


def test_tmux_timeout_on_request_path_answers_504(dash, server, monkeypatch):
    def hang(*_a, **_k):
        raise dash.subprocess.TimeoutExpired("tmux", 5)
    monkeypatch.setattr(dash, "read_states_cached", hang)
    status, body = _request(server, "GET", "/state")
    assert status == 504 and body["error"]


def test_unexpected_exception_answers_500_and_server_keeps_serving(
        dash, server, monkeypatch, capsys):
    calls = []

    def boom(*_a, **_k):
        calls.append(1)
        if len(calls) == 1:
            raise KeyError("x")
        return {"ok": True}
    monkeypatch.setattr(dash, "read_states_cached", boom)
    assert _request(server, "GET", "/state?token=secreto")[0] == 500
    assert _request(server, "GET", "/state") == (200, {"ok": True})
    assert "secreto" not in capsys.readouterr().err


@pytest.mark.parametrize("body", ["[]", "[1, 2]", '"texto"', "42", "null"])
def test_post_with_non_object_json_is_400(dash, server, body):
    status, payload = _request(server, "POST", "/send", body=body)
    assert status == 400 and payload["error"]


def test_post_exception_answers_500(dash, server, monkeypatch):
    monkeypatch.setattr(dash, "ui_log_append", lambda *_a: {}["nope"])
    status, payload = _request(server, "POST", "/ui-log", body='{"events": []}')
    assert status == 500 and payload["error"]


def test_handler_has_socket_timeout(dash):
    assert dash.Handler.timeout and dash.Handler.timeout <= 60


# ---- llamadas lentas fuera del hilo HTTP ----
def test_opencode_models_serves_stale_cache_and_refreshes_in_background(dash, monkeypatch):
    import threading
    import time as _time
    started, release = threading.Event(), threading.Event()

    def slow_fetch():
        started.set()
        release.wait(5)
        return [{"provider": "nuevo", "models": []}]
    monkeypatch.setattr(dash, "_opencode_models_fetch", slow_fetch)
    monkeypatch.setitem(dash._oc_models_cache, "data", [{"provider": "viejo", "models": []}])
    monkeypatch.setitem(dash._oc_models_cache, "at", 0)
    t0 = _time.monotonic()
    assert dash.opencode_models() == [{"provider": "viejo", "models": []}]
    assert _time.monotonic() - t0 < 1
    assert started.wait(2)
    release.set()
    for _ in range(100):
        if dash._oc_models_cache["data"][0]["provider"] == "nuevo":
            break
        _time.sleep(0.02)
    assert dash.opencode_models() == [{"provider": "nuevo", "models": []}]


def test_opencode_models_cold_cache_waits_only_briefly(dash, monkeypatch):
    import threading
    import time as _time
    release = threading.Event()
    monkeypatch.setattr(dash, "_opencode_models_fetch", lambda: (release.wait(5), [])[1])
    monkeypatch.setitem(dash._oc_models_cache, "data", [])
    monkeypatch.setitem(dash._oc_models_cache, "at", 0)
    t0 = _time.monotonic()
    assert dash.opencode_models(wait=0.2) == []
    assert _time.monotonic() - t0 < 1.5
    release.set()


def test_remote_state_get_uses_cache_instead_of_pinning_threads(dash, monkeypatch):
    import time as _time
    calls = []

    def slow_state():
        calls.append(1)
        _time.sleep(0.3)
        return {"host": "zion", "n": len(calls)}
    monkeypatch.setattr(dash, "remote_state", slow_state)
    monkeypatch.setitem(dash._remote_state_cache, "data", None)
    first = dash.remote_state_cached()
    t0 = _time.monotonic()
    for _ in range(5):
        assert dash.remote_state_cached() == first
    assert _time.monotonic() - t0 < 0.2
    assert len(calls) == 1
