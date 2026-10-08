//! Auditoría de lenguajes de los archivos rastreados. Los errores fallan cerrado.
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lang {
    Python,
    Shell,
    JavaScript,
    TypeScript,
    Html,
    Other(String),
}
impl Lang {
    fn name(&self) -> String {
        match self {
            Self::Other(name) => format!("Other({name})"),
            other => format!("{other:?}"),
        }
    }
}

pub fn classify(path: &str, head: &[u8]) -> Option<Lang> {
    match Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "py" | "pyw" => return Some(Lang::Python),
        "sh" | "bash" | "zsh" => return Some(Lang::Shell),
        "js" | "mjs" | "cjs" | "jsx" => return Some(Lang::JavaScript),
        "ts" | "tsx" => return Some(Lang::TypeScript),
        "html" | "htm" => return Some(Lang::Html),
        "pl" => return Some(Lang::Other("Perl".into())),
        "rb" => return Some(Lang::Other("Ruby".into())),
        "lua" => return Some(Lang::Other("Lua".into())),
        "svg" => {
            return head
                .windows(7)
                .any(|s| s == b"<script")
                .then_some(Lang::JavaScript);
        }
        _ => {}
    }
    let first = head.split(|b| *b == b'\n').next()?;
    let shebang = first.strip_prefix(b"#!")?;
    for word in String::from_utf8_lossy(shebang).split_whitespace() {
        let executable = word.rsplit('/').next().unwrap_or(word);
        match executable {
            "sh" | "bash" | "zsh" => return Some(Lang::Shell),
            "node" | "deno" => return Some(Lang::JavaScript),
            "perl" => return Some(Lang::Other("Perl".into())),
            "ruby" => return Some(Lang::Other("Ruby".into())),
            name if name.starts_with("python") => return Some(Lang::Python),
            _ => {}
        }
    }
    None
}

pub struct Allowlist {
    entries: Vec<Value>,
}
impl Default for Allowlist {
    fn default() -> Self {
        // Datos del repositorio compilados; no se confía en una lista del repo auditado.
        Self::parse(include_str!("../lint-allowlist.json")).expect("lista blanca inválida")
    }
}
impl Allowlist {
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if value["version"] != 1 {
            return Err("versión de lista blanca no soportada".into());
        }
        let entries = value["allow"]
            .as_array()
            .ok_or("allow debe ser una lista")?
            .clone();
        for entry in &entries {
            let path = entry["path"].as_str();
            let prefix = entry["prefix"].as_str();
            if path.is_some() == prefix.is_some()
                || prefix.is_some_and(|s| !s.ends_with('/') || s == "/")
            {
                return Err("cada excepción requiere path o prefix terminado en /".into());
            }
            if entry["reason"].as_str().is_none_or(|s| s.is_empty()) {
                return Err("cada excepción requiere reason".into());
            }
            if !entry["max_lines"].is_null() && entry["max_lines"].as_u64().is_none() {
                return Err("max_lines debe ser entero no negativo".into());
            }
        }
        Ok(Self { entries })
    }
    fn allows(&self, path: &str, body: &[u8]) -> bool {
        self.entries.iter().any(|entry| {
            let matches = entry["path"].as_str() == Some(path)
                || entry["prefix"]
                    .as_str()
                    .is_some_and(|prefix| path.starts_with(prefix));
            matches
                && entry["max_lines"].as_u64().is_none_or(|limit| {
                    body.split(|b| *b == b'\n')
                        .filter(|line| line.iter().any(|b| !b.is_ascii_whitespace()))
                        .count() as u64
                        <= limit
                })
        })
    }
}

#[derive(Debug)]
pub struct Violation {
    pub path: String,
    pub lang: Lang,
}
#[derive(Debug, Default)]
pub struct Report {
    pub violations: Vec<Violation>,
    pub by_lang: BTreeMap<String, usize>,
    pub errors: Vec<String>,
}
impl Report {
    pub fn json(&self) -> Value {
        let mut value = json!({"total": self.violations.len(), "by_lang": self.by_lang,
            "violations": self.violations.iter().map(|v| json!({"path": v.path, "lang": v.lang.name()})).collect::<Vec<_>>()});
        if !self.errors.is_empty() {
            value["errors"] = json!(self.errors);
        }
        value
    }
}

pub fn scan(root: &Path, allow: &Allowlist) -> Report {
    let mut report = Report::default();
    let output = match Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z", "--cached"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            report
                .errors
                .push(String::from_utf8_lossy(&output.stderr).trim().into());
            return report;
        }
        Err(error) => {
            report.errors.push(error.to_string());
            return report;
        }
    };
    let mut seen = BTreeSet::new();
    for bytes in output.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let path = match std::str::from_utf8(bytes) {
            Ok(path) => path,
            Err(error) => {
                report.errors.push(format!("ruta no UTF-8: {error}"));
                continue;
            }
        };
        if !seen.insert(path) {
            continue;
        }
        let body = match fs::read(root.join(path)) {
            Ok(body) => body,
            Err(error) => {
                report.errors.push(format!("{path}: {error}"));
                continue;
            }
        };
        if let Some(lang) = classify(path, &body)
            && !allow.allows(path, &body)
        {
            *report.by_lang.entry(lang.name()).or_default() += 1;
            report.violations.push(Violation {
                path: path.into(),
                lang,
            });
        }
    }
    report
}

pub fn main(args: &[String]) -> i32 {
    let mut strict = false;
    let mut mode_seen = false;
    let mut root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--report" | "--strict" if !mode_seen => {
                strict = args[i] == "--strict";
                mode_seen = true;
            }
            "--root" if i + 1 < args.len() => {
                i += 1;
                root = args[i].clone().into();
            }
            _ => {
                eprintln!("uso: cargo xtask lint [--report | --strict] [--root DIR]");
                return 2;
            }
        }
        i += 1;
    }
    let report = scan(&root, &Allowlist::default());
    println!("{}", report.json());
    i32::from(!report.errors.is_empty() || (strict && !report.violations.is_empty()))
}
