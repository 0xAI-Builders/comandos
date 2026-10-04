//! Bounded read-only terminal history, with no process or home-directory access.
use crate::pane_typing::TmuxResult;
use serde_json::{Value, json};

pub const HISTORY_LIMIT: usize = 1_000_000;
pub const PANE_FORMAT: &str = "#{pane_id}\t#{pane_active}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}\t#{pane_current_command}\t#{pane_current_path}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryError {
    InvalidSession,
    InvalidLines,
    SessionNotFound,
    MalformedPanes,
    ForeignPane,
    InvalidCoordinates,
    NoPaneAtPosition,
    PaneNotFound,
    CaptureFailed,
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidSession => "Sesión inválida",
            Self::InvalidLines => "El historial admite entre 1 y 5000 líneas",
            Self::SessionNotFound => "No se encuentra la sesión",
            Self::MalformedPanes => "Datos de panel inválidos",
            Self::ForeignPane => "El panel no pertenece a esta sesión",
            Self::InvalidCoordinates => "Coordenadas inválidas",
            Self::NoPaneAtPosition => "No hay un panel en esa posición",
            Self::PaneNotFound => "No se encuentra el panel",
            Self::CaptureFailed => "No se pudo leer el historial del panel",
        })
    }
}
impl std::error::Error for HistoryError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub id: String,
    pub active: bool,
    pub left: i64,
    pub top: i64,
    pub width: i64,
    pub height: i64,
    pub title: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryResponse {
    pub ok: bool,
    pub pane: String,
    pub panes: Vec<Pane>,
    pub text: String,
    pub lines: u32,
    pub truncated: bool,
    pub source: &'static str,
    pub read_only: bool,
}

impl HistoryResponse {
    pub fn to_json(&self) -> Value {
        let panes = self
            .panes
            .iter()
            .map(|pane| {
                json!({
                    "id": pane.id, "active": pane.active,
                    "left": pane.left, "top": pane.top,
                    "width": pane.width, "height": pane.height,
                    "title": pane.title, "path": pane.path,
                })
            })
            .collect::<Vec<_>>();
        json!({"ok":self.ok, "pane":self.pane, "panes":panes,
            "text":self.text, "lines":self.lines, "truncated":self.truncated,
            "source":self.source, "readOnly":self.read_only})
    }
}

pub fn friendly_path(path: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    if !home.is_empty()
        && let Some(suffix) = path.strip_prefix(home)
        && (suffix.is_empty() || suffix.starts_with('/'))
    {
        return format!("~{suffix}");
    }
    path.into()
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn parse_panes(output: &str, home: &str) -> Result<Vec<Pane>, HistoryError> {
    let mut panes = Vec::new();
    // Match Python splitlines, including Unicode line separators. Empty rows are ignored.
    for row in output.split([
        '\n', '\r', '\u{b}', '\u{c}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{85}', '\u{2028}',
        '\u{2029}',
    ]) {
        let fields = row.splitn(8, '\t').collect::<Vec<_>>();
        if fields.len() < 7 {
            continue;
        }
        let parse = |index: usize| {
            fields[index]
                .trim()
                .parse::<i64>()
                .map_err(|_| HistoryError::MalformedPanes)
        };
        let (left, top, width, height) = (parse(2)?, parse(3)?, parse(4)?, parse(5)?);
        let valid_id = fields[0]
            .strip_prefix('%')
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()));
        if !valid_id
            || left < 0
            || top < 0
            || width <= 0
            || height <= 0
            || left.checked_add(width).is_none()
            || top.checked_add(height).is_none()
        {
            return Err(HistoryError::MalformedPanes);
        }
        panes.push(Pane {
            id: fields[0].into(),
            active: fields[1] == "1",
            left,
            top,
            width,
            height,
            title: fields[6].chars().take(100).collect(),
            path: friendly_path(fields.get(7).copied().unwrap_or(""), home)
                .chars()
                .take(300)
                .collect(),
        });
    }
    Ok(panes)
}

pub fn capture(
    mut tmux: impl FnMut(&[&str]) -> TmuxResult,
    data: &Value,
    home: &str,
) -> Result<HistoryResponse, HistoryError> {
    // Legacy callers sometimes supply integer or boolean session identifiers.
    let session = match data.get("session").filter(|value| truthy(value)) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Bool(true)) => "True".into(),
        Some(Value::Number(value)) => value.to_string(),
        _ => return Err(HistoryError::InvalidSession),
    };
    if session.is_empty()
        || session.len() > 120
        || !session
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-')
    {
        return Err(HistoryError::InvalidSession);
    }
    let lines = match data.get("lines") {
        None => 2000,
        Some(value) => value
            .as_u64()
            .filter(|lines| (1..=5000).contains(lines))
            .ok_or(HistoryError::InvalidLines)? as u32,
    };
    let target = format!("={session}");
    let listed = tmux(&["list-panes", "-t", &target, "-F", PANE_FORMAT]);
    if listed.returncode != 0 {
        return Err(HistoryError::SessionNotFound);
    }
    let panes = parse_panes(&listed.stdout, home)?;
    let selected = if let Some(pane) = data.get("pane").filter(|value| truthy(value)) {
        panes
            .iter()
            .find(|candidate| Some(candidate.id.as_str()) == pane.as_str())
            .ok_or(HistoryError::ForeignPane)?
    } else if data.get("col").is_some() || data.get("row").is_some() {
        let coordinate = |name| {
            let number = data
                .get(name)
                .and_then(Value::as_number)
                .ok_or(HistoryError::InvalidCoordinates)?;
            if let Some(value) = number.as_u64() {
                return Ok(Some(i128::from(value)));
            }
            if number.as_i64() == Some(0) {
                return Ok(Some(0));
            }
            // Positive arbitrary-precision integers are valid coordinates outside
            // every supported pane. Decimal/exponent numbers remain invalid.
            if number.to_string().bytes().all(|byte| byte.is_ascii_digit()) {
                Ok(None)
            } else {
                Err(HistoryError::InvalidCoordinates)
            }
        };
        let (col, row) = (coordinate("col")?, coordinate("row")?);
        let (Some(col), Some(row)) = (col, row) else {
            return Err(HistoryError::NoPaneAtPosition);
        };
        panes
            .iter()
            .find(|pane| {
                i128::from(pane.left) <= col
                    && col < i128::from(pane.left) + i128::from(pane.width)
                    && i128::from(pane.top) <= row
                    && row < i128::from(pane.top) + i128::from(pane.height)
            })
            .ok_or(HistoryError::NoPaneAtPosition)?
    } else {
        panes
            .iter()
            .find(|pane| pane.active)
            .ok_or(HistoryError::PaneNotFound)?
    };
    let pane = selected.id.clone();
    let start = format!("-{lines}");
    let captured = tmux(&["capture-pane", "-p", "-J", "-t", &pane, "-S", &start]);
    if captured.returncode != 0 {
        return Err(HistoryError::CaptureFailed);
    }
    let count = captured.stdout.chars().count();
    let truncated = count > HISTORY_LIMIT;
    let text = if truncated {
        captured
            .stdout
            .chars()
            .skip(count - HISTORY_LIMIT)
            .collect()
    } else {
        captured.stdout
    };
    Ok(HistoryResponse {
        ok: true,
        pane,
        panes,
        text,
        lines,
        truncated,
        source: "tmux",
        read_only: true,
    })
}
