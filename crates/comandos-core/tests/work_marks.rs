use comandos_core::work_marks as wm;
use serde_json::{Value, json};

fn observations(group: &str) -> Vec<Value> {
    let fixture: Value = serde_json::from_str(include_str!("work_marks_fixture.json")).unwrap();
    fixture[group].as_array().unwrap().clone()
}

#[test]
fn human_icons_match_reference_frames_and_metadata() {
    for case in observations("human") {
        let name = case["name"].as_str().unwrap();
        let actual = json!({
            "svg": wm::icon_svg(name, case["color"].as_str(), case["size"].as_u64().unwrap() as u32, case["phase"].as_f64()),
            "labelEs": wm::label(name, false), "labelEn": wm::label(name, true),
            "color": wm::icon_color(name), "cycle": wm::cycle(name), "count": wm::frame_count(name), "sticker": wm::sticker(name),
        });
        let mut expected = case["expected"].clone();
        expected["cycle"] = json!(expected["cycle"].as_f64().unwrap());
        assert_eq!(actual, expected, "{case}");
    }
}

#[test]
fn human_and_ai_clocks_match_python_remainder_and_rounding() {
    for case in observations("clock") {
        let name = case["name"].as_str().unwrap();
        let seconds = case["seconds"].as_f64().unwrap();
        assert_eq!(
            json!(wm::frame_index(name, seconds).unwrap()),
            case["human"],
            "{case}"
        );
        assert_eq!(
            json!(wm::ai_frame_index(name, seconds).unwrap()),
            case["ai"],
            "{case}"
        );
    }
    for seconds in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(wm::frame_index("working", seconds).is_err());
        assert!(wm::ai_frame_index("need", seconds).is_err());
        assert_eq!(wm::frame_index("none", seconds), Ok(0));
        assert_eq!(wm::ai_frame_index("idle", seconds), Ok(0));
    }
}

#[test]
fn ai_dot_markup_status_and_labels_match_reference() {
    for case in observations("ai") {
        let name = case["name"].as_str().unwrap();
        let actual = json!({
            "state": wm::ai_state(name), "status": wm::ai_status(name),
            "color": wm::ai_color(name), "cycle": wm::ai_cycle(name),
            "labelEs": wm::ai_label(name, false), "labelEn": wm::ai_label(name, true),
            "svg": wm::ai_dot_svg(name, case["size"].as_u64().unwrap() as u32, case["phase"].as_f64()),
            "html": wm::ai_icon_html(name, case["htmlSize"].as_u64().map(|v| v as u32)),
        });
        let mut expected = case["expected"].clone();
        expected["cycle"] = json!(expected["cycle"].as_f64().unwrap());
        assert_eq!(actual, expected, "{case}");
    }
}

#[test]
fn pane_key_is_present_only_for_one_truthy_exact_match() {
    for case in observations("pane") {
        assert_eq!(
            json!(wm::pane_key_for(
                case["panes"].as_array().map(Vec::as_slice),
                &case["session"],
                &case["paneId"]
            )),
            case["expected"],
            "{case}"
        );
    }
}
