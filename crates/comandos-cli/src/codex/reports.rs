//! Codex reports use catalogued Document authority in every mode.
use super::{Result, release};
use comandos_store::{
    domains::{DocumentDir, DomainStore},
    unified::Mode,
};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
pub struct Reports<'a> {
    documents: DocumentDir<'a>,
}
impl<'a> Reports<'a> {
    pub fn new(home: &'a Path, directory: PathBuf) -> Result<Self> {
        Ok(Self {
            documents: DomainStore { home }
                .document_dir("codex-reports", "STATE/codex-full-access", directory)
                .map_err(|e| e.to_string())?,
        })
    }
    pub fn directory(&self) -> &Path {
        self.documents.directory()
    }
    pub fn mode(&self) -> Result<Mode> {
        self.documents.mode_readonly().map_err(|e| e.to_string())
    }
    pub fn read_path(&self, path: &Path) -> Result<Value> {
        if !path.is_absolute()
            || path.parent() != Some(self.directory())
            || path
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
        {
            return Err(
                "--retry-report requiere ruta absoluta dentro de la colección de informes".into(),
            );
        }
        let key = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("basename de informe inválido")?;
        let body = self
            .documents
            .read_readonly(key)
            .map_err(|e| e.to_string())?
            .ok_or("No existe el informe en la autoridad actual")?;
        parse(&body)
    }
    pub fn latest(&self) -> Result<(PathBuf, Value)> {
        // SQL timestamps are milliseconds. The numeric original filename supplies
        // a deterministic tie without consulting legacy files in Unified/Sealed.
        let row = self
            .documents
            .list_readonly()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|row| !matches!(parse(&row.body),Ok(v) if v["operation"]=="thread-release"))
            .max_by(|a, b| {
                a.modified_ns
                    .cmp(&b.modified_ns)
                    .then_with(|| numeric(&a.key).cmp(&numeric(&b.key)))
                    .then_with(|| a.key.cmp(&b.key))
            })
            .ok_or("No hay un informe anterior que reintentar")?;
        Ok((self.directory().join(row.key), parse(&row.body)?))
    }
    pub fn save(&self, key: &str, payload: &Value, now_ms: i64) -> Result<()> {
        validate(payload)?;
        let body = format!(
            "{}\n",
            comandos_core::json::indent_dumps(payload, 2, false)?
        )
        .into_bytes();
        if body.len() > 16_000_000 {
            return Err("informe excede límite".into());
        }
        if self.mode()? != Mode::Sealed {
            use std::os::unix::fs::MetadataExt;
            match self.directory().symlink_metadata() {
                Ok(m)
                    if m.is_dir()
                        && m.mode() & 0o777 == 0o700
                        && m.uid() == nix::unistd::geteuid().as_raw() => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => {
                    return Err(
                        "colección de informes debe ser directorio propio 0700 sin symlink".into(),
                    );
                }
            }
            // Validate an existing key and its filesystem controls without writing.
            self.documents
                .read_readonly(key)
                .map_err(|e| e.to_string())?;
        }
        self.documents
            .with_document(key, |doc| doc.update_owned(now_ms, |_| Ok(Some(body))))
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
fn numeric(key: &str) -> Option<u128> {
    key.strip_suffix(".json").and_then(|s| s.parse().ok())
}
fn parse(body: &[u8]) -> Result<Value> {
    if body.len() > 16_000_000 {
        return Err("informe excede límite".into());
    }
    let value = comandos_core::json::parse_unique_value(
        std::str::from_utf8(body).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    validate(&value)?;
    Ok(value)
}
pub fn validate(v: &Value) -> Result<()> {
    let plans = v["plans"]
        .as_array()
        .ok_or("El informe anterior no contiene planes y resultados válidos")?;
    let results = v["results"]
        .as_array()
        .ok_or("El informe anterior no contiene planes y resultados válidos")?;
    if v["operation"] == "thread-release" {
        if plans.len() != 1 || results.len() > 1 {
            return Err("informe de liberación inválido".into());
        }
        release::validate(&plans[0])?;
        for result in results {
            if result["sid"] != plans[0]["sid"]
                || !matches!(result["status"].as_str(), Some("released" | "failed"))
            {
                return Err("resultado de liberación inválido".into());
            }
        }
        return Ok(());
    }
    if v.get("operation").is_some_and(|k| k != "full-access") {
        return Err("operación de informe desconocida".into());
    }
    let mut panes = HashSet::new();
    for p in plans {
        let pane = release::string(p, "pane")?;
        if !panes.insert(pane) {
            return Err("El informe repite un pane".into());
        }
        release::validate(p)?;
        for k in ["session", "paneStart", "cwd"] {
            release::string(p, k)?;
        }
        if p["panePid"].as_i64().is_none_or(|p| p <= 1)
            || !p["flags"]
                .as_array()
                .is_some_and(|v| v.iter().all(Value::is_string))
        {
            return Err("identidad/flags del informe inválidos".into());
        }
    }
    let mut seen = HashSet::new();
    for r in results {
        let pane = release::string(r, "pane")?;
        if !panes.contains(pane)
            || !seen.insert(pane)
            || !matches!(
                r["status"].as_str(),
                Some("confirmed" | "restricted" | "unverified" | "failed")
            )
        {
            return Err("resultado de informe inválido".into());
        }
        if r["status"] == "confirmed" {
            let plan = plans
                .iter()
                .find(|p| p["pane"] == pane)
                .ok_or("plan confirmado ausente")?;
            if r["conversationId"] != plan["sid"] || r["prompt"] != "continua" {
                return Err("resultado confirmado no identifica el nuevo turno exacto".into());
            }
        }
    }
    Ok(())
}
