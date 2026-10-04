
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
