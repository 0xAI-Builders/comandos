"""Record deterministic storage contracts from the unchanged Python product."""
import itertools
import json
from pathlib import Path
import sqlite3
import sys
from types import SimpleNamespace
from rust_contract_parity import check_isolation


def main():
    check_isolation()
    sys.path.insert(0, '/work/lib')
    import app_state
    import event_store
    import event_intake
    import work_marks
    counter = itertools.count(1)
    event_store.uuid.uuid4 = lambda: SimpleNamespace(hex=f'{next(counter):032x}')
    event_store.now_ms = lambda: 123456
    conn = sqlite3.connect(':memory:', isolation_level=None)
    schema = app_state.MIGRATIONS[1][2]
    assert schema.rstrip() == Path('/work/crates/comandos-store/tests/fixtures/events.sql').read_text().rstrip()
    conn.executescript(schema)
    for index, name in [(0, 'workspace'), (2, 'marks')]:
        sql = app_state.MIGRATIONS[index][2]
        assert sql.rstrip() == Path(f'/work/crates/comandos-store/tests/fixtures/{name}.sql').read_text().rstrip()
        conn.executescript(sql)
    conn.execute('INSERT INTO workspace_current VALUES (1,1,?,0)', (json.dumps({'bindings': {'logical': {'session': 's', 'paneId': '%1', 'pid': 123, 'startTime': 42}}}),))
    event_intake.process_start_time = lambda pid: 42 if pid == '123' else 99
    work_marks.time.time = lambda: 123.456
    conn.execute('PRAGMA foreign_keys=ON')
    cases = []

    def record(op, **args):
        if op == 'append':
            expected = event_store.append_event(conn, args['event'])
            args['newEventId'] = expected['eventId']
        elif op == 'claim':
            expected = event_store.claim_delivery(conn, args['eventId'], args['channel'], args['deviceId'])
        elif op == 'get':
            expected = event_store.get_event(conn, args['eventId'])
        elif op == 'list':
            expected = event_store.list_events(conn, args['after'], args['limit'])
        elif op == 'set':
            try:
                expected = work_marks.set_mark(conn, args['scope'], args['key'], args['value'], args['revision'])
            except work_marks.Conflict as exc:
                expected = {'error': str(exc), 'current': exc.current}
            except ValueError as exc:
                expected = {'error': str(exc)}
        elif op == 'apply':
            expected = work_marks.apply_turn_event(conn, args['event'])
        elif op == 'record':
            expected = event_intake.record(conn, args['payload'])
            args['newEventId'] = expected['eventId'] if expected else 'unused'
        else:
            conn.execute(op.upper())
            expected = None
        receipts = [list(r) for r in conn.execute('SELECT event_id, source, received_at_ms, duplicate FROM event_receipts ORDER BY rowid')]
        cases.append({'op': op, **args, 'expected': expected,
                      'inTransaction': conn.in_transaction,
                      'sequence': event_store.latest_sequence(conn), 'receipts': receipts,
                      'marks': work_marks.list_marks(conn),
                      'applied': [list(row) for row in conn.execute('SELECT * FROM work_mark_applied ORDER BY event_id, scope, key')]})
        return expected

    base = {'source': 'hook:codex', 'harness': 'codex', 'kind': 'turn_completed', 'turnId': 't1',
            'conversationId': 'c', 'correlation': 'source', 'evidence': 'confirmed',
            'title': 'Terminó 🦀', 'paneKey': 'p1', 'occurredAtMs': 1000}
    first = record('append', event=base)
    record('append', event={**base, 'source': 'notify:codex', 'title': 'duplicate title'})
    record('append', event={**base, 'kind': 'prompt_accepted'})
    for i in range(3):
        record('append', event={**base, 'correlation': 'local', 'eventId': f'ambiguous-{i}', 'paneKey': None, 'sessionKey': 's'})
    for i in range(2):
        record('append', event={**base, 'correlation': 'unknown', 'sourceEventId': 'source-1', 'paneKey': None, 'sessionKey': None})
    record('claim', eventId=first['eventId'], channel='sound', deviceId='desktop')
    record('claim', eventId=first['eventId'], channel='sound', deviceId='desktop')
    record('claim', eventId=first['eventId'], channel='sound', deviceId='phone')
    record('begin')
    transient = record('append', event={**base, 'turnId': 'transient'})
    record('claim', eventId=transient['eventId'], channel='push', deviceId='phone')
    record('rollback')
    record('get', eventId=transient['eventId'])
    record('begin')
    record('append', event={**base, 'turnId': 'committed'})
    record('commit')
    record('get', eventId=first['eventId'])
    record('get', eventId='missing')
    for after, limit in [(0, 100), (1, 2), (-1, 0), (999, 100), (0, 10000)]:
        record('list', after=after, limit=limit)
    for scope in ['pane', 'session']:
        record('set', scope=scope, key='logical' if scope == 'pane' else 's', value={'mark': 'resolved', 'favorite': True}, revision=0)
    prompt = {'hookEvent': 'UserPromptSubmit', 'agent': 'codex', 'session': 's',
              'pane': '%1', 'panePid': '123', 'turnId': 'accepted', 'conversationId': 'conversation'}
    record('record', payload=prompt)
    record('set', scope='pane', key='logical', value='resolved', revision=2)
    record('record', payload=prompt)  # duplicate does not reopen it again
    record('set', scope='pane', key='logical', value='frozen', revision=3)
    record('record', payload={**prompt, 'turnId': 'next'})
    record('set', scope='pane', key='logical', value='resolved', revision=4)
    record('record', payload={**prompt, 'turnId': 'foreign', 'panePid': '999'})
    record('record', payload={**prompt, 'hookEvent': 'Notification', 'notificationType': 'idle_prompt'})
    record('set', scope='pane', key='logical', value='awaiting_reply', revision=0)  # conflict
    for value in [{}, {'favorite': 1}, {'mark': 'wrong'}, {'unknown': True}, [], None]:
        record('set', scope='pane', key='logical', value=value, revision=5)
    for revision in [True, -1, 1.0, '5', None]:
        record('set', scope='pane', key='logical', value='resolved', revision=revision)
    record('begin')
    record('set', scope='pane', key='logical', value='awaiting_reply', revision=5)
    record('rollback')
    for evidence in ['historical', 'inferred', 'confirmed']:
        record('apply', event={'eventId': 'apply-' + evidence, 'kind': 'prompt_accepted', 'evidence': evidence, 'paneKey': 'logical'})
    conn.close()
    fixture = Path('/work/crates/comandos-store/tests/fixtures/storage-cases.json')
    # One operation per line keeps generated expectations small and diffable.
    content = '[\n' + ',\n'.join(json.dumps(case, ensure_ascii=False, separators=(',', ':')) for case in cases) + '\n]\n'
    if sys.argv[1:] == ['--write']:
        fixture.write_text(content)
    else:
        assert fixture.read_text() == content, 'Python storage contract changed; review before regenerating'
    print(f'{len(cases)} storage oracle steps verified')


if __name__ == '__main__':
    main()
