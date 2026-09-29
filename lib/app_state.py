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
    # N5: sourced news editions. A source is unique by normalized URL; its
    # discovery date is never presented as a publication date.
    (6, "news_editions", """
        CREATE TABLE news_editions (
            id TEXT PRIMARY KEY,
            local_date TEXT NOT NULL,
            slot TEXT NOT NULL,
            timezone TEXT NOT NULL,
            scheduled_at_ms INTEGER NOT NULL,
            status TEXT NOT NULL CHECK (status IN
                ('scheduled','running','published','partial','empty','failed','not_published')),
            published_at_ms INTEGER,
            title TEXT,
            story_count INTEGER NOT NULL DEFAULT 0,
            source_count INTEGER NOT NULL DEFAULT 0,
            failed_source_count INTEGER NOT NULL DEFAULT 0,
            cost_usd REAL NOT NULL DEFAULT 0,
            model TEXT,
            notes TEXT NOT NULL DEFAULT '[]',
            UNIQUE (local_date, slot));
        CREATE INDEX news_editions_by_time ON news_editions (scheduled_at_ms);
        CREATE TABLE news_jobs (
            edition_id TEXT PRIMARY KEY REFERENCES news_editions(id) ON DELETE CASCADE,
            state TEXT NOT NULL CHECK (state IN ('queued','running','done','failed','skipped')),
            attempts INTEGER NOT NULL DEFAULT 0,
            lease_until_ms INTEGER,
            started_at_ms INTEGER,
            finished_at_ms INTEGER,
            error TEXT);
        CREATE TABLE news_sources (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            url TEXT NOT NULL UNIQUE,
            original_url TEXT NOT NULL,
            title TEXT NOT NULL,
            origin TEXT NOT NULL,
            category TEXT NOT NULL,
            published_at_ms INTEGER,
            discovered_at_ms INTEGER NOT NULL,
            verified_at_ms INTEGER,
            fetch_status TEXT NOT NULL CHECK (fetch_status IN ('ok','failed','not_fetched')),
            fetch_error TEXT,
            meta TEXT NOT NULL DEFAULT '{}');
        CREATE TABLE news_stories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            edition_id TEXT NOT NULL REFERENCES news_editions(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            story_key TEXT NOT NULL,
            category TEXT NOT NULL,
            title TEXT NOT NULL,
            summary_md TEXT NOT NULL,
            body_md TEXT NOT NULL,
            opportunity TEXT,
            UNIQUE (edition_id, story_key));
        CREATE TABLE news_story_sources (
            story_id INTEGER NOT NULL REFERENCES news_stories(id) ON DELETE CASCADE,
            source_id INTEGER NOT NULL REFERENCES news_sources(id),
            PRIMARY KEY (story_id, source_id));
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
