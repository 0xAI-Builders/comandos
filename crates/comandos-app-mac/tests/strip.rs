#![allow(clippy::unwrap_used)]
use comandos_app_mac::strip::{MenuAction, menu_spec, zoom};
use comandos_desktop::Lang;
#[test]
fn native_menu_has_exact_shortcuts_and_languages() {
    let es = menu_spec(Lang::Es);
    let en = menu_spec(Lang::En);
    assert_eq!(es.iter().flat_map(|g| &g.items).count(), 10);
    assert_eq!(en[0].items[0].title, "Quit ComandOS");
    assert_eq!(es[0].items[0].title, "Salir");
    assert_eq!(es[1].title, "Vista");
    let items: Vec<_> = es.iter().flat_map(|g| &g.items).collect();
    let toggle = items
        .iter()
        .find(|i| i.action == MenuAction::Toggle)
        .unwrap();
    assert_eq!(toggle.key, "\u{f70f}");
    assert!(!toggle.command && !toggle.shift);
    let next = items.iter().find(|i| i.action == MenuAction::Next).unwrap();
    assert_eq!(next.key, "]");
    assert!(next.command && next.shift);
    let close = items
        .iter()
        .find(|i| i.action == MenuAction::Close)
        .unwrap();
    assert_eq!(close.key, "w");
    assert!(close.command && !close.shift);
}
#[test]
fn zoom_matches_original_clamp_and_zero_fallback() {
    assert_eq!(zoom(0.0, 1.1), 1.1);
    assert_eq!(zoom(0.5, 1.0 / 1.1), 0.5);
    assert_eq!(zoom(3.0, 1.1), 3.0);
    assert_eq!(zoom(1.0, 1.1), 1.1);
}

#[test]
fn zoom_reset_returns_one_even_when_current_is_limited() {
    assert_eq!(comandos_app_mac::strip::zoom(2.5, 0.), 1.);
}
