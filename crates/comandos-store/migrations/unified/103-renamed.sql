CREATE TABLE news_radar_events (
    url TEXT PRIMARY KEY, source TEXT NOT NULL, kind TEXT NOT NULL,
    title TEXT NOT NULL, at INTEGER NOT NULL,
    first_seen INTEGER NOT NULL, last_seen INTEGER NOT NULL,
    meta TEXT NOT NULL DEFAULT '{}');
CREATE TABLE operator_actions (
    id TEXT PRIMARY KEY, tool TEXT, status TEXT, detail TEXT, created REAL, updated REAL);
