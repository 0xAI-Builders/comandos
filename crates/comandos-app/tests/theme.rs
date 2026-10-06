#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::theme::{
    button_style_css, desktop_theme, header_css, theme_css, themes_from_file,
};

#[test]
fn config_themes_produce_desktop_tokens_for_all_nine_names() {
    let raw = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/themes.json"
    ));
    let themes = themes_from_file(Some(raw));
    for name in [
        "noche",
        "dia",
        "calido",
        "termius",
        "bruno",
        "superglass",
        "neon",
        "contraste",
        "ubuntu",
    ] {
        let tokens = desktop_theme(name, &themes).expect(name);
        assert_eq!(tokens.ansi.len(), 16);
        assert!(tokens.ansi.iter().all(|color| color.starts_with('#')));
        assert!(tokens.values.contains_key("name"));
        assert!(theme_css(&tokens).contains("notebook > header"));
        assert!(header_css(&tokens).contains(".tabstrip"));
    }
}

#[test]
fn five_button_styles_and_fallback_emit_header_button_css() {
    let raw = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/themes.json"
    ));
    let themes = themes_from_file(Some(raw));
    let tokens = desktop_theme("noche", &themes).unwrap();
    let sutil = button_style_css("sutil", &tokens);
    assert_eq!(button_style_css("nope", &tokens), sutil);
    for style in ["sutil", "arcade", "tecla", "pixel", "consola"] {
        let css = button_style_css(style, &tokens);
        assert!(css.contains("button.cc-key"));
        assert!(css.contains("scrollbar button"));
    }
}
