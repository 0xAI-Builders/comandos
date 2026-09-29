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
    (3, "work_marks", """
        CREATE TABLE work_marks (
            scope TEXT NOT NULL CHECK (scope IN ('session', 'pane')),
            key TEXT NOT NULL,
            mark TEXT NOT NULL DEFAULT 'none'
                CHECK (mark IN ('none', 'resolved', 'frozen', 'awaiting_reply')),
            favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0, 1)),
            revision INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            updated_by TEXT NOT NULL,
            PRIMARY KEY (scope, key));
        CREATE TABLE work_mark_applied (
            event_id TEXT NOT NULL,
            scope TEXT NOT NULL,
            key TEXT NOT NULL,
            PRIMARY KEY (event_id, scope, key));
    """),
    (4, "pomodoro", """
        CREATE TABLE pomodoro_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            revision INTEGER NOT NULL,
            block_id TEXT,
            settled_from INTEGER);
        INSERT INTO pomodoro_state (id, revision, block_id, settled_from) VALUES (1, 0, NULL, NULL);
        CREATE TABLE pomodoro_blocks (
            block_id TEXT PRIMARY KEY,
            mode TEXT NOT NULL CHECK (mode IN ('focus', 'break')),
            status TEXT NOT NULL CHECK (status IN ('running', 'paused', 'completed', 'cancelled')),
            target_ms INTEGER NOT NULL,
            active_ms INTEGER NOT NULL,
            resumed_at_ms INTEGER,
            deadline_ms INTEGER,
            started_at_ms INTEGER NOT NULL,
            ended_at_ms INTEGER,
            project TEXT NOT NULL DEFAULT '',
            session_key TEXT NOT NULL DEFAULT '',
            pane_key TEXT NOT NULL DEFAULT '',
            cycle_index INTEGER,
            cycle_total INTEGER,
            source TEXT NOT NULL DEFAULT 'comandos',
            updated_at_ms INTEGER NOT NULL);
        CREATE TABLE pomodoro_requests (
            request_id TEXT PRIMARY KEY,
            digest TEXT NOT NULL,
            revision INTEGER NOT NULL,
            response TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL);
        CREATE TABLE pomodoro_records (
            block_id TEXT PRIMARY KEY,
            mode TEXT NOT NULL,
            status TEXT NOT NULL,
            target_ms INTEGER NOT NULL,
            active_ms INTEGER,
            planned_ms INTEGER NOT NULL,
            started_at_ms INTEGER NOT NULL,
            ended_at_ms INTEGER,
            project TEXT NOT NULL DEFAULT '',
            session_key TEXT NOT NULL DEFAULT '',
            pane_key TEXT NOT NULL DEFAULT '',
            provenance TEXT NOT NULL CHECK (provenance IN ('measured', 'legacy-planned')),
            recorded_at_ms INTEGER NOT NULL);
        CREATE INDEX idx_pomodoro_records_started ON pomodoro_records(started_at_ms);
        CREATE INDEX idx_pomodoro_records_project ON pomodoro_records(project, started_at_ms);
    """),
    (5, "focus_progress", """
        CREATE TABLE focus_policies (
            policy_version TEXT PRIMARY KEY,
            document TEXT NOT NULL,
            activated_at_ms INTEGER NOT NULL);
        CREATE TABLE focus_rewards (
            block_id TEXT NOT NULL,
            policy_version TEXT NOT NULL,
            xp INTEGER NOT NULL,
            minutes INTEGER NOT NULL,
            status TEXT NOT NULL,
            started_at_ms INTEGER NOT NULL,
            ended_at_ms INTEGER NOT NULL,
            level_reached INTEGER,
            awarded_at_ms INTEGER NOT NULL,
            PRIMARY KEY (block_id, policy_version));
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
    (7, "quick_terminal_requests", """
        CREATE TABLE quick_terminal_requests (
            request_id TEXT PRIMARY KEY,
            state TEXT NOT NULL CHECK (state IN ('launching', 'ready', 'failed')),
            cwd TEXT NOT NULL,
            session TEXT NOT NULL UNIQUE,
            pane_key TEXT NOT NULL UNIQUE,
            error TEXT,
            lease_until REAL NOT NULL,
            created_at REAL NOT NULL,
            updated_at REAL NOT NULL);
    """),
    # N4: Web Push subscriptions and one delivery row per (event, device).
    (8, "push", """
        CREATE TABLE push_subscriptions (
            id TEXT PRIMARY KEY,
            endpoint TEXT NOT NULL UNIQUE,
            p256dh TEXT NOT NULL,
            auth TEXT NOT NULL,
            device_id TEXT,
            user_agent TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            disabled_at_ms INTEGER,
            disabled_reason TEXT,
            backoff_until_ms INTEGER NOT NULL DEFAULT 0,
            last_status INTEGER);
        CREATE TABLE push_deliveries (
            event_id TEXT NOT NULL,
            subscription_id TEXT NOT NULL REFERENCES push_subscriptions(id) ON DELETE CASCADE,
            state TEXT NOT NULL CHECK (state IN ('sent','retry','failed','gone')),
            attempts INTEGER NOT NULL DEFAULT 0,
            next_attempt_ms INTEGER,
            last_status INTEGER,
            last_error TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (event_id, subscription_id));
    """),
    # N2: client presence for sound routing, shared read state and notice prefs.
    (9, "notices", """
        CREATE TABLE client_presence (
            device_id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            visible INTEGER NOT NULL,
            can_play_audio INTEGER NOT NULL,
            last_seen_at_ms INTEGER NOT NULL,
            last_interaction_at_ms INTEGER,
            hidden_since_ms INTEGER);
        CREATE TABLE notice_reads (
            event_id TEXT PRIMARY KEY,
            read_at_ms INTEGER NOT NULL);
        CREATE TABLE notice_prefs (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            value TEXT NOT NULL);
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
    # Versions are assigned per feature ahead of time, so a lower one can land
    # after a higher one was applied: run every version not yet recorded.
    applied = {row[0] for row in conn.execute("SELECT version FROM schema_migrations")}
    pending = [m for m in sorted(MIGRATIONS) if m[0] not in applied]
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
