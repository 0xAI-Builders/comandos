"""Keep usage polling bounded without changing historical totals."""
import sqlite3
import threading
import time
from concurrent.futures import ThreadPoolExecutor

from test_usage_dash import load_dash_module


def test_usage_summary_does_not_read_raw_transcript_payloads(tmp_path, monkeypatch):
    dash = load_dash_module()
    usage = dash.cc_usage
    database = tmp_path / 'usage.sqlite'
    usage.record_turn(database, {
        'id': 'turn', 'provider': 'codex', 'agent': 'codex',
        'tmux_session': 'historic', 'tmux_pane': '%2',
        'pane_pwd': '/project', 'git_root': '/project', 'model': 'gpt-test',
        'turn_started_at': 90, 'turn_finished_at': 100,
        'total_tokens': 123, 'cost_usd': 0.5, 'raw': 'unused' * 100000,
    })
    original = usage.connect

    def without_raw(path):
        connection = original(path)
        connection.set_authorizer(lambda action, table, column, *_:
            sqlite3.SQLITE_DENY if action == sqlite3.SQLITE_READ
            and table == 'usage_turns' and column == 'raw' else sqlite3.SQLITE_OK)
        return connection

    monkeypatch.setattr(usage, 'connect', without_raw)
    state = usage.build_usage_state(database, now=110)
    assert state['totals'] == {'total_tokens': 123, 'cost_usd': 0.5}
    assert state['projects'][0]['panes'][0]['model'] == 'gpt-test'
    assert state['windows']['items'][0]['used'] == 123


def test_rows_consume_cursor_without_duplicate_fetchall_buffer():
    usage = load_dash_module().cc_usage

    class Cursor:
        def __iter__(self):
            return iter([{'total_tokens': 123}, {'total_tokens': 456}])

        def fetchall(self):
            raise AssertionError('would allocate a second full result buffer')

    assert usage._rows(Cursor()) == [{'total_tokens': 123}, {'total_tokens': 456}]


def test_concurrent_usage_polls_build_once_and_keep_request_limits(monkeypatch):
    dash = load_dash_module()
    calls = []
    start = threading.Barrier(6)

    def build(*args, **kwargs):
        calls.append(1)
        time.sleep(0.08)
        return {'totals': {'total_tokens': 123}}

    monkeypatch.setattr(dash.cc_usage, 'build_usage_state', build)

    def request(index):
        start.wait(timeout=3)
        return dash.cached_usage_state([], {}, [{'percent': index}])

    with ThreadPoolExecutor(max_workers=6) as pool:
        results = list(pool.map(request, range(6)))
    assert len(calls) == 1
    assert [r['limits'][0]['percent'] for r in results] == list(range(6))
    assert all(r['totals']['total_tokens'] == 123 for r in results)
    dash.cached_usage_state([], {}, [])
    assert len(calls) == 1


def test_failed_usage_build_can_be_retried(monkeypatch):
    import pytest
    dash = load_dash_module()
    calls = []

    def build(*args, **kwargs):
        calls.append(1)
        if len(calls) == 1:
            raise RuntimeError('temporary read failure')
        return {'totals': {'total_tokens': 123}}

    monkeypatch.setattr(dash.cc_usage, 'build_usage_state', build)
    with pytest.raises(RuntimeError, match='temporary read failure'):
        dash.cached_usage_state([], {}, [])
    assert dash.cached_usage_state([], {}, [])['totals']['total_tokens'] == 123


def test_usage_cache_refreshes_after_import_finishes(monkeypatch):
    dash = load_dash_module()
    entered, release = threading.Event(), threading.Event()
    imported = {'tokens': 123}
    builds = []

    def build(*args, **kwargs):
        builds.append(1)
        return {'totals': {'total_tokens': imported['tokens']}}

    def import_codex(*args, **kwargs):
        entered.set()
        assert release.wait(timeout=3)
        imported['tokens'] = 456
        return 1

    monkeypatch.setattr(dash, 'usage_runtime_env', lambda: {})
    monkeypatch.setattr(dash.cc_usage, 'build_usage_state', build)
    monkeypatch.setattr(dash.cc_usage, 'prune_old_turns', lambda *a, **k: 0)
    monkeypatch.setattr(dash.cc_usage, 'record_local_codex_threads', import_codex)
    for name in ('record_local_grok_updates', 'record_local_claude_jsonl', 'record_local_opencode_db'):
        monkeypatch.setattr(dash.cc_usage, name, lambda *a, **k: 0)
    monkeypatch.setattr(dash.grok_state, 'account_homes', lambda: [])
    assert dash.cached_usage_state([], {}, [])['totals']['total_tokens'] == 123
    with ThreadPoolExecutor(max_workers=1) as pool:
        pending = pool.submit(dash.refresh_local_usage, True)
        assert entered.wait(timeout=3)
        try:
            assert dash.cached_usage_state([], {}, [])['totals']['total_tokens'] == 123
        finally:
            release.set()
        pending.result(timeout=3)
    assert dash.cached_usage_state([], {}, [])['totals']['total_tokens'] == 456
    assert len(builds) == 2
