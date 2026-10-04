
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
