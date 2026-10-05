//! El aviso normalizado de `POST /notify` y la configuración de `cc-notify.conf`.
use serde_json::Value;
use std::path::Path;

/// Idioma de los textos del popup (`UI_LANG` del Python).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Es,
    En,
}

/// Aviso tal como lo recibe el hilo de GTK (`native_notify` del Python).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    pub session: String,
    pub kind: String,
    pub project: String,
    pub options: String,
    pub full: String,
    pub pane: String,
}

impl Notice {
    /// `str(d.get(clave, omisión))[:n]` de cada campo, recortando por carácter.
    /// `None` si el cuerpo no es un objeto: en el Python `d.get` lanza
    /// `AttributeError` y la conexión se cierra sin respuesta.
    pub fn from_payload(payload: &Value) -> Option<Notice> {
        let object = payload.as_object()?;
        let field = |key: &str, default: &str, limit: Option<usize>| {
            let text = object.get(key).map_or_else(|| default.to_string(), py_str);
            match limit {
                Some(n) => text.chars().take(n).collect(),
                None => text,
            }
        };
        Some(Notice {
            title: field("title", "Claude Code", Some(200)),
            body: field("body", "", Some(400)),
            session: field("session", "", None),
            kind: field("kind", "done", None),
            project: field("project", "", Some(80)),
            options: field("options", "", Some(600)),
            full: field("full", "", Some(60_000)),
            pane: field("pane", "", Some(10)),
        })
    }

    /// Campos en el orden de los argumentos de `native_notify`.
    pub fn fields(&self) -> [&str; 8] {
        [
            &self.title,
            &self.body,
            &self.session,
            &self.kind,
            &self.project,
            &self.options,
            &self.full,
            &self.pane,
        ]
    }
}

/// `str(valor)` de Python para un valor de `json.loads`: cadenas tal cual,
/// `None`/`True`/`False`, números como `int`/`float` y contenedores con su `repr`.
pub fn py_str(value: &Value) -> String {
    comandos_core::pomodoro::python_str(value)
}

/// Espacios de `str.strip()` de Python: `White_Space` de Unicode más `\x1c`–`\x1f`.
pub(crate) fn is_py_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

/// Líneas de un archivo de texto como las itera Python (saltos universales:
/// `\n`, `\r\n` y `\r`). `None` si no existe, no se lee o no es UTF-8 (el
/// Python cae al valor por omisión en cualquier excepción).
fn conf_lines(path: &Path) -> Option<Vec<String>> {
    let text = String::from_utf8(std::fs::read(path).ok()?).ok()?;
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    Some(text.split('\n').map(str::to_string).collect())
}

/// `line.split("=", 1)[1].strip().strip('"\'')` si la línea (sin espacios) empieza por `clave=`.
fn conf_match(line: &str, key: &str) -> Option<String> {
    let rest = line.trim_matches(is_py_space).strip_prefix(key)?;
    let value = rest.strip_prefix('=')?;
    Some(
        value
            .trim_matches(is_py_space)
            .trim_matches(['"', '\''])
            .to_string(),
    )
}

/// `_conf_value(clave, omisión)`: la primera línea de `cc-notify.conf` que empieza
/// por `clave=` (tras quitar espacios); el valor sin espacios ni comillas.
pub fn conf_value(path: &Path, key: &str, default: &str) -> String {
    conf_lines(path)
        .and_then(|lines| lines.iter().find_map(|line| conf_match(line, key)))
        .unwrap_or_else(|| default.to_string())
}

/// `_ui_lang()`: la última línea `CC_LANG=` de la conf si vale `es`/`en`; si no,
/// `es` cuando `LANG` empieza por `es` (sin distinguir mayúsculas) y `en` en otro caso.
pub fn ui_lang(conf: &Path, lang_env: Option<&str>) -> Lang {
    let chosen = conf_lines(conf)
        .and_then(|lines| {
            lines
                .iter()
                .rev()
                .find_map(|line| conf_match(line, "CC_LANG"))
        })
        .unwrap_or_else(|| "auto".to_string());
    match chosen.as_str() {
        "es" => Lang::Es,
        "en" => Lang::En,
        _ if lang_env
            .unwrap_or_default()
            .get(..2)
            .is_some_and(|p| p.eq_ignore_ascii_case("es")) =>
        {
            Lang::Es
        }
        _ => Lang::En,
    }
}
