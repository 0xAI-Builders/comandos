
        ALTER TABLE news_editions ADD COLUMN lead TEXT;
        ALTER TABLE news_stories ADD COLUMN meta TEXT NOT NULL DEFAULT '{}';
        CREATE TABLE news_captures (
            source_id INTEGER PRIMARY KEY REFERENCES news_sources(id) ON DELETE CASCADE,
            captured_at_ms INTEGER NOT NULL,
            final_url TEXT NOT NULL,
            title TEXT,
            byline TEXT,
            lang TEXT,
            blocks TEXT NOT NULL DEFAULT '[]',
            partial INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE news_translations (
            source_id INTEGER NOT NULL REFERENCES news_sources(id) ON DELETE CASCADE,
            lang TEXT NOT NULL,
            state TEXT NOT NULL CHECK (state IN ('running','done','failed')),
            title TEXT,
            blocks TEXT NOT NULL DEFAULT '[]',
            model TEXT,
            error TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (source_id, lang));
        CREATE TABLE news_chat (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            story_id INTEGER NOT NULL,
            edition_id TEXT NOT NULL,
            role TEXT NOT NULL CHECK (role IN ('user','assistant')),
            state TEXT NOT NULL CHECK (state IN ('done','pending','failed')),
            text TEXT NOT NULL DEFAULT '',
            cite TEXT,
            model TEXT,
            created_at_ms INTEGER NOT NULL);
        CREATE INDEX news_chat_by_story ON news_chat (story_id, id);
        CREATE TABLE news_notes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            story_id INTEGER NOT NULL,
            edition_id TEXT NOT NULL,
            story_title TEXT NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('chat','libre')),
            chat_id INTEGER,
            quote TEXT,
            cite TEXT,
            text TEXT NOT NULL DEFAULT '',
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL);
        CREATE INDEX news_notes_by_story ON news_notes (story_id);
        CREATE INDEX news_notes_by_time ON news_notes (created_at_ms);
        CREATE TABLE news_saved (
            story_id INTEGER PRIMARY KEY,
            edition_id TEXT NOT NULL,
            saved_at_ms INTEGER NOT NULL);
