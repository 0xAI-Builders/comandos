//! Compatibility reader for tmux borders; never invokes grep or a shell.
use std::{
    io::{self, Write},
    path::Path,
};

pub fn render(bytes: &[u8], pane: &str) -> Vec<u8> {
    if pane.is_empty() || pane.contains(['\n', '\r']) {
        return Vec::new();
    }
    let mut prefix = pane.as_bytes().to_vec();
    prefix.push(b' ');
    let Some(rest) = bytes
        .split(|b| *b == b'\n')
        .find_map(|line| line.strip_prefix(prefix.as_slice()))
    else {
        return Vec::new();
    };
    let color = [
        (b"claude".as_slice(), 141),
        (b"codex", 43),
        (b"grok", 208),
        (b"opencode", 215),
        (b"gemini", 111),
        (b"agy", 183),
    ]
    .into_iter()
    .find(|(agent, _)| rest.starts_with(agent))
    .map_or(244, |(_, color)| color);
    let mut output = format!("#[fg=colour{color},bold]▸ ").into_bytes();
    output.extend_from_slice(rest);
    output.extend_from_slice(b"#[default]");
    output
}

pub fn read(home: &Path, pane: &str) -> comandos_store::Result<Vec<u8>> {
    let body = comandos_store::domains::DomainStore { home }
        .document(
            "hooks/pane-models.txt",
            "quota-docs",
            home.join(".claude/hooks/pane-models.txt"),
        )
        .read_readonly()?;
    Ok(body.map_or_else(Vec::new, |bytes| render(&bytes, pane)))
}

pub fn main(args: &[String]) -> i32 {
    let Some(pane) = args.first().filter(|p| !p.is_empty()) else {
        return 0;
    };
    let Some(home) = std::env::var_os("HOME") else {
        return 0;
    };
    match read(Path::new(&home), pane) {
        Ok(bytes) => {
            if io::stdout().lock().write_all(&bytes).is_ok() {
                0
            } else {
                1
            }
        }
        Err(error) => {
            eprintln!("pane-model: {error}");
            1
        }
    }
}
