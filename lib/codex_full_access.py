"""User-operated batch restart of existing local Codex YOLO panes.

This module never changes the permissions of its caller. Run it from a normal
user terminal with access to that user's tmux server. Inspection is the default.
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import subprocess
import tempfile
import time

from codex_thread_release import release_writer, validate_recovery, writer_locked

YOLO = '--dangerously-bypass-approvals-and-sandbox'
UUID = re.compile(r'^[0-9a-f-]{36}$')
SHELLS = {'zsh', 'bash', 'sh', 'fish', 'dash', 'ksh'}


def launch_command(plan, prompt='continua'):
    from codex_yolo_policy import normalize_args
    args = ['resume', plan['sid'], '-C', plan['cwd'], *plan['flags'], YOLO]
    if prompt:
        args.append(prompt)
    return shlex.join(['env', f"CODEX_HOME={plan['home']}", plan['binary'], *normalize_args(args)])


def full_access(context):
    if context.get('approval_policy') != 'never':
        return False
    if (context.get('sandbox_policy') or {}).get('type') != 'danger-full-access':
        return False
    profile = context.get('permission_profile')
    if profile and profile.get('type') != 'disabled':
        return False
    fs = context.get('file_system_sandbox_policy')
    if fs and fs.get('type') != 'unrestricted':
        return False
    return True


def read_new_context(path, offset):
    last = None
    with Path(path).open('rb') as f:
        f.seek(offset)
        for line in f:
            try:
                row = json.loads(line)
            except (ValueError, UnicodeDecodeError):
                continue
            if isinstance(row, dict) and row.get('type') == 'turn_context' and isinstance(row.get('payload'), dict):
                last = row['payload']
    return last


def run_batch(plans, restart, report=None):
    results = []
    for plan in plans:
        try:
            result = {'pane': plan['pane'], **restart(plan)}
        except Exception as exc:
            result = {'pane': plan['pane'], 'status': 'failed', 'error': str(exc)}
        results.append(result)
        if report:
            report(results)
        print(f"{result['pane']}: {result['status']}" + (f" · {result['error']}" if result.get('error') else ''), flush=True)
    return results


def select_retry(pending, fresh, runtime):
    by_pane = {plan['pane']: plan for plan in fresh}
    selected = []
    for old in pending:
        current = by_pane.get(old['pane'])
        if current:
            keys = ('session', 'sid', 'home', 'panePid', 'paneStart')
            if any(str(current[key]) != str(old[key]) for key in keys):
                raise ValueError(f"{old['pane']}: cambió la sesión pendiente; se conserva sin cerrar")
            if old.get('releaseRecovery', {}).get('pending'):
                current = {**current, 'releaseRecovery': old['releaseRecovery']}
            selected.append(current)
        else:
            # A failed launch may already have returned to its original shell.
            # Only reuse the saved command after proving that shell is unchanged.
            runtime.check_pane(old)
            if runtime.tmux('display-message', '-p', '-t', old['pane'], '#{pane_current_command}') not in SHELLS:
                raise ValueError(f"{old['pane']}: cambió el agente; no se reiniciará otra conversación")
            restored = {**old, 'pid': None, 'start': None}
            restored['command'] = launch_command(restored)
            restored['recoveryCommand'] = launch_command(restored, prompt='')
            selected.append(restored)
    return selected


def write_report(path, payload):
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, prefix='.report-', delete=False) as f:
            temporary = Path(f.name)
            f.write(payload)
            f.flush()
            os.fsync(f.fileno())
        os.replace(temporary, path)
        fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        if temporary:
            temporary.unlink(missing_ok=True)


def codex_binary():
    from cli_catalog import native_binary
    binary = native_binary(shutil.which('codex'))
    if not binary:
        raise RuntimeError('No se encontró el ejecutable original de Codex')
    return binary


def close_session(sid):
    """Unload the exact daemon writer while retaining its resumable history.

    This mode never operates terminals. A durable account-local recovery record
    lets the same command restore a tree after an interrupted archive operation.
    """
    if not re.fullmatch(r'[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}', sid):
        raise ValueError('El cierre requiere el ID exacto de la conversación')
    home = Path(os.environ.get('CODEX_HOME', Path.home() / '.codex')).resolve()
    directory = home / 'comandos-close-recovery'
    report = directory / f'{sid}.json'

    def target():
        if not directory.resolve().is_relative_to(home) or not report.resolve().is_relative_to(directory.resolve()):
            raise ValueError('La recuperación salió de la cuenta exacta')
        previous = json.loads(report.read_text()) if report.exists() else {}
        if not isinstance(previous, dict) or (report.exists() and
                (previous.get('sid') != sid or previous.get('home') != str(home))):
            raise ValueError('La recuperación no corresponde a la conversación y cuenta exactas')
        recovery = previous.get('releaseRecovery', {})
        validate_recovery(recovery, sid)
        roots = [home / 'sessions']
        if recovery.get('pending'):
            roots.append(home / 'archived_sessions')
        paths = [p for root in roots for p in root.glob(f'**/rollout-*{sid}.jsonl')]
        if len(paths) != 1:
            raise ValueError('No hay un transcript único para la conversación exacta')
        path = paths[0].resolve()
        if not any(path.is_relative_to(root) for root in roots):
            raise ValueError('El transcript salió de la cuenta exacta')
        if previous:
            recorded = Path(previous.get('transcript', '')).resolve()
            if recorded.name != path.name or not any(recorded.is_relative_to(root) for root in roots):
                raise ValueError('La recuperación no identifica el transcript exacto de la cuenta')
        with path.open() as stream:
            meta = json.loads(stream.readline())
        if not isinstance(meta, dict) or meta.get('type') != 'session_meta' or not isinstance(meta.get('payload'), dict) or meta['payload'].get('id') != sid:
            raise ValueError('El transcript no corresponde al ID exacto de la conversación')
        return {'sid': sid, 'home': str(home), 'transcript': str(path),
                **({'releaseRecovery': recovery} if recovery else {})}

    plan = target()
    if not plan.get('releaseRecovery', {}).get('pending') and not writer_locked(str(home), sid):
        print('La conversación ya no tiene un escritor activo: ' + sid)
        return 0
    binary = codex_binary()
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    lock_path = directory / f'{sid}.lock'
    if not lock_path.resolve().is_relative_to(directory.resolve()):
        raise ValueError('El bloqueo de recuperación salió de la cuenta exacta')
    with lock_path.open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        # A concurrent close may have updated recovery after our first read.
        plan = {**target(), 'binary': binary}
        def checkpoint():
            write_report(report, json.dumps(plan, ensure_ascii=False, indent=2))
        checkpoint()
        plan['transcript'] = release_writer(plan, checkpoint)
        checkpoint()
    print('Escritor liberado; historial conservado: ' + sid)
    return 0


class LocalRuntime:
    def __init__(self):
        from pane_snapshot import PaneInspector
        self.inspector = PaneInspector()

    def tmux(self, *args):
        result = subprocess.run(['tmux', *args], capture_output=True, text=True, timeout=10)
        if result.returncode:
            raise RuntimeError(result.stderr.strip() or 'tmux rechazó la operación')
        return result.stdout.strip()

    @staticmethod
    def process(pid):
        p = Path('/proc') / str(pid)
        stat = (p / 'stat').read_text().rsplit(')', 1)[1].split()
        args = [os.fsdecode(a) for a in (p / 'cmdline').read_bytes().split(b'\0') if a]
        return {'pid': pid, 'start': stat[19], 'state': stat[0], 'args': args}

    def cli_process(self, pane):
        queue, seen = [int(pane['pid'])], set()
        while queue:
            pid = queue.pop(0)
            if pid in seen:
                continue
            seen.add(pid)
            try:
                process = self.process(pid)
            except OSError:
                continue
            args = process['args']
            if args and Path(args[0]).name == 'codex':
                if any(a in args[1:] for a in ('app-server', 'exec-server')):
                    continue
                return process
            queue.extend(self.inspector.children.get(pid, []))
        return None

    def inventory(self, panes=None):
        separator = '\x1f'
        fmt = separator.join(['#{session_name}', '#{pane_id}', '#{pane_pid}', '#{pane_current_command}', '#{pane_current_path}'])
        text = self.tmux('list-panes', '-a', '-F', fmt)
        try:
            saved = json.loads((Path.home() / '.claude/hooks/app-sessions-v2.json').read_text()).get('sessions', {})
        except (OSError, ValueError):
            saved = {}
        plans, failures = [], []
        for line in text.splitlines():
            session, ident, pid, command, cwd = line.split(separator)
            if panes is not None and ident not in panes:
                continue
            pane = {'id': ident, 'pid': int(pid), 'command': command}
            if command in SHELLS:
                continue
            process = self.cli_process(pane)
            if not process:
                continue
            if not any(a in process['args'] for a in (YOLO, '--yolo')):
                continue
            try:
                info = self.inspector(pane)
                shell = self.process(int(pid))
                if process['pid'] == int(pid) or Path(shell['args'][0]).name not in SHELLS:
                    raise ValueError('El pane no tiene un shell al que regresar; se conserva sin cerrar')
                if not info.get('resume_id'):
                    previous = next((p for w in saved.get(session, {}).get('windows', []) for p in w.get('panes', [])
                                     if p.get('id') == ident and p.get('pid') == int(pid) and str(p.get('start')) == shell['start']), {})
                    info = {**previous, 'flags': self.inspector.flags(process['pid'])}
                sid = info.get('resume_id', '')
                if not UUID.fullmatch(sid):
                    raise ValueError('No se identificó la conversación exacta; no se usará --last')
                env = dict(v.split(b'=', 1) for v in (Path('/proc') / str(process['pid']) / 'environ').read_bytes().split(b'\0') if b'=' in v)
                home = Path(os.fsdecode(env.get(b'CODEX_HOME', os.fsencode(Path.home() / '.codex')))).resolve()
                paths = list(home.glob(f'sessions/*/*/*/rollout-*{sid}.jsonl'))
                recovery = getattr(self, 'pending_recoveries', {}).get(ident, {})
                if not paths and recovery.get('releaseRecovery', {}).get('pending') and recovery.get('sid') == sid and recovery.get('home') == str(home):
                    paths = list((home / 'archived_sessions').glob(f'rollout-*{sid}.jsonl'))
                if len(paths) != 1:
                    raise ValueError('No hay un transcript único para guardar y comprobar la reanudación')
                path = paths[0]
                with path.open() as f:
                    meta = json.loads(f.readline())
                if meta.get('type') != 'session_meta' or meta.get('payload', {}).get('id') != sid:
                    raise ValueError('El transcript no corresponde a la conversación exacta')
                flags = self.inspector.flags(process['pid'])
                context = read_new_context(path, max(0, path.stat().st_size - 2_000_000)) or read_new_context(path, 0) or {}
                if context.get('model') and not any(a.split('=', 1)[0] in ('-m', '--model') for a in flags):
                    flags.extend(['--model', context['model']])
                effort = context.get('effort') or context.get('reasoning_effort')
                if effort and not any('model_reasoning_effort=' in a for a in flags):
                    flags.extend(['-c', f'model_reasoning_effort={json.dumps(effort)}'])
                plan = {'session': session, 'pane': ident, 'panePid': int(pid), 'paneStart': shell['start'],
                        'pid': process['pid'], 'start': process['start'], 'sid': sid, 'home': str(home),
                        'cwd': cwd, 'flags': flags, 'binary': process['args'][0], 'transcript': str(path)}
                plan['command'] = launch_command(plan)
                plan['recoveryCommand'] = launch_command(plan, prompt='')
                plans.append(plan)
            except (OSError, ValueError, KeyError, IndexError) as exc:
                failures.append(f'{session} {ident}: {exc}')
        return plans, failures

    def check_pane(self, plan):
        checks = [
            (self.tmux('display-message', '-p', '-t', plan['pane'], '#{session_name}'), plan['session'], 'El pane cambió de sesión'),
            (self.tmux('display-message', '-p', '-t', plan['pane'], '#{pane_pid}'), str(plan['panePid']), 'Cambió el proceso del pane'),
            (self.process(plan['panePid'])['start'], plan['paneStart'], 'Cambió el shell del pane'),
        ]
        for actual, expected, message in checks:
            if actual != expected:
                raise RuntimeError(message)

    def agent_alive(self, plan):
        try:
            process = self.process(plan['pid'])
            return process['start'] == plan['start'] and process['state'] != 'Z'
        except FileNotFoundError:
            return False

    def wait_stopped(self, plan, seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if not self.agent_alive(plan):
                return True
            time.sleep(.2)
        return not self.agent_alive(plan)

    def agent_keys(self, plan, *keys):
        self.check_pane(plan)
        if not self.agent_alive(plan):
            return False
        if self.tmux('display-message', '-p', '-t', plan['pane'], '#{pane_current_command}') in SHELLS:
            raise RuntimeError('El shell volvió mientras el agente sigue vivo; se conserva sin teclear')
        self.tmux('send-keys', '-t', plan['pane'], *keys)
        return True

    def stop_agent(self, plan):
        # First cancel the turn and exit through the TUI. Rapid interrupts are
        # deliberate: Codex's quit shortcut requires the second Ctrl-C promptly.
        print(f"{plan['pane']}: cerrando Codex…", flush=True)
        for _ in range(3):
            if not self.agent_keys(plan, 'C-c'):
                return
            time.sleep(.3)
        if self.wait_stopped(plan, 3):
            return
        for keys in [('Escape',), ('C-u',), ('-l', '--', '/exit'), ('Enter',)]:
            if not self.agent_keys(plan, *keys):
                return
            time.sleep(.15)
        if self.wait_stopped(plan, 5):
            return
        # Only the validated TUI, never a shared server or a process-name match.
        # pidfd keeps this signal bound to the same process even if a PID is reused.
        self.check_pane(plan)
        if not self.agent_alive(plan):
            return
        try:
            fd = os.pidfd_open(plan['pid'])
        except ProcessLookupError:
            return
        try:
            self.check_pane(plan)
            if not self.agent_alive(plan):
                return
            print(f"{plan['pane']}: Codex no respondió; cerrando su proceso exacto…", flush=True)
            signal.pidfd_send_signal(fd, signal.SIGTERM)
        finally:
            os.close(fd)
        if not self.wait_stopped(plan, 10):
            raise RuntimeError('Codex no salió tras SIGTERM; no se tecleó otro comando')

    def restart(self, plan):
        self.check_pane(plan)
        if plan.get('pid') is not None:
            if not self.agent_alive(plan):
                raise RuntimeError('Cambió el agente')
            self.stop_agent(plan)
        # Do not type into an agent/foreground child that has not exited.
        deadline = time.monotonic() + 5
        while self.tmux('display-message', '-p', '-t', plan['pane'], '#{pane_current_command}') not in SHELLS:
            if time.monotonic() >= deadline:
                raise RuntimeError('El shell todavía no está disponible; usa el comando de recuperación guardado')
            time.sleep(.2)
        self.check_pane(plan)
        print(f"{plan['pane']}: comprobando y liberando el bloqueo de la conversación…", flush=True)
        plan['transcript'] = release_writer(plan, getattr(self, 'checkpoint', None))
        tty = self.tmux('display-message', '-p', '-t', plan['pane'], '#{pane_tty}')
        subprocess.run(['stty', 'sane', '-F', tty], capture_output=True, timeout=3, check=True)
        # The source may append context as it closes. None of that can confirm
        # the new launch; only read records written after the source has exited.
        offset = Path(plan['transcript']).stat().st_size
        print(f"{plan['pane']}: reanudando la misma conversación con continua…", flush=True)
        self.tmux('send-keys', '-t', plan['pane'], 'C-u')
        self.tmux('send-keys', '-t', plan['pane'], '-l', '--', plan['command'])
        self.tmux('send-keys', '-t', plan['pane'], 'Enter')
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            context = read_new_context(plan['transcript'], offset)
            if context:
                if not full_access(context):
                    return {'status': 'restricted', 'error': 'El nuevo turno sigue restringido; no se confirmó acceso total'}
                return {'status': 'confirmed', 'conversationId': plan['sid'], 'prompt': 'continua'}
            time.sleep(.5)
        screen = self.tmux('capture-pane', '-p', '-t', plan['pane'], '-S', '-12')
        return {'status': 'unverified', 'error': 'Se envió el arranque con continua, pero no apareció un nuevo turno verificable',
                'screen': screen}


def main():
    parser = argparse.ArgumentParser(description='Instala la regla YOLO permanente y reanuda los panes Codex YOLO con continua. Por defecto solo inspecciona.')
    parser.add_argument('--apply', action='store_true', help='Instalar la regla permanente y reiniciar las sesiones desde la terminal del usuario')
    parser.add_argument('--install-only', action='store_true', help='Instalar la regla permanente sin reiniciar sesiones')
    parser.add_argument('--close-session', metavar='ID', help='Liberar el escritor de una conversación exacta y conservar su historial, sin operar terminales')
    retries = parser.add_mutually_exclusive_group()
    retries.add_argument('--retry-failed', action='store_true', help='Reintentar solo lo pendiente del último informe')
    retries.add_argument('--retry-report', metavar='RUTA', help='Reintentar solo lo pendiente de un informe concreto')
    args = parser.parse_args()
    if args.close_session is not None and (args.apply or args.install_only or args.retry_failed or args.retry_report is not None):
        parser.error('--close-session no se combina con instalación, reinicio ni reintentos de panes')
    try:
        if args.close_session is not None:
            return close_session(args.close_session)
        from codex_yolo_install import install
        if args.install_only:
            installation = install()
            print('Regla YOLO permanente instalada: ' + installation['wrapper'])
            print('Lanzador original conservado: ' + installation['original'])
            return 0
        directory = Path(os.environ.get('XDG_STATE_HOME', Path.home() / '.local/state')) / 'comandos/codex-full-access'
        previous_path = None
        previous = None
        baseline = []
        pending = None
        if args.retry_report or args.retry_failed:
            if args.retry_report:
                previous_path = Path(args.retry_report).resolve()
            else:
                reports = list(directory.glob('*.json'))
                if not reports:
                    raise RuntimeError('No hay un informe anterior que reintentar')
                previous_path = max(reports, key=lambda p: p.stat().st_mtime_ns)
            previous = json.loads(previous_path.read_text())
            if not isinstance(previous.get('plans'), list) or not isinstance(previous.get('results'), list):
                raise ValueError('El informe anterior no contiene planes y resultados válidos')
            panes = [p['pane'] for p in previous['plans']]
            if len(panes) != len(set(panes)):
                raise ValueError('El informe repite un pane')
            baseline = [r for r in previous['results'] if r['status'] == 'confirmed']
            confirmed_panes = {r['pane'] for r in baseline}
            pending = [p for p in previous['plans'] if p['pane'] not in confirmed_panes]
            print(f'Informe anterior: {previous_path}', flush=True)
            print(f'{len(baseline)} sesiones ya confirmadas; {len(pending)} pendientes.', flush=True)
            if not pending:
                return 0
        runtime = LocalRuntime()
        if pending is not None:
            runtime.pending_recoveries = {p['pane']: p for p in pending}
        plans, failures = runtime.inventory({p['pane'] for p in pending}) if pending is not None else runtime.inventory()
        if pending is not None and not failures:
            plans = select_retry(pending, plans, runtime)
        cohort = [{**p} for p in previous['plans']] if previous is not None else plans
        if previous is not None:
            replacements = {p['pane']: p for p in plans}
            cohort = [replacements.get(p['pane'], p) for p in cohort]
        for plan in plans:
            print(f"{plan['session']} {plan['pane']} · {plan['sid']} · {plan['home']}")
        if failures:
            for error in failures:
                print('SIN CAMBIOS: ' + error)
            print('El lote no empezó: falta identificar todas las sesiones YOLO.')
            return 1
        print(f'{len(plans)} panes Codex YOLO identificados.')
        if not args.apply:
            return 0
        if plans:
            help_text = subprocess.run([plans[0]['binary'], 'resume', '--help'], capture_output=True, text=True, check=True).stdout
            if '--no-daemon' not in help_text:
                raise RuntimeError('Esta versión de Codex no admite --no-daemon; el lote no empezó')
        installation = install()
        print('Regla YOLO permanente instalada: ' + installation['wrapper'], flush=True)
        if not plans:
            return 0
        directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        with (directory / 'batch.lock').open('w') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            report = directory / f'{time.time_ns()}.json'
            completed = []
            def save(results):
                completed[:] = results
                by_pane = {r['pane']: r for r in [*baseline, *results]}
                combined = [by_pane[p['pane']] for p in cohort if p['pane'] in by_pane]
                payload = json.dumps({'plans': cohort, 'results': combined,
                    **({'previousReport': str(previous_path)} if previous_path else {})}, ensure_ascii=False, indent=2)
                write_report(report, payload)
            save([])
            runtime.checkpoint = lambda: save(list(completed))
            print('Plan y recuperación: ' + str(report), flush=True)
            results = run_batch(plans, runtime.restart, save)
            confirmed = len(baseline) + sum(r['status'] == 'confirmed' for r in results)
            print(f'Acceso total confirmado y continua enviado: {confirmed}/{len(cohort)}. Informe: {report}')
            return 0 if confirmed == len(cohort) else 1
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as exc:
        print(f'No se pudo completar el lote: {exc}')
        return 1
