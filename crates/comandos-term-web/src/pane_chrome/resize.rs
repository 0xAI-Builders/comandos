//! The pane resize queue only compares its three protocol fields.
use serde_json::{Number, Value, json};
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ResizeRequest {
    pane: String,
    axis: &'static str,
    // Keep Number's original lexical equality (-0 differs from 0); nonfinite
    // geometry becomes null, exactly as json! with an f64 does.
    size: Option<Number>,
}
impl ResizeRequest {
    pub(crate) fn new(pane: String, vertical: bool, size: f64) -> Self {
        Self {
            pane,
            axis: if vertical { "x" } else { "y" },
            size: Number::from_f64(size),
        }
    }
    pub(crate) fn body(&self, session: &str) -> Value {
        let mut body =
            json!({"session":session,"action":"resize","pane":self.pane,"axis":self.axis});
        // Insert the existing number directly: re-serializing a Number through
        // ValueSerializer would retain its generic RawValue deserializer.
        body["size"] = self.size.clone().map(Value::Number).unwrap_or(Value::Null);
        body
    }
}
