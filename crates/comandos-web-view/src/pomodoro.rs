use serde_json::Value;
pub const STYLE_ORDER: [&str; 6] = [
    "alchemy", "arcade", "shikashi", "soul", "garden", "crystals",
];
pub fn catalog() -> Value {
    serde_json::from_str(include_str!("pomodoro_data.json")).unwrap_or(Value::Null)
}
pub fn style(id: &str) -> &str {
    if STYLE_ORDER.contains(&id) {
        id
    } else {
        "alchemy"
    }
}
pub fn asset(name: &str, id: &str, extra: &str) -> String {
    let set = style(id);
    let d = catalog();
    let assets = d
        .get("STYLES")
        .and_then(|v| v.get(set))
        .and_then(|v| v.get("assets"));
    let a = assets
        .and_then(|v| v.get(name))
        .or_else(|| assets.and_then(|v| v.get("clock")))
        .unwrap_or(&Value::Null);
    let n = |k: &str, default: f64| a.get(k).and_then(Value::as_f64).unwrap_or(default);
    let file = a.get("file").and_then(Value::as_str).unwrap_or_default();
    let native = n("native", 32.);
    let variable = a.get("width").is_some();
    let frames = n("frames", 0.);
    let motion = a.get("motion").and_then(Value::as_str).unwrap_or_default();
    let mut vars = vec![
        format!("background-image:url('/assets/pomodoro/{file}')"),
        format!("background-position:-{}px -{}px", n("x", 0.), n("y", 0.)),
        format!("--native:{native}"),
        format!("--pixel-multiplier:{}", 32. / native),
    ];
    if variable {
        vars.push(format!("--frame-width:{}", n("width", 0.)));
        vars.push(format!("--frame-height:{}", n("height", 0.)));
        vars.push(format!("--strip-duration:{:.3}s", frames / n("fps", 1.)));
    }
    if frames != 0. {
        vars.push(format!("--frames:{frames}"));
        vars.push(format!("--strip-end:-{}px", frames * n("width", native)));
    }
    let mut classes = vec!["pm-asset".to_string(), format!("pm-asset-{name}")];
    if frames != 0. {
        classes.push("pm-framed".into())
    }
    if variable {
        classes.push("pm-variable".into())
    }
    if !motion.is_empty() {
        classes.push(format!("pm-motion-{motion}"))
    }
    if !extra.is_empty() {
        classes.push(extra.into())
    }
    format!(
        "<span class=\"{}\" aria-hidden=\"true\" data-art=\"{set}\" data-asset=\"{name}\"><span class=\"pm-pixel\" style=\"{}\"></span></span>",
        classes.join(" "),
        vars.join(";")
    )
}
pub fn files() -> Value {
    let d = catalog();
    let mut files = std::collections::BTreeSet::new();
    for style in d
        .get("STYLES")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|v| v.values())
    {
        for a in style
            .get("assets")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|v| v.values())
        {
            if let Some(f) = a.get("file").and_then(Value::as_str) {
                files.insert(f.to_string());
            }
        }
    }
    serde_json::json!(files)
}
