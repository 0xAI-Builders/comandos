//! Inventario estático de referencias: no importa Python ni ejecuta pruebas.
use serde_json::{Value, json};
use std::{fs, path::Path};
fn collect(dir: &Path, rows: &mut Vec<Value>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    let mut paths = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    paths.sort();
    for path in paths {
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "golden") {
                continue;
            }
            collect(&path, rows)?;
        } else if path.extension().is_some_and(|e| e == "rs" || e == "py") {
            let source = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let references: Vec<_> = source
                .lines()
                .enumerate()
                .filter_map(|(i, line)| {
                    let text = line.trim();
                    (text.contains(".py")
                        || text.contains("python3")
                        || text.contains("run_python")
                        || text.contains("Command::new(\"bash\")")
                        || text.starts_with("from ")
                        || text.starts_with("import ")
                        || text.contains("support/python.rs")
                        || text.contains("comandos_oracle"))
                    .then(|| json!({"line":i+1,"reference":text}))
                })
                .collect();
            if !references.is_empty() {
                rows.push(json!({"path":path,"references":references}));
            }
        }
    }
    Ok(())
}
pub fn run(root: &Path) -> Result<(), String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    collect(&root.join("tests"), &mut rows)?;
    let mut crates = fs::read_dir(root.join("crates"))
        .map_err(|e| e.to_string())?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    crates.sort();
    for path in crates {
        collect(&path.join("tests"), &mut rows)?;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"kind":"declared-source-references","tests":rows}))
            .map_err(|e| e.to_string())?
    );
    Ok(())
}
