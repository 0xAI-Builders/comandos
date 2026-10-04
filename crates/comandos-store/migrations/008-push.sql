
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
