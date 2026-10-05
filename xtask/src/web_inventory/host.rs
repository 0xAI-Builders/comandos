//! Llamadas del host (Python de `bin/cc-app` y `bin/cc-app-mac`) a la página.
//!
//! Léxico, como el resto del inventario: un tokenizador mínimo de Python
//! encuentra los sumideros (`run_javascript`, `evaluateJavaScript…`), descubre
//! los envoltorios que reenvían un parámetro al sumidero (`_dash_js(code)`,
//! `_dash_click(elem_id)`) y reconstruye el JS de cada llamada: literales y
//! f-strings (los huecos que no son el parámetro quedan como `__py__`), y para
//! un argumento variable, la última asignación de esa variable.
use std::collections::BTreeMap;

/// Funciones del host que ejecutan JS en una vista web.
const SINKS: &[&str] = &[
    "run_javascript",
    "evaluate_javascript",
    "evaluateJavaScript_completionHandler_",
    "evaluateJavaScript_",
];

/// Hueco de un valor Python dentro del JS reconstruido.
pub const PY_HOLE: &str = "__py__";

/// Marca de un parámetro del envoltorio dentro de su plantilla.
fn param_mark(p: &str) -> String {
    format!("\u{1}{p}\u{1}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCall {
    /// `bin/cc-app` o `bin/cc-app-mac`.
    pub from: String,
    pub line: usize,
    /// Sumidero o envoltorio por el que entra el JS.
    pub via: String,
    /// JS reconstruido; `None` si el origen no es literal (archivo, red…).
    pub js: Option<String>,
    /// `literal`, `variable:<nombre>` o `dinamico`.
    pub origin: String,
}

#[derive(Debug, Default, Clone)]
pub struct HostScan {
    pub calls: Vec<HostCall>,
    /// Manejadores `window.webkit.messageHandlers.<nombre>` que registra.
    pub handlers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PyKind {
    Name,
    Str { fstring: bool },
    Op,
}

#[derive(Debug, Clone)]
struct PyTok {
    kind: PyKind,
    text: String,
    line: usize,
}

fn py_lex(src: &str) -> Vec<PyTok> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1;
    let at = |k: usize| c.get(k).copied();
    while let Some(ch) = at(i) {
        if ch == '\n' {
            line += 1;
            i += 1;
        } else if ch.is_whitespace() {
            i += 1;
        } else if ch == '#' {
            while at(i).is_some_and(|x| x != '\n') {
                i += 1;
            }
        } else if ch.is_alphanumeric() || ch == '_' {
            let start = i;
            while at(i).is_some_and(|x| x.is_alphanumeric() || x == '_') {
                i += 1;
            }
            let word: String = c
                .get(start..i)
                .map(|s| s.iter().collect())
                .unwrap_or_default();
            let lower = word.to_ascii_lowercase();
            let is_prefix = matches!(
                lower.as_str(),
                "r" | "b" | "f" | "u" | "rb" | "br" | "fr" | "rf"
            );
            if is_prefix && matches!(at(i), Some('\'' | '"')) {
                let (text, end, nl) = py_string(&c, i, lower.contains('r'));
                out.push(PyTok {
                    kind: PyKind::Str {
                        fstring: lower.contains('f'),
                    },
                    text,
                    line,
                });
                line += nl;
                i = end;
            } else {
                out.push(PyTok {
                    kind: PyKind::Name,
                    text: word,
                    line,
                });
            }
        } else if ch == '\'' || ch == '"' {
            let (text, end, nl) = py_string(&c, i, false);
            out.push(PyTok {
                kind: PyKind::Str { fstring: false },
                text,
                line,
            });
            line += nl;
            i = end;
        } else {
            let two: String = [Some(ch), at(i + 1)].iter().flatten().collect();
            let op = if matches!(
                two.as_str(),
                "==" | "!=" | "<=" | ">=" | "->" | "**" | "//" | "+=" | "-=" | ":="
            ) {
                two
            } else {
                ch.to_string()
            };
            i += op.chars().count();
            out.push(PyTok {
                kind: PyKind::Op,
                text: op,
                line,
            });
        }
    }
    out
}

/// Literal de cadena desde la comilla en `i`: (valor, fin, saltos de línea).
fn py_string(c: &[char], i: usize, raw: bool) -> (String, usize, usize) {
    let q = c.get(i).copied().unwrap_or('"');
    let triple = c.get(i + 1) == Some(&q) && c.get(i + 2) == Some(&q);
    let mut k = if triple { i + 3 } else { i + 1 };
    let mut s = String::new();
    let mut nl = 0;
    while let Some(&ch) = c.get(k) {
        if ch == q && (!triple || (c.get(k + 1) == Some(&q) && c.get(k + 2) == Some(&q))) {
            k += if triple { 3 } else { 1 };
            return (s, k, nl);
        }
        if ch == '\n' {
            nl += 1;
            if !triple {
                return (s, k, nl - 1);
            }
        }
        if ch == '\\' && !raw {
            match c.get(k + 1) {
                Some('n') => s.push('\n'),
                Some('t') => s.push('\t'),
                Some('\n') => nl += 1,
                Some(&o @ ('\\' | '\'' | '"')) => s.push(o),
                Some(&o) => {
                    s.push('\\');
                    s.push(o);
                }
                None => {}
            }
            k += 2;
            continue;
        }
        s.push(ch);
        k += 1;
    }
    (s, k, nl)
}

/// Valor JS de un literal: en f-strings `{{`/`}}` son llaves y cada `{expr}`
/// es el parámetro marcado (si `expr` es uno de `params`) o un hueco.
fn literal_js(t: &PyTok, params: &[String]) -> String {
    if t.kind != (PyKind::Str { fstring: true }) {
        return t.text.clone();
    }
    let mut out = String::new();
    let mut it = t.text.chars().peekable();
    while let Some(ch) = it.next() {
        match ch {
            '{' if it.peek() == Some(&'{') => {
                it.next();
                out.push('{');
            }
            '}' if it.peek() == Some(&'}') => {
                it.next();
                out.push('}');
            }
            '{' => {
                let mut depth = 1;
                let mut expr = String::new();
                for n in it.by_ref() {
                    if n == '{' {
                        depth += 1;
                    } else if n == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    expr.push(n);
                }
                let name = expr.split(['!', ':']).next().unwrap_or("").trim();
                if params.iter().any(|p| p == name) {
                    out.push_str(&param_mark(name));
                } else {
                    out.push_str(PY_HOLE);
                }
            }
            o => out.push(o),
        }
    }
    out
}

#[derive(Debug, Clone)]
struct Def {
    name: String,
    params: Vec<String>,
    line: usize,
    /// Última línea del cuerpo (por sangría).
    end: usize,
}

#[derive(Debug, Clone)]
struct Wrapper {
    param: String,
    /// Línea del `def`.
    line: usize,
    /// JS con `param_mark(param)` donde va el argumento.
    template: String,
}

/// Valor de un argumento: JS reconstruido y de dónde sale.
struct Arg {
    js: Option<String>,
    origin: String,
    /// Parámetro del `def` envolvente que el argumento reenvía.
    forwards: Option<String>,
}

pub fn scan_host(label: &str, src: &str) -> HostScan {
    let toks = py_lex(src);
    let defs = defs(&toks, src);
    let mut wrappers: BTreeMap<String, Wrapper> = BTreeMap::new();
    // Envoltorios cuya plantilla reutiliza otro envoltorio.
    let mut reused: Vec<String> = Vec::new();
    let callees = |w: &BTreeMap<String, Wrapper>| -> Vec<usize> {
        (0..toks.len())
            .filter(|&i| {
                let Some(t) = toks.get(i) else { return false };
                t.kind == PyKind::Name
                    && (SINKS.contains(&t.text.as_str()) || w.contains_key(&t.text))
                    && toks
                        .get(i + 1)
                        .is_some_and(|n| n.kind == PyKind::Op && n.text == "(")
                    && i.checked_sub(1)
                        .and_then(|p| toks.get(p))
                        .is_none_or(|p| p.text != "def")
            })
            .collect()
    };
    // Punto fijo: un `def` que pasa su parámetro a un sumidero o a otro
    // envoltorio es a su vez un envoltorio.
    loop {
        let mut changed = false;
        for i in callees(&wrappers) {
            let Some(callee) = toks.get(i).map(|t| t.text.clone()) else {
                continue;
            };
            let chain = enclosing(&defs, toks.get(i).map(|t| t.line).unwrap_or(0));
            let arg = eval_arg(&toks, i + 2, &chain, true);
            let (Some(p), Some(js)) = (arg.forwards, arg.js) else {
                continue;
            };
            let Some(owner) = chain.iter().find(|d| d.params.contains(&p)) else {
                continue;
            };
            if wrappers.contains_key(&owner.name) {
                continue;
            }
            let template = match wrappers.get(&callee) {
                Some(w) => {
                    reused.push(callee.clone());
                    w.template.replace(&param_mark(&w.param), &js)
                }
                None => js,
            };
            wrappers.insert(
                owner.name.clone(),
                Wrapper {
                    param: p,
                    line: owner.line,
                    template,
                },
            );
            changed = true;
        }
        if !changed {
            break;
        }
    }
    let mut scan = HostScan::default();
    for i in callees(&wrappers) {
        let Some(t) = toks.get(i) else { continue };
        let chain = enclosing(&defs, t.line);
        let arg = eval_arg(&toks, i + 2, &chain, true);
        if arg.forwards.is_some() {
            continue; // cuerpo de un envoltorio: su JS sale en cada llamada
        }
        let js = arg.js.map(|js| {
            let full = match wrappers.get(&t.text) {
                Some(w) => w.template.replace(&param_mark(&w.param), &js),
                None => js,
            };
            strip_marks(&full)
        });
        scan.calls.push(HostCall {
            from: label.to_string(),
            line: t.line,
            via: t.text.clone(),
            origin: if js.is_some() {
                arg.origin
            } else {
                "dinamico".to_string()
            },
            js,
        });
    }
    // Un envoltorio sin llamadas literales aún aporta su plantilla.
    for (name, w) in &wrappers {
        if !reused.contains(name) && !scan.calls.iter().any(|c| &c.via == name) {
            scan.calls.push(HostCall {
                from: label.to_string(),
                line: w.line,
                via: name.clone(),
                js: Some(strip_marks(&w.template)),
                origin: "plantilla".to_string(),
            });
        }
    }
    // Manejadores de mensajes que la app registra.
    for (i, t) in toks.iter().enumerate() {
        let registers = t.kind == PyKind::Name
            && (t.text.contains("ScriptMessageHandler")
                || t.text.contains("script_message_handler"))
            && toks.get(i + 1).is_some_and(|n| n.text == "(");
        if !registers {
            continue;
        }
        let name = toks
            .iter()
            .skip(i + 2)
            .take_while(|x| x.text != ")")
            .find(|x| matches!(x.kind, PyKind::Str { .. }))
            .map(|x| x.text.clone());
        if let Some(n) = name
            && !scan.handlers.contains(&n)
        {
            scan.handlers.push(n);
        }
    }
    scan
}

fn strip_marks(s: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for ch in s.chars() {
        if ch == '\u{1}' {
            if !inside {
                out.push_str(PY_HOLE);
            }
            inside = !inside;
        } else if !inside {
            out.push(ch);
        }
    }
    out
}

/// `def nombre(params):` con la extensión de su cuerpo por sangría.
fn defs(toks: &[PyTok], src: &str) -> Vec<Def> {
    let lines: Vec<&str> = src.lines().collect();
    let indent = |l: usize| -> Option<usize> {
        let s = lines.get(l.checked_sub(1)?)?;
        if s.trim().is_empty() || s.trim_start().starts_with('#') {
            return None;
        }
        Some(s.len() - s.trim_start().len())
    };
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if t.kind != PyKind::Name || t.text != "def" {
            continue;
        }
        let Some(name) = toks.get(i + 1).filter(|n| n.kind == PyKind::Name) else {
            continue;
        };
        let mut params = Vec::new();
        let mut depth = 0usize;
        let mut expect = true;
        for p in toks.iter().skip(i + 2) {
            match p.text.as_str() {
                "(" | "[" | "{" => {
                    depth += 1;
                    if depth == 1 {
                        expect = true;
                    }
                }
                ")" | "]" | "}" => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                "," if depth == 1 => expect = true,
                "*" | "**" => {}
                _ if depth == 1 && expect && p.kind == PyKind::Name => {
                    params.push(p.text.clone());
                    expect = false;
                }
                _ => {
                    if depth == 1 {
                        expect = false;
                    }
                }
            }
        }
        let base = indent(t.line).unwrap_or(0);
        let mut end = t.line;
        for l in t.line + 1..=lines.len() {
            match indent(l) {
                None => {}
                Some(n) if n > base => end = l,
                Some(_) => break,
            }
        }
        out.push(Def {
            name: name.text.clone(),
            params,
            line: t.line,
            end,
        });
    }
    out
}

/// `def`s que contienen la línea, del más interno al más externo.
fn enclosing(defs: &[Def], line: usize) -> Vec<Def> {
    let mut v: Vec<Def> = defs
        .iter()
        .filter(|d| d.line < line && line <= d.end)
        .cloned()
        .collect();
    v.sort_by_key(|d| std::cmp::Reverse(d.line));
    v
}

/// Primer argumento de la llamada cuyo contenido empieza en `start`.
fn eval_arg(toks: &[PyTok], start: usize, chain: &[Def], resolve: bool) -> Arg {
    let params: Vec<String> = chain.iter().flat_map(|d| d.params.clone()).collect();
    let mut end = start;
    let mut depth = 0usize;
    while let Some(t) = toks.get(end) {
        match t.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            "," if depth == 0 && t.kind == PyKind::Op => break,
            _ => {}
        }
        end += 1;
    }
    expr_js(toks, start, end, chain, &params, resolve)
}

fn expr_js(
    toks: &[PyTok],
    start: usize,
    end: usize,
    chain: &[Def],
    params: &[String],
    resolve: bool,
) -> Arg {
    let part = toks.get(start..end).unwrap_or(&[]);
    let has_str = part.iter().any(|t| matches!(t.kind, PyKind::Str { .. }));
    if !has_str {
        // Sin literales: un nombre solo es un parámetro reenviado o una variable.
        let names: Vec<&PyTok> = part
            .iter()
            .filter(|t| !(t.kind == PyKind::Op && matches!(t.text.as_str(), "(" | ")")))
            .collect();
        if let [only] = names.as_slice()
            && only.kind == PyKind::Name
        {
            if params.contains(&only.text) {
                return Arg {
                    js: Some(param_mark(&only.text)),
                    origin: "literal".to_string(),
                    forwards: Some(only.text.clone()),
                };
            }
            if resolve && let Some(a) = assignment(toks, &only.text, start, chain) {
                return a;
            }
        }
        return Arg {
            js: None,
            origin: "dinamico".to_string(),
            forwards: None,
        };
    }
    let mut js = String::new();
    let mut gap: Vec<&PyTok> = Vec::new();
    let mut forwards = None;
    let flush = |gap: &mut Vec<&PyTok>, js: &mut String, forwards: &mut Option<String>| {
        let real: Vec<&&PyTok> = gap
            .iter()
            .filter(|t| !(t.kind == PyKind::Op && matches!(t.text.as_str(), "+" | "(" | ")")))
            .collect();
        if let [only] = real.as_slice()
            && only.kind == PyKind::Name
            && params.contains(&only.text)
        {
            js.push_str(&param_mark(&only.text));
            *forwards = Some(only.text.clone());
        } else if !real.is_empty() {
            js.push_str(PY_HOLE);
        }
        gap.clear();
    };
    for t in part {
        if matches!(t.kind, PyKind::Str { .. }) {
            flush(&mut gap, &mut js, &mut forwards);
            let lit = literal_js(t, params);
            if forwards.is_none()
                && let Some(p) = params.iter().find(|p| lit.contains(&param_mark(p)))
            {
                forwards = Some(p.clone());
            }
            js.push_str(&lit);
        } else {
            gap.push(t);
        }
    }
    flush(&mut gap, &mut js, &mut forwards);
    Arg {
        js: Some(js),
        origin: "literal".to_string(),
        forwards,
    }
}

/// Última asignación `nombre = …` antes de `before` dentro del `def` más interno.
fn assignment(toks: &[PyTok], name: &str, before: usize, chain: &[Def]) -> Option<Arg> {
    let lo = chain.first().map(|d| d.line).unwrap_or(0);
    let at = (0..before).rev().find(|&k| {
        toks.get(k)
            .is_some_and(|t| t.kind == PyKind::Name && t.text == name && t.line >= lo)
            && toks
                .get(k + 1)
                .is_some_and(|n| n.kind == PyKind::Op && n.text == "=")
            && k.checked_sub(1)
                .and_then(|p| toks.get(p))
                .is_none_or(|p| p.text != ".")
    })?;
    // Fin de la sentencia: salto de línea con paréntesis cerrados.
    let start = at + 2;
    let mut end = start;
    let mut depth = 0usize;
    let mut last_line = toks.get(start)?.line;
    while let Some(t) = toks.get(end) {
        if depth == 0 && t.line > last_line && end > start {
            break;
        }
        match t.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
        last_line = t.line;
        end += 1;
    }
    let mut a = expr_js(toks, start, end, chain, &[], false);
    if a.js.is_some() {
        a.origin = format!("variable:{name}");
    }
    Some(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_call_and_fstring_wrapper() {
        let src = "def w(code):\n    wv.run_javascript(f\"try{{{code}}}catch(e){{}}\", None)\n\nw(f\"go({json.dumps(x)})\")\nwv.run_javascript('a()' 'b()')\n";
        let s = scan_host("bin/cc-app", src);
        let js: Vec<&str> = s.calls.iter().filter_map(|c| c.js.as_deref()).collect();
        assert_eq!(js, ["try{go(__py__)}catch(e){}", "a()b()"]);
        assert_eq!(s.calls[0].via, "w");
        assert_eq!(s.calls[0].line, 4);
    }

    #[test]
    fn forwarding_wrapper_of_wrapper() {
        let src = "def a(code):\n    wv.run_javascript(f\"try{{{code}}}catch(e){{}}\")\n\ndef b(c):\n    a(c)\n\nb(\"x()\")\n";
        let s = scan_host("h", src);
        assert_eq!(s.calls.len(), 1, "{:?}", s.calls);
        assert_eq!(s.calls[0].js.as_deref(), Some("try{x()}catch(e){}"));
    }
}
