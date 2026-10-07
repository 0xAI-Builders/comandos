//! Lazy extension tables shared by runtime writers and physical DB moves.
pub const DRAFTS_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS pane_extension_drafts (key TEXT PRIMARY KEY,identity TEXT NOT NULL,conversation TEXT NOT NULL,harness TEXT NOT NULL,desired TEXT NOT NULL,revision INTEGER NOT NULL DEFAULT 0,updated REAL NOT NULL)";
pub const TEMPLATES_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS pane_extension_templates (id TEXT PRIMARY KEY,name TEXT NOT NULL,selection TEXT NOT NULL,updated REAL NOT NULL)";
