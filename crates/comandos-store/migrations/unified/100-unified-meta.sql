CREATE TABLE domain_modes (
    domain TEXT PRIMARY KEY,
    mode TEXT NOT NULL CHECK (mode IN ('legacy', 'mirror', 'unified', 'sealed')),
    changed_at_ms INTEGER NOT NULL,
    changed_by TEXT NOT NULL);
CREATE TABLE migration_runs (
    run_id TEXT PRIMARY KEY,
    started_at_ms INTEGER NOT NULL,
    finished_at_ms INTEGER,
    backup_dir TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running', 'done', 'failed')));
CREATE TABLE migration_steps (
    run_id TEXT NOT NULL REFERENCES migration_runs (run_id),
    domain TEXT NOT NULL,
    source TEXT NOT NULL,
    source_sha256 TEXT NOT NULL,
    source_size INTEGER NOT NULL,
    rows INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('done', 'skipped', 'failed')),
    detail TEXT NOT NULL DEFAULT '',
    finished_at_ms INTEGER NOT NULL,
    PRIMARY KEY (run_id, domain, source));
