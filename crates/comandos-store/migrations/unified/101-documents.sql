CREATE TABLE documents (
    name TEXT PRIMARY KEY,
    domain TEXT NOT NULL,
    body BLOB NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    updated_at_ms INTEGER NOT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('import', 'mirror', 'unified')));
CREATE INDEX documents_by_domain ON documents (domain);
