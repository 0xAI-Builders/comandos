//! Semantic comparison for remote GTK layout gates. Capture metadata is
//! explicitly declared; only widget geometry permits one CSS pixel rounding.
use serde_json::{Value, json};
use std::{collections::BTreeSet, fmt, path::Path};

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutDifference {
    pub path: String,
    pub reference: Option<Value>,
    pub candidate: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutDiff {
    pub geometry_tolerance_css_px: u8,
    pub differences: Vec<LayoutDifference>,
}

impl LayoutDiff {
    pub fn matches(&self) -> bool {
        self.differences.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutError(pub String);

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LayoutError {}

pub fn compare_layout(reference: &Value, candidate: &Value) -> Result<LayoutDiff, LayoutError> {
    for (name, value) in [("reference", reference), ("candidate", candidate)] {
        if !value.is_object() {
            return Err(LayoutError(format!("{name}: root must be an object")));
        }
        finite_numbers(value, name)?;
    }
    let mut differences = Vec::new();
    compare(
        reference,
        candidate,
        Context::Root,
        &mut Vec::new(),
        &mut differences,
    );
    Ok(LayoutDiff {
        geometry_tolerance_css_px: 1,
        differences,
    })
}

fn finite_numbers(value: &Value, name: &str) -> Result<(), LayoutError> {
    match value {
        Value::Number(number) if number.as_f64().is_none_or(|v| !v.is_finite()) => {
            Err(LayoutError(format!("{name}: non-finite number")))
        }
        Value::Array(values) => values.iter().try_for_each(|v| finite_numbers(v, name)),
        Value::Object(values) => values.values().try_for_each(|v| finite_numbers(v, name)),
        _ => Ok(()),
    }
}

fn ephemeral(path: &[String]) -> bool {
    matches!(path, [parent, key] if parent == "capture" && matches!(key.as_str(), "captured_at_unix_ms" | "process_pid" | "window_handle"))
}

#[derive(Clone, Copy)]
enum Context {
    Root,
    Data,
    WidgetList,
    Widget,
    Geometry,
    Coordinate,
}

fn child_context(parent: Context, key: &str, value: &Value) -> Context {
    match parent {
        Context::Root if matches!(key, "widgets" | "children") && value.is_array() => {
            Context::WidgetList
        }
        Context::Widget if key == "children" && value.is_array() => Context::WidgetList,
        Context::Widget if key == "geometry" && value.is_object() => Context::Geometry,
        Context::Geometry if matches!(key, "x" | "y" | "width" | "height") => Context::Coordinate,
        _ => Context::Data,
    }
}

fn within_rounding(a: &serde_json::Number, b: &serde_json::Number) -> bool {
    let integer = |n: &serde_json::Number| {
        n.as_i64()
            .map(i128::from)
            .or_else(|| n.as_u64().map(i128::from))
    };
    if let Some((a, b)) = integer(a).zip(integer(b)) {
        return (a - b).abs() <= 1;
    }
    a.as_f64().zip(b.as_f64()).is_some_and(|(a, b)| {
        a.abs() <= 9_007_199_254_740_991.0
            && b.abs() <= 9_007_199_254_740_991.0
            && (a - b).abs() <= 1.0
    })
}

fn pointer(path: &[String]) -> String {
    path.iter().fold(String::new(), |mut out, part| {
        out.push('/');
        out.push_str(&part.replace('~', "~0").replace('/', "~1"));
        out
    })
}

fn difference(
    path: &[String],
    reference: Option<&Value>,
    candidate: Option<&Value>,
    out: &mut Vec<LayoutDifference>,
) {
    out.push(LayoutDifference {
        path: pointer(path),
        reference: reference.cloned(),
        candidate: candidate.cloned(),
    });
}

fn compare(
    reference: &Value,
    candidate: &Value,
    context: Context,
    path: &mut Vec<String>,
    out: &mut Vec<LayoutDifference>,
) {
    if ephemeral(path) || reference == candidate {
        return;
    }
    match (reference, candidate) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
                path.push(key.clone());
                if !ephemeral(path) {
                    match (a.get(key), b.get(key)) {
                        (Some(a), Some(b)) => {
                            compare(a, b, child_context(context, key, a), path, out)
                        }
                        (a, b) => difference(path, a, b, out),
                    }
                }
                path.pop();
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            for index in 0..a.len().max(b.len()) {
                path.push(index.to_string());
                match (a.get(index), b.get(index)) {
                    (Some(a), Some(b)) => compare(
                        a,
                        b,
                        if matches!(context, Context::WidgetList) {
                            Context::Widget
                        } else {
                            Context::Data
                        },
                        path,
                        out,
                    ),
                    (a, b) => difference(path, a, b, out),
                }
                path.pop();
            }
        }
        (Value::Number(a), Value::Number(b)) if matches!(context, Context::Coordinate) => {
            if !within_rounding(a, b) {
                difference(path, Some(reference), Some(candidate), out);
            }
        }
        _ => difference(path, Some(reference), Some(candidate), out),
    }
}

fn read(path: &str) -> Result<Value, LayoutError> {
    if !Path::new(path).is_absolute() {
        return Err(LayoutError(format!("absolute path required: {path}")));
    }
    let bytes = std::fs::read(path).map_err(|e| LayoutError(format!("{path}: {e}")))?;
    serde_json::from_slice(&bytes).map_err(|e| LayoutError(format!("{path}: {e}")))
}

pub fn main(args: &[String]) -> i32 {
    let result = (|| {
        let [reference_flag, reference, candidate_flag, candidate] = args else {
            return Err(LayoutError("usage: app-layout --reference /absolute/reference.json --candidate /absolute/candidate.json".into()));
        };
        if reference_flag != "--reference" || candidate_flag != "--candidate" {
            return Err(LayoutError("expected --reference and --candidate".into()));
        }
        let diff = compare_layout(&read(reference)?, &read(candidate)?)?;
        let rows: Vec<Value> = diff
            .differences
            .iter()
            .map(|row| json!({"path":row.path,"reference":row.reference,"candidate":row.candidate}))
            .collect();
        let output = serde_json::to_string_pretty(
            &json!({"geometry_tolerance_css_px":diff.geometry_tolerance_css_px,"differences":rows}),
        )
        .map_err(|e| LayoutError(e.to_string()))?;
        println!("{output}");
        Ok(i32::from(!diff.matches()))
    })();
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("app-layout: {error}");
            2
        }
    }
}
