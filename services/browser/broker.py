#!/usr/bin/env python3
"""Bounded remote-only MCP workers over JSONL on an SSH-forwarded loopback port."""
import argparse
import asyncio
from collections import deque
import contextlib
import json
import os
from pathlib import Path
import shutil
import signal
import time
import uuid

MAX_MESSAGE = 8 * 1024 * 1024


def tool_error(message):
    return {'content': [{'type': 'text', 'text': message}], 'isError': True}


def process_identity(pid):
    try:
        raw = Path(f'/proc/{pid}/stat').read_text()
        fields = raw[raw.rfind(')') + 2:].split()
        return int(fields[19]), int(fields[1])
    except (OSError, ValueError, IndexError):
        return None


def owned_processes(root_pid, profile):
    """Capture PID generations; Puppeteer may detach Chrome from Node's group."""
    processes = {}
    selected = {root_pid}
    needle = ('--user-data-dir=' + str(profile)).encode()
    for entry in Path('/proc').iterdir():
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        identity = process_identity(pid)
        if identity:
            processes[pid] = identity
            try:
                if needle in (entry / 'cmdline').read_bytes().split(b'\0'):
                    selected.add(pid)
            except OSError:
                pass
    while True:
        children = {pid for pid, (_, parent) in processes.items() if parent in selected}
        if children <= selected:
            break
        selected |= children
    return {pid: processes[pid][0] for pid in selected if pid in processes}


def signal_owned(identities, sig):
    for pid, generation in identities.items():
        current = process_identity(pid)
        if current and current[0] == generation:
            with contextlib.suppress(ProcessLookupError, PermissionError):
                os.kill(pid, sig)


class Worker:
    def __init__(self, broker, session):
        self.broker, self.session = broker, session
        self.profile = broker.state_dir / ('session-' + session.ident)
        self.process = None
        self.pending = {}
        self.counter = 0
        self.busy = False
        self.last_used = time.monotonic()
        self.closing = asyncio.Lock()
        self.closed = False
        self.tasks = []
        self.errors = deque(maxlen=8)

    async def start(self):
        self.profile.mkdir(mode=0o700, parents=True, exist_ok=True)
        command = self.broker.config['command'] + ['--user-data-dir=' + str(self.profile)]
        env = {**os.environ, **self.broker.config.get('env', {}),
               'CHROME_DEVTOOLS_MCP_NO_UPDATE_CHECKS': '1',
               'CHROME_DEVTOOLS_MCP_NO_USAGE_STATISTICS': '1'}
        self.process = await asyncio.create_subprocess_exec(
            *command, stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE, limit=MAX_MESSAGE + 1,
            env=env, start_new_session=True)
        self.tasks = [asyncio.create_task(self.pump()), asyncio.create_task(self.drain_errors())]
        response = await asyncio.wait_for(self.request('initialize', {
            'protocolVersion': self.broker.catalog.get('protocolVersion', '2025-11-25'),
            'capabilities': {}, 'clientInfo': {'name': 'comandos-browser', 'version': '1'}}), 20)
        if 'error' in response:
            raise RuntimeError('Worker initialization failed')
        await self.send({'jsonrpc': '2.0', 'method': 'notifications/initialized'})

    async def send(self, message):
        self.process.stdin.write((json.dumps(message, separators=(',', ':')) + '\n').encode())
        await self.process.stdin.drain()

    async def request(self, method, params):
        self.counter += 1
        ident = self.counter
        future = asyncio.get_running_loop().create_future()
        self.pending[ident] = future
        try:
            await self.send({'jsonrpc': '2.0', 'id': ident, 'method': method, 'params': params})
            return await future
        finally:
            self.pending.pop(ident, None)

    async def pump(self):
        try:
            while line := await self.process.stdout.readline():
                if len(line) > MAX_MESSAGE:
                    raise ValueError('Worker response too large')
                message = json.loads(line)
                if 'id' in message:
                    future = self.pending.get(message['id'])
                    if future and not future.done():
                        future.set_result(message)
                elif message.get('method', '').startswith('notifications/'):
                    await self.session.send(message)
        except (ValueError, OSError, asyncio.LimitOverrunError):
            pass
        finally:
            for future in list(self.pending.values()):
                if not future.done():
                    future.set_exception(ConnectionError('Remote browser worker disconnected'))

    async def drain_errors(self):
        while True:
            chunk = await self.process.stderr.read(2048)
            if not chunk:
                break
            self.errors.append(chunk.decode(errors='replace'))

    async def close(self):
        async with self.closing:
            if self.closed:
                return
            if self.process:
                owned = owned_processes(self.process.pid, self.profile)
                if self.process.returncode is None:
                    with contextlib.suppress(ProcessLookupError):
                        self.process.terminate()
                    try:
                        await asyncio.wait_for(self.process.wait(), self.broker.config.get('stop_grace', 7))
                    except asyncio.TimeoutError:
                        owned.update(owned_processes(self.process.pid, self.profile))
                        signal_owned(owned, signal.SIGKILL)
                        await self.process.wait()
                # Detached browser children can survive the parent exit.
                owned.update(owned_processes(self.process.pid, self.profile))
                signal_owned(owned, signal.SIGTERM)
                await asyncio.sleep(.05)
                signal_owned(owned, signal.SIGKILL)
                for _ in range(40):
                    remaining = {}
                    for pid, generation in owned.items():
                        identity = process_identity(pid)
                        if identity and identity[0] == generation:
                            try:
                                raw = Path(f'/proc/{pid}/stat').read_text()
                                state = raw[raw.rfind(')') + 2:].split()[0]
                                if state != 'Z':
                                    remaining[pid] = generation
                            except OSError:
                                pass
                    if not remaining:
                        break
                    await asyncio.sleep(.05)
                else:
                    raise RuntimeError('Browser descendants did not exit; retaining worker capacity')
            for task in self.tasks:
                task.cancel()
            await asyncio.gather(*self.tasks, return_exceptions=True)
            shutil.rmtree(self.profile, ignore_errors=True)
            self.closed = True


class Session:
    def __init__(self, writer):
        self.ident = uuid.uuid4().hex
        self.writer = writer
        self.worker = None
        self.initialized = False
        self.expired = False
        self.lock = asyncio.Lock()
        self.tasks = set()
        self.closed = False
        self.client_name = 'unknown'
        self.send_timeout = 10

    async def send(self, message):
        if self.closed:
            return
        try:
            self.writer.write((json.dumps(message, separators=(',', ':')) + '\n').encode())
            await asyncio.wait_for(self.writer.drain(), self.send_timeout)
        except (OSError, ConnectionError, asyncio.TimeoutError):
            self.closed = True
            self.writer.close()


class Broker:
    def __init__(self, config):
        self.config = config
        self.catalog = config['catalog']
        self.tools = {t['name'] for t in self.catalog['tools']}
        self.state_dir = Path(config['state_dir'])
        self.state_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        self.sessions = set()
        self.workers = set()
        self.pool_lock = asyncio.Lock()
        self.capacity = asyncio.Condition(self.pool_lock)
        self.waiters = 0
        self.max_workers = min(2, max(1, config.get('max_workers', 2)))
        self.idle_seconds = config.get('idle_seconds', 300)
        self.tool_timeout = config.get('tool_timeout', 120)
        self.closing = False

    @property
    def worker_count(self):
        return len(self.workers)

    async def release(self, session, expired=False):
        worker = session.worker
        if not worker:
            return
        await worker.close()  # Retain the slot until Node AND Chrome have exited.
        async with self.pool_lock:
            self.workers.discard(worker)
            self.capacity.notify_all()
            if session.worker is worker:
                session.worker = None
                session.expired |= expired

    async def acquire(self, session):
        async with self.pool_lock:
            if session.worker:
                return session.worker
            if self.closing:
                return None
            if len(self.workers) >= self.max_workers:
                queue_timeout = self.config.get('queue_timeout', 0)
                if not queue_timeout or self.waiters >= 16:
                    return None
                self.waiters += 1
                try:
                    await asyncio.wait_for(self.capacity.wait_for(
                        lambda: self.closing or len(self.workers) < self.max_workers), queue_timeout)
                except asyncio.TimeoutError:
                    return None
                finally:
                    self.waiters -= 1
                if self.closing:
                    return None
            worker = Worker(self, session)
            session.worker = worker
            self.workers.add(worker)
        try:
            await worker.start()
            return worker
        except BaseException:
            await self.release(session)
            raise

    async def call(self, session, params):
        if not isinstance(params.get('name'), str) or params['name'] not in self.tools:
            return tool_error('Unknown browser tool')
        if session.expired:
            session.expired = False
            return tool_error('Remote browser session expired or reset; its pages were closed. Start a new page before continuing. No local browser was launched.')
        try:
            worker = await self.acquire(session)
            if worker is None:
                return tool_error('Mac mini browser is busy: two sessions are in use. Retry later, or close an unused browser MCP connection. No local browser was launched.')
            worker.busy = True
            response = await asyncio.wait_for(worker.request('tools/call', params), self.tool_timeout)
            return response.get('result', tool_error('Remote tool failed: ' + str(response.get('error', {}).get('message', 'unknown error'))))
        except asyncio.TimeoutError:
            await self.release(session, expired=True)
            return tool_error('Remote browser tool timed out; its worker and browser were stopped before releasing capacity. Check the result before retrying an action.')
        except (ConnectionError, OSError, RuntimeError):
            await self.release(session, expired=True)
            return tool_error('Remote browser unavailable. Its worker was stopped; no local fallback was started. Reconnect or retry once the Mac service is healthy.')
        finally:
            if session.worker:
                session.worker.busy = False
                session.worker.last_used = time.monotonic()

    async def dispatch(self, session, message):
        ident = message.get('id')
        method = message.get('method')
        if ident is None:
            # Cancellation never frees an executing worker; results drain normally.
            return
        async with session.lock:
            if session.closed:
                return
            result, error = None, None
            if method == 'initialize':
                params = message.get('params') or {}
                session.client_name = str((params.get('clientInfo') or {}).get('name', 'unknown'))[:80]
                session.initialized = True
                result = {'protocolVersion': self.catalog.get('protocolVersion', '2025-11-25'),
                          'capabilities': {'tools': {}},
                          'serverInfo': {'name': 'comandos-browser-macmini', 'version': '1.0.0'},
                          'instructions': 'Browser runs on the Mac mini. Each connection owns isolated pages. Two browser sessions maximum; idle sessions expire after five minutes. Local laptop paths/localhost are not remote paths. Never start a local browser fallback.'}
            elif not session.initialized:
                error = {'code': -32000, 'message': 'Initialize the MCP connection first'}
            elif method == 'tools/list':
                result = {'tools': self.catalog['tools']}
            elif method == 'ping':
                result = {}
            elif method == 'tools/call':
                result = await self.call(session, message.get('params') or {})
            else:
                error = {'code': -32601, 'message': 'Method not found'}
            await session.send({'jsonrpc': '2.0', 'id': ident, **({'error': error} if error else {'result': result})})

    async def handle_client(self, reader, writer):
        if self.closing or len(self.sessions) >= 128:
            writer.close()
            return
        session = Session(writer)
        self.sessions.add(session)
        try:
            while line := await reader.readline():
                if len(line) > MAX_MESSAGE:
                    break
                try:
                    message = json.loads(line)
                except (ValueError, UnicodeError):
                    await session.send({'jsonrpc': '2.0', 'id': None, 'error': {'code': -32700, 'message': 'Invalid JSON'}})
                    continue
                if not isinstance(message, dict) or not isinstance(message.get('method'), str) or not isinstance(message.get('params', {}), dict):
                    await session.send({'jsonrpc': '2.0', 'id': None, 'error': {'code': -32600, 'message': 'Invalid request'}})
                    continue
                if len(session.tasks) >= 4:
                    if 'id' in message:
                        await session.send({'jsonrpc': '2.0', 'id': message['id'], 'error': {'code': -32002, 'message': 'Too many queued requests'}})
                    continue
                task = asyncio.create_task(self.dispatch(session, message))
                session.tasks.add(task)
                task.add_done_callback(session.tasks.discard)
        except (OSError, ValueError, asyncio.LimitOverrunError):
            pass
        finally:
            session.closed = True
            for task in list(session.tasks):
                task.cancel()
            await asyncio.gather(*session.tasks, return_exceptions=True)
            await self.release(session)
            self.sessions.discard(session)
            writer.close()
            with contextlib.suppress(OSError):
                await writer.wait_closed()

    async def reap_idle(self):
        now = time.monotonic()
        for session in list(self.sessions):
            worker = session.worker
            if worker and not worker.busy and not session.lock.locked() and now - worker.last_used > self.idle_seconds:
                async with session.lock:
                    await self.release(session, expired=True)

    def status(self):
        return {'connections': len(self.sessions), 'workers': self.worker_count,
                'maxWorkers': self.max_workers, 'queued': self.waiters,
                'sessions': [{'id': s.ident, 'client': s.client_name, 'workerPid': s.worker.process.pid if s.worker and s.worker.process else None,
                              'busy': bool(s.worker and s.worker.busy)} for s in self.sessions]}

    async def housekeeping(self):
        while not self.closing:
            await self.reap_idle()
            temporary = self.state_dir / 'status.json.tmp'
            temporary.write_text(json.dumps(self.status()))
            temporary.chmod(0o600)
            temporary.replace(self.state_dir / 'status.json')
            await asyncio.sleep(5)

    async def close(self):
        self.closing = True
        for session in list(self.sessions):
            session.closed = True
            session.writer.close()
        await asyncio.gather(*(self.release(s) for s in list(self.sessions)))


async def main(config):
    broker = Broker(config)
    server = await asyncio.start_server(broker.handle_client, '127.0.0.1', config.get('port', 19441), limit=MAX_MESSAGE + 1)
    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGTERM, signal.SIGINT):
        loop.add_signal_handler(sig, stop.set)
    housekeeping = asyncio.create_task(broker.housekeeping())
    print('ComandOS browser broker ready on loopback', flush=True)
    try:
        await stop.wait()
    finally:
        server.close()
        await server.wait_closed()
        housekeeping.cancel()
        await asyncio.gather(housekeeping, return_exceptions=True)
        await broker.close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--config', required=True)
    args = parser.parse_args()
    config = json.loads(Path(args.config).read_text())
    if isinstance(config['catalog'], str):
        config['catalog'] = json.loads(Path(config['catalog']).read_text())
    asyncio.run(main(config))
