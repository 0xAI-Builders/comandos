//! Validation of the three result models consumed by access checks.
//!
//! Contract: MCP 1.30.0 with Pydantic 2.13.5. Validate modeled fields only;
//! every model permits arbitrary extra fields. JSON dictionaries/schema bodies
//! remain opaque. Coercion is limited to the SDK's bool and numeric scalars.
use serde_json::Value;

fn optional(value: &Value, key: &str, valid: fn(&Value) -> bool) -> bool {
    value.get(key).is_none_or(|v| v.is_null() || valid(v))
}
fn strings(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|a| a.iter().all(Value::is_string))
}
fn metadata(value: &Value) -> bool {
    optional(value, "_meta", Value::is_object)
}
fn boolean(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::Number(_) => match value.as_f64()? {
            0.0 => Some(false),
            1.0 => Some(true),
            _ => None,
        },
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "0" | "off" | "f" | "false" | "n" | "no" => Some(false),
            "1" | "on" | "t" | "true" | "y" | "yes" => Some(true),
            _ => None,
        },
        _ => None,
    }
}
fn valid_boolean(value: &Value) -> bool {
    boolean(value).is_some()
}
fn integer(value: &Value) -> bool {
    let digits = |s: &str| {
        !s.is_empty()
            && s.split('_')
                .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
    };
    match value {
        Value::Bool(_) => true,
        Value::Number(n) => {
            // JSON integers remain arbitrary precision, as in json.loads.
            let raw = n.as_str();
            let unsigned = raw.strip_prefix('-').unwrap_or(raw);
            (!unsigned.is_empty() && unsigned.bytes().all(|c| c.is_ascii_digit()))
                || n.as_f64()
                    .is_some_and(|n| n.is_finite() && n.fract() == 0.0)
        }
        Value::String(s) => {
            let s = s.trim();
            let s = s.strip_prefix(['+', '-']).unwrap_or(s);
            // The SDK accepts underscores between integer digits, or a decimal
            // spelling with an all-zero fractional part; never an exponent.
            if let Some((whole, fraction)) = s.split_once('.') {
                digits(whole) && !fraction.is_empty() && fraction.bytes().all(|c| c == b'0')
            } else {
                digits(s)
            }
        }
        _ => false,
    }
}
fn priority(value: &Value) -> bool {
    let number = match value {
        Value::Bool(b) => Some(u8::from(*b) as f64),
        Value::Number(_) => value.as_f64(),
        Value::String(s) => {
            let s = s.trim();
            // Underscores must occur between digits, as in Pydantic's parser.
            if s.as_bytes().iter().enumerate().any(|(i, c)| {
                *c == b'_'
                    && (i == 0
                        || !s.as_bytes()[i - 1].is_ascii_digit()
                        || !s.as_bytes().get(i + 1).is_some_and(u8::is_ascii_digit))
            }) {
                return false;
            }
            s.replace('_', "").parse::<f64>().ok()
        }
        _ => None,
    };
    number.is_some_and(|n| (0.0..=1.0).contains(&n))
}
fn uri(value: &Value) -> bool {
    // Pydantic AnyUrl and url use WHATWG URL parsing, including opaque schemes,
    // whitespace normalization and special-scheme host/port validation.
    value.as_str().is_some_and(|s| url::Url::parse(s).is_ok())
}
fn icons(value: &Value) -> bool {
    value.as_array().is_some_and(|a| {
        a.iter().all(|icon| {
            icon.is_object()
                && icon["src"].is_string()
                && optional(icon, "mimeType", Value::is_string)
                && optional(icon, "sizes", strings)
        })
    })
}
fn named(value: &Value) -> bool {
    value.is_object() && value["name"].is_string() && optional(value, "title", Value::is_string)
}
fn implementation(value: &Value) -> bool {
    named(value)
        && value["version"].is_string()
        && optional(value, "websiteUrl", Value::is_string)
        && optional(value, "icons", icons)
}
fn list_changed(value: &Value) -> bool {
    value.is_object() && optional(value, "listChanged", valid_boolean)
}
fn resources_capability(value: &Value) -> bool {
    list_changed(value) && optional(value, "subscribe", valid_boolean)
}
fn task_tools(value: &Value) -> bool {
    value.is_object() && optional(value, "call", Value::is_object)
}
fn task_requests(value: &Value) -> bool {
    value.is_object() && optional(value, "tools", task_tools)
}
fn tasks(value: &Value) -> bool {
    value.is_object()
        && optional(value, "list", Value::is_object)
        && optional(value, "cancel", Value::is_object)
        && optional(value, "requests", task_requests)
}
fn experimental(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.values().all(Value::is_object))
}
fn capabilities(value: &Value) -> bool {
    value.is_object()
        && optional(value, "tools", list_changed)
        && optional(value, "prompts", list_changed)
        && optional(value, "resources", resources_capability)
        && optional(value, "logging", Value::is_object)
        && optional(value, "completions", Value::is_object)
        && optional(value, "experimental", experimental)
        && optional(value, "tasks", tasks)
}
pub(super) fn initialize(value: &Value) -> bool {
    value.is_object()
        && metadata(value)
        && (value["protocolVersion"].is_string() || integer(&value["protocolVersion"]))
        && implementation(&value["serverInfo"])
        && capabilities(&value["capabilities"])
        && optional(value, "instructions", Value::is_string)
}
fn tool_annotations(value: &Value) -> bool {
    value.is_object()
        && optional(value, "title", Value::is_string)
        && [
            "readOnlyHint",
            "destructiveHint",
            "idempotentHint",
            "openWorldHint",
        ]
        .iter()
        .all(|key| optional(value, key, valid_boolean))
}
fn execution(value: &Value) -> bool {
    value.is_object()
        && optional(value, "taskSupport", |v| {
            matches!(v.as_str(), Some("forbidden" | "optional" | "required"))
        })
}
fn tool(value: &Value) -> bool {
    named(value)
        && metadata(value)
        && value["inputSchema"].is_object()
        && optional(value, "description", Value::is_string)
        && optional(value, "outputSchema", Value::is_object)
        && optional(value, "icons", icons)
        && optional(value, "annotations", tool_annotations)
        && optional(value, "execution", execution)
}
pub(super) fn tools(value: &Value) -> bool {
    value.is_object()
        && metadata(value)
        && optional(value, "nextCursor", Value::is_string)
        && value["tools"]
            .as_array()
            .is_some_and(|a| a.iter().all(tool))
}
fn annotations(value: &Value) -> bool {
    value.is_object()
        && optional(value, "priority", priority)
        && optional(value, "audience", |v| {
            v.as_array().is_some_and(|a| {
                a.iter()
                    .all(|v| matches!(v.as_str(), Some("user" | "assistant")))
            })
        })
}
fn resource_contents(value: &Value) -> bool {
    value.is_object() && metadata(value) && uri(&value["uri"])
        && optional(value, "mimeType", Value::is_string)
        // This is a union. A valid blob variant permits an invalid extra text
        // field, and conversely a valid text variant permits any extra blob.
        && (value["text"].is_string() || value["blob"].is_string())
}
fn resource_link(value: &Value) -> bool {
    named(value)
        && uri(&value["uri"])
        && optional(value, "description", Value::is_string)
        && optional(value, "mimeType", Value::is_string)
        && optional(value, "size", integer)
        && optional(value, "icons", icons)
}
fn content(value: &Value) -> bool {
    value.is_object()
        && metadata(value)
        && optional(value, "annotations", annotations)
        && match value["type"].as_str() {
            Some("text") => value["text"].is_string(),
            Some("image" | "audio") => value["data"].is_string() && value["mimeType"].is_string(),
            Some("resource_link") => resource_link(value),
            Some("resource") => resource_contents(&value["resource"]),
            _ => false,
        }
}
/// Return the SDK-coerced isError only after validating the complete result.
/// Tool outputSchema execution is a separate session-level contract.
pub(super) fn call(value: &Value) -> Option<bool> {
    if !value.is_object()
        || !metadata(value)
        || !optional(value, "structuredContent", Value::is_object)
        || !value["content"]
            .as_array()
            .is_some_and(|a| a.iter().all(content))
    {
        return None;
    }
    value.get("isError").map_or(Some(false), boolean)
}
