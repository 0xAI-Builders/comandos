#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]
use comandos_app::layout_dump::{Decision, Diagnostic, normalize};
use serde_json::{Value, json};
fn context() -> Value {
    use comandos_app::term::engine::{Palette, TermEngine};
    let now = std::time::Instant::now();
    let mut engine = TermEngine::new(
        2,
        1,
        100,
        Palette::xterm_default([220; 3], [20; 3], [200; 3], [0; 3], [0; 3]),
        false,
        now,
    );
    engine.feed(b"\x1b[1mA", now);
    json!({"viewport":{"width":1000,"height":800,"dpr":2},"theme":{"name":"bruno","tokens":{"fg":"#eeeeee"},"ansi":["#000000"]},"font":{"family":"Ubuntu Sans","size":11,"size_unit":"pt"},"tabs":[{"key":"term-測試","label":"日本 A","selected":true}],"strip":{"order":["term-測試"],"selected":"term-測試","layout":"rows"},"workspace":{"available":true,"revision":7,"groups":[{"id":"g","tree":{"type":"split","axis":"y","ratio":0.4,"first":{"type":"tab","tabId":"a"},"second":{"type":"tab","tabId":"b"}}}],"focused_tab":"term-測試","active_group":"g"},"widgets":[{"id":"term:term-測試","role":"terminal","mapped":true,"geometry":{"x":100,"y":40,"width":300,"height":400}}],"terminals":[{"key":"term-測試","grid":engine.diagnostic_grid(2).unwrap(),"font":{"family":"Ubuntu Sans Mono","size":13,"size_unit":"pt"},"cell_metrics":{"width":8,"height":19,"dpr":2,"font_size_px":17.333,"origin_x":8,"origin_y":14},"cursor":{"row":0,"col":1,"visible":true}}]})
}
#[test]
fn normalization_preserves_content_labels_order_focus_theme_font_and_geometry() {
    let expected = context();
    let mut normalized = normalize(expected.clone()).unwrap();
    assert_eq!(normalized["schema"], json!(1));
    assert_eq!(
        normalized["readiness"],
        json!({"status":"ready","issues":[]})
    );
    normalized.as_object_mut().unwrap().remove("schema");
    normalized.as_object_mut().unwrap().remove("readiness");
    assert_eq!(normalized, expected);
}
#[test]
fn incomplete_or_invalid_semantic_capture_cannot_be_ready() {
    for key in [
        "viewport",
        "theme",
        "font",
        "tabs",
        "strip",
        "workspace",
        "widgets",
        "terminals",
    ] {
        let mut value = context();
        value.as_object_mut().unwrap().remove(key);
        assert!(normalize(value).is_err(), "missing {key}");
    }
    let mut invalid = context();
    invalid["viewport"]["dpr"] = Value::Null;
    assert!(normalize(invalid).is_err());
    let mut invalid = context();
    invalid["workspace"]["groups"][0]["tree"]["ratio"] = Value::Null;
    assert!(
        normalize(invalid).is_err(),
        "even hidden authority has exact finite ratios"
    );
}
#[test]
fn diagnostic_waits_for_restore_and_quiet_semantics_and_publishes_once() {
    let mut diagnostic = Diagnostic::new(0, 2000);
    let first = context();
    assert_eq!(
        diagnostic.observe(0, &["restore_running".into()], Some(&first)),
        Decision::Pending
    );
    assert_eq!(
        diagnostic.observe(100, &[], Some(&first)),
        Decision::Pending
    );
    let mut changed = first.clone();
    changed["terminals"][0]["grid"]["cells"][0][0]["text"] = json!("B");
    assert_eq!(
        diagnostic.observe(350, &[], Some(&changed)),
        Decision::Pending
    );
    assert_eq!(
        diagnostic.observe(650, &[], Some(&changed)),
        Decision::Ready(changed)
    );
    assert!(diagnostic.done());
    assert_eq!(
        diagnostic.observe(1800, &[], Some(&first)),
        Decision::Pending
    );
}
#[test]
fn diagnostic_deadline_exposes_real_readiness_failure_without_success() {
    let mut diagnostic = Diagnostic::new(100, 500);
    assert_eq!(
        diagnostic.observe(599, &["terminal_not_ready:term-a".into()], None),
        Decision::Pending
    );
    assert_eq!(
        diagnostic.observe(600, &["terminal_not_ready:term-a".into()], None),
        Decision::Failed(vec!["terminal_not_ready:term-a".into()])
    );
    assert!(diagnostic.done());
}

#[test]
fn a_late_timer_cannot_turn_an_expired_diagnostic_into_success() {
    let mut diagnostic = Diagnostic::new(0, 500);
    let value = context();
    assert_eq!(
        diagnostic.observe(100, &[], Some(&value)),
        Decision::Pending
    );
    assert_eq!(
        diagnostic.observe(600, &[], Some(&value)),
        Decision::Failed(vec!["layout_not_quiet".into()])
    );
}

#[test]
fn diagnostic_opt_in_is_explicit_and_native_load_failure_survives_finished() {
    use comandos_app::{layout_dump::enabled, ui::webview::LoadObservation};
    assert!(enabled(Some("1")));
    for value in [None, Some("0"), Some("true"), Some(""), Some(" 1")] {
        assert!(!enabled(value));
    }
    let mut observation = LoadObservation::default();
    assert!(!observation.ready());
    observation.started();
    observation.failed("fixture unavailable".into());
    observation.finished();
    assert!(
        !observation.ready(),
        "WebKit Finished follows a failed load too"
    );
    assert_eq!(observation.error.as_deref(), Some("fixture unavailable"));
    observation.started();
    assert!(!observation.ready());
    observation.finished();
    assert!(observation.ready());
}

#[test]
fn software_rendering_requires_literal_private_opt_in_and_never_changes_live() {
    use comandos_app::{config::RunMode, ui::webview::diagnostic_software_rendering};
    for mode in [RunMode::Sandbox, RunMode::Shadow] {
        assert!(diagnostic_software_rendering(mode, Some("1")));
        for value in [None, Some("0"), Some("true"), Some(""), Some(" 1")] {
            assert!(!diagnostic_software_rendering(mode, value));
        }
    }
    assert!(!diagnostic_software_rendering(RunMode::Live, Some("1")));
}

#[test]
fn missing_content_measurements_and_mapped_allocations_fail_closed() {
    for field in ["font", "cell_metrics", "grid"] {
        let mut value = context();
        value["terminals"][0].as_object_mut().unwrap().remove(field);
        assert!(normalize(value).is_err(), "missing {field}");
    }
    let mut value = context();
    value["terminals"][0]["grid"]["cells"][0]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(normalize(value).is_err(), "missing trailing blank cell");
    let mut value = context();
    value["terminals"][0]["grid"]["cells"][0][0]
        .as_object_mut()
        .unwrap()
        .remove("hyperlink");
    assert!(normalize(value).is_err(), "missing cell content attribute");
    let mut value = context();
    value["widgets"][0]["geometry"] = Value::Null;
    assert!(
        normalize(value).is_err(),
        "mapped widget without allocation"
    );
    let mut value = context();
    value["terminals"][0]["cell_metrics"]["height"] = json!(0);
    assert!(normalize(value).is_err(), "unmeasured terminal");
}

#[test]
fn terminal_grid_retains_unicode_blank_spacers_attributes_and_osc_colors() {
    use comandos_app::term::engine::{Palette, TermEngine};
    let now = std::time::Instant::now();
    let palette = Palette::xterm_default(
        [220, 220, 220],
        [20, 20, 20],
        [200, 200, 200],
        [0, 0, 0],
        [0, 0, 0],
    );
    let red = palette.ansi[1];
    let mut engine = TermEngine::new(8, 2, 100, palette, false, now);
    engine.feed("\x1b[1;31mA\x1b[0m e\u{301}界".as_bytes(), now);
    let grid = engine.diagnostic_grid(16).unwrap();
    assert_eq!(grid["cells"][0][0]["text"], json!("A"));
    assert_eq!(grid["cells"][0][0]["bold"], json!(true));
    assert_eq!(grid["cells"][0][0]["fg"], json!(red));
    assert_eq!(grid["cells"][0][2]["text"], json!("e\u{301}"));
    assert_eq!(grid["cells"][0][3]["text"], json!("界"));
    assert_eq!(grid["cells"][0][3]["width"], json!(2));
    assert_eq!(grid["cells"][0][4]["text"], json!(""));
    assert_eq!(grid["cells"][0][4]["width"], json!(0));
    assert_eq!(grid["cells"][1].as_array().unwrap().len(), 8);
    assert!(
        grid["cells"][1]
            .as_array()
            .unwrap()
            .iter()
            .all(|cell| cell["text"] == " ")
    );
    assert!(
        engine.diagnostic_grid(15).is_err(),
        "a too-large capture fails instead of truncating content"
    );
    engine.feed(b"\x1b]4;1;rgb:11/22/33\x07\r\x1b[31mZ", now);
    let grid = engine.diagnostic_grid(16).unwrap();
    assert_eq!(grid["cells"][0][0]["fg"], json!([17, 34, 51]));
}

#[test]
fn terminal_grid_retains_dim_truecolor_inverse_hidden_and_hyperlinks() {
    use comandos_app::term::engine::{Palette, TermEngine};
    let now = std::time::Instant::now();
    let palette = Palette::xterm_default(
        [210, 210, 210],
        [12, 12, 12],
        [200, 200, 200],
        [0, 0, 0],
        [0, 0, 0],
    );
    let green_dim = comandos_app::term::paint::vte_dim(palette.ansi[2]);
    let mut engine = TermEngine::new(8, 1, 100, palette, false, now);
    engine.feed(b"\x1b[2;32mA\x1b[38;2;30;60;90mB\x1b[0;7mC\x1b[0;8mD\x1b[0m\x1b]8;;https://example.invalid/private\x07E\x1b]8;;\x07", now);
    let grid = engine.diagnostic_grid(8).unwrap();
    assert_eq!(grid["cells"][0][0]["fg"], json!(green_dim));
    assert_eq!(grid["cells"][0][1]["fg"], json!([30, 60, 90]));
    assert_eq!(grid["cells"][0][2]["inverse"], json!(true));
    assert_eq!(grid["cells"][0][3]["hidden"], json!(true));
    assert_eq!(grid["cells"][0][3]["text"], json!("D"));
    assert_eq!(
        grid["cells"][0][4]["hyperlink"],
        json!("https://example.invalid/private")
    );
}

#[test]
fn guarded_atomic_dump_and_stale_removal_are_confined_in_every_mode() {
    use comandos_app::{config::parse_args, guard::WriteGuard, layout_dump::publish};
    use std::os::unix::fs::PermissionsExt;
    for mode in ["sandbox", "shadow", "live"] {
        let root = std::env::temp_dir().join(format!(
            "comandos-layout-dump-{}-{mode}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let home = root.join("home");
        let run = root.join("run");
        let tmp = root.join("tmp");
        for path in [&home, &run, &tmp] {
            std::fs::create_dir(path).unwrap();
        }
        std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
        let env = |key: &str| match key {
            "HOME" => Some(home.display().to_string()),
            "XDG_RUNTIME_DIR" => Some(run.display().to_string()),
            "TMPDIR" => Some(tmp.display().to_string()),
            _ => None,
        };
        let cfg = parse_args(
            &[
                "--mode".into(),
                mode.into(),
                "--tmux-socket".into(),
                "layout-private".into(),
            ],
            false,
            &env,
        )
        .unwrap();
        let guard = WriteGuard::from_config(&cfg, ":private");
        let path = cfg.layout_dump_path();
        let value = normalize(context()).unwrap();
        publish(&guard, &path, &value).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap(),
            value
        );
        assert!(guard.remove_if_exists(&path).unwrap());
        assert!(!guard.remove_if_exists(&path).unwrap());
        let outside = root.join("outside");
        std::fs::create_dir(&outside).unwrap();
        assert!(publish(&guard, &outside.join("layout.json"), &value).is_err());
        std::fs::rename(&run, root.join("run-before-symlink")).unwrap();
        std::os::unix::fs::symlink(&outside, &run).unwrap();
        assert!(publish(&guard, &path, &value).is_err());
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        std::fs::remove_dir_all(root).unwrap();
    }
}
