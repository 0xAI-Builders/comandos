"""Pomodoro authority: the server owns the block, its revision and its end.

Clients only render ``remaining_ms`` from the confirmed block plus the
``serverNowMs`` of the response. Commands are idempotent by ``requestId`` and
optimistic by ``expectedRevision``. Completion happens only here, when the
backend scheduler (or any command) finds a running block past its deadline:
the final state, one record per ``blockId`` and the ``focus_completed`` event
are written in one transaction of the shared app-state database.

The connection follows ``app_state``: ``isolation_level=None``; this module
opens and closes its own transactions and never leaves one pending.
"""
import hashlib
import json
import time
import uuid

MINUTE_MS = 60_000
MIN_TARGET_MS = MINUTE_MS           # existing product limit: 1..180 minutes
MAX_TARGET_MS = 180 * MINUTE_MS
MODES = ("focus", "break")
ACTIONS = ("start", "pause", "resume", "extend", "cancel")
LIVE = ("running", "paused")
REQUEST_TTL_MS = 7 * 24 * 3600_000
MAX_TEXT = 160


class PomodoroError(Exception):
    status = 400

    def __init__(self, code, message, snapshot=None):
        super().__init__(message)
        self.code = code
        self.message = message
        self.snapshot = snapshot

    def payload(self):
        out = {"ok": False, "error": self.message, "code": self.code}
        if self.snapshot is not None:
            out["state"] = self.snapshot
        return out


class Invalid(PomodoroError):
    status = 400


class Conflict(PomodoroError):
    status = 409


def elapsed_ms(block, now_ms):
    running = block["status"] == "running"
    delta = max(0, now_ms - block["resumedAtMs"]) if running else 0
    return min(block["targetMs"], block["activeMs"] + delta)


def remaining_ms(block, now_ms):
    return max(0, block["targetMs"] - elapsed_ms(block, now_ms))


def _system_clock():
    return int(time.time() * 1000)


def _text(value):
    if value is None:
        return ""
    return str(value).strip()[:MAX_TEXT]


def _int(value, what):
    if isinstance(value, bool) or not isinstance(value, int):
        raise Invalid("invalid_" + what, f"{what} inválido")
    return value


def _opt_int(value, lo, hi):
    if isinstance(value, bool) or not isinstance(value, int):
        return None
    return max(lo, min(hi, value))


_BLOCK_COLUMNS = ("block_id, mode, status, target_ms, active_ms, resumed_at_ms, deadline_ms, "
                  "started_at_ms, ended_at_ms, project, session_key, pane_key, cycle_index, "
                  "cycle_total, source")


def _block(row, revision):
    if row is None:
        return None
    (block_id, mode, status, target, active, resumed, deadline, started, ended,
     project, session_key, pane_key, cycle_index, cycle_total, source) = row
    return {"blockId": block_id, "revision": revision, "mode": mode, "status": status,
            "targetMs": target, "activeMs": active, "resumedAtMs": resumed,
            "deadlineMs": deadline, "startedAtMs": started, "endedAtMs": ended,
            "project": project, "sessionKey": session_key, "paneKey": pane_key,
            "cycleIndex": cycle_index, "cycleTotal": cycle_total, "source": source}


class PomodoroStore:
    """One authority per state database.

    ``clock()`` returns epoch milliseconds (inject a fake in tests; the system
    clock is never changed). ``emit(conn, event)`` runs inside the settlement
    transaction; an exception rolls back the whole completion so it is retried
    later. ``rewards(conn, record, now_ms)`` likewise runs inside the same
    transaction for every terminal focus record.
    """

    def __init__(self, conn, clock=None, emit=None, rewards=None, log=None):
        self.conn = conn
        self.clock = clock or _system_clock
        self.emit = emit
        self.rewards = rewards
        self.log = log or (lambda *_: None)

    # ---- reads -----------------------------------------------------------
    def _state(self):
        return self.conn.execute("SELECT revision, block_id FROM pomodoro_state WHERE id = 1").fetchone()

    def _current(self):
        revision, block_id = self._state()
        row = None
        if block_id:
            row = self.conn.execute(f"SELECT {_BLOCK_COLUMNS} FROM pomodoro_blocks WHERE block_id = ?",
                                    (block_id,)).fetchone()
        return revision, _block(row, revision)

    def _snapshot_locked(self, now_ms):
        revision, block = self._current()
        return {"revision": revision, "serverNowMs": now_ms, "block": block}

    def snapshot(self):
        """Current confirmed state. Never mutates the timer."""
        now = self.clock()
        self.conn.execute("BEGIN")
        try:
            return self._snapshot_locked(now)
        finally:
            self.conn.execute("COMMIT")

    def next_deadline_ms(self):
        _, block = self._current()
        if block and block["status"] == "running":
            return block["deadlineMs"]
        return None

    # ---- transactions ----------------------------------------------------
    def _write(self, fn):
        self.conn.execute("BEGIN IMMEDIATE")
        try:
            out = fn()
        except BaseException:
            self.conn.execute("ROLLBACK")
            raise
        self.conn.execute("COMMIT")
        return out

    def _bump(self, block_id=None, keep_block=True, settled=False):
        revision, current = self._state()
        target = current if keep_block and block_id is None else block_id
        # A revision whose only successor is an automatic completion stays a
        # valid basis for the next command: the client could not have known.
        self.conn.execute("UPDATE pomodoro_state SET revision = ?, block_id = ?, settled_from = ? WHERE id = 1",
                          (revision + 1, target, revision if settled else None))
        return revision + 1

    def _record(self, block, status, active_ms, ended_ms, now_ms):
        cur = self.conn.execute(
            "INSERT INTO pomodoro_records (block_id, mode, status, target_ms, active_ms, planned_ms, "
            "started_at_ms, ended_at_ms, project, session_key, pane_key, provenance, recorded_at_ms) "
            "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'measured', ?) ON CONFLICT(block_id) DO NOTHING",
            (block["blockId"], block["mode"], status, block["targetMs"], active_ms, block["targetMs"],
             block["startedAtMs"], ended_ms, block["project"], block["sessionKey"], block["paneKey"], now_ms))
        if cur.rowcount != 1:
            return None
        record = {"blockId": block["blockId"], "mode": block["mode"], "status": status,
                  "targetMs": block["targetMs"], "activeMs": active_ms, "plannedMs": block["targetMs"],
                  "startedAtMs": block["startedAtMs"], "endedAtMs": ended_ms,
                  "project": block["project"], "sessionKey": block["sessionKey"],
                  "paneKey": block["paneKey"], "provenance": "measured"}
        if self.rewards and block["mode"] == "focus":
            self.rewards(self.conn, record, now_ms)
        return record

    def _settle_locked(self, now_ms):
        _, block = self._current()
        if not block or block["status"] != "running" or block["deadlineMs"] > now_ms:
            return None
        ended = block["deadlineMs"]
        self.conn.execute("UPDATE pomodoro_blocks SET status = 'completed', active_ms = target_ms, "
                          "resumed_at_ms = NULL, deadline_ms = NULL, ended_at_ms = ?, updated_at_ms = ? "
                          "WHERE block_id = ? AND status = 'running'", (ended, now_ms, block["blockId"]))
        record = self._record(block, "completed", block["targetMs"], ended, now_ms)
        self._bump(settled=True)
        if record is not None and block["mode"] == "focus" and self.emit:
            minutes = block["targetMs"] // MINUTE_MS
            project = block["project"] or "sin proyecto"
            self.emit(self.conn, {
                "eventId": f"pomodoro:{block['blockId']}:completed",
                "source": "pomodoro", "kind": "focus_completed",
                "evidence": "confirmed", "correlation": "source",
                "projectKey": block["project"], "sessionKey": block["sessionKey"],
                "paneKey": block["paneKey"],
                "occurredAtMs": ended, "receivedAtMs": now_ms,
                "title": "Pomodoro completado",
                "excerpt": f"{minutes} min de foco · {project}",
                "blockId": block["blockId"],
            })
        return block["blockId"]

    def settle_due(self, now_ms=None):
        """Complete a running block whose deadline has passed. Idempotent."""
        now = self.clock() if now_ms is None else now_ms
        done = self._write(lambda: self._settle_locked(now))
        if done:
            self.log(f"pomodoro: block {done} completed")
        return [done] if done else []

    # ---- commands --------------------------------------------------------
    @staticmethod
    def _digest(request):
        body = {k: v for k, v in request.items() if k not in ("requestId", "expectedRevision")}
        return hashlib.sha256(json.dumps(body, sort_keys=True, default=str).encode()).hexdigest()

    def command(self, request):
        if not isinstance(request, dict):
            raise Invalid("invalid_request", "Solicitud inválida")
        request_id = request.get("requestId")
        if not isinstance(request_id, str) or not request_id.strip() or len(request_id) > 120:
            raise Invalid("invalid_request_id", "requestId requerido")
        action = request.get("action")
        if action not in ACTIONS:
            raise Invalid("invalid_action", "Acción no soportada")
        expected = request.get("expectedRevision")
        if expected is not None:
            _int(expected, "expectedRevision")
        digest = self._digest(request)
        now = self.clock()
        # A due completion commits on its own, even if this command then fails.
        self.settle_due(now)

        def run():
            row = self.conn.execute("SELECT digest, response FROM pomodoro_requests WHERE request_id = ?",
                                    (request_id,)).fetchone()
            if row:
                if row[0] != digest:
                    raise Conflict("request_reused", "requestId ya usado con otra operación",
                                   self._snapshot_locked(now))
                out = json.loads(row[1])
                out.update(serverNowMs=now, replayed=True)
                return out
            self._settle_locked(now)
            revision, block = self._current()
            settled_from = self.conn.execute("SELECT settled_from FROM pomodoro_state WHERE id = 1").fetchone()[0]
            if expected is not None and expected not in (revision, settled_from):
                raise Conflict("stale_revision", "El temporizador cambió en otro dispositivo",
                               self._snapshot_locked(now))
            getattr(self, "_do_" + action)(request, block, now)
            revision, block = self._current()
            out = {"ok": True, "revision": revision, "block": block, "replayed": False}
            self.conn.execute("DELETE FROM pomodoro_requests WHERE created_at_ms < ?", (now - REQUEST_TTL_MS,))
            self.conn.execute("INSERT INTO pomodoro_requests VALUES (?, ?, ?, ?, ?)",
                              (request_id, digest, revision, json.dumps(out), now))
            out = dict(out, serverNowMs=now)
            return out

        return self._write(run)

    def _require_live(self, block, now, states=LIVE):
        if not block or block["status"] not in states:
            raise Conflict("not_active", "No hay un bloque en ese estado", self._snapshot_locked(now))

    def _do_start(self, request, block, now):
        if block and block["status"] in LIVE:
            raise Conflict("active_block", "Ya hay un bloque en curso", self._snapshot_locked(now))
        mode = request.get("mode", "focus")
        if mode not in MODES:
            raise Invalid("invalid_mode", "Modo inválido")
        target = _int(request.get("targetMs"), "targetMs")
        if not MIN_TARGET_MS <= target <= MAX_TARGET_MS:
            raise Invalid("invalid_targetMs", "La duración debe estar entre 1 y 180 minutos")
        block_id = uuid.uuid4().hex
        self.conn.execute(
            f"INSERT INTO pomodoro_blocks ({_BLOCK_COLUMNS}, updated_at_ms) "
            "VALUES (?, ?, 'running', ?, 0, ?, ?, ?, NULL, ?, ?, ?, ?, ?, 'comandos', ?)",
            (block_id, mode, target, now, now + target, now, _text(request.get("project")),
             _text(request.get("sessionKey")), _text(request.get("paneKey")),
             _opt_int(request.get("cycleIndex"), 1, 99), _opt_int(request.get("cycleTotal"), 1, 99), now))
        self._bump(block_id, keep_block=False)

    def _do_pause(self, request, block, now):
        self._require_live(block, now, ("running",))
        self.conn.execute("UPDATE pomodoro_blocks SET status = 'paused', active_ms = ?, resumed_at_ms = NULL, "
                          "deadline_ms = NULL, updated_at_ms = ? WHERE block_id = ?",
                          (elapsed_ms(block, now), now, block["blockId"]))
        self._bump()

    def _do_resume(self, request, block, now):
        self._require_live(block, now, ("paused",))
        self.conn.execute("UPDATE pomodoro_blocks SET status = 'running', resumed_at_ms = ?, deadline_ms = ?, "
                          "updated_at_ms = ? WHERE block_id = ?",
                          (now, now + block["targetMs"] - block["activeMs"], now, block["blockId"]))
        self._bump()

    def _do_extend(self, request, block, now):
        self._require_live(block, now)
        delta = _int(request.get("deltaMs"), "deltaMs")
        if delta == 0:
            raise Invalid("invalid_deltaMs", "deltaMs no puede ser 0")
        target = block["targetMs"] + delta
        spent = elapsed_ms(block, now)
        if target > MAX_TARGET_MS or target < max(MIN_TARGET_MS, spent + 1000):
            raise Invalid("invalid_deltaMs", "La duración resultante queda fuera de 1 a 180 minutos "
                          "o antes del tiempo ya trabajado")
        deadline = None
        if block["status"] == "running":
            deadline = block["resumedAtMs"] + target - block["activeMs"]
        self.conn.execute("UPDATE pomodoro_blocks SET target_ms = ?, deadline_ms = ?, updated_at_ms = ? "
                          "WHERE block_id = ?", (target, deadline, now, block["blockId"]))
        self._bump()

    def _do_cancel(self, request, block, now):
        self._require_live(block, now)
        active = elapsed_ms(block, now)
        self.conn.execute("UPDATE pomodoro_blocks SET status = 'cancelled', active_ms = ?, resumed_at_ms = NULL, "
                          "deadline_ms = NULL, ended_at_ms = ?, updated_at_ms = ? WHERE block_id = ?",
                          (active, now, now, block["blockId"]))
        block = dict(block, activeMs=active)
        self._record(block, "cancelled", active, now, now)
        self._bump()

    # ---- migration from the former focus.json authority -------------------
    def import_legacy_focus(self, data, now_ms=None):
        """Adopt a still-running block written by the old ``focus.json`` file.

        Expired or malformed files are ignored; the caller keeps the file
        (renamed) so nothing is lost. Returns True only when a block was added.
        """
        now = self.clock() if now_ms is None else now_ms
        if not isinstance(data, dict):
            return False
        try:
            until = int(float(data.get("until")) * 1000)
            started = int(float(data.get("startedAt")) * 1000)
            mins = int(data.get("mins") or round((until - started) / MINUTE_MS))
        except (TypeError, ValueError):
            return False
        mode = data.get("mode") if data.get("mode") in MODES else "focus"
        target = mins * MINUTE_MS
        if until <= now or not MIN_TARGET_MS <= target <= MAX_TARGET_MS:
            return False
        block_id = _text(data.get("blockId")) or f"legacy-focus-{started}"

        def run():
            _, block = self._current()
            if block and block["status"] in LIVE:
                return False
            if self.conn.execute("SELECT 1 FROM pomodoro_blocks WHERE block_id = ?", (block_id,)).fetchone():
                return False
            self.conn.execute(
                f"INSERT INTO pomodoro_blocks ({_BLOCK_COLUMNS}, updated_at_ms) "
                "VALUES (?, ?, 'running', ?, 0, ?, ?, ?, NULL, ?, ?, ?, ?, ?, 'legacy-focus-file', ?)",
                (block_id, mode, target, until - target, until, started, _text(data.get("project")),
                 _text(data.get("session")), _text(data.get("pane")),
                 _opt_int(data.get("cycleIndex"), 1, 99), _opt_int(data.get("cycleTotal"), 1, 99), now))
            self._bump(block_id, keep_block=False)
            return True

        return self._write(run)
