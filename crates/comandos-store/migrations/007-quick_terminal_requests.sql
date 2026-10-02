
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
