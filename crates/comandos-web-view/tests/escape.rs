use comandos_web_view::escape::{attr_esc, md_esc, text};

#[test]
fn escapes_like_the_dashboard() {
    assert_eq!(
        text(r#"<a href="x">'&'</a>"#),
        "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
    );
    assert_eq!(md_esc("<b>&\"'</b>"), "&lt;b&gt;&amp;\"'&lt;/b&gt;");
}

#[test]
fn attr_esc_is_md_esc_plus_double_quote() {
    // `attrEsc(s)` de index.html: `mdEsc(String(s||"")).replace(/"/g,"&quot;")`.
    assert_eq!(attr_esc(r#"a"b'<&>"#), "a&quot;b'&lt;&amp;&gt;");
    assert_eq!(attr_esc(""), "");
}

#[test]
fn escapes_leave_everything_else_byte_for_byte() {
    let s = "ñ 🍅 \u{0}\t\n/\\`=";
    assert_eq!(text(s), s);
    assert_eq!(md_esc(s), s);
    assert_eq!(
        text("&amp;"),
        "&amp;amp;",
        "no hay doble escape inteligente"
    );
}

#[test]
fn maud_escapes_user_text() {
    let name = "<img src=x onerror=alert(1)>";
    let html = maud::html! { span.row-name { (name) } }.into_string();
    assert!(!html.contains("<img"), "{html}");
    assert_eq!(
        html,
        r#"<span class="row-name">&lt;img src=x onerror=alert(1)&gt;</span>"#
    );
}

#[test]
fn reexported_maud_is_the_same_macro() {
    let html = comandos_web_view::maud::html! { b { ("a\"b") } }.into_string();
    assert_eq!(html, "<b>a&quot;b</b>");
}
