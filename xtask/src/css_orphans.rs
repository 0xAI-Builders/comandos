//! Auditoría de selectores CSS conservando el algoritmo de la herramienta original.
use regex::Regex;
use std::{collections::BTreeSet, fs, path::Path};
mod python_word;

pub(crate) fn python_alnum_ranges() -> &'static [(u32, u32)] {
    python_word::ALNUM_RANGES
}

fn word(c: char) -> bool {
    if c == '_' || c == '-' {
        return true;
    }
    let cp = u32::from(c);
    let ranges = python_word::ALNUM_RANGES;
    let end = ranges.partition_point(|(start, _)| *start <= cp);
    end > 0 && cp <= ranges[end - 1].1
}

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
    // El original usa UCD13: la tabla evita que versiones nuevas cambien los nombres.
    let starts = Regex::new(r"[.#][A-Za-z]").unwrap();
    let names = starts
        .find_iter(&selectors)
        .map(|found| {
            let end = selectors[found.end()..]
                .char_indices()
                .find(|(_, c)| !word(*c))
                .map_or(selectors.len(), |(offset, _)| found.end() + offset);
            &selectors[found.start()..end]
        })
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
                !before.is_some_and(word) && !after.is_some_and(word)
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

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    #[test]
    fn frozen_word_table_matches_original_for_every_codepoint() {
        let mut digest = Sha256::new();
        for cp in 0..=0x10ffff {
            digest.update([u8::from(char::from_u32(cp).is_some_and(super::word))]);
        }
        // re.fullmatch(r"[\w-]", chr(cp)), Python3.10/UCD13; surrogates are false.
        assert_eq!(
            format!("{:x}", digest.finalize()),
            "0d527713cc4fb4a2f3c010874f2a38e3110c0b0535be4abad37808c65c9db9cc"
        );
    }
}
