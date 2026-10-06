CREATE TABLE session_status (
    file_key TEXT PRIMARY KEY,
    body BLOB NOT NULL,
    mtime_ns INTEGER NOT NULL,
    origin TEXT NOT NULL);
CREATE TABLE native_processes (
    pid INTEGER PRIMARY KEY,
    body BLOB NOT NULL,
    mtime_ns INTEGER NOT NULL,
    origin TEXT NOT NULL);
CREATE TABLE log_lines (
    log TEXT NOT NULL CHECK (log IN ('events', 'ui-events', 'focus-queue')),
    seq INTEGER NOT NULL,
    line BLOB NOT NULL,
    PRIMARY KEY (log, seq));
CREATE TABLE layout_snapshots (
    stamp INTEGER NOT NULL,
    generation TEXT NOT NULL CHECK (generation IN ('current', 'previous', 'minute')),
    body BLOB NOT NULL,
    PRIMARY KEY (generation, stamp));
CREATE TABLE app_commands (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK (kind IN ('focus', 'open', 'close', 'command', 'back')),
    body BLOB NOT NULL,
    created_at_ms INTEGER NOT NULL,
    consumed_at_ms INTEGER);
CREATE INDEX app_commands_pending ON app_commands (kind, seq) WHERE consumed_at_ms IS NULL;
