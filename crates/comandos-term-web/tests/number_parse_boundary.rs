//! Node-only differential proof of browser numeric conversion and custom glyphs.
#![cfg(target_arch = "wasm32")]
#[allow(dead_code)]
#[path = "../src/number_text.rs"]
mod number_text;
use comandos_term::glyphs::{self, DrawOp};
use serde_json::{Value, json};
use wasm_bindgen_test::wasm_bindgen_test;
fn compare(source: &str) {
    let actual = number_text::parse(source);
    let expected = source.parse::<f64>().ok();
    match (actual, expected) {
        (Some(a), Some(e)) if a.is_nan() && e.is_nan() => {}
        (Some(a), Some(e)) => assert_eq!(a.to_bits(), e.to_bits(), "{source}"),
        (a, e) => assert_eq!(a, e, "{source}"),
    }
}
#[wasm_bindgen_test]
fn browser_conversion_preserves_rust_grammar_and_ieee_bits() {
    for source in [
        "",
        "+",
        "-",
        ".",
        "+.",
        "-0",
        "+0",
        ".5",
        "1.",
        "+.5",
        "1e-7",
        "1e+21",
        "5e-324",
        "2.2250738585072014e-308",
        "1.7976931348623157e308",
        "1e309",
        "-1e10000",
        "1e-10000",
        "-1e-10000",
        "1e",
        "1e+",
        "1e--2",
        "1_000",
        "0x10",
        "0b10",
        "0o10",
        "0XFF",
        " 1",
        "1 ",
        "\t1",
        "1\n",
        "１２",
        "١٢",
        "NaN",
        "nan",
        "NAN",
        "+NaN",
        "-NaN",
        "inf",
        "INF",
        "Inf",
        "Infinity",
        "infinity",
        "INFINITY",
        "-inf",
        "+Infinity",
    ] {
        compare(source);
    }
    let mut bits = 0xfeed_cafe_1234_5678u64;
    for _ in 0..20_000 {
        bits = bits
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let value = f64::from_bits(bits);
        if value.is_finite() {
            compare(&value.to_string());
            compare(&format!("{value:e}"));
        }
    }
    let alphabet = b"0129.eE+- xinfNa";
    for a in alphabet {
        for b in alphabet {
            for c in alphabet {
                compare(std::str::from_utf8(&[*a, *b, *c]).unwrap());
            }
        }
    }
    for n in [20, 21, 30, 100, 309, 1000] {
        compare(&"9".repeat(n));
        compare(&format!("-{}", "9".repeat(n)));
    }
}
#[wasm_bindgen_test]
fn wasm_custom_glyph_operations_match_original_xterm_fixture() {
    let fixtures: serde_json::Map<String, Value> = serde_json::from_str(include_str!(
        "../../comandos-term/tests/fixtures/xterm-custom-glyphs.json"
    ))
    .unwrap();
    assert_eq!(fixtures.len(), 414);
    for (key, expected) in fixtures {
        let (cell, code) = key.split_once(':').unwrap();
        let (size, rest) = cell.split_once('@').unwrap();
        let (w, h) = size.split_once('x').unwrap();
        let (dpr, font) = rest.split_once('/').unwrap();
        let n = |s: &str| s.parse::<f64>().unwrap();
        let m = glyphs::CellMetrics {
            cell_w: n(w),
            cell_h: n(h),
            dpr: n(dpr),
            font_size: n(font),
        };
        let mut ops = Vec::new();
        assert!(glyphs::draw_ops(
            char::from_u32(u32::from_str_radix(code, 16).unwrap()).unwrap(),
            &m,
            &mut ops
        ));
        let actual: Vec<Value> = ops
            .into_iter()
            .map(|op| match op {
                DrawOp::FillRect { x, y, w, h } => json!(["FillRect", x, y, w, h]),
                DrawOp::FillPattern { mask } => json!(["FillPattern", mask]),
                DrawOp::ClipCell => json!(["ClipCell"]),
                DrawOp::BeginPath => json!(["BeginPath"]),
                DrawOp::MoveTo { x, y } => json!(["MoveTo", x, y]),
                DrawOp::LineTo { x, y } => json!(["LineTo", x, y]),
                DrawOp::CurveTo {
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                } => json!(["CurveTo", x1, y1, x2, y2, x, y]),
                DrawOp::Stroke { line_width } => json!(["Stroke", line_width]),
                DrawOp::Fill => json!(["Fill"]),
            })
            .collect();
        let flat = |v: &Value| -> Vec<String> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|op| {
                    op.as_array()
                        .unwrap()
                        .iter()
                        .map(|x| match x.as_f64() {
                            Some(f) => format!("{f:?}"),
                            None => x.to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .collect()
        };
        assert_eq!(flat(&Value::Array(actual)), flat(&expected), "{key}");
    }
}
#[wasm_bindgen_test]
fn css_and_pane_number_edges_keep_existing_contract() {
    for text in [
        "rgb(1.5,2.5,3.5)",
        "rgba(-100,1e2,999,0.1)",
        "rgb(1e999,2,3)",
        "rgb(NaN,2,3)",
        "rgb(0x10,2,3)",
        "rgb(.5,2.,+3)",
    ] {
        let inner = text
            .trim()
            .strip_prefix("rgba(")
            .or_else(|| text.trim().strip_prefix("rgb("))
            .unwrap()
            .strip_suffix(')')
            .unwrap();
        let expected: Option<Vec<u8>> = inner
            .split(',')
            .take(3)
            .map(|p| {
                p.trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .map(|n| n.round().clamp(0., 255.) as u8)
            })
            .collect();
        assert_eq!(
            comandos_term_web::theme::parse_color(text),
            expected.map(|v| [v[0], v[1], v[2]]),
            "{text}"
        );
    }
    for source in [
        "0",
        "-0",
        "1e-7",
        "9007199254740993",
        "1e308",
        "1e309",
        "1e-999",
        "-1e-999",
    ] {
        let number: Value = serde_json::from_str(source).unwrap();
        let expected = number.as_f64().unwrap_or(0.0);
        assert_eq!(
            comandos_term_web::pane_chrome::number(&json!({"width":number}), "width").to_bits(),
            expected.to_bits(),
            "{source}"
        );
    }
    assert_eq!(
        comandos_term_web::pane_chrome::number(&json!({"width":"12"}), "width"),
        0.
    );
}
