use serde_json::{Value, json};
pub const MARKS: [&str; 4] = ["none", "resolved", "frozen", "awaiting_reply"];
pub fn data() -> Value {
    serde_json::from_str(include_str!("work_marks_data.json")).unwrap_or(Value::Null)
}
pub fn label(name: &str, english: bool) -> String {
    let pair = match name {
        "none" => ["Sin marca", "No mark"],
        "resolved" => ["Resuelto", "Resolved"],
        "frozen" => ["Congelado", "Frozen"],
        "awaiting_reply" => ["Esperando respuesta", "Awaiting reply"],
        "working" => ["Trabajando", "Working"],
        "favorite" => ["Favorito", "Favorite"],
        _ => [name, name],
    };
    pair.get(usize::from(english))
        .copied()
        .unwrap_or(name)
        .into()
}
pub fn ai_state(name: &str) -> &str {
    if ["work", "need", "done", "error", "idle"].contains(&name) {
        name
    } else {
        "idle"
    }
}
pub fn icon(name: &str, size: f64) -> String {
    let d = data();
    let body = d
        .get("ICONS")
        .and_then(|v| v.get(name))
        .or_else(|| d.get("ICONS").and_then(|v| v.get("none")))
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!(
        "<svg class=\"wm-icon wm-{name}\" width=\"{size}\" height=\"{size}\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.8\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\" focusable=\"false\">{body}</svg>"
    )
}
pub fn ai_icon(name: &str, size: f64, english: bool) -> String {
    let state = ai_state(name);
    let d = data();
    let color = d
        .get("AI_COLORS")
        .and_then(|v| v.get(state))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let label = d
        .get("AI_LABELS")
        .and_then(|v| v.get(state))
        .and_then(|v| v.get(usize::from(english)))
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!(
        "<span class=\"ai-icon ai-dot ai-{state}\" style=\"--ai-c:{color};width:{size}px;height:{size}px\" role=\"img\" aria-label=\"{label}\"></span>"
    )
}
pub fn menu(items: &Value) -> String {
    items.as_array().into_iter().flatten().enumerate().map(|(i,it)| {let fav=it.get("kind").and_then(Value::as_str)==Some("favorite");let name=if fav{"favorite"}else{it.get("value").and_then(Value::as_str).unwrap_or("none")};let checked=it.get("checked").and_then(Value::as_bool).unwrap_or(false);format!("{}<button type=\"button\" role=\"{}\" aria-checked=\"{checked}\" data-i=\"{i}\" tabindex=\"-1\" class=\"wm-item{}\">{}<span>{}</span></button>",if fav{"<div class=\"wm-sep\" role=\"separator\"></div>"}else{""},if fav{"menuitemcheckbox"}else{"menuitemradio"},if checked{" on"}else{""},icon(name,16.),super::escape::text(it.get("label").and_then(Value::as_str).unwrap_or_default()))}).collect()
}
pub fn menu_items(scope: &str, row: &Value, favorite: bool, english: bool) -> Value {
    let mut out=MARKS.iter().map(|m|json!({"kind":"mark","value":m,"label":label(m,english),"checked":row.get("mark").and_then(Value::as_str)==Some(m)})).collect::<Vec<_>>();
    let fav = if scope == "session" {
        favorite
    } else {
        row.get("favorite")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    out.push(
        json!({"kind":"favorite","value":!fav,"label":label("favorite",english),"checked":fav}),
    );
    json!(out)
}
