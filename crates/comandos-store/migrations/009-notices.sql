
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
