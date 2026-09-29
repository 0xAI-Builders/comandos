"""Shared CommandOS state database: one SQLite file, versioned migrations.

Callers own their transactions (``isolation_level=None``): helpers never
commit on their own, so a caller can group several writes atomically.
Every process opens its own connection; do not share one across threads.
"""
import os
from pathlib import Path
import sqlite3
import time

BUSY_TIMEOUT_MS = 5000

# (version, name, sql). Append only; never edit a released migration.
MIGRATIONS = [
    (1, "workspace", """
        CREATE TABLE workspace_current (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            revision INTEGER NOT NULL,
            document TEXT NOT NULL,
            updated_at REAL NOT NULL);
        CREATE TABLE workspace_previous (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            revision INTEGER NOT NULL,
            document TEXT NOT NULL,
            updated_at REAL NOT NULL);
        CREATE TABLE workspace_requests (
            request_id TEXT PRIMARY KEY,
            digest TEXT NOT NULL,
            revision INTEGER NOT NULL,
            created_at REAL NOT NULL);
        CREATE TABLE workspace_clients (
            device_id TEXT PRIMARY KEY,
            state TEXT NOT NULL,
            updated_at REAL NOT NULL);
        CREATE TABLE workspace_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL);
    """),
    (2, "events", """
        CREATE TABLE events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            event_id TEXT NOT NULL UNIQUE,
            dedupe_key TEXT UNIQUE,
            source_event_id TEXT,
            source TEXT NOT NULL,
            harness TEXT,
            project_key TEXT,
            session_key TEXT,
            pane_key TEXT,
            pane_id TEXT,
            process_key TEXT,
            conversation_id TEXT,
            turn_id TEXT,
            request_id TEXT,
            kind TEXT NOT NULL,
            evidence TEXT NOT NULL,
            correlation TEXT NOT NULL,
            occurred_at_ms INTEGER NOT NULL,
            received_at_ms INTEGER NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            excerpt TEXT NOT NULL DEFAULT '');
        CREATE INDEX events_pane ON events (pane_key, sequence);
        CREATE INDEX events_session ON events (session_key, sequence);
        CREATE TABLE event_receipts (
            receipt_id TEXT PRIMARY KEY,
            event_id TEXT NOT NULL REFERENCES events (event_id),
            source TEXT NOT NULL,
            received_at_ms INTEGER NOT NULL,
            duplicate INTEGER NOT NULL);
        CREATE INDEX event_receipts_event ON event_receipts (event_id);
        CREATE TABLE deliveries (
            delivery_id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_id TEXT NOT NULL REFERENCES events (event_id),
            channel TEXT NOT NULL,
            device_id TEXT NOT NULL DEFAULT '',
            state TEXT NOT NULL DEFAULT 'claimed',
            attempts INTEGER NOT NULL DEFAULT 0,
            next_attempt_at_ms INTEGER,
            detail TEXT NOT NULL DEFAULT '',
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            UNIQUE (event_id, channel, device_id));
    """),
]


def default_path():
    base = os.environ.get("XDG_STATE_HOME") or os.path.expanduser("~/.local/state")
    return Path(base) / "comandos" / "app-state.sqlite3"


def connect(path=None):
    path = Path(path or os.environ.get("COMANDOS_STATE_DB") or default_path())
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    conn = sqlite3.connect(str(path), timeout=BUSY_TIMEOUT_MS / 1000, isolation_level=None)
    conn.execute("PRAGMA foreign_keys = ON")
    conn.execute(f"PRAGMA busy_timeout = {BUSY_TIMEOUT_MS}")
    conn.execute("PRAGMA journal_mode = WAL")
    conn.execute("CREATE TABLE IF NOT EXISTS schema_migrations "
                 "(version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at REAL NOT NULL)")
    return conn


def schema_version(conn):
    return conn.execute("SELECT COALESCE(MAX(version), 0) FROM schema_migrations").fetchone()[0]


def _db_file(conn):
    row = conn.execute("PRAGMA database_list").fetchone()
    return Path(row[2]) if row and row[2] else None


def _has_user_tables(conn):
    return conn.execute("SELECT 1 FROM sqlite_master WHERE type='table' "
                        "AND name NOT IN ('schema_migrations') LIMIT 1").fetchone() is not None


def _backup(conn, version):
    path = _db_file(conn)
    if path is None:
        return None
    target = path.with_name(f"{path.name}.pre-migration-v{version}-{int(time.time())}")
    dest = sqlite3.connect(str(target))
    try:
        conn.backup(dest)
    finally:
        dest.close()
    os.chmod(target, 0o600)
    return target


def _statements(sql):
    # Split on complete statements so a syntax error aborts inside our
    # transaction instead of executescript's implicit COMMIT.
    buf = ""
    for part in sql.split(";"):
        buf += part + ";"
        if sqlite3.complete_statement(buf):
            if buf.strip(" \n\t;"):
                yield buf
            buf = ""
    if buf.strip(" \n\t;"):
        yield buf


def migrate(conn):
    """Apply pending migrations in one transaction; copy the file first."""
    current = schema_version(conn)
    pending = [m for m in sorted(MIGRATIONS) if m[0] > current]
    if not pending:
        return current
    if _has_user_tables(conn):
        _backup(conn, current)
    conn.execute("BEGIN IMMEDIATE")
    try:
        for version, name, sql in pending:
            for statement in _statements(sql):
                conn.execute(statement)
            conn.execute("INSERT INTO schema_migrations VALUES (?, ?, ?)", (version, name, time.time()))
        conn.execute("COMMIT")
    except BaseException:
        conn.execute("ROLLBACK")
        raise
    return schema_version(conn)
