use super::ComponentState;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn states_json(states: &BTreeMap<String, ComponentState>) -> Value {
    let mut out = serde_json::Map::new();
    for (id, state) in states {
        let value = match state {
            ComponentState::Off => json!({"state":"off"}),
            ComponentState::On => json!({"state":"on"}),
            ComponentState::Shadow => json!({"state":"shadow"}),
            ComponentState::Drift { expected, found } => {
                json!({"state":"drift","expected":expected,"found":found})
            }
            ComponentState::MissingSource => json!({"state":"missing_source"}),
            ComponentState::MissingDep(dep) => json!({"state":"missing_dep","dep":dep}),
        };
        out.insert(id.clone(), value);
    }
    Value::Object(out)
}
