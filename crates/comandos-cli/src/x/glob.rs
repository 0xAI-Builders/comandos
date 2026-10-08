//! Character glob matching: simple lowercase is distinct from Unicode case folding.
use std::{collections::VecDeque, sync::OnceLock};

pub(super) struct Pattern {
    wide: Vec<Token>,
    bytes: Vec<Token>,
}
#[derive(Clone, Copy)]
enum Mode {
    Wide,
    Bytes,
}
impl Mode {
    fn lower(self, c: char) -> char {
        match self {
            Self::Wide => c.to_lowercase().next().unwrap_or(c),
            Self::Bytes => c.to_ascii_lowercase(),
        }
    }
}
enum Token {
    Star,
    Any,
    Literal(char),
    Set { negated: bool, items: Vec<Item> },
}
enum Item {
    Literal(char),
    Range(char, char),
    Class(Class),
}
enum Class {
    Alpha,
    Alnum,
    Digit,
    Upper,
    Lower,
    Blank,
    Space,
    Punct,
    Graph,
    Print,
    Control,
    Hex,
}
impl Class {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "alpha" => Self::Alpha,
            "alnum" => Self::Alnum,
            "digit" => Self::Digit,
            "upper" => Self::Upper,
            "lower" => Self::Lower,
            "blank" => Self::Blank,
            "space" => Self::Space,
            "punct" => Self::Punct,
            "graph" => Self::Graph,
            "print" => Self::Print,
            "cntrl" => Self::Control,
            "xdigit" => Self::Hex,
            _ => return None,
        })
    }
    fn matches(&self, c: char, mode: Mode) -> bool {
        if matches!(mode, Mode::Bytes) {
            return match self {
                Self::Alpha => c.is_ascii_alphabetic(),
                Self::Alnum => c.is_ascii_alphanumeric(),
                Self::Digit => c.is_ascii_digit(),
                Self::Upper => c.is_ascii_uppercase(),
                Self::Lower => c.is_ascii_lowercase(),
                Self::Space => c.is_ascii_whitespace() || c == '\u{b}',
                Self::Blank => matches!(c, ' ' | '\t'),
                Self::Control => c.is_ascii_control(),
                Self::Print => c.is_ascii_graphic() || c == ' ',
                Self::Graph => c.is_ascii_graphic(),
                Self::Punct => c.is_ascii_punctuation(),
                Self::Hex => c.is_ascii_hexdigit(),
            };
        }
        // GNU C.UTF-8 predicates apply before case conversion. In particular,
        // -iname does not merge [:upper:] and [:lower:], or expand [:digit:].
        match self {
            Self::Alpha => alpha(c),
            Self::Alnum => alpha(c) || c.is_ascii_digit(),
            Self::Digit => c.is_ascii_digit(),
            Self::Upper => c.is_uppercase(),
            Self::Lower => c.is_lowercase(),
            Self::Space => space(c),
            Self::Blank => {
                space(c) && !matches!(c, '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{2028}' | '\u{2029}')
            }
            Self::Control => control(c),
            Self::Print => printable(c),
            Self::Graph => printable(c) && !space(c),
            Self::Punct => printable(c) && !space(c) && !alpha(c) && !c.is_ascii_digit(),
            Self::Hex => c.is_ascii_hexdigit(),
        }
    }
}
fn alpha(c: char) -> bool {
    static DECIMAL: OnceLock<Option<regex::Regex>> = OnceLock::new();
    c.is_alphabetic()
        || (!c.is_ascii()
            && property(
                c,
                DECIMAL.get_or_init(|| regex::Regex::new(r"\A\p{Nd}\z").ok()),
            ))
}
fn space(c: char) -> bool {
    c.is_whitespace() && !matches!(c, '\u{85}' | '\u{a0}' | '\u{2007}' | '\u{202f}')
}
fn control(c: char) -> bool {
    c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')
}
fn printable(c: char) -> bool {
    static ASSIGNED: OnceLock<Option<regex::Regex>> = OnceLock::new();
    !control(c)
        && property(
            c,
            ASSIGNED.get_or_init(|| regex::Regex::new(r"\A\P{Cn}\z").ok()),
        )
}
fn property(c: char, predicate: &Option<regex::Regex>) -> bool {
    let mut buffer = [0u8; 4];
    predicate
        .as_ref()
        .is_some_and(|regex| regex.is_match(c.encode_utf8(&mut buffer)))
}
impl Pattern {
    pub(super) fn compile(pattern: &str) -> Option<Self> {
        Some(Self {
            wide: Self::tokens(&pattern.chars().collect::<Vec<_>>())?,
            bytes: Self::tokens(&pattern.bytes().map(char::from).collect::<Vec<_>>())?,
        })
    }
    fn tokens(chars: &[char]) -> Option<Vec<Token>> {
        let mut tokens = Vec::new();
        let mut index = 0;
        while let Some(&c) = chars.get(index) {
            match c {
                '*' => {
                    if !matches!(tokens.last(), Some(Token::Star)) {
                        tokens.push(Token::Star);
                    }
                }
                '?' => tokens.push(Token::Any),
                '\\' => {
                    index += 1;
                    tokens.push(Token::Literal(*chars.get(index)?));
                }
                '[' => match bracket(chars, index + 1)? {
                    Some((token, end)) => {
                        tokens.push(token);
                        index = end;
                    }
                    None => tokens.push(Token::Literal('[')),
                },
                _ => tokens.push(Token::Literal(c)),
            }
            index += 1;
        }
        Some(tokens)
    }
    pub(super) fn is_match(&self, name: &str) -> bool {
        // GNU fnmatch tries its multibyte matcher and then its byte matcher.
        // Consequently both ? and ?? can match É; byte classes stay ASCII.
        Self::matches(&self.wide, &name.chars().collect::<Vec<_>>(), Mode::Wide)
            || Self::matches(
                &self.bytes,
                &name.bytes().map(char::from).collect::<Vec<_>>(),
                Mode::Bytes,
            )
    }
    fn matches(tokens: &[Token], chars: &[char], mode: Mode) -> bool {
        let mut previous = vec![false; chars.len() + 1];
        previous[0] = true;
        for token in tokens {
            let mut next = vec![false; previous.len()];
            if matches!(token, Token::Star) {
                let mut reachable = false;
                for (target, &source) in next.iter_mut().zip(&previous) {
                    reachable |= source;
                    *target = reachable;
                }
            } else {
                for ((target, &source), &c) in next.iter_mut().skip(1).zip(&previous).zip(chars) {
                    *target = source && token.matches(c, mode);
                }
            }
            previous = next;
        }
        previous.last().copied().unwrap_or(false)
    }
}
impl Token {
    fn matches(&self, c: char, mode: Mode) -> bool {
        match self {
            Self::Any | Self::Star => true,
            Self::Literal(wanted) => mode.lower(c) == mode.lower(*wanted),
            Self::Set { negated, items } => {
                items.iter().any(|item| match item {
                    Item::Literal(wanted) => mode.lower(c) == mode.lower(*wanted),
                    Item::Range(first, last) => {
                        (mode.lower(*first)..=mode.lower(*last)).contains(&mode.lower(c))
                    }
                    Item::Class(class) => class.matches(c, mode),
                }) != *negated
            }
        }
    }
}

struct Atom {
    item: Item,
    separator: bool,
}
fn bracket(chars: &[char], mut index: usize) -> Option<Option<(Token, usize)>> {
    let negated = chars.get(index).is_some_and(|c| matches!(c, '!' | '^'));
    index += usize::from(negated);
    let mut atoms = Vec::new();
    if chars.get(index) == Some(&']') {
        atoms.push(Atom {
            item: Item::Literal(']'),
            separator: false,
        });
        index += 1;
    }
    while let Some(&c) = chars.get(index) {
        if c == ']' {
            let mut items = Vec::new();
            let mut atoms = VecDeque::from(atoms);
            while let Some(atom) = atoms.pop_front() {
                if let Item::Literal(first) = atom.item {
                    if atoms.front().is_some_and(|atom| atom.separator) {
                        // A trailing '-' remains literal; class endpoints are not ranges.
                        if let Some(Atom {
                            item: Item::Literal(last),
                            ..
                        }) = atoms.get(1)
                        {
                            let last = *last;
                            atoms.pop_front();
                            atoms.pop_front();
                            items.push(Item::Range(first, last));
                            continue;
                        }
                    }
                    items.push(Item::Literal(first));
                } else {
                    items.push(atom.item);
                }
            }
            return Some(Some((Token::Set { negated, items }, index)));
        }
        let (item, separator) = if c == '\\' {
            index += 1;
            let Some(&escaped) = chars.get(index) else {
                return Some(None);
            };
            (Item::Literal(escaped), false)
        } else if c == '[' && chars.get(index + 1) == Some(&':') {
            let start = index + 2;
            let mut end = start;
            while let Some(&c) = chars.get(end) {
                if c == ':' && chars.get(end + 1) == Some(&']') {
                    break;
                }
                end += 1;
            }
            if chars.get(end).is_none() {
                // Without a :], GNU treats the inner '[' as an ordinary set
                // member rather than starting a POSIX class.
                (Item::Literal(c), false)
            } else {
                let name = chars.get(start..end)?.iter().collect::<String>();
                index = end + 1;
                (Item::Class(Class::parse(&name)?), false)
            }
        } else {
            (Item::Literal(c), c == '-')
        };
        atoms.push(Atom { item, separator });
        index += 1;
    }
    Some(None)
}

#[cfg(test)]
mod tests {
    use super::Pattern;
    use std::{collections::BTreeSet, fs, path::PathBuf, process::Command};
    struct Home(PathBuf);
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn every_class_and_bracket_match_agrees_with_gnu_not_just_the_first_directory() {
        let home =
            Home(std::env::temp_dir().join(format!("comandos-x-glob-{}", std::process::id())));
        let base = home.0.join("codebase");
        fs::create_dir_all(&base).unwrap();
        let names = [
            "É",
            "é",
            "İ",
            "ı",
            "K",
            "K",
            "Σ",
            "σ",
            "ς",
            "ß",
            "ẞ",
            "ſ",
            "Ⅻ",
            "١",
            "²",
            "\u{0345}",
            "\u{0301}",
            "\u{200d}",
            "\u{a0}",
            "\u{2003}",
            "\u{2007}",
            "\u{202f}",
            "\u{85}",
            "\u{2028}",
            "\u{2029}",
            "\u{378}",
            "\u{e000}",
            " ",
            "\t",
            "\n",
            "1",
            "A",
            "a",
            "b",
            "z",
            "]",
            "-",
            "^",
            "[",
            "&",
            "~",
            "?",
            "\\",
            "[]",
            "[!]",
            "[^]",
            "[[:digit:]",
            "[[:digit]]",
            "a]",
            "e\u{301}",
            "界",
            "école",
            "ſchool",
        ];
        for name in names {
            fs::create_dir(base.join(name)).unwrap();
        }
        let patterns = [
            "i",
            "k",
            "σ",
            "ς",
            "s",
            "[A-Z]",
            "[a-Z]",
            "[Z-a]",
            "[z-A]",
            "[É-é]",
            "[z-a]",
            "[!z-a]",
            "[]a]",
            "[!]]",
            "[^]]",
            "[-a]",
            "[a-]",
            "[a\\-z]",
            "[a-b-c]",
            "[a--]",
            "[--a]",
            "[a[:digit:]]",
            "[[:alpha:]]",
            "[[:alnum:]]",
            "[[:digit:]]",
            "[[:upper:]]",
            "[[:lower:]]",
            "[[:blank:]]",
            "[[:space:]]",
            "[[:punct:]]",
            "[[:graph:]]",
            "[[:print:]]",
            "[[:cntrl:]]",
            "[[:xdigit:]]",
            "[[:unknown:]]",
            "[![:unknown:]]",
            "[[:digit]]",
            "[[:digit:]",
            "[]",
            "[!]",
            "[^]",
            "\\",
            "[\\]",
            "[",
            "[?]",
            "?",
            "??",
            "*?*",
            "**",
            "e?",
            "é*",
            "[É]*",
            "[é]*",
            "[!É]*",
            "[[:alpha:]]?",
            "[[:alpha:]][[:alpha:]]",
            "[a-z]??",
            "é??",
            "??é",
            "*school*",
            "*\\?*",
            "*\\*",
            "*[[]*",
            "*]*",
        ];
        for pattern in patterns {
            let oracle = Command::new("/usr/bin/find")
                .arg(&base)
                .args([
                    "-mindepth",
                    "1",
                    "-maxdepth",
                    "1",
                    "-type",
                    "d",
                    "-iname",
                    pattern,
                    "-printf",
                    "%f\\0",
                ])
                .current_dir(&home.0)
                .env_clear()
                .env("HOME", &home.0)
                .env("PWD", &home.0)
                .env("PATH", "/usr/bin:/bin")
                .env("LC_ALL", "C.UTF-8")
                .env("TMUX_TMPDIR", home.0.join("private-tmux"))
                .env("X_TEST_SOCKET", home.0.join("private-tmux/socket"))
                .output()
                .unwrap();
            assert!(
                oracle.status.success(),
                "{}",
                String::from_utf8_lossy(&oracle.stderr)
            );
            assert!(oracle.stderr.is_empty());
            let expected = oracle
                .stdout
                .split(|c| *c == 0)
                .filter(|name| !name.is_empty())
                .map(|name| String::from_utf8(name.to_vec()).unwrap())
                .collect::<BTreeSet<_>>();
            let compiled = Pattern::compile(pattern);
            let actual = names
                .into_iter()
                .filter(|name| {
                    compiled
                        .as_ref()
                        .is_some_and(|pattern| pattern.is_match(name))
                })
                .map(str::to_owned)
                .collect::<BTreeSet<_>>();
            assert_eq!(actual, expected, "pattern={pattern:?}");
        }
    }
}
