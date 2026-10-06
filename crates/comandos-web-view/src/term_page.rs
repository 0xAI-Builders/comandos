//! Literal document shell of the existing terminal; scripts belong to the wasm boot.
use maud::{Markup, PreEscaped, html};
#[derive(Debug)]
pub struct TermParams<'a> {
    pub lang: &'a str,
}
impl Default for TermParams<'_> {
    fn default() -> Self {
        Self { lang: "es" }
    }
}
pub fn shell(params: &TermParams<'_>) -> Markup {
    html! { (maud::DOCTYPE) html lang=(params.lang) {
        head { (PreEscaped(include_str!("term_page_head.html"))) }
        body { (PreEscaped(include_str!("term_page_body.html"))) }
    } }
}
