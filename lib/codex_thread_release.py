"""Release one daemon-owned conversation before a user-operated local resume.

The existing daemon is reached through Codex's native stdio proxy. Maintenance
archives and immediately restores the same thread tree; no history is deleted,
no conversation is forked, and no daemon is started or stopped by this module.
"""
import fcntl
import json
import os
from pathlib import Path
import re
import selectors
import shlex
import subprocess
import time

UUID = re.compile(r'^[0-9a-f-]{36}$')
SOURCE_KINDS = ['cli', 'vscode', 'exec', 'appServer', 'subAgent', 'subAgentReview',
                'subAgentCompact', 'subAgentThreadSpawn', 'subAgentOther', 'unknown']


def writer_locked(home, sid):
    path = Path(home) / 'thread-writer-locks' / (sid + '.lock')
    try:
        with path.open('rb') as f:
            try:
                fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                return True
            fcntl.flock(f, fcntl.LOCK_UN)
    except FileNotFoundError:
        pass
    return False


class ControlClient:
    def __init__(self, plan):
        sock = Path(plan['home']) / 'app-server-control/app-server-control.sock'
        if not sock.exists():
            raise RuntimeError('La conversación está bloqueada y no hay control del servidor existente')
        self.proc = subprocess.Popen([plan['binary'], 'app-server', 'proxy', '--sock', str(sock)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            env=dict(os.environ, CODEX_HOME=plan['home']), bufsize=0)
        self.buffer = b''
        self.ident = 0
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.proc.stdout, selectors.EVENT_READ)
        try:
            self.call('initialize', {'clientInfo': {'name': 'comandos_full_access', 'version': '1'},
                                     'capabilities': {'experimentalApi': True}})
            self.send({'method': 'initialized'})
        except BaseException:
            self.close()
            raise

    def send(self, message):
        self.proc.stdin.write((json.dumps(message) + '\n').encode())

    def call(self, method, params, timeout=30):
        self.ident += 1
        ident = self.ident
        self.send({'id': ident, 'method': method, 'params': params})
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if b'\n' not in self.buffer:
                if not self.selector.select(max(0, deadline - time.monotonic())):
                    break
                chunk = os.read(self.proc.stdout.fileno(), 65536)
                if not chunk:
                    raise RuntimeError(f'El control de Codex se cerró durante {method}')
                self.buffer += chunk
                if len(self.buffer) > 16_000_000:
                    raise RuntimeError('La respuesta de mantenimiento de Codex excede el límite')
                continue
            line, self.buffer = self.buffer.split(b'\n', 1)
            if not line.strip():
                continue
            message = json.loads(line)
            if 'method' in message:
                if 'id' in message:
                    self.send({'id': message['id'], 'error': {'code': -32601,
                        'message': 'This maintenance client cannot approve or execute requests'}})
                continue
            if message.get('id') != ident:
                continue
            if 'error' in message:
                raise RuntimeError(f"{method}: {message['error'].get('message', 'Codex rechazó la operación')}")
            return message['result']
        raise RuntimeError(f'Codex no respondió a {method} en {timeout} segundos')

    def close(self):
        if self.proc.stdin:
            self.proc.stdin.close()
        try:
            self.proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            # This is only our stdio proxy, not the daemon it connects to.
            self.proc.terminate()
            try:
                self.proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=2)
        self.proc.stdout.close()
        self.selector.close()


def pages(client, method, params):
    cursor = None
    seen = set()
    while True:
        response = client.call(method, {**params, **({'cursor': cursor} if cursor else {})})
        yield from response['data']
        cursor = response.get('nextCursor')
        if not cursor:
            return
        if cursor in seen:
            raise RuntimeError('Codex repitió un cursor de mantenimiento')
        seen.add(cursor)


def restore_tree(client, plan, ids):
    if plan['sid'] not in ids or any(not UUID.fullmatch(ident) for ident in ids):
        raise ValueError('La recuperación guardada contiene IDs inválidos')
    restore_errors = []
    for ident in ids:
        try:
            current = client.call('thread/read', {'threadId': ident, 'includeTurns': False})['thread']
            if current['id'] != ident or not current.get('path'):
                raise RuntimeError('No se identificó el transcript que debe restaurarse')
            current_path = Path(current['path']).resolve()
            if current_path.is_relative_to((Path(plan['home']) / 'archived_sessions').resolve()):
                restored = client.call('thread/unarchive', {'threadId': ident})['thread']
            elif current_path.is_relative_to((Path(plan['home']) / 'sessions').resolve()):
                # A descendant may refuse the archive, or the request may fail
                # before moving anything. Preserve it as-is.
                restored = current
            else:
                raise RuntimeError('El transcript salió de la cuenta que debe restaurarse')
            if restored['id'] != ident or not restored.get('path'):
                raise RuntimeError('La restauración devolvió otra conversación')
            if not Path(restored['path']).resolve().is_relative_to((Path(plan['home']) / 'sessions').resolve()):
                raise RuntimeError('Codex no devolvió el transcript a sus sesiones activas')
            if ident == plan['sid']:
                plan['transcript'] = restored['path']
        except Exception as exc:
            restore_errors.append(f'{ident}: {exc}')
    if restore_errors:
        raise RuntimeError('No se pudo restaurar todo; la recuperación está guardada: ' + '; '.join(restore_errors))


def refresh_recovery(client, plan, checkpoint):
    recovery = plan['releaseRecovery']
    if 'previouslyArchived' not in recovery:
        return
    archived = list(pages(client, 'thread/list', {'ancestorThreadId': plan['sid'],
        'archived': True, 'sourceKinds': SOURCE_KINDS, 'limit': 100}))
    previous = set(recovery['previouslyArchived'])
    children = list(dict.fromkeys([ident for ident in recovery['threadIds'] if ident != plan['sid']] +
        [t['id'] for t in archived if t['id'] not in previous]))
    if plan['sid'] in children or any(not UUID.fullmatch(ident) for ident in children):
        raise ValueError('La recuperación encontró descendientes inválidos')
    recovery['threadIds'] = [*children, plan['sid']]
    recovery['commands'] = [shlex.join(['env', f"CODEX_HOME={plan['home']}", plan['binary'],
        'unarchive', ident]) for ident in recovery['threadIds']]
    if checkpoint:
        checkpoint()


def release_writer(plan, checkpoint=None):
    sid = plan['sid']
    if not UUID.fullmatch(sid):
        raise ValueError('La recuperación requiere el ID exacto de la conversación')
    recovery = plan.get('releaseRecovery', {})
    if not recovery.get('pending') and not writer_locked(plan['home'], sid):
        return plan['transcript']
    client = ControlClient(plan)
    try:
        if recovery.get('pending'):
            refresh_recovery(client, plan, checkpoint)
            restore_tree(client, plan, recovery['threadIds'])
            recovery['pending'] = False
            if checkpoint:
                checkpoint()
            if not writer_locked(plan['home'], sid):
                return plan['transcript']
        if sid not in set(pages(client, 'thread/loaded/list', {'limit': 100})):
            raise RuntimeError('El bloqueo pertenece a otro proceso; no se modificó ninguna conversación')
        thread = client.call('thread/read', {'threadId': sid, 'includeTurns': False})['thread']
        if thread['id'] != sid or not thread.get('path') or Path(thread['path']).resolve() != Path(plan['transcript']).resolve():
            raise RuntimeError('El servidor no posee el transcript exacto de la conversación')
        descendants = list(pages(client, 'thread/list', {'ancestorThreadId': sid,
            'archived': False, 'sourceKinds': SOURCE_KINDS, 'limit': 100}))
        children = list(dict.fromkeys(t['id'] for t in descendants))
        if sid in children or any(not UUID.fullmatch(child) for child in children):
            raise RuntimeError('Codex devolvió una lista de descendientes inválida')
        ids = [*children, sid]
        previous_archived = [t['id'] for t in pages(client, 'thread/list', {'ancestorThreadId': sid,
            'archived': True, 'sourceKinds': SOURCE_KINDS, 'limit': 100})]
        plan['releaseRecovery'] = {'pending': True, 'threadIds': ids, 'previouslyArchived': previous_archived, 'commands': [
            shlex.join(['env', f"CODEX_HOME={plan['home']}", plan['binary'], 'unarchive', ident]) for ident in ids]}
        if checkpoint:
            checkpoint()
        error = None
        try:
            client.call('thread/archive', {'threadId': sid})
        except Exception as exc:
            error = exc
        finally:
            refresh_error = None
            try:
                refresh_recovery(client, plan, checkpoint)
            except Exception as exc:
                refresh_error = exc
            restore_tree(client, plan, plan['releaseRecovery']['threadIds'])
            # A lost reply can leave an archive operation running on the server.
            # Keep the recovery record until a later retry checks its final state.
            plan['releaseRecovery']['pending'] = error is not None or refresh_error is not None
            if checkpoint:
                checkpoint()
            if refresh_error:
                raise refresh_error
        if error:
            raise error
        if writer_locked(plan['home'], sid):
            raise RuntimeError('La conversación todavía tiene un escritor activo; se conserva sin duplicarla')
        return plan['transcript']
    finally:
        client.close()
