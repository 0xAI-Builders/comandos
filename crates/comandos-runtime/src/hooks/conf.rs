//! `~/.claude/hooks/cc-notify.conf`: los valores por defecto del bash y la parte de
//! `source` que el archivo usa de verdad (asignaciones `CLAVE=valor`, con comillas
//! simples o dobles y comentarios). Solo se aplican las claves que el hook lee.
use std::path::Path;

pub struct Conf {
    pub sound_done: Vec<u8>,
    pub sound_attention: Vec<u8>,
    pub desktop_notify: bool,
    pub sound_enabled: bool,
    pub notify_on_done: bool,
    pub notify_on_attention: bool,
    pub speak_attention: bool,
    pub speak_done: bool,
    pub piper_voice: Vec<u8>,
    /// Ya validado: 0..=100.
    pub volume: u32,
    pub lang: Lang,
}

/// Textos de la interfaz según `CC_LANG` (o `$LANG` en modo `auto`).
#[derive(Clone, Copy)]
pub struct Lang {
    pub attn: &'static str,
    pub wait_body: &'static str,
    pub done: &'static str,
    pub done_body: &'static str,
    pub opts: &'static str,
    pub yes_always: &'static str,
    pub speak_wait: &'static str,
    pub speak_done: &'static str,
    pub spd_lang: &'static str,
}

const ES: Lang = Lang {
    attn: "necesita tu atencion",
    wait_body: "Esperando input o permiso",
    done: "termino",
    done_body: "Iteracion completada, listo para tu siguiente instruccion",
    opts: "Opciones:",
    yes_always: "Si\x1fSi, siempre\x1fNo",
    speak_wait: "necesita tu respuesta",
    speak_done: "terminó",
    spd_lang: "es",
};
const EN: Lang = Lang {
    attn: "needs your attention",
    wait_body: "Waiting for input or permission",
    done: "finished",
    done_body: "Turn complete — ready for your next instruction",
    opts: "Options:",
    yes_always: "Yes\x1fYes, always\x1fNo",
    speak_wait: "needs your answer",
    speak_done: "finished",
    spd_lang: "en",
};

impl Conf {
    /// Defaults del bash, luego el archivo, luego idioma y volumen derivados.
    pub fn load(hooks_dir: &Path, sys_lang: &[u8]) -> Conf {
        let mut raw: Vec<(&str, Vec<u8>)> = vec![
            (
                "SOUND_DONE",
                b"/usr/share/sounds/freedesktop/stereo/message.oga".to_vec(),
            ),
            (
                "SOUND_ATTENTION",
                b"/usr/share/sounds/freedesktop/stereo/window-attention.oga".to_vec(),
            ),
            ("DESKTOP_NOTIFY", b"1".to_vec()),
            ("SOUND_ENABLED", b"1".to_vec()),
            ("NOTIFY_ON_DONE", b"1".to_vec()),
            ("NOTIFY_ON_ATTENTION", b"1".to_vec()),
            ("SPEAK_ATTENTION", b"1".to_vec()),
            ("SPEAK_DONE", b"1".to_vec()),
            ("PIPER_VOICE", b"es_MX-ald-medium".to_vec()),
            ("VOLUME", b"60".to_vec()),
            ("CC_LANG", b"auto".to_vec()),
        ];
        if let Ok(text) = std::fs::read(hooks_dir.join("cc-notify.conf")) {
            for (key, value) in parse(&text) {
                if let Some(slot) = raw.iter_mut().find(|(k, _)| k.as_bytes() == key.as_slice()) {
                    slot.1 = value;
                }
            }
        }
        let get = |k: &str| {
            raw.iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let on = |k: &str| get(k) == b"1";
        let lang = match get("CC_LANG").as_slice() {
            b"es" => ES,
            b"en" => EN,
            _ if sys_lang.starts_with(b"es") || sys_lang.starts_with(b"ES") => ES,
            _ => EN,
        };
        let volume = get("VOLUME");
        let volume = if volume.is_empty() || !volume.iter().all(u8::is_ascii_digit) {
            60
        } else {
            std::str::from_utf8(&volume)
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .map_or(100, |v| v.min(100) as u32)
        };
        Conf {
            sound_done: get("SOUND_DONE"),
            sound_attention: get("SOUND_ATTENTION"),
            desktop_notify: on("DESKTOP_NOTIFY"),
            sound_enabled: on("SOUND_ENABLED"),
            notify_on_done: on("NOTIFY_ON_DONE"),
            notify_on_attention: on("NOTIFY_ON_ATTENTION"),
            speak_attention: on("SPEAK_ATTENTION"),
            speak_done: on("SPEAK_DONE"),
            piper_voice: get("PIPER_VOICE"),
            volume,
            lang,
        }
    }
}

/// Asignaciones `[export ]CLAVE=valor` del archivo, en orden.
fn parse(text: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    for line in text.split(|&b| b == b'\n') {
        let line = trim_start(line);
        let line = line
            .strip_prefix(b"export ")
            .map(trim_start)
            .unwrap_or(line);
        let Some(eq) = line.iter().position(|&b| b == b'=') else {
            continue;
        };
        let key = &line[..eq];
        let valid = key
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
            && key.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_');
        if valid {
            out.push((key.to_vec(), word(&line[eq + 1..])));
        }
    }
    out
}

fn trim_start(line: &[u8]) -> &[u8] {
    let start = line
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(line.len());
    &line[start..]
}

/// Una palabra de shell: hasta el primer espacio sin comillas.
fn word(rest: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            b' ' | b'\t' | b'\r' => break,
            b'\'' => {
                let end = rest[i + 1..]
                    .iter()
                    .position(|&b| b == b'\'')
                    .map_or(rest.len(), |p| i + 1 + p);
                out.extend_from_slice(&rest[i + 1..end]);
                i = end + 1;
            }
            b'"' => {
                i += 1;
                while i < rest.len() && rest[i] != b'"' {
                    if rest[i] == b'\\'
                        && matches!(rest.get(i + 1), Some(b'"' | b'\\' | b'$' | b'`'))
                    {
                        i += 1;
                    }
                    out.push(rest[i]);
                    i += 1;
                }
                i += 1;
            }
            b'\\' if i + 1 < rest.len() => {
                out.push(rest[i + 1]);
                i += 2;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shell_assignments() {
        let parsed = parse(b"# c\nVOLUME=12\n  export CC_LANG='en'\nPIPER_VOICE=\"es MX\" # nota\nNO VALE=1\nSOUND_DONE=a\\ b\n");
        let as_str: Vec<(String, String)> = parsed
            .into_iter()
            .map(|(k, v)| (String::from_utf8(k).unwrap(), String::from_utf8(v).unwrap()))
            .collect();
        assert_eq!(
            as_str,
            [
                ("VOLUME", "12"),
                ("CC_LANG", "en"),
                ("PIPER_VOICE", "es MX"),
                ("SOUND_DONE", "a b")
            ]
            .map(|(k, v)| (k.to_string(), v.to_string()))
        );
    }
}
