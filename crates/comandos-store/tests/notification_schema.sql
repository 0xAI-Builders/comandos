CREATE TABLE client_presence(device_id TEXT PRIMARY KEY,kind TEXT NOT NULL,visible INTEGER NOT NULL,can_play_audio INTEGER NOT NULL,last_seen_at_ms INTEGER NOT NULL,last_interaction_at_ms INTEGER,hidden_since_ms INTEGER);
CREATE TABLE notice_reads(event_id TEXT PRIMARY KEY,read_at_ms INTEGER NOT NULL);
CREATE TABLE notice_prefs(id INTEGER PRIMARY KEY CHECK(id=1),value TEXT NOT NULL);
CREATE TABLE pomodoro_blocks(block_id TEXT PRIMARY KEY,mode TEXT,status TEXT);
CREATE TABLE pomodoro_state(id INTEGER PRIMARY KEY,block_id TEXT);
INSERT INTO pomodoro_state VALUES(1,NULL);
