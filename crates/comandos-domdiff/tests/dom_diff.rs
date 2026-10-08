use comandos_domdiff::{first_difference, normalize};

#[test]
fn attribute_order_and_whitespace_do_not_matter() {
    let a = r#"<div class="row" id="r1">  <b>Hola</b>
      </div>"#;
    let b = r#"<div id="r1" class="row"><b>Hola</b></div>"#;
    assert_eq!(normalize(a), normalize(b));
}

#[test]
fn class_order_matters_and_is_reported() {
    let a = r#"<span class="a b">x</span>"#;
    let b = r#"<span class="b a">x</span>"#;
    let d = first_difference(a, b).unwrap();
    assert!(d.contains("class"), "{d}");
}

#[test]
fn text_difference_is_located() {
    let d = first_difference("<p>Pomodoro 25</p>", "<p>Pomodoro 15</p>").unwrap();
    assert!(d.contains("25") && d.contains("15"), "{d}");
}

#[test]
fn identical_markup_has_no_difference() {
    assert_eq!(
        first_difference("<p>a <i>b</i></p>", "<p>a  <i>b</i></p>"),
        None
    );
}

#[test]
fn missing_trailing_node_is_reported_as_end() {
    let d = first_difference("<p>a</p><p>b</p>", "<p>a</p>").unwrap();
    assert!(d.contains("<fin>"), "{d}");
}
