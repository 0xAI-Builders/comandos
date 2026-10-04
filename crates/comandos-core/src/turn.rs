use crate::json::{legacy_id, truthy};
use serde_json::{Map, Value};

fn at(event: &Value) -> Value {
    let value = &event["occurredAtMs"];
    if value.is_number() {
        value.clone()
    } else {
        Value::Null
    }
}

fn get<'a>(current: &'a Map<String, Value>, key: &str) -> &'a Value {
    current.get(key).unwrap_or(&Value::Null)
}

fn not_older(event: &Value, current: &Map<String, Value>) -> bool {
    let at = at(event);
    let started = get(current, "startedAtMs");
    if at.is_null() || started.is_null() {
        return true;
    }
    let integer = |v: &Value| {
        v.as_i64()
            .map(i128::from)
            .or_else(|| v.as_u64().map(i128::from))
    };
    // Do not round an integer to f64 before comparing mixed numbers: an older
    // event at 2^53 could otherwise finish a turn started at 2^53 + 1.
    let int_float = |integer: i128, float: f64| {
        integer.cmp(&(float.trunc() as i128)).then_with(|| {
            0.0_f64
                .partial_cmp(&float.fract())
                .expect("JSON numbers are finite")
        })
    };
    match (integer(&at), integer(started)) {
        (Some(a), Some(b)) => a >= b,
        (Some(a), None) => started.as_f64().is_some_and(|b| int_float(a, b).is_ge()),
        (None, Some(b)) => at.as_f64().is_some_and(|a| int_float(b, a).is_le()),
        (None, None) => at
            .as_f64()
            .zip(started.as_f64())
            .is_some_and(|(a, b)| a >= b),
    }
}

fn base(event: &Value, current: &Map<String, Value>) -> Map<String, Value> {
    let mut out = current.clone();
    out.insert("lastEventId".into(), event["eventId"].clone());
    out.insert("updatedAtMs".into(), at(event));
    out.insert("evidence".into(), event["evidence"].clone());
    for key in ["processKey", "conversationId"] {
        if truthy(&event[key]) {
            out.insert(key.into(), event[key].clone());
        }
    }
    out
}

fn identify(out: &mut Map<String, Value>, event: &Value, started: Value) {
    let turn = &event["turnId"];
    let (id, correlation) = if truthy(turn) {
        (turn.clone(), event["correlation"].clone())
    } else {
        (
            Value::String(format!("local:{}", legacy_id(&event["eventId"]))),
            Value::from("local"),
        )
    };
    out.insert("turnId".into(), id);
    out.insert("correlation".into(), correlation);
    out.insert("startedAtMs".into(), started);
}

/// A changed state, or `None` to keep the current state without cloning it.
pub fn reduce_turn(
    current: Option<&Map<String, Value>>,
    event: &Value,
) -> Option<Map<String, Value>> {
    let (state, category) = match event["kind"].as_str()? {
        "prompt_accepted" | "turn_started" => ("working", 0),
        "permission_requested" => ("awaiting_permission", 1),
        "input_requested" => ("awaiting_input", 1),
        "turn_completed" => ("completed", 2),
        "turn_cancelled" => ("cancelled", 2),
        "turn_failed" => ("failed", 2),
        "pane_closed" => ("closed", 3),
        "session_ended" => ("ended", 3),
        _ => return None,
    };
    if event["evidence"] != "confirmed" {
        return None;
    }
    let empty = Map::new();
    let cur = current.unwrap_or(&empty);
    let process = &event["processKey"];
    let foreign =
        truthy(process) && truthy(get(cur, "processKey")) && process != get(cur, "processKey");
    if category == 3 {
        if foreign {
            return None;
        }
        let mut out = base(event, cur);
        out.insert("state".into(), state.into());
        return Some(out);
    }
    let turn = &event["turnId"];
    let known = get(cur, "turnId");
    if category == 0 {
        if (!foreign && truthy(turn) && turn == known)
            || (!cur.is_empty() && !not_older(event, cur))
        {
            return None;
        }
        let mut out = base(event, if foreign { &empty } else { cur });
        identify(&mut out, event, at(event));
        out.insert("state".into(), state.into());
        out.insert("requestId".into(), Value::Null);
        out.remove("finishedAtMs");
        return Some(out);
    }
    let same_turn = !truthy(known)
        || if truthy(turn) && !legacy_id(known).starts_with("local:") {
            turn == known
        } else {
            not_older(event, cur)
        };
    if foreign
        || !same_turn
        || matches!(
            get(cur, "state").as_str(),
            Some("completed" | "cancelled" | "failed" | "closed" | "ended")
        )
    {
        return None;
    }
    let mut out = base(event, cur);
    if !truthy(known) {
        identify(&mut out, event, Value::Null);
    }
    out.insert("state".into(), state.into());
    if category == 1 {
        out.insert("requestId".into(), event["requestId"].clone());
    } else {
        out.insert("finishedAtMs".into(), at(event));
    }
    Some(out)
}

/// Exact logical pane, or the legacy tmux session/pane pair. Never a project.
pub fn target_of(event: &Value) -> Option<String> {
    if let Some(pane) = event["paneKey"].as_str().filter(|s| !s.is_empty()) {
        return Some(format!("pane:{pane}"));
    }
    if truthy(&event["sessionKey"]) && truthy(&event["paneId"]) {
        return Some(format!(
            "tmux:{}:{}",
            legacy_id(&event["sessionKey"]),
            legacy_id(&event["paneId"])
        ));
    }
    None
}

/// Input order is the persisted sequence order; timestamps must not sort it.
pub fn turns_from_events(events: &[Value]) -> Map<String, Value> {
    let mut turns = Map::new();
    for event in events {
        let Some(target) = target_of(event) else {
            continue;
        };
        let before = turns.get(&target).and_then(Value::as_object);
        if let Some(mut after) = reduce_turn(before, event) {
            after.insert("target".into(), target.clone().into());
            for key in ["sessionKey", "paneKey", "paneId", "projectKey", "harness"] {
                after.insert(key.into(), event[key].clone());
            }
            turns.insert(target, Value::Object(after));
        }
    }
    turns
}
