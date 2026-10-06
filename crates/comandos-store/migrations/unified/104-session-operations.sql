CREATE TABLE IF NOT EXISTS session_operations (
 id TEXT PRIMARY KEY, pane_key TEXT NOT NULL, fingerprint TEXT NOT NULL,
 request TEXT NOT NULL, state TEXT NOT NULL, owner INTEGER NOT NULL,
 snapshot TEXT, result TEXT, updated REAL NOT NULL);
 CREATE UNIQUE INDEX IF NOT EXISTS session_operations_active_pane
 ON session_operations(pane_key) WHERE state NOT IN ('confirmed','failed','rolled_back');
 CREATE INDEX IF NOT EXISTS session_operations_target_updated
 ON session_operations(json_extract(request,'$.session'), json_extract(request,'$.pane'), updated DESC);
 CREATE INDEX IF NOT EXISTS session_operations_pane_updated
 ON session_operations(pane_key, updated DESC) WHERE snapshot IS NOT NULL;
 CREATE TABLE IF NOT EXISTS session_operation_events (
 operation_id TEXT NOT NULL, stage TEXT NOT NULL, detail TEXT NOT NULL, at REAL NOT NULL);
