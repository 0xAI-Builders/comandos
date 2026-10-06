mod bytes;
pub use bytes::workspace_loads_bytes;

use serde_json::Value;

mod decode_error;
pub use decode_error::{PythonLoads, python_loads};

mod comparison;
pub(crate) use comparison::number_cmp;
pub use comparison::python_eq;

mod parser;
pub use parser::{object_fields, parse_slice, parse_unique_value, parse_value};
mod python;
pub use python::{
    dumps, float_repr, indent_dumps, join_response_entries, response_dumps, response_dumps_compact,
    response_dumps_entries, response_dumps_entry, response_dumps_entry_chunks,
    response_dumps_unicode, workspace_dumps, workspace_dumps_with_options,
};

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
