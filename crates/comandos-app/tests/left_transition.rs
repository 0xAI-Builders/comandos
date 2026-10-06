#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[path = "support/t18_oracle.rs"]
mod oracle;
#[test]
fn original_actual_draw_transition_geometry_matches_native() {
    use comandos_app::ui::side::left_frame;
    for hiding in [false, true] {
        for progress in [0.0, 0.125, 0.5, 0.875, 1.0] {
            let native = left_frame(hiding, progress, 12.0, 620.0);
            let expected = oracle::original(
                serde_json::json!({"op":"left-frame","hiding":hiding,"progress":progress,"x":12,"width":620}),
            );
            for key in ["offset", "mask_start", "alpha", "edge", "shadow_alpha"] {
                assert_eq!(
                    native.to_json()[key].as_f64(),
                    expected[key].as_f64(),
                    "{key}"
                );
            }
        }
    }
}
#[test]
fn original_actual_frame_clock_tween_starts_at_first_frame() {
    for millis in [120, 140, 150, 160, 220] {
        let times = [1_000_000, 1_016_000, 1_080_000, 1_140_000, 1_220_000];
        let clock = comandos_app::ui::side::Tween::new(1., 0., millis);
        let values = times
            .into_iter()
            .map(|t| clock.step(t).0)
            .collect::<Vec<_>>();
        let expected =
            oracle::original(serde_json::json!({"op":"tween","times":times,"millis":millis}));
        assert_eq!(serde_json::json!(values), expected);
    }
}
