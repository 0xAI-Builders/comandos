//! Exact-thread release with durable recovery before archive and best-effort restoration.
use super::{Result, policy};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
pub trait Client {
    fn call(&mut self, method: &str, params: Value) -> Result<Value>;
}
pub fn sid(s: &str) -> bool {
    s.len() == 36
        && s.bytes()
            .all(|c| c.is_ascii_digit() || matches!(c, b'a'..=b'f' | b'-'))
}
pub(super) fn string<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("plan inválido: {k}"))
}
pub(super) fn resolve(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err("ruta de cuenta/transcript no absoluta".into());
    }
    let mut normalized = PathBuf::new();
    for p in path.components() {
        match p {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    if normalized.exists() {
        return fs::canonicalize(&normalized).map_err(|e| e.to_string());
    }
    let mut tail = vec![];
    let mut cursor = normalized.as_path();
    while !cursor.exists() {
        tail.push(
            cursor
                .file_name()
                .ok_or("ruta sin raíz existente")?
                .to_owned(),
        );
        cursor = cursor.parent().ok_or("ruta sin padre")?;
    }
    let mut root = fs::canonicalize(cursor).map_err(|e| e.to_string())?;
    for p in tail.into_iter().rev() {
        root.push(p);
    }
    Ok(root)
}
pub fn validate(plan: &Value) -> Result<()> {
    if !sid(string(plan, "sid")?) {
        return Err("La recuperación requiere el ID exacto de la conversación".into());
    }
    for k in ["home", "binary", "transcript"] {
        if !Path::new(string(plan, k)?).is_absolute() {
            return Err(format!("plan: {k} debe ser absoluto"));
        }
    }
    if plan["releaseRecovery"]["pending"]
        .as_bool()
        .unwrap_or(false)
    {
        ids(&plan["releaseRecovery"]["threadIds"], string(plan, "sid")?)?;
    }
    Ok(())
}
fn ids(value: &Value, parent: &str) -> Result<Vec<String>> {
    let rows = value
        .as_array()
        .ok_or("La recuperación guardada contiene IDs inválidos")?;
    let rows = rows
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| sid(s))
                .map(str::to_owned)
                .ok_or("La recuperación guardada contiene IDs inválidos".to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    if !rows.iter().any(|s| s == parent) {
        return Err("La recuperación guardada contiene IDs inválidos".into());
    }
    Ok(rows)
}
fn kinds() -> Value {
    json!([
        "cli",
        "vscode",
        "exec",
        "appServer",
        "subAgent",
        "subAgentReview",
        "subAgentCompact",
        "subAgentThreadSpawn",
        "subAgentOther",
        "unknown"
    ])
}
fn pages(c: &mut impl Client, method: &str, mut params: Value) -> Result<Vec<Value>> {
    let mut seen = HashSet::new();
    let mut out = vec![];
    loop {
        let response = c.call(method, params.clone())?;
        let data = response["data"]
            .as_array()
            .ok_or("Codex respondió una página inválida")?;
        out.extend_from_slice(data);
        if out.len() > 100_000 {
            return Err("demasiados resultados de mantenimiento".into());
        }
        let cursor = match &response["nextCursor"] {
            Value::Null => break,
            Value::String(s) if s.is_empty() => break,
            Value::String(s) => s.clone(),
            _ => return Err("cursor de mantenimiento inválido".into()),
        };
        if !seen.insert(cursor.clone()) {
            return Err("Codex repitió un cursor de mantenimiento".into());
        }
        params["cursor"] = json!(cursor);
    }
    Ok(out)
}
fn descendants(c: &mut impl Client, plan: &Value, archived: bool) -> Result<Vec<String>> {
    pages(c,"thread/list",json!({"ancestorThreadId":string(plan,"sid")?,"archived":archived,"sourceKinds":kinds(),"limit":100}))?.iter().map(|t|string(t,"id").map(str::to_owned)).collect()
}
fn commands(plan: &Value, rows: &[String]) -> Result<Vec<String>> {
    let home = string(plan, "home")?;
    let binary = string(plan, "binary")?;
    Ok(rows
        .iter()
        .map(|id| {
            [
                "env".to_string(),
                format!("CODEX_HOME={home}"),
                binary.to_string(),
                "unarchive".into(),
                id.clone(),
            ]
            .iter()
            .map(|s| policy::quote(s))
            .collect::<Vec<_>>()
            .join(" ")
        })
        .collect())
}
fn refresh(
    c: &mut impl Client,
    plan: &mut Value,
    checkpoint: &mut impl FnMut(&Value) -> Result<()>,
) -> Result<()> {
    if plan["releaseRecovery"].get("previouslyArchived").is_none() {
        return Ok(());
    }
    let archived = descendants(c, plan, true)?;
    let parent = string(plan, "sid")?.to_owned();
    let previous = plan["releaseRecovery"]["previouslyArchived"]
        .as_array()
        .ok_or("previouslyArchived inválido")?
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| sid(s))
                .map(str::to_owned)
                .ok_or("previouslyArchived inválido".into())
        })
        .collect::<Result<HashSet<_>>>()?;
    let mut children = ids(&plan["releaseRecovery"]["threadIds"], &parent)?
        .into_iter()
        .filter(|s| s != &parent)
        .collect::<Vec<_>>();
    for child in archived {
        if !previous.contains(&child) && !children.contains(&child) {
            children.push(child);
        }
    }
    if children.iter().any(|s| s == &parent || !sid(s)) {
        return Err("La recuperación encontró descendientes inválidos".into());
    }
    children.push(parent);
    plan["releaseRecovery"]["commands"] = json!(commands(plan, &children)?);
    plan["releaseRecovery"]["threadIds"] = json!(children);
    checkpoint(plan)
}
fn restore(c: &mut impl Client, plan: &mut Value, rows: &[String]) -> Result<()> {
    let parent = string(plan, "sid")?.to_owned();
    let home = PathBuf::from(string(plan, "home")?);
    let active = resolve(&home.join("sessions"))?;
    let archive = resolve(&home.join("archived_sessions"))?;
    let mut errors = vec![];
    for id in rows {
        let result: Result<()> = (|| {
            let current = c.call("thread/read", json!({"threadId":id,"includeTurns":false}))?;
            let current = &current["thread"];
            if string(current, "id")? != id {
                return Err("No se identificó el transcript que debe restaurarse".into());
            }
            let path = resolve(Path::new(string(current, "path")?))?;
            let restored = if path.starts_with(&archive) {
                c.call("thread/unarchive", json!({"threadId":id}))?["thread"].clone()
            } else if path.starts_with(&active) {
                current.clone()
            } else {
                return Err("El transcript salió de la cuenta que debe restaurarse".into());
            };
            if string(&restored, "id")? != id {
                return Err("La restauración devolvió otra conversación".into());
            }
            if !resolve(Path::new(string(&restored, "path")?))?.starts_with(&active) {
                return Err("Codex no devolvió el transcript a sus sesiones activas".into());
            }
            if id == &parent {
                plan["transcript"] = restored["path"].clone();
            }
            Ok(())
        })();
        if let Err(e) = result {
            errors.push(format!("{id}: {e}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "No se pudo restaurar todo; la recuperación está guardada: {}",
            errors.join("; ")
        ))
    }
}
pub fn release_with<C: Client>(
    plan: &mut Value,
    mut locked: impl FnMut() -> Result<bool>,
    open: impl FnOnce() -> Result<C>,
    mut checkpoint: impl FnMut(&Value) -> Result<()>,
) -> Result<String> {
    validate(plan)?;
    let parent = string(plan, "sid")?.to_owned();
    if !plan["releaseRecovery"]["pending"]
        .as_bool()
        .unwrap_or(false)
        && !locked()?
    {
        return Ok(string(plan, "transcript")?.to_owned());
    }
    let mut c = open()?;
    if plan["releaseRecovery"]["pending"]
        .as_bool()
        .unwrap_or(false)
    {
        refresh(&mut c, plan, &mut checkpoint)?;
        let rows = ids(&plan["releaseRecovery"]["threadIds"], &parent)?;
        restore(&mut c, plan, &rows)?;
        plan["releaseRecovery"]["pending"] = json!(false);
        checkpoint(plan)?;
        if !locked()? {
            return Ok(string(plan, "transcript")?.to_owned());
        }
    }
    let loaded = pages(&mut c, "thread/loaded/list", json!({"limit":100}))?;
    if !loaded.iter().any(|v| v.as_str() == Some(parent.as_str())) {
        return Err(
            "El bloqueo pertenece a otro proceso; no se modificó ninguna conversación".into(),
        );
    }
    let t = c.call(
        "thread/read",
        json!({"threadId":parent,"includeTurns":false}),
    )?;
    if string(&t["thread"], "id")? != parent
        || resolve(Path::new(string(&t["thread"], "path")?))?
            != resolve(Path::new(string(plan, "transcript")?))?
    {
        return Err("El servidor no posee el transcript exacto de la conversación".into());
    }
    let mut children = vec![];
    for child in descendants(&mut c, plan, false)? {
        if child == parent || !sid(&child) {
            return Err("Codex devolvió una lista de descendientes inválida".into());
        }
        if !children.contains(&child) {
            children.push(child);
        }
    }
    children.push(parent.clone());
    let previous = descendants(&mut c, plan, true)?;
    if previous.iter().any(|s| s == &parent || !sid(s)) {
        return Err("Codex devolvió historial archivado inválido".into());
    }
    plan["releaseRecovery"] = json!({"pending":true,"threadIds":children,"previouslyArchived":previous,"commands":commands(plan,&children)?});
    checkpoint(plan)?;
    let archive_error = c.call("thread/archive", json!({"threadId":parent})).err();
    let refresh_error = refresh(&mut c, plan, &mut checkpoint).err();
    let rows = ids(&plan["releaseRecovery"]["threadIds"], &parent)?;
    restore(&mut c, plan, &rows)?;
    plan["releaseRecovery"]["pending"] = json!(archive_error.is_some() || refresh_error.is_some());
    checkpoint(plan)?;
    if let Some(e) = refresh_error.or(archive_error) {
        return Err(e);
    }
    if locked()? {
        return Err(
            "La conversación todavía tiene un escritor activo; se conserva sin duplicarla".into(),
        );
    }
    Ok(string(plan, "transcript")?.to_owned())
}
pub fn writer_locked(home: &Path, parent: &str) -> Result<bool> {
    if !sid(parent) {
        return Err("ID exacto inválido".into());
    }
    let path = home
        .join("thread-writer-locks")
        .join(format!("{parent}.lock"));
    let m = match path.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    if !m.is_file() {
        return Err("writer lock no regular".into());
    }
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let f = fs::OpenOptions::new()
        .read(true)
        .custom_flags((nix::fcntl::OFlag::O_NONBLOCK | nix::fcntl::OFlag::O_NOCTTY).bits())
        .open(&path)
        .map_err(|e| e.to_string())?;
    let opened = f.metadata().map_err(|e| e.to_string())?;
    if !opened.is_file() || (m.dev(), m.ino()) != (opened.dev(), opened.ino()) {
        return Err("writer lock cambió".into());
    }
    match f.try_lock() {
        Ok(()) => {
            f.unlock().map_err(|e| e.to_string())?;
            Ok(false)
        }
        Err(fs::TryLockError::WouldBlock) => Ok(true),
        Err(fs::TryLockError::Error(e)) => Err(e.to_string()),
    }
}
