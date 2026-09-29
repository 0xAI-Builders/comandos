"""Quick terminals: a plain shell in its own uniquely dated folder.

``open_quick_terminal`` is idempotent per ``requestId``: the reserved folder,
tmux session and pane key are stored (table ``quick_terminal_requests``,
migration 7 of ``app_state``) before the shell is started, so a repeated or
concurrent request returns the same terminal and a retry after a failure
reuses the same folder without opening a second shell.

The shell creator, existence probe and tab registration are injected; this
module never talks to tmux on its own.
"""
from datetime import datetime
import hashlib
import os
from pathlib import Path
import re
import time
from zoneinfo import ZoneInfo

ZONE = ZoneInfo("America/Mexico_City")
REQUEST_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}\Z")
LEASE_SECONDS = 30.0
WAIT_SECONDS = 15.0
POLL_SECONDS = 0.05


class QuickTerminalError(Exception):
    """``code``: request | folder | launch | busy. ``cwd`` when one is reserved."""

    def __init__(self, code, message, retryable=False, cwd=None):
        super().__init__(message)
        self.code, self.retryable, self.cwd = code, retryable, cwd


def default_base():
    """Approved base folder; ``COMANDOS_QUICK_TERMINAL_BASE`` overrides it."""
    override = os.environ.get("COMANDOS_QUICK_TERMINAL_BASE")
    return Path(override) if override else Path(os.path.expanduser("~/codebase/0xJesus/Terminal"))


def reserve_directory(base, now=None):
    """Create and return a new ``T-YYYY-MM-DD-HH-MM-SS[-n]`` folder.

    ``mkdir`` is the atomic reservation: concurrent callers in the same second
    get distinct suffixes and an existing folder is never reused.
    """
    base = Path(base)
    base.mkdir(parents=True, exist_ok=True)
    at = now or datetime.now(ZONE)
    if at.tzinfo is None:
        raise ValueError("Se requiere fecha con zona horaria")
    stem = at.astimezone(ZONE).strftime("T-%Y-%m-%d-%H-%M-%S")
    suffix = 1
    while True:
        candidate = base / (stem if suffix == 1 else f"{stem}-{suffix}")
        try:
            candidate.mkdir()
            return candidate
        except FileExistsError:
            suffix += 1


def identity(request_id):
    """Deterministic tmux session and logical pane key for one request."""
    digest = hashlib.sha256(request_id.encode()).hexdigest()
    return "term-q" + digest[:12], "pane-q" + digest[:24]


def _result(row, created):
    cwd, session, pane_key = row
    return {"tabId": session, "paneKey": pane_key, "cwd": cwd,
            "label": os.path.basename(cwd), "created": created}


def _claim(conn, request_id, base, now, clock, lease):
    """One transaction: return ('ready', row) or ('own', row) or ('wait', None)."""
    conn.execute("BEGIN IMMEDIATE")
    try:
        row = conn.execute("SELECT state, cwd, session, pane_key, lease_until "
                           "FROM quick_terminal_requests WHERE request_id = ?",
                           (request_id,)).fetchone()
        t = clock()
        if row is None:
            try:
                cwd = str(reserve_directory(base, now))
            except OSError as exc:
                raise QuickTerminalError(
                    "folder", f"No se pudo crear la carpeta en {base}: {exc.strerror or exc}",
                    retryable=True) from exc
            session, pane_key = identity(request_id)
            conn.execute("INSERT INTO quick_terminal_requests VALUES (?, 'launching', ?, ?, ?, NULL, ?, ?, ?)",
                         (request_id, cwd, session, pane_key, t + lease, t, t))
            out = ("own", (cwd, session, pane_key))
        elif row[0] == "ready":
            out = ("ready", row[1:4])
        elif row[0] == "launching" and row[4] > t:
            out = ("wait", None)
        else:
            conn.execute("UPDATE quick_terminal_requests SET state = 'launching', error = NULL, "
                         "lease_until = ?, updated_at = ? WHERE request_id = ?",
                         (t + lease, t, request_id))
            out = ("own", row[1:4])
        conn.execute("COMMIT")
        return out
    except BaseException:
        conn.execute("ROLLBACK")
        raise


def _finish(conn, request_id, state, error, clock):
    conn.execute("BEGIN IMMEDIATE")
    try:
        conn.execute("UPDATE quick_terminal_requests SET state = ?, error = ?, updated_at = ? "
                     "WHERE request_id = ?", (state, error, clock(), request_id))
        conn.execute("COMMIT")
    except BaseException:
        conn.execute("ROLLBACK")
        raise


def open_quick_terminal(conn, request_id, *, base, launch, exists, register, now=None,
                        clock=time.time, sleep=time.sleep, lease=LEASE_SECONDS, wait=WAIT_SECONDS):
    """Open (or return) the quick terminal for ``request_id``.

    ``launch(session, cwd, pane_key)`` starts a plain shell (no AI) and raises
    on failure; ``exists(session)`` reports a live session; ``register(session,
    label, cwd)`` adds the tab to the shared registry. Returns
    ``{tabId, paneKey, cwd, label, created}``.
    """
    if not isinstance(request_id, str) or not REQUEST_RE.fullmatch(request_id):
        raise QuickTerminalError("request", "requestId inválido")
    deadline = clock() + wait
    while True:
        kind, row = _claim(conn, request_id, base, now, clock, lease)
        if kind == "ready":
            return _result(row, created=False)
        if kind == "own":
            break
        if clock() >= deadline:
            raise QuickTerminalError("busy", "La terminal se está abriendo; reintenta en unos segundos",
                                     retryable=True)
        sleep(POLL_SECONDS)
    cwd, session, pane_key = row
    try:
        os.makedirs(cwd, exist_ok=True)   # a retry keeps the same reserved name
        if not exists(session):
            launch(session, cwd, pane_key)
        register(session, os.path.basename(cwd), cwd)
    except Exception as exc:
        message = str(exc) or exc.__class__.__name__
        _finish(conn, request_id, "failed", message[:500], clock)
        raise QuickTerminalError("launch", f"No se pudo abrir la terminal: {message}",
                                 retryable=True, cwd=cwd) from exc
    _finish(conn, request_id, "ready", None, clock)
    return _result(row, created=True)
