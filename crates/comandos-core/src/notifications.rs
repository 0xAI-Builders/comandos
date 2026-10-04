//! Pure notification policy. Times and JSON values come from the caller.
use crate::legacy_truthy as truthy;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
pub const LOCAL_SPEAKER: &str = "local-speaker";
pub const PRESENCE_STALE_MS: i64 = 90_000;
pub const PUSH_AFTER_MS: i64 = 120_000;
pub const ATTENDED_MS: i64 = 120_000;
pub const HISTORY_WINDOW: i64 = 500;
pub type LiveCheck<'a> = Option<&'a dyn Fn(&Value, &Value) -> bool>;

pub fn default_prefs() -> Value {
    json!({"modes":{"attention":"sound","error":"sound","focus":"sound","done":"visual","news":"visual","usage":"visual","info":"visual"},"volume":0.6,"muted":false,"floatMs":6000,"burstMs":10000})
}
pub fn merge_prefs(current: &Value, update: &Value) -> Result<Value, String> {
    let mut prefs = if truthy(current) {
        current.clone()
    } else {
        default_prefs()
    };
    let update = update.as_object().ok_or("Preferencias inválidas")?;
    if let Some(modes) = update.get("modes").filter(|m| !m.is_null()) {
        for (category, mode) in modes.as_object().ok_or("Modos inválidos")? {
            if prefs["modes"].get(category).is_none()
                || !matches!(mode.as_str(), Some("visual" | "sound"))
            {
                return Err("Modo de aviso inválido".into());
            }
            prefs["modes"][category] = mode.clone();
        }
    }
    if let Some(volume) = update.get("volume") {
        let n = volume
            .as_f64()
            .filter(|v| (0.0..=1.0).contains(v))
            .ok_or("Volumen inválido")?;
        prefs["volume"] = json!(n);
    }
    if let Some(muted) = update.get("muted") {
        prefs["muted"] = json!(muted.as_bool().ok_or("Silencio inválido")?);
    }
    Ok(prefs)
}
pub fn classify(event: &Value) -> Value {
    let kind = event["kind"].as_str().unwrap_or("");
    let category = match kind {
        "permission_requested" | "input_requested" => "attention",
        "turn_failed" => "error",
        "turn_completed" | "turn_cancelled" => "done",
        "focus_completed" => "focus",
        "news_edition" | "announcement" => "news",
        "usage_alert" => "usage",
        _ => "info",
    };
    let cue = match category {
        "attention" if kind == "permission_requested" => "permission",
        "error" => "error",
        "focus" | "news" => "complete",
        "done" => "success",
        "usage" => "warning",
        _ => "attention",
    };
    json!({"category":category,"needsHuman":matches!(kind,"permission_requested"|"input_requested"),"cue":cue,"notice":!matches!(kind,"prompt_accepted"|"turn_started")})
}
fn integer(value: &Value) -> i64 {
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|n| n as i64))
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}
fn or<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    if truthy(a) { a } else { b }
}
fn connected(c: &Value) -> bool {
    c.get("connected") != Some(&Value::Bool(false))
}
fn visible(c: &Value, now: i64) -> bool {
    truthy(&c["visible"])
        && connected(c)
        && i128::from(now) - i128::from(integer(&c["lastSeenAt"])) <= i128::from(PRESENCE_STALE_MS)
}
pub fn sound_device(clients: &[Value], now_ms: i64) -> Value {
    let mut visible: Vec<_> = clients.iter().filter(|c| visible(c, now_ms)).collect();
    if visible.is_empty() {
        return json!(LOCAL_SPEAKER);
    }
    visible.sort_by_key(|c| std::cmp::Reverse(integer(&c["lastInteractionAt"])));
    visible
        .into_iter()
        .find(|c| truthy(&c["canPlayAudio"]))
        .map(|c| c["deviceId"].clone())
        .unwrap_or(Value::Null)
}
fn last_visible_ms(clients: &[Value]) -> i128 {
    clients
        .iter()
        .map(|c| {
            let seen = if truthy(&c["visible"]) && connected(c) {
                integer(&c["lastSeenAt"])
            } else {
                integer(or(&c["hiddenSince"], &c["lastSeenAt"]))
            };
            i128::from(seen)
                .min(i128::from(integer(&c["lastInteractionAt"])) + i128::from(ATTENDED_MS))
        })
        .max()
        .unwrap_or(0)
        .max(0)
}
pub fn route_event(
    event: &Value,
    clients: &[Value],
    prefs: &Value,
    now_ms: i64,
    focus_active: bool,
) -> Value {
    let mut route = classify(event);
    let default = default_prefs();
    let prefs = if truthy(prefs) { prefs } else { &default };
    let category = route["category"].as_str().unwrap();
    let sound = route["notice"] == true
        && prefs["modes"][category] == "sound"
        && !truthy(&prefs["muted"])
        && (!focus_active || event["kind"] == "permission_requested");
    let last = last_visible_ms(clients);
    let at = if truthy(&event["occurredAtMs"]) {
        integer(&event["occurredAtMs"])
    } else if truthy(&event["receivedAtMs"]) {
        integer(&event["receivedAtMs"])
    } else {
        now_ms
    };
    let push = matches!(category, "attention" | "error" | "focus" | "done")
        && i128::from(now_ms) - last >= i128::from(PUSH_AFTER_MS)
        && i128::from(at) >= last;
    route["float"] = json!({"show":route["notice"]==true&&category!="info","ms":prefs.get("floatMs").cloned().unwrap_or(json!(6000))});
    route["sound"] = json!(sound);
    route["soundDevice"] = if sound {
        sound_device(clients, now_ms)
    } else {
        Value::Null
    };
    route["push"] = json!(push);
    route
}
pub fn group_keys(events: &[Value], prefs: &Value) -> BTreeMap<String, String> {
    let default = default_prefs();
    let prefs = if truthy(prefs) { prefs } else { &default };
    let burst = prefs.get("burstMs").map(integer).unwrap_or(10000);
    let mut events: Vec<_> = events.iter().collect();
    events.sort_by_key(|e| (integer(&e["occurredAtMs"]), integer(&e["sequence"])));
    let mut last: BTreeMap<String, (i64, String)> = BTreeMap::new();
    let mut groups = BTreeMap::new();
    for e in events {
        let key = json!([e["projectKey"], e["kind"]]).to_string();
        let at = integer(&e["occurredAtMs"]);
        let id = e["eventId"].as_str().expect("eventId protocol string");
        let group = last
            .get(&key)
            .filter(|(time, _)| i128::from(at) - i128::from(*time) <= i128::from(burst))
            .map(|(_, g)| g.clone())
            .unwrap_or_else(|| id.into());
        groups.insert(id.into(), group.clone());
        last.insert(key, (at, group));
    }
    groups
}
fn pane_of(event: &Value) -> Option<String> {
    if truthy(&event["paneKey"]) {
        Some(event["paneKey"].to_string())
    } else if truthy(&event["paneId"]) {
        Some(json!([event["sessionKey"], event["paneId"]]).to_string())
    } else if !event["conversationId"].is_null() {
        Some(event["conversationId"].to_string())
    } else {
        None
    }
}
pub fn pending_requests(events: &[Value]) -> Vec<String> {
    let mut events: Vec<_> = events.iter().collect();
    events.sort_by_key(|e| (integer(&e["sequence"]), integer(&e["occurredAtMs"])));
    let mut pending = BTreeMap::new();
    for e in events {
        if let Some(pane) = pane_of(e) {
            if classify(e)["needsHuman"] == true {
                pending.insert(
                    pane,
                    e["eventId"]
                        .as_str()
                        .expect("eventId protocol string")
                        .to_string(),
                );
            } else {
                pending.remove(&pane);
            }
        }
    }
    let mut ids: Vec<_> = pending.into_values().collect();
    ids.sort();
    ids
}
pub fn live_pending(history: &[Value], is_live: LiveCheck<'_>) -> Vec<String> {
    let pending = pending_requests(history);
    let Some(check) = is_live else { return pending };
    let by: BTreeMap<_, _> = history
        .iter()
        .map(|e| (e["eventId"].as_str().unwrap(), e))
        .collect();
    pending
        .into_iter()
        .filter(|id| {
            by.get(id.as_str())
                .is_some_and(|e| check(&e["sessionKey"], &e["paneId"]))
        })
        .collect()
}
pub fn notice(
    event: &Value,
    clients: &[Value],
    prefs: &Value,
    now_ms: i64,
    read: &BTreeSet<String>,
    group: &str,
    focus_active: bool,
) -> Value {
    let route = route_event(event, clients, prefs, now_ms, focus_active);
    let project = if route["category"] == "news" {
        Value::Null
    } else {
        event["projectKey"].clone()
    };
    let edition = event["sourceEventId"]
        .as_str()
        .and_then(|s| s.strip_prefix("news-edition:"));
    json!({"eventId":event["eventId"],"sequence":event["sequence"],"kind":event["kind"],"category":route["category"],"needsHuman":route["needsHuman"],"projectKey":project,"project":project,"sessionKey":event["sessionKey"],"paneKey":event["paneKey"],"paneId":event["paneId"],"title":or(&event["title"],&json!("")),"excerpt":or(&event["excerpt"],&json!("")),"occurredAtMs":event["occurredAtMs"],"read":read.contains(event["eventId"].as_str().unwrap()),"float":route["float"],"group":group,"editionId":edition})
}
/// Unread filter used by a push transport; performs no delivery.
pub fn push_policy(
    event: &Value,
    clients: &[Value],
    prefs: &Value,
    now_ms: i64,
    focus_active: bool,
    read: &BTreeSet<String>,
) -> bool {
    !event["eventId"]
        .as_str()
        .is_some_and(|id| read.contains(id))
        && route_event(event, clients, prefs, now_ms, focus_active)["push"] == true
}
