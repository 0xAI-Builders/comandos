
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
