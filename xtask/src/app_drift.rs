//! Deriva de `bin/cc-app`: hash por función (incluidas las anidadas) para saber qué
//! cambió en el Python desde que se portó cada rebanada.
//!
//! El análisis no usa un parser de Python sino un escáner de líneas que sabe lo justo:
//! cadenas simples, dobles y triples (con prefijos y escapes, y campos `{…}` de f-strings),
//! comentarios fuera de cadenas, paréntesis abiertos y barras de continuación. Con eso
//! el final de cada cuerpo se decide solo por líneas de código reales.
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, collections::HashMap, fs, path::Path};

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

/// Una cadena abierta. `braces > 0` en una f-string: estamos dentro de un campo `{…}`,
/// donde rige la sintaxis de código (puede abrir cadenas anidadas).
struct Ctx {
    quote: u8,
    triple: bool,
    fstring: bool,
    braces: u32,
}

/// Lo que el escáner sabe de una línea física.
struct Line<'a> {
    text: &'a str,
    /// La línea empieza dentro de una cadena (continuación de una cadena abierta antes).
    in_string_at_start: bool,
    /// La línea acaba dentro de una cadena que sigue abierta.
    in_string_at_end: bool,
    /// Posición del `#` que abre un comentario fuera de cadenas.
    comment_at: Option<usize>,
    /// Primera línea de una sentencia: no es continuación de cadena, de paréntesis ni de `\`.
    starts_statement: bool,
}

impl Line<'_> {
    fn indent(&self) -> usize {
        self.text.len() - self.text.trim_start().len()
    }

    fn trimmed(&self) -> &str {
        self.text.trim_start()
    }

    /// Línea de código: empieza una sentencia y no está vacía ni es solo un comentario.
    fn is_code(&self) -> bool {
        self.starts_statement && !self.trimmed().is_empty() && !self.trimmed().starts_with('#')
    }

    /// Aporta al hash: nunca se descarta una línea que está dentro de una cadena.
    fn hashed(&self) -> Option<String> {
        if self.in_string_at_start {
            // Contenido de una cadena: verbatim salvo que el comentario final sea real.
            return Some(self.code_text());
        }
        let t = self.trimmed();
        if t.is_empty() || t.starts_with('#') {
            return None;
        }
        Some(self.code_text())
    }

    /// Texto sin el comentario final; si la línea acaba dentro de una cadena se conserva entera.
    fn code_text(&self) -> String {
        if self.in_string_at_end {
            return self.text.to_string();
        }
        let end = self.comment_at.unwrap_or(self.text.len());
        self.text
            .get(..end)
            .unwrap_or(self.text)
            .trim_end()
            .to_string()
    }
}

/// ¿Las letras justo antes de la comilla en `i` son un prefijo de cadena con `f`/`F`?
fn is_fstring_prefix(b: &[u8], i: usize) -> bool {
    let start = b
        .get(..i)
        .unwrap_or_default()
        .iter()
        .rposition(|c| !(c.is_ascii_alphanumeric() || *c == b'_'))
        .map_or(0, |p| p + 1);
    let prefix = b.get(start..i).unwrap_or_default();
    prefix.len() <= 2
        && !prefix.is_empty()
        && prefix.iter().all(|c| b"rRbBfFuU".contains(c))
        && prefix.iter().any(|c| matches!(c, b'f' | b'F'))
}

#[derive(Default)]
struct Scanner {
    stack: Vec<Ctx>,
    depth: usize,
    continued: bool,
}

impl Scanner {
    fn open_string(&mut self, b: &[u8], i: usize) -> usize {
        let quote = b[i];
        let triple = b.get(i + 1) == Some(&quote) && b.get(i + 2) == Some(&quote);
        self.stack.push(Ctx {
            quote,
            triple,
            fstring: is_fstring_prefix(b, i),
            braces: 0,
        });
        if triple { 3 } else { 1 }
    }

    fn scan<'a>(&mut self, text: &'a str) -> Line<'a> {
        let b = text.as_bytes();
        let in_string_at_start = !self.stack.is_empty();
        let starts_statement = !in_string_at_start && self.depth == 0 && !self.continued;
        self.continued = false;
        let mut comment_at = None;
        let mut escaped_eol = false;
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            let next = b.get(i + 1).copied();
            let Some(top) = self.stack.last_mut() else {
                // Código.
                match c {
                    b'#' => {
                        comment_at = Some(i);
                        break;
                    }
                    b'\'' | b'"' => {
                        i += self.open_string(b, i);
                        continue;
                    }
                    b'(' | b'[' | b'{' => self.depth += 1,
                    b')' | b']' | b'}' => self.depth = self.depth.saturating_sub(1),
                    b'\\' if i + 1 == b.len() => self.continued = true,
                    _ => {}
                }
                i += 1;
                continue;
            };
            if top.fstring && top.braces > 0 {
                // Campo de f-string: código con posibles cadenas anidadas.
                match c {
                    b'\'' | b'"' => {
                        i += self.open_string(b, i);
                        continue;
                    }
                    b'{' => top.braces += 1,
                    b'}' => top.braces -= 1,
                    _ => {}
                }
                i += 1;
                continue;
            }
            // Texto literal de la cadena.
            if c == b'\\' {
                if i + 1 == b.len() {
                    escaped_eol = true;
                }
                i += 2;
                continue;
            }
            if top.fstring && c == b'{' {
                if next == Some(b'{') {
                    i += 2;
                } else {
                    top.braces = 1;
                    i += 1;
                }
                continue;
            }
            if top.fstring && c == b'}' && next == Some(b'}') {
                i += 2;
                continue;
            }
            if c == top.quote {
                if !top.triple {
                    self.stack.pop();
                    i += 1;
                } else if next == Some(top.quote) && b.get(i + 2) == Some(&top.quote) {
                    self.stack.pop();
                    i += 3;
                } else {
                    i += 1;
                }
                continue;
            }
            i += 1;
        }
        // Una cadena simple sin cerrar no cruza líneas, salvo con `\` final.
        if !escaped_eol {
            while self.stack.last().is_some_and(|c| !c.triple) {
                self.stack.pop();
            }
        }
        Line {
            text,
            in_string_at_start,
            in_string_at_end: !self.stack.is_empty(),
            comment_at,
            starts_statement,
        }
    }
}

/// Nombre tras `def `/`async def `/`class ` si la sentencia abre una definición.
fn opener(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix("def ")
        .or_else(|| line.strip_prefix("async def "))
        .or_else(|| line.strip_prefix("class "))?
        .trim_start();
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    rest.get(..end).filter(|n| !n.is_empty())
}

fn body_hash(lines: &[Line]) -> String {
    let body: Vec<String> = lines.iter().filter_map(Line::hashed).collect();
    let digest = Sha256::digest(body.join("\n").as_bytes());
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

struct Open {
    def: usize,
    indent: usize,
}

pub fn defs(src: &str) -> Vec<Def> {
    let mut scanner = Scanner::default();
    let lines: Vec<Line> = src.lines().map(|l| scanner.scan(l)).collect();
    let mut out: Vec<(String, usize, usize)> = Vec::new(); // (qualname, inicio, fin)
    let mut names: Vec<String> = Vec::new(); // nombres simples de la pila, paralelo a `open`
    let mut open: Vec<Open> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    // Inicio de los decoradores contiguos que esperan a su def (mismo sangrado).
    let mut pending: Option<(usize, usize)> = None;
    for (i, line) in lines.iter().enumerate() {
        if !line.is_code() {
            continue;
        }
        let ind = line.indent();
        // Cualquier línea de código a sangría <= la de la cima cierra esa definición.
        while open.last().is_some_and(|o| o.indent >= ind) {
            if let Some(o) = open.pop() {
                names.pop();
                if let Some(slot) = out.get_mut(o.def) {
                    slot.2 = i;
                }
            }
        }
        let text = line.trimmed();
        if text.starts_with('@') {
            if pending.is_none_or(|(_, pi)| pi != ind) {
                pending = Some((i, ind));
            }
            continue;
        }
        let deco_start = pending.take().filter(|(_, pi)| *pi == ind).map(|(s, _)| s);
        let Some(name) = opener(text) else { continue };
        let qual = names
            .iter()
            .map(String::as_str)
            .chain([name])
            .collect::<Vec<_>>()
            .join(".");
        // Mismo nombre cualificado repetido (redefiniciones, setters): la n-ésima lleva `#n`.
        let n = seen.entry(qual.clone()).or_insert(0);
        *n += 1;
        let qualname = if *n == 1 { qual } else { format!("{qual}#{n}") };
        open.push(Open {
            def: out.len(),
            indent: ind,
        });
        names.push(name.to_string());
        out.push((qualname, deco_start.unwrap_or(i), lines.len()));
    }
    out.into_iter()
        .map(|(qualname, start, end)| Def {
            qualname,
            hash: body_hash(lines.get(start..end).unwrap_or_default()),
        })
        .collect()
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

/// Claves ordenadas: mover una función de sitio no reordena el archivo.
fn to_json(defs: &[Def]) -> Value {
    let sorted: BTreeMap<&str, &str> = defs
        .iter()
        .map(|d| (d.qualname.as_str(), d.hash.as_str()))
        .collect();
    json!({ "defs": sorted })
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
/// Un archivo ilegible o una línea base corrupta es `Err` (el llamador sale con 2),
/// para que no se confunda con deriva.
pub fn run(
    baseline: &Path,
    paths: &[String],
    write: bool,
    accept: &[String],
) -> Result<i32, String> {
    let previous: Value = match fs::read_to_string(baseline) {
        Ok(t) => match serde_json::from_str(&t) {
            Ok(v) => v,
            Err(_) if write => Value::Null,
            Err(e) => return Err(format!("línea base corrupta {}: {e}", baseline.display())),
        },
        Err(_) => Value::Null,
    };
    // Los archivos que no se pasan ahora conservan su entrada.
    let mut files: BTreeMap<String, Value> = previous
        .get("files")
        .and_then(Value::as_object)
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    let mut drifted = false;
    let mut known: Vec<String> = Vec::new();
    for path in paths {
        let src = fs::read_to_string(path).map_err(|e| format!("no se puede leer {path}: {e}"))?;
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
        known.extend(now.iter().chain(&base).map(|d| d.qualname.clone()));
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
            files.insert(key, to_json(&now));
        } else if !accept.is_empty() {
            files.insert(key, to_json(&accepted(&base, &now, accept)));
        }
    }
    for a in accept {
        if !known.contains(a) {
            eprintln!("aviso: --accept {a} no coincide con ninguna definición");
        }
    }
    if write || !accept.is_empty() {
        let files: Map<String, Value> = files.into_iter().collect();
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

    /// Directorio temporal que se borra también si una aserción falla.
    struct Tmp(std::path::PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Self {
            let d = std::env::temp_dir().join(format!("app-drift-{tag}-{}", std::process::id()));
            fs::create_dir_all(&d).unwrap();
            Tmp(d)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn run_baseline_roundtrip_in_tempdir() {
        let tmp = Tmp::new("rt");
        let src = tmp.0.join("cc-app");
        let base = tmp.0.join("base.json");
        fs::write(&src, SRC).unwrap();
        let p = [src.to_string_lossy().into_owned()];
        assert_eq!(run(&base, &p, true, &[]).unwrap(), 0);
        assert_eq!(run(&base, &p, false, &[]).unwrap(), 0);
        fs::write(&src, SRC.replace("return 1", "return 2")).unwrap();
        assert_eq!(run(&base, &p, false, &[]).unwrap(), 1);
        let acc = ["K".to_string(), "K.m".to_string()];
        assert_eq!(run(&base, &p, false, &acc).unwrap(), 1);
        assert_eq!(run(&base, &p, false, &[]).unwrap(), 0);
    }

    #[test]
    fn missing_input_is_an_error_not_drift() {
        let tmp = Tmp::new("missing");
        let gone = tmp.0.join("no-existe").to_string_lossy().into_owned();
        let err = run(
            &tmp.0.join("b.json"),
            std::slice::from_ref(&gone),
            false,
            &[],
        )
        .unwrap_err();
        assert!(
            err.contains("no se puede leer") && err.contains(&gone),
            "{err}"
        );
    }

    #[test]
    fn corrupt_baseline_is_an_error_unless_rewriting() {
        let tmp = Tmp::new("corrupt");
        let src = tmp.0.join("cc-app");
        let base = tmp.0.join("b.json");
        fs::write(&src, SRC).unwrap();
        fs::write(&base, "{no es json").unwrap();
        let p = [src.to_string_lossy().into_owned()];
        assert!(run(&base, &p, false, &["a".into()]).is_err());
        assert_eq!(run(&base, &p, true, &[]).unwrap(), 0);
    }

    #[test]
    fn baseline_keys_are_sorted_and_stable() {
        let tmp = Tmp::new("sorted");
        let src = tmp.0.join("cc-app");
        let base = tmp.0.join("b.json");
        let p = [src.to_string_lossy().into_owned()];
        fs::write(&src, "def z():\n    pass\n\ndef a():\n    pass\n").unwrap();
        run(&base, &p, true, &[]).unwrap();
        let first = fs::read_to_string(&base).unwrap();
        assert!(first.find("\"a\"").unwrap() < first.find("\"z\"").unwrap());
        // Mover las funciones de sitio no cambia el archivo.
        fs::write(&src, "def a():\n    pass\n\ndef z():\n    pass\n").unwrap();
        run(&base, &p, true, &[]).unwrap();
        assert_eq!(first, fs::read_to_string(&base).unwrap());
    }

    fn changed(a: &str, b: &str) -> Vec<String> {
        diff(&defs(a), &defs(b)).changed
    }

    fn names(src: &str) -> Vec<String> {
        defs(src).into_iter().map(|d| d.qualname).collect()
    }

    #[test]
    fn decorator_is_part_of_the_hash() {
        let a = "@a\ndef f():\n    return 1\n\n@x.y(1)\nclass K:\n    @property\n    def m(self):\n        return 1\n";
        assert_eq!(changed(a, &a.replace("@a", "@b")), ["f"]);
        assert_eq!(changed(a, &a.replace("@property", "@cached")), ["K", "K.m"]);
        // El decorador no pertenece al cuerpo de la def anterior.
        let prev = "def p():\n    return 1\n\n@a\ndef f():\n    return 1\n";
        assert_eq!(changed(prev, &prev.replace("@a", "@b")), ["f"]);
        // Con comentarios y líneas en blanco entre decorador y def.
        let gap = "@a\n\n# nota\ndef f():\n    return 1\n";
        assert_eq!(changed(gap, &gap.replace("@a", "@b")), ["f"]);
    }

    #[test]
    fn multiline_decorator_arguments_are_hashed() {
        let a = "@route(\n    '/x',\n)\ndef f():\n    return 1\n";
        assert_eq!(changed(a, &a.replace("'/x'", "'/y'")), ["f"]);
    }

    #[test]
    fn column_zero_triple_quoted_body_is_hashed() {
        // Caso de `build_css` en bin/cc-notifyd: f-string triple con líneas en columna 0.
        let a = "def build_css(t):\n    return f\"\"\"\n.card {{ color: {t['text']}; }}\n#id {{ top: 0; }}\n.hd {{ left: 1px; }}\n\"\"\"\n\ndef next_one():\n    pass\n";
        assert_eq!(
            changed(a, &a.replace("left: 1px", "left: 2px")),
            ["build_css"]
        );
        assert_eq!(changed(a, &a.replace("top: 0", "top: 9")), ["build_css"]);
        assert_eq!(names(a), ["build_css", "next_one"]);
    }

    #[test]
    fn hash_and_blank_lines_inside_strings_count() {
        let a = "def f():\n    s = \"\"\"\n# no es comentario\n\nfin\n\"\"\"\n    return s\n";
        assert_eq!(changed(a, &a.replace("# no es", "# es")), ["f"]);
        assert_eq!(changed(a, &a.replace("\n\nfin", "\nfin")), ["f"]);
        // Un comentario real sigue ignorándose, también al final de una línea de código.
        let c = "def f():\n    x = 1  # uno\n    return x\n";
        assert!(changed(c, &c.replace("# uno", "# otro")).is_empty());
        // Un `#` dentro de una cadena en una línea de código no corta la línea.
        let h = "def f():\n    x = '# a'\n    return x\n";
        assert_eq!(changed(h, &h.replace("# a", "# b")), ["f"]);
    }

    #[test]
    fn single_quoted_strings_prefixes_and_escapes() {
        let a = "def f():\n    a = r'\\'\n    b = b\"\\\"\"\n    c = '''x\n''' + \"\"\"y\n\"\"\"\n    return a\n\ndef g():\n    return 1\n";
        assert_eq!(names(a), ["f", "g"]);
        assert_eq!(changed(a, &a.replace("return a", "return b")), ["f"]);
        assert_eq!(changed(a, &a.replace("return 1", "return 2")), ["g"]);
    }

    #[test]
    fn backslash_continuation_inside_single_quoted_string() {
        let a = "def f():\n    s = 'uno \\\ndos'\n    return s\n\ndef g():\n    pass\n";
        assert_eq!(names(a), ["f", "g"]);
        assert_eq!(changed(a, &a.replace("dos", "tres")), ["f"]);
    }

    #[test]
    fn multiline_signature_closing_at_def_indent() {
        let a = "def f(\n    a,\n    b=1,\n):\n    return 1\n\nclass K:\n    def m(\n        self,\n    ):\n        return 1\n\n    def n(\n    self):\n        return 3\n";
        assert_eq!(names(a), ["f", "K", "K.m", "K.n"]);
        assert_eq!(changed(a, &a.replacen("return 1", "return 2", 1)), ["f"]);
        assert_eq!(changed(a, &a.replace("return 3", "return 4")), ["K", "K.n"]);
        assert_eq!(
            changed(a, &a.replace("self,\n    )", "self, z,\n    )")),
            ["K", "K.m"]
        );
        let mut m = a.replace("return 1", "return 2");
        m = m.replacen("return 2", "return 1", 1);
        assert_eq!(changed(a, &m), ["K", "K.m"]);
    }

    #[test]
    fn def_under_if_is_not_nested_in_previous_def() {
        let a = "def a():\n    return 1\n\nif X:\n    def b():\n        return 2\nelse:\n    def b():\n        return 3\n\nclass K:\n    if Y:\n        def m(self):\n            pass\n";
        assert_eq!(names(a), ["a", "b", "b#2", "K", "K.m"]);
        assert_eq!(changed(a, &a.replace("return 2", "return 9")), ["b"]);
    }

    #[test]
    fn nested_function_after_statement_in_body() {
        let a = "def a():\n    x = 1\n    def inner():\n        pass\n    y = 2\n    def other():\n        pass\n";
        assert_eq!(names(a), ["a", "a.inner", "a.other"]);
    }

    #[test]
    fn def_keyword_inside_a_string_is_not_a_def() {
        let a =
            "def f():\n    s = \"\"\"\ndef fake():\n    pass\nclass Fake:\n\"\"\"\n    return s\n";
        assert_eq!(names(a), ["f"]);
    }

    #[test]
    fn crlf_tabs_async_and_empty_input() {
        let a =
            "async def f():\r\n\treturn 1\r\n\r\nclass K:\r\n\tdef m(self):\r\n\t\treturn 1\r\n";
        assert_eq!(names(a), ["f", "K", "K.m"]);
        // Mismo código con LF y con CRLF: mismo hash.
        assert_eq!(defs(a), defs(&a.replace("\r\n", "\n")));
        assert_eq!(
            changed(
                a,
                &a.replace("return 1\r\n\r\nclass", "return 2\r\n\r\nclass")
            ),
            ["f"]
        );
        assert!(defs("").is_empty());
        assert!(defs("\n\n# solo comentario\n").is_empty());
        assert!(defs("x = 1\n").is_empty());
    }

    #[test]
    fn nested_quotes_inside_fstring_fields() {
        // Comillas anidadas (distintas e iguales, estilo 3.12) no desvían el escáner.
        let a = "def f(t):\n    s = f\"{t['k']} {t[\"j\"]}\"\n    return s\n\ndef g():\n    pass\n";
        assert_eq!(names(a), ["f", "g"]);
        assert_eq!(changed(a, &a.replace("return s", "return t")), ["f"]);
    }

    #[test]
    fn one_liners_and_trailing_dedent() {
        let a = "def f(): return 1\ndef g(): return 2\nclass K: pass\nx = 1\n";
        assert_eq!(names(a), ["f", "g", "K"]);
        assert_eq!(changed(a, &a.replace("return 2", "return 3")), ["g"]);
    }
}
