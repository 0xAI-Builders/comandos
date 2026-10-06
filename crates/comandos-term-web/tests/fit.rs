//! `metrics::fit` frente a `FitAddon.fit()` + `CanvasRenderer._updateDimensions`
//! ejecutados del bundle vendorizado (`fixtures/xterm-fit.json`, generado con
//! `fixtures/gen-xterm-fit.js`): mismas columnas y filas, incluida la
//! histéresis por depender de la rejilla actual. Si difieren, cambia el
//! layout de tmux.
#![cfg(not(target_arch = "wasm32"))]

use comandos_term_web::metrics::{fit, from_measure};
use serde_json::Value;

fn num(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

#[test]
fn fit_matches_the_xterm_bundle_case_by_case() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/xterm-fit.json"
    ))
    .unwrap_or_default();
    let doc: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let scrollbar = num(&doc, "scrollbar");
    let cases = doc
        .get("cases")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(cases.len() > 2000, "fixture vacía");
    let mut checked = 0;
    for case in &cases {
        let font_name = case.get("font").and_then(Value::as_str).unwrap_or("");
        let font = doc
            .get("fonts")
            .and_then(|f| f.get(font_name))
            .cloned()
            .unwrap_or(Value::Null);
        let m = from_measure(
            num(&font, "w"),
            num(&font, "h"),
            num(case, "dpr"),
            num(&font, "lineHeight"),
            num(&font, "letterSpacing"),
        );
        let mut current = (80_u16, 24_u16);
        let steps = case
            .get("steps")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (i, step) in steps.iter().enumerate() {
            let want = (
                step.get(0).and_then(Value::as_u64).unwrap_or(0) as u16,
                step.get(1).and_then(Value::as_u64).unwrap_or(0) as u16,
            );
            let got =
                fit(num(case, "w"), num(case, "h"), scrollbar, &m, current).unwrap_or(current);
            assert_eq!(got, want, "caso {case} paso {i}");
            current = got;
            checked += 1;
        }
    }
    assert_eq!(checked, cases.len() * 3);
}

#[test]
fn named_phone_and_desktop_widths() {
    // Valores del bundle (ver la fixture): 14 px de `term.html`, 11 px de
    // las preferencias; barra de 10 px; desde 80×24.
    let m14 = |dpr| from_measure(8.4, 17.0, dpr, 1.2, 0.0);
    let m11 = |dpr| from_measure(6.6, 13.0, dpr, 1.2, 0.0);
    let start = (80, 24);
    assert_eq!(fit(412.0, 700.0, 10.0, &m14(2.625), start), Some((48, 34)));
    assert_eq!(fit(360.0, 700.0, 10.0, &m14(3.0), start), Some((41, 34)));
    assert_eq!(fit(390.0, 700.0, 10.0, &m11(3.0), start), Some((59, 45)));
    assert_eq!(fit(390.0, 700.0, 10.0, &m14(3.0), start), Some((45, 34)));
    assert_eq!(fit(1400.0, 700.0, 10.0, &m14(1.0), start), Some((173, 35)));
    assert_eq!(fit(844.0, 700.0, 10.0, &m14(2.0), start), Some((104, 35)));
    assert_eq!(fit(320.0, 700.0, 10.0, &m14(2.0), start), Some((38, 35)));
    // 377 px a dpr 3 oscila entre 44 y 43 en xterm.js; igual aquí.
    let first = fit(377.0, 700.0, 10.0, &m14(3.0), start);
    assert_eq!(first, Some((44, 34)));
    assert_eq!(fit(377.0, 700.0, 10.0, &m14(3.0), (44, 34)), Some((43, 34)));
}
