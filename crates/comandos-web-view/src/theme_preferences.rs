//! Appearance metadata and exact gallery markup shared by native/WASM callers.
use serde_json::Value;

pub const THEMES: &[&str] = &[
    "noche",
    "dia",
    "calido",
    "termius",
    "bruno",
    "superglass",
    "neon",
    "contraste",
    "ubuntu",
];
pub const BUTTONS: &[(&str, &str, &str)] = &[
    (
        "sutil",
        "Elevación sutil",
        "Neutral, con un canto que se hunde",
    ),
    ("arcade", "Arcade", "Domo redondo de maquinita"),
    ("tecla", "Tecla mecánica", "Cara clara y faldón grueso"),
    ("pixel", "Pixel 3D", "Esquinas escalonadas, sombra dura"),
    (
        "consola",
        "Consola",
        "Un color por función, como un control",
    ),
];
pub fn metadata() -> Value {
    serde_json::from_str(include_str!("theme_meta.json")).unwrap_or(Value::Null)
}
pub fn theme(name: &str) -> &str {
    if THEMES.contains(&name) {
        name
    } else {
        "noche"
    }
}
pub fn button_style(name: &str) -> &str {
    if BUTTONS.iter().any(|(id, _, _)| *id == name) {
        name
    } else {
        "sutil"
    }
}
pub fn theme_gallery(current: &str, icon: impl Fn(&str, u32) -> String) -> String {
    let metadata = metadata();
    THEMES.iter().enumerate().map(|(index, id)| {
        let m = metadata.get(*id).unwrap_or(&Value::Null);
        let text = |key| m[key].as_str().unwrap_or_default();
        let swatches = m["sw"].as_array().into_iter().flatten().map(|c| format!("<i style=\"background:{}\"></i>", c.as_str().unwrap_or_default())).collect::<String>();
        format!("<button type=\"button\" class=\"theme-card\" role=\"radio\" aria-checked=\"{}\" data-theme-id=\"{id}\" data-index=\"{index}\">\n    <span class=\"theme-ico\">{}</span><span><b>{}</b><small>{}</small></span>\n    <span class=\"theme-swatches\">{swatches}</span><span class=\"theme-check\">{}</span></button>", *id==current, icon(text("icon"),18), crate::escape::md_esc(text("label")), crate::escape::md_esc(text("desc")), icon("check",15))
    }).collect()
}
pub fn button_gallery(current: &str, icon: impl Fn(&str, u32) -> String) -> String {
    BUTTONS.iter().map(|(id,label,desc)| format!("<button type=\"button\" class=\"btn-style-card\" role=\"radio\" aria-checked=\"{}\" data-btn-style-id=\"{id}\">\n    <span class=\"btn-style-demo\" data-btn-style=\"{id}\"><span class=\"hdr-btn demo-key\">{}</span><span class=\"hdr-btn demo-key on\">{}</span></span>\n    <span><b>{}</b><small>{}</small></span></button>", *id==current, icon("bell",17), icon("smartphone",17), crate::escape::md_esc(label), crate::escape::md_esc(desc))).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn all_gallery_states_match_frozen_original_bytes() {
        let expected: Value =
            serde_json::from_str(include_str!("../tests/fixtures/theme-gallery.json")).unwrap();
        let icon =
            |name: &str, size| format!("<svg data-icon=\"{name}\" data-size=\"{size}\"></svg>");
        for name in THEMES {
            assert_eq!(
                theme_gallery(name, icon),
                expected
                    .get("themes")
                    .unwrap()
                    .get(*name)
                    .unwrap()
                    .as_str()
                    .unwrap()
            );
        }
        for (name, _, _) in BUTTONS {
            assert_eq!(
                button_gallery(name, icon),
                expected
                    .get("buttons")
                    .unwrap()
                    .get(*name)
                    .unwrap()
                    .as_str()
                    .unwrap()
            );
        }
        assert_eq!(theme("unknown"), "noche");
        assert_eq!(button_style("unknown"), "sutil");
    }
}
