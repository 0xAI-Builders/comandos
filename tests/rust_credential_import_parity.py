"""Synthetic credential-store parity; run via the readonly Python oracle runner."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

if len(sys.argv) > 1:
    os.environ['TZ'] = sys.argv[1]
    time.tzset()

sys.path.insert(0, '/work/lib')
import extension_auth as oracle

BIN = str(Path(os.environ['CARGO_TARGET_DIR']) / 'debug/examples/credential_import_contract')
ENDPOINT = 'https://example.test/v1/mcp'
CATALOG = {'version': 1, 'servers': {'demo': {'url': ENDPOINT}, 'linear': {'url': 'https://linear.test/mcp'}, 'stdio': {'command': '/bin/true'}}}
TIMESTAMPS = {
    'iso': '2099-01-01T00:00:00.123456Z', 'number': 4070908800,
    'object': {'secs_since_epoch': 4070908800}, 'invalid': 'not-a-date',
    'basic': '20240102T030405.123456+0530',
    'week': '2024-W01-2T03:04:05Z', 'basic-week': '2024W012T030405Z',
    'week-monday': '2024-W01', 'basic-week-monday': '2024W01',
    'date': '2024-01-02', 'basic-date': '20240102',
    'local': '2024-01-02 03:04:05.123456',
    'local-fold': '2024-11-03T01:30:00.123456',
    'local-gap': '2024-03-10T02:30:00.123456',
    'local-half-hour-fold': '2024-04-07T01:45:00.123456',
    'local-half-hour-gap': '2024-10-06T02:15:00.123456',
    'local-skipped-day': '2011-12-30T12:00:00.123456',
    'unicode-separator': '2024-01-02🐍03:04:05Z',
    'hours': '2024-01-02T03.123456Z', 'minutes': '2024-01-02T03:04,123456Z',
    'offset-seconds': '2024-01-02T03:04:05+05:30:12.345678',
    'offset-normalized': '2024-01-02T03:04:05+01:99',
    'offset-subsecond-zero': '2024-01-02T03:04:05+00:00:00.5',
    'nanoseconds': '2024-01-02T03:04:05.123456789Z',
    'far-future': '9999-12-31T23:59:59.123456Z',
    'past': '0001-01-02T03:04:05.123456+02:00',
    'negative-epoch': '1969-12-31T23:59:59.999999Z',
    'invalid-leap-second': '2024-01-01T00:00:60Z',
    'invalid-lowercase-z': '2024-01-01T00:00:00z',
    'invalid-offset': '2024-01-01T00:00:00+24:00',
    'bool': True, 'object-bool': {'secs_since_epoch': True},
    'object-invalid': {'secs_since_epoch': 'not-a-number'},
}

def write(home, name, value):
    path = home / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value))
    return path

def claude(token, expiry, endpoint=ENDPOINT, name='demo'):
    return {'serverName': name, 'serverUrl': endpoint, 'accessToken': token,
            'refreshToken': 'fixture-refresh', 'clientId': 'fixture-client',
            'expiresAt': expiry, 'discoveryState': {'authorizationServerUrl': 'https://issuer.test'}}

def setup(home, case):
    if case in ('claude', 'existing', 'rotation', 'mismatch', 'malformed'):
        write(home, '.claude/.credentials.json', {'claudeAiOauth': {'accessToken': 'model-only-sentinel'},
            'mcpOAuth': {'primary': claude('primary-token', 1_000_000),
                         'alias': claude('alias-token', 900_000, 'https://linear.test/mcp', 'linear-server'),
                         'ignored': claude('foreign-token', 999_000_000, 'https://foreign.test'),
                         'not-an-object': None}})
        write(home, '.claude-accounts/two/.credentials.json', {'mcpOAuth': {'account': claude('account-token', 2_000_000)}})
    if case in ('existing', 'rotation'):
        write(home, '.config/comandos/extensions/credentials.json', {'demo': {'url': ENDPOINT if case == 'existing' else 'https://old.test', 'access_token': 'saved-token', 'opaque': {'keep': True}}, 'unrelated': {'keep': 7}})
    if case == 'mismatch':
        return {'servers': {'demo': {'url': 'https://no-match.test'}}}
    if case == 'malformed':
        (home / '.claude/.credentials.json').write_text('{"mcpOAuth":{},"mcpOAuth":"private-sentinel"}')
        write(home, '.config/comandos/extensions/credentials.json', {'keep': {'value': 9}})
    if case.startswith('grok'):
        timestamp = TIMESTAMPS[case.removeprefix('grok-')]
        write(home, '.grok/mcp_credentials.json', {'demo:' + ENDPOINT: {'token_received_at': timestamp,
            'token_response': {'access_token': 'grok-token', 'refresh_token': 'grok-refresh', 'expires_in': 600},
            'client_id': 'grok-client', 'issuer': 'https://issuer.test'}, 'ignored': None})
    if case == 'remote':
        key = hashlib.md5(ENDPOINT.encode()).hexdigest()
        for package, timestamp, token in [('older', 1000.25, 'older-token'), ('newer', 2000.75, 'newer-token')]:
            path = write(home, f'.mcp-auth/{package}/{key}_tokens.json', {'access_token': token, 'refresh_token': 'remote-refresh', 'expires_in': 300})
            os.utime(path, (timestamp, timestamp))
            write(home, f'.mcp-auth/{package}/{key}_client_info.json', {'client_id': 'remote-client', 'client_secret': 'fixture-secret', 'token_endpoint_auth_method': 'client_secret_post'})
    return CATALOG

def restored(path, original):
    if original is None:
        path.unlink(missing_ok=True)
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(original)

cases = ['empty', 'claude', 'existing', 'rotation', 'mismatch', 'malformed', 'remote'] + ['grok-' + key for key in TIMESTAMPS]
for case in cases:
    with tempfile.TemporaryDirectory(prefix='credential-parity-') as directory:
        home = Path(directory)
        catalog = setup(home, case)
        path = home / '.config/comandos/extensions/credentials.json'
        original = path.read_bytes() if path.exists() else None
        try:
            expected_names = oracle.import_credentials(home, catalog)
            expected = json.loads(path.read_text()) if path.exists() else None
            failed = False
        except Exception:
            expected = path.read_bytes() if path.exists() else None
            failed = True
        restored(path, original)
        process = subprocess.run([BIN, str(home)], input=json.dumps(catalog), text=True, capture_output=True, timeout=5)
        assert (process.returncode != 0) == failed, (case, process.returncode, process.stderr)
        if failed:
            assert (path.read_bytes() if path.exists() else None) == expected, case
            assert 'private-sentinel' not in process.stderr, case
            continue
        assert json.loads(process.stdout) == expected_names, case
        actual = json.loads(path.read_text()) if path.exists() else None
        assert actual == expected, (case, 'credential field mismatch')
        assert 'model-only-sentinel' not in json.dumps(actual), case
        before = path.read_bytes() if path.exists() else None
        again = subprocess.run([BIN, str(home)], input=json.dumps(catalog), text=True, capture_output=True, timeout=5)
        assert again.returncode == 0, case
        assert (path.read_bytes() if path.exists() else None) == before, (case, 'non-idempotent write')
print(f'{len(cases)} synthetic credential-import oracle cases passed, including repeat no-op writes; timezone={os.environ.get("TZ", "system")}')
