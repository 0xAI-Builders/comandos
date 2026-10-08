use comandos_web_dom::audio::{CATALOG, render_cue};

#[test]
fn every_catalog_cue_renders_short_and_bounded() {
    assert!(!CATALOG.is_empty(), "catalogo de sonidos vacio");
    for (cue, _) in CATALOG {
        let s = render_cue(cue, 48_000.0);
        assert!(
            !s.is_empty() && s.len() < 48_000 * 2,
            "{cue}: sin bucles ni colas largas"
        );
        assert!(s.iter().all(|v| v.abs() <= 1.0), "{cue}: sin saturar");
    }
}

#[test]
fn loop_cues_are_refused() {
    for cue in [
        "loading",
        "processing",
        "recording",
        "connecting",
        "scanning",
        "streaming",
    ] {
        assert!(render_cue(cue, 48_000.0).is_empty());
    }
}
