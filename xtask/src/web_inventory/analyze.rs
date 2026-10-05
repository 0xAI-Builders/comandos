//! Hechos léxicos de un trozo de JavaScript: qué globales define, qué
//! identificadores libres lee o asigna, y los literales que interesan al port
//! (rutas, ids del DOM, claves de `localStorage`, intervalos, mensajes).
//!
//! Heurística, no semántica: los ámbitos son los bloques `{}`; los parámetros
//! de funciones, flechas y `catch` se declaran en el bloque del cuerpo; `var`
//! se trata como `let` (no se eleva a la función).
use super::js_lex::{Kind, Token, lex};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Cómo nace un global: decide si es propiedad de `window`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Binding {
    Function,
    Var,
    Let,
    Const,
    Class,
    /// `window.X =`, `root.X =`, `globalThis.X =`, `self.X =`.
    Window,
}

impl Binding {
    pub fn as_str(self) -> &'static str {
        match self {
            Binding::Function => "function",
            Binding::Var => "var",
            Binding::Let => "let",
            Binding::Const => "const",
            Binding::Class => "class",
            Binding::Window => "window",
        }
    }
    /// `let`, `const` y `class` de nivel superior no son propiedades de `window`.
    pub fn lexical(self) -> bool {
        matches!(self, Binding::Let | Binding::Const | Binding::Class)
    }
}

/// Lectura de un identificador libre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    pub name: String,
    pub line: usize,
    /// Leído como propiedad (`window.X`, `root.X`…), no por su nombre.
    pub member: bool,
}

#[derive(Debug, Default, Clone)]
pub struct Facts {
    pub defines: Vec<(String, Binding)>,
    pub uses: Vec<Use>,
    pub assigns: Vec<String>,
    pub parent_refs: Vec<String>,
    pub dom_ids: Vec<String>,
    pub routes: Vec<String>,
    pub storage_keys: Vec<String>,
    pub intervals_ms: Vec<u64>,
    pub post_messages: Vec<String>,
    pub message_types: Vec<String>,
    pub strings: usize,
}

const KEYWORDS: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "async",
    "await",
    "of",
    "null",
    "true",
    "false",
    "undefined",
    "arguments",
    "static",
    "get",
    "set",
    "NaN",
    "Infinity",
];

/// Antes de `(` estas palabras abren una condición, no una lista de parámetros.
const NOT_PARAMS: &[&str] = &[
    "if", "while", "for", "switch", "with", "return", "typeof", "new", "await", "in", "of",
];

/// Bases que son `window`: `X` tras ellas es un global.
const WINDOW_ALIASES: &[&str] = &["window", "self", "globalThis", "root", "top"];

const ASSIGN_OPS: &[&str] = &[
    "=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", ">>>=", "&=", "|=", "^=", "&&=", "||=",
    "??=",
];

pub fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

fn push_unique<T: PartialEq>(v: &mut Vec<T>, x: T) {
    if !v.contains(&x) {
        v.push(x);
    }
}

struct Scope {
    parent: Option<usize>,
    decls: HashSet<String>,
}

struct Pass<'a> {
    t: &'a [Token],
    /// Ámbito de cada token.
    scope_of: Vec<usize>,
    scopes: Vec<Scope>,
    /// Tokens que nombran algo (declaración, clave, método): no son lecturas.
    naming: Vec<bool>,
    facts: Facts,
}

pub fn analyze(src: &str) -> Facts {
    let tokens = lex(src);
    let mut p = Pass {
        t: &tokens,
        scope_of: vec![0; tokens.len()],
        scopes: vec![Scope {
            parent: None,
            decls: HashSet::new(),
        }],
        naming: vec![false; tokens.len()],
        facts: Facts::default(),
    };
    p.declarations();
    p.references();
    p.literals();
    p.facts
}

impl Pass<'_> {
    fn tok(&self, i: usize) -> Option<&Token> {
        self.t.get(i)
    }

    fn punct_at(&self, i: usize, s: &str) -> bool {
        self.tok(i).is_some_and(|t| t.is_punct(s))
    }

    /// `true` si el token `i` va precedido de `.` o `?.` (acceso a propiedad).
    fn after_dot(&self, i: usize) -> bool {
        i > 0 && (self.punct_at(i - 1, ".") || self.punct_at(i - 1, "?."))
    }

    fn mark(&mut self, i: usize) {
        if let Some(m) = self.naming.get_mut(i) {
            *m = true;
        }
    }

    fn declare(&mut self, scope: usize, name: &str) {
        if let Some(s) = self.scopes.get_mut(scope) {
            s.decls.insert(name.to_string());
        }
    }

    /// Índice del cierre que empareja la apertura en `open`.
    fn matching(&self, open: usize) -> Option<usize> {
        let mut depth = 0usize;
        for (k, t) in self.t.iter().enumerate().skip(open) {
            if t.kind != Kind::Punct {
                continue;
            }
            match t.text.as_str() {
                "(" | "[" | "{" | "${" => depth += 1,
                ")" | "]" | "}" | "}$" => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(k);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Identificadores que un patrón de enlace (`a`, `{a, b: c}`, `[x, ...y]`,
    /// lista de parámetros) declara entre `from` y `to` exclusivos. Los
    /// valores por defecto (`= …`) y las claves (`k:`) no declaran.
    fn binding_names(&self, from: usize, to: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut depth = 0usize;
        let mut default_at: Option<usize> = None;
        for k in from..to {
            let Some(t) = self.tok(k) else { break };
            if t.kind == Kind::Punct {
                match t.text.as_str() {
                    "(" | "[" | "{" | "${" => depth += 1,
                    ")" | "]" | "}" | "}$" => {
                        if default_at == Some(depth) {
                            default_at = None;
                        }
                        depth = depth.saturating_sub(1);
                    }
                    "," if default_at == Some(depth) => default_at = None,
                    "=" if default_at.is_none() => default_at = Some(depth),
                    _ => {}
                }
                continue;
            }
            if default_at.is_some()
                || t.kind != Kind::Ident
                || is_keyword(&t.text)
                || self.after_dot(k)
            {
                continue;
            }
            if self.punct_at(k + 1, ":") {
                continue; // clave de un patrón de objeto
            }
            out.push(k);
        }
        out
    }

    /// Primer pase: árbol de ámbitos y declaraciones.
    fn declarations(&mut self) {
        let n = self.t.len();
        let mut stack: Vec<usize> = vec![0];
        let mut brackets: Vec<&str> = Vec::new();
        // Parámetros a declarar en el `{` que está en ese índice.
        let mut pending: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut i = 0;
        while i < n {
            let cur = stack.last().copied().unwrap_or(0);
            if let Some(s) = self.scope_of.get_mut(i) {
                *s = cur;
            }
            let Some(t) = self.tok(i).cloned() else { break };
            if t.kind == Kind::Punct {
                match t.text.as_str() {
                    "{" => {
                        let id = self.scopes.len();
                        self.scopes.push(Scope {
                            parent: Some(cur),
                            decls: HashSet::new(),
                        });
                        if let Some(ps) = pending.remove(&i) {
                            for k in ps {
                                if let Some(name) = self.tok(k).map(|t| t.text.clone()) {
                                    self.declare(id, &name);
                                }
                            }
                        }
                        stack.push(id);
                        brackets.push("{");
                        if let Some(s) = self.scope_of.get_mut(i) {
                            *s = id;
                        }
                    }
                    "(" => {
                        self.param_group(i, cur, &mut pending);
                        brackets.push("(");
                    }
                    "[" | "${" => brackets.push("["),
                    "}" => {
                        if brackets.pop() == Some("{") && stack.len() > 1 {
                            stack.pop();
                        }
                    }
                    ")" | "]" | "}$" => {
                        brackets.pop();
                    }
                    // Flecha de un solo parámetro sin paréntesis: `x => …`.
                    "=>" if i > 0
                        && self
                            .tok(i - 1)
                            .is_some_and(|p| p.kind == Kind::Ident && !is_keyword(&p.text)) =>
                    {
                        self.mark(i - 1);
                        self.bind_params(i + 1, cur, vec![i - 1], &mut pending);
                    }
                    _ => {}
                }
            } else if t.kind == Kind::Ident && !self.after_dot(i) {
                let top = brackets.is_empty();
                match t.text.as_str() {
                    "const" | "let" | "var" => {
                        let b = match t.text.as_str() {
                            "const" => Binding::Const,
                            "let" => Binding::Let,
                            _ => Binding::Var,
                        };
                        self.declarators(i + 1, cur, top, b);
                    }
                    "function" | "class" => {
                        let mut k = i + 1;
                        if self.punct_at(k, "*") {
                            k += 1;
                        }
                        if let Some(name) = self
                            .tok(k)
                            .filter(|x| x.kind == Kind::Ident && !is_keyword(&x.text))
                        {
                            let name = name.text.clone();
                            self.mark(k);
                            self.declare(cur, &name);
                            // `export default function`… no aplica: el tablero no usa módulos.
                            let is_expr = i > 0
                                && self.tok(i - 1).is_some_and(|p| {
                                    p.kind == Kind::Punct
                                        && matches!(
                                            p.text.as_str(),
                                            "=" | "(" | "," | ":" | "?" | "||" | "&&" | "??"
                                        )
                                });
                            if top && !is_expr {
                                let b = if t.text == "class" {
                                    Binding::Class
                                } else {
                                    Binding::Function
                                };
                                push_unique(&mut self.facts.defines, (name, b));
                            }
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }
    }

    /// Una lista entre paréntesis que va seguida de `=>` o de un cuerpo `{`
    /// de función, método o `catch` declara parámetros en ese cuerpo.
    fn param_group(&mut self, open: usize, cur: usize, pending: &mut HashMap<usize, Vec<usize>>) {
        let Some(close) = self.matching(open) else {
            return;
        };
        let toks = self.t;
        let before = open.checked_sub(1).and_then(|b| toks.get(b));
        let is_arrow = self.punct_at(close + 1, "=>");
        let is_body = self.punct_at(close + 1, "{")
            && before.is_some_and(|b| {
                b.kind == Kind::Ident
                    && (b.text == "function"
                        || b.text == "catch"
                        || !NOT_PARAMS.contains(&b.text.as_str()))
            });
        if !is_arrow && !is_body {
            return;
        }
        let names = self.binding_names(open + 1, close);
        for &k in &names {
            self.mark(k);
        }
        // Nombre de método (`foo(a) {`): nombra, no lee.
        if is_body
            && let Some(b) = before
            && b.kind == Kind::Ident
            && !is_keyword(&b.text)
        {
            self.mark(open.saturating_sub(1));
        }
        let body = if is_arrow { close + 2 } else { close + 1 };
        if is_arrow {
            self.bind_params(body, cur, names, pending);
        } else {
            pending.insert(body, names);
        }
    }

    /// Parámetros de una flecha: al bloque si lo hay, si no al ámbito actual.
    fn bind_params(
        &mut self,
        body: usize,
        cur: usize,
        names: Vec<usize>,
        pending: &mut HashMap<usize, Vec<usize>>,
    ) {
        if self.punct_at(body, "{") {
            pending.entry(body).or_default().extend(names);
        } else {
            for k in names {
                if let Some(name) = self.tok(k).map(|t| t.text.clone()) {
                    self.declare(cur, &name);
                }
            }
        }
    }

    /// `const a = 1, {b, c} = x, [d] = y`.
    fn declarators(&mut self, mut k: usize, cur: usize, top: bool, b: Binding) {
        loop {
            let Some(t) = self.tok(k) else { return };
            let names: Vec<usize> = if t.kind == Kind::Ident && !is_keyword(&t.text) {
                vec![k]
            } else if t.is_punct("{") || t.is_punct("[") {
                let Some(close) = self.matching(k) else {
                    return;
                };
                let ns = self.binding_names(k + 1, close);
                k = close;
                ns
            } else {
                return;
            };
            for idx in names {
                self.mark(idx);
                if let Some(name) = self.tok(idx).map(|t| t.text.clone()) {
                    self.declare(cur, &name);
                    if top {
                        push_unique(&mut self.facts.defines, (name, b));
                    }
                }
            }
            k += 1;
            if self.punct_at(k, "=") {
                match self.skip_initializer(k + 1) {
                    Some(next) => k = next,
                    None => return,
                }
            }
            if self.punct_at(k, ",") {
                k += 1;
                continue;
            }
            return;
        }
    }

    /// Salta una expresión de inicialización; devuelve el índice de la `,`
    /// que separa el siguiente declarador, o `None` si la sentencia acabó.
    fn skip_initializer(&self, mut k: usize) -> Option<usize> {
        let mut depth = 0usize;
        while let Some(t) = self.tok(k) {
            if t.kind == Kind::Punct {
                match t.text.as_str() {
                    "(" | "[" | "{" | "${" => depth += 1,
                    ")" | "]" | "}" | "}$" => {
                        if depth == 0 {
                            return None;
                        }
                        depth -= 1;
                    }
                    ";" if depth == 0 => return None,
                    "," if depth == 0 => return Some(k),
                    _ => {}
                }
            }
            // Fin de línea sin operador pendiente: inserción automática de `;`.
            if depth == 0
                && let Some(next) = self.tok(k + 1)
                && next.line > t.line
                && !ends_expression_open(t)
                && !starts_continuation(next)
            {
                return None;
            }
            k += 1;
        }
        None
    }

    fn shadowed(&self, i: usize, name: &str) -> bool {
        let mut s = self.scope_of.get(i).copied();
        while let Some(id) = s {
            let Some(scope) = self.scopes.get(id) else {
                return false;
            };
            if scope.decls.contains(name) {
                return true;
            }
            s = scope.parent;
        }
        false
    }

    fn is_assign_target(&self, i: usize) -> bool {
        let next_assign = self.tok(i + 1).is_some_and(|n| {
            n.kind == Kind::Punct
                && (ASSIGN_OPS.contains(&n.text.as_str()) || n.text == "++" || n.text == "--")
        });
        let prev_incr = i > 0 && (self.punct_at(i - 1, "++") || self.punct_at(i - 1, "--"));
        next_assign || prev_incr
    }

    /// Segundo pase: lecturas y asignaciones de identificadores libres.
    fn references(&mut self) {
        for i in 0..self.t.len() {
            let Some(t) = self.tok(i) else { break };
            if t.kind != Kind::Ident
                || is_keyword(&t.text)
                || self.naming.get(i).copied().unwrap_or(false)
            {
                continue;
            }
            let name = t.text.clone();
            let line = t.line;
            if self.after_dot(i) {
                self.member_ref(i, &name, line);
                continue;
            }
            // Clave de objeto: `{ k: v }`.
            let prev_open = i > 0 && (self.punct_at(i - 1, "{") || self.punct_at(i - 1, ","));
            if prev_open && self.punct_at(i + 1, ":") {
                continue;
            }
            if self.shadowed(i, &name) {
                continue;
            }
            if self.is_assign_target(i) {
                push_unique(&mut self.facts.assigns, name.clone());
            }
            self.facts.uses.push(Use {
                name,
                line,
                member: false,
            });
        }
    }

    /// `window.X` define (si se asigna) o lee `X`; `parent.X` es una
    /// llamada de un iframe al tablero.
    fn member_ref(&mut self, i: usize, name: &str, line: usize) {
        let Some(base_i) = i.checked_sub(2) else {
            return;
        };
        let Some(base) = self.tok(base_i) else { return };
        if base.kind != Kind::Ident || self.after_dot(base_i) {
            return;
        }
        let base_name = base.text.clone();
        // `root` es el parámetro de la envoltura UMD: siempre `window`.
        let local = base_name != "root" && self.shadowed(base_i, &base_name);
        if local {
            return;
        }
        if base_name == "parent" {
            if name != "postMessage" && name != "document" {
                push_unique(&mut self.facts.parent_refs, name.to_string());
            }
            return;
        }
        if !WINDOW_ALIASES.contains(&base_name.as_str()) {
            return;
        }
        let assigns = self
            .tok(i + 1)
            .is_some_and(|n| n.kind == Kind::Punct && ASSIGN_OPS.contains(&n.text.as_str()));
        if assigns {
            push_unique(&mut self.facts.defines, (name.to_string(), Binding::Window));
        } else {
            self.facts.uses.push(Use {
                name: name.to_string(),
                line,
                member: true,
            });
        }
    }

    /// Valores literales de `const X = "…"` / `const X = 123` en la unidad
    /// (un nombre con dos valores distintos no se resuelve).
    fn constants(&self) -> (HashMap<String, String>, HashMap<String, u64>) {
        let mut strs: HashMap<String, Option<String>> = HashMap::new();
        let mut nums: HashMap<String, Option<u64>> = HashMap::new();
        for i in 0..self.t.len() {
            if !self
                .tok(i)
                .is_some_and(|t| t.is_ident("const") && !self.after_dot(i))
            {
                continue;
            }
            let mut k = i + 1;
            while let (Some(name), Some(v)) = (self.tok(k), self.tok(k + 2)) {
                if name.kind != Kind::Ident || !self.punct_at(k + 1, "=") {
                    break;
                }
                let end_ok = self
                    .tok(k + 3)
                    .is_none_or(|e| e.is_punct(";") || e.is_punct(",") || e.line > v.line);
                if !end_ok {
                    break;
                }
                match v.kind {
                    Kind::Str => {
                        let slot = strs
                            .entry(name.text.clone())
                            .or_insert_with(|| Some(v.text.clone()));
                        if slot.as_deref() != Some(v.text.as_str()) {
                            *slot = None;
                        }
                    }
                    Kind::Num => {
                        let val = parse_num(&v.text);
                        let slot = nums.entry(name.text.clone()).or_insert(val);
                        if *slot != val {
                            *slot = None;
                        }
                    }
                    _ => break,
                }
                if !self.punct_at(k + 3, ",") {
                    break;
                }
                k += 4;
            }
        }
        (
            strs.into_iter()
                .filter_map(|(k, v)| v.map(|v| (k, v)))
                .collect(),
            nums.into_iter()
                .filter_map(|(k, v)| v.map(|v| (k, v)))
                .collect(),
        )
    }

    /// Valor de cadena del token `k`: literal, plantilla o `const`.
    fn string_arg(&self, k: usize, strs: &HashMap<String, String>) -> Option<String> {
        let t = self.tok(k)?;
        match t.kind {
            Kind::Str | Kind::Template => Some(t.text.clone()),
            Kind::Ident => strs.get(&t.text).cloned(),
            _ => None,
        }
    }

    /// Tercer pase: rutas, DOM, almacenamiento, intervalos, mensajes.
    fn literals(&mut self) {
        let (strs, nums) = self.constants();
        let mut f = std::mem::take(&mut self.facts);
        for i in 0..self.t.len() {
            let Some(t) = self.tok(i) else { break };
            if matches!(t.kind, Kind::Str | Kind::Template) {
                f.strings += 1;
            }
            if t.kind != Kind::Ident {
                if t.is_punct(".") && self.tok(i + 1).is_some_and(|x| x.is_ident("type")) {
                    // `x.type === "y"`
                    if (self.punct_at(i + 2, "===") || self.punct_at(i + 2, "=="))
                        && let Some(v) = self.tok(i + 3).filter(|v| v.kind == Kind::Str)
                    {
                        push_unique(&mut f.message_types, v.text.clone());
                    }
                }
                continue;
            }
            let name = t.text.as_str();
            let call = self.punct_at(i + 1, "(");
            match name {
                "getElementById" if call => {
                    if let Some(id) = self.string_arg(i + 2, &strs).filter(|s| !s.contains("${}")) {
                        push_unique(&mut f.dom_ids, id);
                    }
                }
                "querySelector" | "querySelectorAll" | "closest" if call => {
                    if let Some(sel) = self.string_arg(i + 2, &strs) {
                        for id in selector_ids(&sel) {
                            push_unique(&mut f.dom_ids, id);
                        }
                    }
                }
                "$" | "$$" if call && !self.after_dot(i) => {
                    if let Some(sel) = self.string_arg(i + 2, &strs) {
                        for id in selector_ids(&sel) {
                            push_unique(&mut f.dom_ids, id);
                        }
                    }
                }
                "api" | "fetch" | "sendBeacon" if call => {
                    if let Some(r) = self.string_arg(i + 2, &strs).and_then(|s| route(&s)) {
                        push_unique(&mut f.routes, r);
                    }
                }
                "EventSource"
                    if call && i > 0 && self.tok(i - 1).is_some_and(|p| p.is_ident("new")) =>
                {
                    if let Some(r) = self.string_arg(i + 2, &strs).and_then(|s| route(&s)) {
                        push_unique(&mut f.routes, r);
                    }
                }
                "localStorage" | "sessionStorage" => {
                    let base_ok = !self.after_dot(i)
                        || (i >= 2 && self.tok(i - 2).is_some_and(|b| b.is_ident("window")));
                    if base_ok && let Some(k) = self.storage_key(i, &strs) {
                        let k = if name == "sessionStorage" {
                            format!("session:{k}")
                        } else {
                            k
                        };
                        push_unique(&mut f.storage_keys, k);
                    }
                }
                "setInterval" if call && (!self.after_dot(i) || self.window_member(i)) => {
                    if let Some(ms) = self.second_arg_ms(i + 1, &nums) {
                        f.intervals_ms.push(ms);
                    }
                }
                "postMessage" if call && self.after_dot(i) => {
                    let m = self.post_message(i, &strs);
                    push_unique(&mut f.post_messages, m);
                }
                _ => {}
            }
        }
        f.message_types.sort();
        self.facts = f;
    }

    fn window_member(&self, i: usize) -> bool {
        i >= 2
            && self.after_dot(i)
            && self
                .tok(i - 2)
                .is_some_and(|b| WINDOW_ALIASES.contains(&b.text.as_str()))
    }

    fn storage_key(&self, i: usize, strs: &HashMap<String, String>) -> Option<String> {
        if self.punct_at(i + 1, "[") {
            return self.string_arg(i + 2, strs);
        }
        if !self.punct_at(i + 1, ".") {
            return None;
        }
        let m = self.tok(i + 2)?;
        if m.kind != Kind::Ident {
            return None;
        }
        match m.text.as_str() {
            "getItem" | "setItem" | "removeItem" => self
                .string_arg(i + 4, strs)
                .filter(|_| self.punct_at(i + 3, "(")),
            "clear" | "key" | "length" => None,
            other => Some(other.to_string()),
        }
    }

    /// Segundo argumento de la llamada cuyo `(` está en `open`, como milisegundos.
    fn second_arg_ms(&self, open: usize, nums: &HashMap<String, u64>) -> Option<u64> {
        let close = self.matching(open)?;
        let mut depth = 0usize;
        let mut comma = None;
        for k in open + 1..close {
            let t = self.tok(k)?;
            if t.kind != Kind::Punct {
                continue;
            }
            match t.text.as_str() {
                "(" | "[" | "{" | "${" => depth += 1,
                ")" | "]" | "}" | "}$" => depth = depth.saturating_sub(1),
                "," if depth == 0 => {
                    comma = Some(k);
                    break;
                }
                _ => {}
            }
        }
        let start = comma? + 1;
        // Plegado de constantes: `n`, `N`, `a * b`, `a + b` (sin precedencia mixta).
        let mut acc: Option<u64> = None;
        let mut op = "";
        for k in start..close {
            let t = self.tok(k)?;
            match t.kind {
                Kind::Num | Kind::Ident => {
                    let v = if t.kind == Kind::Num {
                        parse_num(&t.text)?
                    } else {
                        *nums.get(&t.text)?
                    };
                    acc = Some(match (acc, op) {
                        (None, _) => v,
                        (Some(a), "*") => a.checked_mul(v)?,
                        (Some(a), "+") => a.checked_add(v)?,
                        _ => return None,
                    });
                }
                Kind::Punct if t.text == "*" || t.text == "+" => {
                    op = if t.text == "*" { "*" } else { "+" }
                }
                Kind::Punct if t.text == "," => break,
                _ => return None,
            }
        }
        acc
    }

    /// Descripción de `….postMessage(arg)`.
    fn post_message(&self, i: usize, strs: &HashMap<String, String>) -> String {
        // `messageHandlers.H.postMessage(…)` → puente con la app nativa.
        let handler = (i >= 4
            && self
                .tok(i - 4)
                .is_some_and(|t| t.is_ident("messageHandlers")))
        .then(|| self.tok(i - 2).map(|t| t.text.clone()))
        .flatten();
        let arg = i + 2;
        let desc = self.message_arg(arg, strs);
        let desc = desc.unwrap_or_else(|| "?".to_string());
        match handler {
            Some(h) => format!("webkit.{h}:{desc}"),
            None => desc,
        }
    }

    fn message_arg(&self, k: usize, strs: &HashMap<String, String>) -> Option<String> {
        let t = self.tok(k)?;
        if t.is_ident("JSON")
            && self.tok(k + 2).is_some_and(|s| s.is_ident("stringify"))
            && self.punct_at(k + 3, "(")
        {
            return self.message_arg(k + 4, strs);
        }
        if t.is_punct("{") {
            let close = self.matching(k)?;
            let props = self.object_props(k + 1, close);
            if let Some(ty) = props.get("type").cloned().flatten() {
                let src = props
                    .get("source")
                    .cloned()
                    .flatten()
                    .unwrap_or_else(|| "*".to_string());
                return Some(format!("{src}/{ty}"));
            }
            let body: Vec<String> = props
                .iter()
                .map(|(key, v)| match v {
                    Some(v) => format!("{key}={v}"),
                    None => key.clone(),
                })
                .collect();
            return Some(format!("{{{}}}", body.join(",")));
        }
        match t.kind {
            Kind::Str => Some(format!("'{}'", t.text)),
            Kind::Ident => strs.get(&t.text).map(|s| format!("'{s}'")),
            _ => None,
        }
    }

    /// Propiedades de primer nivel de un literal de objeto, con su valor si es cadena.
    fn object_props(&self, from: usize, to: usize) -> BTreeMap<String, Option<String>> {
        let mut out = BTreeMap::new();
        let mut depth = 0usize;
        let mut expect_key = true;
        for k in from..to {
            let Some(t) = self.tok(k) else { break };
            if t.kind == Kind::Punct {
                match t.text.as_str() {
                    "(" | "[" | "{" | "${" => depth += 1,
                    ")" | "]" | "}" | "}$" => depth = depth.saturating_sub(1),
                    "," if depth == 0 => expect_key = true,
                    _ => {}
                }
                continue;
            }
            if depth != 0 || !expect_key {
                continue;
            }
            expect_key = false;
            if !matches!(t.kind, Kind::Ident | Kind::Str) {
                continue;
            }
            let val = if self.punct_at(k + 1, ":") {
                self.tok(k + 2)
                    .filter(|v| v.kind == Kind::Str && (self.punct_at(k + 3, ",") || k + 3 == to))
                    .map(|v| v.text.clone())
            } else {
                None
            };
            out.insert(t.text.clone(), val);
        }
        out
    }
}

/// El token deja la expresión abierta: la línea siguiente la continúa.
fn ends_expression_open(t: &Token) -> bool {
    t.kind == Kind::Punct && !matches!(t.text.as_str(), ")" | "]" | "}" | "}$" | "++" | "--")
}

fn starts_continuation(t: &Token) -> bool {
    t.kind == Kind::Punct
        && matches!(
            t.text.as_str(),
            "." | "?."
                | "?"
                | ":"
                | "+"
                | "-"
                | "*"
                | "/"
                | "%"
                | "&&"
                | "||"
                | "??"
                | ","
                | "="
                | "=="
                | "==="
                | "!="
                | "!=="
                | "<"
                | ">"
                | "<="
                | ">="
        )
}

pub fn parse_num(s: &str) -> Option<u64> {
    let s = s.replace('_', "");
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(h, 16).ok();
    }
    if let Ok(v) = s.parse::<u64>() {
        return Some(v);
    }
    let f: f64 = s.parse().ok()?;
    (f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f < 1e15).then_some(f as u64)
}

/// `#id` de un selector CSS (sin los huecos `${}` de una plantilla ni los
/// `#` dentro de corchetes de atributo).
pub fn selector_ids(sel: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut attr = 0usize;
    let mut it = sel.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '[' => attr += 1,
            ']' => attr = attr.saturating_sub(1),
            '#' if attr == 0 => {
                let mut id = String::new();
                while let Some(&n) = it.peek() {
                    if n.is_alphanumeric() || n == '-' || n == '_' {
                        id.push(n);
                        it.next();
                    } else {
                        break;
                    }
                }
                let hole = it.peek() == Some(&'$');
                if !id.is_empty() && !hole && id.chars().next().is_some_and(|c| !c.is_ascii_digit())
                {
                    out.push(id);
                }
            }
            _ => {}
        }
    }
    out
}

/// Ruta normalizada: sin consulta; los huecos de plantilla quedan como `${}`.
pub fn route(s: &str) -> Option<String> {
    let path = s.split('?').next().unwrap_or("").trim();
    if path.is_empty() || path == "${}" || path.contains(char::is_whitespace) {
        return None;
    }
    Some(path.to_string())
}

/// Conjunto de nombres leídos (libres o por `window.`) en un trozo de JS.
pub fn referenced(f: &Facts) -> BTreeSet<String> {
    f.uses.iter().map(|u| u.name.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[(String, Binding)]) -> Vec<&str> {
        v.iter().map(|(n, _)| n.as_str()).collect()
    }

    #[test]
    fn top_level_declarations_only() {
        let f = analyze(
            "const a = 1, {b, c: d} = x\nlet e\nfunction g(h){ const i = h }\n(function(){ var j })()\nclass K {}",
        );
        assert_eq!(names(&f.defines), ["a", "b", "d", "e", "g", "K"]);
    }

    #[test]
    fn asi_ends_initializer() {
        let f = analyze("let a = 1\nfoo(), bar\nconst z = 2");
        assert_eq!(names(&f.defines), ["a", "z"]);
        assert!(f.uses.iter().any(|u| u.name == "foo"));
        assert!(f.uses.iter().any(|u| u.name == "bar"));
    }

    #[test]
    fn params_and_shadowing() {
        let f = analyze(
            "function f({ api, toast = () => {} }) { api() }\nconst g = (x, y) => x + y + z\nlist.map(q => q.id)\napi()",
        );
        let uses: Vec<&str> = f.uses.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(uses, ["z", "list", "api"], "solo el último api es libre");
    }

    #[test]
    fn object_keys_methods_and_members() {
        let f = analyze(
            "const o = { esc: 1, render() { return esc2 }, ok }\nobj.esc()\nwindow.boot = 1\nwindow.boot2()",
        );
        let uses: Vec<&str> = f
            .uses
            .iter()
            .map(|u| u.name.as_str())
            .filter(|n| *n != "window")
            .collect();
        assert_eq!(uses, ["esc2", "ok", "obj", "boot2"]);
        assert!(f.uses.iter().any(|u| u.name == "boot2" && u.member));
        assert!(f.defines.contains(&("boot".to_string(), Binding::Window)));
    }

    #[test]
    fn assignments_to_free_names() {
        let f = analyze("x = 1; y += 2; z++; --w; let v; v = 3; a.b = 4; c == d");
        assert_eq!(f.assigns, ["x", "y", "z", "w"]);
    }

    #[test]
    fn selectors_and_routes() {
        assert_eq!(
            selector_ids("#a .b > #c-d, [href='#x'], #${}"),
            ["a", "c-d"]
        );
        assert_eq!(route("/state?x=1").as_deref(), Some("/state"));
        assert_eq!(route("/x/${}/y").as_deref(), Some("/x/${}/y"));
        assert_eq!(route("${}"), None);
    }

    #[test]
    fn literals_from_calls() {
        let f = analyze(
            "const K = 'k1', MS = 500\nlocalStorage.getItem(K); localStorage.cc_token; sessionStorage['s']\n\
             setInterval(f, MS); setInterval(g, 2 * 1000); setInterval(h, dyn)\n\
             frame.contentWindow.postMessage({source: 'comandos', type: 'select-pane', pane}, '*')\n\
             window.webkit.messageHandlers.centro.postMessage(JSON.stringify({headerAction: 'chains'}))\n\
             new EventSource(`/events/${id}?x`); if (e.data.type === 'theme') {}",
        );
        assert_eq!(f.storage_keys, ["k1", "cc_token", "session:s"]);
        assert_eq!(f.intervals_ms, [500, 2000]);
        assert_eq!(
            f.post_messages,
            [
                "comandos/select-pane",
                "webkit.centro:{headerAction=chains}"
            ]
        );
        assert_eq!(f.routes, ["/events/${}"]);
        assert_eq!(f.message_types, ["theme"]);
    }
}
