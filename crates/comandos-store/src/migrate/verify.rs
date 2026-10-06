use super::{
    import,
    sources::{self, Source},
};
use crate::{
    Error, Result,
    domains::catalog::{TargetKind, catalog, domain},
    unified,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};
pub struct VerifyReport {
    pub domain: String,
    pub mismatches: Vec<String>,
}
impl VerifyReport {
    pub fn json(&self) -> Value {
        json!({"domain":self.domain,"mismatches":self.mismatches})
    }
}
pub fn verify(home: &Path, db: &Connection, name: &str) -> Result<VerifyReport> {
    if domain(name).is_none() || name.starts_with("db-") {
        return Err(Error::Validation(
            "verify S3 requiere un dominio de archivo".into(),
        ));
    }
    let (_, _guard) = unified::modes::access_mode(home, Some(db), name)?;
    verify_locked(home, db, name)
}

pub(super) fn verify_locked(home: &Path, db: &Connection, name: &str) -> Result<VerifyReport> {
    let path = unified::modes::connection_path(db)
        .ok_or_else(|| Error::Validation("verify requiere base en disco".into()))?;
    let specs: Vec<_> = catalog()
        .iter()
        .filter(|s| s.domain == name)
        .copied()
        .collect();
    let sources = sources::collect(home, &path, &specs)?;
    let mut report = VerifyReport {
        domain: name.into(),
        mismatches: vec![],
    };
    let mut expected = BTreeSet::new();
    let mut collection_keys = BTreeSet::new();
    for source in sources {
        let _lock = sources::lock(&source)?;
        let snapshot = sources::read_source(home, &source)?;
        let key = sources::doc_name(&source.symbolic);
        expected.insert(key.clone());
        match source.spec.kind {
            TargetKind::LayoutSnapshot => {
                let (generation, stamp) = import::layout_key(&source, &snapshot)?;
                collection_keys.insert(format!("layout:{generation}:{stamp}"));
            }
            TargetKind::AppCommand => {
                collection_keys.insert(format!(
                    "command:{}",
                    import::command_kind(&source.symbolic)?
                ));
            }
            TargetKind::LogLines => {
                collection_keys.insert(format!("log:{}", import::log_name(&source.symbolic)?));
            }
            _ => {}
        }
        let equal = compare(db, &source, &snapshot)?;
        if !equal {
            report.mismatches.push(key);
        }
    }
    // Una fila sin archivo también es una diferencia; las colecciones se contrastan por clave.
    let query = match name {
        "session-status" => Some("SELECT 'hooks/state/'||file_key FROM session_status"),
        "processes" => Some("SELECT 'hooks/native-processes/'||pid||'.json' FROM native_processes"),
        _ if specs.iter().any(|s| s.kind == TargetKind::Document) => {
            Some("SELECT name FROM documents WHERE domain=?1")
        }
        _ => None,
    };
    if let Some(query) = query {
        let mut statement = db.prepare(query)?;
        let mut rows = if query.contains("?1") {
            statement.query([name])?
        } else {
            statement.query([])?
        };
        while let Some(row) = rows.next()? {
            let key: String = row.get(0)?;
            if !expected.contains(&key) {
                report.mismatches.push(key);
            }
        }
    }
    let query = match name {
        "layout" => Some("SELECT 'layout:'||generation||':'||stamp FROM layout_snapshots"),
        "logs" => Some("SELECT DISTINCT 'log:'||log FROM log_lines"),
        "app-commands" => Some("SELECT DISTINCT 'command:'||kind FROM app_commands"),
        _ => None,
    };
    if let Some(query) = query {
        for row in db
            .prepare(query)?
            .query_map([], |r| r.get::<_, String>(0))?
        {
            let key = row?;
            if !collection_keys.contains(&key) {
                report.mismatches.push(key);
            }
        }
    }
    report.mismatches.sort();
    report.mismatches.dedup();
    Ok(report)
}
fn compare(c: &Connection, s: &Source, snapshot: &sources::Snapshot) -> Result<bool> {
    let bytes = &snapshot.body;
    let row: Option<Vec<u8>> = match s.spec.kind {
        TargetKind::Document => {
            unified::doc_get(c, &sources::doc_name(&s.symbolic))?.map(|d| d.body)
        }
        TargetKind::SessionStatus => c
            .query_row(
                "SELECT body FROM session_status WHERE file_key=?1",
                [s.symbolic.strip_prefix("H/state/").unwrap_or_default()],
                |r| r.get(0),
            )
            .optional()?,
        TargetKind::NativeProcess => {
            let pid = s
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| Error::Validation("pid inválido".into()))?
                .parse::<i64>()
                .map_err(|e| Error::Validation(e.to_string()))?;
            c.query_row(
                "SELECT body FROM native_processes WHERE pid=?1",
                [pid],
                |r| r.get(0),
            )
            .optional()?
        }
        TargetKind::LayoutSnapshot => {
            let (generation, stamp) = import::layout_key(s, snapshot)?;
            if generation == "minute" {
                let retained:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM migration_steps WHERE domain='layout' AND source=?1 AND detail LIKE '%retención%' AND status='done')",[&s.symbolic],|r|r.get(0))?;
                if retained {
                    return Ok(true);
                }
            }
            c.query_row(
                "SELECT body FROM layout_snapshots WHERE generation=?1 AND stamp=?2",
                params![generation, stamp],
                |r| r.get(0),
            )
            .optional()?
        }
        TargetKind::AppCommand => c
            .query_row(
                "SELECT body FROM app_commands WHERE kind=?1 ORDER BY seq DESC LIMIT 1",
                [import::command_kind(&s.symbolic)?],
                |r| r.get(0),
            )
            .optional()?,
        TargetKind::LogLines => {
            let (lines, incomplete) = import::complete_lines(bytes);
            if incomplete {
                return Ok(false);
            }
            let stored: Vec<Vec<u8>> = c
                .prepare("SELECT line FROM log_lines WHERE log=?1 ORDER BY seq")?
                .query_map([import::log_name(&s.symbolic)?], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            return Ok(stored.iter().map(Vec::as_slice).eq(lines));
        }
        TargetKind::Sqlite => {
            return Err(Error::Validation("verify SQLite pendiente de S4".into()));
        }
    };
    Ok(row.as_deref() == Some(bytes.as_slice()))
}
