mod support;

use base64::Engine as _;
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};
use xtask::png_diff::{Rect, Rgba, encode};
use xtask::shots::{
    check_base, compare_styles, device_rect, page_url, parse_pair_suite, parse_remote_suite,
    run_pair,
};

#[test]
fn pair_suite_defaults_follow_the_plan() {
    let s = parse_pair_suite(r##"[{"id":"top","page":"/","selectors":["#topbar"]}]"##).unwrap();
    let e = &s[0];
    assert_eq!(e.widths, [1400, 844, 390, 320]);
    assert_eq!(e.dpr, [1, 2]);
    assert_eq!(e.touch, [false]);
    assert_eq!(
        (e.channel, e.ratio, e.height, e.full_page),
        (24, 0.001, 900, true)
    );
    let s = parse_pair_suite(
        r##"[{"id":"x","page":"/p","selectors":["a"],"touch":[false,true],"mask":["#clock"],"widths":[320]}]"##,
    )
    .unwrap();
    assert_eq!(
        (s[0].touch.clone(), s[0].mask.clone()),
        (vec![false, true], vec!["#clock".to_string()])
    );
}

#[test]
fn bad_pair_suites_are_rejected_with_the_entry_id() {
    assert!(parse_pair_suite("[]").is_err());
    let e = parse_pair_suite(r#"[{"id":"sin-sel","page":"/"}]"#).unwrap_err();
    assert!(e.contains("sin-sel") && e.contains("selectors"), "{e}");
    assert!(parse_pair_suite(r#"[{"id":"r","page":"/","selectors":["a"],"ratio":2}]"#).is_err());
}

#[test]
fn remote_suite_needs_components() {
    let s = parse_remote_suite(r##"{"components":["#topbar"],"remote_only":["#term-toolbar"]}"##)
        .unwrap();
    assert_eq!(
        (s.page.as_str(), s.remote_query.as_str()),
        ("/", "__devwebterm=1")
    );
    assert!(parse_remote_suite(r#"{"components":[]}"#).is_err());
}

#[test]
fn live_dashboard_ports_are_refused() {
    for p in 4777..=4782 {
        assert!(check_base(&format!("http://127.0.0.1:{p}")).is_err(), "{p}");
    }
    assert!(check_base("http://127.0.0.1:7344/").is_ok());
    assert!(check_base("file:///etc").is_err());
}

#[test]
fn page_url_appends_params_after_existing_query() {
    assert_eq!(
        page_url("http://h:1/", "/a", "web=off"),
        "http://h:1/a?web=off"
    );
    assert_eq!(
        page_url("http://h:1", "a?x=1", "web=off"),
        "http://h:1/a?x=1&web=off"
    );
    assert_eq!(page_url("http://h:1", "/", ""), "http://h:1/");
}

#[test]
fn css_rects_scale_by_dpr_and_clip_to_the_image() {
    assert_eq!(
        device_rect([1.5, 2.0, 3.0, 4.0], 2.0, 100, 100),
        Some(Rect {
            x: 3,
            y: 4,
            w: 6,
            h: 8
        })
    );
    assert_eq!(
        device_rect([90.0, 0.0, 50.0, 10.0], 1.0, 100, 100),
        Some(Rect {
            x: 90,
            y: 0,
            w: 10,
            h: 10
        })
    );
    assert_eq!(device_rect([0.0, 0.0, 0.0, 10.0], 1.0, 100, 100), None);
    assert_eq!(device_rect([200.0, 0.0, 5.0, 5.0], 1.0, 100, 100), None);
}

fn styles(color: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("color".into(), json!(color));
    m.insert("display".into(), json!("flex"));
    m
}

#[test]
fn remote_and_desktop_styles_must_match_except_layout_and_remote_only() {
    let (a, b) = (styles("rgb(1, 2, 3)"), styles("rgb(1, 2, 3)"));
    assert!(compare_styles("#t", Some(&a), Some(&b), false).is_empty());
    let mut c = styles("rgb(9, 9, 9)");
    c.insert("display".into(), json!("block")); // disposición: no cuenta
    let p = compare_styles("#t", Some(&a), Some(&c), false);
    assert_eq!(p.len(), 1);
    assert!(
        p[0].contains("color") && p[0].contains("rgb(9, 9, 9)"),
        "{p:?}"
    );
    assert_eq!(compare_styles("#t", Some(&a), None, false).len(), 1);
    assert!(compare_styles("#term-toolbar", None, Some(&a), true).is_empty());
    assert_eq!(compare_styles("#term-toolbar", None, None, true).len(), 1);
}

fn png_b64(rgba: [u8; 4]) -> String {
    let img = Rgba {
        width: 4,
        height: 4,
        pixels: rgba.repeat(16),
    };
    base64::engine::general_purpose::STANDARD.encode(encode(&img).unwrap())
}

/// Broker falso con una página de 4×4 px: el elemento `#b` ocupa 2×2 en (1,1).
/// `web_color` pinta la variante `web=shadow`.
fn pair_broker(
    calls: Arc<Mutex<Vec<Value>>>,
    web_color: [u8; 4],
    web_html: &'static str,
) -> xtask::mcp::Client {
    let mut last_url = String::new();
    support::fake_broker(calls, move |name, args| match name {
        "new_page" => support::text("## Pages\n1: about:blank\n3: about:blank [selected]"),
        "navigate_page" => {
            last_url = args["url"].as_str().unwrap().to_string();
            support::text("ok")
        }
        "take_screenshot" => {
            let c = if last_url.contains("web=shadow") {
                web_color
            } else {
                [0, 0, 0, 255]
            };
            json!({"content": [{"type": "image", "mimeType": "image/png", "data": png_b64(c)}]})
        }
        "evaluate_script" => {
            let f = args["function"].as_str().unwrap();
            if f.contains("readyState") {
                support::eval_reply(&json!(true))
            } else {
                let html = if last_url.contains("web=shadow") {
                    web_html
                } else {
                    "<b id=\"b\">x</b>"
                };
                support::eval_reply(
                    &json!({"scale": 1, "sel": [[[[1, 1, 2, 2], html]]], "mask": [[]]}),
                )
            }
        }
        _ => support::text("ok"),
    })
}

fn out_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("xtask-shots-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn pair_with_identical_variants_passes_and_writes_the_report() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut client = pair_broker(calls.clone(), [0, 0, 0, 255], "<b id=\"b\">x</b>");
    let suite = parse_pair_suite(
        r##"[{"id":"smoke","page":"/","selectors":["#b"],"widths":[390],"dpr":[2],"touch":true}]"##,
    )
    .unwrap();
    let out = out_dir("ok");
    assert_eq!(
        run_pair(&mut client, "http://127.0.0.1:7344", "smoke", &suite, &out).unwrap(),
        0
    );
    let report: Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["failures"], 0);
    let row = &report["results"][0];
    assert_eq!(
        (
            row["pass"].clone(),
            row["total"].clone(),
            row["touch_mode"].clone()
        ),
        (json!(true), json!(4), json!("emulado"))
    );
    assert!(
        std::path::Path::new(row["dir"].as_str().unwrap())
            .join("diff.png")
            .is_file()
    );
    let calls = calls.lock().unwrap();
    let urls: Vec<&str> = calls
        .iter()
        .filter(|c| c["name"] == "navigate_page")
        .map(|c| c["arguments"]["url"].as_str().unwrap())
        .collect();
    assert!(
        urls[0].starts_with("http://127.0.0.1:7344/?web=off&__clock="),
        "{urls:?}"
    );
    assert!(
        urls[1].starts_with("http://127.0.0.1:7344/?web=shadow&__clock="),
        "{urls:?}"
    );
    // El mismo reloj fijo para ambas variantes.
    assert_eq!(
        urls[0].split("__clock=").nth(1),
        urls[1].split("__clock=").nth(1)
    );
    assert_eq!(calls.last().unwrap()["name"], "close_page");
    // Se mide (rectángulos y máscaras) antes de capturar, en cada variante.
    let seq: Vec<String> = calls
        .iter()
        .map(|c| match c["name"].as_str().unwrap() {
            "evaluate_script"
                if c["arguments"]["function"]
                    .as_str()
                    .unwrap()
                    .contains("readyState") =>
            {
                "ready".to_string()
            }
            "evaluate_script" => "measure".to_string(),
            other => other.to_string(),
        })
        .collect();
    assert_eq!(
        seq,
        [
            "new_page",
            "emulate",
            "navigate_page",
            "ready",
            "measure",
            "take_screenshot",
            "navigate_page",
            "ready",
            "measure",
            "take_screenshot",
            "close_page"
        ]
    );
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn pair_with_a_different_crop_fails_and_locates_the_dom_difference() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut client = pair_broker(calls, [255, 255, 255, 255], "<b id=\"b\">y</b>");
    let suite = parse_pair_suite(
        r##"[{"id":"smoke","page":"/","selectors":["#b"],"widths":[390],"dpr":[1]}]"##,
    )
    .unwrap();
    let out = out_dir("fail");
    assert_eq!(
        run_pair(&mut client, "http://127.0.0.1:7344", "smoke", &suite, &out).unwrap(),
        1
    );
    let report: Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("report.json")).unwrap()).unwrap();
    let row = &report["results"][0];
    assert_eq!(
        (row["pass"].clone(), row["differing"].clone()),
        (json!(false), json!(4))
    );
    let d = row["dom_difference"].as_str().unwrap();
    assert!(d.contains("\\\"x\\\"") && d.contains("\\\"y\\\""), "{d}");
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn an_error_mid_run_still_writes_the_partial_report() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut shots = 0;
    let mut client = support::fake_broker(calls, move |name, args| match name {
        "new_page" => support::text("## Pages\n3: about:blank [selected]"),
        "take_screenshot" => {
            shots += 1;
            if shots > 2 {
                json!({"content": [{"type": "text", "text": "pantalla rota"}], "isError": true})
            } else {
                json!({"content": [{"type": "image", "mimeType": "image/png", "data": png_b64([0, 0, 0, 255])}]})
            }
        }
        "evaluate_script" if args["function"].as_str().unwrap().contains("readyState") => {
            support::eval_reply(&json!(true))
        }
        "evaluate_script" => support::eval_reply(
            &json!({"scale": 1, "sel": [[[[1, 1, 2, 2], "<b>x</b>"]]], "mask": [[]]}),
        ),
        _ => support::text("ok"),
    });
    let suite = parse_pair_suite(
        r##"[{"id":"smoke","page":"/","selectors":["#b"],"widths":[390, 320],"dpr":[1]}]"##,
    )
    .unwrap();
    let out = out_dir("partial");
    let err = run_pair(&mut client, "http://127.0.0.1:7344", "smoke", &suite, &out).unwrap_err();
    assert!(err.contains("pantalla rota"), "{err}");
    let report: Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["results"].as_array().unwrap().len(), 1);
    assert_eq!(report["results"][0]["width"], 390);
    assert!(report["error"].as_str().unwrap().contains("pantalla rota"));
    let _ = std::fs::remove_dir_all(&out);
}
