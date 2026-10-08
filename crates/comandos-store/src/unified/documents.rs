use super::Origin;
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub name: String,
    pub domain: String,
    pub body: Vec<u8>,
    pub revision: i64,
    pub updated_at_ms: i64,
}
pub fn doc_get(c: &Connection, name: &str) -> Result<Option<Document>> {
    Ok(c.query_row(
        "SELECT name,domain,body,revision,updated_at_ms FROM documents WHERE name=?1",
        [name],
        |r| {
            Ok(Document {
                name: r.get(0)?,
                domain: r.get(1)?,
                body: r.get(2)?,
                revision: r.get(3)?,
                updated_at_ms: r.get(4)?,
            })
        },
    )
    .optional()?)
}
fn valid(name: &str, domain: &str) -> Result<()> {
    if domain.is_empty()
        || name.starts_with('/')
        || name.contains('\0')
        || name.split('/').any(|s| matches!(s, "" | "." | ".."))
    {
        return Err(Error::Validation("clave de documento inválida".into()));
    }
    Ok(())
}
pub fn doc_put(
    c: &Connection,
    name: &str,
    domain: &str,
    body: &[u8],
    origin: Origin,
    now_ms: i64,
) -> Result<i64> {
    valid(name, domain)?;
    let revision=c.query_row("INSERT INTO documents(name,domain,body,revision,updated_at_ms,origin) VALUES (?1,?2,?3,1,?4,?5) ON CONFLICT(name) DO UPDATE SET body=excluded.body,revision=documents.revision+1,updated_at_ms=excluded.updated_at_ms,origin=excluded.origin WHERE documents.domain=excluded.domain AND documents.revision<9223372036854775807 RETURNING revision",params![name,domain,body,now_ms,origin.as_str()],|r|r.get(0)).optional()?;
    revision.ok_or_else(|| Error::Validation("dominio distinto o revisión agotada".into()))
}
pub fn doc_put_if_newer(
    c: &Connection,
    name: &str,
    domain: &str,
    body: &[u8],
    mtime_ms: i64,
) -> Result<bool> {
    valid(name, domain)?;
    let count=c.execute("INSERT INTO documents(name,domain,body,revision,updated_at_ms,origin) VALUES (?1,?2,?3,1,?4,'import') ON CONFLICT(name) DO UPDATE SET body=excluded.body,revision=documents.revision+1,updated_at_ms=excluded.updated_at_ms,origin='import' WHERE documents.domain=excluded.domain AND documents.updated_at_ms<excluded.updated_at_ms AND documents.revision<9223372036854775807",params![name,domain,body,mtime_ms])?;
    Ok(count != 0)
}
