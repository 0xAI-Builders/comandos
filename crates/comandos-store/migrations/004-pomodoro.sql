
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
