#!/usr/bin/env python3
import importlib.machinery
import importlib.util
import sys
import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from types import SimpleNamespace


def load_dash_module():
    bin_dir = str(Path("bin").resolve())
    if bin_dir not in sys.path:
        sys.path.insert(0, bin_dir)
    path = str(Path("bin/cc-dash").resolve())
    loader = importlib.machinery.SourceFileLoader("cc_dash_concurrency_under_test", path)
    spec = importlib.util.spec_from_loader(loader.name, loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


class _CountingLock:
    """Expose when every caller has completed its first cache-lock entry."""

    def __init__(self, expected_entries):
        self._lock = threading.Lock()
        self._expected_entries = expected_entries
        self._entries = 0
        self.all_entered = threading.Event()

    def __enter__(self):
        self._lock.acquire()
        self._entries += 1
        if self._entries == self._expected_entries:
            self.all_entered.set()
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        self._lock.release()


def fresh_dash_with_model_state(tmp_path, monkeypatch, *, discovered=True):
    dash = load_dash_module()
    dash.PANE_MODELS_FILE = str(tmp_path / "pane-models.txt")
    dash._pane_model_lock = threading.Lock()
    dash._pane_model_state = {
        "desired": {},
        "applied": {},
        "file_text": None,
        "generation": 0,
        "worker_running": False,
        "discovered": discovered,
        "retry_after": 0.0,
    }

    real_thread = threading.Thread
    dash._pane_model_test_threads = []

    def recording_thread(*args, **kwargs):
        worker = real_thread(*args, **kwargs)
        dash._pane_model_test_threads.append(worker)
        return worker

    monkeypatch.setattr(dash.threading, "Thread", recording_thread)
    return dash


def install_fake_tmux(dash):
    calls = []

    def fake_tmux(*args, **kwargs):
        calls.append(args)
        return SimpleNamespace(returncode=0, stdout="", stderr="")

    dash.tmux = fake_tmux
    return calls


def wait_for_model_worker(dash):
    """Join every worker created by a public write, including late starts."""
    joined = 0
    while joined < len(dash._pane_model_test_threads):
        worker = dash._pane_model_test_threads[joined]
        worker.join(timeout=2)
        assert not worker.is_alive(), "pane-model worker did not become idle"
        joined += 1


def test_unchanged_pane_models_do_not_write_twice(tmp_path, monkeypatch):
    dash = fresh_dash_with_model_state(tmp_path, monkeypatch)
    calls = install_fake_tmux(dash)
    panes = [{"tmux_pane": "%1", "agent": "codex", "model": "gpt-5.6"}]

    dash.write_pane_models(panes)
    wait_for_model_worker(dash)
    first_mtime = Path(dash.PANE_MODELS_FILE).stat().st_mtime_ns
    dash.write_pane_models(panes)
    wait_for_model_worker(dash)

    assert len(calls) == 1
    assert Path(dash.PANE_MODELS_FILE).stat().st_mtime_ns == first_mtime


def test_reordered_pane_models_do_not_rewrite_compatibility_file(
        tmp_path, monkeypatch):
    dash = fresh_dash_with_model_state(tmp_path, monkeypatch)
    install_fake_tmux(dash)
    first = [
        {"tmux_pane": "%1", "agent": "codex", "model": "gpt-5.6"},
        {"tmux_pane": "%2", "agent": "claude", "model": "sonnet"},
    ]
    dash.write_pane_models(first)
    wait_for_model_worker(dash)
    before = Path(dash.PANE_MODELS_FILE).read_text()
    before_mtime = Path(dash.PANE_MODELS_FILE).stat().st_mtime_ns

    dash.write_pane_models(list(reversed(first)))
    wait_for_model_worker(dash)

    assert Path(dash.PANE_MODELS_FILE).read_text() == before
    assert Path(dash.PANE_MODELS_FILE).stat().st_mtime_ns == before_mtime


def test_changed_and_stale_pane_models_are_reconciled(tmp_path, monkeypatch):
    dash = fresh_dash_with_model_state(tmp_path, monkeypatch)
    calls = install_fake_tmux(dash)
    dash.write_pane_models([
        {"tmux_pane": "%1", "agent": "codex", "model": "gpt-5.5"},
        {"tmux_pane": "%2", "agent": "claude", "model": "sonnet"},
    ])
    wait_for_model_worker(dash)

    dash.write_pane_models([
        {"tmux_pane": "%1", "agent": "codex", "model": "gpt-5.6"},
        {"tmux_pane": "%2", "agent": "", "model": ""},
    ])
    wait_for_model_worker(dash)

    assert any(args[:5] == (
        "set-option", "-p", "-t", "%1", "@ccmodel"
    ) for args in calls)
    assert ("set-option", "-p", "-u", "-t", "%2", "@ccmodel") in calls
    assert Path(dash.PANE_MODELS_FILE).read_text() == "%1 codex · gpt-5.6\n"


def test_explicit_none_pane_model_clears_unknown_restart_state_once(
        tmp_path, monkeypatch):
    dash = fresh_dash_with_model_state(tmp_path, monkeypatch)
    calls = install_fake_tmux(dash)
    cleared = [{"tmux_pane": "%7", "agent": "", "model": ""}]

    dash.write_pane_models(cleared)
    wait_for_model_worker(dash)
    dash.write_pane_models(cleared)
    wait_for_model_worker(dash)

    clear = ("set-option", "-p", "-u", "-t", "%7", "@ccmodel")
    assert calls == [clear]
    assert dash._pane_model_state["applied"] == {"%7": None}


def test_restart_discovers_and_clears_preexisting_physical_pane_model(
        tmp_path, monkeypatch):
    dash = fresh_dash_with_model_state(tmp_path, monkeypatch, discovered=False)
    calls = []

    def fake_tmux(*args, **kwargs):
        calls.append(args)
        if args[:3] == ("list-panes", "-a", "-F"):
            return SimpleNamespace(
                returncode=0,
                stdout="%7\t#[fg=colour43,bold]▸ codex · stale#[default]\n",
                stderr="",
            )
        return SimpleNamespace(returncode=0, stdout="", stderr="")

    dash.tmux = fake_tmux
    dash.write_pane_models([])
    wait_for_model_worker(dash)

    assert calls[0] == (
        "list-panes", "-a", "-F", "#{pane_id}\t#{@ccmodel}"
    )
    assert ("set-option", "-p", "-u", "-t", "%7", "@ccmodel") in calls
    assert dash._pane_model_state["discovered"] is True
    assert dash._pane_model_state["applied"] == {}


def test_failed_unset_for_vanished_pane_is_not_retried_every_poll(
        tmp_path, monkeypatch):
    dash = fresh_dash_with_model_state(tmp_path, monkeypatch)
    dash._pane_model_state["applied"] = {"%9": "old"}
    calls = []

    def fake_tmux(*args, **kwargs):
        calls.append(args)
        if args[:3] == ("set-option", "-p", "-u"):
            return SimpleNamespace(returncode=1, stdout="", stderr="can't find pane")
        if args == ("list-panes", "-a", "-F", "#{pane_id}"):
            return SimpleNamespace(returncode=0, stdout="%1\n", stderr="")
        return SimpleNamespace(returncode=0, stdout="", stderr="")

    dash.tmux = fake_tmux
    dash.write_pane_models([])
    wait_for_model_worker(dash)
    first_calls = list(calls)
    dash.write_pane_models([])
    wait_for_model_worker(dash)

    clear = ("set-option", "-p", "-u", "-t", "%9", "@ccmodel")
    assert first_calls.count(clear) == 1
    assert calls == first_calls
    assert dash._pane_model_state["applied"] == {}


def test_absent_pane_model_is_cleared_only_once(tmp_path, monkeypatch):
    dash = fresh_dash_with_model_state(tmp_path, monkeypatch)
    calls = install_fake_tmux(dash)
    dash.write_pane_models([
        {"tmux_pane": "%1", "agent": "codex", "model": "gpt-5.6"},
        {"tmux_pane": "%2", "agent": "claude", "model": "sonnet"},
    ])
    wait_for_model_worker(dash)

    latest = [{"tmux_pane": "%1", "agent": "codex", "model": "gpt-5.6"}]
    dash.write_pane_models(latest)
    wait_for_model_worker(dash)
    dash.write_pane_models(latest)
    wait_for_model_worker(dash)

    clear = ("set-option", "-p", "-u", "-t", "%2", "@ccmodel")
    assert calls.count(clear) == 1


def test_failed_pane_model_write_waits_for_backoff_before_retrying(
        tmp_path, monkeypatch):
    dash = fresh_dash_with_model_state(tmp_path, monkeypatch)
    now = [100.0]
    monkeypatch.setattr(dash.time, "monotonic", lambda: now[0])
    calls = []

    def flaky_tmux(*args, **kwargs):
        calls.append(args)
        return SimpleNamespace(
            returncode=1 if len(calls) == 1 else 0,
            stdout="",
            stderr="tmux unavailable" if len(calls) == 1 else "",
        )

    dash.tmux = flaky_tmux
    panes = [{"tmux_pane": "%1", "agent": "codex", "model": "gpt-5.6"}]

    dash.write_pane_models(panes)
    wait_for_model_worker(dash)
    assert len(calls) == 1
    assert dash._pane_model_state["applied"] == {}
    assert "gpt-5.6" in dash._pane_model_state["desired"]["%1"]

    dash.write_pane_models(panes)
    wait_for_model_worker(dash)
    assert len(calls) == 1

    now[0] += dash.PANE_MODEL_RETRY_SECONDS
    dash.write_pane_models(panes)
    wait_for_model_worker(dash)
    assert len(calls) == 2
    assert dash._pane_model_state["applied"]["%1"] == (
        dash._pane_model_state["desired"]["%1"]
    )


def test_failed_pane_model_discovery_waits_for_backoff_before_retrying(
        tmp_path, monkeypatch):
    dash = fresh_dash_with_model_state(tmp_path, monkeypatch, discovered=False)
    now = [200.0]
    monkeypatch.setattr(dash.time, "monotonic", lambda: now[0])
    calls = []

    def unavailable_tmux(*args, **kwargs):
        calls.append(args)
        return SimpleNamespace(returncode=1, stdout="", stderr="no server")

    dash.tmux = unavailable_tmux

    dash.write_pane_models([])
    wait_for_model_worker(dash)
    assert len(calls) == 1

    dash.write_pane_models([])
    wait_for_model_worker(dash)
    assert len(calls) == 1

    now[0] += dash.PANE_MODEL_RETRY_SECONDS
    dash.write_pane_models([])
    wait_for_model_worker(dash)
    assert len(calls) == 2


def test_pane_model_worker_start_failure_does_not_wedge_next_call(
        tmp_path, monkeypatch):
    dash = fresh_dash_with_model_state(tmp_path, monkeypatch)
    calls = install_fake_tmux(dash)
    recording_thread = dash.threading.Thread
    thread_attempts = 0

    class FailedStart:
        def start(self):
            raise RuntimeError("thread unavailable")

    def flaky_thread(*args, **kwargs):
        nonlocal thread_attempts
        thread_attempts += 1
        if thread_attempts == 1:
            return FailedStart()
        return recording_thread(*args, **kwargs)

    monkeypatch.setattr(dash.threading, "Thread", flaky_thread)
    panes = [{"tmux_pane": "%1", "agent": "codex", "model": "gpt-5.6"}]

    dash.write_pane_models(panes)
    assert dash._pane_model_state["worker_running"] is False
    assert calls == []

    dash.write_pane_models(panes)
    wait_for_model_worker(dash)
    assert thread_attempts == 2
    assert len(calls) == 1
    assert dash._pane_model_state["applied"] == dash._pane_model_state["desired"]


def test_pane_model_worker_constructor_failure_does_not_wedge_next_call(
        tmp_path, monkeypatch):
    dash = fresh_dash_with_model_state(tmp_path, monkeypatch)
    calls = install_fake_tmux(dash)
    recording_thread = dash.threading.Thread
    thread_attempts = 0

    def flaky_thread(*args, **kwargs):
        nonlocal thread_attempts
        thread_attempts += 1
        if thread_attempts == 1:
            raise RuntimeError("thread unavailable")
        return recording_thread(*args, **kwargs)

    monkeypatch.setattr(dash.threading, "Thread", flaky_thread)
    panes = [{"tmux_pane": "%1", "agent": "codex", "model": "gpt-5.6"}]

    dash.write_pane_models(panes)
    assert dash._pane_model_state["worker_running"] is False
    assert calls == []

    dash.write_pane_models(panes)
    wait_for_model_worker(dash)
    assert thread_attempts == 2
    assert len(calls) == 1
    assert dash._pane_model_state["applied"] == dash._pane_model_state["desired"]


def test_concurrent_pane_model_generations_converge_to_latest_values(
        tmp_path, monkeypatch):
    dash = fresh_dash_with_model_state(tmp_path, monkeypatch)
    old_started = threading.Event()
    release_old = threading.Event()
    calls = []
    physical = {}

    def controlled_tmux(*args, **kwargs):
        value = args[5]
        calls.append(args)
        if "gpt-5.5" in value:
            old_started.set()
            assert release_old.wait(2)
        physical[args[3]] = value
        return SimpleNamespace(returncode=0, stdout="", stderr="")

    dash.tmux = controlled_tmux
    dash.write_pane_models([
        {"tmux_pane": "%1", "agent": "codex", "model": "gpt-5.5"},
    ])
    assert old_started.wait(1)

    dash.write_pane_models([
        {"tmux_pane": "%1", "agent": "codex", "model": "gpt-5.6"},
    ])
    assert len(dash._pane_model_test_threads) == 1
    release_old.set()
    wait_for_model_worker(dash)

    assert len(calls) == 2
    assert "gpt-5.6" in physical["%1"]
    assert dash._pane_model_state["applied"] == dash._pane_model_state["desired"]


def test_usage_route_reconciles_only_live_panes_not_historical_state():
    src = Path("bin/cc-dash").read_text()
    route = src.split('self.path.startswith("/usage/state")', 1)[1]
    route = route.split('self.path.startswith("/ssh")', 1)[0]

    assert "write_pane_models(_pane_models_for_live_state(panes, state))" in route
    assert 'write_pane_models(state.get("panes")' not in route


def test_cached_models_survive_live_cache_hits_but_history_clears(
        tmp_path, monkeypatch):
    dash = load_dash_module()
    dash.USAGE_DB = str(tmp_path / "usage.sqlite")
    dash.LOCAL_USAGE_REFRESH_AT = 123
    dash._usage_state_cache = {"key": None, "state": None}
    builder_calls = []

    def fake_build(_db, panes, settings=None):
        builder_calls.append(list(panes))
        if panes:
            panes[0]["model"] = "gpt-from-local-turn"
            return {"panes": panes}
        return {
            "panes": [{
                "tmux_session": "old",
                "tmux_pane": "%9",
                "pane_pwd": "/old",
                "agent": "codex",
                "model": "stale-history",
            }]
        }

    monkeypatch.setattr(dash.cc_usage, "build_usage_state", fake_build)
    first_live = [{
        "tmux_session": "dev",
        "tmux_pane": "%1",
        "pane_pwd": "/repo",
        "agent": "codex",
        "model": "",
    }]
    first_state = dash.cached_usage_state(first_live, {}, [])
    second_live = [dict(first_live[0], model="")]
    second_state = dash.cached_usage_state(second_live, {}, [])

    assert len(builder_calls) == 1
    assert dash._pane_models_for_live_state(first_live, first_state)[0][
        "model"
    ] == "gpt-from-local-turn"
    assert second_live[0]["model"] == ""
    assert dash._pane_models_for_live_state(second_live, second_state) == [{
        "tmux_session": "dev",
        "tmux_pane": "%1",
        "pane_pwd": "/repo",
        "agent": "codex",
        "model": "gpt-from-local-turn",
    }]

    historical_state = dash.cached_usage_state([], {}, [])
    assert historical_state["panes"][0]["model"] == "stale-history"
    assert dash._pane_models_for_live_state([], historical_state) == []


def test_eight_expired_state_requests_share_one_result():
    dash = load_dash_module()
    dash._state_cache = {"at": 0.0, "items": None, "flight": None}
    dash._state_lock = state_lock = _CountingLock(8)
    entered = threading.Event()
    release = threading.Event()
    calls = 0
    calls_lock = threading.Lock()

    def reader():
        nonlocal calls
        with calls_lock:
            calls += 1
        entered.set()
        assert release.wait(2)
        return [{"session": "alpha"}]

    dash.read_states = reader
    with ThreadPoolExecutor(max_workers=8) as pool:
        futures = [pool.submit(dash.read_states_cached, 0) for _ in range(8)]
        assert entered.wait(1)
        assert state_lock.all_entered.wait(1)
        release.set()
        assert [future.result(timeout=2) for future in futures] == [
            [{"session": "alpha"}]
        ] * 8
    assert calls == 1


def test_state_single_flight_shares_failure_then_retries():
    dash = load_dash_module()
    dash._state_cache = {"at": 0.0, "items": None, "flight": None}
    dash._state_lock = state_lock = _CountingLock(8)
    entered = threading.Event()
    release = threading.Event()
    calls = 0
    calls_lock = threading.Lock()

    def reader():
        nonlocal calls
        with calls_lock:
            calls += 1
            call = calls
        entered.set()
        assert release.wait(2)
        if call == 1:
            raise RuntimeError("tmux unavailable")
        return [{"session": "recovered"}]

    dash.read_states = reader
    with ThreadPoolExecutor(max_workers=8) as pool:
        futures = [pool.submit(dash.read_states_cached, 0) for _ in range(8)]
        assert entered.wait(1)
        assert state_lock.all_entered.wait(1)
        release.set()
        errors = [future.exception(timeout=2) for future in futures]
    assert calls == 1
    assert all(
        type(error) is RuntimeError and str(error) == "tmux unavailable"
        for error in errors
    )
    assert dash.read_states_cached(0) == [{"session": "recovered"}]
    assert calls == 2
