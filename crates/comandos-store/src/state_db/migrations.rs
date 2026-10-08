//! Released SQL is copied unchanged except final whitespace from Python.
use super::Migration;
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "workspace",
        sql: include_str!("../../migrations/001-workspace.sql"),
    },
    Migration {
        version: 2,
        name: "events",
        sql: include_str!("../../migrations/002-events.sql"),
    },
    Migration {
        version: 3,
        name: "work_marks",
        sql: include_str!("../../migrations/003-work_marks.sql"),
    },
    Migration {
        version: 4,
        name: "pomodoro",
        sql: include_str!("../../migrations/004-pomodoro.sql"),
    },
    Migration {
        version: 5,
        name: "focus_progress",
        sql: include_str!("../../migrations/005-focus_progress.sql"),
    },
    Migration {
        version: 6,
        name: "news_editions",
        sql: include_str!("../../migrations/006-news_editions.sql"),
    },
    Migration {
        version: 7,
        name: "quick_terminal_requests",
        sql: include_str!("../../migrations/007-quick_terminal_requests.sql"),
    },
    Migration {
        version: 8,
        name: "push",
        sql: include_str!("../../migrations/008-push.sql"),
    },
    Migration {
        version: 9,
        name: "notices",
        sql: include_str!("../../migrations/009-notices.sql"),
    },
    Migration {
        version: 10,
        name: "news_story_model",
        sql: include_str!("../../migrations/010-news_story_model.sql"),
    },
    Migration {
        version: 11,
        name: "news_reading",
        sql: include_str!("../../migrations/011-news_reading.sql"),
    },
];

/// Esquemas publicados sin cambios, seguidos por la base única.
pub const UNIFIED_MIGRATIONS: &[Migration] = &[
    MIGRATIONS[0],
    MIGRATIONS[1],
    MIGRATIONS[2],
    MIGRATIONS[3],
    MIGRATIONS[4],
    MIGRATIONS[5],
    MIGRATIONS[6],
    MIGRATIONS[7],
    MIGRATIONS[8],
    MIGRATIONS[9],
    MIGRATIONS[10],
    Migration {
        version: 100,
        name: "unified-meta",
        sql: include_str!("../../migrations/unified/100-unified-meta.sql"),
    },
    Migration {
        version: 101,
        name: "documents",
        sql: include_str!("../../migrations/unified/101-documents.sql"),
    },
    Migration {
        version: 102,
        name: "collections",
        sql: include_str!("../../migrations/unified/102-collections.sql"),
    },
    Migration {
        version: 103,
        name: "renamed",
        sql: include_str!("../../migrations/unified/103-renamed.sql"),
    },
    Migration {
        version: 104,
        name: "session-operations",
        sql: include_str!("../../migrations/unified/104-session-operations.sql"),
    },
];
