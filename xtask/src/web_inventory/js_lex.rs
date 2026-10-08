//! Tokenizador léxico mínimo de JavaScript para el inventario (B3).
//!
//! No es un parser: separa identificadores, literales, números, puntuación y
//! expresiones regulares para que el texto de cadenas, plantillas y
//! comentarios nunca se confunda con código. Las expresiones `${…}` de una
//! plantilla se tokenizan como código, rodeadas por los centinelas `${` y `}$`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ident,
    /// Cadena entre comillas; `text` es su valor (escapes simples resueltos).
    Str,
    /// Plantilla; `text` es su parte estática con `${}` en cada hueco.
    Template,
    Num,
    Punct,
    Regex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub text: String,
    /// Línea (1-based) dentro del texto tokenizado.
    pub line: usize,
}

impl Token {
    pub fn is_punct(&self, p: &str) -> bool {
        self.kind == Kind::Punct && self.text == p
    }
    pub fn is_ident(&self, s: &str) -> bool {
        self.kind == Kind::Ident && self.text == s
    }
}

/// Puntuación de varios caracteres, la más larga primero.
const PUNCTS: &[&str] = &[
    ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "=>", "==", "!=",
    "<=", ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=",
    "**", "<<", ">>",
];

/// Palabras tras las que `/` abre una expresión regular.
const REGEX_AFTER: &[&str] = &[
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "do",
    "else",
    "yield",
    "await",
];

pub fn lex(src: &str) -> Vec<Token> {
    let mut lx = Lexer {
        c: src.chars().collect(),
        i: 0,
        line: 1,
        out: Vec::new(),
    };
    lx.run(false);
    lx.out
}

struct Lexer {
    c: Vec<char>,
    i: usize,
    line: usize,
    out: Vec<Token>,
}

fn ident_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphabetic()
}

fn ident_char(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphanumeric()
}

impl Lexer {
    fn at(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }

    fn push(&mut self, kind: Kind, text: String, line: usize) {
        self.out.push(Token { kind, text, line });
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.at(0)?;
        self.i += 1;
        if ch == '\n' {
            self.line += 1;
        }
        Some(ch)
    }

    /// `/` abre regex si el token anterior no termina una expresión.
    fn regex_allowed(&self) -> bool {
        match self.out.last() {
            None => true,
            Some(t) => match t.kind {
                Kind::Ident => REGEX_AFTER.contains(&t.text.as_str()),
                Kind::Punct => !matches!(t.text.as_str(), ")" | "]" | "}" | "}$"),
                _ => false,
            },
        }
    }

    /// Tokeniza hasta el final, o hasta la `}` que cierra un `${` si
    /// `in_template` (la consume y vuelve).
    fn run(&mut self, in_template: bool) {
        let mut depth = 0usize;
        while let Some(ch) = self.at(0) {
            let line = self.line;
            if ch.is_whitespace() {
                self.bump();
            } else if ch == '/' && self.at(1) == Some('/') {
                while let Some(c) = self.at(0) {
                    if c == '\n' {
                        break;
                    }
                    self.bump();
                }
            } else if ch == '/' && self.at(1) == Some('*') {
                self.bump();
                self.bump();
                while let Some(c) = self.at(0) {
                    if c == '*' && self.at(1) == Some('/') {
                        self.bump();
                        self.bump();
                        break;
                    }
                    self.bump();
                }
            } else if ch == '\'' || ch == '"' {
                let s = self.string(ch);
                self.push(Kind::Str, s, line);
            } else if ch == '`' {
                self.template(line);
            } else if ch.is_ascii_digit()
                || (ch == '.' && self.at(1).is_some_and(|d| d.is_ascii_digit()))
            {
                let mut s = String::new();
                while let Some(c) = self.at(0) {
                    let exp_sign = (c == '+' || c == '-')
                        && s.ends_with(['e', 'E'])
                        && !s.starts_with("0x")
                        && !s.starts_with("0X");
                    if c.is_alphanumeric() || c == '.' || c == '_' || exp_sign {
                        s.push(c);
                        self.bump();
                    } else {
                        break;
                    }
                }
                self.push(Kind::Num, s, line);
            } else if ident_start(ch) || (ch == '#' && self.at(1).is_some_and(ident_start)) {
                let mut s = String::new();
                s.push(ch);
                self.bump();
                while let Some(c) = self.at(0) {
                    if !ident_char(c) {
                        break;
                    }
                    s.push(c);
                    self.bump();
                }
                self.push(Kind::Ident, s, line);
            } else if ch == '/' && self.regex_allowed() {
                let s = self.regex();
                self.push(Kind::Regex, s, line);
            } else {
                if in_template {
                    if ch == '{' {
                        depth += 1;
                    } else if ch == '}' {
                        if depth == 0 {
                            self.bump();
                            self.push(Kind::Punct, "}$".to_string(), line);
                            return;
                        }
                        depth -= 1;
                    }
                }
                let p = self.punct();
                self.push(Kind::Punct, p, line);
            }
        }
    }

    fn punct(&mut self) -> String {
        for p in PUNCTS {
            let n = p.chars().count();
            let matches = p.chars().enumerate().all(|(k, pc)| self.at(k) == Some(pc));
            // `?.5` es un ternario con un número, no un acceso opcional.
            let bad_optional = *p == "?." && self.at(2).is_some_and(|d| d.is_ascii_digit());
            if matches && !bad_optional {
                for _ in 0..n {
                    self.bump();
                }
                return (*p).to_string();
            }
        }
        self.bump().map(String::from).unwrap_or_default()
    }

    fn string(&mut self, q: char) -> String {
        self.bump();
        let mut s = String::new();
        while let Some(c) = self.at(0) {
            if c == q {
                self.bump();
                break;
            }
            if c == '\n' {
                break; // cadena sin cerrar: se corta en la línea
            }
            if c == '\\' {
                self.bump();
                match self.bump() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('\n') => {}
                    Some(o) => s.push(o),
                    None => break,
                }
                continue;
            }
            s.push(c);
            self.bump();
        }
        s
    }

    fn template(&mut self, line: usize) {
        self.bump();
        let mut s = String::new();
        let mut holes: Vec<Vec<Token>> = Vec::new();
        while let Some(c) = self.at(0) {
            if c == '`' {
                self.bump();
                break;
            }
            if c == '\\' {
                self.bump();
                if let Some(o) = self.bump() {
                    s.push(o);
                }
                continue;
            }
            if c == '$' && self.at(1) == Some('{') {
                self.bump();
                self.bump();
                s.push_str("${}");
                // El hueco se tokeniza aparte y se emite tras la plantilla.
                let outer = std::mem::take(&mut self.out);
                self.push(Kind::Punct, "${".to_string(), self.line);
                self.run(true);
                holes.push(std::mem::replace(&mut self.out, outer));
                continue;
            }
            s.push(c);
            self.bump();
        }
        self.push(Kind::Template, s, line);
        for h in holes {
            self.out.extend(h);
        }
    }

    fn regex(&mut self) -> String {
        let mut s = String::new();
        if let Some(c) = self.bump() {
            s.push(c);
        }
        let mut class = false;
        while let Some(c) = self.at(0) {
            if c == '\n' {
                break;
            }
            self.bump();
            s.push(c);
            if c == '\\' {
                if let Some(o) = self.bump() {
                    s.push(o);
                }
                continue;
            }
            if c == '[' {
                class = true;
            } else if c == ']' {
                class = false;
            } else if c == '/' && !class {
                break;
            }
        }
        while let Some(c) = self.at(0) {
            if !c.is_ascii_alphabetic() {
                break;
            }
            s.push(c);
            self.bump();
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(Kind, String)> {
        lex(src).into_iter().map(|t| (t.kind, t.text)).collect()
    }

    #[test]
    fn strings_and_comments_hide_code() {
        let t = kinds("a('x(y)') // b(\n/* c( */ d");
        let ids: Vec<&str> = t
            .iter()
            .filter(|(k, _)| *k == Kind::Ident)
            .map(|(_, s)| s.as_str())
            .collect();
        assert_eq!(ids, ["a", "d"]);
    }

    #[test]
    fn template_holes_are_code() {
        let t = kinds("`a ${f(`b ${g}`)} c`");
        assert_eq!(t[0], (Kind::Template, "a ${} c".to_string()));
        let ids: Vec<&str> = t
            .iter()
            .filter(|(k, _)| *k == Kind::Ident)
            .map(|(_, s)| s.as_str())
            .collect();
        assert_eq!(ids, ["f", "g"]);
    }

    #[test]
    fn regex_versus_division() {
        let t = kinds("x = a / 2; y = /\"[/]'/g.test(s); return /a/");
        assert!(t.contains(&(Kind::Punct, "/".to_string())));
        assert!(t.contains(&(Kind::Regex, "/\"[/]'/g".to_string())));
        assert!(t.contains(&(Kind::Regex, "/a/".to_string())));
        assert!(!t.iter().any(|(k, _)| *k == Kind::Str));
    }

    #[test]
    fn numbers_and_lines() {
        let t = lex("1e3\n0x1F 60_000 .5");
        let n: Vec<&str> = t.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(n, ["1e3", "0x1F", "60_000", ".5"]);
        assert_eq!(t[1].line, 2);
    }
}
