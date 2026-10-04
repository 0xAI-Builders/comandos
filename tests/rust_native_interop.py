"""Compare native schema, hook command and legacy import with the Python product."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
from rust_contract_parity import check_isolation


def main():
    check_isolation()
    sys.path.insert(0, '/work/lib')
    import app_state
    import event_intake
    import event_store
    binary = Path(os.environ['CARGO_TARGET_DIR']) / 'debug/comandos-events'

    def run(path, *args, payload=None):
        return subprocess.run([str(binary), '--state', str(path), *args],
                              input=json.dumps(payload) if payload is not None else '',
                              text=True, capture_output=True, timeout=20)

    def schema(conn):
        return conn.execute("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type,name").fetchall()

    with tempfile.TemporaryDirectory() as directory:
        folder = Path(directory)
        py_path, rs_path = folder / 'python.sqlite3', folder / 'rust.sqlite3'
        py = app_state.connect(py_path)
        app_state.migrate(py)
        result = run(rs_path, 'migrate')
        assert result.returncode == 0, result.stderr
        rs = app_state.connect(rs_path)
        assert schema(py) == schema(rs), 'SQLite schemas differ'
        assert py.execute('SELECT version,name FROM schema_migrations ORDER BY version').fetchall() == rs.execute('SELECT version,name FROM schema_migrations ORDER BY version').fetchall()
        for version, name, sql in app_state.MIGRATIONS:
            assert Path(f'/work/crates/comandos-store/migrations/{version:03}-{name}.sql').read_text().rstrip() == sql.rstrip()

        # Native process discovery must bind the child command's supplied parent
        # pid to this live synthetic workspace without discovering any host PID.
        start = event_intake.process_start_time(os.getpid())
        doc = json.dumps({'bindings': {'test-pane': {'session': 's', 'paneId': '%1', 'pid': os.getpid(), 'startTime': start}}})
        for conn in [py, rs]:
            conn.execute('INSERT INTO workspace_current VALUES (1,1,?,0)', (doc,))
        payload = {'hookEvent': 'PermissionRequest', 'agent': 'codex', 'session': 's', 'pane': '%1',
                   'panePid': str(os.getpid()), 'turnId': 't', 'requestId': 'r', 'conversationId': 'c', 'occurredAtMs': 50500}
        expected = event_intake.record(py, payload)
        result = run(rs_path, 'record', '--claim-sound', 'test-device', payload=payload)
        assert result.returncode == 0 and result.stdout == 'play\n', (result.returncode, result.stdout, result.stderr)
        actual = event_store.list_events(rs)[0]
        expected.pop('duplicate')
        for field in ['eventId', 'receivedAtMs']:
            expected.pop(field); actual.pop(field)
        assert actual == expected, (actual, expected)
        assert run(rs_path, 'record', '--claim-sound', 'test-device', payload=payload).stdout == ''

        # Whitespace and Unicode line boundaries affect the historical digest.
        lines = [json.dumps({'status': 'done', 'ts': 49, 'project': 'p', 'detail': '🦀'}, ensure_ascii=False),
                 'not json', json.dumps({'status': 'waiting', 'ts': 49.5}),
                 json.dumps({'status': 'done', 'ts': 50}), json.dumps({'status': 'done', 'ts': True})]
        legacy = folder / 'timeline.jsonl'
        legacy.write_bytes(('\r\n'.join(lines + [lines[0]]) + '\u2028' + lines[0]).encode())
        count = event_intake.import_legacy(py, legacy)
        result = run(rs_path, 'import-legacy', str(legacy))
        assert result.returncode == 0 and json.loads(result.stdout)['count'] == count, result.stderr
        def historical(conn):
            return [{k: v for k, v in e.items() if k not in {'eventId', 'receivedAtMs'}}
                    for e in event_store.list_events(conn) if e['evidence'] == 'historical']
        assert historical(py) == historical(rs), 'Legacy history/digests differ'
        assert json.loads(run(rs_path, 'import-legacy', str(legacy)).stdout)['count'] == 0
        for conn in [py, rs]:
            assert conn.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
            assert conn.execute('PRAGMA foreign_key_check').fetchall() == []
            conn.close()

        # Upgrade an older Python-created file and read the native backup.
        old_path = folder / 'old.sqlite3'
        old = app_state.connect(old_path)
        old.executescript(app_state.MIGRATIONS[0][2])
        old.execute("INSERT INTO schema_migrations VALUES (1,'workspace',123)")
        old.execute("INSERT INTO workspace_meta VALUES ('keep','value')")
        old.close()
        result = run(old_path, 'migrate')
        assert result.returncode == 0, result.stderr
        backup = json.loads(result.stdout)['backup']
        with sqlite3.connect(backup) as conn:
            assert app_state.schema_version(conn) == 1
            assert conn.execute("SELECT value FROM workspace_meta WHERE key='keep'").fetchone()[0] == 'value'
        assert os.stat(backup).st_mode & 0o777 == 0o600
    print('Native schema, hooks, exact process identity, sound claims, legacy digests and backup interoperability passed')


if __name__ == '__main__':
    main()
