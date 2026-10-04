"""Rust writes Python's full schema; Python reads and verifies the resulting data."""
import itertools
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
from rust_contract_parity import check_isolation


def main():
    check_isolation()
    sys.path.insert(0, '/work/lib')
    import app_state
    import event_intake
    import event_store
    import work_marks
    work_marks.time.time = lambda: 123.456
    event_store.now_ms = lambda: 123456
    event_intake.process_start_time = lambda pid: 42 if pid == '123' else 43
    base = {'hookEvent': 'UserPromptSubmit', 'agent': 'codex', 'session': 's',
            'pane': '%1', 'panePid': '123', 'turnId': 't1', 'conversationId': 'c'}
    payloads = [base, base, {**base, 'turnId': 't2', 'panePid': '999'},
                {**base, 'hookEvent': 'Stop', 'turnId': 't2', 'panePid': '999'},
                {**base, 'hookEvent': 'Notification', 'notificationType': 'idle_prompt'},
                {'kind': 'bad', 'source': 'test'}]
    with tempfile.TemporaryDirectory() as directory:
        paths = [Path(directory) / name for name in ('python.sqlite3', 'rust.sqlite3')]
        for path in paths:
            conn = app_state.connect(path)
            app_state.migrate(conn)
            conn.execute('INSERT INTO workspace_current VALUES(1,1,?,0)', (json.dumps({'bindings': {'logical': {'session': 's', 'paneId': '%1', 'pid': 123, 'startTime': 42}}}),))
            work_marks.set_mark(conn, 'pane', 'logical', {'mark': 'resolved', 'favorite': True}, 0)
            work_marks.set_mark(conn, 'session', 's', 'resolved', 0)
            conn.close()
        expected, requests = [], []
        conn = app_state.connect(paths[0])
        for index, payload in enumerate(payloads):
            # Python requests an event UUID and then a receipt UUID.
            ids = iter([f'event-{index}', f'receipt-{index}'])
            event_store.uuid.uuid4 = lambda: SimpleNamespace(hex=next(ids))
            try:
                result = event_intake.record(conn, payload)
            except ValueError as exc:
                result = {'error': str(exc)}
            expected.append(result)
            requests.append({'payload': payload, 'eventId': f'event-event-{index}',
                             'receiptId': f'receipt-receipt-{index}', 'nowMs': 123456,
                             'processStart': '42' if payload.get('panePid') == '123' else '43'})
        conn.close()
        binary = Path(os.environ['CARGO_TARGET_DIR']) / 'debug/examples/replay'
        result = subprocess.run([str(binary), str(paths[1])], input=''.join(json.dumps(r) + '\n' for r in requests),
                                text=True, capture_output=True, check=True, timeout=20)
        assert [json.loads(line) for line in result.stdout.splitlines()] == expected, result.stdout
        snapshots = []
        for path in paths:
            conn = app_state.connect(path)
            snapshots.append({
                'events': event_store.list_events(conn), 'marks': work_marks.list_marks(conn),
                'receipts': conn.execute('SELECT * FROM event_receipts ORDER BY rowid').fetchall(),
                'applied': conn.execute('SELECT * FROM work_mark_applied ORDER BY event_id, scope, key').fetchall(),
                'schema': conn.execute('SELECT * FROM schema_migrations ORDER BY version').fetchall(),
                'integrity': conn.execute('PRAGMA integrity_check').fetchall(),
                'foreignKeys': conn.execute('PRAGMA foreign_key_check').fetchall(),
            })
            conn.close()
        assert snapshots[0] == snapshots[1], 'Database contents differ after Rust replay'
        assert snapshots[1]['integrity'] == [('ok',)]
        assert snapshots[1]['foreignKeys'] == []
    print('Full Python schema read/write interoperability passed (6 receptions, marks, receipts, integrity)')


if __name__ == '__main__':
    main()
