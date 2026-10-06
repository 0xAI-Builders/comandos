//! Auditoría de selectores CSS conservando el algoritmo de la herramienta original.
use regex::Regex;
use std::{collections::BTreeSet, fs, path::Path};

fn read_text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path)
        // Python abre archivos de texto con universal-newlines.
        .map(|text| text.replace("\r\n", "\n").replace('\r', "\n"))
        .map_err(|error| format!("{}: {error}", path.display()))
}

pub fn scan(root: &Path, prefixes: &[String]) -> Result<Vec<String>, String> {
    let dash = root.join("dash");
    let html = read_text(&dash.join("index.html"))?;
    let style_blocks = Regex::new(r"(?s)<style>(.*?)</style>").unwrap();
    let style = style_blocks
        .captures_iter(&html)
        .map(|capture| capture[1].to_owned())
        .collect::<Vec<_>>()
        .join("\n");
    let mut rest = style_blocks.replace_all(&html, "").into_owned();
    let mut scripts = fs::read_dir(&dash)
        .map_err(|error| format!("{}: {error}", dash.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("{}: {error}", dash.display()))?;
    scripts.retain(|path| {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".js"))
    });
    scripts.sort();
    for script in scripts {
        rest.push_str(&read_text(&script)?);
    }
    // Una sola sustitución, como re.sub: no se borran bloques CSS anidados de nuevo.
    let declarations = Regex::new(r"\{[^{}]*\}").unwrap();
    let selectors = declarations.replace_all(&style, "{}");
    // Python \w admite letras/números/_, pero no combining marks ni conectores Unicode.
    let names = Regex::new(r"[.#][A-Za-z][\p{L}\p{N}_-]*").unwrap();
    let word = Regex::new(r"^[\p{L}\p{N}_-]$").unwrap();
    let names = names
        .find_iter(&selectors)
        .map(|found| found.as_str())
        .collect::<BTreeSet<_>>();
    Ok(names
        .into_iter()
        .filter(|name| {
            if !prefixes.is_empty() && !prefixes.iter().any(|prefix| name.starts_with(prefix)) {
                return false;
            }
            let bare = &name[1..];
            !rest.match_indices(bare).any(|(at, _)| {
                let before = rest[..at].chars().next_back();
                let after = rest[at + bare.len()..].chars().next();
                !before.is_some_and(|c| word.is_match(&c.to_string()))
                    && !after.is_some_and(|c| word.is_match(&c.to_string()))
            })
        })
        .map(str::to_owned)
        .collect())
}

pub fn main(args: &[String]) -> i32 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    match scan(root, args) {
        Ok(names) => {
            for name in names {
                println!("{name}");
            }
            0
        }
        Err(error) => {
            eprintln!("css-orphans: {error}");
            1
        }
    }
}
