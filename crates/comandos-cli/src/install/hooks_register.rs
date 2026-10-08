//! Pure equivalent of the installer hook transformation, preserving foreign hooks.
use serde_json::{Value, json};

fn normalized(command: &str, home: &str) -> String {
    command
        .strip_prefix('~')
        .map_or_else(|| command.to_owned(), |suffix| format!("{home}{suffix}"))
}

pub fn configure(input: &Value, home: &str, notify: &str, tool: &str) -> Result<Value, String> {
    let mut output = input.clone();
    let document = output
        .as_object_mut()
        .ok_or("Claude settings must be an object")?;
    let hooks = document.entry("hooks").or_insert_with(|| json!({}));
    if hooks.is_null() {
        *hooks = json!({});
    }
    let hooks = hooks
        .as_object_mut()
        .ok_or("Claude hooks must be an object")?;
    for (event, command, timeout) in [
        ("UserPromptSubmit", notify, 15),
        ("Stop", notify, 15),
        ("Notification", notify, 15),
        ("SessionEnd", notify, 15),
        ("PreToolUse", tool, 5),
        ("PostToolUse", tool, 5),
    ] {
        let groups = hooks.entry(event).or_insert_with(|| json!([]));
        if groups.is_null() {
            *groups = json!([]);
        }
        let groups = groups
            .as_array_mut()
            .ok_or_else(|| format!("{event} must be an array"))?;
        let mut found = false;
        for group in groups.iter_mut() {
            let group = group
                .as_object_mut()
                .ok_or("hook group must be an object")?;
            let entries = group.entry("hooks").or_insert_with(|| json!([]));
            if entries.is_null() {
                *entries = json!([]);
            }
            let entries = entries
                .as_array_mut()
                .ok_or("hook entries must be an array")?;
            for entry in entries {
                let entry = entry
                    .as_object_mut()
                    .ok_or("hook entry must be an object")?;
                if entry
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|value| normalized(value, home) == normalized(command, home))
                {
                    found = true;
                    entry.entry("timeout").or_insert(json!(timeout));
                }
            }
        }
        if !found {
            groups.push(json!({"hooks":[{"type":"command","command":command,"timeout":timeout}]}));
        }
    }
    let groups = hooks
        .entry("PostToolUseFailure")
        .or_insert_with(|| json!([]));
    if groups.is_null() {
        *groups = json!([]);
    }
    let groups = groups
        .as_array_mut()
        .ok_or("PostToolUseFailure must be an array")?;
    for group in groups.iter_mut() {
        let group = group
            .as_object_mut()
            .ok_or("hook group must be an object")?;
        let entries = group.entry("hooks").or_insert_with(|| json!([]));
        if entries.is_null() {
            *entries = json!([]);
        }
        entries
            .as_array_mut()
            .ok_or("hook entries must be an array")?
            .retain(|entry| {
                entry
                    .get("command")
                    .and_then(Value::as_str)
                    .is_none_or(|value| normalized(value, home) != normalized(tool, home))
            });
    }
    groups.retain(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|hooks| !hooks.is_empty())
    });
    Ok(output)
}
