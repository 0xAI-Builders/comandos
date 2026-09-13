"""Durable per-pane configuration operations. No terminal IO belongs in this module."""
import hashlib
from contextlib import contextmanager
import json
import os
from pathlib import Path
import sqlite3
import time


class OperationConflict(RuntimeError):
    pass


class OperationStore:
    def __init__(self, path):
        self.path = str(path)
        Path(path).parent.mkdir(parents=True, exist_ok=True)
        with self.connect() as db:
            db.executescript('''
                CREATE TABLE IF NOT EXISTS session_operations (
                    id TEXT PRIMARY KEY, pane_key TEXT NOT NULL, fingerprint TEXT NOT NULL,
                    request TEXT NOT NULL, state TEXT NOT NULL, owner INTEGER NOT NULL,
                    snapshot TEXT, result TEXT, updated REAL NOT NULL);
                CREATE UNIQUE INDEX IF NOT EXISTS session_operations_active_pane
                    ON session_operations(pane_key) WHERE state NOT IN ('confirmed','failed','rolled_back');
                CREATE INDEX IF NOT EXISTS session_operations_target_updated
                    ON session_operations(json_extract(request,'$.session'), json_extract(request,'$.pane'), updated DESC);
                CREATE INDEX IF NOT EXISTS session_operations_pane_updated
                    ON session_operations(pane_key, updated DESC) WHERE snapshot IS NOT NULL;
                CREATE TABLE IF NOT EXISTS session_operation_events (
                    operation_id TEXT NOT NULL, stage TEXT NOT NULL, detail TEXT NOT NULL, at REAL NOT NULL);
            ''')
        os.chmod(self.path, 0o600)

    @contextmanager
    def connect(self):
        db = sqlite3.connect(self.path, timeout=15)
        db.row_factory = sqlite3.Row
        try:
            with db:
                yield db
        finally:
            db.close()

    def get(self, operation_id):
        with self.connect() as db:
            row = db.execute('SELECT * FROM session_operations WHERE id=?', (operation_id,)).fetchone()
        if row is None:
            return None
        result = dict(row)
        for key in ('request', 'snapshot', 'result'):
            result[key] = json.loads(result[key]) if result[key] else None
        return result

    def claim(self, operation_id, pane_key, request):
        raw = json.dumps(request, sort_keys=True, separators=(',', ':'))
        fingerprint = hashlib.sha256(raw.encode()).hexdigest()
        with self.connect() as db:
            db.execute('BEGIN IMMEDIATE')
            existing = db.execute('SELECT fingerprint FROM session_operations WHERE id=?', (operation_id,)).fetchone()
            if existing:
                if existing['fingerprint'] != fingerprint:
                    raise OperationConflict('requestId ya se usó con otra configuración')
                return False
            try:
                db.execute('INSERT INTO session_operations VALUES (?,?,?,?,?,?,NULL,NULL,?)',
                           (operation_id, pane_key, fingerprint, raw, 'validating', os.getpid(), time.time()))
            except sqlite3.IntegrityError as exc:
                raise OperationConflict('el panel tiene una operación pendiente o necesita recuperación') from exc
        return True

    def stage(self, operation_id, state, *, snapshot=None, result=None):
        with self.connect() as db:
            db.execute('BEGIN IMMEDIATE')
            current = db.execute('SELECT state FROM session_operations WHERE id=?', (operation_id,)).fetchone()
            if current and current['state'] == 'failed' and state != 'failed':
                raise OperationConflict('operación cancelada')
            db.execute('UPDATE session_operations SET state=?, snapshot=COALESCE(?,snapshot), '
                       'result=COALESCE(?,result), updated=? WHERE id=?',
                       (state, json.dumps(snapshot) if snapshot is not None else None,
                        json.dumps(result) if result is not None else None, time.time(), operation_id))
            db.execute('INSERT INTO session_operation_events VALUES (?,?,?,?)',
                       (operation_id, state, json.dumps(result or {}), time.time()))

    def cancel_waiting(self, operation_id):
        result = json.dumps({'ok': False, 'error': 'operación cancelada antes de cerrar el origen', 'cancelled': True})
        with self.connect() as db:
            return bool(db.execute("UPDATE session_operations SET state='failed',result=?,updated=? WHERE id=? "
                                   "AND state IN ('validating','waiting','snapshot')", (result, time.time(), operation_id)).rowcount)

    def saved_origin(self, pane_key, harness):
        with self.connect() as db:
            rows = db.execute('SELECT snapshot FROM session_operations WHERE pane_key=? AND snapshot IS NOT NULL '
                              'ORDER BY updated DESC', (pane_key,)).fetchall()
        for row in rows:
            origin = json.loads(row['snapshot']).get('origin') or {}
            if origin.get('agent') == harness and (origin.get('observed') or {}).get('conversationId'):
                return origin
        return None

    def latest_for_target(self, session, pane):
        with self.connect() as db:
            row = db.execute("SELECT id FROM session_operations WHERE json_extract(request,'$.session')=? "
                             "AND json_extract(request,'$.pane')=? ORDER BY updated DESC LIMIT 1", (session, pane)).fetchone()
        return self.get(row['id']) if row else None

    def claim_recovery(self, operation_id):
        with self.connect() as db:
            changed = db.execute("UPDATE session_operations SET state='recovering',owner=?,updated=? "
                                 "WHERE id=? AND state='recovery_required'", (os.getpid(), time.time(), operation_id)).rowcount
        return bool(changed)

    def recover_abandoned(self):
        """Never replay uncertain terminal input after a dashboard restart."""
        with self.connect() as db:
            rows = db.execute("SELECT id,owner,state FROM session_operations WHERE state NOT IN "
                              "('confirmed','failed','rolled_back','recovery_required')").fetchall()
        for row in rows:
            try:
                os.kill(row['owner'], 0)
            except ProcessLookupError:
                state = 'failed' if row['state'] in ('validating', 'waiting', 'snapshot') else 'recovery_required'
                self.stage(row['id'], state, result={'ok': False, 'error': 'operación interrumpida; revisar recuperación',
                                                    'recoveryRequired': state == 'recovery_required'})
            except PermissionError:
                pass


def run_operation(store, operation_id, adapter, notify=lambda stage, result: None):
    """Validate and pin an exact recovery point before the first destructive step.

    Adapter.prepare validates and returns launch details without altering the pane.
    Snapshot and stage commits finish before exit/apply. A failed rollback retains
    the lock so another request cannot destroy the remaining recovery evidence.
    """
    snapshot = None
    destructive = False
    def stage(name, result=None, saved=None):
        store.stage(operation_id, name, snapshot=saved, result=result)
        try:
            notify(name, result)
        except Exception:
            pass  # A status transport failure must not undo committed success.
    try:
        plan = adapter.prepare()
        if plan.get('unchanged'):
            adapter.check_identity()
            observed = adapter.verify(plan, None)
            if not observed:
                raise RuntimeError('no se confirmó la configuración actual')
            result = {'ok': True, 'observed': observed, 'unchanged': True}
            stage('confirmed', result)
            return result
        stage('waiting')
        adapter.wait_idle()
        adapter.check_identity()
        snapshot = adapter.snapshot(plan)
        stage('snapshot', saved=snapshot)
        adapter.check_identity()
        stage('applying')
        destructive = True
        adapter.apply(plan, snapshot)
        stage('verifying')
        observed = adapter.verify(plan, snapshot)
        if not observed:
            raise RuntimeError('el destino no confirmó conversación y configuración')
        result = {'ok': True, 'observed': observed}
        stage('confirmed', result)
        return result
    except Exception as exc:
        result = {'ok': False, 'error': str(exc)}
        if destructive and snapshot:
            stage('recovering', result)
            try:
                adapter.rollback(snapshot)
                result['rolledBack'] = True
                stage('rolled_back', result)
            except Exception as recovery:
                result.update(recoveryRequired=True, recoveryError=str(recovery))
                stage('recovery_required', result)
        else:
            stage('failed', result)
        return result
