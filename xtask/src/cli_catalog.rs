//! Native replacements for the frozen CLI catalogue development scripts.
use comandos_core::json::{indent_dumps, parse_value, response_dumps_unicode};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

fn whitespace(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | '\u{001c}'..='\u{0020}' | '\u{0085}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}
const SPACE: &str = r"[\x09-\x0d\x1c-\x20\x{85}\x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}]";

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_value(&text).map_err(|e| format!("{}: {e}", path.display()))
}
fn object(value: &Value) -> Result<&Map<String, Value>, String> {
    value.as_object().ok_or_else(|| "expected object".into())
}
fn array(value: &Value) -> Result<&Vec<Value>, String> {
    value.as_array().ok_or_else(|| "expected array".into())
}
fn string(value: &Value) -> Result<&str, String> {
    value.as_str().ok_or_else(|| "expected string".into())
}
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value, String> {
    object(value)?
        .get(key)
        .ok_or_else(|| format!("missing {key}"))
}
fn pair(value: &Value) -> Result<(&str, &str), String> {
    let values = array(value)?;
    if let [name, description] = values.as_slice() {
        Ok((string(name)?, string(description)?))
    } else {
        Err("command must contain exactly name and description".into())
    }
}

fn arguments(cli: &str, name: &str) -> Option<Value> {
    match (cli, name) {
        ("claude", "model") => Some(
            json!({"argsFrom":"models","args":["claude-fable-5-1","claude-opus-5-5","claude-sonnet-5-5"]}),
        ),
        ("claude", "effort") => Some(json!({"args":["low","medium","high","max"]})),
        ("grok", "model") => Some(json!({"argsFrom":"models","args":["grok-4.6","grok-4.5"]})),
        ("grok" | "agy", "effort") => Some(json!({"args":["low","high"]})),
        ("claude" | "agy", "add-dir") | ("codex", "cd") => Some(json!({})),
        _ => None,
    }
}

pub fn build_bytes(root: &Path) -> Result<Vec<u8>, String> {
    let old = read_json(&root.join("config/cli-commands.json"))?;
    let old_clis = array(field(&old, "clis")?)?;
    let mut by_id = HashMap::new();
    for cli in old_clis {
        by_id.insert(string(field(cli, "id")?)?, cli);
    }
    // Python dot excludes LF, and its end anchor permits a final LF.
    let clean =
        Regex::new(&format!(r"{SPACE}*\(currently .*\){SPACE}*\z")).map_err(|e| e.to_string())?;
    let mut clis = Vec::new();
    for old_cli in old_clis {
        let id = string(field(old_cli, "id")?)?;
        let scraped = read_json(
            &root
                .join("tools/cli-commands/scraped")
                .join(format!("{id}.json")),
        )?;
        let mut commands = Vec::new();
        for value in array(field(&scraped, "commands")?)? {
            let (mut name, description) = pair(value)?;
            if id == "grok" {
                match name {
                    "m" => name = "model",
                    "t" => continue,
                    _ => {}
                }
            }
            let extra = arguments(id, name);
            let text = format!("/{name}{}", if extra.is_some() { " " } else { "" });
            let mut command =
                json!({"text":text,"description":clean.replace_all(description, "").as_ref()});
            if let Some(extra) = extra {
                object(&extra)?.iter().for_each(|(key, value)| {
                    command
                        .as_object_mut()
                        .unwrap()
                        .insert(key.clone(), value.clone());
                });
            }
            commands.push(command);
        }
        // Stable ties preserve duplicate aliases, as Python's list.sort does.
        commands.sort_by(|a, b| a["text"].as_str().cmp(&b["text"].as_str()));
        let metadata = by_id
            .get(id)
            .ok_or_else(|| format!("missing metadata {id}"))?;
        let mut cli: Map<_, _> = object(metadata)?
            .iter()
            .filter(|(key, _)| key.as_str() != "groups" && key.as_str() != "launch")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        cli.insert("pinnedVersion".into(), field(&scraped, "version")?.clone());
        cli.insert("verified".into(), Value::Bool(true));
        cli.insert(
            "groups".into(),
            json!([{"title":"","icon":"","commands":commands}]),
        );
        clis.push(Value::Object(cli));
    }
    let result =
        json!({"version":1,"source":"tools/cli-commands/scraped (menú / de cada CLI)","clis":clis});
    Ok(format!("{}\n", indent_dumps(&result, 2, false)?).into_bytes())
}

pub fn filter_bytes(commands: &Value, binary: &[u8]) -> Result<String, String> {
    let suffix =
        Regex::new(&format!(r"{SPACE}{{2,}}built-in{SPACE}*\z")).map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for value in array(commands)? {
        let (name, description) = pair(value)?;
        let stripped = suffix.replace_all(description, "");
        let description = stripped.trim_matches(whitespace);
        let probe = description
            .chars()
            .take(40)
            .collect::<String>()
            .into_bytes();
        if !probe.is_empty() && binary.windows(probe.len()).any(|window| window == probe) {
            result.push(json!([name, description]));
        }
    }
    Ok(format!(
        "{}\n",
        response_dumps_unicode(&Value::Array(result))?
    ))
}

/// Injected interaction/clock. The implementation never sends Enter.
pub trait Terminal {
    fn send(&mut self, keys: &[String]) -> Result<(), String>;
    fn capture(&mut self) -> Result<String, String>;
    fn wait(&mut self, duration: Duration);
}

/// The required explicit -L socket is included in every invocation.
pub struct CommandTerminal {
    program: PathBuf,
    socket: String,
    session: String,
}
impl CommandTerminal {
    pub fn new(program: PathBuf, socket: String, session: String) -> Result<Self, String> {
        if socket.is_empty() || session.is_empty() {
            return Err("socket and session must be explicit".into());
        }
        Ok(Self {
            program,
            socket,
            session,
        })
    }
    fn command(&self, operation: &str) -> Command {
        let mut command = Command::new(&self.program);
        command.args(["-L", &self.socket, operation]);
        command
    }
}
impl Terminal for CommandTerminal {
    fn send(&mut self, keys: &[String]) -> Result<(), String> {
        // Original scripts ignore the child's status; a spawn failure still fails.
        self.command("send-keys")
            .args(["-t", &self.session])
            .args(keys)
            .status()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn capture(&mut self) -> Result<String, String> {
        let result = self
            .command("capture-pane")
            .args(["-p", "-J", "-t", &self.session])
            .output()
            .map_err(|e| e.to_string())?;
        String::from_utf8(result.stdout)
            .map(|text| text.replace("\r\n", "\n").replace('\r', "\n"))
            .map_err(|e| e.to_string())
    }
    fn wait(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

// Freeze the actual UCD13 word class, rather than Rust regex's newer Unicode.
fn menu_regex(pattern: &str) -> Result<Regex, String> {
    let mut word = String::from("[_");
    for (start, end) in crate::css_orphans::python_alnum_ranges() {
        word.push_str(&format!(r"\x{{{start:x}}}"));
        if end != start {
            word.push_str(&format!(r"-\x{{{end:x}}}"));
        }
    }
    word.push(']');
    let mut translated = String::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            translated.push(c);
            continue;
        }
        let Some(next) = chars.next() else {
            return Err("trailing regex escape".into());
        };
        match next {
            'w' => translated.push_str(&word),
            'W' => {
                translated.push_str("[^");
                translated.push_str(&word[1..]);
            }
            's' => translated.push_str(SPACE),
            'S' => {
                translated.push_str("[^");
                translated.push_str(&SPACE[1..]);
            }
            // These require Python-specific digit/boundary semantics. Reject
            // them before any terminal interaction instead of guessing.
            'd' | 'D' | 'b' | 'B' => {
                return Err(format!(
                    "unsupported Python regex escape \\{next}; use explicit menu classes"
                ));
            }
            _ => {
                translated.push('\\');
                translated.push(next);
            }
        }
    }
    Regex::new(&translated).map_err(|e| format!("unsupported menu regex: {e}"))
}

fn python_lines(text: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if matches!(
            c,
            '\n' | '\r'
                | '\u{000b}'
                | '\u{000c}'
                | '\u{001c}'
                | '\u{001d}'
                | '\u{001e}'
                | '\u{0085}'
                | '\u{2028}'
                | '\u{2029}'
        ) {
            result.push(&text[start..at]);
            start = at + c.len_utf8();
            if c == '\r' && chars.peek().is_some_and(|(_, next)| *next == '\n') {
                let (at, c) = chars.next().unwrap();
                start = at + c.len_utf8();
            }
        }
    }
    if start < text.len() {
        result.push(&text[start..]);
    }
    result
}

fn clear_prefix(terminal: &mut impl Terminal) -> Result<(), String> {
    terminal.send(&["C-u".into()])?;
    terminal.wait(Duration::from_millis(150));
    Ok(())
}
fn walk(
    terminal: &mut impl Terminal,
    regex: &Regex,
    lead: &str,
    prefix: &str,
    depth: u8,
    maximum: i64,
    seen: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    clear_prefix(terminal)?;
    terminal.send(&["-l".into(), format!("{lead}{prefix}")])?;
    terminal.wait(Duration::from_millis(600));
    let capture = terminal.capture()?;
    let mut found = BTreeMap::new();
    for line in python_lines(&capture) {
        if let Some(groups) = regex.captures(line) {
            if groups.get(0).is_none_or(|m| m.start() != 0) {
                continue;
            }
            let name = groups
                .get(1)
                .ok_or("menu regex requires capture 1")?
                .as_str();
            if name.starts_with(prefix) {
                let description = groups
                    .get(2)
                    .ok_or("menu regex requires capture 2")?
                    .as_str()
                    .trim_matches(whitespace);
                found.insert(name.to_owned(), description.to_owned());
            }
        }
    }
    let count = i64::try_from(found.len()).map_err(|e| e.to_string())?;
    seen.extend(found);
    if count >= maximum && depth < 4 {
        for c in ('a'..='z').chain(std::iter::once('-')) {
            walk(
                terminal,
                regex,
                lead,
                &format!("{prefix}{c}"),
                depth + 1,
                maximum,
                seen,
            )?;
        }
    }
    Ok(())
}
pub fn scrape_prefix(
    terminal: &mut impl Terminal,
    pattern: &str,
    lead: &str,
    maximum: i64,
) -> Result<String, String> {
    let regex = menu_regex(pattern)?;
    let mut seen = BTreeMap::new();
    for c in 'a'..='z' {
        walk(
            terminal,
            &regex,
            lead,
            &c.to_string(),
            1,
            maximum,
            &mut seen,
        )?;
    }
    clear_prefix(terminal)?;
    let rows = seen
        .into_iter()
        .map(|(name, description)| json!([name, description]))
        .collect();
    Ok(format!(
        "{}\n",
        response_dumps_unicode(&Value::Array(rows))?
    ))
}
fn clear_name(terminal: &mut impl Terminal) -> Result<(), String> {
    terminal.send(&vec!["BSpace".into(); 40])?;
    terminal.wait(Duration::from_millis(200));
    Ok(())
}
pub fn verify_names(terminal: &mut impl Terminal, names: &[String]) -> Result<String, String> {
    let regex = menu_regex(r"^\s*[❯›]?\s*/([a-z][\w:.-]*)\s{2,}(\S.*?)\s*$")?;
    let mut result = Map::new();
    for name in names.iter().collect::<BTreeSet<_>>() {
        let prompt = menu_regex(&format!(r"(?m)[❯›]\s/{}\s*$", regex::escape(name)))?;
        clear_name(terminal)?;
        terminal.send(&["-l".into(), format!("/{name}")])?;
        let mut found = None;
        for _ in 0..12 {
            terminal.wait(Duration::from_millis(250));
            let capture = terminal.capture()?;
            if !prompt.is_match(&capture) {
                continue;
            }
            found = python_lines(&capture)
                .into_iter()
                .filter_map(|line| regex.captures(line))
                .find(|groups| {
                    groups
                        .get(1)
                        .is_some_and(|matched| matched.as_str() == name)
                })
                .and_then(|groups| groups.get(2).map(|matched| matched.as_str().to_owned()));
            if found
                .as_ref()
                .is_some_and(|description| !description.is_empty())
            {
                break;
            }
        }
        result.insert(name.clone(), found.map_or(Value::Null, Value::String));
    }
    clear_name(terminal)?;
    Ok(format!(
        "{}\n",
        indent_dumps(&Value::Object(result), 0, false)?
    ))
}

pub fn main(args: &[String]) -> i32 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let result = match args {
        [] => build_bytes(root)
            .and_then(|bytes| {
                fs::write(root.join("config/cli-commands.json"), bytes).map_err(|e| e.to_string())
            })
            .map(|()| format!("{}\n", root.join("config/cli-commands.json").display())),
        [operation] if operation == "build" => build_bytes(root)
            .and_then(|bytes| {
                fs::write(root.join("config/cli-commands.json"), bytes).map_err(|e| e.to_string())
            })
            .map(|()| format!("{}\n", root.join("config/cli-commands.json").display())),
        [operation, input, binary] if operation == "builtin-filter" => read_json(Path::new(input))
            .and_then(|commands| {
                fs::read(binary)
                    .map_err(|e| format!("{binary}: {e}"))
                    .and_then(|data| filter_bytes(&commands, &data))
            }),
        [operation, socket, session, pattern, lead] if operation == "scrape-prefix" => {
            CommandTerminal::new("tmux".into(), socket.clone(), session.clone())
                .and_then(|mut terminal| scrape_prefix(&mut terminal, pattern, lead, 8))
        }
        [operation, socket, session, pattern, lead, maximum] if operation == "scrape-prefix" => {
            maximum
                .parse::<i64>()
                .map_err(|e| e.to_string())
                .and_then(|maximum| {
                    CommandTerminal::new("tmux".into(), socket.clone(), session.clone()).and_then(
                        |mut terminal| scrape_prefix(&mut terminal, pattern, lead, maximum),
                    )
                })
        }
        [operation, socket, session, names] if operation == "verify-names" => {
            read_json(Path::new(names))
                .and_then(|value| {
                    array(&value).and_then(|values| {
                        values
                            .iter()
                            .map(|value| string(value).map(str::to_owned))
                            .collect::<Result<Vec<_>, _>>()
                    })
                })
                .and_then(|names| {
                    CommandTerminal::new("tmux".into(), socket.clone(), session.clone())
                        .and_then(|mut terminal| verify_names(&mut terminal, &names))
                })
        }
        _ => {
            eprintln!(
                "uso: cargo xtask cli-catalog [build | builtin-filter INPUT BINARY | scrape-prefix SOCKET SESSION REGEX LEAD [MAXV] | verify-names SOCKET SESSION NAMES]"
            );
            return 2;
        }
    };
    match result {
        Ok(output) => {
            print!("{output}");
            0
        }
        Err(error) => {
            eprintln!("cli-catalog: {error}");
            1
        }
    }
}
