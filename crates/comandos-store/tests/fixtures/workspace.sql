
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
