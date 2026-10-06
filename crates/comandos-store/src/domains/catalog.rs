//! Raíces simbólicas: H=hooks, STATE=estado local, SHARE=datos locales.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourcePattern {
    File(&'static str),
    Dir {
        dir: &'static str,
        suffix: &'static str,
    },
    Sqlite(&'static str),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Document,
    LayoutSnapshot,
    AppCommand,
    SessionStatus,
    NativeProcess,
    LogLines,
    Sqlite,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpec {
    pub domain: &'static str,
    pub pattern: SourcePattern,
    pub kind: TargetKind,
}
impl SourceSpec {
    pub fn matches(&self, path: &str) -> bool {
        match self.pattern {
            SourcePattern::File(file) => path == file,
            SourcePattern::Sqlite(file) => {
                path == file
                    || path
                        .strip_prefix(file)
                        .is_some_and(|tail| matches!(tail, "-wal" | "-shm"))
            }
            SourcePattern::Dir { dir, suffix } => path
                .strip_prefix(dir)
                .is_some_and(|tail| tail.starts_with('/') && tail.ends_with(suffix)),
        }
    }
}
macro_rules! file {
    ($domain:literal, $path:literal, $kind:ident) => {
        SourceSpec {
            domain: $domain,
            pattern: SourcePattern::File($path),
            kind: TargetKind::$kind,
        }
    };
}
macro_rules! db {
    ($domain:literal, $path:literal) => {
        SourceSpec {
            domain: $domain,
            pattern: SourcePattern::Sqlite($path),
            kind: TargetKind::Sqlite,
        }
    };
}
static CATALOG: &[SourceSpec] = &[
    file!("tabs", "H/app-tabs.json", Document),
    file!("tabs", "H/app-tabs-meta.json", Document),
    file!("tabs", "H/app-tabs-history.json", Document),
    file!("tabs", "H/app-tab-models.json", Document),
    file!("tabs", "H/app-tab-active.json", Document),
    file!("tabs", "H/app-tabs-snapshot.json", Document),
    file!("layout", "H/app-sessions-v2.json", LayoutSnapshot),
    file!("layout", "H/app-sessions-v2.json.bak", LayoutSnapshot),
    SourceSpec {
        domain: "layout",
        pattern: SourcePattern::Dir {
            dir: "H/app-sessions-v2.json.history",
            suffix: ".json",
        },
        kind: TargetKind::LayoutSnapshot,
    },
    file!("app-ui", "H/app-layout.json", Document),
    file!("app-ui", "H/app-pane-position.json", Document),
    file!("app-ui", "H/app-extension-shelf.json", Document),
    file!("app-ui", "H/notifyd-pos.json", Document),
    file!("app-commands", "H/app-focus.json", AppCommand),
    file!("app-commands", "H/app-tab-open.json", AppCommand),
    file!("app-commands", "H/app-tab-close.json", AppCommand),
    file!("app-commands", "H/app-command.json", AppCommand),
    file!("app-commands", "H/app-tab-back.json", AppCommand),
    SourceSpec {
        domain: "session-status",
        pattern: SourcePattern::Dir {
            dir: "H/state",
            suffix: ".json",
        },
        kind: TargetKind::SessionStatus,
    },
    SourceSpec {
        domain: "processes",
        pattern: SourcePattern::Dir {
            dir: "H/native-processes",
            suffix: ".json",
        },
        kind: TargetKind::NativeProcess,
    },
    file!("logs", "H/events.jsonl", LogLines),
    file!("logs", "H/ui-events.jsonl", LogLines),
    file!("logs", "H/focus-queue.jsonl", LogLines),
    file!("ui-docs", "H/snippets.json", Document),
    file!("ui-docs", "H/prefs.json", Document),
    file!("ui-docs", "H/model-watch.json", Document),
    file!("ui-docs", "H/motor-queue.json", Document),
    file!("ui-docs", "H/motor-results.json", Document),
    file!("ui-docs", "H/pane-resume.json", Document),
    file!("ui-docs", "H/session-effort.json", Document),
    file!("ui-docs", "H/proxy-env-since.json", Document),
    file!("ui-docs", "H/optimization-default.json", Document),
    file!("ui-docs", "H/acp-panes.json", Document),
    file!("ui-docs", "H/webterm-enabled", Document),
    file!("quota-docs", "H/provider-quotas.json", Document),
    file!("quota-docs", "H/provider-subs.json", Document),
    file!("quota-docs", "H/groq-ratelimit.json", Document),
    file!("quota-docs", "H/agy-quota.json", Document),
    file!("quota-docs", "H/pane-models.txt", Document),
    file!("news-docs", "H/news-editions.json", Document),
    file!("news-docs", "H/news-watch.json", Document),
    file!("news-docs", "H/operator/conversations.json", Document),
    file!("extensions", "STATE/extensions/sizes", Document),
    file!("extensions", "STATE/extensions/snapshot.json", Document),
    file!("extensions", "STATE/extensions/skills.json", Document),
    file!(
        "extensions",
        "STATE/extensions/client-policies.json",
        Document
    ),
    file!("extensions", "STATE/extensions/last-check.json", Document),
    db!("db-operator", "H/operator/actions.sqlite"),
    db!("db-news", "H/news-history.sqlite"),
    db!("db-operations", "H/session-operations.sqlite3"),
    db!("db-app-state", "STATE/app-state.sqlite3"),
    db!("db-usage", "H/comandos-usage.sqlite"),
];
pub fn catalog() -> &'static [SourceSpec] {
    CATALOG
}
pub fn source(path: &str) -> Option<&'static SourceSpec> {
    CATALOG.iter().find(|s| s.matches(path))
}

/// Solo restos observables y excepciones D3/D9; lo desconocido queda visible.
pub fn file_classification(path: &str) -> &'static str {
    let leaf = path.rsplit('/').next().unwrap_or(path);
    if matches!(
        path,
        "H/providers.env" | "H/cc-notify.conf" | "H/dash-token" | "H/terminal-replies.conf"
    ) || leaf.ends_with(".lock")
        || [
            "H/session-handoffs/",
            "H/extension-launches/",
            "STATE/extensions/backups/",
            "SHARE/bin/",
            "SHARE/releases/",
            "SHARE/rollback/",
            "SHARE/localstorage/",
            "SHARE/storage/",
            "SHARE/mediakeys/",
            "SHARE/serviceworkers/",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix))
        || path == "SHARE/hsts-storage.sqlite"
        || path == "SHARE/hsts-storage.sqlite-wal"
        || path == "SHARE/hsts-storage.sqlite-shm"
    {
        return "se-queda-como-archivo";
    }
    if matches!(
        path,
        "H/telegram.env"
            | "H/telegram.env.example"
            | "H/md2tg.py"
            | "H/notify.sh"
            | "H/usage.db"
            | "H/usage.db-wal"
            | "H/usage.db-shm"
    ) || leaf.contains(".bak")
        || leaf.ends_with(".tmp")
        || leaf.contains(".pre-")
        || ["H/backup-", "H/backups-", "H/dash/", "STATE/backup-"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
    {
        return "resto";
    }
    "sin-dominio"
}
