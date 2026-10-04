"""Synthetic differential checks. Run only through scripts/rust-sandbox.

Python is the migration oracle, not a dependency of the Rust executable.
No original module is imported until host-isolation assertions pass.
"""
import json
import os
from pathlib import Path
import random
import runpy
import socket
import sqlite3
import subprocess
import sys
from types import SimpleNamespace


def check_isolation():
    assert Path.cwd() == Path('/work'), 'Use the isolated Rust test runner'
    assert not Path('/home/someguy/.claude').exists()
    assert not Path('/home/someguy/.local/state/comandos').exists()
    assert not Path('/run/user/1000').exists()
    assert not list(Path('/tmp').glob('tmux-*'))
    assert len([p for p in Path('/proc').iterdir() if p.name.isdigit()]) < 20
    with socket.socket() as probe:
        probe.settimeout(.1)
        assert probe.connect_ex(('127.0.0.1', 4777)) != 0


def main():
    check_isolation()
    sys.path.insert(0, '/work/lib')
    import turn_state
    import event_store
    import event_intake

    baseline = runpy.run_path('/work/tests/test_turn_state.py')
    original_tests = [v for k, v in baseline.items() if k.startswith('test_')]
    for test in original_tests:
        test()

    event_store.now_ms = lambda: 123456
    event_store.uuid.uuid4 = lambda: SimpleNamespace(hex='synthetic')
    cases = []

    def add(request, result):
        cases.append((request, result))

    def turn(current, event):
        after = turn_state.reduce_turn(current, event)
        add({'op': 'turn', 'current': current, 'event': event},
            {'changed': after is not current, 'state': after})
        return after

    rng = random.Random(20261002)
    kinds = sorted(event_store.KINDS) + ['unrecognized']
    current = None
    all_events = []
    for i in range(6000):
        event = {
            'kind': rng.choice(kinds), 'eventId': f'event-{i}',
            'evidence': rng.choice(['confirmed'] * 6 + ['historical', 'inferred', 'unknown', None]),
            'turnId': rng.choice([None, '', 'a', 'b', 'c', 'local:old']),
            'processKey': rng.choice([None, '', '10-20', '10-21', '30-40']),
            'occurredAtMs': rng.choice([None, True, False, i, i - 3, 0, 1.5, 9007199254740993]),
            'correlation': rng.choice(['source', 'local', None]),
            'requestId': rng.choice(['r1', 'r2', None]),
            'conversationId': rng.choice(['c1', 'c2', None]),
            'paneKey': rng.choice(['p1', 'p2', None]),
            'sessionKey': rng.choice(['s1', 's2', None]),
            'paneId': rng.choice(['%1', '%2', None]),
            'projectKey': 'project', 'harness': 'codex',
        }
        if i % 23 == 0:
            current = None
        if i % 13 == 0:
            event.pop('turnId')
        if i % 17 == 0:
            event.pop('eventId')
        current = turn(current, event)
        all_events.append(event)
    turn({}, None)
    turn(None, {})
    for start in [9007199254740993, 9007199254740994.0, 1.5, -1.5, 2**64 - 1, -2**63]:
        for at in [9007199254740992.0, 9007199254740993, 1, 2, -1, -2, 1e40, -1e40]:
            turn({'state': 'working', 'turnId': 'local:1', 'startedAtMs': start},
                 {'kind': 'turn_completed', 'evidence': 'confirmed', 'occurredAtMs': at})
    for end in range(0, len(all_events), 100):
        events = all_events[end:end + 100]
        add({'op': 'fold', 'events': events}, turn_state.turns_from_events(events))

    base = {'kind': 'turn_completed', 'source': 'hook:test'}
    variants = [base]
    fields = ['kind', 'source', 'sourceEventId', 'eventId', 'harness', 'projectKey',
              'sessionKey', 'paneKey', 'paneId', 'processKey', 'conversationId',
              'turnId', 'requestId', 'evidence', 'correlation', 'occurredAtMs',
              'receivedAtMs', 'title', 'excerpt']
    values = [None, '', False, True, 0, -1, 1, 1.9, 9007199254740993, 'normal',
              'confirmed', 'source', 'unknown', 'a\nb', '\x7f', '😺' * 200,
              '😺' * 201, 'ñ' * 501, [], {}]
    variants += [{**base, field: value} for field in fields for value in values]
    variants += [{**base, 'kind': kind} for kind in sorted(event_store.KINDS)]
    variants += [None, False, [], 'invalid']
    for event in variants:
        try:
            expected = event_store.normalize(event)
        except (ValueError, TypeError) as exc:
            # Unhashable JSON types are outside Python's declared schema; its
            # public validation error remains an error at the Rust boundary.
            if isinstance(exc, TypeError):
                continue
            expected = {'error': str(exc)}
        add({'op': 'event', 'event': event, 'nowMs': 123456,
             'newEventId': 'event-synthetic'}, expected)
    for correlation in [None, 'source', 'local', 'unknown']:
        for evidence in [None, 'confirmed', 'historical', 'inferred']:
            for source_id in [None, '', 'provider-event']:
                for turn_id, request_id in [(None, None), ('t', None), (None, 'r'), ('t', 'r')]:
                    event = {**base, 'correlation': correlation, 'evidence': evidence,
                             'sourceEventId': source_id, 'turnId': turn_id,
                             'requestId': request_id, 'harness': 'codex', 'conversationId': 'c'}
                    add({'op': 'dedupe', 'event': event}, event_store.dedupe_key(event))
    for event in all_events[:100]:
        add({'op': 'destination', 'event': event}, event_store.destination(event))

    for hook in list(event_intake.HOOK_KINDS) + ['unknown']:
        for notification in [None, '', 'idle_prompt', 'auth_success', 'permission_prompt', 'other']:
            for start in [None, '42']:
                payload = {'hookEvent': hook, 'notificationType': notification, 'agent': 'claude',
                           'session': 's1', 'pane': '%123', 'panePid': '123', 'occurredAtMs': 500}
                event_intake.process_start_time = lambda _pid, start=start: start
                add({'op': 'hook', 'payload': payload, 'processStart': start}, event_intake.normalize_hook(payload))
    payload = {'hookEvent': 'PermissionRequest', 'agent': 'codex', 'session': 's1', 'pane': '%1'}
    for field in ['agent', 'session', 'pane', 'panePid', 'turnId', 'promptId', 'requestId',
                  'conversationId', 'sourceEventId', 'project', 'occurredAtMs', 'title', 'excerpt']:
        for value in values + ['%1', '123', 'a' * 81, 'á', '%1234567890', '0000000012']:
            sample = {**payload, field: value}
            event_intake.process_start_time = lambda _pid: '42'
            add({'op': 'hook', 'payload': sample, 'processStart': '42'}, event_intake.normalize_hook(sample))

    conn = sqlite3.connect(':memory:')
    conn.execute('CREATE TABLE workspace_current (id INTEGER, document TEXT)')
    conn.execute('INSERT INTO workspace_current VALUES (1, ?) ', ('{}',))
    binding = {'session': 's1', 'paneId': '%1', 'pid': 123, 'startTime': 42}
    for pid in [None, '123', '123-42', '123-43', '456-42', '-42', '']:
        for count in [0, 1, 2]:
            for field in ['pid', 'startTime', 'session', 'paneId']:
                for value in [None, '', 123, '123', 42, '42', 's1', '%1']:
                    doc = {'bindings': {f'p{n}': {**binding, field: value} for n in range(count)}}
                    event = {'sessionKey': 's1', 'paneId': '%1', 'processKey': pid}
                    conn.execute('UPDATE workspace_current SET document = ?', (json.dumps(doc),))
                    add({'op': 'resolve', 'document': doc, 'event': event}, event_intake.resolve_pane_key(conn, event))
    conn.close()

    binary = Path(os.environ['CARGO_TARGET_DIR']) / 'debug/comandos-contract'
    result = subprocess.run([str(binary)], input=''.join(json.dumps(q, ensure_ascii=False) + '\n' for q, _ in cases),
                            capture_output=True, text=True, timeout=60, check=True)
    actual = [json.loads(line) for line in result.stdout.splitlines()]
    assert len(actual) == len(cases), (len(actual), len(cases), result.stderr)
    for i, ((request, expected), got) in enumerate(zip(cases, actual)):
        assert got == expected, json.dumps({'case': i, 'request': request, 'expected': expected, 'got': got}, ensure_ascii=False)
    print(f'{len(original_tests)} original lifecycle tests; {len(cases)} differential cases passed')


if __name__ == '__main__':
    main()
