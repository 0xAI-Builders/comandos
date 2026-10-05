//! Deriva de `bin/cc-app`: hash por función (incluidas las anidadas) para saber qué
//! cambió en el Python desde que se portó cada rebanada.
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Def {
    pub qualname: String,
    pub hash: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Drift {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

impl Drift {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Nombre tras `def `/`class ` si la línea abre una definición.
fn opener(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let rest = t
        .strip_prefix("def ")
        .or_else(|| t.strip_prefix("async def "))
        .or_else(|| t.strip_prefix("class "))?;
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    rest.get(..end)
}

fn is_code(line: &str) -> bool {
    !line.trim().is_empty() && !line.trim_start().starts_with('#')
}

/// Cuerpo normalizado: sin líneas vacías ni comentarios de línea completa.
fn normalized(lines: &[&str]) -> String {
    lines
        .iter()
        .filter(|l| is_code(l))
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn defs(src: &str) -> Vec<Def> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(name) = opener(line) else { continue };
        let ind = indent_of(line);
        while stack.last().is_some_and(|(d, _)| *d >= ind) {
            stack.pop();
        }
        let qual = stack
            .iter()
            .map(|(_, n)| n.as_str())
            .chain([name])
            .collect::<Vec<_>>()
            .join(".");
        // El cuerpo acaba en la primera línea con código a sangría <= la de la definición.
        let end = lines
            .iter()
            .enumerate()
            .skip(i + 1)
            .find(|(_, l)| is_code(l) && indent_of(l) <= ind)
            .map_or(lines.len(), |(j, _)| j);
        let body = lines.get(i..end).unwrap_or_default();
        let digest = Sha256::digest(normalized(body).as_bytes());
        let hash: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
        // Mismo nombre cualificado repetido (redefiniciones, setters): la n-ésima lleva `#n`.
        let n = seen.entry(qual.clone()).or_insert(0);
        *n += 1;
        let qualname = if *n == 1 { qual } else { format!("{qual}#{n}") };
        out.push(Def { qualname, hash });
        stack.push((ind, name.to_string()));
    }
    out
}

pub fn diff(base: &[Def], now: &[Def]) -> Drift {
    let find = |set: &[Def], q: &str| set.iter().find(|d| d.qualname == q).map(|d| d.hash.clone());
    let mut d = Drift::default();
    for n in now {
        match find(base, &n.qualname) {
            None => d.added.push(n.qualname.clone()),
            Some(h) if h != n.hash => d.changed.push(n.qualname.clone()),
            Some(_) => {}
        }
    }
    for b in base {
        if find(now, &b.qualname).is_none() {
            d.removed.push(b.qualname.clone());
        }
    }
    d
}

/// Línea base tras aceptar `accept`: el resto de definiciones conserva su estado anterior.
fn accepted(base: &[Def], now: &[Def], accept: &[String]) -> Vec<Def> {
    let mut out: Vec<Def> = Vec::new();
    for b in base {
        match now.iter().find(|n| n.qualname == b.qualname) {
            Some(n) if accept.contains(&b.qualname) => out.push(n.clone()),
            None if accept.contains(&b.qualname) => {} // aceptar un borrado
            _ => out.push(b.clone()),
        }
    }
    for n in now {
        if accept.contains(&n.qualname) && !base.iter().any(|b| b.qualname == n.qualname) {
            out.push(n.clone());
        }
    }
    out
}

fn to_json(file: &str, defs: &[Def]) -> Value {
    let mut m = Map::new();
    for d in defs {
        m.insert(d.qualname.clone(), Value::String(d.hash.clone()));
    }
    json!({ "file": file, "defs": m })
}

fn from_json(v: &Value) -> Vec<Def> {
    v.get("defs")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, h)| {
                    Some(Def {
                        qualname: k.clone(),
                        hash: h.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `paths`: `[cc-app, cc-notifyd?]`. Devuelve el código de salida (1 si hay deriva).
pub fn run(
    baseline: &Path,
    paths: &[String],
    write: bool,
    accept: &[String],
) -> Result<i32, String> {
    let previous: Value = fs::read_to_string(baseline)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    // Los archivos que no se pasan ahora conservan su entrada.
    let mut files: Map<String, Value> = previous
        .get("files")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut drifted = false;
    for path in paths {
        let src = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let key = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archivo")
            .to_string();
        let now = defs(&src);
        let base = previous
            .get("files")
            .and_then(|f| f.get(&key))
            .map(from_json)
            .unwrap_or_default();
        let d = diff(&base, &now);
        if !write {
            for (label, list) in [
                ("added", &d.added),
                ("removed", &d.removed),
                ("changed", &d.changed),
            ] {
                for q in list {
                    println!("{key} {label} {q}");
                }
            }
            drifted |= !d.is_empty();
        }
        if write {
            files.insert(key, to_json(path, &now));
        } else if !accept.is_empty() {
            files.insert(key, to_json(path, &accepted(&base, &now, accept)));
        }
    }
    if write || !accept.is_empty() {
        let text =
            serde_json::to_string_pretty(&json!({ "files": files })).map_err(|e| e.to_string())?;
        fs::write(baseline, text + "\n").map_err(|e| format!("{}: {e}", baseline.display()))?;
    }
    Ok(i32::from(drifted && !write))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "import os\n\ndef a(x):\n    return x\n\n\nclass K:\n    def m(self):\n        # comentario\n        return 1\n\n    def n(self):\n        pass\n\ndef b():\n    def inner():\n        pass\n    return inner\n";

    #[test]
    fn qualified_names_include_nested() {
        let names: Vec<String> = defs(SRC).into_iter().map(|d| d.qualname).collect();
        assert_eq!(names, ["a", "K", "K.m", "K.n", "b", "b.inner"]);
    }

    #[test]
    fn body_change_changes_only_that_def() {
        let base = defs(SRC);
        let now = defs(&SRC.replace("return 1", "return 2"));
        let d = diff(&base, &now);
        assert_eq!(d.changed, ["K", "K.m"]);
        assert!(d.added.is_empty() && d.removed.is_empty());
    }

    #[test]
    fn comment_only_change_is_ignored() {
        let d = diff(&defs(SRC), &defs(&SRC.replace("# comentario", "# otro")));
        assert!(d.changed.is_empty());
    }

    #[test]
    fn repeated_qualname_gets_suffix() {
        let src = "def f():\n    return 1\n\ndef f():\n    return 2\n";
        let names: Vec<String> = defs(src).into_iter().map(|d| d.qualname).collect();
        assert_eq!(names, ["f", "f#2"]);
    }

    #[test]
    fn added_and_removed_are_reported() {
        let d = diff(
            &defs(SRC),
            &defs(&SRC.replace("def n(self)", "def z(self)")),
        );
        assert_eq!(d.added, ["K.z"]);
        assert_eq!(d.removed, ["K.n"]);
    }

    #[test]
    fn accept_only_touches_named_defs() {
        let base = defs(SRC);
        let edited = SRC
            .replace("return 1", "return 2")
            .replace("return x", "return y");
        let now = defs(&edited);
        let out = accepted(&base, &now, &["K.m".to_string()]);
        let d = diff(&out, &now);
        assert_eq!(d.changed, ["a", "K"]);
        assert!(out.iter().any(|x| x.qualname == "K.m" && now.contains(x)));
    }

    #[test]
    fn run_baseline_roundtrip_in_tempdir() {
        let dir = std::env::temp_dir().join(format!("app-drift-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("cc-app");
        let base = dir.join("base.json");
        fs::write(&src, SRC).unwrap();
        let p = [src.to_string_lossy().into_owned()];
        assert_eq!(run(&base, &p, true, &[]).unwrap(), 0);
        assert_eq!(run(&base, &p, false, &[]).unwrap(), 0);
        fs::write(&src, SRC.replace("return 1", "return 2")).unwrap();
        assert_eq!(run(&base, &p, false, &[]).unwrap(), 1);
        let acc = ["K".to_string(), "K.m".to_string()];
        assert_eq!(run(&base, &p, false, &acc).unwrap(), 1);
        assert_eq!(run(&base, &p, false, &[]).unwrap(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }
}
