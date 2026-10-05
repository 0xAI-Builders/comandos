use comandos_web_view::term_page::{TermParams, shell};
#[test]
fn shell_matches_legacy_without_scripts_and_xterm_stylesheet() {
    let original = include_str!("../../../dash/term.html");
    let doc = scraper::Html::parse_document(original);
    // Compare document children separately: normalize parses fragments and drops html/head wrappers.
    let expected = doc
        .select(&scraper::Selector::parse("head, body").unwrap())
        .map(|node| {
            let mut html = node.inner_html();
            for script in
                node.select(&scraper::Selector::parse("script, link[href*=xterm]").unwrap())
            {
                html = html.replace(&script.html(), "");
            }
            html
        })
        .collect::<Vec<_>>()
        .join("");
    let rendered = shell(&TermParams::default()).into_string();
    let actual_doc = scraper::Html::parse_document(&rendered);
    let actual = actual_doc
        .select(&scraper::Selector::parse("head, body").unwrap())
        .map(|node| node.inner_html())
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(comandos_domdiff::first_difference(&actual, &expected), None);
}

#[test]
fn shell_is_a_complete_document_with_escaped_params_and_literal_css() {
    let rendered = shell(&TermParams {
        lang: "es\" onload=\"bad",
    })
    .into_string();
    assert!(rendered.starts_with("<!DOCTYPE html>"));
    let doc = scraper::Html::parse_document(&rendered);
    let root = doc
        .select(&scraper::Selector::parse("html").unwrap())
        .next()
        .unwrap();
    assert_eq!(root.value().attr("lang"), Some("es\" onload=\"bad"));
    assert_eq!(root.value().attr("onload"), None);
    let css = rendered
        .split("<style>")
        .nth(1)
        .unwrap()
        .split("</style>")
        .next()
        .unwrap();
    let original = include_str!("../../../dash/term.html")
        .split("<style>")
        .nth(1)
        .unwrap()
        .split("</style>")
        .next()
        .unwrap();
    assert_eq!(css, original);
    assert!(
        doc.select(&scraper::Selector::parse("script, link[href*=xterm]").unwrap())
            .next()
            .is_none()
    );
}
