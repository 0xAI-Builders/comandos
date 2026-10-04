use serde_json::Value;

mod comparison;
pub(crate) use comparison::number_cmp;
pub use comparison::python_eq;

mod parser;
pub use parser::{parse_slice, parse_unique_value, parse_value};
mod python;
pub use python::{dumps, workspace_dumps};

// Legacy JSON semantics at the compatibility boundary (null, false, 0 and ""
// all select defaults). Domain identifiers are validated before persistence.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

pub(crate) fn legacy_id(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        other => other.to_string(),
    }
}

mod workspace;
pub(crate) use workspace::validate_workspace_depth;
pub use workspace::{MAX_WORKSPACE_JSON_DEPTH, workspace_loads};
