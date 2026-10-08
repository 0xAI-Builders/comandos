use serde_json::Value;
use std::fmt;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    Invalid(String),
    Unknown(String),
    MissingConsumer(String),
    Refused(String),
    Failed(String),
}
impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CommandError {}
pub fn string_arg<'a>(
    args: &'a Value,
    key: &str,
    default: &'a str,
) -> Result<&'a str, CommandError> {
    match args.get(key) {
        None => Ok(default),
        Some(Value::String(value)) => Ok(value),
        _ => Err(CommandError::Invalid(format!("{key} must be a string"))),
    }
}
